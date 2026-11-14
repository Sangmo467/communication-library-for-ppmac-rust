//! Continuous Gather over the controller's gather -w text stream.
use std::collections::HashMap;
use std::io::{ErrorKind, Read};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::helpers::{
    client, finish_request, invalid_handle, read_input, request, submit_owned, text,
};
#[cfg(test)]
use crate::types::PmacBytes;
use crate::{ffi_status, write_id};
use pmac_core::{Error, ErrorCode, Operation, RequestOptions, Runtime};
use pmac_protocol::Command;

/// Return nonzero to request a stop. Data/message pointers live only during the call.
pub type GatherCallback = unsafe extern "C" fn(
    user: *mut std::ffi::c_void,
    status: i32,
    data: *const f64,
    rows: usize,
    items: usize,
    first_sample: u64,
    message: *const u8,
    message_len: usize,
) -> i32;

pub use crate::types::PmacGatherConfig;

struct Config {
    period: u32,
    addresses: Vec<String>,
    local_ip: Option<IpAddr>,
    port: u16,
    max_samples: u32,
    batch_rows: usize,
    queue_batches: usize,
    timeout_ms: u32,
}
impl Config {
    unsafe fn read(input: *const PmacGatherConfig) -> Result<Self, Error> {
        let c = unsafe { read_input(input)? };
        if c.period == 0
            || !(1..=128).contains(&c.items)
            || c.addresses.is_null()
            || c.reserved != 0
        {
            return Err(Error::invalid(
                "Gather needs period > 0 and 1..=128 addresses; reserved must be zero",
            ));
        }
        let mut addresses = Vec::new();
        for a in unsafe { std::slice::from_raw_parts(c.addresses, c.items as usize) } {
            if a.len == 0 || a.len > 256 {
                return Err(Error::invalid("Gather address length must be 1..=256"));
            }
            let address = unsafe { text(a.data, a.len)? };
            // A single address expression, never GPASCII commands or shell text.
            if !address
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.$[]".contains(&b))
            {
                return Err(Error::invalid(
                    "Gather addresses must be single symbolic or numeric address expressions",
                ));
            }
            addresses.push(address);
        }
        if c.local_ip.len > 64 {
            return Err(Error::invalid("local IP is too long"));
        }
        let local_ip = if c.local_ip.len == 0 {
            None
        } else {
            Some(parse_ip(&unsafe {
                text(c.local_ip.data, c.local_ip.len)?
            })?)
        };
        let batch_rows = if c.batch_rows == 0 {
            128
        } else {
            c.batch_rows as usize
        };
        let queue_batches = if c.queue_batches == 0 {
            32
        } else {
            c.queue_batches as usize
        };
        if batch_rows > 4096
            || queue_batches > 1024
            || (batch_rows as u64) * (queue_batches as u64) * (addresses.len() as u64) * 8
                > 64 * 1024 * 1024
        {
            return Err(Error::invalid("Gather queue exceeds the 64 MiB limit"));
        }
        let max_samples = if c.max_samples == 0 {
            2048
        } else {
            c.max_samples
        };
        if !(2..=1_048_576).contains(&max_samples) {
            return Err(Error::invalid("Gather.MaxSamples must be 2..=1048576"));
        }
        Ok(Self {
            period: c.period,
            addresses,
            local_ip,
            port: c.port,
            max_samples,
            batch_rows,
            queue_batches,
            timeout_ms: if c.timeout_ms == 0 {
                5000
            } else {
                c.timeout_ms
            },
        })
    }
}
fn parse_ip(value: &str) -> Result<IpAddr, Error> {
    let ip: IpAddr = value
        .parse()
        .map_err(|_| Error::invalid("Gather local IP must be numeric"))?;
    if ip.is_unspecified() || ip.is_multicast() || ip == IpAddr::V4(std::net::Ipv4Addr::BROADCAST) {
        return Err(Error::invalid("Gather local IP must be a unicast address"));
    }
    Ok(ip)
}

struct Batch {
    values: Vec<f64>,
    first: u64,
}
struct Gather {
    runtime: Arc<Runtime>,
    client: Arc<pmac_core::Client>,
    stop: AtomicBool,
    error: Mutex<Option<Error>>,
    threads: Mutex<Vec<JoinHandle<()>>>,
}
type Sessions = HashMap<u64, Arc<Gather>>;
fn sessions() -> &'static Mutex<Sessions> {
    static SESSIONS: OnceLock<Mutex<Sessions>> = OnceLock::new();
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}
pub(crate) fn has_children(runtime: &Arc<Runtime>) -> bool {
    lock(sessions())
        .values()
        .any(|s| Arc::ptr_eq(&s.runtime, runtime))
}
thread_local! { static IN_CALLBACK: std::cell::Cell<u64> = const { std::cell::Cell::new(0) }; }
fn lookup(id: u64) -> Result<Arc<Gather>, Error> {
    lock(sessions())
        .get(&id)
        .cloned()
        .ok_or_else(invalid_handle)
}
fn stop(id: u64, s: &Gather) -> Result<(), Error> {
    if IN_CALLBACK.with(|v| v.get() == id) {
        return Err(Error::invalid(
            "use pmac_gather_request_stop or return nonzero inside a Gather callback",
        ));
    }
    s.stop.store(true, Ordering::Release);
    // Serialize joins so every successful stop is a callback-lifetime barrier.
    let mut threads = lock(&s.threads);
    for t in threads.drain(..) {
        if t.join().is_err() {
            *lock(&s.error) = Some(Error::internal("Gather thread panicked"));
        }
    }
    lock(&s.error).clone().map_or(Ok(()), Err)
}

