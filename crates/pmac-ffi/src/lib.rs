#![deny(unsafe_op_in_unsafe_fn)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]

mod business;
mod dart;
mod gather;
mod geterrors;
mod helpers;
mod pool;
mod registry;
mod types;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;
use std::time::Duration;

use pmac_core::{Error, ErrorCode, Operation};
use pmac_protocol::regex_match;
use zeroize::Zeroizing;

use helpers::*;
use registry::{client_kind, registry, request_kind, runtime_kind, Entry};
use types::*;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

thread_local! {
    static LAST_ERROR: std::cell::RefCell<Option<(PmacErrorInfo, String)>> = const { std::cell::RefCell::new(None) };
}

pub(crate) fn save_last_error(error: &Error, request_id: u64, connection_epoch: u64) {
    LAST_ERROR.with(|slot| {
        *slot.borrow_mut() = Some((
            PmacErrorInfo {
                struct_size: size_of::<PmacErrorInfo>() as u32,
                abi_version: ABI_VERSION,
                status: error.code as i32,
                certainty: error.certainty as u32,
                request_id,
                connection_epoch,
            },
            error.message.clone(),
        ))
    });
}

pub(crate) fn ffi_status(operation: impl FnOnce() -> Result<(), Error>) -> i32 {
    LAST_ERROR.with(|slot| *slot.borrow_mut() = None);
    match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(Ok(())) => ErrorCode::Ok as i32,
        Ok(Err(error)) => {
            LAST_ERROR.with(|slot| {
                if slot.borrow().is_none() {
                    save_last_error(&error, 0, 0);
                }
            });
            error.code as i32
        }
        Err(_) => {
            let error = Error::internal("native operation panicked").sent();
            save_last_error(&error, 0, 0);
            ErrorCode::Internal as i32
        }
    }
}

/// Copy the calling thread's last FFI error without clearing it.
#[no_mangle]
pub extern "C" fn pmac_copy_last_error(
    out_info: *mut PmacErrorInfo,
    out: *mut u8,
    capacity: usize,
    required: *mut usize,
) -> i32 {
    match catch_unwind(AssertUnwindSafe(|| {
        if out_info.is_null() {
            return Err(Error::invalid("error info output is null"));
        }
        LAST_ERROR.with(|slot| {
            let slot = slot.borrow();
            let (info, message) = slot
                .as_ref()
                .ok_or_else(|| Error::new(ErrorCode::InvalidHandle, "no last error"))?;
            unsafe { out_info.write(*info) };
            copy_bytes(message.as_bytes(), out, capacity, required)
        })
    })) {
        Ok(Ok(())) => ErrorCode::Ok as i32,
        Ok(Err(error)) => error.code as i32,
        Err(_) => ErrorCode::Internal as i32,
    }
}
#[no_mangle]
pub extern "C" fn pmac_runtime_create(config: *const PmacRuntimeConfig, out_id: *mut u64) -> i32 {
    ffi_status(|| {
        unsafe { write_id(out_id, 0)? };
        let workers = if config.is_null() {
            2
        } else {
            let config = unsafe { read_input(config)? };
            validate_header(
                config.struct_size,
                config.abi_version,
                size_of::<PmacRuntimeConfig>(),
            )?;
            config.worker_threads as usize
        };
        let runtime = pmac_core::Runtime::new(workers)?;
        let id = registry().insert(runtime_kind(), Entry::Runtime(runtime))?;
        unsafe { write_id(out_id, id) }
    })
}

#[no_mangle]
pub extern "C" fn pmac_runtime_shutdown(id: u64, wait_ms: u32) -> i32 {
    ffi_status(|| {
        let runtime = registry().runtime(id).ok_or_else(invalid_handle)?;
        runtime.shutdown_clients(wait_ms).map(|_| ())
    })
}

#[no_mangle]
pub extern "C" fn pmac_runtime_release(id: u64) -> i32 {
    ffi_status(|| {
        let runtime = registry().runtime(id).ok_or_else(invalid_handle)?;
        if !runtime.is_shutdown()
            || !runtime.all_clients_closed()
            || registry().has_children(&runtime)
            || gather::has_children(&runtime)
            || geterrors::has_children(&runtime)
        {
            return Err(Error::new(
                ErrorCode::ShutdownPending,
                "runtime still has clients or stopping tasks",
            ));
        }
        registry()
            .remove(id, runtime_kind())
            .map(|_| ())
            .ok_or_else(invalid_handle)
    })
}

