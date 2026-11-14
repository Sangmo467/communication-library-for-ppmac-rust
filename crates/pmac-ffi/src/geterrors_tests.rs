use super::*;
use crate::registry::{client_kind, registry, Entry};
use crate::types::ABI_VERSION;
use async_trait::async_trait;
use pmac_core::{
    ClientConfig, ConnectOptions, ErrorMonitorStream, ExecutionContext, MonitorChunk, Operation,
    OperationResult, RequestOptions, Session, Transport,
};
use pmac_protocol::Command;
use std::sync::atomic::AtomicUsize;

#[test]
fn config_rejects_period_flags_and_memory_overflow_before_handle_lookup() {
    for period in [0., 0.009, 1.001, f64::NAN, f64::INFINITY] {
        let mut c = config();
        c.period_seconds = period;
        let mut id = 123;
        assert_eq!(
            pmac_geterrors_start(0, &c, Some(collect), std::ptr::null_mut(), &mut id),
            ErrorCode::InvalidArgument as i32
        );
        assert_eq!(id, 0);
    }
    for (queue, max_line, flags) in [(4097, 1, 0), (4096, 1048576, 0), (1, 1, 32)] {
        let mut c = config();
        c.queue_lines = queue;
        c.max_line_bytes = max_line;
        c.flags = flags;
        assert!(unsafe { Config::read(&c) }.is_err());
    }
    let mut c = config();
    c.coord_mask = 123;
    c.global_mask = 456;
    let parsed = unsafe { Config::read(&c) }.unwrap();
    assert_eq!(parsed.options.coord_mask, None);
    assert_eq!(parsed.options.global_mask, None);
    c.flags = 31;
    c.coord_mask = 0;
    let parsed = unsafe { Config::read(&c) }.unwrap();
    assert_eq!(parsed.options.coord_mask, Some(0));
    assert!(parsed.options.append_date);
}
#[test]
fn lines_preserve_fragmented_utf8_settings_transitions_and_stderr() {
    let payload = "\r\n\x06\r\n-T\r\n-g0x7ff\r\n-c0x180000000807ffc\n-m0x8c007ffc\n-r0x803e\n-t0.010000\n状态 changed\n-t0.010000\n";
    for split in 0..=payload.len() {
        let (tx, rx) = mpsc::sync_channel(16);
        let mut lines = Lines::new(false, 1000);
        lines.push(&payload.as_bytes()[..split], &tx).unwrap();
        lines.push(&payload.as_bytes()[split..], &tx).unwrap();
        drop(tx);
        let events: Vec<_> = rx.into_iter().collect();
        assert_eq!(events.len(), 8);
        assert!(events[..6].iter().all(|e| e.kind == SETTINGS));
        assert_eq!(events[6].kind, MESSAGE);
        assert_eq!(events[6].text, "状态 changed".as_bytes());
        assert_eq!(events[7].kind, MESSAGE);
    }
    let (tx, rx) = mpsc::sync_channel(1);
    let mut stderr = Lines::new(true, 32);
    stderr.push(b"geterrors failed", &tx).unwrap();
    stderr.finish(&tx).unwrap();
    assert_eq!(rx.recv().unwrap().kind, STDERR);
}
#[test]
fn lines_stop_on_queue_overflow_long_lines_and_invalid_utf8() {
    let (tx, _rx) = mpsc::sync_channel(1);
    assert_eq!(
        Lines::new(false, 100)
            .push(b"a\nb\n", &tx)
            .unwrap_err()
            .code,
        ErrorCode::ResourceLimit
    );
    let (tx, _rx) = mpsc::sync_channel(1);
    assert_eq!(
        Lines::new(false, 3).push(b"abcd", &tx).unwrap_err().code,
        ErrorCode::ResourceLimit
    );
    assert_eq!(
        Lines::new(false, 100)
            .push(b"\xff\n", &tx)
            .unwrap_err()
            .code,
        ErrorCode::ParseError
    );
}
fn config() -> PmacGetErrorsConfig {
    PmacGetErrorsConfig {
        struct_size: size_of::<PmacGetErrorsConfig>() as u32,
        abi_version: ABI_VERSION,
        period_seconds: 0.01,
        coord_mask: 0,
        motor_mask: 0,
        ring_mask: 0,
        global_mask: 0,
        flags: 0,
        timeout_ms: 1000,
        queue_lines: 0,
        max_line_bytes: 0,
    }
}
struct Fake {
    rx: Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<MonitorChunk>>>,
    stops: Arc<AtomicUsize>,
}
struct Stream {
    rx: tokio::sync::mpsc::UnboundedReceiver<MonitorChunk>,
    stops: Arc<AtomicUsize>,
}
#[async_trait]
impl ErrorMonitorStream for Stream {
    async fn read(&mut self) -> Result<Option<MonitorChunk>, Error> {
        Ok(self.rx.recv().await)
    }
    async fn stop(&mut self) -> Result<(), Error> {
        self.stops.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }
}
#[async_trait]
impl Transport for Fake {
    async fn connect(
        &self,
        _: &ConnectOptions,
        _: &ExecutionContext,
    ) -> Result<Arc<dyn Session>, Error> {
        // connect owns a fresh Arc with the one preinstalled monitor stream.
        Ok(Arc::new(Fake {
            rx: Mutex::new(lock(&self.rx).take()),
            stops: self.stops.clone(),
        }))
    }
}
#[async_trait]
impl Session for Fake {
    async fn geterrors(&self, _: &GetErrorsOptions) -> Result<Box<dyn ErrorMonitorStream>, Error> {
        Ok(Box::new(Stream {
            rx: lock(&self.rx)
                .take()
                .ok_or_else(|| Error::internal("mock stream already taken"))?,
            stops: self.stops.clone(),
        }))
    }
    async fn command(&self, _: &Command, _: &ExecutionContext) -> Result<OperationResult, Error> {
        Ok(OperationResult::default())
    }
    async fn shell(
        &self,
        _: &str,
        _: bool,
        _: &ExecutionContext,
    ) -> Result<OperationResult, Error> {
        Ok(OperationResult::default())
    }
    async fn upload(
        &self,
        _: &str,
        _: &str,
        _: &ExecutionContext,
    ) -> Result<OperationResult, Error> {
        Ok(OperationResult::default())
    }
    async fn download(
        &self,
        _: &str,
        _: &str,
        _: &ExecutionContext,
    ) -> Result<OperationResult, Error> {
        Ok(OperationResult::default())
    }
    async fn file_exists(&self, _: &str, _: &ExecutionContext) -> Result<OperationResult, Error> {
        Ok(OperationResult::default())
    }
    async fn file_remove(&self, _: &str, _: &ExecutionContext) -> Result<OperationResult, Error> {
        Ok(OperationResult::default())
    }
    fn is_healthy(&self) -> bool {
        true
    }
    async fn close(&self) -> Result<(), Error> {
        Ok(())
    }
}
struct Fixture {
    runtime: Arc<Runtime>,
    client_id: u64,
    tx: tokio::sync::mpsc::UnboundedSender<MonitorChunk>,
    stops: Arc<AtomicUsize>,
}
impl Fixture {
    fn new() -> Self {
        let runtime = Runtime::new(2).unwrap();
        let client_id = registry().reserve(client_kind()).unwrap();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let stops = Arc::new(AtomicUsize::new(0));
        let fake = Arc::new(Fake {
            rx: Mutex::new(Some(rx)),
            stops: stops.clone(),
        });
        let c = runtime
            .create_client_with_id(client_id, fake, ClientConfig::default())
            .unwrap();
        registry()
            .put(
                client_id,
                Entry::Client {
                    client: c.clone(),
                    runtime: runtime.clone(),
                },
            )
            .unwrap();
        let request = c
            .submit(
                runtime.next_id(),
                Operation::Connect(ConnectOptions::new("mock", "root")),
                RequestOptions::default(),
            )
            .unwrap();
        assert_eq!(request.wait(5000).info.code, ErrorCode::Ok);
        Self {
            runtime,
            client_id,
            tx,
            stops,
        }
    }
    fn send(&self, text: &str) {
        self.tx
            .send(MonitorChunk {
                data: text.as_bytes().to_vec(),
                stderr: false,
            })
            .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(crate::pmac_client_close(self.client_id, 0, 5000), 0);
        assert_eq!(crate::pmac_client_release(self.client_id), 0);
        self.runtime.shutdown_clients(5000).unwrap();
    }
}
type Received = (i32, u32, Vec<u8>);
unsafe extern "C" fn collect(
    user: *mut std::ffi::c_void,
    status: i32,
    kind: u32,
    text: *const u8,
    len: usize,
) -> i32 {
    let tx = unsafe { &*(user as *const mpsc::Sender<Received>) };
    let text = if len == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(text, len).to_vec() }
    };
    let _ = tx.send((status, kind, text));
    0
}
#[test]
fn ffi_continuously_delivers_then_stops_and_releases_with_no_late_callback() {
    let f = Fixture::new();
    let (tx, rx) = mpsc::channel::<Received>();
    let mut id = 0;
    assert_eq!(
        pmac_geterrors_start(
            f.client_id,
            &config(),
            Some(collect),
            &tx as *const _ as *mut _,
            &mut id
        ),
        0
    );
    assert!(has_children(&f.runtime));
    f.send("-g0x7ff\n-t0.010000\n");
    assert_eq!(rx.recv_timeout(Duration::from_secs(1)).unwrap().1, SETTINGS);
    assert_eq!(rx.recv_timeout(Duration::from_secs(1)).unwrap().1, SETTINGS);
    f.send("Motor[1] status changed\r\n");
    let message = rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(message, (0, MESSAGE, b"Motor[1] status changed".to_vec()));
    assert_eq!(pmac_geterrors_stop(id), 0);
    assert!(f.stops.load(Ordering::Acquire) > 0);
    assert!(rx.try_recv().is_err());
    assert_eq!(pmac_geterrors_stop(id), 0);
    assert_eq!(pmac_geterrors_release(id), 0);
    assert!(!has_children(&f.runtime));
    assert_eq!(pmac_geterrors_stop(id), ErrorCode::InvalidHandle as i32);
}
#[test]
fn client_shutdown_stops_monitor_before_runtime_workers_exit() {
    let f = Fixture::new();
    let (tx, rx) = mpsc::channel::<Received>();
    let mut id = 0;
    assert_eq!(
        pmac_geterrors_start(
            f.client_id,
            &config(),
            Some(collect),
            &tx as *const _ as *mut _,
            &mut id
        ),
        0
    );
    f.runtime.shutdown_clients(5000).unwrap();
    assert!(f.stops.load(Ordering::Acquire) > 0);
    assert_eq!(pmac_geterrors_stop(id), ErrorCode::Closing as i32);
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(1)).unwrap().0,
        ErrorCode::Closing as i32
    );
    assert_eq!(pmac_geterrors_release(id), ErrorCode::Closing as i32);
}
unsafe extern "C" fn collect_and_stop(
    user: *mut std::ffi::c_void,
    status: i32,
    kind: u32,
    text: *const u8,
    len: usize,
) -> i32 {
    let id = IN_CALLBACK.with(|v| v.get());
    assert_eq!(pmac_geterrors_stop(id), ErrorCode::InvalidArgument as i32);
    assert_eq!(
        pmac_geterrors_release(id),
        ErrorCode::InvalidArgument as i32
    );
    assert_eq!(pmac_geterrors_request_stop(id), 0);
    unsafe { collect(user, status, kind, text, len) };
    1
}
#[test]
fn callback_can_request_stop_but_blocking_stop_and_release_are_rejected() {
    let f = Fixture::new();
    let (tx, rx) = mpsc::channel::<Received>();
    let mut id = 0;
    assert_eq!(
        pmac_geterrors_start(
            f.client_id,
            &config(),
            Some(collect_and_stop),
            &tx as *const _ as *mut _,
            &mut id
        ),
        0
    );
    f.send("change\n");
    rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(pmac_geterrors_stop(id), 0);
    assert_eq!(pmac_geterrors_release(id), 0);
}

#[test]
fn failed_start_zeroes_handle_and_finishes_callbacks_before_returning() {
    let f = Fixture::new();
    assert_eq!(crate::pmac_client_close(f.client_id, 0, 5000), 0);
    let (tx, rx) = mpsc::channel::<Received>();
    let mut id = 99;
    assert_eq!(
        pmac_geterrors_start(
            f.client_id,
            &config(),
            Some(collect),
            &tx as *const _ as *mut _,
            &mut id
        ),
        ErrorCode::Closing as i32
    );
    assert_eq!(id, 0);
    assert!(!has_children(&f.runtime));
    let terminal = rx.try_recv().unwrap();
    assert_eq!(
        (terminal.0, terminal.1),
        (ErrorCode::Closing as i32, TERMINAL)
    );
    assert!(rx.try_recv().is_err());
}