#[no_mangle]
pub extern "C" fn pmac_gather_start(
    client_id: u64,
    config: *const PmacGatherConfig,
    callback: Option<GatherCallback>,
    user: *mut std::ffi::c_void,
    out_id: *mut u64,
) -> i32 {
    ffi_status(|| {
        unsafe { write_id(out_id, 0)? };
        let config = unsafe { Config::read(config)? };
        let callback = callback.ok_or_else(|| Error::invalid("Gather callback is required"))?;
        let (client, runtime) = client(client_id)?;
        static NEXT: AtomicU64 = AtomicU64::new(0x4000_0000_0000_0001);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let s = Arc::new(Gather {
            runtime,
            client,
            stop: AtomicBool::new(false),
            error: Mutex::new(None),
            threads: Mutex::new(Vec::new()),
        });
        {
            let mut all = lock(sessions());
            if all.len() >= 64 {
                return Err(Error::new(
                    ErrorCode::ResourceLimit,
                    "Gather session quota is full (64)",
                ));
            }
            all.insert(id, s.clone());
        }
        let (tx, rx) = mpsc::sync_channel::<Batch>(config.queue_batches);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let items = config.addresses.len();
        let user = user as usize;
        let dispatch = s.clone();
        let callback_thread = match thread::Builder::new()
            .name("pmac-gather-callback".into())
            .spawn(move || {
                IN_CALLBACK.with(|v| v.set(id));
                for b in rx {
                    if dispatch.stop.load(Ordering::Acquire) {
                        continue;
                    }
                    let halt = unsafe {
                        callback(
                            user as *mut _,
                            0,
                            b.values.as_ptr(),
                            b.values.len() / items,
                            items,
                            b.first,
                            std::ptr::null(),
                            0,
                        )
                    };
                    if halt != 0 {
                        dispatch.stop.store(true, Ordering::Release);
                    }
                }
                if let Some(e) = lock(&dispatch.error).clone() {
                    unsafe {
                        callback(
                            user as *mut _,
                            e.code as i32,
                            std::ptr::null(),
                            0,
                            items,
                            0,
                            e.message.as_ptr(),
                            e.message.len(),
                        );
                    }
                }
                IN_CALLBACK.with(|v| v.set(0));
            }) {
            Ok(t) => t,
            Err(e) => {
                lock(sessions()).remove(&id);
                return Err(Error::internal(format!(
                    "cannot start Gather callback thread: {e}"
                )));
            }
        };
        lock(&s.threads).push(callback_thread);
        let worker = s.clone();
        let t = thread::Builder::new()
            .name("pmac-gather-receiver".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run(client_id, id, &config, &worker, &tx, &ready_tx)
                }))
                .unwrap_or_else(|_| {
                    Err(Error::internal(
                        "Gather receiver panicked; remote cleanup may be required",
                    ))
                });
                if let Err(e) = result {
                    *lock(&worker.error) = Some(e.clone());
                    let _ = ready_tx.send(Err(e));
                }
                // Drop sender only after storing the final error.
                drop(tx);
            });
        match t {
            Ok(t) => lock(&s.threads).push(t),
            Err(e) => {
                s.stop.store(true, Ordering::Release);
                let _ = stop(id, &s);
                lock(sessions()).remove(&id);
                return Err(Error::internal(format!(
                    "cannot start Gather receiver: {e}"
                )));
            }
        }
        match ready_rx.recv() {
            Ok(Ok(())) => unsafe { write_id(out_id, id) },
            result => {
                let _ = stop(id, &s);
                lock(sessions()).remove(&id);
                Err(match result {
                    Ok(Err(e)) => e,
                    _ => Error::internal("Gather startup thread ended"),
                })
            }
        }
    })
}
#[no_mangle]
pub extern "C" fn pmac_gather_request_stop(id: u64) -> i32 {
    ffi_status(|| {
        lookup(id)?.stop.store(true, Ordering::Release);
        Ok(())
    })
}
#[no_mangle]
pub extern "C" fn pmac_gather_stop(id: u64) -> i32 {
    ffi_status(|| stop(id, lookup(id)?.as_ref()))
}
#[no_mangle]
pub extern "C" fn pmac_gather_release(id: u64) -> i32 {
    ffi_status(|| {
        let s = lookup(id)?;
        if IN_CALLBACK.with(|v| v.get() == id) {
            return Err(Error::invalid("cannot release Gather inside its callback"));
        }
        let result = stop(id, &s);
        lock(sessions()).remove(&id);
        result
    })
}
fn execute(client_id: u64, operation: Operation, timeout_ms: u32) -> Result<Vec<u8>, Error> {
    let id = submit_owned(
        client_id,
        operation,
        RequestOptions {
            timeout_ms,
            max_response_bytes: 65536,
            ..RequestOptions::default()
        },
    )?;
    let (r, _) = request(id)?;
    while !r.wait(50).completed {}
    let exit = r
        .result()
        .and_then(|v| v.as_ref().as_ref().ok().and_then(|v| v.exit_code));
    let bytes = finish_request(r)?;
    if let Some(code) = exit.filter(|code| *code != 0) {
        return Err(Error::new(
            ErrorCode::RemoteError,
            format!("Gather shell exited with code {code}"),
        ));
    }
    Ok(bytes)
}
fn shell(client_id: u64, command: String, timeout_ms: u32) -> Result<Vec<u8>, Error> {
    execute(
        client_id,
        Operation::Shell {
            command,
            persistent: false,
        },
        timeout_ms,
    )
}
fn command(client_id: u64, value: String, timeout_ms: u32) -> Result<(), Error> {
    execute(
        client_id,
        Operation::Command(Command::raw(value)?),
        timeout_ms,
    )
    .map(|_| ())
}
fn check_stop(s: &Gather) -> Result<(), Error> {
    if s.stop.load(Ordering::Acquire) {
        Err(Error::new(ErrorCode::Cancelled, "Gather startup cancelled"))
    } else {
        Ok(())
    }
}

