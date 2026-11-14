use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use pmac_core::{
    Client, ClientConfig, ConnectOptions, Error, ErrorCode, ExecutionCertainty, ExecutionContext,
    MemberState, Operation, OperationResult, PoolProfile, Request, RequestOptions, Runtime,
    Session, Transport,
};
use pmac_protocol::Command;
use tokio::sync::watch;

struct Gate {
    blocked: watch::Sender<bool>,
    entered: Mutex<usize>,
    changed: Condvar,
}
impl Default for Gate {
    fn default() -> Self {
        Self {
            blocked: watch::channel(false).0,
            entered: Mutex::new(0),
            changed: Condvar::new(),
        }
    }
}
impl Gate {
    fn block(&self) {
        self.blocked.send_replace(true);
    }
    fn release(&self) {
        self.blocked.send_replace(false);
    }
    async fn enter(&self) {
        let mut receiver = self.blocked.subscribe();
        {
            let mut entered = self.entered.lock().unwrap();
            *entered += 1;
            self.changed.notify_all();
        }
        while *receiver.borrow_and_update() {
            if receiver.changed().await.is_err() {
                break;
            }
        }
    }
    fn wait(&self, count: usize) {
        let entered = self.entered.lock().unwrap();
        let (entered, _) = self
            .changed
            .wait_timeout_while(entered, Duration::from_secs(5), |value| *value < count)
            .unwrap();
        assert!(
            *entered >= count,
            "gate {count} was never entered; observed {}",
            *entered
        );
    }
}

struct PoolSession {
    id: usize,
    healthy: AtomicBool,
    commands: Mutex<Vec<String>>,
    control_gate: Gate,
    maintenance_gate: Gate,
    fail_next: AtomicBool,
    maintenance_fail: AtomicBool,
    retire_gate: Gate,
    close_gate: Gate,
    close_panic_once: AtomicBool,
    maintenance_retired: AtomicBool,
}
impl PoolSession {
    fn new(id: usize) -> Self {
        Self {
            id,
            healthy: AtomicBool::new(true),
            commands: Mutex::new(Vec::new()),
            control_gate: Gate::default(),
            maintenance_gate: Gate::default(),
            fail_next: AtomicBool::new(false),
            maintenance_fail: AtomicBool::new(false),
            retire_gate: Gate::default(),
            close_gate: Gate::default(),
            close_panic_once: AtomicBool::new(false),
            maintenance_retired: AtomicBool::new(false),
        }
    }
}
#[async_trait]
impl Session for PoolSession {
    async fn command(
        &self,
        command: &Command,
        _context: &ExecutionContext,
    ) -> Result<OperationResult, Error> {
        assert!(
            self.healthy.load(Ordering::Acquire),
            "retired session must never receive a command"
        );
        self.commands.lock().unwrap().push(command.text().into());
        self.control_gate.enter().await;
        if command.maintenance() != pmac_protocol::MaintenanceKind::None {
            self.maintenance_gate.enter().await;
        }
        if self.fail_next.swap(false, Ordering::AcqRel) {
            return Err(Error::disconnected("injected member failure").sent());
        }
        Ok(OperationResult::response(
            format!("{}:{}", self.id, command.text()).into_bytes(),
        ))
    }
    async fn shell(
        &self,
        _command: &str,
        _persistent: bool,
        _context: &ExecutionContext,
    ) -> Result<OperationResult, Error> {
        self.maintenance_gate.enter().await;
        if self.maintenance_fail.swap(false, Ordering::AcqRel) {
            return Err(Error::disconnected("injected maintenance failure").sent());
        }
        if self.maintenance_retired.load(Ordering::Acquire) {
            return Err(Error::disconnected("maintenance retired").sent());
        }
        Ok(OperationResult {
            response: b"EOF".to_vec(),
            exit_code: Some(0),
            ..OperationResult::default()
        })
    }
    async fn upload(
        &self,
        _local: &str,
        _remote: &str,
        _context: &ExecutionContext,
    ) -> Result<OperationResult, Error> {
        self.maintenance_gate.enter().await;
        Ok(OperationResult::default())
    }
    async fn download(
        &self,
        _remote: &str,
        _local: &str,
        _context: &ExecutionContext,
    ) -> Result<OperationResult, Error> {
        self.maintenance_gate.enter().await;
        Ok(OperationResult::default())
    }
    async fn file_exists(
        &self,
        _remote: &str,
        _context: &ExecutionContext,
    ) -> Result<OperationResult, Error> {
        Ok(OperationResult::response(b"1".to_vec()))
    }
    async fn file_remove(
        &self,
        _remote: &str,
        _context: &ExecutionContext,
    ) -> Result<OperationResult, Error> {
        Ok(OperationResult::default())
    }
    fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::Acquire)
    }
    async fn retire_control(&self) -> Result<(), Error> {
        self.retire_gate.enter().await;
        self.healthy.store(false, Ordering::Release);
        Ok(())
    }
    async fn retire_maintenance(&self) -> Result<(), Error> {
        self.maintenance_retired.store(true, Ordering::Release);
        Ok(())
    }
    async fn repair_maintenance(&self, _context: &ExecutionContext) -> Result<(), Error> {
        self.maintenance_retired.store(false, Ordering::Release);
        Ok(())
    }
    async fn close(&self) -> Result<(), Error> {
        if self.close_panic_once.swap(false, Ordering::AcqRel) {
            panic!("injected transport close panic");
        }
        self.close_gate.enter().await;
        self.healthy.store(false, Ordering::Release);
        self.maintenance_retired.store(true, Ordering::Release);
        Ok(())
    }
}

#[derive(Default)]
struct IndependentTransport {
    sessions: Mutex<Vec<Arc<PoolSession>>>,
    connect_gate: Gate,
    gated_attempt: AtomicUsize,
    attempts: AtomicUsize,
    panic_attempt: AtomicUsize,
    fail_attempt: AtomicUsize,
}
#[async_trait]
impl Transport for IndependentTransport {
    async fn connect(
        &self,
        options: &ConnectOptions,
        _context: &ExecutionContext,
    ) -> Result<Arc<dyn Session>, Error> {
        assert_eq!(
            options.control_pool_size, 1,
            "each pool member owns one independent SSH connection"
        );
        let attempt = self.attempts.fetch_add(1, Ordering::AcqRel) + 1;
        if self.gated_attempt.load(Ordering::Acquire) == attempt {
            self.connect_gate.enter().await;
        }
        if self.panic_attempt.load(Ordering::Acquire) == attempt {
            panic!("injected connect future panic");
        }
        if self.fail_attempt.load(Ordering::Acquire) == attempt {
            return Err(Error::new(
                ErrorCode::ConnectError,
                "injected pre-channel connection failure",
            ));
        }
        let session = Arc::new(PoolSession::new(attempt));
        self.sessions.lock().unwrap().push(session.clone());
        Ok(session)
    }
}
impl IndependentTransport {
    fn member(&self, index: usize) -> Arc<PoolSession> {
        self.sessions.lock().unwrap()[index].clone()
    }
}

