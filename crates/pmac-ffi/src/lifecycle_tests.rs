use std::sync::Arc;

use pmac_core::ClientConfig;
use pmac_testkit::MockTransport;

use super::*;
use crate::pool::{pmac_pool_confirm_recovery, pmac_pool_get_state};
use crate::registry::Entry;

static CLEANUP_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn new_runtime() -> u64 {
    let mut runtime = 0;
    assert_eq!(
        pmac_runtime_create(std::ptr::null(), &mut runtime),
        ErrorCode::Ok as i32
    );
    assert_ne!(runtime, 0);
    runtime
}

fn mock_client(runtime_id: u64) -> u64 {
    let runtime = registry().runtime(runtime_id).unwrap();
    let id = registry().reserve(client_kind()).unwrap();
    let client = runtime
        .create_client_with_id(
            id,
            Arc::new(MockTransport::default()),
            ClientConfig::default(),
        )
        .unwrap();
    registry()
        .put(id, Entry::Client { client, runtime })
        .unwrap();
    id
}

fn bytes(text: &'static str) -> PmacBytes {
    PmacBytes {
        data: text.as_ptr(),
        len: text.len(),
    }
}
fn empty() -> PmacBytes {
    PmacBytes {
        data: std::ptr::null(),
        len: 0,
    }
}
fn connect_options() -> PmacConnectOptions {
    PmacConnectOptions {
        struct_size: size_of::<PmacConnectOptions>() as u32,
        abi_version: ABI_VERSION,
        host: bytes("mock"),
        username: bytes("test"),
        password: bytes("test-password"),
        private_key: empty(),
        public_key: empty(),
        passphrase: empty(),
        known_hosts: empty(),
        pinned_sha256: empty(),
        port: 22,
        control_pool_size: 1,
        reserved0: 0,
        flags: CONNECT_ACCEPT_ANY_HOST_KEY,
        tcp_probe_timeout_ms: 100,
        keepalive_interval_ms: 1_000,
        keepalive_max: 3,
        reserved: [0; 4],
    }
}

#[test]
fn shutdown_permanently_rejects_new_clients_and_zeroes_output_handle() {
    let runtime = new_runtime();
    assert_eq!(pmac_runtime_shutdown(runtime, 5_000), ErrorCode::Ok as i32);
    let mut client = u64::MAX;
    assert_eq!(
        pmac_client_create(runtime, std::ptr::null(), &mut client),
        ErrorCode::Closing as i32
    );
    assert_eq!(client, 0);
    assert_eq!(pmac_runtime_release(runtime), ErrorCode::Ok as i32);
    assert_eq!(
        pmac_runtime_release(runtime),
        ErrorCode::InvalidHandle as i32
    );
}

#[test]
fn runtime_release_requires_shutdown_and_explicit_client_id_release() {
    let runtime = new_runtime();
    let mut client = 0;
    assert_eq!(
        pmac_client_create(runtime, std::ptr::null(), &mut client),
        ErrorCode::Ok as i32
    );
    assert_eq!(
        pmac_runtime_release(runtime),
        ErrorCode::ShutdownPending as i32
    );
    assert_eq!(pmac_runtime_shutdown(runtime, 5_000), ErrorCode::Ok as i32);
    assert_eq!(
        pmac_runtime_release(runtime),
        ErrorCode::ShutdownPending as i32,
        "closed clients still own IDs and must be explicitly released"
    );
    assert_eq!(
        pmac_client_close(client, 0, 1_000),
        ErrorCode::Ok as i32,
        "closing a client after runtime shutdown must remain idempotent"
    );
    assert_eq!(pmac_client_release(client), ErrorCode::Ok as i32);
    assert_eq!(pmac_runtime_release(runtime), ErrorCode::Ok as i32);
}

#[test]
fn cleanup_supervisor_retains_runtime_until_child_handles_are_released() {
    let _lock = CLEANUP_TEST_LOCK.lock().unwrap();
    let runtime = new_runtime();
    let client = mock_client(runtime);
    assert_eq!(pmac_runtime_cleanup(runtime), ErrorCode::Ok as i32);
    assert_eq!(pmac_runtime_cleanup(runtime), ErrorCode::Ok as i32);
    assert!(registry().runtime(runtime).is_some());
    assert_eq!(pmac_client_close(client, 0, 1_000), ErrorCode::Ok as i32);
    assert_eq!(pmac_client_release(client), ErrorCode::Ok as i32);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while registry().runtime(runtime).is_some() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(pmac_runtime_cleanup_wait(), ErrorCode::Ok as i32);
}