fn ssh_endpoints(connection: &[u8]) -> Result<(IpAddr, IpAddr), Error> {
    let connection = std::str::from_utf8(connection)
        .map_err(|_| Error::new(ErrorCode::ParseError, "invalid SSH endpoint discovery"))?;
    let fields: Vec<_> = connection.split_whitespace().collect();
    if fields.len() != 4 {
        return Err(Error::new(
            ErrorCode::ParseError,
            "invalid SSH endpoint discovery",
        ));
    }
    Ok((parse_ip(fields[0])?, parse_ip(fields[2])?))
}
fn authentication_line() -> String {
    let secret: [u8; 32] = rand::random();
    let mut line = String::from("PMAC-GATHER-1 ");
    for byte in secret {
        use std::fmt::Write;
        // Formatting a byte into a String cannot fail.
        let _ = write!(line, "{byte:02x}");
    }
    line.push('\n');
    line
}
fn check_peer_wait(s: &Gather) -> Result<(), Error> {
    check_stop(s)?;
    if s.runtime.is_shutdown()
        || s.client.snapshot().closing
        || s.client.snapshot().state != pmac_core::ClientState::Ready
    {
        return Err(Error::new(ErrorCode::Closing, "Gather client is closing"));
    }
    Ok(())
}
fn accept_authenticated(
    listener: &TcpListener,
    controller: IpAddr,
    authentication: &[u8],
    s: &Gather,
    deadline: Instant,
) -> Result<TcpStream, Error> {
    if authentication.is_empty() || authentication.len() > 128 {
        return Err(Error::internal("invalid Gather authentication length"));
    }
    loop {
        check_peer_wait(s)?;
        if Instant::now() >= deadline {
            return Err(Error::new(
                ErrorCode::ConnectError,
                "Gather authenticated reverse TCP connection timed out",
            ));
        }
        let (mut stream, peer) = match listener.accept() {
            Ok(pair) => pair,
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5));
                continue;
            }
            Err(e) => return Err(io_error(e)),
        };
        if peer.ip().to_canonical() != controller.to_canonical() {
            continue;
        }
        stream.set_nonblocking(true).map_err(io_error)?;
        let peer_deadline = deadline.min(Instant::now() + Duration::from_millis(200));
        let mut handshake = [0u8; 128];
        let mut received = 0;
        while received < authentication.len() && Instant::now() < peer_deadline {
            check_peer_wait(s)?;
            // Read exactly the handshake, leaving any coalesced sample data in the socket.
            let mut byte = [0u8; 1];
            match stream.read(&mut byte) {
                Ok(1) => {
                    handshake[received] = byte[0];
                    received += 1;
                }
                Ok(_) => break,
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        if received == authentication.len() && handshake[..received] == *authentication {
            check_peer_wait(s)?;
            if Instant::now() >= deadline {
                continue;
            }
            stream.set_nonblocking(false).map_err(io_error)?;
            return Ok(stream);
        }
    }
}
fn run(
    client_id: u64,
    id: u64,
    c: &Config,
    s: &Gather,
    tx: &mpsc::SyncSender<Batch>,
    ready: &mpsc::SyncSender<Result<(), Error>>,
) -> Result<(), Error> {
    // Read both endpoints from the authenticated SSH session, even with a local override.
    let connection = shell(
        client_id,
        "printf '%s\\n' \"$SSH_CONNECTION\"".into(),
        c.timeout_ms,
    )?;
    let (ssh_client, controller) = ssh_endpoints(&connection)?;
    let ip = c.local_ip.unwrap_or(ssh_client);
    let listener = TcpListener::bind(SocketAddr::new(ip, c.port)).map_err(|e| {
        Error::new(
            ErrorCode::ConnectError,
            format!("cannot listen for Gather: {e}"),
        )
    })?;
    listener.set_nonblocking(true).map_err(io_error)?;
    let port = listener.local_addr().map_err(io_error)?.port();
    // Atomic controller-wide ownership prevents two library clients reconfiguring Gather.
    // The directory intentionally survives abrupt loss of SSH until explicit recovery.
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let token = format!("{}-{id}-{nonce}", std::process::id());
    let acquire = format!("umask 077; mkdir /tmp/pmac-library-gather.lock 2>/dev/null || exit 73; printf '%s' '{token}' > /tmp/pmac-library-gather.lock/owner");
    shell(client_id, acquire, c.timeout_ms)?;
    let mut configured = false;
    let result = (|| {
        check_stop(s)?;
        // Match scanner2's preparation: disable the global sampler before opening its data pipe.
        configured = true;
        command(client_id, "Gather.Enable=0".into(), c.timeout_ms)?;
        shell(
            client_id,
            "killall gather nc >/dev/null 2>&1 || true".into(),
            c.timeout_ms,
        )?;
        // All GPASCII submissions originate on this one worker to retain channel affinity.
        for (i, address) in c.addresses.iter().enumerate() {
            check_stop(s)?;
            command(
                client_id,
                format!("Gather.Addr[{i}]={address}"),
                c.timeout_ms,
            )?;
        }
        for text in [
            format!("Gather.Period={}", c.period),
            format!("Gather.Items={}", c.addresses.len()),
            format!("Gather.MaxSamples={}", c.max_samples),
            "Gather.Enable=1".into(),
            "Gather.Enable=0".into(),
        ] {
            check_stop(s)?;
            command(client_id, text, c.timeout_ms)?;
        }
        // Configure a fresh capability over SSH; plaintext TCP still needs a trusted network.
        let authentication = authentication_line();
        let deadline = Instant::now() + Duration::from_millis(c.timeout_ms as u64);
        // nohup detaches the long-running pipeline; the normal Shell lane stays available.
        let capability = authentication
            .strip_prefix("PMAC-GATHER-1 ")
            .and_then(|line| line.strip_suffix('\n'))
            .ok_or_else(|| Error::internal("invalid Gather authentication framing"))?;
        // Keep stdin open without a PTY or helper process. EOF makes gather -w spin
        // on waiting messages on some controllers. Opening this FIFO read/write
        // supplies no bytes and keeps its writer alive until the pipeline exits.
        shell(
            client_id,
            "mkfifo -m 600 /tmp/pmac-library-gather.lock/stdin".into(),
            c.timeout_ms,
        )?;
        shell(client_id, format!("nohup sh -c 'exec 3<>/tmp/pmac-library-gather.lock/stdin; {{ printf \"PMAC-GATHER-1 %s\\n\" \"{capability}\"; stdbuf -oL gather -w <&3; }} | nc -n {ip} {port}' >/dev/null 2>&1 </dev/null &"), c.timeout_ms).map_err(|mut error| {
            // Remote diagnostics must never expose the capability carried by this command.
            error.message = "Gather data pipeline startup failed".into();
            error
        })?;
        let mut stream = accept_authenticated(
            &listener,
            controller,
            authentication.as_bytes(),
            s,
            deadline,
        )?;
        drop(listener);
        stream
            .set_read_timeout(Some(Duration::from_millis(20)))
            .map_err(io_error)?;
        check_stop(s)?;
        command(client_id, "Gather.Enable=3".into(), c.timeout_ms)?;
        let _ = ready.send(Ok(()));
        receive(&mut stream, c, s, tx)
    })();
    let mut cleanup = None;
    if configured {
        if let Err(e) = command(client_id, "Gather.Enable=0".into(), c.timeout_ms) {
            cleanup = Some(e);
        }
    }
    // The single-session contract reserves gather and nc for this API.
    // Verify ownership before killing processes or removing the lock.
    let cleanup_command = format!("if [ \"$(cat /tmp/pmac-library-gather.lock/owner 2>/dev/null)\" = '{token}' ]; then killall gather nc >/dev/null 2>&1 || true; rm -f /tmp/pmac-library-gather.lock/stdin /tmp/pmac-library-gather.lock/owner; rmdir /tmp/pmac-library-gather.lock; fi");
    if let Err(e) = shell(client_id, cleanup_command, c.timeout_ms) {
        cleanup = Some(e);
    }
    match (result, cleanup) {
        (Err(mut e), Some(cleanup)) => {
            e.message.push_str(&format!(
                "; Gather cleanup failed: {} (remote lock may require recovery)",
                cleanup.message
            ));
            Err(e)
        }
        (Err(e), None) => Err(e),
        (Ok(()), Some(e)) => Err(e),
        (Ok(()), None) => Ok(()),
    }
}
fn io_error(e: std::io::Error) -> Error {
    Error::new(ErrorCode::Disconnected, format!("Gather TCP: {e}"))
}