struct Fixture {
    runtime: Arc<Runtime>,
    client: Arc<Client>,
    transport: Arc<IndependentTransport>,
}
impl Fixture {
    fn new() -> Self {
        let runtime = Runtime::new(2).unwrap();
        let transport = Arc::new(IndependentTransport::default());
        let client = runtime
            .create_client(transport.clone(), ClientConfig::default())
            .unwrap();
        Self {
            runtime,
            client,
            transport,
        }
    }
    fn submit(&self, operation: Operation, options: RequestOptions) -> Arc<Request> {
        self.client
            .submit(self.runtime.next_id(), operation, options)
            .unwrap()
    }
    fn prepare(&self, target_size: u32, max_members: u32) {
        self.finish(self.submit(
            Operation::PoolPrepare {
                options: ConnectOptions::new("mock", "test"),
                profile: profile(target_size, max_members),
            },
            options(),
        ))
        .unwrap();
    }
    fn finish(&self, request: Arc<Request>) -> Result<OperationResult, Error> {
        assert!(
            request.wait(5_000).completed,
            "request did not finish within the test budget"
        );
        request.result().unwrap().as_ref().clone()
    }
    fn command(&self, command: &str, read_pool: bool) -> Arc<Request> {
        self.submit(
            Operation::Command(
                self.client
                    .parse_command_for_current_thread(command)
                    .unwrap(),
            ),
            RequestOptions {
                read_pool,
                ..options()
            },
        )
    }
    fn command_on_new_thread(
        &self,
        command: &str,
        read_pool: bool,
    ) -> (Result<OperationResult, Error>, u64) {
        std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let request = self.command(command, read_pool);
                    let result = self.finish(request.clone());
                    (result, request.info().member_id)
                })
                .join()
                .unwrap()
        })
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.runtime.shutdown_clients(5_000);
    }
}
fn options() -> RequestOptions {
    RequestOptions {
        timeout_ms: 5_000,
        ..RequestOptions::default()
    }
}
fn profile(target_size: u32, max_members: u32) -> PoolProfile {
    PoolProfile {
        target_size,
        max_members,
        ..PoolProfile::default()
    }
}

#[test]
fn slow_member_creation_does_not_block_existing_control_lane() {
    let fixture = Fixture::new();
    fixture.prepare(1, 4);
    fixture.transport.gated_attempt.store(2, Ordering::Release);
    fixture.transport.connect_gate.block();
    let resize = fixture.submit(Operation::PoolResize(profile(2, 4)), options());
    fixture.transport.connect_gate.wait(1);
    let result = fixture.finish(fixture.command("#1p", false)).unwrap();
    assert_eq!(result.response, b"1:#1p");
    assert!(!resize.wait(0).completed);
    assert_eq!(fixture.client.pool_snapshot().creating, 1);
    fixture.transport.connect_gate.release();
    fixture.finish(resize).unwrap();
    assert_eq!(fixture.client.pool_snapshot().ready, 2);
}

#[test]
fn independent_read_requests_execute_concurrently_and_keep_member_identity() {
    let fixture = Fixture::new();
    fixture.prepare(2, 4);
    let first = fixture.transport.member(0);
    let second = fixture.transport.member(1);
    first.control_gate.block();
    second.control_gate.block();
    std::thread::scope(|scope| {
        let first_thread = scope.spawn(|| {
            let request = fixture.command("#1p", true);
            let result = fixture.finish(request.clone());
            (request, result)
        });
        first.control_gate.wait(1);
        let second_thread = scope.spawn(|| {
            let request = fixture.command("#2p", true);
            let result = fixture.finish(request.clone());
            (request, result)
        });
        second.control_gate.wait(1);
        assert_eq!(fixture.client.pool_snapshot().busy, 2);
        first.control_gate.release();
        second.control_gate.release();
        let (first_read, first_result) = first_thread.join().unwrap();
        let (second_read, second_result) = second_thread.join().unwrap();
        assert_eq!(first_read.info().member_id, 1);
        assert_eq!(second_read.info().member_id, 2);
        assert_eq!(first_result.unwrap().response, b"1:#1p");
        assert_eq!(second_result.unwrap().response, b"2:#2p");
    });
}

#[test]
fn open_program_is_isolated_from_another_calling_thread() {
    let fixture = Fixture::new();
    fixture.prepare(2, 4);
    let open = fixture.command("open prog 1", false);
    fixture.finish(open.clone()).unwrap();
    let owner = open.info().member_id;
    let program_line = fixture.command("dwell 10", false);
    fixture.finish(program_line.clone()).unwrap();
    assert_eq!(program_line.info().member_id, owner);

    let (other_result, other_member) = fixture.command_on_new_thread("Motor[1].ServoCtrl=1", false);
    assert_ne!(other_member, owner);
    assert_eq!(
        other_result.unwrap().response,
        format!("{other_member}:Motor[1].ServoCtrl=1").as_bytes()
    );

    let close = fixture.command("close", false);
    fixture.finish(close.clone()).unwrap();
    assert_eq!(close.info().member_id, owner);
    assert_eq!(
        fixture
            .transport
            .member(0)
            .commands
            .lock()
            .unwrap()
            .as_slice(),
        ["open prog 1", "dwell 10", "close"]
    );
    assert_eq!(
        fixture
            .transport
            .member(1)
            .commands
            .lock()
            .unwrap()
            .as_slice(),
        ["Motor[1].ServoCtrl=1"]
    );
}