#[test]
fn completed_cleanup_handles_are_reaped_during_normal_scheduling() {
    let _lock = CLEANUP_TEST_LOCK.lock().unwrap();
    for _ in 0..24 {
        let runtime = new_runtime();
        assert_eq!(pmac_runtime_cleanup(runtime), ErrorCode::Ok as i32);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while registry().runtime(runtime).is_some() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
    }
    assert!(cleanup_workers().lock().unwrap().len() <= 1);
    assert_eq!(pmac_runtime_cleanup_wait(), ErrorCode::Ok as i32);
    assert!(cleanup_workers().lock().unwrap().is_empty());
}

#[test]
fn completed_request_remains_readable_after_shutdown_but_prevents_runtime_release() {
    let runtime = new_runtime();
    let client = mock_client(runtime);
    let mut request = 0;
    assert_eq!(
        pmac_connect_submit(client, &connect_options(), &mut request),
        ErrorCode::Ok as i32
    );
    let mut info = std::mem::MaybeUninit::<PmacRequestInfo>::uninit();
    assert_eq!(
        pmac_request_wait(request, 5_000, info.as_mut_ptr()),
        ErrorCode::Ok as i32
    );
    assert_eq!(pmac_runtime_shutdown(runtime, 5_000), ErrorCode::Ok as i32);
    assert_eq!(
        pmac_client_close(client, 0, 1_000),
        ErrorCode::Ok as i32,
        "closing a client after runtime shutdown must remain idempotent"
    );
    assert_eq!(pmac_client_release(client), ErrorCode::Ok as i32);
    assert_eq!(
        pmac_runtime_release(runtime),
        ErrorCode::ShutdownPending as i32,
        "completed request IDs retain their result/runtime ownership until release"
    );
    let mut required = 0;
    let copy_status = pmac_request_copy_response(request, std::ptr::null_mut(), 0, &mut required);
    assert!(copy_status == ErrorCode::Ok as i32 || copy_status == ErrorCode::BufferTooSmall as i32);
    assert_eq!(pmac_request_release(request), ErrorCode::Ok as i32);
    assert_eq!(pmac_runtime_release(runtime), ErrorCode::Ok as i32);
}

#[test]
fn shutdown_does_not_invalidate_other_runtimes() {
    let first = new_runtime();
    let second = new_runtime();
    assert_eq!(pmac_runtime_shutdown(first, 5_000), ErrorCode::Ok as i32);
    let mut client = 0;
    assert_eq!(
        pmac_client_create(second, std::ptr::null(), &mut client),
        ErrorCode::Ok as i32
    );
    assert_eq!(pmac_runtime_release(first), ErrorCode::Ok as i32);
    assert_eq!(pmac_runtime_shutdown(second, 5_000), ErrorCode::Ok as i32);
    assert_eq!(
        pmac_client_close(client, 0, 1_000),
        ErrorCode::Ok as i32,
        "closing a client after runtime shutdown must remain idempotent"
    );
    assert_eq!(pmac_client_release(client), ErrorCode::Ok as i32);
    assert_eq!(pmac_runtime_release(second), ErrorCode::Ok as i32);
}

#[test]
fn rejected_client_creation_after_shutdown_does_not_consume_handle_quota() {
    let runtime = new_runtime();
    assert_eq!(pmac_runtime_shutdown(runtime, 5_000), ErrorCode::Ok as i32);
    for _ in 0..1_025 {
        let mut client = u64::MAX;
        assert_eq!(
            pmac_client_create(runtime, std::ptr::null(), &mut client),
            ErrorCode::Closing as i32
        );
        assert_eq!(client, 0);
    }
    assert_eq!(pmac_runtime_release(runtime), ErrorCode::Ok as i32);
    let runtime = new_runtime();
    let mut client = 0;
    assert_eq!(
        pmac_client_create(runtime, std::ptr::null(), &mut client),
        ErrorCode::Ok as i32
    );
    assert_eq!(pmac_runtime_shutdown(runtime, 5_000), ErrorCode::Ok as i32);
    assert_eq!(
        pmac_client_close(client, 0, 1_000),
        ErrorCode::Ok as i32,
        "closing a client after runtime shutdown must remain idempotent"
    );
    assert_eq!(pmac_client_release(client), ErrorCode::Ok as i32);
    assert_eq!(pmac_runtime_release(runtime), ErrorCode::Ok as i32);
}