struct Decoder {
    pending: Vec<u8>,
    items: usize,
}
impl Decoder {
    fn new(items: usize) -> Self {
        Self {
            pending: Vec::new(),
            items,
        }
    }
    fn push(
        &mut self,
        input: &[u8],
        mut row: impl FnMut(Vec<f64>) -> Result<(), Error>,
    ) -> Result<(), Error> {
        for &byte in input {
            if byte != b'\n' {
                if self.pending.len() >= 65536 {
                    return Err(Error::new(
                        ErrorCode::ProtocolError,
                        "Gather line exceeds 64 KiB",
                    ));
                }
                self.pending.push(byte);
                continue;
            }
            let line = std::str::from_utf8(&self.pending)
                .map_err(|_| Error::new(ErrorCode::ParseError, "Gather text is not UTF-8"))?
                .trim();
            if !line.is_empty() {
                let lower = line.to_ascii_lowercase();
                if lower.contains("overrun") {
                    return Err(Error::new(
                        ErrorCode::ResourceLimit,
                        "controller Gather buffer overrun; samples were lost",
                    ));
                }
                // gather emits alphabetic headings. Numeric special values are errors.
                if lower.contains("error") || lower.contains("failed") {
                    return Err(Error::new(
                        ErrorCode::RemoteError,
                        format!("Gather: {line}"),
                    ));
                }
                let heading = line.as_bytes()[0].is_ascii_alphabetic()
                    && !line.split_whitespace().any(|v| v.parse::<f64>().is_ok());
                if !heading {
                    let values = line
                        .split_whitespace()
                        .map(|s| s.parse::<f64>())
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|_| {
                            Error::new(ErrorCode::ParseError, "invalid Gather numeric row")
                        })?;
                    if values.len() != self.items || values.iter().any(|v| !v.is_finite()) {
                        return Err(Error::new(
                            ErrorCode::ParseError,
                            "Gather row has wrong column count or non-finite data",
                        ));
                    }
                    row(values)?;
                }
            }
            self.pending.clear();
        }
        Ok(())
    }
}
fn enqueue(
    tx: &mpsc::SyncSender<Batch>,
    values: &mut Vec<f64>,
    next: &mut u64,
    items: usize,
) -> Result<(), Error> {
    if values.is_empty() {
        return Ok(());
    }
    let count = values.len() / items;
    let batch = Batch {
        values: std::mem::take(values),
        first: *next,
    };
    tx.try_send(batch).map_err(|e| match e {
        mpsc::TrySendError::Full(_) => Error::new(
            ErrorCode::ResourceLimit,
            "Gather callback queue overflow; acquisition stopped",
        ),
        mpsc::TrySendError::Disconnected(_) => {
            Error::new(ErrorCode::Cancelled, "Gather callback dispatcher ended")
        }
    })?;
    *next += count as u64;
    Ok(())
}
fn receive(
    stream: &mut TcpStream,
    c: &Config,
    s: &Gather,
    tx: &mpsc::SyncSender<Batch>,
) -> Result<(), Error> {
    let mut decoder = Decoder::new(c.addresses.len());
    let mut data = [0; 8192];
    let mut values = Vec::new();
    let mut next = 0;
    let mut flushed = Instant::now();
    while !s.stop.load(Ordering::Acquire) {
        // Client/runtime close cancels acquisition even when the controller sends no data.
        // Keep an Arc locally; FFI clients may be released concurrently.
        match stream.read(&mut data) {
            Ok(0) => {
                return Err(Error::new(
                    ErrorCode::Disconnected,
                    "Gather data connection closed",
                ))
            }
            Ok(n) => decoder.push(&data[..n], |row| {
                values.extend(row);
                if values.len() / c.addresses.len() >= c.batch_rows {
                    enqueue(tx, &mut values, &mut next, c.addresses.len())?;
                    flushed = Instant::now();
                }
                Ok(())
            })?,
            Err(e)
                if matches!(
                    e.kind(),
                    ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
                ) => {}
            Err(e) => return Err(io_error(e)),
        }
        if flushed.elapsed() >= Duration::from_millis(20) {
            enqueue(tx, &mut values, &mut next, c.addresses.len())?;
            flushed = Instant::now();
        }
        if s.runtime.is_shutdown()
            || s.client.snapshot().closing
            || s.client.snapshot().state != pmac_core::ClientState::Ready
        {
            return Err(Error::new(ErrorCode::Closing, "Gather runtime shut down"));
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{client_kind, registry, Entry};
    use pmac_core::{ClientConfig, ConnectOptions};
    use pmac_testkit::MockTransport;
    use std::io::Write;

    #[test]
    fn decoder_handles_every_fragment_boundary_and_dynamic_columns() {
        let input = b"Gather Data\r\n1 -2.5 3e2\r\n4 5 6\n";
        for split in 0..=input.len() {
            let mut d = Decoder::new(3);
            let mut rows = Vec::new();
            d.push(&input[..split], |r| {
                rows.push(r);
                Ok(())
            })
            .unwrap();
            d.push(&input[split..], |r| {
                rows.push(r);
                Ok(())
            })
            .unwrap();
            assert_eq!(rows, vec![vec![1., -2.5, 300.], vec![4., 5., 6.]]);
        }
    }
    #[test]
    fn decoder_reports_loss_invalid_rows_and_unbounded_lines() {
        for (line, code) in [
            ("Buffer OVERrun\n", ErrorCode::ResourceLimit),
            ("1 2\n", ErrorCode::ParseError),
            ("NaN 2 3\n", ErrorCode::ParseError),
            ("1 inf 3\n", ErrorCode::ParseError),
            ("1 nope 3\n", ErrorCode::ParseError),
        ] {
            assert_eq!(
                Decoder::new(3)
                    .push(line.as_bytes(), |_| Ok(()))
                    .unwrap_err()
                    .code,
                code
            );
        }
        assert_eq!(
            Decoder::new(1)
                .push(&vec![b'1'; 65537], |_| Ok(()))
                .unwrap_err()
                .code,
            ErrorCode::ProtocolError
        );
    }
    #[test]
    fn decoder_mixed_case_diagnostics_survive_byte_fragmentation() {
        for (line, expected) in [
            ("Gather ErRoR: offline\r\n", ErrorCode::RemoteError),
            ("Gather FaIlEd: stopped\n", ErrorCode::RemoteError),
            ("Buffer OvErRuN\n", ErrorCode::ResourceLimit),
        ] {
            let mut decoder = Decoder::new(3);
            let mut rows = Vec::new();
            let mut error = None;
            for byte in line.as_bytes() {
                if let Err(value) = decoder.push(&[*byte], |row| {
                    rows.push(row);
                    Ok(())
                }) {
                    error = Some(value);
                    break;
                }
            }
            assert_eq!(error.unwrap().code, expected);
            assert!(rows.is_empty());
        }
    }
    #[test]
    fn bounded_queue_reports_overflow_and_preserves_sequence() {
        let (tx, rx) = mpsc::sync_channel(1);
        let mut next = 0;
        enqueue(&tx, &mut vec![1., 2., 3., 4.], &mut next, 2).unwrap();
        assert_eq!(next, 2);
        assert_eq!(
            enqueue(&tx, &mut vec![5., 6.], &mut next, 2)
                .unwrap_err()
                .code,
            ErrorCode::ResourceLimit
        );
        assert_eq!(next, 2);
        let batch = rx.recv().unwrap();
        assert_eq!(batch.first, 0);
        assert_eq!(batch.values, vec![1., 2., 3., 4.]);
    }
    fn bytes(value: &str) -> PmacBytes {
        PmacBytes {
            data: value.as_ptr(),
            len: value.len(),
        }
    }
    fn config(addresses: &[PmacBytes]) -> PmacGatherConfig {
        PmacGatherConfig {
            struct_size: size_of::<PmacGatherConfig>() as u32,
            abi_version: crate::types::ABI_VERSION,
            period: 3,
            items: addresses.len() as u32,
            addresses: addresses.as_ptr(),
            local_ip: bytes("127.0.0.1"),
            port: 0,
            reserved: 0,
            max_samples: 0,
            batch_rows: 2,
            queue_batches: 32,
            timeout_ms: 1000,
        }
    }
    unsafe extern "C" fn ignored(
        _: *mut std::ffi::c_void,
        _: i32,
        _: *const f64,
        _: usize,
        _: usize,
        _: u64,
        _: *const u8,
        _: usize,
    ) -> i32 {
        0
    }
    #[test]
    fn config_validates_before_starting_and_zeroes_handle() {
        let addresses = [bytes("Motor[1].ActPos.a\nGather.Enable=3")];
        let c = config(&addresses);
        let mut id = 99;
        assert_eq!(
            pmac_gather_start(0, &c, Some(ignored), std::ptr::null_mut(), &mut id),
            ErrorCode::InvalidArgument as i32
        );
        assert_eq!(id, 0);
        let addresses = [bytes("Motor[1].ActPos.a")];
        let mut c = config(&addresses);
        c.period = 0;
        assert_eq!(
            pmac_gather_start(0, &c, Some(ignored), std::ptr::null_mut(), &mut id),
            ErrorCode::InvalidArgument as i32
        );
        c.period = 1;
        c.items = 129;
        assert_eq!(
            pmac_gather_start(0, &c, Some(ignored), std::ptr::null_mut(), &mut id),
            ErrorCode::InvalidArgument as i32
        );
        assert_eq!(
            parse_ip("0.0.0.0").unwrap_err().code,
            ErrorCode::InvalidArgument
        );
        assert!(parse_ip("127.0.0.1;echo").is_err());
    }

    struct Fixture {
        client_id: u64,
        runtime: Arc<Runtime>,
        mock: Arc<MockTransport>,
    }
    impl Fixture {
        fn new() -> Self {
            let runtime = Runtime::new(2).unwrap();
            // Keep the core runtime directly; avoid consuming the shared 16-ID FFI quota.
            let mock = Arc::new(MockTransport::default());
            let client_id = registry().reserve(client_kind()).unwrap();
            let client = runtime
                .create_client_with_id(client_id, mock.clone(), ClientConfig::default())
                .unwrap();
            registry()
                .put(
                    client_id,
                    Entry::Client {
                        client: client.clone(),
                        runtime: runtime.clone(),
                    },
                )
                .unwrap();
            let connect = submit_owned(
                client_id,
                Operation::Connect(ConnectOptions::new("gather-mock", "root")),
                RequestOptions::default(),
            )
            .unwrap();
            let (r, _) = request(connect).unwrap();
            assert!(r.wait(5000).completed);
            finish_request(r).unwrap();
            Self {
                client_id,
                runtime,
                mock,
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            assert_eq!(crate::pmac_client_close(self.client_id, 0, 5000), 0);
            assert_eq!(crate::pmac_client_release(self.client_id), 0);
            self.runtime.shutdown_clients(5000).unwrap();
        }
    }
    struct Event {
        status: i32,
        values: Vec<f64>,
        rows: usize,
        items: usize,
        first: u64,
    }
    unsafe extern "C" fn collect(
        user: *mut std::ffi::c_void,
        status: i32,
        data: *const f64,
        rows: usize,
        items: usize,
        first: u64,
        _: *const u8,
        _: usize,
    ) -> i32 {
        let sender = unsafe { &*(user as *const mpsc::Sender<Event>) };
        let values = if rows == 0 {
            Vec::new()
        } else {
            unsafe { std::slice::from_raw_parts(data, rows * items).to_vec() }
        };
        let _ = sender.send(Event {
            status,
            values,
            rows,
            items,
            first,
        });
        0
    }
    fn pipeline_authentication(mock: &MockTransport, deadline: Instant) -> String {
        loop {
            for command in mock.session.shell_commands() {
                if command.starts_with("nohup sh -c") {
                    let marker = "printf \"PMAC-GATHER-1 %s\\n\" \"";
                    let start = command
                        .find(marker)
                        .expect("missing authenticated pipeline")
                        + marker.len();
                    let capability = command[start..].split('"').next().unwrap();
                    assert_eq!(capability.len(), 64);
                    assert!(capability.bytes().all(|v| v.is_ascii_hexdigit()));
                    return format!("PMAC-GATHER-1 {capability}\n");
                }
            }
            assert!(Instant::now() < deadline, "pipeline never started");
            thread::sleep(Duration::from_millis(5));
        }
    }
    fn gather_state(f: &Fixture) -> Arc<Gather> {
        Arc::new(Gather {
            runtime: f.runtime.clone(),
            client: client(f.client_id).unwrap().0,
            stop: AtomicBool::new(false),
            error: Mutex::new(None),
            threads: Mutex::new(Vec::new()),
        })
    }
    #[test]
    fn authenticated_accept_has_one_deadline_for_silent_and_repeated_bad_peers() {
        for silent in [true, false] {
            let f = Fixture::new();
            let state = gather_state(&f);
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let address = listener.local_addr().unwrap();
            let stop = Arc::new(AtomicBool::new(false));
            let halt = stop.clone();
            let peer = thread::spawn(move || {
                if silent {
                    let _socket = TcpStream::connect(address).unwrap();
                    while !halt.load(Ordering::Acquire) {
                        thread::sleep(Duration::from_millis(5));
                    }
                } else {
                    while !halt.load(Ordering::Acquire) {
                        if let Ok(mut socket) =
                            TcpStream::connect_timeout(&address, Duration::from_millis(20))
                        {
                            let _ = socket.write_all(b"PMAC-GATHER-1 invalid\n");
                        }
                    }
                }
            });
            let began = Instant::now();
            let error = accept_authenticated(
                &listener,
                "127.0.0.1".parse().unwrap(),
                authentication_line().as_bytes(),
                &state,
                began + Duration::from_millis(70),
            )
            .unwrap_err();
            stop.store(true, Ordering::Release);
            drop(listener);
            peer.join().unwrap();
            assert_eq!(error.code, ErrorCode::ConnectError);
            assert!(began.elapsed() < Duration::from_millis(500));
        }
    }
    #[test]
    fn silent_handshake_is_interruptible_by_stop_and_client_close() {
        for close in [false, true] {
            let f = Fixture::new();
            let state = gather_state(&f);
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let _peer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
            let worker = state.clone();
            let interrupter = thread::spawn(move || {
                thread::sleep(Duration::from_millis(25));
                if close {
                    worker.runtime.shutdown_clients(1000).unwrap();
                } else {
                    worker.stop.store(true, Ordering::Release);
                }
            });
            let began = Instant::now();
            let error = accept_authenticated(
                &listener,
                "127.0.0.1".parse().unwrap(),
                authentication_line().as_bytes(),
                &state,
                began + Duration::from_secs(5),
            )
            .unwrap_err();
            interrupter.join().unwrap();
            assert_eq!(
                error.code,
                if close {
                    ErrorCode::Closing
                } else {
                    ErrorCode::Cancelled
                }
            );
            assert!(began.elapsed() < Duration::from_secs(1));
        }
    }
    #[test]
    fn wrong_source_is_rejected_even_with_the_correct_capability() {
        let f = Fixture::new();
        let state = gather_state(&f);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let authentication = authentication_line();
        let mut peer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        peer.write_all(authentication.as_bytes()).unwrap();
        // Linux may report ENOTCONN after the rejected socket has been reset.
        let peer_ip = peer.peer_addr().unwrap().ip();
        let error = accept_authenticated(
            &listener,
            "127.0.0.2".parse().unwrap(),
            authentication.as_bytes(),
            &state,
            Instant::now() + Duration::from_millis(50),
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::ConnectError);
        assert!(!error.message.contains(authentication.trim()));
        assert_eq!(
            "::ffff:127.0.0.1".parse::<IpAddr>().unwrap().to_canonical(),
            peer_ip
        );
    }
    #[test]
    fn mapped_controller_source_authenticates_without_consuming_first_sample() {
        let f = Fixture::new();
        let state = gather_state(&f);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let authentication = authentication_line();
        let mut peer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        peer.write_all(format!("{authentication}1 2\n").as_bytes())
            .unwrap();
        let mut accepted = accept_authenticated(
            &listener,
            "::ffff:127.0.0.1".parse().unwrap(),
            authentication.as_bytes(),
            &state,
            Instant::now() + Duration::from_secs(1),
        )
        .unwrap();
        accepted
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let mut first = [0; 4];
        accepted.read_exact(&mut first).unwrap();
        assert_eq!(&first, b"1 2\n");
    }
    #[test]
    fn missing_or_invalid_authenticated_endpoint_discovery_fails_closed() {
        for value in [
            b"".as_slice(),
            b"127.0.0.1 22".as_slice(),
            b"127.0.0.1 22 invalid-controller 22".as_slice(),
            b"127.0.0.1 22 0.0.0.0 22".as_slice(),
            b"\xff".as_slice(),
        ] {
            assert!(ssh_endpoints(value).is_err());
        }
        let f = Fixture::new();
        for value in ["", "127.0.0.1 22 invalid-controller 22"] {
            f.mock.session.set_ssh_connection(value);
            let addresses = [bytes("Motor[1].ActPos.a")];
            // A bindable local override cannot bypass controller discovery.
            let c = config(&addresses);
            let mut id = 77;
            let (events, receiver) = mpsc::channel::<Event>();
            assert_ne!(
                pmac_gather_start(
                    f.client_id,
                    &c,
                    Some(collect),
                    &events as *const _ as *mut _,
                    &mut id,
                ),
                0
            );
            assert_eq!(id, 0);
            let error = receiver.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_ne!(error.status, 0);
            assert!(error.values.is_empty());
            assert!(receiver.try_recv().is_err());
            assert!(!has_children(&f.runtime));
            assert!(f.mock.session.commands().is_empty());
            assert!(!f
                .mock
                .session
                .shell_commands()
                .iter()
                .any(|cmd| cmd.starts_with("nohup")));
        }
    }

    // Real localhost TCP, production coordinator/dispatcher, mocked controller commands.
    fn integration(payload: &'static [u8], expected_status: i32, callback: GatherCallback) {
        let f = Fixture::new();
        let reserved = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = reserved.local_addr().unwrap().port();
        drop(reserved);
        let mock = f.mock.clone();
        let (finish_tx, finish_rx) = mpsc::channel();
        let peer = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let socket = loop {
                match TcpStream::connect(("127.0.0.1", port)) {
                    Ok(s) => break s,
                    Err(_) => {
                        assert!(Instant::now() < deadline);
                        thread::sleep(Duration::from_millis(5));
                    }
                }
            };
            let authentication = pipeline_authentication(&mock, deadline);
            // A wrong capability, a capability from an old session, and raw sample data
            // must all be discarded before the legitimate peer authenticates.
            drop(socket);
            for bad in [
                b"PMAC-GATHER-1 wrong\n".as_slice(),
                authentication_line().as_bytes(),
                b"1 2\n".as_slice(),
            ] {
                let mut rejected = TcpStream::connect(("127.0.0.1", port)).unwrap();
                rejected.write_all(bad).unwrap();
            }
            let silent = TcpStream::connect(("127.0.0.1", port)).unwrap();
            let mut socket = TcpStream::connect(("127.0.0.1", port)).unwrap();
            // Coalescing the first sample with the handshake must preserve every byte.
            let mut packet = authentication.into_bytes();
            packet.extend_from_slice(payload);
            socket.write_all(&packet).unwrap();
            drop(silent);
            let _ = finish_rx.recv_timeout(Duration::from_secs(5));
        });
        let addresses = [bytes("Motor[1].ActPos.a"), bytes("Motor[2].ActPos.a")];
        let mut c = config(&addresses);
        c.port = port;
        let (events_tx, events_rx) = mpsc::channel::<Event>();
        let mut id = 0;
        assert_eq!(
            pmac_gather_start(
                f.client_id,
                &c,
                Some(callback),
                &events_tx as *const _ as *mut _,
                &mut id
            ),
            0
        );
        assert!(has_children(&f.runtime));
        let event = events_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(event.status, expected_status);
        if expected_status == 0 {
            assert_eq!(event.values, vec![1., 2., 3., 4.]);
            assert_eq!((event.rows, event.items, event.first), (2, 2, 0));
        }
        assert_eq!(pmac_gather_stop(id), expected_status);
        assert_eq!(pmac_gather_release(id), expected_status);
        assert!(!has_children(&f.runtime));
        assert_eq!(
            pmac_gather_request_stop(id),
            ErrorCode::InvalidHandle as i32
        );
        assert_eq!(events_rx.try_recv().err(), Some(mpsc::TryRecvError::Empty));
        assert_eq!(f.mock.session.commands().last().unwrap(), "Gather.Enable=0");
        finish_tx.send(()).unwrap();
        peer.join().unwrap();
    }
    #[test]
    fn continuous_gather_configures_delivers_and_joins_callbacks() {
        integration(b"Gather Data\n1 2\n3 4\n", 0, collect);
    }
    #[test]
    fn controller_overrun_is_a_terminal_callback_and_stop_error() {
        integration(
            b"Gather buffer overrun\n",
            ErrorCode::ResourceLimit as i32,
            collect,
        );
    }
    unsafe extern "C" fn collect_and_stop(
        user: *mut std::ffi::c_void,
        status: i32,
        data: *const f64,
        rows: usize,
        items: usize,
        first: u64,
        message: *const u8,
        message_len: usize,
    ) -> i32 {
        let id = IN_CALLBACK.with(|v| v.get());
        let valid = pmac_gather_stop(id) == ErrorCode::InvalidArgument as i32
            && pmac_gather_release(id) == ErrorCode::InvalidArgument as i32
            && pmac_gather_request_stop(id) == 0;
        unsafe {
            collect(
                user,
                if valid {
                    status
                } else {
                    ErrorCode::Internal as i32
                },
                data,
                rows,
                items,
                first,
                message,
                message_len,
            )
        };
        1
    }
    #[test]
    fn callback_can_request_stop_and_blocking_reentrant_operations_are_rejected() {
        integration(b"1 2\n3 4\n", 0, collect_and_stop);
    }
    #[test]
    fn failed_reverse_connection_rolls_back_local_session_and_callbacks() {
        let f = Fixture::new();
        let addresses = [bytes("Motor[1].ActPos.a")];
        let mut c = config(&addresses);
        c.timeout_ms = 50;
        let (sender, receiver) = mpsc::channel::<Event>();
        let mut id = 77;
        assert_eq!(
            pmac_gather_start(
                f.client_id,
                &c,
                Some(collect),
                &sender as *const _ as *mut _,
                &mut id
            ),
            ErrorCode::ConnectError as i32
        );
        assert_eq!(id, 0);
        assert!(!has_children(&f.runtime));
        assert_eq!(
            receiver
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .status,
            ErrorCode::ConnectError as i32
        );
        assert_eq!(receiver.try_recv().err(), Some(mpsc::TryRecvError::Empty));
    }
    #[test]
    fn selected_port_conflict_returns_before_controller_configuration() {
        let f = Fixture::new();
        let occupied = TcpListener::bind("127.0.0.1:0").unwrap();
        let addresses = [bytes("Motor[1].ActPos.a")];
        let mut c = config(&addresses);
        c.port = occupied.local_addr().unwrap().port();
        let mut id = 77;
        assert_eq!(
            pmac_gather_start(
                f.client_id,
                &c,
                Some(ignored),
                std::ptr::null_mut(),
                &mut id
            ),
            ErrorCode::ConnectError as i32
        );
        assert_eq!(id, 0);
        assert!(!has_children(&f.runtime));
        assert!(f.mock.session.commands().is_empty());
    }
}
