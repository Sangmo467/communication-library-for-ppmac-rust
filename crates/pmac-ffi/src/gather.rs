//! Continuous Gather over the controller's gather -W text stream.
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
        // Configure a fresh capability over SSH; plaintext TCP still needs a trusted network.
        let authentication = authentication_line();
        let deadline = Instant::now() + Duration::from_millis(c.timeout_ms as u64);
        // nohup detaches the long-running pipeline; the normal Shell lane stays available.
        let capability = authentication
            .strip_prefix("PMAC-GATHER-1 ")
            .and_then(|line| line.strip_suffix('\n'))
            .ok_or_else(|| Error::internal("invalid Gather authentication framing"))?;
        shell(client_id, format!("nohup sh -c '{{ printf \"PMAC-GATHER-1 %s\\n\" \"{capability}\"; stdbuf -oL gather -W; }} | nc -n {ip} {port}' >/dev/null 2>&1 </dev/null &"), c.timeout_ms).map_err(|mut error| {
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
            "Gather.Enable=3".into(),
        ] {
            check_stop(s)?;
            command(client_id, text, c.timeout_ms)?;
        }
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
    let cleanup_command = format!("if [ \"$(cat /tmp/pmac-library-gather.lock/owner 2>/dev/null)\" = '{token}' ]; then killall gather nc >/dev/null 2>&1 || true; rm -f /tmp/pmac-library-gather.lock/owner; rmdir /tmp/pmac-library-gather.lock; fi");
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
                // gather -W emits alphabetic headings. Numeric special values are errors.
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
