use crate::budget::Budget;
use crate::pool::{JobClass, Lease, MaintenanceBlock, Member, Pool};
use crate::{
    ConnectOptions, Error, ErrorCode, ExecutionCertainty, ExecutionContext, MemberSnapshot,
    MemberState, Operation, OperationResult, PoolProfile, PoolSnapshot, Request, RequestOptions,
    RequestState, Transport,
};
use futures::FutureExt;
use pmac_protocol::{Command, CommandEffect, MaintenanceKind};
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{Duration, SystemTime};
use tokio::sync::{Notify, Semaphore};

#[derive(Debug, Clone, Copy)]
pub struct ClientConfig {
    pub max_requests: usize,
    pub max_retained_bytes: usize,
    pub max_file_tasks: usize,
}
impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            max_requests: 64,
            max_retained_bytes: 80 * 1024 * 1024,
            max_file_tasks: 1,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ClientState {
    Created = 0,
    Connecting = 1,
    Authenticating = 2,
    Initializing = 3,
    Ready = 4,
    Suspect = 5,
    Recovering = 6,
    Disconnected = 7,
    Closing = 8,
    Closed = 9,
}
#[derive(Debug, Clone, Copy)]
pub struct StateSnapshot {
    pub state: ClientState,
    pub connection_epoch: u64,
    pub last_valid_response_unix_ms: u64,
    pub active_requests: usize,
    pub closing: bool,
}
struct MutableState {
    state: ClientState,
    epoch: u64,
    last_valid_response_unix_ms: u64,
    closing: bool,
    next_sequence: u64,
}
#[derive(Clone)]
struct ProgramBuffer {
    member_id: u64,
    epoch: u64,
    closing: Option<Weak<Request>>,
    owner: Weak<()>,
}