#[test]
fn timed_out_member_isolated_without_control_migration_or_automatic_repair() {
    let fixture = Fixture::new();
    fixture.prepare(2, 4);
    let first = fixture.transport.member(0);
    first.control_gate.block();
    let timeout = fixture.submit(
        Operation::Command(Command::raw("#1p").unwrap()),
        RequestOptions {
            read_pool: false,
            timeout_ms: 50,
            ..options()
        },
    );
    first.control_gate.wait(1);
    let error = fixture.finish(timeout).unwrap_err();
    assert_eq!(error.code, ErrorCode::ExecTimeout);
    assert_eq!(error.certainty, ExecutionCertainty::MayHaveExecuted);
    let snapshot = fixture.client.pool_snapshot();
    assert_eq!(snapshot.ready, 1);
    assert_eq!(snapshot.failed, 1);
    assert_eq!(snapshot.release_uncertain, 0);
    assert!(!snapshot.unknown_write);
    let ordered_error = fixture.finish(fixture.command("#2p", false)).unwrap_err();
    assert_eq!(ordered_error.code, ErrorCode::NoReadyLane);
    assert_eq!(ordered_error.certainty, ExecutionCertainty::NotSent);
    let read_error = fixture.finish(fixture.command("#2p", true)).unwrap_err();
    assert_eq!(read_error.code, ErrorCode::NoReadyLane);
    let (other_thread, member) = fixture.command_on_new_thread("#2p", true);
    assert_eq!(member, 2);
    assert_eq!(other_thread.unwrap().response, b"2:#2p");
    assert_eq!(fixture.transport.attempts.load(Ordering::Acquire), 2);
    first.control_gate.release();
}

#[test]
fn unknown_write_blocks_new_conflicting_work_until_explicit_reconciliation() {
    let fixture = Fixture::new();
    fixture.prepare(2, 4);
    fixture
        .transport
        .member(0)
        .fail_next
        .store(true, Ordering::Release);
    let write = Command::motor_abs_jog(1, 1.0).unwrap();
    let error = fixture
        .finish(fixture.submit(Operation::Command(write.clone()), options()))
        .unwrap_err();
    assert_eq!(error.certainty, ExecutionCertainty::MayHaveExecuted);
    assert!(fixture.client.pool_snapshot().unknown_write);
    let blocked = fixture
        .client
        .submit(
            fixture.runtime.next_id(),
            Operation::Command(write.clone()),
            options(),
        )
        .unwrap_err();
    assert_eq!(blocked.code, ErrorCode::RecoveryRequired);
    fixture.command_on_new_thread("#1p", true).0.unwrap();
    fixture
        .finish(fixture.submit(Operation::PoolRepair { member_id: 1 }, options()))
        .unwrap();
    assert!(
        fixture.client.pool_snapshot().unknown_write,
        "healthy capacity does not reconcile unknown effects"
    );
    let revision = fixture.client.pool_snapshot().revision;
    assert_eq!(
        fixture
            .client
            .confirm_recovery(revision - 1, 1)
            .unwrap_err()
            .code,
        ErrorCode::Superseded
    );
    fixture.client.confirm_recovery(revision, 1).unwrap();
    fixture
        .finish(fixture.submit(Operation::Command(write), options()))
        .unwrap();
    assert!(!fixture.client.pool_snapshot().unknown_write);
}

#[test]
fn maintenance_barrier_waits_earlier_work_and_rejects_new_admission() {
    let fixture = Fixture::new();
    fixture.prepare(2, 4);
    let first = fixture.transport.member(0);
    first.control_gate.block();
    first.maintenance_gate.block();
    let earlier = fixture.command("#1p", false);
    first.control_gate.wait(1);
    let maintenance = fixture.command("save", false);
    let blocked = fixture
        .client
        .submit(
            fixture.runtime.next_id(),
            Operation::Command(Command::raw("#2p").unwrap()),
            options(),
        )
        .unwrap_err();
    assert_eq!(blocked.code, ErrorCode::MaintenanceActive);
    assert_eq!(first.commands.lock().unwrap().as_slice(), ["#1p"]);
    first.control_gate.release();
    fixture.finish(earlier).unwrap();
    first.maintenance_gate.wait(1);
    assert!(fixture.client.pool_snapshot().maintenance_active);
    assert_eq!(
        fixture
            .client
            .submit(
                fixture.runtime.next_id(),
                Operation::Command(Command::raw("#2p").unwrap()),
                options()
            )
            .unwrap_err()
            .code,
        ErrorCode::MaintenanceActive
    );
    first.maintenance_gate.release();
    fixture.finish(maintenance).unwrap();
    fixture.finish(fixture.command("#2p", false)).unwrap();
}

#[test]
fn queued_cancellation_is_never_sent_while_owner_is_blocked() {
    let fixture = Fixture::new();
    fixture.prepare(1, 4);
    let session = fixture.transport.member(0);
    session.control_gate.block();
    let owner = fixture.command("#1p", false);
    session.control_gate.wait(1);
    let queued = fixture.command("#2p", false);
    queued.cancel();
    let error = fixture.finish(queued).unwrap_err();
    assert_eq!(error.code, ErrorCode::Cancelled);
    assert_eq!(error.certainty, ExecutionCertainty::NotSent);
    let next = fixture.command("#3p", false);
    assert_eq!(session.commands.lock().unwrap().as_slice(), ["#1p"]);
    session.control_gate.release();
    fixture.finish(owner).unwrap();
    fixture.finish(next).unwrap();
    assert_eq!(session.commands.lock().unwrap().as_slice(), ["#1p", "#3p"]);
}

#[test]
fn superseded_creation_cannot_restore_an_obsolete_pool_revision() {
    let fixture = Fixture::new();
    fixture.prepare(1, 5);
    fixture.transport.gated_attempt.store(2, Ordering::Release);
    fixture.transport.connect_gate.block();
    let obsolete = fixture.submit(Operation::PoolResize(profile(2, 5)), options());
    fixture.transport.connect_gate.wait(1);
    let current = fixture.submit(Operation::PoolResize(profile(3, 5)), options());
    let revision = fixture.client.pool_snapshot().revision;
    fixture.transport.connect_gate.release();
    assert_eq!(
        fixture.finish(obsolete).unwrap_err().code,
        ErrorCode::Superseded
    );
    fixture.finish(current).unwrap();
    let snapshot = fixture.client.pool_snapshot();
    assert_eq!(snapshot.revision, revision);
    assert_eq!(snapshot.desired, 3);
    assert_eq!(snapshot.creating, 0);
    assert_eq!(snapshot.release_uncertain, 1);
    let obsolete_member = fixture
        .client
        .pool_members()
        .into_iter()
        .find(|member| member.member_id == 2)
        .unwrap();
    assert_eq!(obsolete_member.state, MemberState::Failed);
    assert!(!fixture.transport.member(1).healthy.load(Ordering::Acquire));
}

