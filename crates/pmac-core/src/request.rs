use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use pmac_protocol::Command;
use tokio::sync::Notify;

use crate::budget::AdmissionPermit;
use crate::{ConnectOptions, Error, ErrorCode, ExecutionCertainty, PoolProfile};

std::thread_local! {
    static ORIGIN_LIVENESS: Arc<()> = Arc::new(());
}

#[derive(Clone)]
pub enum Operation {
    Connect(ConnectOptions),
    PoolPrepare {
        options: ConnectOptions,
        profile: PoolProfile,
    },
    PoolResize(PoolProfile),
    PoolRepair {
        member_id: u64,
    },
    PoolDrain {
        member_id: u64,
    },
    RepairMaintenance,
    Command(Command),
    Shell {
        command: String,
        persistent: bool,
    },
    Upload {
        local: String,
        remote: String,
    },
    Download {
        remote: String,
        local: String,
    },
    BufferUpload {
        local: String,
        remote: String,
        cleanup_remote: bool,
    },
    FileExists {
        remote: String,
    },
    FileRemove {
        remote: String,
    },
}

impl Operation {
    pub(crate) fn input_bytes(&self) -> usize {
        match self {
            Self::Connect(options) | Self::PoolPrepare { options, .. } => options.owned_bytes(),
            Self::PoolResize(_)
            | Self::PoolRepair { .. }
            | Self::PoolDrain { .. }
            | Self::RepairMaintenance => 0,
            Self::Command(command) => command.text().len(),
            Self::Shell { command, .. } => command.len(),
            Self::Upload { local, remote }
            | Self::Download { local, remote }
            | Self::BufferUpload { local, remote, .. } => local.len().saturating_add(remote.len()),
            Self::FileExists { remote } | Self::FileRemove { remote } => remote.len(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OperationResult {
    pub response: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: Option<i32>,
}

impl OperationResult {
    pub fn response(response: Vec<u8>) -> Self {
        Self {
            response,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RequestOptions {
    pub timeout_ms: u32,
    pub infinite: bool,
    pub read_pool: bool,
    pub max_response_bytes: usize,
}

impl Default for RequestOptions {
    fn default() -> Self {
        Self {
            timeout_ms: 5_000,
            infinite: false,
            read_pool: false,
            max_response_bytes: 1024 * 1024,
        }
    }
}

impl RequestOptions {
    pub fn deadline(&self) -> Option<tokio::time::Instant> {
        (!self.infinite).then(|| {
            tokio::time::Instant::now() + Duration::from_millis(u64::from(self.timeout_ms.max(1)))
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum RequestState {
    Queued = 0,
    Sending = 1,
    AwaitingResponse = 2,
    Completing = 3,
    Completed = 4,
}

#[derive(Debug, Clone, Copy)]
pub struct RequestInfo {
    pub request_id: u64,
    pub client_id: u64,
    pub state: RequestState,
    pub code: ErrorCode,
    pub certainty: ExecutionCertainty,
    pub connection_epoch: u64,
    pub member_id: u64,
    pub elapsed_us: u64,
}

pub struct RequestSnapshot {
    pub info: RequestInfo,
    pub result: Option<Arc<Result<OperationResult, Error>>>,
}

pub struct Request {
    id: u64,
    client_id: u64,
    options: RequestOptions,
    origin_thread: std::thread::ThreadId,
    origin_liveness: std::sync::Weak<()>,
    created: Instant,
    deadline: Option<tokio::time::Instant>,
    inner: Mutex<RequestInner>,
    completed: Condvar,
    cancellation: Cancellation,
    publish_pending: AtomicBool,
    completion_signal: std::sync::OnceLock<Arc<crate::completion::CompletionSignal>>,
    completion_announced: AtomicBool,
    _permit: AdmissionPermit,
}

struct RequestInner {
    state: RequestState,
    epoch: u64,
    member_id: u64,
    result: Option<Arc<Result<OperationResult, Error>>>,
}

impl std::fmt::Debug for Request {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Request")
            .field("id", &self.id)
            .field("client_id", &self.client_id)
            .finish_non_exhaustive()
    }
}

impl Request {
    pub(crate) fn new(
        id: u64,
        client_id: u64,
        epoch: u64,
        options: RequestOptions,
        permit: AdmissionPermit,
    ) -> Self {
        Self {
            id,
            client_id,
            deadline: options.deadline(),
            options,
            origin_thread: std::thread::current().id(),
            origin_liveness: ORIGIN_LIVENESS.with(Arc::downgrade),
            created: Instant::now(),
            inner: Mutex::new(RequestInner {
                state: RequestState::Queued,
                epoch,
                member_id: 0,
                result: None,
            }),
            completed: Condvar::new(),
            cancellation: Cancellation::default(),
            publish_pending: AtomicBool::new(false),
            completion_signal: std::sync::OnceLock::new(),
            completion_announced: AtomicBool::new(false),
            _permit: permit,
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }
    pub fn client_id(&self) -> u64 {
        self.client_id
    }
    pub(crate) fn origin_thread(&self) -> std::thread::ThreadId {
        self.origin_thread
    }
    pub(crate) fn origin_liveness(&self) -> &std::sync::Weak<()> {
        &self.origin_liveness
    }
    pub fn max_response_bytes(&self) -> usize {
        self.options.max_response_bytes
    }
    pub fn deadline(&self) -> Option<tokio::time::Instant> {
        self.deadline
    }
    pub(crate) fn set_completion_signal(&self, signal: Arc<crate::completion::CompletionSignal>) {
        let _ = self.completion_signal.set(signal);
    }
    fn signal_completion(&self) {
        if !self.completion_announced.swap(true, Ordering::AcqRel) {
            if let Some(signal) = self.completion_signal.get() {
                signal.wake();
            }
        }
    }
    pub(crate) fn mark_publish_started(&self) {
        self.publish_pending.store(true, Ordering::Release);
    }
    pub(crate) fn mark_publish_confirmed(&self) {
        self.publish_pending.store(false, Ordering::Release);
    }
    pub(crate) fn has_unconfirmed_publish(&self) -> bool {
        self.publish_pending.load(Ordering::Acquire)
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
    }
    pub async fn cancelled(&self) {
        self.cancellation.cancelled().await
    }

    pub fn info(&self) -> RequestInfo {
        self.info_from(&self.lock())
    }

    pub fn snapshot(&self) -> RequestSnapshot {
        let inner = self.lock();
        RequestSnapshot {
            info: self.info_from(&inner),
            result: inner.result.clone(),
        }
    }

    pub fn result(&self) -> Option<Arc<Result<OperationResult, Error>>> {
        self.lock().result.clone()
    }

    pub fn cancel(&self) -> ErrorCode {
        self.cancellation.cancel();
        let mut inner = self.lock();
        if inner.state == RequestState::Completed {
            return ErrorCode::Ok;
        }
        if inner.state == RequestState::Queued {
            Self::complete_locked(
                &mut inner,
                Err(Error::new(
                    ErrorCode::Cancelled,
                    "request cancelled before dispatch",
                )),
            );
            self.completed.notify_all();
            self.signal_completion();
        }
        ErrorCode::Ok
    }

    pub fn wait(&self, wait_ms: u32) -> WaitOutcome {
        let mut inner = self.lock();
        if inner.state != RequestState::Completed {
            let timeout = Duration::from_millis(u64::from(wait_ms));
            let (next, _) = self
                .completed
                .wait_timeout_while(inner, timeout, |state| {
                    state.state != RequestState::Completed
                })
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            inner = next;
        }
        WaitOutcome {
            completed: inner.state == RequestState::Completed,
            info: self.info_from(&inner),
        }
    }

    pub(crate) fn begin_send(&self) -> Result<(), Error> {
        let mut inner = self.lock();
        if inner.state == RequestState::Completed {
            return Err(inner
                .result
                .as_ref()
                .and_then(|result| result.as_ref().as_ref().err())
                .cloned()
                .unwrap_or_else(|| Error::new(ErrorCode::Cancelled, "request already completed")));
        }
        if self.is_cancelled() {
            let error = Error::new(ErrorCode::Cancelled, "request cancelled before dispatch");
            Self::complete_locked(&mut inner, Err(error.clone()));
            drop(inner);
            self.completed.notify_all();
            self.signal_completion();
            return Err(error);
        }
        if self
            .deadline
            .is_some_and(|deadline| tokio::time::Instant::now() >= deadline)
        {
            let error = Error::new(
                ErrorCode::QueueTimeout,
                "deadline expired before send arbitration",
            );
            Self::complete_locked(&mut inner, Err(error.clone()));
            self.completed.notify_all();
            self.signal_completion();
            return Err(error);
        }
        inner.state = RequestState::Sending;
        Ok(())
    }

    pub(crate) fn bind_member(&self, member_id: u64, epoch: u64) {
        let mut inner = self.lock();
        inner.member_id = member_id;
        inner.epoch = epoch;
    }

    pub(crate) fn mark_awaiting(&self) {
        let mut inner = self.lock();
        if inner.state != RequestState::Completed {
            inner.state = RequestState::AwaitingResponse;
        }
    }

    pub(crate) fn complete(&self, result: Result<OperationResult, Error>) -> bool {
        let mut inner = self.lock();
        if inner.state == RequestState::Completed {
            return false;
        }
        inner.state = RequestState::Completing;
        Self::complete_locked(&mut inner, result);
        drop(inner);
        self.completed.notify_all();
        self.signal_completion();
        true
    }

    fn complete_locked(inner: &mut RequestInner, result: Result<OperationResult, Error>) {
        inner.result = Some(Arc::new(result));
        inner.state = RequestState::Completed;
    }

    fn info_from(&self, inner: &RequestInner) -> RequestInfo {
        let (code, certainty) = inner.result.as_ref().map_or(
            (
                ErrorCode::Ok,
                if inner.state == RequestState::Queued {
                    ExecutionCertainty::NotSent
                } else {
                    ExecutionCertainty::MayHaveExecuted
                },
            ),
            |result| match result.as_ref() {
                Ok(_) => (ErrorCode::Ok, ExecutionCertainty::Confirmed),
                Err(error) => (error.code, error.certainty),
            },
        );
        RequestInfo {
            request_id: self.id,
            client_id: self.client_id,
            state: inner.state,
            code,
            certainty,
            connection_epoch: inner.epoch,
            member_id: inner.member_id,
            elapsed_us: self.created.elapsed().as_micros().min(u128::from(u64::MAX)) as u64,
        }
    }

    fn lock(&self) -> MutexGuard<'_, RequestInner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct WaitOutcome {
    pub completed: bool,
    pub info: RequestInfo,
}

#[derive(Default)]
pub(crate) struct Cancellation {
    cancelled: AtomicBool,
    notify: Notify,
}

impl Cancellation {
    pub(crate) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.notify.notify_waiters();
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        let notified = self.notify.notified();
        if self.is_cancelled() {
            return;
        }
        notified.await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::Budget;

    fn request() -> Request {
        Request::new(
            1,
            2,
            0,
            RequestOptions::default(),
            Arc::new(Budget::new(1, 1024)).reserve(1).unwrap(),
        )
    }

    #[test]
    fn pending_wait_reports_the_same_send_certainty_as_a_snapshot() {
        let request = request();
        assert_eq!(request.wait(0).info.certainty, ExecutionCertainty::NotSent);
        request.begin_send().unwrap();
        let sending = request.wait(0);
        assert!(!sending.completed);
        assert_eq!(sending.info.certainty, ExecutionCertainty::MayHaveExecuted);
        assert_eq!(sending.info.certainty, request.info().certainty);
        request.mark_awaiting();
        assert_eq!(
            request.wait(0).info.certainty,
            ExecutionCertainty::MayHaveExecuted
        );
    }

    #[test]
    fn queued_cancel_is_terminal_and_cannot_send() {
        let request = request();
        assert_eq!(request.cancel(), ErrorCode::Ok);
        assert!(request.begin_send().is_err());
        assert_eq!(request.info().certainty, ExecutionCertainty::NotSent);
        assert!(!request.complete(Ok(OperationResult::default())));
    }

    #[test]
    fn completion_is_published_once() {
        let request = request();
        request.begin_send().unwrap();
        assert!(request.complete(Ok(OperationResult::response(b"first".to_vec()))));
        assert!(!request.complete(Err(Error::disconnected("late"))));
        assert_eq!(
            request
                .result()
                .unwrap()
                .as_ref()
                .as_ref()
                .unwrap()
                .response,
            b"first"
        );
    }
}