#[test]
fn invalid_file_task_limit_rolls_back_every_reservation() {
    let runtime_id = new_runtime();
    let runtime = registry().runtime(runtime_id).unwrap();
    for _ in 0..1_025 {
        let error = match create_client(
            runtime.clone(),
            ClientConfig {
                max_file_tasks: usize::MAX,
                ..ClientConfig::default()
            },
        ) {
            Ok(_) => panic!("invalid file task limit unexpectedly succeeded"),
            Err(error) => error,
        };
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }
    let (client, _) = create_client(runtime, ClientConfig::default()).unwrap();
    assert_eq!(
        pmac_runtime_shutdown(runtime_id, 5_000),
        ErrorCode::Ok as i32
    );
    assert_eq!(pmac_client_close(client, 0, 1_000), ErrorCode::Ok as i32);
    assert_eq!(pmac_client_release(client), ErrorCode::Ok as i32);
    assert_eq!(pmac_runtime_release(runtime_id), ErrorCode::Ok as i32);
}

#[test]
fn short_input_structure_is_rejected_before_reading_its_body() {
    let header = [8_u32, ABI_VERSION];
    let mut runtime = 123_u64;
    assert_eq!(
        pmac_runtime_create(header.as_ptr().cast(), &mut runtime),
        ErrorCode::InvalidArgument as i32
    );
    assert_eq!(runtime, 0);
}

#[test]
fn size_only_input_is_rejected_before_reading_abi_version() {
    let size = 4_u32;
    let mut runtime = 123_u64;
    assert_eq!(
        pmac_runtime_create((&size as *const u32).cast(), &mut runtime),
        ErrorCode::InvalidArgument as i32
    );
    assert_eq!(runtime, 0);
}