#[test]
fn close_cancels_inflight_creation_and_prevents_late_member_installation() {
    let fixture = Fixture::new();
    fixture.prepare(1, 4);
    fixture.transport.gated_attempt.store(2, Ordering::Release);
    fixture.transport.connect_gate.block();
    let creating = fixture.submit(Operation::PoolResize(profile(2, 4)), options());
    fixture.transport.connect_gate.wait(1);
    fixture.runtime.shutdown_clients(5_000).unwrap();
    assert!(creating.wait(0).completed);
    let snapshot = fixture.client.pool_snapshot();
    assert_eq!(snapshot.ready, 0);
    assert_eq!(snapshot.creating, 0);
    assert!(fixture.client.snapshot().closing);
    assert_eq!(
        fixture.client.snapshot().state,
        pmac_core::ClientState::Closed
    );
    fixture.transport.connect_gate.release();
    assert_eq!(fixture.transport.sessions.lock().unwrap().len(), 1);
    assert!(fixture
        .client
        .pool_members()
        .iter()
        .all(|member| member.state == MemberState::Drained));
}

#[test]
fn uncertain_remote_lease_counts_against_quota_until_explicit_release_acknowledgement() {
    let fixture = Fixture::new();
    fixture.prepare(2, 2);
    let session = fixture.transport.member(1);
    session.close_gate.block();
    let drain = fixture.submit(
        Operation::PoolDrain { member_id: 2 },
        RequestOptions {
            timeout_ms: 50,
            ..options()
        },
    );
    session.close_gate.wait(1);
    assert_eq!(
        fixture.finish(drain).unwrap_err().code,
        ErrorCode::ExecTimeout
    );
    session.close_gate.release();
    assert_eq!(fixture.client.pool_snapshot().release_uncertain, 1);
    let repair = fixture
        .finish(fixture.submit(Operation::PoolRepair { member_id: 2 }, options()))
        .unwrap_err();
    assert_eq!(repair.code, ErrorCode::ResourceLimit);
    assert_eq!(fixture.transport.attempts.load(Ordering::Acquire), 2);
    let revision = fixture.client.pool_snapshot().revision;
    fixture.client.confirm_recovery(revision, 2).unwrap();
    fixture
        .finish(fixture.submit(Operation::PoolRepair { member_id: 2 }, options()))
        .unwrap();
    assert_eq!(fixture.client.pool_snapshot().ready, 2);
    assert_eq!(fixture.transport.attempts.load(Ordering::Acquire), 3);
    assert_eq!(fixture.client.pool_members()[1].epoch, 2);
}

#[test]
fn read_pool_rejects_side_effects_before_any_transport_call() {
    let fixture = Fixture::new();
    fixture.prepare(2, 4);
    let error = fixture
        .client
        .submit(
            fixture.runtime.next_id(),
            Operation::Command(Command::motor_abs_jog(1, 2.0).unwrap()),
            RequestOptions {
                read_pool: true,
                ..options()
            },
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert!(fixture
        .transport
        .member(0)
        .commands
        .lock()
        .unwrap()
        .is_empty());
    assert!(fixture
        .transport
        .member(1)
        .commands
        .lock()
        .unwrap()
        .is_empty());
}

#[test]
fn maintenance_fault_preserves_control_and_requires_explicit_repair() {
    let fixture = Fixture::new();
    fixture.prepare(1, 4);
    let session = fixture.transport.member(0);
    session.maintenance_fail.store(true, Ordering::Release);
    let shell = Operation::Shell {
        command: "test".into(),
        persistent: true,
    };
    let error = fixture
        .finish(fixture.submit(shell.clone(), options()))
        .unwrap_err();
    assert_eq!(error.certainty, ExecutionCertainty::MayHaveExecuted);
    assert!(fixture.client.pool_snapshot().maintenance_failed);
    assert!(fixture.client.is_connected());
    fixture.finish(fixture.command("#1p", false)).unwrap();
    let revision = fixture.client.pool_snapshot().revision;
    fixture.client.confirm_recovery(revision, 1).unwrap();
    let error = fixture
        .finish(fixture.submit(shell.clone(), options()))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Disconnected);
    assert_eq!(error.certainty, ExecutionCertainty::NotSent);
    fixture
        .finish(fixture.submit(Operation::RepairMaintenance, options()))
        .unwrap();
    fixture.finish(fixture.submit(shell, options())).unwrap();
    assert!(!fixture.client.pool_snapshot().maintenance_failed);
    assert_eq!(fixture.transport.attempts.load(Ordering::Acquire), 1);
}

#[test]
fn close_reports_pending_while_transport_cleanup_has_not_quiesced() {
    let fixture = Fixture::new();
    fixture.prepare(1, 4);
    let session = fixture.transport.member(0);
    session.retire_gate.block();
    session.fail_next.store(true, Ordering::Release);
    let failing = fixture.command("#1p", false);
    session.retire_gate.wait(1);
    assert!(!failing.wait(0).completed);
    assert_eq!(
        fixture.runtime.shutdown_clients(10).unwrap_err().code,
        ErrorCode::ShutdownPending
    );
    assert_ne!(
        fixture.client.snapshot().state,
        pmac_core::ClientState::Closed
    );
    session.retire_gate.release();
    fixture.runtime.shutdown_clients(5_000).unwrap();
    assert!(failing.wait(0).completed);
    assert_eq!(
        fixture.client.snapshot().state,
        pmac_core::ClientState::Closed
    );
}

#[test]
fn queued_ordered_request_is_not_replayed_across_a_repaired_member_epoch() {
    let fixture = Fixture::new();
    fixture.prepare(1, 4);
    let session = fixture.transport.member(0);
    session.control_gate.block();
    session.fail_next.store(true, Ordering::Release);
    let failing = fixture.command("#1p", false);
    session.control_gate.wait(1);
    let queued = fixture.command("#2p", false);
    session.control_gate.release();
    fixture.finish(failing).unwrap_err();
    fixture
        .finish(fixture.submit(Operation::PoolRepair { member_id: 1 }, options()))
        .unwrap();
    let error = fixture.finish(queued).unwrap_err();
    assert_eq!(error.certainty, ExecutionCertainty::NotSent);
    assert!(matches!(
        error.code,
        ErrorCode::NoReadyLane | ErrorCode::Disconnected
    ));
    assert!(!fixture
        .transport
        .member(1)
        .commands
        .lock()
        .unwrap()
        .iter()
        .any(|command| command == "#2p"));
    fixture.finish(fixture.command("#3p", false)).unwrap();
}

#[test]
fn cancel_during_old_session_cleanup_does_not_leak_creating_capacity() {
    let fixture = Fixture::new();
    fixture.prepare(1, 4);
    let session = fixture.transport.member(0);
    session.fail_next.store(true, Ordering::Release);
    fixture.finish(fixture.command("#1p", false)).unwrap_err();
    session.close_gate.block();
    let repair = fixture.submit(Operation::PoolRepair { member_id: 1 }, options());
    session.close_gate.wait(1);
    repair.cancel();
    session.close_gate.release();
    let error = fixture.finish(repair).unwrap_err();
    assert_eq!(error.code, ErrorCode::Cancelled);
    fixture.runtime.shutdown_clients(5_000).unwrap();
    assert_eq!(
        fixture.client.pool_snapshot().creating,
        0,
        "cancelled pre-creation cleanup must return its capacity token"
    );
}