pub struct Client {
    id: u64,
    runtime: tokio::runtime::Handle,
    transport: Arc<dyn Transport>,
    budget: Arc<Budget>,
    completion: Arc<crate::completion::CompletionSignal>,
    admission: Arc<Mutex<bool>>,
    state: Mutex<MutableState>,
    pool: Arc<Pool>,
    requests: Mutex<HashMap<u64, Weak<Request>>>,
    program_buffers: Mutex<HashMap<std::thread::ThreadId, ProgramBuffer>>,
    management_lock: tokio::sync::Mutex<()>,
    shell_lock: tokio::sync::Mutex<()>,
    close_started: std::sync::atomic::AtomicBool,
    file_semaphore: Semaphore,
    active_tasks: Mutex<usize>,
    tasks_stopped: Notify,
}
impl Client {
    pub(crate) fn new(
        id: u64,
        runtime: tokio::runtime::Handle,
        transport: Arc<dyn Transport>,
        config: ClientConfig,
        global_budget: Arc<Budget>,
        completion: Arc<crate::completion::CompletionSignal>,
        admission: Arc<Mutex<bool>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            id,
            runtime,
            transport,
            completion,
            admission,
            budget: Arc::new(Budget::with_parent(
                config.max_requests,
                config.max_retained_bytes,
                global_budget,
            )),
            state: Mutex::new(MutableState {
                state: ClientState::Created,
                epoch: 0,
                last_valid_response_unix_ms: 0,
                closing: false,
                next_sequence: 1,
            }),
            pool: Pool::new(),
            requests: Mutex::new(HashMap::new()),
            program_buffers: Mutex::new(HashMap::new()),
            management_lock: tokio::sync::Mutex::new(()),
            shell_lock: tokio::sync::Mutex::new(()),
            file_semaphore: Semaphore::new(config.max_file_tasks.max(1)),
            close_started: std::sync::atomic::AtomicBool::new(false),
            active_tasks: Mutex::new(0),
            tasks_stopped: Notify::new(),
        })
    }
    pub fn id(&self) -> u64 {
        self.id
    }
    /// Parse on the calling thread, using its confirmed GPASCII program-buffer context.
    pub fn parse_command_for_current_thread(
        &self,
        text: impl Into<String>,
    ) -> Result<Command, Error> {
        let text = text.into();
        if self
            .program_buffer_for(std::thread::current().id(), true)?
            .is_some()
        {
            Self::validate_program_line(&text)?;
            Command::program_line(text).map_err(Error::from)
        } else {
            let command = Command::raw(text).map_err(Error::from)?;
            if Self::opens_program_buffer(command.text()) && command.text().contains(';') {
                return Err(Error::invalid(
                    "open must be submitted and confirmed separately before program lines",
                ));
            }
            Ok(command)
        }
    }

    fn program_buffer_for(
        &self,
        thread: std::thread::ThreadId,
        admission: bool,
    ) -> Result<Option<ProgramBuffer>, Error> {
        let binding = self
            .program_buffers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&thread)
            .cloned();
        let Some(binding) = binding else {
            return Ok(None);
        };
        let valid = {
            let pool = self.pool.lock();
            pool.affinity
                .get(&thread)
                .is_some_and(|affinity| affinity.member_id == binding.member_id)
                && pool.members.iter().any(|member| {
                    member.id == binding.member_id
                        && member.epoch == binding.epoch
                        && matches!(member.state, MemberState::Ready | MemberState::Busy)
                        && member
                            .session
                            .as_ref()
                            .is_some_and(|session| session.is_healthy())
                })
        };
        if !valid {
            self.program_buffers
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&thread);
            return Ok(None);
        }
        if admission
            && binding
                .closing
                .as_ref()
                .and_then(Weak::upgrade)
                .is_some_and(|request| {
                    let info = request.info();
                    info.state != RequestState::Completed
                        || info.certainty != ExecutionCertainty::NotSent
                })
        {
            return Err(Error::new(
                ErrorCode::MaintenanceActive,
                "wait for the program close request before submitting more commands",
            ));
        }
        Ok(Some(binding))
    }

    fn validate_program_line(text: &str) -> Result<(), Error> {
        if text.trim().eq_ignore_ascii_case("close") {
            return Ok(());
        }
        let check = |fragment: &str| {
            let lower = fragment.trim_start().to_ascii_lowercase();
            let close_token = lower.strip_prefix("close").is_some_and(|tail| {
                tail.is_empty()
                    || tail.starts_with(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            });
            if close_token {
                return Err(Error::invalid(
                    "close must be submitted separately without trailing program text or comments",
                ));
            }
            if matches!(
                lower.trim(),
                "$$$" | "$$$***" | "reboot" | "save" | "ecat reset" | "ecatreset"
            ) {
                return Err(Error::invalid(
                    "online maintenance commands cannot be sent as program-buffer text",
                ));
            }
            Ok(())
        };
        let bytes = text.as_bytes();
        let mut quote = None;
        let mut escaped = false;
        let mut start = 0;
        let mut index = 0;
        while index < bytes.len() {
            let byte = bytes[index];
            if let Some(delimiter) = quote {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == delimiter {
                    quote = None;
                }
            } else if matches!(byte, b'\'' | b'"') {
                quote = Some(byte);
            } else if byte == b'/' && bytes.get(index + 1) == Some(&b'/') {
                check(&text[start..index])?;
                return Ok(());
            } else if byte == b';' {
                check(&text[start..index])?;
                start = index + 1;
            }
            index += 1;
        }
        check(&text[start..])
    }
    fn opens_program_buffer(text: &str) -> bool {
        let lower = text.trim().to_ascii_lowercase();
        let operation = lower.strip_prefix('&').map_or(lower.as_str(), |tail| {
            tail.trim_start_matches(|c: char| c.is_ascii_digit() || matches!(c, ',' | '.' | '*'))
                .trim_start()
        });
        [
            "open prog",
            "open plc",
            "open subprog",
            "open forward",
            "open inverse",
            "open rotary",
        ]
        .iter()
        .any(|prefix| {
            operation.strip_prefix(prefix).is_some_and(|tail| {
                tail.is_empty()
                    || tail.starts_with(|c: char| {
                        c.is_ascii_whitespace() || c.is_ascii_digit() || c == ','
                    })
            })
        })
    }
    pub fn submit(
        self: &Arc<Self>,
        request_id: u64,
        operation: Operation,
        options: RequestOptions,
    ) -> Result<Arc<Request>, Error> {
        let runtime_admission = self
            .admission
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *runtime_admission {
            return Err(Error::new(
                ErrorCode::Closing,
                "runtime shutdown has stopped request admission",
            ));
        }
        if request_id == 0 || options.max_response_bytes == 0 {
            return Err(Error::invalid(
                "request ID and response limit must be nonzero",
            ));
        }
        let operation = match operation {
            Operation::Command(command) => {
                if self
                    .program_buffer_for(std::thread::current().id(), true)?
                    .is_some()
                {
                    Self::validate_program_line(command.text())?;
                    Operation::Command(Command::program_line(command.text()).map_err(Error::from)?)
                } else if command.is_program_line() {
                    return Err(Error::invalid("program passthrough requires a confirmed open buffer on this calling thread"));
                } else {
                    if Self::opens_program_buffer(command.text()) && command.text().contains(';') {
                        return Err(Error::invalid(
                            "open must be confirmed separately before program text",
                        ));
                    }
                    Operation::Command(command)
                }
            }
            other => other,
        };
        let management = is_management(&operation);
        let (class, writes) = classify(&operation, options.read_pool)?;
        let reserved = operation
            .input_bytes()
            .checked_add(options.max_response_bytes)
            .ok_or_else(|| Error::invalid("request budget overflow"))?;
        let mut state = self.lock_state();
        if state.closing {
            return Err(Error::new(ErrorCode::Closing, "client is closing"));
        }
        let permit = self.budget.reserve(reserved)?;
        let sequence = state.next_sequence;
        state.next_sequence = sequence
            .checked_add(1)
            .ok_or_else(|| Error::new(ErrorCode::ResourceLimit, "admission sequence exhausted"))?;
        let request = Arc::new(Request::new(
            request_id,
            self.id,
            state.epoch,
            options,
            permit,
        ));
        let revision = if management {
            let mut pool = self.pool.lock();
            pool.revision = pool
                .revision
                .checked_add(1)
                .ok_or_else(|| Error::new(ErrorCode::ResourceLimit, "pool revision exhausted"))?;
            pool.revision
        } else {
            self.pool.register(
                sequence,
                class,
                writes,
                request.origin_thread(),
                request.origin_liveness(),
            )?;
            self.pool.lock().revision
        };
        if matches!(&operation, Operation::Command(command) if command.is_program_line() && command.text().trim().eq_ignore_ascii_case("close"))
        {
            if let Some(buffer) = self
                .program_buffers
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get_mut(&request.origin_thread())
            {
                buffer.closing = Some(Arc::downgrade(&request));
            }
        }
        request.set_completion_signal(self.completion.clone());
        self.lock_requests()
            .insert(request_id, Arc::downgrade(&request));
        *self.lock_active_tasks() += 1;
        drop(state);
        let client = self.clone();
        let task_request = request.clone();
        self.runtime.spawn(async move {
            let _active = ActiveTask {
                client: &client,
                sequence,
            };
            let outcome = std::panic::AssertUnwindSafe(client.execute(
                task_request.clone(),
                operation,
                sequence,
                revision,
                class,
                writes,
            ))
            .catch_unwind()
            .await;
            if outcome.is_err() {
                task_request.complete(Err(Error::internal("operation task panicked")
                    .sent()
                    .fatal()));
            }
        });
        Ok(request)
    }
    pub fn snapshot(&self) -> StateSnapshot {
        let state = self.lock_state();
        let pool = self.pool.snapshot();
        let min_ready = self.pool.lock().profile.min_ready;
        let value = if matches!(state.state, ClientState::Closing | ClientState::Closed) {
            state.state
        } else if pool.ready + pool.busy >= min_ready {
            ClientState::Ready
        } else if matches!(state.state, ClientState::Created | ClientState::Connecting) {
            state.state
        } else {
            ClientState::Disconnected
        };
        StateSnapshot {
            state: value,
            connection_epoch: state.epoch,
            last_valid_response_unix_ms: state.last_valid_response_unix_ms,
            active_requests: self.lock_requests().len(),
            closing: state.closing,
        }
    }
    pub fn is_connected(&self) -> bool {
        self.snapshot().state == ClientState::Ready
    }
    pub fn pool_snapshot(&self) -> PoolSnapshot {
        self.pool.snapshot()
    }
    pub fn pool_members(&self) -> Vec<MemberSnapshot> {
        self.pool.members()
    }
    pub fn has_unreleased_gpascii(&self) -> bool {
        let pool = self.pool.lock();
        !pool.uncertain.is_empty()
            || pool.options.as_ref().is_some_and(|options| {
                options.gpascii_budget.owner_usage(options.gpascii_owner) != 0
            })
    }
    pub fn recovery_pending(&self) -> bool {
        self.pool.lock().closing && self.has_unreleased_gpascii()
    }
    /// Application attests it has reconciled unknown effects and/or verified remote sessions are gone.
    pub fn confirm_recovery(&self, expected_revision: u64, flags: u32) -> Result<(), Error> {
        if flags == 0 || flags & !3 != 0 {
            return Err(Error::invalid("unknown recovery acknowledgement flags"));
        }
        let mut s = self.pool.lock();
        if s.closing && flags & 2 == 0 {
            return Err(Error::new(ErrorCode::Closing, "pool is closing"));
        }
        if s.revision != expected_revision {
            return Err(Error::new(
                ErrorCode::Superseded,
                "pool revision changed before recovery acknowledgement",
            ));
        }
        if s.running != 0 || s.creating != 0 {
            return Err(Error::new(
                ErrorCode::MaintenanceActive,
                "recovery acknowledgement requires quiescent operations",
            ));
        }
        if flags & 1 != 0 {
            s.unknown_write = false;
        }
        if flags & 2 != 0 {
            if let Some(options) = &s.options {
                options
                    .gpascii_budget
                    .confirm_remote_release(options.gpascii_owner);
            }
            s.uncertain.clear();
        }
        drop(s);
        self.pool.changed.notify_waiters();
        Ok(())
    }
    pub async fn close(self: &Arc<Self>, wait_ms: u32) -> Result<(), Error> {
        {
            let mut state = self.lock_state();
            if state.state == ClientState::Closed {
                return Ok(());
            }
            state.closing = true;
            state.state = ClientState::Closing;
        }
        {
            let mut pool = self.pool.lock();
            pool.closing = true;
            pool.revision = pool.revision.saturating_add(1);
        }
        for request in self.lock_requests().values().filter_map(Weak::upgrade) {
            request.cancel();
        }
        self.pool.changed.notify_waiters();
        if !self
            .close_started
            .swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            let client = self.clone();
            self.runtime.spawn(async move {
                loop {
                    let notified = client.tasks_stopped.notified();
                    if *client.lock_active_tasks() == 0 {
                        break;
                    }
                    notified.await;
                }
                let sessions = client
                    .pool
                    .lock()
                    .members
                    .iter()
                    .filter_map(|m| m.session.clone().map(|session| (m.id, m.epoch, session)))
                    .collect::<Vec<_>>();
                loop {
                    let results = futures::future::join_all(
                        sessions.iter().map(|(_, _, session)| session.close()),
                    )
                    .await;
                    {
                        let mut pool = client.pool.lock();
                        for ((id, epoch, _), result) in sessions.iter().zip(&results) {
                            if result
                                .as_ref()
                                .is_err_and(|error| error.code == ErrorCode::ShutdownPending)
                                && !pool.uncertain.contains(&(*id, *epoch))
                            {
                                pool.uncertain.push((*id, *epoch));
                            }
                        }
                    }
                    if results.iter().all(Result::is_ok) {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                {
                    let mut pool = client.pool.lock();
                    for member in &mut pool.members {
                        member.session = None;
                        member.state = MemberState::Drained;
                    }
                }
                {
                    let mut state = client.lock_state();
                    state.state = ClientState::Closed;
                    state.epoch = state.epoch.saturating_add(1);
                }
                client.tasks_stopped.notify_waiters();
            });
        }
        let wait = async {
            loop {
                let notified = self.tasks_stopped.notified();
                if self.lock_state().state == ClientState::Closed {
                    return;
                }
                notified.await;
            }
        };
        tokio::time::timeout(Duration::from_millis(u64::from(wait_ms)), wait)
            .await
            .map_err(|_| {
                Error::new(
                    ErrorCode::ShutdownPending,
                    "client transport/tasks are still stopping",
                )
            })
    }
    async fn execute(
        &self,
        request: Arc<Request>,
        operation: Operation,
        sequence: u64,
        revision: u64,
        class: JobClass,
        writes: bool,
    ) {
        let result = if is_management(&operation) {
            self.manage(&request, operation, revision).await
        } else {
            self.execute_data(&request, operation, sequence, class, writes)
                .await
        };
        request.complete(result);
    }
    async fn execute_data(
        &self,
        request: &Arc<Request>,
        operation: Operation,
        sequence: u64,
        class: JobClass,
        writes: bool,
    ) -> Result<OperationResult, Error> {
        let lease = self
            .drive(request, async {
                loop {
                    let changed = self.pool.changed.notified();
                    if let Some(lease) = self.pool.try_acquire(sequence, writes)? {
                        break Ok::<Lease, Error>(lease);
                    }
                    changed.await;
                }
            })
            .await??;
        request.bind_member(lease.member_id, lease.epoch);
        let shell_guard = if matches!(class, JobClass::Shell) {
            Some(self.drive(request, self.shell_lock.lock()).await?)
        } else {
            None
        };
        let file_guard = if matches!(
            operation,
            Operation::Upload { .. }
                | Operation::Download { .. }
                | Operation::FileExists { .. }
                | Operation::FileRemove { .. }
                | Operation::BufferUpload { .. }
        ) {
            Some(
                self.drive(request, self.file_semaphore.acquire())
                    .await?
                    .map_err(|_| Error::internal("file semaphore closed"))?,
            )
        } else {
            None
        };
        let program_action = match &operation {
            Operation::Command(command) => Some((
                command.is_program_line(),
                Self::opens_program_buffer(command.text()),
                command.text().trim().eq_ignore_ascii_case("close"),
                !command.is_program_line()
                    && command
                        .text()
                        .trim()
                        .eq_ignore_ascii_case("close all buffers"),
            )),
            _ => None,
        };
        if program_action.is_some_and(|action| action.0) {
            let buffer = self.program_buffer_for(request.origin_thread(), false)?;
            if !buffer.is_some_and(|buffer| {
                buffer.member_id == lease.member_id && buffer.epoch == lease.epoch
            }) {
                return Err(Error::invalid(
                    "program buffer context changed before dispatch",
                ));
            }
        }
        request.begin_send()?;
        request.mark_awaiting();
        let context = ExecutionContext::new(request.clone());
        let reset = matches!(&operation,Operation::Command(c) if matches!(c.maintenance(),MaintenanceKind::Reset|MaintenanceKind::GlobalReset));
        let control = matches!(
            &operation,
            Operation::Command(_)
                | Operation::DartConnect
                | Operation::DartDisconnect
                | Operation::Dart(_)
        );
        let dart = matches!(
            &operation,
            Operation::DartConnect | Operation::DartDisconnect | Operation::Dart(_)
        );
        let dart_transfer = matches!(&operation, Operation::Dart(_));
        let dart_connect = matches!(&operation, Operation::DartConnect);
        let operation_future = async {
            match operation {
                Operation::Command(command) => lease.session.command(&command, &context).await,
                Operation::DartConnect => lease.session.dart_connect(&context).await,
                Operation::DartDisconnect => lease.session.dart_disconnect(&context).await,
                Operation::Dart(transfer) => lease.session.dart(transfer, &context).await,
                Operation::Shell {
                    command,
                    persistent,
                } => lease.session.shell(&command, persistent, &context).await,
                Operation::Upload { local, remote } => {
                    lease.session.upload(&local, &remote, &context).await
                }
                Operation::Download { remote, local } => {
                    lease.session.download(&remote, &local, &context).await
                }
                Operation::FileExists { remote } => {
                    lease.session.file_exists(&remote, &context).await
                }
                Operation::FileRemove { remote } => {
                    lease.session.file_remove(&remote, &context).await
                }
                Operation::BufferUpload {
                    local,
                    remote,
                    cleanup_remote,
                } => {
                    lease.session.upload(&local, &remote, &context).await?;
                    let command = format!("gpascii -2 -i{}", shell_quote(&remote)?);
                    let loaded = lease.session.shell(&command, false, &context).await?;
                    if loaded.exit_code != Some(0)
                        || !String::from_utf8_lossy(&loaded.response)
                            .lines()
                            .any(|l| l.trim() == "EOF")
                    {
                        return Err(Error::new(
                            ErrorCode::RemoteError,
                            "buffer load did not return EOF with exit zero",
                        )
                        .sent());
                    }
                    if cleanup_remote {
                        lease.session.file_remove(&remote, &context).await?;
                    }
                    Ok(loaded)
                }
                _ => Err(Error::internal("management operation in data dispatcher")),
            }
        };
        let outcome = std::panic::AssertUnwindSafe(self.drive(request, operation_future))
            .catch_unwind()
            .await;
        let mut result = match outcome {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => Err(e),
            Err(_) => Err(Error::internal("transport operation panicked")
                .sent()
                .fatal()),
        };
        if let Some((program, opens, closes, closes_all)) = program_action {
            if result.is_ok() {
                let mut buffers = self
                    .program_buffers
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if closes_all {
                    buffers.clear();
                } else if program && closes {
                    buffers.remove(&request.origin_thread());
                } else if !program && opens {
                    buffers.retain(|_, buffer| buffer.owner.strong_count() != 0);
                    buffers.insert(
                        request.origin_thread(),
                        ProgramBuffer {
                            member_id: lease.member_id,
                            epoch: lease.epoch,
                            closing: None,
                            owner: request.origin_liveness(),
                        },
                    );
                }
            } else if closes_all {
                self.program_buffers
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clear();
            } else if program {
                self.program_buffers
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .remove(&request.origin_thread());
                // A compiler/protocol error may have ended the remote buffer. Retire the channel
                // rather than risk sending a subsequent program line as an online command.
                if let Err(error) = &mut result {
                    error.fatal_transport = true;
                }
            }
        }
        if dart_connect && context.independent_tcp_started() {
            if let Err(error) = &mut result {
                if matches!(error.code, ErrorCode::Cancelled | ErrorCode::ExecTimeout) {
                    error.fatal_transport = false;
                }
            }
        }
        if request.has_unconfirmed_publish() {
            if let Err(error) = &result {
                result = Err(Error::new(
                    ErrorCode::PublishUnknown,
                    format!("publish result is unknown: {}", error.message),
                )
                .sent()
                .fatal());
            }
        }
        if let Ok(value) = &result {
            if value.response.len().saturating_add(value.stderr.len())
                > request.max_response_bytes()
            {
                let mut error = Error::new(
                    ErrorCode::ResourceLimit,
                    "completed response exceeds reserved byte budget",
                );
                error.certainty = ExecutionCertainty::Confirmed;
                result = Err(error);
            }
        }
        let uncertain = result
            .as_ref()
            .err()
            .is_some_and(|e| e.certainty == ExecutionCertainty::MayHaveExecuted);
        if writes && uncertain {
            self.pool.lock().unknown_write = true;
        }
        let broken = result.as_ref().err().is_some_and(|e| {
            !((dart_transfer || (dart_connect && context.independent_tcp_started()))
                && matches!(e.code, ErrorCode::Cancelled | ErrorCode::ExecTimeout))
                && (!dart || e.fatal_transport)
                && (e.fatal_transport
                    || (uncertain
                        && matches!(
                            e.code,
                            ErrorCode::Cancelled
                                | ErrorCode::ExecTimeout
                                | ErrorCode::Internal
                                | ErrorCode::Disconnected
                        )))
        });
        if reset || (program_action.is_some_and(|action| action.3) && result.is_err()) {
            self.retire_device().await;
        } else if broken {
            if control {
                self.fail_member(
                    lease.member_id,
                    lease.epoch,
                    result
                        .as_ref()
                        .err()
                        .map_or(ErrorCode::Disconnected, |e| e.code),
                )
                .await;
            } else {
                self.pool.lock().maintenance_failed = true;
                let _ = lease.session.retire_maintenance().await;
            }
        } else if control
            && (result.is_ok()
                || result.as_ref().err().is_some_and(|error| {
                    error.certainty == ExecutionCertainty::Confirmed && !error.fatal_transport
                }))
        {
            let now = unix_ms();
            if let Some(member) = self
                .pool
                .lock()
                .members
                .iter_mut()
                .find(|m| m.id == lease.member_id && m.epoch == lease.epoch)
            {
                member.last_response = now;
            }
            self.lock_state().last_valid_response_unix_ms = now;
        }
        drop(file_guard);
        drop(shell_guard);
        drop(lease);
        result
    }
    async fn manage(
        &self,
        request: &Arc<Request>,
        operation: Operation,
        revision: u64,
    ) -> Result<OperationResult, Error> {
        let _guard = self.drive(request, self.management_lock.lock()).await?;
        self.wait_management_allowed(request).await?;
        self.check_revision(revision)?;
        let result = match operation {
            Operation::Connect(options) => {
                let profile = PoolProfile {
                    target_size: u32::from(options.control_pool_size),
                    max_members: u32::from(options.control_pool_size)
                        .saturating_add(2)
                        .clamp(4, 255),
                    ..PoolProfile::default()
                };
                self.prepare(request, options, profile, revision).await
            }
            Operation::PoolPrepare { options, profile } => {
                self.prepare(request, options, profile, revision).await
            }
            Operation::PoolResize(profile) => {
                profile.validate()?;
                {
                    let s = self.pool.lock();
                    if s.options.is_none() {
                        return Err(Error::invalid("prepare the pool before resizing"));
                    }
                    if profile.max_members
                        < s.uncertain.len() as u32
                            + s.members
                                .iter()
                                .filter(|m| {
                                    matches!(
                                        m.state,
                                        MemberState::Ready
                                            | MemberState::Busy
                                            | MemberState::Retiring
                                    )
                                })
                                .count() as u32
                    {
                        return Err(Error::new(
                            ErrorCode::ResourceLimit,
                            "new quota is below occupied plus uncertain members",
                        ));
                    }
                }
                self.pool.set_profile(profile)?;
                self.resize(request, revision).await
            }
            Operation::PoolRepair { member_id } => {
                let desired = self.pool_snapshot().desired;
                let ids = self
                    .pool
                    .lock()
                    .members
                    .iter()
                    .filter(|m| {
                        (member_id == 0 || m.id == member_id)
                            && matches!(m.state, MemberState::Failed | MemberState::Drained)
                            && m.id <= u64::from(desired)
                    })
                    .map(|m| m.id)
                    .collect::<Vec<_>>();
                if ids.is_empty() {
                    return Err(Error::invalid("no selected failed member requires repair"));
                }
                for id in ids {
                    self.create_member(request, id, revision).await?;
                }
                Ok(OperationResult::default())
            }
            Operation::PoolDrain { member_id } => {
                if member_id == 1 {
                    return Err(Error::invalid(
                        "the ordered control member can only be retired by close or repair",
                    ));
                }
                let ids = self
                    .pool
                    .lock()
                    .members
                    .iter()
                    .filter(|m| member_id == 0 && m.id > 1 || m.id == member_id)
                    .map(|m| m.id)
                    .collect::<Vec<_>>();
                if ids.is_empty() {
                    return Err(Error::invalid("selected member does not exist"));
                }
                for id in ids {
                    self.drain_member(request, id, revision).await?;
                }
                Ok(OperationResult::default())
            }
            Operation::RepairMaintenance => {
                let _block = MaintenanceBlock::new(self.pool.clone());
                self.wait_maintenance_idle(request).await?;
                let session = self
                    .pool
                    .lock()
                    .members
                    .first()
                    .and_then(|m| m.session.clone())
                    .ok_or_else(|| Error::new(ErrorCode::NoReadyLane, "no prepared session"))?;
                request.begin_send()?;
                request.mark_awaiting();
                self.drive(
                    request,
                    session.repair_maintenance(&ExecutionContext::new(request.clone())),
                )
                .await??;
                self.check_revision(revision)?;
                self.pool.lock().maintenance_failed = false;
                Ok(OperationResult::default())
            }
            _ => Err(Error::internal("unexpected management operation")),
        };
        self.refresh_state();
        result
    }
    async fn prepare(
        &self,
        request: &Arc<Request>,
        mut options: ConnectOptions,
        profile: PoolProfile,
        revision: u64,
    ) -> Result<OperationResult, Error> {
        profile.validate()?;
        {
            let mut s = self.pool.lock();
            if s.options.is_some() {
                return Err(Error::invalid(
                    "pool is already prepared; use resize or explicit repair",
                ));
            }
            options.control_pool_size = 1;
            options.gpascii_budget =
                crate::transport::GpasciiBudget::shared(&options.host, options.port);
            options.gpascii_owner = crate::transport::GpasciiBudget::allocate_owner()?;
            s.options = Some(options);
        }
        self.pool.set_profile(profile)?;
        self.lock_state().state = ClientState::Connecting;
        let result = self.resize(request, revision).await;
        self.refresh_state();
        result
    }
    async fn resize(
        &self,
        request: &Arc<Request>,
        revision: u64,
    ) -> Result<OperationResult, Error> {
        let target = self.pool.lock().profile.target_size;
        let drains = self
            .pool
            .lock()
            .members
            .iter()
            .filter(|m| {
                m.id > u64::from(target)
                    && !matches!(m.state, MemberState::Drained | MemberState::Failed)
            })
            .map(|m| m.id)
            .collect::<Vec<_>>();
        for id in drains {
            self.drain_member(request, id, revision).await?;
        }
        for id in 1..=u64::from(target) {
            self.check_revision(revision)?;
            let create = {
                let s = self.pool.lock();
                s.members
                    .iter()
                    .find(|m| m.id == id)
                    .is_none_or(|m| m.state == MemberState::Drained)
            };
            if create {
                self.create_member(request, id, revision).await?;
            }
        }
        let snapshot = self.pool.snapshot();
        if snapshot.ready + snapshot.busy < self.pool.lock().profile.min_ready {
            return Err(Error::new(
                ErrorCode::NoReadyLane,
                "prepared capacity is below min_ready",
            ));
        }
        Ok(OperationResult::response(
            format!(
                "revision={revision};ready={};desired={}",
                snapshot.ready, snapshot.desired
            )
            .into_bytes(),
        ))
    }
    async fn create_member(
        &self,
        request: &Arc<Request>,
        id: u64,
        revision: u64,
    ) -> Result<(), Error> {
        self.check_revision(revision)?;
        request.reset_gpascii_resource_attempted();
        request.begin_send()?;
        request.mark_awaiting();
        let _block = if id == 1 {
            Some(MaintenanceBlock::new(self.pool.clone()))
        } else {
            None
        };
        if id == 1 {
            self.wait_maintenance_idle(request).await?;
        }
        self.wait_management_allowed(request).await?;
        self.check_revision(revision)?;
        let (options, epoch, old, connect_budget) = {
            let mut s = self.pool.lock();
            let occupied = s
                .members
                .iter()
                .filter(|m| {
                    matches!(
                        m.state,
                        MemberState::Ready | MemberState::Busy | MemberState::Retiring
                    )
                })
                .count();
            if occupied + s.uncertain.len() + s.creating as usize >= s.profile.max_members as usize
            {
                return Err(Error::new(ErrorCode::ResourceLimit,"pool quota includes remote release-uncertain members; confirm remote release before repair"));
            }
            let options = s
                .options
                .clone()
                .ok_or_else(|| Error::invalid("pool has no connection configuration"))?;
            let (epoch, old) = if let Some(member) = s.members.iter_mut().find(|m| m.id == id) {
                if member.state == MemberState::Busy {
                    return Err(Error::new(
                        ErrorCode::MaintenanceActive,
                        "selected member is busy",
                    ));
                }
                member.state = MemberState::Creating;
                (
                    member.epoch.checked_add(1).ok_or_else(|| {
                        Error::new(ErrorCode::ResourceLimit, "member epoch exhausted")
                    })?,
                    member.session.take(),
                )
            } else {
                s.members.push(Member {
                    id,
                    epoch: 0,
                    state: MemberState::Creating,
                    session: None,
                    last_response: 0,
                    drain: false,
                    last_error: ErrorCode::Ok,
                });
                (1, None)
            };
            s.creating += 1;
            (options, epoch, old, s.profile.connect_timeout_ms)
        };
        if let Some(old) = old {
            let closed = self.drive(request, old.close()).await;
            if !matches!(closed, Ok(Ok(()))) {
                let error = match closed {
                    Err(e) | Ok(Err(e)) => e,
                    _ => unreachable!(),
                };
                let mut s = self.pool.lock();
                s.creating -= 1;
                if let Some(m) = s.members.iter_mut().find(|m| m.id == id) {
                    m.session = Some(old);
                    m.state = MemberState::Failed;
                    m.last_error = error.code;
                }
                return Err(error);
            }
        }
        let context = ExecutionContext::new(request.clone());
        let attempted = std::panic::AssertUnwindSafe(self.drive(
            request,
            tokio::time::timeout(
                Duration::from_millis(u64::from(connect_budget)),
                self.transport.connect(&options, &context),
            ),
        ))
        .catch_unwind()
        .await;
        let attempt = match attempted {
            Ok(value) => value,
            Err(_) => Err(Error::internal("connection task panicked").sent().fatal()),
        };
        let result = match attempt {
            Ok(Ok(value)) => value,
            Ok(Err(_)) => Err(Error::new(
                ErrorCode::ConnectError,
                "pool member initialization budget expired",
            )
            .sent()),
            Err(e) => Err(e),
        };
        self.pool.lock().creating -= 1;
        self.pool.changed.notify_waiters();
        match result {
            Ok(session) => {
                if let Err(error) = self.check_revision(revision) {
                    let _ = session.close().await;
                    let mut s = self.pool.lock();
                    s.uncertain.push((id, epoch));
                    if let Some(m) = s.members.iter_mut().find(|m| m.id == id) {
                        m.state = MemberState::Failed;
                        m.epoch = epoch;
                        m.last_error = error.code;
                    }
                    return Err(error);
                }
                {
                    let mut s = self.pool.lock();
                    let m = s
                        .members
                        .iter_mut()
                        .find(|m| m.id == id)
                        .ok_or_else(|| Error::internal("creating member disappeared"))?;
                    m.session = Some(session);
                    m.epoch = epoch;
                    m.state = MemberState::Ready;
                    m.drain = false;
                    m.last_response = unix_ms();
                    m.last_error = ErrorCode::Ok;
                    s.last_create_error = ErrorCode::Ok;
                }
                let mut state = self.lock_state();
                state.epoch = state.epoch.saturating_add(1);
                state.last_valid_response_unix_ms = unix_ms();
            }
            Err(error) => {
                let mut s = self.pool.lock();
                s.last_create_error = error.code;
                if request.gpascii_resource_attempted() && !s.uncertain.contains(&(id, epoch)) {
                    s.uncertain.push((id, epoch));
                }
                if let Some(m) = s.members.iter_mut().find(|m| m.id == id) {
                    m.epoch = epoch;
                    m.state = MemberState::Failed;
                    m.last_error = error.code;
                }
                return Err(error);
            }
        }
        self.pool.changed.notify_waiters();
        Ok(())
    }
    async fn wait_management_allowed(&self, request: &Arc<Request>) -> Result<(), Error> {
        self.drive(request, async {
            loop {
                let changed = self.pool.changed.notified();
                if !self.pool.maintenance_pending() {
                    break;
                }
                changed.await;
            }
        })
        .await
    }
    async fn wait_maintenance_idle(&self, request: &Arc<Request>) -> Result<(), Error> {
        self.drive(request, async {
            loop {
                let changed = self.pool.changed.notified();
                if self.pool.lock().maintenance_running == 0 {
                    break;
                }
                changed.await;
            }
        })
        .await
    }
    async fn drain_member(
        &self,
        request: &Arc<Request>,
        id: u64,
        revision: u64,
    ) -> Result<(), Error> {
        {
            let mut s = self.pool.lock();
            let m = s
                .members
                .iter_mut()
                .find(|m| m.id == id)
                .ok_or_else(|| Error::invalid("member does not exist"))?;
            m.drain = true;
        }
        self.pool.changed.notify_waiters();
        let waiting = self
            .drive(request, async {
                loop {
                    let changed = self.pool.changed.notified();
                    self.check_revision(revision)?;
                    if !self
                        .pool
                        .lock()
                        .members
                        .iter()
                        .any(|m| m.id == id && m.state == MemberState::Busy)
                    {
                        break Ok::<(), Error>(());
                    }
                    changed.await;
                }
            })
            .await
            .and_then(|result| result);
        if let Err(error) = waiting {
            let mut s = self.pool.lock();
            if let Some(m) = s.members.iter_mut().find(|m| m.id == id) {
                m.drain = false;
            }
            drop(s);
            self.pool.changed.notify_waiters();
            return Err(error);
        }
        if let Err(error) = request.begin_send() {
            let mut s = self.pool.lock();
            if let Some(m) = s.members.iter_mut().find(|m| m.id == id) {
                m.drain = false;
            }
            drop(s);
            self.pool.changed.notify_waiters();
            return Err(error);
        }
        request.mark_awaiting();
        let session = {
            let mut s = self.pool.lock();
            let m = s
                .members
                .iter_mut()
                .find(|m| m.id == id)
                .ok_or_else(|| Error::invalid("member does not exist"))?;
            m.state = MemberState::Retiring;
            m.session.clone()
        };
        let closed = match session {
            Some(session) => self
                .drive(request, session.close())
                .await
                .and_then(|result| result),
            None => Ok(()),
        };
        let mut s = self.pool.lock();
        if let Some(m) = s.members.iter_mut().find(|m| m.id == id) {
            let epoch = m.epoch;
            m.state = if closed.is_ok() {
                MemberState::Drained
            } else {
                MemberState::Failed
            };
            if closed.is_ok() {
                m.session = None;
            } else if !s.uncertain.contains(&(id, epoch)) {
                s.uncertain.push((id, epoch));
            }
        }
        s.affinity.retain(|_, bound| bound.member_id != id);
        drop(s);
        self.pool.changed.notify_waiters();
        closed
    }
    async fn fail_member(&self, id: u64, epoch: u64, code: ErrorCode) {
        let session = {
            let mut s = self.pool.lock();
            s.members
                .iter_mut()
                .find(|m| m.id == id && m.epoch == epoch)
                .and_then(|m| {
                    m.state = MemberState::Retiring;
                    m.last_error = code;
                    m.session.clone()
                })
        };
        let confirmed = match session {
            Some(session) => session.retire_control().await.is_ok(),
            None => false,
        };
        {
            let mut s = self.pool.lock();
            if let Some(m) = s
                .members
                .iter_mut()
                .find(|m| m.id == id && m.epoch == epoch)
            {
                m.state = MemberState::Failed;
            }
            if !confirmed && !s.uncertain.contains(&(id, epoch)) {
                s.uncertain.push((id, epoch));
            }
        }
        self.pool.changed.notify_waiters();
        self.refresh_state();
    }
    async fn retire_device(&self) {
        let sessions = {
            let mut s = self.pool.lock();
            s.generation = s.generation.saturating_add(1);
            s.revision = s.revision.saturating_add(1);
            s.members
                .iter_mut()
                .filter_map(|m| {
                    m.state = MemberState::Retiring;
                    m.session.clone().map(|v| (m.id, m.epoch, v))
                })
                .collect::<Vec<_>>()
        };
        for (id, epoch, session) in sessions {
            let _ = session.retire_control().await;
            let mut s = self.pool.lock();
            if let Some(m) = s
                .members
                .iter_mut()
                .find(|m| m.id == id && m.epoch == epoch)
            {
                m.state = MemberState::Failed;
            }
            if !s.uncertain.contains(&(id, epoch)) {
                s.uncertain.push((id, epoch));
            }
        }
        self.pool.changed.notify_waiters();
        self.refresh_state();
    }
    fn check_revision(&self, revision: u64) -> Result<(), Error> {
        let s = self.pool.lock();
        if s.closing {
            return Err(Error::new(
                ErrorCode::Closing,
                "pool closed during management operation",
            ));
        }
        if s.revision != revision {
            return Err(Error::new(
                ErrorCode::Superseded,
                "management operation superseded by a newer pool revision",
            ));
        }
        Ok(())
    }
    fn refresh_state(&self) {
        let ready = {
            let s = self.pool.lock();
            s.members.iter().any(|m| {
                matches!(m.state, MemberState::Ready | MemberState::Busy)
                    && m.session.as_ref().is_some_and(|v| v.is_healthy())
            })
        };
        let mut state = self.lock_state();
        if !state.closing {
            state.state = if ready {
                ClientState::Ready
            } else {
                ClientState::Disconnected
            };
        }
    }
    async fn drive<F: Future>(
        &self,
        request: &Arc<Request>,
        future: F,
    ) -> Result<F::Output, Error> {
        if let Some(deadline) = request.deadline() {
            tokio::select! { biased;
                _=request.cancelled()=>Err(self.cancel_error(request)),
                _=tokio::time::sleep_until(deadline)=>Err(self.timeout_error(request)),
                result=future=>Ok(result),
            }
        } else {
            tokio::select! {biased; _=request.cancelled()=>Err(self.cancel_error(request)),result=future=>Ok(result)}
        }
    }
    fn cancel_error(&self, request: &Request) -> Error {
        let error = Error::new(ErrorCode::Cancelled, "request cancelled");
        if request.info().state == RequestState::Queued || request.control_channel_initializing() {
            error
        } else {
            error.sent().fatal()
        }
    }
    fn timeout_error(&self, request: &Request) -> Error {
        if request.info().state == RequestState::Queued {
            Error::new(ErrorCode::QueueTimeout, "queue deadline expired")
        } else if request.control_channel_initializing() {
            Error::new(ErrorCode::ExecTimeout, "execution deadline expired")
        } else {
            Error::new(ErrorCode::ExecTimeout, "execution deadline expired")
                .sent()
                .fatal()
        }
    }
    fn lock_state(&self) -> MutexGuard<'_, MutableState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    fn lock_requests(&self) -> MutexGuard<'_, HashMap<u64, Weak<Request>>> {
        self.requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    fn lock_active_tasks(&self) -> MutexGuard<'_, usize> {
        self.active_tasks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}
struct ActiveTask<'a> {
    client: &'a Client,
    sequence: u64,
}
impl Drop for ActiveTask<'_> {
    fn drop(&mut self) {
        self.client.pool.unregister(self.sequence);
        self.client.lock_requests().retain(|_, r| {
            r.upgrade()
                .is_some_and(|r| r.info().state != RequestState::Completed)
        });
        let mut active = self.client.lock_active_tasks();
        *active = active.saturating_sub(1);
        drop(active);
        self.client.tasks_stopped.notify_waiters();
    }
}
fn is_management(operation: &Operation) -> bool {
    matches!(
        operation,
        Operation::Connect(_)
            | Operation::PoolPrepare { .. }
            | Operation::PoolResize(_)
            | Operation::PoolRepair { .. }
            | Operation::PoolDrain { .. }
            | Operation::RepairMaintenance
    )
}
fn classify(operation: &Operation, read_pool: bool) -> Result<(JobClass, bool), Error> {
    match operation {
        Operation::Command(command) => {
            let writes = command.effect() != CommandEffect::ReadOnly;
            if read_pool && writes {
                return Err(Error::invalid(
                    "read-pool routing accepts only verified read-only commands",
                ));
            }
            Ok((
                if command.effect() == CommandEffect::Maintenance {
                    JobClass::Maintenance
                } else if read_pool {
                    JobClass::ReadPool
                } else {
                    JobClass::Ordered
                },
                writes,
            ))
        }
        Operation::DartConnect | Operation::DartDisconnect => Ok((JobClass::Ordered, false)),
        Operation::Dart(transfer) => Ok((JobClass::Ordered, transfer.command().is_write())),
        Operation::BufferUpload { .. } => Ok((JobClass::Maintenance, true)),
        Operation::Shell { .. } => Ok((JobClass::Shell, true)),
        Operation::Upload { .. } | Operation::FileRemove { .. } => Ok((JobClass::File, true)),
        _ => Ok((JobClass::File, false)),
    }
}
fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}
fn shell_quote(path: &str) -> Result<String, Error> {
    if path.is_empty() || path.as_bytes().contains(&0) || path.contains(['\r', '\n']) {
        return Err(Error::invalid("invalid shell path"));
    }
    Ok(format!("'{}'", path.replace('\'', "'\\''")))
}