#[test]
fn error_info_tracks_sent_cancellation_and_pending_certainty() {
    fn error_info(id: u64) -> PmacErrorInfo {
        let mut info = std::mem::MaybeUninit::<PmacErrorInfo>::uninit();
        let mut message = [0_u8; 256];
        let mut required = 0;
        assert_eq!(
            pmac_request_copy_error(
                id,
                info.as_mut_ptr(),
                message.as_mut_ptr(),
                message.len(),
                &mut required,
            ),
            ErrorCode::Ok as i32
        );
        assert!(required <= message.len());
        unsafe { info.assume_init() }
    }

    let runtime_id = new_runtime();
    let runtime = registry().runtime(runtime_id).unwrap();
    let mock = Arc::new(MockTransport::default());
    let client_id = registry().reserve(client_kind()).unwrap();
    let client = runtime
        .create_client_with_id(client_id, mock.clone(), ClientConfig::default())
        .unwrap();
    registry()
        .put(client_id, Entry::Client { client, runtime })
        .unwrap();
    let mut connect = 0;
    assert_eq!(
        pmac_connect_submit(client_id, &connect_options(), &mut connect),
        0
    );
    let mut info = std::mem::MaybeUninit::<PmacRequestInfo>::uninit();
    assert_eq!(pmac_request_wait(connect, 5_000, info.as_mut_ptr()), 0);
    assert_eq!(pmac_request_release(connect), 0);

    mock.session.block(true);
    let options = PmacRequestOptions {
        struct_size: size_of::<PmacRequestOptions>() as u32,
        abi_version: ABI_VERSION,
        timeout_ms: 5_000,
        flags: REQUEST_INFINITE,
        max_response_bytes: 1_024,
    };
    let mut sent = 0;
    let command = b"P1=42";
    assert_eq!(
        pmac_command_submit(
            client_id,
            command.as_ptr(),
            command.len(),
            &options,
            &mut sent
        ),
        0
    );
    let until = std::time::Instant::now() + Duration::from_secs(5);
    while mock.session.started() == 0 {
        assert!(
            std::time::Instant::now() < until,
            "mock command was not entered"
        );
        std::thread::yield_now();
    }
    assert_eq!(
        pmac_request_wait(sent, 0, info.as_mut_ptr()),
        ErrorCode::WaitPending as i32
    );
    let pending = unsafe { info.assume_init() };
    assert_eq!(
        pending.state,
        pmac_core::RequestState::AwaitingResponse as u32
    );
    assert_eq!(
        pending.certainty,
        pmac_core::ExecutionCertainty::MayHaveExecuted as u32
    );
    let error = error_info(sent);
    assert_eq!(error.status, ErrorCode::WaitPending as i32);
    assert_eq!(
        error.certainty,
        pmac_core::ExecutionCertainty::MayHaveExecuted as u32
    );
    assert_eq!(error.request_id, sent);
    assert_eq!(error.connection_epoch, pending.connection_epoch);
    assert_ne!(
        error.certainty,
        pmac_core::ExecutionCertainty::Confirmed as u32
    );

    let mut queued = 0;
    assert_eq!(
        pmac_command_submit(
            client_id,
            command.as_ptr(),
            command.len(),
            &options,
            &mut queued
        ),
        0
    );
    let error = error_info(queued);
    assert_eq!(error.status, ErrorCode::WaitPending as i32);
    assert_eq!(
        error.certainty,
        pmac_core::ExecutionCertainty::NotSent as u32
    );
    assert_eq!(pmac_request_cancel(queued), 0);
    assert_eq!(
        pmac_request_wait(queued, 5_000, info.as_mut_ptr()),
        ErrorCode::Cancelled as i32
    );
    let error = error_info(queued);
    assert_eq!(error.status, ErrorCode::Cancelled as i32);
    assert_eq!(
        error.certainty,
        pmac_core::ExecutionCertainty::NotSent as u32
    );

    assert_eq!(pmac_request_cancel(sent), 0);
    assert_eq!(
        pmac_request_wait(sent, 5_000, info.as_mut_ptr()),
        ErrorCode::Cancelled as i32
    );
    let terminal = unsafe { info.assume_init() };
    for _ in 0..32 {
        let error = error_info(sent);
        assert_eq!(error.status, ErrorCode::Cancelled as i32);
        assert_eq!(
            error.certainty,
            pmac_core::ExecutionCertainty::MayHaveExecuted as u32
        );
        assert_eq!(error.request_id, sent);
        assert_eq!(error.connection_epoch, terminal.connection_epoch);
        assert_eq!(error.connection_epoch, pending.connection_epoch);
    }
    assert_eq!(
        mock.session.started(),
        1,
        "queued request must not enter transport"
    );
    assert_eq!(pmac_request_release(queued), 0);
    assert_eq!(pmac_request_release(sent), 0);
    assert_eq!(pmac_runtime_shutdown(runtime_id, 5_000), 0);
    let mut pool = unsafe { std::mem::zeroed::<PmacPoolState>() };
    assert_eq!(pmac_pool_get_state(client_id, &mut pool), 0);
    assert_ne!(pool.release_uncertain, 0);
    assert_eq!(
        pmac_client_release(client_id),
        ErrorCode::ShutdownPending as i32
    );
    let mut required = 0;
    assert_eq!(
        pmac_client_recovery_ids(std::ptr::null_mut(), 0, &mut required),
        ErrorCode::BufferTooSmall as i32
    );
    let mut recovery_ids = vec![0; required];
    assert_eq!(
        pmac_client_recovery_ids(recovery_ids.as_mut_ptr(), recovery_ids.len(), &mut required),
        0
    );
    assert!(recovery_ids.contains(&client_id));
    assert_eq!(pmac_pool_confirm_recovery(client_id, pool.revision, 2), 0);
    assert_eq!(pmac_client_release(client_id), 0);
    assert_eq!(pmac_runtime_release(runtime_id), 0);
    assert_eq!(
        pmac_client_recovery_ids(std::ptr::null_mut(), 0, &mut required),
        0
    );
    assert_eq!(required, 0);
}
