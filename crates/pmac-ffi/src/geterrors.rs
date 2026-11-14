//! Continuous geterrors monitoring, with a serialized callback thread and stop barrier.
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use futures::FutureExt;
use pmac_core::{Client, Error, ErrorCode, GetErrorsOptions, Runtime};
use tokio::sync::Notify;

use crate::helpers::{client, invalid_handle, read_input};
use crate::types::PmacGetErrorsConfig;
use crate::{ffi_status, write_id};

const SETTINGS: u32 = 1;
const MESSAGE: u32 = 2;
const STDERR: u32 = 3;
const TERMINAL: u32 = 4;
pub type Callback = unsafe extern "C" fn(*mut std::ffi::c_void, i32, u32, *const u8, usize) -> i32;

struct Config {
    options: GetErrorsOptions,
    timeout: Duration,
    queue: usize,
    max_line: usize,
}
impl Config {
    unsafe fn read(input: *const PmacGetErrorsConfig) -> Result<Self, Error> {
        let c = unsafe { read_input(input)? };
        if !c.period_seconds.is_finite()
            || !(0.01..=1.0).contains(&c.period_seconds)
            || c.flags & !31 != 0
        {
            return Err(Error::invalid(
                "geterrors needs a finite period in 0.01..=1.0 seconds and valid flags",
            ));
        }
        let queue = if c.queue_lines == 0 {
            256
        } else {
            c.queue_lines as usize
        };
        let max_line = if c.max_line_bytes == 0 {
            65536
        } else {
            c.max_line_bytes as usize
        };
        if queue > 4096
            || max_line > 1048576
            || (queue as u64) * (max_line as u64) > 64 * 1024 * 1024
        {
            return Err(Error::invalid(
                "geterrors queue exceeds the line or 64 MiB capacity limit",
            ));
        }
        Ok(Self {
            options: GetErrorsOptions {
                period_seconds: c.period_seconds,
                append_date: c.flags & 1 != 0,
                coord_mask: (c.flags & 2 != 0).then_some(c.coord_mask),
                motor_mask: (c.flags & 4 != 0).then_some(c.motor_mask),
                ring_mask: (c.flags & 8 != 0).then_some(c.ring_mask),
                global_mask: (c.flags & 16 != 0).then_some(c.global_mask),
            },
            timeout: Duration::from_millis(u64::from(if c.timeout_ms == 0 {
                5000
            } else {
                c.timeout_ms
            })),
            queue,
            max_line,
        })
    }
}
struct Event {
    kind: u32,
    text: Vec<u8>,
}
struct Monitor {
    runtime: Arc<Runtime>,
    client: Arc<Client>,
    halt: AtomicBool,
    changed: Notify,
    error: Mutex<Option<Error>>,
    done: Mutex<Option<mpsc::Receiver<()>>>,
    callback: Mutex<Option<JoinHandle<()>>>,
    join: Mutex<()>,
}
impl Monitor {
    fn request_stop(&self) {
        self.halt.store(true, Ordering::Release);
        self.changed.notify_one();
    }
    fn stopping(&self) -> bool {
        self.halt.load(Ordering::Acquire)
    }
}
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}
fn monitors() -> &'static Mutex<HashMap<u64, Arc<Monitor>>> {
    static ALL: OnceLock<Mutex<HashMap<u64, Arc<Monitor>>>> = OnceLock::new();
    ALL.get_or_init(|| Mutex::new(HashMap::new()))
}
pub(crate) fn has_children(runtime: &Arc<Runtime>) -> bool {
    lock(monitors())
        .values()
        .any(|m| Arc::ptr_eq(&m.runtime, runtime))
}
fn lookup(id: u64) -> Result<Arc<Monitor>, Error> {
    lock(monitors())
        .get(&id)
        .cloned()
        .ok_or_else(invalid_handle)
}
thread_local! { static IN_CALLBACK: std::cell::Cell<u64> = const { std::cell::Cell::new(0) }; }
fn stop(id: u64, monitor: &Monitor) -> Result<(), Error> {
    if IN_CALLBACK.with(|v| v.get() == id) {
        return Err(Error::invalid(
            "use geterrors request_stop or return nonzero from its callback",
        ));
    }
    monitor.request_stop();
    let _join = lock(&monitor.join);
    if let Some(done) = lock(&monitor.done).take() {
        if done.recv().is_err() && lock(&monitor.error).is_none() {
            *lock(&monitor.error) = Some(Error::internal(
                "geterrors worker exited without completion",
            ));
        }
    }
    if let Some(callback) = lock(&monitor.callback).take() {
        if callback.join().is_err() {
            *lock(&monitor.error) = Some(Error::internal("geterrors callback thread panicked"));
        }
    }
    lock(&monitor.error).clone().map_or(Ok(()), Err)
}