#[test]
fn panicking_pre_channel_connect_releases_capacity_and_records_failed_member() {
    let fixture = Fixture::new();
    fixture.prepare(1, 4);
    fixture.transport.panic_attempt.store(2, Ordering::Release);
    let error = fixture
        .finish(fixture.submit(Operation::PoolResize(profile(2, 4)), options()))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Internal);
    let snapshot = fixture.client.pool_snapshot();
    assert_eq!(snapshot.creating, 0);
    assert_eq!(snapshot.failed, 1);
    assert_eq!(snapshot.ready, 1);
    assert_eq!(snapshot.release_uncertain, 0);
    fixture.finish(fixture.command("#1p", false)).unwrap();
}

#[test]
fn asynchronously_closed_transport_is_observable_and_explicitly_repairable() {
    let fixture = Fixture::new();
    fixture.prepare(2, 4);
    fixture
        .transport
        .member(1)
        .healthy
        .store(false, Ordering::Release);
    let snapshot = fixture.client.pool_snapshot();
    assert_eq!(snapshot.ready, 1);
    assert_eq!(
        snapshot.failed, 1,
        "transport health must update the stored member state"
    );
    fixture
        .finish(fixture.submit(Operation::PoolRepair { member_id: 2 }, options()))
        .unwrap();
    let snapshot = fixture.client.pool_snapshot();
    assert_eq!(snapshot.ready, 2);
    assert_eq!(snapshot.failed, 0);
    assert_eq!(fixture.transport.attempts.load(Ordering::Acquire), 3);
}

#[test]
fn replacing_failed_control_owner_waits_for_its_healthy_maintenance_operation() {
    let fixture = Fixture::new();
    fixture.prepare(1, 4);
    let session = fixture.transport.member(0);
    session.maintenance_gate.block();
    let shell = fixture.submit(
        Operation::Shell {
            command: "long running but healthy".into(),
            persistent: true,
        },
        options(),
    );
    session.maintenance_gate.wait(1);
    session.fail_next.store(true, Ordering::Release);
    fixture.finish(fixture.command("#1p", false)).unwrap_err();
    let repair = fixture.submit(Operation::PoolRepair { member_id: 1 }, options());
    assert!(
        !repair.wait(25).completed,
        "repair must wait for the maintenance owner to quiesce"
    );
    assert_eq!(
        *session.close_gate.entered.lock().unwrap(),
        0,
        "control repair must not kill independent healthy maintenance work"
    );
    session.maintenance_gate.release();
    fixture.finish(shell).unwrap();
    fixture.finish(repair).unwrap();
    assert_eq!(*session.close_gate.entered.lock().unwrap(), 1);
    assert_eq!(fixture.client.pool_snapshot().ready, 1);
}

#[test]
fn explicit_maintenance_repair_waits_for_existing_maintenance_work() {
    let fixture = Fixture::new();
    fixture.prepare(1, 4);
    let session = fixture.transport.member(0);
    session.maintenance_gate.block();
    let shell = fixture.submit(
        Operation::Shell {
            command: "long running".into(),
            persistent: true,
        },
        options(),
    );
    session.maintenance_gate.wait(1);
    let repair = fixture.submit(Operation::RepairMaintenance, options());
    assert!(!repair.wait(25).completed);
    assert!(!session.maintenance_retired.load(Ordering::Acquire));
    session.maintenance_gate.release();
    fixture.finish(shell).unwrap();
    fixture.finish(repair).unwrap();
    assert!(!fixture.client.pool_snapshot().maintenance_failed);
}

#[test]
fn failed_thread_member_does_not_disrupt_another_thread() {
    let fixture = Fixture::new();
    fixture.prepare(2, 4);
    let read_member = fixture.transport.member(0);
    read_member.fail_next.store(true, Ordering::Release);
    let error = fixture.finish(fixture.command("#1p", true)).unwrap_err();
    assert_eq!(error.certainty, ExecutionCertainty::MayHaveExecuted);
    assert_eq!(fixture.client.pool_snapshot().failed, 1);
    let same_thread = fixture.finish(fixture.command("#2p", false)).unwrap_err();
    assert_eq!(same_thread.code, ErrorCode::NoReadyLane);
    let (other_thread, member) = fixture.command_on_new_thread("#2p", false);
    assert_eq!(member, 2);
    assert_eq!(other_thread.unwrap().response, b"2:#2p");
    assert!(fixture.transport.member(1).healthy.load(Ordering::Acquire));
    assert_eq!(fixture.transport.attempts.load(Ordering::Acquire), 2);
}

#[test]
fn device_maintenance_waits_for_previously_authorized_member_creation() {
    let fixture = Fixture::new();
    fixture.prepare(1, 4);
    fixture.transport.gated_attempt.store(2, Ordering::Release);
    fixture.transport.connect_gate.block();
    let resize = fixture.submit(Operation::PoolResize(profile(2, 4)), options());
    fixture.transport.connect_gate.wait(1);
    let session = fixture.transport.member(0);
    session.maintenance_gate.block();
    let maintenance = fixture.command("save", false);
    assert!(!maintenance.wait(25).completed);
    let entered = *session.maintenance_gate.entered.lock().unwrap();
    fixture.transport.connect_gate.release();
    fixture.finish(resize).unwrap();
    session.maintenance_gate.wait(1);
    session.maintenance_gate.release();
    fixture.finish(maintenance).unwrap();
    assert_eq!(
        entered, 0,
        "device maintenance must not run concurrently with GPASCII creation"
    );
}

#[test]
fn explicit_resize_does_not_create_members_during_device_maintenance() {
    let fixture = Fixture::new();
    fixture.prepare(1, 4);
    let session = fixture.transport.member(0);
    session.maintenance_gate.block();
    let maintenance = fixture.command("save", false);
    session.maintenance_gate.wait(1);
    let resize = fixture.submit(Operation::PoolResize(profile(2, 4)), options());
    let resize_completed = resize.wait(25).completed;
    let attempts = fixture.transport.attempts.load(Ordering::Acquire);
    session.maintenance_gate.release();
    fixture.finish(maintenance).unwrap();
    fixture.finish(resize).unwrap();
    assert!(!resize_completed);
    assert_eq!(
        attempts, 1,
        "management operations must honor the device maintenance barrier"
    );
    assert_eq!(fixture.transport.attempts.load(Ordering::Acquire), 2);
}

