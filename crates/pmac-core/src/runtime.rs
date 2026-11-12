use crate::{Client, ClientConfig, ClientState, Error, ErrorCode, Transport};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};
use tokio::runtime::Builder;
use tokio::task::JoinHandle;

struct ShutdownState {
    closing: bool,
    runtime_stopped: bool,
    joined: bool,
    joining: bool,
    worker: Option<std::thread::JoinHandle<()>>,
}
pub struct Runtime {
    runtime: Mutex<Option<tokio::runtime::Runtime>>,
    handle: tokio::runtime::Handle,
    next_id: Mutex<u64>,
    completion: Arc<crate::completion::CompletionSignal>,
    clients: Mutex<Vec<Weak<Client>>>,
    shutdown: Mutex<ShutdownState>,
    shutdown_changed: Condvar,
    admission: Arc<Mutex<bool>>,
}
impl Runtime {
    pub fn new(worker_threads: usize) -> Result<Arc<Self>, Error> {
        if worker_threads == 0 || worker_threads > 256 {
            return Err(Error::invalid("runtime worker_threads must be in 1..=256"));
        }
        let runtime = Builder::new_multi_thread()
            .worker_threads(worker_threads)
            .thread_name("pmac-worker")
            .enable_all()
            .build()
            .map_err(|e| Error::internal(format!("failed to create Tokio runtime: {e}")))?;
        let handle = runtime.handle().clone();
        Ok(Arc::new(Self {
            runtime: Mutex::new(Some(runtime)),
            handle,
            next_id: Mutex::new(1),
            completion: Arc::new(crate::completion::CompletionSignal::new()),
            clients: Mutex::new(Vec::new()),
            shutdown: Mutex::new(ShutdownState {
                closing: false,
                runtime_stopped: false,
                joined: false,
                joining: false,
                worker: None,
            }),
            admission: Arc::new(Mutex::new(false)),
            shutdown_changed: Condvar::new(),
        }))
    }
    pub fn create_client(
        self: &Arc<Self>,
        transport: Arc<dyn Transport>,
        config: ClientConfig,
    ) -> Result<Arc<Client>, Error> {
        self.create_client_with_id(self.next_id(), transport, config)
    }
    pub fn create_client_with_id(
        self: &Arc<Self>,
        id: u64,
        transport: Arc<dyn Transport>,
        config: ClientConfig,
    ) -> Result<Arc<Client>, Error> {
        let lifecycle = self.lock_shutdown();
        if lifecycle.closing {
            return Err(Error::new(
                ErrorCode::Closing,
                "runtime shutdown has stopped client admission",
            ));
        }
        if id == 0
            || config.max_requests == 0
            || config.max_retained_bytes == 0
            || config.max_file_tasks == 0
        {
            return Err(Error::invalid(
                "client ID and all resource limits must be nonzero",
            ));
        }
        if config.max_file_tasks > tokio::sync::Semaphore::MAX_PERMITS {
            return Err(Error::invalid(
                "max_file_tasks exceeds the Tokio semaphore limit",
            ));
        }
        static GLOBAL_BUDGET: std::sync::OnceLock<Arc<crate::budget::Budget>> =
            std::sync::OnceLock::new();
        let global = GLOBAL_BUDGET
            .get_or_init(|| Arc::new(crate::budget::Budget::new(16_384, 256 * 1024 * 1024)))
            .clone();
        let client = Client::new(
            id,
            self.handle.clone(),
            transport,
            config,
            global,
            self.completion.clone(),
            self.admission.clone(),
        );
        let mut clients = self.lock_clients();
        clients.retain(|client| client.strong_count() > 0);
        clients.push(Arc::downgrade(&client));
        Ok(client)
    }
    pub fn spawn<F>(&self, future: F) -> JoinHandle<F::Output>
    where
        F: std::future::Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.handle.spawn(future)
    }
    pub fn wait_completion(&self, cursor: &mut u64, wait_ms: u32) -> bool {
        self.completion.wait(cursor, wait_ms)
    }
    pub fn wake(&self) {
        self.completion.wake();
    }
    pub fn next_id(&self) -> u64 {
        let mut next = self
            .next_id
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let id = *next;
        *next = next
            .checked_add(1)
            .expect("non-reusable ID space exhausted");
        id
    }
    pub fn clients(&self) -> Vec<Arc<Client>> {
        let mut clients = self.lock_clients();
        let live = clients.iter().filter_map(Weak::upgrade).collect::<Vec<_>>();
        clients.retain(|c| c.strong_count() > 0);
        live
    }
    /// Starts shutdown once. A dedicated joiner owns the Runtime until every Tokio and
    /// blocking task has stopped. Timeout never abandons it or claims worker exit.
    pub fn shutdown_clients(self: &Arc<Self>, wait_ms: u32) -> Result<(), Error> {
        let started = Instant::now();
        let mut state = self.lock_shutdown();
        if !state.closing {
            state.closing = true;
            *self
                .admission
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = true;

            let owner = self.clone();
            let clients = self.clients();
            let worker = std::thread::Builder::new()
                .name("pmac-shutdown".into())
                .spawn(move || {
                    let runtime = owner
                        .runtime
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .take()
                        .expect("runtime owner missing during shutdown");
                    runtime.block_on(async {
                        let closes = clients.iter().map(|client| async move {
                            loop {
                                if client.close(1000).await.is_ok() {
                                    break;
                                }
                                tokio::task::yield_now().await;
                            }
                        });
                        futures::future::join_all(closes).await;
                    });
                    // Drop waits for blocking I/O and protocol tasks too; it runs outside all Tokio workers.
                    drop(runtime);
                    let mut state = owner.lock_shutdown();
                    state.runtime_stopped = true;
                    drop(state);
                    owner.shutdown_changed.notify_all();
                    owner.wake();
                });
            match worker {
                Ok(worker) => state.worker = Some(worker),
                Err(error) => {
                    state.closing = false;
                    *self
                        .admission
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = false;
                    return Err(Error::internal(format!(
                        "failed to start runtime shutdown joiner: {error}"
                    )));
                }
            }
        }
        loop {
            if state.joined {
                return Ok(());
            }
            if state.runtime_stopped && !state.joining {
                let worker = state
                    .worker
                    .take()
                    .ok_or_else(|| Error::internal("shutdown join handle is missing"))?;
                state.joining = true;
                drop(state);
                worker
                    .join()
                    .map_err(|_| Error::internal("runtime shutdown joiner panicked"))?;
                state = self.lock_shutdown();
                state.joined = true;
                state.joining = false;
                self.shutdown_changed.notify_all();
                return Ok(());
            }
            let remaining =
                Duration::from_millis(u64::from(wait_ms)).saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return Err(Error::new(
                    ErrorCode::ShutdownPending,
                    "runtime workers are still stopping; keep the runtime ID and wait again",
                ));
            }
            let (next, timeout) = self
                .shutdown_changed
                .wait_timeout(state, remaining)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state = next;
            if timeout.timed_out() && !state.joined && !state.runtime_stopped {
                return Err(Error::new(
                    ErrorCode::ShutdownPending,
                    "runtime shutdown wait expired",
                ));
            }
        }
    }
    pub fn is_shutdown(&self) -> bool {
        self.lock_shutdown().joined
    }
    pub fn all_clients_closed(&self) -> bool {
        self.clients()
            .iter()
            .all(|c| c.snapshot().state == ClientState::Closed)
    }
    fn lock_clients(&self) -> MutexGuard<'_, Vec<Weak<Client>>> {
        self.clients
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    fn lock_shutdown(&self) -> MutexGuard<'_, ShutdownState> {
        self.shutdown
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}