#[no_mangle]
pub extern "C" fn pmac_geterrors_start(
    client_id: u64,
    config: *const PmacGetErrorsConfig,
    callback: Option<Callback>,
    user: *mut std::ffi::c_void,
    out: *mut u64,
) -> i32 {
    ffi_status(|| {
        unsafe { write_id(out, 0)? };
        let config = unsafe { Config::read(config)? };
        let callback = callback.ok_or_else(|| Error::invalid("geterrors callback is required"))?;
        let (client, runtime) = client(client_id)?;
        static NEXT: AtomicU64 = AtomicU64::new(0x5000_0000_0000_0001);
        let id = NEXT
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
                (v < 0x5fff_ffff_ffff_ffff).then_some(v + 1)
            })
            .map_err(|_| {
                Error::new(ErrorCode::ResourceLimit, "geterrors handle space exhausted")
            })?;
        let (done_tx, done_rx) = mpsc::channel();
        let monitor = Arc::new(Monitor {
            runtime,
            client,
            halt: AtomicBool::new(false),
            changed: Notify::new(),
            error: Mutex::new(None),
            done: Mutex::new(Some(done_rx)),
            callback: Mutex::new(None),
            join: Mutex::new(()),
        });
        {
            let mut all = lock(monitors());
            if all.len() >= 64
                || all
                    .values()
                    .any(|m| Arc::ptr_eq(&m.client, &monitor.client))
            {
                return Err(Error::new(
                    ErrorCode::ResourceLimit,
                    "only one geterrors handle per Client and 64 per process are allowed",
                ));
            }
            all.insert(id, monitor.clone());
        }
        let (tx, rx) = mpsc::sync_channel::<Event>(config.queue);
        let dispatch = monitor.clone();
        let user = user as usize;
        let callback_thread = thread::Builder::new()
            .name("pmac-geterrors-callback".into())
            .spawn(move || {
                IN_CALLBACK.with(|v| v.set(id));
                for event in rx {
                    if dispatch.stopping() {
                        continue;
                    }
                    if unsafe {
                        callback(
                            user as *mut _,
                            0,
                            event.kind,
                            event.text.as_ptr(),
                            event.text.len(),
                        )
                    } != 0
                    {
                        dispatch.request_stop();
                    }
                }
                if let Some(error) = lock(&dispatch.error).clone() {
                    unsafe {
                        callback(
                            user as *mut _,
                            error.code as i32,
                            TERMINAL,
                            error.message.as_ptr(),
                            error.message.len(),
                        );
                    }
                }
                IN_CALLBACK.with(|v| v.set(0));
            });
        match callback_thread {
            Ok(t) => *lock(&monitor.callback) = Some(t),
            Err(e) => {
                lock(monitors()).remove(&id);
                return Err(Error::internal(format!(
                    "cannot start geterrors callback thread: {e}"
                )));
            }
        }
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let worker = monitor.clone();
        monitor.runtime.spawn(async move {
            let outcome = std::panic::AssertUnwindSafe(run(&config, &worker, &tx, &ready_tx))
                .catch_unwind()
                .await;
            let result = outcome.unwrap_or_else(|_| {
                Err(Error::internal(
                    "geterrors worker panicked; remote cleanup may be required",
                ))
            });
            if let Err(error) = result {
                *lock(&worker.error) = Some(error.clone());
                let _ = ready_tx.try_send(Err(error));
            }
            drop(tx);
            drop(ready_tx);
            let _ = done_tx.send(());
        });
        match ready_rx.recv() {
            Ok(Ok(())) => unsafe { write_id(out, id) },
            result => {
                let _ = stop(id, &monitor);
                lock(monitors()).remove(&id);
                Err(match result {
                    Ok(Err(e)) => e,
                    _ => Error::internal("geterrors startup ended unexpectedly"),
                })
            }
        }
    })
}
#[no_mangle]
pub extern "C" fn pmac_geterrors_request_stop(id: u64) -> i32 {
    ffi_status(|| {
        lookup(id)?.request_stop();
        Ok(())
    })
}
#[no_mangle]
pub extern "C" fn pmac_geterrors_stop(id: u64) -> i32 {
    ffi_status(|| stop(id, lookup(id)?.as_ref()))
}
#[no_mangle]
pub extern "C" fn pmac_geterrors_release(id: u64) -> i32 {
    ffi_status(|| {
        let monitor = lookup(id)?;
        if IN_CALLBACK.with(|v| v.get() == id) {
            return Err(Error::invalid(
                "cannot release geterrors inside its callback",
            ));
        }
        let result = stop(id, &monitor);
        lock(monitors()).remove(&id);
        result
    })
}
fn enqueue(tx: &mpsc::SyncSender<Event>, event: Event) -> Result<(), Error> {
    tx.try_send(event).map_err(|e| match e {
        mpsc::TrySendError::Full(_) => Error::new(
            ErrorCode::ResourceLimit,
            "geterrors callback queue overflow; monitoring stopped",
        ),
        mpsc::TrySendError::Disconnected(_) => {
            Error::new(ErrorCode::Cancelled, "geterrors callback dispatcher ended")
        }
    })
}
struct Lines {
    pending: Vec<u8>,
    startup: bool,
    stderr: bool,
    maximum: usize,
}
impl Lines {
    fn new(stderr: bool, maximum: usize) -> Self {
        Self {
            pending: Vec::new(),
            startup: true,
            stderr,
            maximum,
        }
    }
    fn emit(&mut self, tx: &mpsc::SyncSender<Event>) -> Result<(), Error> {
        let mut bytes = std::mem::take(&mut self.pending);
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| Error::new(ErrorCode::ParseError, "geterrors output is not UTF-8"))?;
        if text.bytes().all(|b| b == 0x06 || b.is_ascii_whitespace()) {
            return Ok(());
        }
        let settings = self.startup && is_setting(text.trim());
        let kind = if self.stderr {
            STDERR
        } else if settings {
            SETTINGS
        } else {
            MESSAGE
        };
        if !self.stderr && (!settings || text.trim().starts_with("-t")) {
            self.startup = false;
        }
        enqueue(tx, Event { kind, text: bytes })
    }
    fn push(&mut self, bytes: &[u8], tx: &mpsc::SyncSender<Event>) -> Result<(), Error> {
        for &b in bytes {
            if b == b'\n' {
                self.emit(tx)?;
            } else {
                if self.pending.len() >= self.maximum {
                    return Err(Error::new(
                        ErrorCode::ResourceLimit,
                        "geterrors line exceeds configured maximum",
                    ));
                }
                self.pending.push(b);
            }
        }
        Ok(())
    }
    fn finish(&mut self, tx: &mpsc::SyncSender<Event>) -> Result<(), Error> {
        if !self.pending.is_empty() {
            self.emit(tx)?;
        }
        Ok(())
    }
}
fn is_setting(text: &str) -> bool {
    if text == "-T" {
        return true;
    }
    if let Some(period) = text.strip_prefix("-t") {
        return period
            .parse::<f64>()
            .is_ok_and(|v| v.is_finite() && (0.01..=1.0).contains(&v));
    }
    ["-g0x", "-c0x", "-m0x", "-r0x"].iter().any(|prefix| {
        text.strip_prefix(prefix)
            .is_some_and(|hex| !hex.is_empty() && hex.bytes().all(|b| b.is_ascii_hexdigit()))
    })
}
async fn run(
    c: &Config,
    monitor: &Monitor,
    tx: &mpsc::SyncSender<Event>,
    ready: &mpsc::SyncSender<Result<(), Error>>,
) -> Result<(), Error> {
    let mut stream = tokio::time::timeout(c.timeout, monitor.client.start_geterrors(&c.options))
        .await
        .map_err(|_| {
            Error::new(
                ErrorCode::ExecTimeout,
                "geterrors SSH channel startup timed out",
            )
        })??;
    let _ = ready.try_send(Ok(()));
    let mut stdout = Lines::new(false, c.max_line);
    let mut stderr = Lines::new(true, c.max_line);
    let result = async {
        loop {
            if monitor.stopping() {
                return Ok(());
            }
            if monitor.client.snapshot().closing || monitor.runtime.is_shutdown() {
                return Err(Error::new(
                    ErrorCode::Closing,
                    "geterrors Client or Runtime is closing",
                ));
            }
            let chunk = tokio::select! {
                _ = monitor.changed.notified() => { continue; }
                _ = tokio::time::sleep(Duration::from_millis(50)) => { continue; }
                chunk = stream.read() => chunk?,
            };
            match chunk {
                Some(chunk) if chunk.stderr => stderr.push(&chunk.data, tx)?,
                Some(chunk) => stdout.push(&chunk.data, tx)?,
                None => {
                    stdout.finish(tx)?;
                    stderr.finish(tx)?;
                    return Err(Error::new(
                        ErrorCode::Disconnected,
                        "geterrors exited or its SSH channel closed unexpectedly",
                    ));
                }
            }
        }
    }
    .await;
    let cleanup = stream.stop().await;
    match (result, cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(e), Ok(())) | (Ok(()), Err(e)) => Err(e),
        (Err(mut e), Err(cleanup)) => {
            e.message.push_str(&format!(
                "; geterrors Ctrl+C cleanup failed: {}",
                cleanup.message
            ));
            Err(e)
        }
    }
}
#[cfg(test)]
#[path = "geterrors_tests.rs"]
mod tests;