#[test]
fn initial_prepare_reports_service_ready_after_minimum_capacity_is_verified() {
    let fixture = Fixture::new();
    fixture.transport.gated_attempt.store(2, Ordering::Release);
    fixture.transport.connect_gate.block();
    let prepare = fixture.submit(
        Operation::PoolPrepare {
            options: ConnectOptions::new("mock", "test"),
            profile: profile(2, 4),
        },
        options(),
    );
    fixture.transport.connect_gate.wait(1);
    let connected_at_minimum = fixture.client.is_connected();
    let snapshot = fixture.client.pool_snapshot();
    fixture.finish(fixture.command("#1p", false)).unwrap();
    fixture.transport.connect_gate.release();
    fixture.finish(prepare).unwrap();
    assert!(
        connected_at_minimum,
        "verified minimum capacity is available while additional members initialize"
    );
    assert_eq!(snapshot.ready, 1);
    assert_eq!(snapshot.creating, 1);
    assert_eq!(snapshot.desired, 2);
}

#[test]
fn pre_channel_connection_failure_allows_immediate_explicit_repair() {
    let fixture = Fixture::new();
    fixture.transport.fail_attempt.store(1, Ordering::Release);
    let error = fixture
        .finish(fixture.submit(
            Operation::PoolPrepare {
                options: ConnectOptions::new("mock", "test"),
                profile: profile(1, 1),
            },
            options(),
        ))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::ConnectError);
    assert_eq!(fixture.client.pool_snapshot().release_uncertain, 0);
    fixture
        .finish(fixture.submit(Operation::PoolRepair { member_id: 1 }, options()))
        .unwrap();
    assert_eq!(fixture.client.pool_snapshot().ready, 1);
    assert_eq!(fixture.client.pool_snapshot().release_uncertain, 0);
}

#[test]
fn buffer_load_does_not_require_an_additional_gpascii_service_slot() {
    let fixture = Fixture::new();
    fixture.prepare(1, 1);
    let result = fixture
        .finish(fixture.submit(
            Operation::BufferUpload {
                local: "mock-source".into(),
                remote: "/owned-buffer".into(),
                cleanup_remote: true,
            },
            options(),
        ))
        .unwrap();
    assert_eq!(result.response, b"EOF");
    assert_eq!(result.exit_code, Some(0));
    assert_eq!(fixture.client.pool_snapshot().release_uncertain, 0);
}

#[test]
fn compound_resets_retire_every_device_lane_regardless_of_maintenance_order() {
    for wire in ["save;$$$", "$$$;save", "ecat reset;$$$", "$$$***;save"] {
        let fixture = Fixture::new();
        fixture.prepare(2, 4);
        fixture.finish(fixture.command(wire, false)).unwrap();
        assert!(
            fixture
                .client
                .pool_members()
                .iter()
                .all(|member| member.state == MemberState::Failed),
            "all old lanes must retire after {wire:?}"
        );
        let snapshot = fixture.client.pool_snapshot();
        assert_eq!(snapshot.ready, 0, "{wire:?}");
        assert_eq!(snapshot.release_uncertain, 2, "{wire:?}");
        assert_eq!(
            fixture.transport.attempts.load(Ordering::Acquire),
            2,
            "a reset must not reconnect or replay automatically"
        );
    }
}

#[test]
fn complete_coordinate_position_queries_keep_the_calling_thread_affinity() {
    let fixture = Fixture::new();
    fixture.prepare(2, 4);
    for wire in ["&1P", "&1p", "&1P;#1p"] {
        let request = fixture.command(wire, true);
        let result = fixture.finish(request.clone()).unwrap();
        assert_eq!(request.info().member_id, 1, "{wire:?}");
        assert_eq!(result.response, format!("1:{wire}").as_bytes());
    }
    assert_eq!(
        fixture.transport.member(0).commands.lock().unwrap().len(),
        3
    );
    assert!(fixture
        .transport
        .member(1)
        .commands
        .lock()
        .unwrap()
        .is_empty());
}

#[test]
fn verified_global_queries_keep_thread_affinity_but_invalid_read_pool_syntax_is_rejected() {
    let fixture = Fixture::new();
    fixture.prepare(2, 4);
    for wire in ["EncTable[1].type", "P82", "ECAT[0].Enable", "list prog 100"] {
        let request = fixture.command(wire, true);
        assert_eq!(
            fixture.finish(request.clone()).unwrap().response,
            format!("1:{wire}").as_bytes()
        );
        assert_eq!(request.info().member_id, 1);
    }
    for wire in [
        "EncTable[1].type=1",
        "P82=1",
        "ECAT[0].Enable extra",
        "list prog 100 extra",
        "Q82",
    ] {
        let Ok(command) = Command::raw(wire) else {
            continue;
        };
        let error = fixture
            .client
            .submit(
                fixture.runtime.next_id(),
                Operation::Command(command),
                RequestOptions {
                    read_pool: true,
                    ..options()
                },
            )
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidArgument, "{wire:?}");
    }
    assert_eq!(
        fixture.transport.member(0).commands.lock().unwrap().len(),
        4
    );
    assert!(fixture
        .transport
        .member(1)
        .commands
        .lock()
        .unwrap()
        .is_empty());
}

#[test]
fn bounded_parameter_range_can_use_read_pool_but_oversize_reverse_and_write_ranges_cannot() {
    let fixture = Fixture::new();
    fixture.prepare(2, 4);
    let request = fixture.command("P1..8192", true);
    assert_eq!(
        fixture.finish(request.clone()).unwrap().response,
        b"1:P1..8192"
    );
    assert_eq!(request.info().member_id, 1);
    for wire in [
        "P1..8193",
        "P3..1",
        "P1..4294967296",
        "P1..8192=1",
        "P1..8192 extra",
    ] {
        let Ok(command) = Command::raw(wire) else {
            continue;
        };
        let error = fixture
            .client
            .submit(
                fixture.runtime.next_id(),
                Operation::Command(command),
                RequestOptions {
                    read_pool: true,
                    ..options()
                },
            )
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidArgument, "{wire:?}");
    }
    assert_eq!(
        fixture
            .transport
            .member(0)
            .commands
            .lock()
            .unwrap()
            .as_slice(),
        ["P1..8192"]
    );
}