struct CleanupAttempt {
    runtime: Arc<pmac_core::Runtime>,
    completed: bool,
}
impl CleanupAttempt {
    fn complete(&mut self) {
        self.completed = true;
    }
}
impl Drop for CleanupAttempt {
    fn drop(&mut self) {
        if !self.completed {
            self.runtime.allow_cleanup_retry();
        }
    }
}

fn cleanup_workers() -> &'static std::sync::Mutex<Vec<std::thread::JoinHandle<()>>> {
    static WORKERS: std::sync::OnceLock<std::sync::Mutex<Vec<std::thread::JoinHandle<()>>>> =
        std::sync::OnceLock::new();
    WORKERS.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

fn reap_finished_cleanup_workers() {
    let mut running = Vec::new();
    let mut workers = cleanup_workers()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for worker in workers.drain(..) {
        if worker.is_finished() {
            let _ = worker.join();
        } else {
            running.push(worker);
        }
    }
    *workers = running;
}

#[no_mangle]
pub extern "C" fn pmac_runtime_cleanup(id: u64) -> i32 {
    ffi_status(|| {
        reap_finished_cleanup_workers();
        let runtime = registry().runtime(id).ok_or_else(invalid_handle)?;
        if !runtime.schedule_cleanup() {
            return Ok(());
        }
        let worker_runtime = runtime.clone();
        let worker = std::thread::Builder::new()
            .name("pmac-cleanup".into())
            .spawn(move || {
                let mut attempt = CleanupAttempt {
                    runtime: worker_runtime,
                    completed: false,
                };
                loop {
                    match attempt.runtime.shutdown_clients(1_000) {
                        Ok(()) => {}
                        Err(error) if error.code == ErrorCode::ShutdownPending => continue,
                        Err(_) => return,
                    }
                    match pmac_runtime_release(id) {
                        status
                            if status == ErrorCode::Ok as i32
                                || status == ErrorCode::InvalidHandle as i32 =>
                        {
                            attempt.complete();
                            break;
                        }
                        status if status == ErrorCode::ShutdownPending as i32 => {
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        _ => return,
                    }
                }
            });
        match worker {
            Ok(worker) => {
                cleanup_workers()
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(worker);
                Ok(())
            }
            Err(error) => {
                runtime.allow_cleanup_retry();
                Err(Error::internal(format!(
                    "failed to start runtime cleanup: {error}"
                )))
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn pmac_runtime_cleanup_wait() -> i32 {
    ffi_status(|| loop {
        let workers = cleanup_workers()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .drain(..)
            .collect::<Vec<_>>();
        if workers.is_empty() {
            return Ok(());
        }
        let mut failed = false;
        for worker in workers {
            failed |= worker.join().is_err();
        }
        if failed {
            return Err(Error::internal("runtime cleanup worker panicked"));
        }
    })
}

#[no_mangle]
pub extern "C" fn pmac_get_runtime_info(out: *mut PmacRuntimeInfo) -> i32 {
    ffi_status(|| {
        if out.is_null() {
            return Err(Error::invalid("runtime info output is null"));
        }
        unsafe {
            out.write(PmacRuntimeInfo {
                struct_size: size_of::<PmacRuntimeInfo>() as u32,
                abi_major: ABI_MAJOR,
                abi_minor: ABI_MINOR,
                version_number: 20_000,
                feature_flags: 0x0000_01ff,
                pointer_width: usize::BITS,
            })
        };
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn pmac_version_string() -> *const std::ffi::c_char {
    c"2.0.0".as_ptr()
}
#[no_mangle]
pub extern "C" fn pmac_version_number() -> u32 {
    20_000
}
#[no_mangle]
pub extern "C" fn pmac_abi_version() -> u32 {
    ABI_VERSION
}

#[no_mangle]
pub extern "C" fn pmac_client_create(
    runtime_id: u64,
    config: *const PmacClientConfig,
    out_id: *mut u64,
) -> i32 {
    ffi_status(|| {
        unsafe { write_id(out_id, 0)? };
        let runtime = registry().runtime(runtime_id).ok_or_else(invalid_handle)?;
        let config = unsafe { client_config(config)? };
        if config.max_requests == 0 || config.max_file_tasks == 0 || config.max_retained_bytes == 0
        {
            return Err(Error::invalid("client resource limits must be non-zero"));
        }
        let (id, _) = create_client(runtime, config)?;
        unsafe { write_id(out_id, id) }
    })
}

#[no_mangle]
pub extern "C" fn pmac_client_close(id: u64, close_mode: u32, wait_ms: u32) -> i32 {
    ffi_status(|| {
        if close_mode != 0 {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "only cancel-and-close mode is currently defined",
            ));
        }
        let (client, runtime) = client(id)?;
        if client.snapshot().state == pmac_core::ClientState::Closed {
            return Ok(());
        }
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        runtime.spawn(async move {
            let _ = sender.send(client.close(wait_ms).await);
        });
        receiver
            .recv_timeout(Duration::from_millis(u64::from(wait_ms)))
            .map_err(|_| Error::new(ErrorCode::ShutdownPending, "client close wait expired"))??;
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn pmac_client_release(id: u64) -> i32 {
    ffi_status(|| {
        let (client, _) = client(id)?;
        if client.snapshot().state != pmac_core::ClientState::Closed
            || client.has_unreleased_gpascii()
        {
            return Err(Error::new(
                ErrorCode::ShutdownPending,
                "client must complete close and release all GPASCII resources before release",
            ));
        }
        registry()
            .remove(id, client_kind())
            .map(|_| ())
            .ok_or_else(invalid_handle)
    })
}

#[no_mangle]
pub extern "C" fn pmac_client_recovery_ids(
    out: *mut u64,
    capacity: usize,
    required: *mut usize,
) -> i32 {
    ffi_status(|| {
        if required.is_null() {
            return Err(Error::invalid("recovery ID count output is null"));
        }
        let ids = registry().recovery_clients();
        unsafe { required.write(ids.len()) };
        if capacity < ids.len() || out.is_null() && !ids.is_empty() {
            return Err(Error::new(
                ErrorCode::BufferTooSmall,
                "recovery ID output buffer is too small",
            ));
        }
        if capacity > isize::MAX as usize / size_of::<u64>() {
            return Err(Error::invalid("recovery ID capacity overflow"));
        }
        for (index, id) in ids.into_iter().enumerate() {
            unsafe { out.add(index).write(id) };
        }
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn pmac_client_get_state(id: u64, out: *mut PmacStateSnapshot) -> i32 {
    ffi_status(|| {
        if out.is_null() {
            return Err(Error::invalid("state output is null"));
        }
        let (client, _) = client(id)?;
        let state = client.snapshot();
        unsafe {
            out.write(PmacStateSnapshot {
                struct_size: size_of::<PmacStateSnapshot>() as u32,
                abi_version: ABI_VERSION,
                state: state.state as u32,
                closing: u32::from(state.closing),
                connection_epoch: state.connection_epoch,
                last_valid_response_unix_ms: state.last_valid_response_unix_ms,
                active_requests: state.active_requests,
            })
        };
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn pmac_connect_submit(
    id: u64,
    options: *const PmacConnectOptions,
    out_id: *mut u64,
) -> i32 {
    ffi_status(|| {
        unsafe { write_id(out_id, 0)? };
        let options = unsafe { parse_connect_options(options)? };
        let request = submit_owned(
            id,
            Operation::Connect(options),
            pmac_core::RequestOptions {
                timeout_ms: 60_000,
                ..Default::default()
            },
        )?;
        unsafe { write_id(out_id, request) }
    })
}

#[no_mangle]
pub extern "C" fn pmac_command_submit(
    id: u64,
    command: *const u8,
    command_len: usize,
    options: *const PmacRequestOptions,
    out_id: *mut u64,
) -> i32 {
    ffi_status(|| {
        unsafe { write_id(out_id, 0)? };
        let command = client(id)?
            .0
            .parse_command_for_current_thread(unsafe { text(command, command_len)? })?;
        let request = submit_request(id, Operation::Command(command), options)?;
        unsafe { write_id(out_id, request) }
    })
}

#[no_mangle]
pub extern "C" fn pmac_get_response_async(
    id: u64,
    command: *const u8,
    command_len: usize,
    options: *const PmacRequestOptions,
    out_id: *mut u64,
) -> i32 {
    pmac_command_submit(id, command, command_len, options, out_id)
}

#[no_mangle]
pub extern "C" fn pmac_shell_submit(
    id: u64,
    command: *const u8,
    command_len: usize,
    persistent: u32,
    options: *const PmacRequestOptions,
    out_id: *mut u64,
) -> i32 {
    ffi_status(|| {
        unsafe { write_id(out_id, 0)? };
        if persistent > 1 {
            return Err(Error::invalid("persistent must be 0 or 1"));
        }
        let command = unsafe { text(command, command_len)? };
        let request = submit_request(
            id,
            Operation::Shell {
                command,
                persistent: persistent == 1,
            },
            options,
        )?;
        unsafe { write_id(out_id, request) }
    })
}

macro_rules! file_two_path_export {
    ($name:ident, $variant:ident) => {
        #[no_mangle]
        pub extern "C" fn $name(
            id: u64,
            first: *const u8,
            first_len: usize,
            second: *const u8,
            second_len: usize,
            options: *const PmacRequestOptions,
            out_id: *mut u64,
        ) -> i32 {
            ffi_status(|| {
                unsafe { write_id(out_id, 0)? };
                let first = unsafe { text(first, first_len)? };
                let second = unsafe { text(second, second_len)? };
                let request = submit_request(
                    id,
                    Operation::$variant {
                        local: first,
                        remote: second,
                    },
                    options,
                )?;
                unsafe { write_id(out_id, request) }
            })
        }
    };
}
file_two_path_export!(pmac_upload_submit, Upload);

#[no_mangle]
pub extern "C" fn pmac_download_submit(
    id: u64,
    remote: *const u8,
    remote_len: usize,
    local: *const u8,
    local_len: usize,
    options: *const PmacRequestOptions,
    out_id: *mut u64,
) -> i32 {
    ffi_status(|| {
        unsafe { write_id(out_id, 0)? };
        let remote = unsafe { text(remote, remote_len)? };
        let local = unsafe { text(local, local_len)? };
        let request = submit_request(id, Operation::Download { remote, local }, options)?;
        unsafe { write_id(out_id, request) }
    })
}

#[no_mangle]
pub extern "C" fn pmac_buffer_upload_submit(
    id: u64,
    local: *const u8,
    local_len: usize,
    remote: *const u8,
    remote_len: usize,
    cleanup_remote: u32,
    options: *const PmacRequestOptions,
    out_id: *mut u64,
) -> i32 {
    ffi_status(|| {
        unsafe { write_id(out_id, 0)? };
        if cleanup_remote > 1 {
            return Err(Error::invalid("cleanup_remote must be 0 or 1"));
        }
        let local = unsafe { text(local, local_len)? };
        let remote = unsafe { text(remote, remote_len)? };
        let request = submit_request(
            id,
            Operation::BufferUpload {
                local,
                remote,
                cleanup_remote: cleanup_remote == 1,
            },
            options,
        )?;
        unsafe { write_id(out_id, request) }
    })
}

#[no_mangle]
pub extern "C" fn pmac_file_exists_submit(
    id: u64,
    remote: *const u8,
    remote_len: usize,
    options: *const PmacRequestOptions,
    out_id: *mut u64,
) -> i32 {
    ffi_status(|| {
        unsafe { write_id(out_id, 0)? };
        let remote = unsafe { text(remote, remote_len)? };
        let request = submit_request(id, Operation::FileExists { remote }, options)?;
        unsafe { write_id(out_id, request) }
    })
}

#[no_mangle]
pub extern "C" fn pmac_file_remove_submit(
    id: u64,
    remote: *const u8,
    remote_len: usize,
    options: *const PmacRequestOptions,
    out_id: *mut u64,
) -> i32 {
    ffi_status(|| {
        unsafe { write_id(out_id, 0)? };
        let remote = unsafe { text(remote, remote_len)? };
        let request = submit_request(id, Operation::FileRemove { remote }, options)?;
        unsafe { write_id(out_id, request) }
    })
}

#[no_mangle]
pub extern "C" fn pmac_request_wait(id: u64, wait_ms: u32, out: *mut PmacRequestInfo) -> i32 {
    ffi_status(|| {
        if out.is_null() {
            return Err(Error::invalid("request info output is null"));
        }
        let (request, _) = request(id)?;
        request.wait(wait_ms);
        let snapshot = request.snapshot();
        let info = snapshot.info;
        let result = snapshot.result;
        let (response_len, stderr_len, exit_code) = result
            .as_ref()
            .and_then(|result| result.as_ref().as_ref().ok())
            .map_or((0, 0, None), |result| {
                (result.response.len(), result.stderr.len(), result.exit_code)
            });
        unsafe {
            out.write(PmacRequestInfo::from_request(
                info,
                response_len,
                stderr_len,
                exit_code,
            ))
        };
        if info.state == pmac_core::RequestState::Completed {
            let mut error = result
                .as_ref()
                .and_then(|result| result.as_ref().as_ref().err())
                .cloned()
                .unwrap_or_else(|| Error::new(info.code, "request completed"));
            error.certainty = info.certainty;
            save_last_error(&error, id, info.connection_epoch);
            Err(error)
        } else {
            Err(Error::new(
                ErrorCode::WaitPending,
                "request is still running",
            ))
        }
    })
}

#[no_mangle]
pub extern "C" fn pmac_request_cancel(id: u64) -> i32 {
    ffi_status(|| {
        let (request, _) = request(id)?;
        Err(Error::new(request.cancel(), "cancel result"))
    })
}

#[no_mangle]
pub extern "C" fn pmac_request_copy_response(
    id: u64,
    out: *mut u8,
    capacity: usize,
    required: *mut usize,
) -> i32 {
    ffi_status(|| copy_response_part(id, out, capacity, required, false))
}

#[no_mangle]
pub extern "C" fn pmac_request_copy_stderr(
    id: u64,
    out: *mut u8,
    capacity: usize,
    required: *mut usize,
) -> i32 {
    ffi_status(|| copy_response_part(id, out, capacity, required, true))
}

fn copy_response_part(
    id: u64,
    out: *mut u8,
    capacity: usize,
    required: *mut usize,
    stderr: bool,
) -> Result<(), Error> {
    let (request, _) = request(id)?;
    let result = request
        .result()
        .ok_or_else(|| Error::new(ErrorCode::WaitPending, "request is not complete"))?;
    let result = match result.as_ref() {
        Ok(result) => result,
        Err(error) => return Err(error.clone()),
    };
    copy_bytes(
        if stderr {
            &result.stderr
        } else {
            &result.response
        },
        out,
        capacity,
        required,
    )
}

#[no_mangle]
pub extern "C" fn pmac_request_copy_error(
    id: u64,
    out_info: *mut PmacErrorInfo,
    out: *mut u8,
    capacity: usize,
    required: *mut usize,
) -> i32 {
    ffi_status(|| {
        if out_info.is_null() {
            return Err(Error::invalid("error info output is null"));
        }
        let (request, _) = request(id)?;
        let snapshot = request.snapshot();
        let info = snapshot.info;
        let error = match snapshot.result.as_deref() {
            Some(Err(error)) => error.clone(),
            Some(Ok(_)) => Error::new(ErrorCode::Ok, ""),
            None => Error::new(ErrorCode::WaitPending, "request is not complete"),
        };
        unsafe {
            out_info.write(PmacErrorInfo {
                struct_size: size_of::<PmacErrorInfo>() as u32,
                abi_version: ABI_VERSION,
                status: error.code as i32,
                certainty: info.certainty as u32,
                request_id: id,
                connection_epoch: info.connection_epoch,
            })
        };
        copy_bytes(error.message.as_bytes(), out, capacity, required)
    })
}

#[no_mangle]
pub extern "C" fn pmac_request_copy_match(
    id: u64,
    pattern: *const u8,
    pattern_len: usize,
    group: u32,
    out: *mut u8,
    capacity: usize,
    required: *mut usize,
) -> i32 {
    ffi_status(|| {
        let pattern = unsafe { text(pattern, pattern_len)? };
        let (request, _) = request(id)?;
        let result = request
            .result()
            .ok_or_else(|| Error::new(ErrorCode::WaitPending, "request is not complete"))?;
        let response = match result.as_ref() {
            Ok(result) => &result.response,
            Err(error) => return Err(error.clone()),
        };
        let found = regex_match(response, &pattern)?.ok_or_else(|| {
            Error::new(ErrorCode::RemoteError, "regular expression did not match")
        })?;
        let selected = if group == 0 {
            Some(found.matched)
        } else {
            found.groups.into_iter().nth(group as usize - 1).flatten()
        };
        let selected = selected.ok_or_else(|| {
            Error::new(
                ErrorCode::RemoteError,
                "regular expression capture group did not match",
            )
        })?;
        copy_bytes(&selected, out, capacity, required)
    })
}

#[no_mangle]
pub extern "C" fn pmac_request_release(id: u64) -> i32 {
    ffi_status(|| {
        registry()
            .remove(id, request_kind())
            .map(|_| ())
            .ok_or_else(invalid_handle)
    })
}

unsafe fn parse_connect_options(
    options: *const PmacConnectOptions,
) -> Result<pmac_core::ConnectOptions, Error> {
    if options.is_null() {
        return Err(Error::invalid("connect options are null"));
    }
    let options = unsafe { read_input(options)? };
    validate_header(
        options.struct_size,
        options.abi_version,
        size_of::<PmacConnectOptions>(),
    )?;
    if options.flags & !(CONNECT_TCP_PROBE | CONNECT_ACCEPT_ANY_HOST_KEY) != 0 {
        return Err(Error::invalid("unknown connect flags"));
    }
    let mut parsed = pmac_core::ConnectOptions::new(
        unsafe { text(options.host.data, options.host.len)? },
        unsafe { text(options.username.data, options.username.len)? },
    );
    parsed.port = if options.port == 0 { 22 } else { options.port };
    parsed.password = optional_text(options.password)?.map(Zeroizing::new);
    parsed.private_key = optional_text(options.private_key)?.map(Into::into);
    parsed.set_public_key_path(optional_text(options.public_key)?.map(Into::into));
    parsed.passphrase = optional_text(options.passphrase)?.map(Zeroizing::new);
    let known_hosts = optional_text(options.known_hosts)?;
    parsed.host_key = if let Some(path) = known_hosts {
        if options.pinned_sha256.len != 0 {
            return Err(Error::invalid(
                "known_hosts and pinned SHA-256 are mutually exclusive",
            ));
        }
        pmac_core::HostKeyPolicy::KnownHosts(path.into())
    } else if options.pinned_sha256.len != 0 {
        let digest: [u8; 32] =
            unsafe { bytes(options.pinned_sha256.data, options.pinned_sha256.len)? }
                .try_into()
                .map_err(|_| Error::invalid("pinned SHA-256 must contain exactly 32 bytes"))?;
        pmac_core::HostKeyPolicy::PinnedSha256(digest)
    } else if options.flags & CONNECT_ACCEPT_ANY_HOST_KEY != 0 {
        pmac_core::HostKeyPolicy::AcceptAnyExplicit
    } else {
        return Err(Error::invalid(
            "configure known_hosts, a pinned SHA-256 key, or explicit accept-any risk mode",
        ));
    };
    parsed.tcp_probe = options.flags & CONNECT_TCP_PROBE != 0;
    parsed.tcp_probe_timeout =
        Duration::from_millis(u64::from(options.tcp_probe_timeout_ms.max(1)));
    parsed.keepalive_interval =
        Duration::from_millis(u64::from(options.keepalive_interval_ms.max(1)));
    parsed.keepalive_max = options.keepalive_max;
    parsed.control_pool_size = if options.control_pool_size == 0 {
        1
    } else {
        options.control_pool_size
    };
    Ok(parsed)
}

fn optional_text(value: PmacBytes) -> Result<Option<String>, Error> {
    if value.len == 0 {
        Ok(None)
    } else {
        unsafe { text(value.data, value.len) }.map(Some)
    }
}

#[no_mangle]
pub extern "C" fn pmac_runtime_wait_completion(id: u64, cursor: *mut u64, wait_ms: u32) -> i32 {
    ffi_status(|| {
        if cursor.is_null() {
            return Err(Error::invalid("completion cursor is null"));
        }
        let runtime = registry().runtime(id).ok_or_else(invalid_handle)?;
        let mut value = unsafe { cursor.read() };
        let advanced = runtime.wait_completion(&mut value, wait_ms);
        unsafe {
            cursor.write(value);
        }
        if advanced {
            Ok(())
        } else {
            Err(Error::new(
                ErrorCode::WaitPending,
                "no completion notification within wait budget",
            ))
        }
    })
}
#[no_mangle]
pub extern "C" fn pmac_runtime_wake(id: u64) -> i32 {
    ffi_status(|| {
        registry().runtime(id).ok_or_else(invalid_handle)?.wake();
        Ok(())
    })
}