#[test]
fn rereview_drain_cancel_after_close_started() {
    let fixture = Fixture::new();
    fixture.prepare(2, 4);
    let session = fixture.transport.member(1);
    session.close_gate.block();
    let request = fixture.submit(Operation::PoolDrain { member_id: 2 }, options());
    session.close_gate.wait(1);
    request.cancel();
    let result = request.wait(1000);
    assert!(result.completed);
    assert_eq!(result.info.code, ErrorCode::Cancelled);
    assert_eq!(result.info.certainty, ExecutionCertainty::MayHaveExecuted);
    assert_eq!(
        fixture
            .client
            .pool_members()
            .into_iter()
            .find(|m| m.member_id == 2)
            .unwrap()
            .state,
        MemberState::Failed
    );
    session.close_gate.release();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(
        fixture
            .client
            .pool_members()
            .into_iter()
            .find(|m| m.member_id == 2)
            .unwrap()
            .state,
        MemberState::Failed
    );
}
#[test]
fn rereview_drain_success_after_execution_deadline() {
    let fixture = Fixture::new();
    fixture.prepare(2, 4);
    let session = fixture.transport.member(1);
    session.close_gate.block();
    let request = fixture.submit(
        Operation::PoolDrain { member_id: 2 },
        RequestOptions {
            timeout_ms: 100,
            ..options()
        },
    );
    session.close_gate.wait(1);
    let result = request.wait(1000);
    assert!(result.completed);
    assert_eq!(result.info.code, ErrorCode::ExecTimeout);
    assert_eq!(result.info.certainty, ExecutionCertainty::MayHaveExecuted);
    session.close_gate.release();
}

fn parsed_program_request(fixture: &Fixture, text: &str) -> Arc<Request> {
    let command = fixture
        .client
        .parse_command_for_current_thread(text)
        .unwrap();
    fixture.submit(Operation::Command(command), options())
}

#[test]
fn confirmed_program_buffer_passthrough_is_thread_and_epoch_bound() {
    let fixture = Fixture::new();
    fixture.prepare(2, 4);
    assert!(fixture
        .client
        .parse_command_for_current_thread("abort")
        .is_err());
    fixture
        .finish(parsed_program_request(&fixture, "open prog 1"))
        .unwrap();
    let program = fixture
        .client
        .parse_command_for_current_thread("abort")
        .unwrap();
    assert!(program.is_program_line());
    std::thread::scope(|scope| {
        scope
            .spawn(|| {
                assert!(fixture
                    .client
                    .parse_command_for_current_thread("abort")
                    .is_err())
            })
            .join()
            .unwrap();
    });
    assert!(fixture
        .client
        .parse_command_for_current_thread("close;abort")
        .is_err());
    let body = parsed_program_request(&fixture, "abort");
    fixture.finish(body.clone()).unwrap();
    let close = parsed_program_request(&fixture, "close");
    fixture.finish(close.clone()).unwrap();
    assert_eq!(body.info().member_id, close.info().member_id);
    assert!(fixture
        .client
        .parse_command_for_current_thread("abort")
        .is_err());
    fixture
        .finish(parsed_program_request(&fixture, "open prog 2"))
        .unwrap();
    fixture
        .transport
        .member(0)
        .fail_next
        .store(true, Ordering::Release);
    assert!(fixture
        .finish(parsed_program_request(&fixture, "P1=2"))
        .is_err());
    assert!(fixture
        .client
        .parse_command_for_current_thread("abort")
        .is_err());
}

#[test]
fn program_buffer_requires_successful_open_confirmation() {
    let fixture = Fixture::new();
    fixture.prepare(1, 4);
    let member = fixture.transport.member(0);
    member.control_gate.block();
    let open = parsed_program_request(&fixture, "open prog 1");
    member.control_gate.wait(1);
    assert!(fixture
        .client
        .parse_command_for_current_thread("abort")
        .is_err());
    member.control_gate.release();
    fixture.finish(open).unwrap();
    assert!(fixture
        .client
        .parse_command_for_current_thread("abort")
        .unwrap()
        .is_program_line());
    fixture
        .finish(parsed_program_request(&fixture, "close"))
        .unwrap();
    member.fail_next.store(true, Ordering::Release);
    assert!(fixture
        .finish(parsed_program_request(&fixture, "open prog 2"))
        .is_err());
    assert!(fixture
        .client
        .parse_command_for_current_thread("abort")
        .is_err());
}

#[test]
fn queued_close_blocks_program_admission_and_not_sent_cancel_reopens_it() {
    let fixture = Fixture::new();
    fixture.prepare(1, 4);
    fixture
        .finish(parsed_program_request(&fixture, "open prog 1"))
        .unwrap();
    let member = fixture.transport.member(0);
    member.control_gate.block();
    let body = parsed_program_request(&fixture, "P1=1");
    member.control_gate.wait(2);
    let close = parsed_program_request(&fixture, "close");
    assert_eq!(
        fixture
            .client
            .parse_command_for_current_thread("abort")
            .unwrap_err()
            .code,
        ErrorCode::MaintenanceActive
    );
    assert_eq!(close.cancel(), ErrorCode::Ok);
    assert_eq!(close.info().certainty, ExecutionCertainty::NotSent);
    assert!(fixture
        .client
        .parse_command_for_current_thread("abort")
        .unwrap()
        .is_program_line());
    member.control_gate.release();
    fixture.finish(body).unwrap();
    fixture
        .finish(parsed_program_request(&fixture, "abort"))
        .unwrap();
    fixture
        .finish(parsed_program_request(&fixture, "close"))
        .unwrap();
    assert!(fixture
        .client
        .parse_command_for_current_thread("abort")
        .is_err());
}

#[test]
fn program_close_boundary_respects_quotes_and_rejects_context_escape() {
    let fixture = Fixture::new();
    fixture.prepare(1, 4);
    fixture
        .finish(parsed_program_request(&fixture, "open prog 1"))
        .unwrap();
    for text in [
        "close;abort",
        "close // comment",
        "P1=1; close",
        "$$$",
        "reboot",
    ] {
        assert!(
            fixture
                .client
                .parse_command_for_current_thread(text)
                .is_err(),
            "accepted {text}"
        );
    }
    for text in ["cmd \"text;close\"", "P1=1 // ;close", "P1=1;P2=2"] {
        assert!(
            fixture
                .client
                .parse_command_for_current_thread(text)
                .unwrap()
                .is_program_line(),
            "{text}"
        );
    }
    fixture
        .finish(parsed_program_request(&fixture, "close"))
        .unwrap();
}

#[test]
fn confirmed_close_all_buffers_invalidates_other_threads_program_contexts() {
    for wire in [
        "CLOSE  all\tbuffers",
        "P1;close all buffers",
        "close all buffers;P1",
    ] {
        let fixture = Fixture::new();
        fixture.prepare(2, 4);
        fixture
            .finish(parsed_program_request(&fixture, "open prog 1"))
            .unwrap();
        assert!(fixture
            .client
            .parse_command_for_current_thread("abort")
            .unwrap()
            .is_program_line());
        std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let command = fixture
                        .client
                        .parse_command_for_current_thread(wire)
                        .unwrap();
                    assert_eq!(command.effect(), pmac_protocol::CommandEffect::Maintenance);
                    fixture
                        .finish(fixture.submit(Operation::Command(command), options()))
                        .unwrap();
                })
                .join()
                .unwrap();
        });
        assert!(
            fixture
                .client
                .parse_command_for_current_thread("abort")
                .is_err(),
            "stale program context after {wire:?}"
        );
        let stale = Command::program_line("abort").unwrap();
        assert!(fixture
            .client
            .submit(
                fixture.runtime.next_id(),
                Operation::Command(stale),
                options()
            )
            .is_err());
    }
}
#[test]
fn program_buffer_open_accepts_protocol_whitespace() {
    for wire in ["open  prog 1", "open\tprog 1", "&1open  forward"] {
        let fixture = Fixture::new();
        fixture.prepare(1, 4);
        fixture
            .finish(parsed_program_request(&fixture, wire))
            .unwrap();
        assert!(
            fixture
                .client
                .parse_command_for_current_thread("dwell 10")
                .unwrap()
                .is_program_line(),
            "{wire:?}"
        );
    }
}

#[test]
fn compound_program_open_is_rejected_even_after_online_prefix() {
    let fixture = Fixture::new();
    for wire in ["P1;open prog 1", "P1=2;open  prog 1", "&1P;open\tprog 1"] {
        assert!(
            fixture
                .client
                .parse_command_for_current_thread(wire)
                .is_err(),
            "{wire:?}"
        );
    }
    assert!(fixture
        .client
        .parse_command_for_current_thread("&1cpx P1=2;open prog 1")
        .is_ok());
}

#[test]
fn close_all_buffers_in_inline_program_text_preserves_other_threads_contexts() {
    for wire in [
        "&1cpx P1=2;close all buffers",
        "&1cx P1=2;close all buffers",
        "P1;&1cpx P1=2;close all buffers",
    ] {
        let fixture = Fixture::new();
        fixture.prepare(2, 4);
        fixture
            .finish(parsed_program_request(&fixture, "open prog 1"))
            .unwrap();
        let (result, _) = fixture.command_on_new_thread(wire, false);
        result.unwrap();
        assert!(
            fixture
                .client
                .parse_command_for_current_thread("dwell 10")
                .unwrap()
                .is_program_line(),
            "inline program text cleared context: {wire:?}"
        );
    }
}

#[test]
fn failed_compound_close_all_buffers_retires_all_control_members() {
    for wire in ["P1;close all buffers", "close all buffers;P1"] {
        let fixture = Fixture::new();
        fixture.prepare(2, 4);
        fixture
            .finish(parsed_program_request(&fixture, "open prog 1"))
            .unwrap();
        for session in fixture.transport.sessions.lock().unwrap().iter() {
            session.fail_next.store(true, Ordering::Release);
        }
        let (result, _) = fixture.command_on_new_thread(wire, false);
        assert!(result.is_err());
        assert!(fixture
            .client
            .pool_members()
            .iter()
            .all(|member| member.state == MemberState::Failed));
        assert!(fixture
            .client
            .parse_command_for_current_thread("dwell 10")
            .is_err());
        assert_eq!(fixture.client.pool_snapshot().release_uncertain, 2);
    }
}

#[test]
fn opaque_commands_bypass_program_context_and_never_enter_read_pool() {
    let fixture = Fixture::new();
    fixture.prepare(1, 2);
    fixture
        .finish(parsed_program_request(&fixture, "open prog 1"))
        .unwrap();
    let wire = "  save  ";
    assert!(fixture
        .client
        .parse_command_for_current_thread(wire)
        .is_err());
    let result = fixture.submit(
        Operation::Command(Command::passthrough(wire).unwrap()),
        options(),
    );
    fixture.finish(result).unwrap();
    assert_eq!(
        fixture
            .transport
            .member(0)
            .commands
            .lock()
            .unwrap()
            .last()
            .unwrap(),
        wire
    );
    let result = fixture.client.submit(
        fixture.runtime.next_id(),
        Operation::Command(Command::passthrough("P1").unwrap()),
        RequestOptions {
            read_pool: true,
            ..options()
        },
    );
    assert!(matches!(result, Err(error) if error.code == ErrorCode::InvalidArgument));
    assert!(fixture
        .client
        .parse_command_for_current_thread("dwell 10")
        .unwrap()
        .is_program_line());
}

#[test]
fn panicked_transport_close_is_retried_without_claiming_release() {
    let fixture = Fixture::new();
    fixture.prepare(1, 2);
    let session = fixture.transport.member(0);
    session.close_panic_once.store(true, Ordering::Release);
    session.close_gate.block();
    assert_eq!(
        fixture.runtime.shutdown_clients(10).unwrap_err().code,
        ErrorCode::ShutdownPending
    );
    // The first attempt panicked; the next attempt remains blocked in real cleanup.
    session.close_gate.wait(1);
    assert_eq!(
        fixture.client.snapshot().state,
        pmac_core::ClientState::Closing
    );
    assert_eq!(fixture.client.pool_snapshot().release_uncertain, 1);
    assert!(session.healthy.load(Ordering::Acquire));
    session.close_gate.release();
    fixture.runtime.shutdown_clients(5_000).unwrap();
    assert_eq!(
        fixture.client.snapshot().state,
        pmac_core::ClientState::Closed
    );
    assert!(!session.healthy.load(Ordering::Acquire));
    assert!(fixture.runtime.is_shutdown());
    assert!(!fixture.client.has_unreleased_gpascii());
}

#[test]
fn close_retries_only_members_without_confirmation() {
    let fixture = Fixture::new();
    fixture.prepare(2, 2);
    let confirmed = fixture.transport.member(0);
    let retried = fixture.transport.member(1);
    retried.close_panic_once.store(true, Ordering::Release);
    fixture.runtime.shutdown_clients(5_000).unwrap();
    assert_eq!(*confirmed.close_gate.entered.lock().unwrap(), 1);
    assert_eq!(*retried.close_gate.entered.lock().unwrap(), 1);
    assert!(!fixture.client.has_unreleased_gpascii());
    assert!(fixture.runtime.is_shutdown());
}
