use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use pmac_core::{
    Client, ClientConfig, ConnectOptions, Error, ErrorCode, ExecutionCertainty, HostKeyPolicy,
    Operation, OperationResult, RequestOptions, Runtime,
};
use pmac_protocol::Command;
use russh::keys::{Algorithm, PrivateKey, PublicKey, PublicKeyBase64};
use russh::server::{self, Auth, Msg, Session};
use russh::{Channel, ChannelId};
use sha2::{Digest, Sha256};
use tokio::sync::Notify;
use zeroize::Zeroizing;

use crate::RusshTransport;

#[derive(Default)]
struct Observed {
    password_attempts: AtomicUsize,
    key_attempts: AtomicUsize,
    commands: std::sync::Mutex<Vec<String>>,
    command_channels: std::sync::Mutex<Vec<(ChannelId, String)>>,
    next_connection: AtomicUsize,
    open_programs: std::sync::Mutex<HashMap<(usize, ChannelId), Vec<String>>>,
    program_lines: std::sync::Mutex<Vec<String>>,
    gated_command: std::sync::Mutex<String>,
    command_gate: AsyncGate,
    max_gpascii_channels: AtomicUsize,
    gpascii_channels: AtomicUsize,
    closed_gpascii_channels: AtomicUsize,
    delayed_started: Notify,
    delayed_release: Notify,
    filesystem: Arc<sftp::Memory>,
    publish_gate: Arc<AsyncGate>,
    reset_mode: AtomicUsize,
    reset_gate: AsyncGate,
    reset_probe_gate: Arc<AsyncGate>,
}

#[derive(Default)]
struct AsyncGate {
    blocked: std::sync::atomic::AtomicBool,
    entered: std::sync::Mutex<usize>,
    changed: std::sync::Condvar,
    release: Notify,
}
impl AsyncGate {
    fn block(&self) {
        self.blocked.store(true, Ordering::Release);
    }
    fn release(&self) {
        self.blocked.store(false, Ordering::Release);
        self.release.notify_waiters();
    }
    async fn enter(&self) {
        {
            let mut entered = self.entered.lock().unwrap();
            *entered += 1;
            self.changed.notify_all();
        }
        loop {
            let changed = self.release.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if !self.blocked.load(Ordering::Acquire) {
                break;
            }
            changed.await;
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
            "expected gate count {count}; observed {}",
            *entered
        );
    }
}
struct TestServer {
    observed: Arc<Observed>,
    connection_id: usize,
    accepted_key: Option<PublicKey>,
    channels: HashSet<ChannelId>,
    resetting: HashSet<ChannelId>,
    shells: HashSet<ChannelId>,
    buffers: HashMap<ChannelId, Vec<u8>>,
    streams: HashMap<ChannelId, Channel<Msg>>,
}

impl server::Handler for TestServer {
    type Error = russh::Error;

    async fn auth_password(&mut self, username: &str, password: &str) -> Result<Auth, Self::Error> {
        self.observed
            .password_attempts
            .fetch_add(1, Ordering::SeqCst);
        Ok(if username == "test" && password == "local-test-password" {
            Auth::Accept
        } else {
            Auth::reject()
        })
    }

    async fn auth_publickey(
        &mut self,
        username: &str,
        key: server::VerifiedPublicKey<'_>,
    ) -> Result<Auth, Self::Error> {
        self.observed.key_attempts.fetch_add(1, Ordering::SeqCst);
        Ok(
            if username == "test"
                && self
                    .accepted_key
                    .as_ref()
                    .is_some_and(|accepted| accepted.key_data() == key.public_key.key_data())
            {
                Auth::Accept
            } else {
                Auth::reject()
            },
        )
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        reply: server::ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.streams.insert(channel.id(), channel);
        reply.accept().await;
        Ok(())
    }

    async fn channel_close(
        &mut self,
        channel: ChannelId,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        if self.channels.remove(&channel) {
            self.observed
                .gpascii_channels
                .fetch_sub(1, Ordering::AcqRel);
            self.observed
                .closed_gpascii_channels
                .fetch_add(1, Ordering::AcqRel);
        }
        self.buffers.remove(&channel);
        self.streams.remove(&channel);
        Ok(())
    }

    async fn exec_request(
        &mut self,
        channel: ChannelId,
        command: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        if command == b"gpascii -2" {
            let opened = self
                .observed
                .gpascii_channels
                .fetch_add(1, Ordering::AcqRel)
                + 1;
            let limit = self.observed.max_gpascii_channels.load(Ordering::Acquire);
            if limit != 0 && opened > limit {
                self.observed
                    .gpascii_channels
                    .fetch_sub(1, Ordering::AcqRel);
                session.channel_failure(channel)?;
                return Ok(());
            }
        }
        session.channel_success(channel)?;
        match command {
            b"gpascii -2" => {
                self.channels.insert(channel);
                session.data(channel, b"STDIN Open for ASCII Input\r\n".to_vec())?;
            }
            command if command.starts_with(b"/bin/mv -T -- ") => {
                let command = String::from_utf8_lossy(command);
                let args = sftp::quoted_args(&command[13..]);
                let filesystem = self.observed.filesystem.clone();
                let gate = self.observed.publish_gate.clone();
                let handle = session.handle();
                tokio::spawn(async move {
                    gate.enter().await;
                    let success = args.len() == 2 && filesystem.publish(&args[0], &args[1]);
                    let _ = handle
                        .exit_status_request(channel, if success { 0 } else { 1 })
                        .await;
                    let _ = handle.eof(channel).await;
                    let _ = handle.close(channel).await;
                });
            }
            b"exec-with-status" => {
                session.extended_data(channel, 1, b"err".to_vec())?;
                session.data(channel, b"out".to_vec())?;
                // The exit status can follow EOF; it must not be lost.
                session.eof(channel)?;
                session.exit_status_request(channel, 7)?;
                session.close(channel)?;
            }
            b"exec-without-status" => {
                session.data(channel, b"partial".to_vec())?;
                session.eof(channel)?;
                session.close(channel)?;
            }
            b"exec-overflow" => {
                session.extended_data(channel, 1, b"123456".to_vec())?;
                session.data(channel, b"abcdef".to_vec())?;
                session.exit_status_request(channel, 0)?;
                session.close(channel)?;
            }
            _ => {
                session.data(channel, b"ok".to_vec())?;
                session.exit_status_request(channel, 0)?;
                session.close(channel)?;
            }
        }
        Ok(())
    }

    async fn subsystem_request(
        &mut self,
        channel: ChannelId,
        name: &str,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        if name != "sftp" {
            session.channel_failure(channel)?;
            return Ok(());
        }
        session.channel_success(channel)?;
        let stream = self.streams.remove(&channel).unwrap();
        let filesystem = self.observed.filesystem.clone();
        tokio::spawn(async move {
            russh_sftp::server::run(stream.into_stream(), sftp::MemorySession::new(filesystem))
                .await;
        });
        Ok(())
    }
    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.shells.insert(channel);
        session.channel_success(channel)?;
        Ok(())
    }

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        if self.shells.contains(&channel) {
            let buffer = self.buffers.entry(channel).or_default();
            buffer.extend_from_slice(data);
            if !buffer.ends_with(b"\n") {
                return Ok(());
            }
            let wire = String::from_utf8_lossy(buffer).into_owned();
            let begin = marker(&wire, "__PMAC_BEGIN_");
            let end = marker(&wire, "__PMAC_END_");
            let stderr_end = marker(&wire, "__PMAC_STDERR_END_");
            buffer.clear();
            session.data(channel, format!("{begin}\nkept\n").into_bytes())?;
            session.data(channel, format!("{end}:0\n").into_bytes())?;
            if !wire.contains("printf quiet") {
                session.extended_data(channel, 1, b"warn".to_vec())?;
            }
            session.extended_data(channel, 1, stderr_end.into_bytes())?;
            return Ok(());
        }
        if !self.channels.contains(&channel) {
            return Ok(());
        }
        let buffer = self.buffers.entry(channel).or_default();
        buffer.extend_from_slice(data);
        while let Some(end) = buffer.iter().position(|&byte| byte == b'\n') {
            let command = String::from_utf8_lossy(&buffer.drain(..=end).collect::<Vec<_>>())
                .trim()
                .to_owned();
            self.observed.commands.lock().unwrap().push(command.clone());
            self.observed
                .command_channels
                .lock()
                .unwrap()
                .push((channel, command.clone()));
            let gated = self.observed.gated_command.lock().unwrap().as_str() == command;
            if gated {
                self.observed.command_gate.enter().await;
            }
            let program_key = (self.connection_id, channel);
            match command.as_str() {
                "open prog 1" | "open plc 1" => {
                    self.observed
                        .open_programs
                        .lock()
                        .unwrap()
                        .insert(program_key, Vec::new());
                    session.data(channel, b"\x06".to_vec())?;
                }
                "close" => {
                    if let Some(lines) = self
                        .observed
                        .open_programs
                        .lock()
                        .unwrap()
                        .remove(&program_key)
                    {
                        *self.observed.program_lines.lock().unwrap() = lines;
                    }
                    session.data(channel, b"\x06".to_vec())?;
                }
                "list prog 1" | "list plc 1" => {
                    let lines = self.observed.program_lines.lock().unwrap().join("\r\n");
                    session.data(
                        channel,
                        format!("{command}\r\n{lines}\r\n\x06").into_bytes(),
                    )?;
                }
                "$$$" | "$$$***" => {
                    self.resetting.insert(channel);
                    self.observed.reset_gate.enter().await;
                    match self.observed.reset_mode.load(Ordering::Acquire) {
                        1 | 4 | 6 => {
                            for fragment in [
                                b"STDIN Open for ASCII In".as_slice(),
                                b"put\r".as_slice(),
                                b"\n".as_slice(),
                            ] {
                                session.data(channel, fragment.to_vec())?;
                            }
                        }
                        2 => {}
                        3 => {
                            session.eof(channel)?;
                            session.close(channel)?;
                        }
                        5 => {
                            session.data(
                                channel,
                                b"diagnostic mentions STDIN Open for ASCII Input\n".to_vec(),
                            )?;
                        }
                        _ => {
                            session.data(channel, b"\x06".to_vec())?;
                        }
                    }
                }
                "Sys.ServoCount" if self.resetting.contains(&channel) => {
                    let mode = self.observed.reset_mode.load(Ordering::Acquire);
                    let gate = self.observed.reset_probe_gate.clone();
                    let handle = session.handle();
                    tokio::spawn(async move {
                        gate.enter().await;
                        if mode == 4 {
                            return;
                        }
                        let reply = if mode == 6 {
                            b"Motor[1].Pos=70173\x06".as_slice()
                        } else {
                            b"\x06\r\nSTDIN Open for ASCII Input\nSys.ServoCount=70173\r\n\x06\r\n"
                                .as_slice()
                        };
                        for fragment in reply.chunks(3) {
                            if handle.data(channel, fragment.to_vec()).await.is_err() {
                                break;
                            }
                            tokio::task::yield_now().await;
                        }
                    });
                }
                "Sys.ServoCount" => {
                    session.data(channel, b"Sys.ServoCount\r\n123\r\n\x06\r\n".to_vec())?
                }
                "P1..8192" => {
                    let handle = session.handle();
                    tokio::spawn(async move {
                        let mut wire = b"P1..8192\r\n".to_vec();
                        wire.extend(parameter_range_response());
                        wire.extend(b"\r\n\x06\r\n");
                        let mut offset = 0;
                        for chunk_size in [1, 127, 4093, 23, 8192].into_iter().cycle() {
                            if offset == wire.len() {
                                break;
                            }
                            let end = (offset + chunk_size).min(wire.len());
                            if handle
                                .data(channel, wire[offset..end].to_vec())
                                .await
                                .is_err()
                            {
                                break;
                            }
                            offset = end;
                            tokio::task::yield_now().await;
                        }
                    });
                }
                "list prog 100" => {
                    let handle = session.handle();
                    tokio::spawn(async move {
                        let mut body = b"list prog 100\r\n".to_vec();
                        body.extend(program_listing());
                        body.extend(b"\r\n\x06\r\n");
                        for fragment in body.chunks(7) {
                            if handle.data(channel, fragment.to_vec()).await.is_err() {
                                break;
                            }
                            tokio::task::yield_now().await;
                        }
                    });
                }
                "#2p" => session.data(channel, b"ERR003\r\n\x15".to_vec())?,
                "#5p" => {
                    session.extended_data(channel, 1, b"diagnostic".to_vec())?;
                    session.data(channel, b"5.0\r\n\x06".to_vec())?;
                }
                "#3p" => {
                    session.data(channel, b"incomplete".to_vec())?;
                    session.eof(channel)?;
                    session.close(channel)?;
                }
                "#4p" => session.data(channel, b"1\x06old\x06".to_vec())?,
                "#9p" => {
                    let observed = self.observed.clone();
                    let handle = session.handle();
                    tokio::spawn(async move {
                        observed.delayed_started.notify_one();
                        observed.delayed_release.notified().await;
                        let _ = handle.data(channel, b"late\x06".to_vec()).await;
                    });
                }
                _ if self
                    .observed
                    .open_programs
                    .lock()
                    .unwrap()
                    .contains_key(&program_key) =>
                {
                    self.observed
                        .open_programs
                        .lock()
                        .unwrap()
                        .get_mut(&program_key)
                        .unwrap()
                        .push(command.clone());
                    session.data(channel, b"\x06".to_vec())?;
                }
                _ => {
                    let handle = session.handle();
                    tokio::spawn(async move {
                        for fragment in [
                            b"#1p\r".as_slice(),
                            b"\n12".as_slice(),
                            b".5\r\n".as_slice(),
                            b"\x06\r\n".as_slice(),
                        ] {
                            if handle.data(channel, fragment.to_vec()).await.is_err() {
                                break;
                            }
                            tokio::task::yield_now().await;
                        }
                    });
                }
            }
        }
        Ok(())
    }
}

fn marker(wire: &str, prefix: &str) -> String {
    let start = wire
        .find(prefix)
        .expect("shell command contains its marker");
    let end = wire[start + prefix.len()..].find("__").unwrap() + start + prefix.len() + 2;
    wire[start..end].to_owned()
}

struct Fixture {
    runtime: Arc<Runtime>,
    client: Arc<Client>,
    address: SocketAddr,
    host_key: [u8; 32],
    host_public: PublicKey,
    observed: Arc<Observed>,
    listener: tokio::task::JoinHandle<()>,
}

impl Fixture {
    fn new(accepted_key: Option<PublicKey>) -> Self {
        let runtime = Runtime::new(2).unwrap();
        let socket = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        socket.set_nonblocking(true).unwrap();
        let address = socket.local_addr().unwrap();
        let key = PrivateKey::random(&mut rand10::rng(), Algorithm::Ed25519).unwrap();
        let host_public = key.public_key().clone();
        let host_key: [u8; 32] = Sha256::digest(host_public.public_key_bytes()).into();
        let config = Arc::new(server::Config {
            keys: vec![key],
            auth_rejection_time: Duration::ZERO,
            auth_rejection_time_initial: Some(Duration::ZERO),
            ..server::Config::default()
        });
        let observed = Arc::new(Observed::default());
        let server_observed = observed.clone();
        let listener = runtime.spawn(async move {
            let listener = tokio::net::TcpListener::from_std(socket).unwrap();
            while let Ok((stream, _)) = listener.accept().await {
                let config = config.clone();
                let observed = server_observed.clone();
                let accepted_key = accepted_key.clone();
                let connection_id = observed.next_connection.fetch_add(1, Ordering::AcqRel);
                tokio::spawn(async move {
                    let handler = TestServer {
                        observed,
                        connection_id,
                        accepted_key,
                        channels: HashSet::new(),
                        resetting: HashSet::new(),
                        shells: HashSet::new(),
                        buffers: HashMap::new(),
                        streams: HashMap::new(),
                    };
                    if let Ok(session) = server::run_stream(config, stream, handler).await {
                        let _ = session.await;
                    }
                });
            }
        });
        let client = runtime
            .create_client(Arc::new(RusshTransport), ClientConfig::default())
            .unwrap();
        Self {
            runtime,
            client,
            address,
            host_key,
            host_public,
            observed,
            listener,
        }
    }

    fn options(&self) -> ConnectOptions {
        let mut options = ConnectOptions::new("127.0.0.1", "test");
        options.port = self.address.port();
        options.password = Some(Zeroizing::new("local-test-password".into()));
        options.host_key = HostKeyPolicy::PinnedSha256(self.host_key);
        options.tcp_probe_timeout = Duration::from_secs(2);
        options
    }

    fn run(&self, operation: Operation) -> Result<OperationResult, Error> {
        self.run_with_options(
            operation,
            RequestOptions {
                timeout_ms: 10_000,
                ..RequestOptions::default()
            },
        )
    }

    fn run_with_options(
        &self,
        operation: Operation,
        options: RequestOptions,
    ) -> Result<OperationResult, Error> {
        let request = self
            .client
            .submit(self.runtime.next_id(), operation, options)
            .unwrap();
        self.run_result(request)
    }
    fn run_result(&self, request: Arc<pmac_core::Request>) -> Result<OperationResult, Error> {
        let started = std::time::Instant::now();
        loop {
            let remaining = Duration::from_secs(15).saturating_sub(started.elapsed());
            assert!(
                !remaining.is_zero(),
                "request must complete within the test budget"
            );
            if request
                .wait(remaining.as_millis().min(u128::from(u32::MAX)) as u32)
                .completed
            {
                break;
            }
        }
        request.result().unwrap().as_ref().clone()
    }

    fn connect(&self) {
        self.run(Operation::Connect(self.options())).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.runtime.shutdown_clients(5_000);
        self.listener.abort();
        self.observed.delayed_release.notify_one();
    }
}

#[test]
fn password_authentication_banner_and_fragmented_gpascii_roundtrip() {
    let fixture = Fixture::new(None);
    fixture.connect();
    let response = fixture
        .run(Operation::Command(Command::raw("#1p").unwrap()))
        .unwrap();
    assert_eq!(response.response, b"12.5");
    assert_eq!(fixture.observed.password_attempts.load(Ordering::SeqCst), 1);
    assert!(fixture.client.is_connected());
}

#[test]
fn rejected_password_and_host_identity_are_distinct_failures() {
    let fixture = Fixture::new(None);
    let mut wrong_password = fixture.options();
    wrong_password.password = Some(Zeroizing::new("wrong".into()));
    assert_eq!(
        fixture
            .run(Operation::Connect(wrong_password))
            .unwrap_err()
            .code,
        ErrorCode::Authentication
    );
    let fixture = Fixture::new(None);
    let mut wrong_host = fixture.options();
    wrong_host.host_key = HostKeyPolicy::PinnedSha256([0; 32]);
    assert_eq!(
        fixture
            .run(Operation::Connect(wrong_host))
            .unwrap_err()
            .code,
        ErrorCode::HostKey
    );
}

#[test]
fn gpascii_nak_preserves_connection_but_incomplete_eof_retires_it() {
    let fixture = Fixture::new(None);
    fixture.connect();
    let rejected = fixture
        .run(Operation::Command(Command::raw("#2p").unwrap()))
        .unwrap_err();
    assert_eq!(rejected.code, ErrorCode::RemoteError);
    assert_eq!(rejected.certainty, ExecutionCertainty::Confirmed);
    fixture
        .run(Operation::Command(Command::raw("#1p").unwrap()))
        .unwrap();
    let incomplete = fixture
        .run(Operation::Command(Command::raw("#3p").unwrap()))
        .unwrap_err();
    assert_eq!(incomplete.code, ErrorCode::Disconnected);
    assert_eq!(incomplete.certainty, ExecutionCertainty::MayHaveExecuted);
    assert!(!fixture.client.is_connected());
}

#[test]
fn multiple_gpascii_frames_are_rejected_before_result_delivery() {
    let fixture = Fixture::new(None);
    fixture.connect();
    let error = fixture
        .run(Operation::Command(Command::raw("#4p").unwrap()))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::ProtocolError);
    assert!(!fixture.client.is_connected());
}

#[test]
fn exec_preserves_stderr_and_exit_status_after_eof() {
    let fixture = Fixture::new(None);
    fixture.connect();
    let result = fixture
        .run(Operation::Shell {
            command: "exec-with-status".into(),
            persistent: false,
        })
        .unwrap();
    assert_eq!(result.response, b"out");
    assert_eq!(result.stderr, b"err");
    assert_eq!(result.exit_code, Some(7));
}

#[test]
fn exec_without_exit_status_is_unknown_and_does_not_retire_control() {
    let fixture = Fixture::new(None);
    fixture.connect();
    let error = fixture
        .run(Operation::Shell {
            command: "exec-without-status".into(),
            persistent: false,
        })
        .unwrap_err();
    assert_eq!(error.certainty, ExecutionCertainty::MayHaveExecuted);
    assert_eq!(error.code, ErrorCode::Disconnected);
    fixture
        .run(Operation::Command(Command::raw("#1p").unwrap()))
        .unwrap();
}

#[test]
fn stdout_and_stderr_share_one_output_budget() {
    let fixture = Fixture::new(None);
    fixture.connect();
    let error = fixture
        .run_with_options(
            Operation::Shell {
                command: "exec-overflow".into(),
                persistent: false,
            },
            RequestOptions {
                max_response_bytes: 8,
                timeout_ms: 10_000,
                ..RequestOptions::default()
            },
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::ResourceLimit);
    fixture
        .run(Operation::Command(Command::raw("#1p").unwrap()))
        .unwrap();
}

#[test]
fn persistent_shell_waits_for_stderr_boundary_and_isolates_requests() {
    let fixture = Fixture::new(None);
    fixture.connect();
    let first = fixture
        .run(Operation::Shell {
            command: "printf kept".into(),
            persistent: true,
        })
        .unwrap();
    assert_eq!(first.response, b"kept");
    assert_eq!(first.stderr, b"warn");
    assert_eq!(first.exit_code, Some(0));
    let second = fixture
        .run(Operation::Shell {
            command: "printf quiet".into(),
            persistent: true,
        })
        .unwrap();
    assert_eq!(second.response, b"kept");
    assert!(second.stderr.is_empty());
    assert_eq!(second.exit_code, Some(0));
}

#[test]
fn calling_threads_use_distinct_channels_and_open_program_keeps_its_channel() {
    let fixture = Fixture::new(None);
    fixture.connect();
    for command in ["open prog 1", "dwell 10"] {
        fixture
            .run(Operation::Command(Command::raw(command).unwrap()))
            .unwrap();
    }
    std::thread::scope(|scope| {
        scope.spawn(|| {
            fixture
                .run(Operation::Command(
                    Command::raw("Motor[1].ServoCtrl=1").unwrap(),
                ))
                .unwrap();
        });
    });
    fixture
        .run(Operation::Command(Command::raw("close").unwrap()))
        .unwrap();
    let channels = fixture.observed.command_channels.lock().unwrap();
    let channel = |command| {
        channels
            .iter()
            .find(|(_, observed)| observed == command)
            .unwrap()
            .0
    };
    let owner = channel("open prog 1");
    assert_eq!(channel("dwell 10"), owner);
    assert_eq!(channel("close"), owner);
    assert_ne!(channel("Motor[1].ServoCtrl=1"), owner);
    drop(channels);
    let listed = fixture
        .run(Operation::Command(Command::raw("list prog 1").unwrap()))
        .unwrap();
    assert!(listed
        .response
        .windows(b"dwell 10".len())
        .any(|bytes| bytes == b"dwell 10"));
    assert!(!listed
        .response
        .windows(b"Motor[1].ServoCtrl=1".len())
        .any(|bytes| bytes == b"Motor[1].ServoCtrl=1"));
}

#[test]
fn mt06_open_and_close_response_interleavings_preserve_program_ownership() {
    for gated in ["open prog 1", "close"] {
        let fixture = Fixture::new(None);
        let mut options = fixture.options();
        options.control_pool_size = 2;
        fixture.run(Operation::Connect(options)).unwrap();
        fixture
            .observed
            .gated_command
            .lock()
            .unwrap()
            .push_str(gated);
        fixture.observed.command_gate.block();
        std::thread::scope(|scope| {
            let owner = scope.spawn(|| {
                if gated == "close" {
                    for command in ["open prog 1", "dwell 10"] {
                        fixture
                            .run(Operation::Command(Command::raw(command).unwrap()))
                            .unwrap();
                    }
                }
                fixture
                    .run(Operation::Command(Command::raw(gated).unwrap()))
                    .unwrap();
                if gated == "open prog 1" {
                    for command in ["dwell 10", "close"] {
                        fixture
                            .run(Operation::Command(Command::raw(command).unwrap()))
                            .unwrap();
                    }
                }
            });
            fixture.observed.command_gate.wait(1);
            fixture
                .run(Operation::Command(
                    Command::raw("Motor[1].ServoCtrl=1").unwrap(),
                ))
                .unwrap();
            fixture.observed.command_gate.release();
            owner.join().unwrap();
        });
        let listed = fixture
            .run(Operation::Command(Command::raw("list prog 1").unwrap()))
            .unwrap();
        assert!(listed
            .response
            .windows(b"dwell 10".len())
            .any(|bytes| bytes == b"dwell 10"));
        assert!(!listed
            .response
            .windows(b"Motor[1].ServoCtrl=1".len())
            .any(|bytes| bytes == b"Motor[1].ServoCtrl=1"));
    }
}

#[test]
fn queued_program_sequence_survives_owner_exit_and_interleaved_channel_reclamation() {
    let fixture = Fixture::new(None);
    fixture.connect();
    fixture
        .run(Operation::Command(Command::raw("#1p").unwrap()))
        .unwrap();
    fixture
        .observed
        .gated_command
        .lock()
        .unwrap()
        .push_str("#1p");
    fixture.observed.command_gate.block();
    let blocker = fixture
        .client
        .submit(
            fixture.runtime.next_id(),
            Operation::Command(Command::raw("#1p").unwrap()),
            RequestOptions::default(),
        )
        .unwrap();
    fixture.observed.command_gate.wait(1);
    let (open_sent, open_received) = std::sync::mpsc::channel();
    let (interleave_sent, interleave_received) = std::sync::mpsc::channel();
    std::thread::scope(|scope| {
        let fixture = &fixture;
        let owner = scope.spawn(move || {
            fixture
                .client
                .submit(
                    fixture.runtime.next_id(),
                    Operation::Command(Command::raw("open plc 1").unwrap()),
                    RequestOptions::default(),
                )
                .unwrap();
            open_sent.send(()).unwrap();
            interleave_received.recv().unwrap();
            for command in ["P1=42", "close"] {
                fixture
                    .client
                    .submit(
                        fixture.runtime.next_id(),
                        Operation::Command(Command::raw(command).unwrap()),
                        RequestOptions::default(),
                    )
                    .unwrap();
            }
        });
        let interleaved = scope.spawn(move || {
            open_received.recv().unwrap();
            let request = fixture
                .client
                .submit(
                    fixture.runtime.next_id(),
                    Operation::Command(Command::raw("#1p").unwrap()),
                    RequestOptions::default(),
                )
                .unwrap();
            interleave_sent.send(()).unwrap();
            request
        });
        owner.join().unwrap();
        let interleaved = interleaved.join().unwrap();
        fixture.observed.command_gate.release();
        fixture.run_result(blocker).unwrap();
        fixture.run_result(interleaved).unwrap();
    });
    let listed = fixture
        .run(Operation::Command(Command::raw("list plc 1").unwrap()))
        .unwrap();
    assert!(listed
        .response
        .split(|byte| matches!(byte, b'\r' | b'\n'))
        .any(|line| line == b"P1=42"));
    assert_eq!(
        fixture.observed.program_lines.lock().unwrap().as_slice(),
        ["P1=42"]
    );
}

#[test]
fn cancellation_and_timeout_during_channel_initialization_preserve_the_member() {
    for cancel in [true, false] {
        let fixture = Fixture::new(None);
        fixture.connect();
        fixture
            .run(Operation::Command(Command::raw("#1p").unwrap()))
            .unwrap();
        fixture
            .observed
            .gated_command
            .lock()
            .unwrap()
            .push_str("Sys.ServoCount");
        fixture.observed.command_gate.block();
        let request = std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    fixture.client.submit(
                        fixture.runtime.next_id(),
                        Operation::Command(Command::raw("open plc 1").unwrap()),
                        RequestOptions {
                            timeout_ms: if cancel { 10_000 } else { 100 },
                            ..RequestOptions::default()
                        },
                    )
                })
                .join()
                .unwrap()
        })
        .unwrap();
        fixture.observed.command_gate.wait(1);
        if cancel {
            request.cancel();
        }
        let error = fixture.run_result(request).unwrap_err();
        assert_eq!(
            error.code,
            if cancel {
                ErrorCode::Cancelled
            } else {
                ErrorCode::ExecTimeout
            }
        );
        assert_eq!(error.certainty, ExecutionCertainty::NotSent);
        assert!(!error.fatal_transport);
        assert!(fixture.client.is_connected());
        fixture.observed.command_gate.release();
        fixture
            .run(Operation::Command(Command::raw("#1p").unwrap()))
            .unwrap();
        assert!(!fixture
            .observed
            .commands
            .lock()
            .unwrap()
            .iter()
            .any(|command| command == "open plc 1"));
    }
}

#[test]
fn mt08_channel_exhaustion_never_borrows_another_threads_channel() {
    let fixture = Fixture::new(None);
    fixture
        .observed
        .max_gpascii_channels
        .store(1, Ordering::Release);
    fixture.connect();
    fixture
        .run(Operation::Command(Command::raw("open prog 1").unwrap()))
        .unwrap();
    let error = std::thread::scope(|scope| {
        scope
            .spawn(|| {
                fixture.run(Operation::Command(
                    Command::raw("Motor[1].ServoCtrl=1").unwrap(),
                ))
            })
            .join()
            .unwrap()
    })
    .unwrap_err();
    assert!(matches!(
        error.code,
        ErrorCode::Disconnected | ErrorCode::ConnectError
    ));
    assert_eq!(error.certainty, ExecutionCertainty::NotSent);
    assert!(!error.fatal_transport);
    assert!(fixture.client.is_connected());
    assert!(!fixture
        .observed
        .commands
        .lock()
        .unwrap()
        .iter()
        .any(|command| command == "Motor[1].ServoCtrl=1"));
    fixture
        .run(Operation::Command(Command::raw("close").unwrap()))
        .unwrap();
}

#[test]
fn mt08_exited_threads_release_channels_before_new_threads_are_admitted() {
    let fixture = Fixture::new(None);
    fixture.connect();
    fixture
        .run(Operation::Command(Command::raw("#1p").unwrap()))
        .unwrap();
    for _ in 0..70 {
        std::thread::scope(|scope| {
            scope
                .spawn(|| fixture.run(Operation::Command(Command::raw("#1p").unwrap())))
                .join()
                .unwrap()
                .unwrap();
        });
    }
    fixture
        .run(Operation::Command(Command::raw("#1p").unwrap()))
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while fixture
        .observed
        .closed_gpascii_channels
        .load(Ordering::Acquire)
        < 70
    {
        assert!(
            std::time::Instant::now() < deadline,
            "exited-thread GPASCII channels were not closed"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        fixture
            .observed
            .closed_gpascii_channels
            .load(Ordering::Acquire),
        70
    );
    assert_eq!(fixture.observed.gpascii_channels.load(Ordering::Acquire), 1);
}

#[test]
fn mt08_local_channel_quota_rejects_a_live_overflow_thread_without_retiring_the_member() {
    let fixture = Fixture::new(None);
    fixture.connect();
    fixture
        .run(Operation::Command(Command::raw("#1p").unwrap()))
        .unwrap();
    let ready = std::sync::Barrier::new(64);
    let release = std::sync::Barrier::new(64);
    std::thread::scope(|scope| {
        let fixture = &fixture;
        for _ in 0..63 {
            let ready = &ready;
            let release = &release;
            scope.spawn(move || {
                fixture
                    .run(Operation::Command(Command::raw("#1p").unwrap()))
                    .unwrap();
                ready.wait();
                release.wait();
            });
        }
        ready.wait();
        let error = scope
            .spawn(|| {
                fixture.run(Operation::Command(
                    Command::raw("Motor[1].ServoCtrl=1").unwrap(),
                ))
            })
            .join()
            .unwrap()
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::ResourceLimit);
        assert_eq!(error.certainty, ExecutionCertainty::NotSent);
        assert!(!error.fatal_transport);
        release.wait();
    });
    assert!(fixture.client.is_connected());
    assert!(!fixture
        .observed
        .commands
        .lock()
        .unwrap()
        .iter()
        .any(|command| command == "Motor[1].ServoCtrl=1"));
    fixture
        .run(Operation::Command(Command::raw("#1p").unwrap()))
        .unwrap();
}

#[test]
fn readiness_response_budget_is_independent_of_the_business_command_budget() {
    let fixture = Fixture::new(None);
    fixture.connect();
    fixture
        .run(Operation::Command(Command::raw("#1p").unwrap()))
        .unwrap();
    let response = std::thread::scope(|scope| {
        scope
            .spawn(|| {
                fixture.run_with_options(
                    Operation::Command(Command::raw("open prog 1").unwrap()),
                    RequestOptions {
                        max_response_bytes: 1,
                        timeout_ms: 10_000,
                        ..RequestOptions::default()
                    },
                )
            })
            .join()
            .unwrap()
    })
    .unwrap();
    assert!(response.response.is_empty());
    assert!(fixture.client.is_connected());
    fixture
        .run(Operation::Command(Command::raw("#1p").unwrap()))
        .unwrap();
}

struct TestFile(std::path::PathBuf);
impl TestFile {
    fn new(text: &[u8]) -> Self {
        use std::io::Write;
        let path = std::env::temp_dir().join(format!(
            "pmac-russh-test-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .unwrap();
        file.write_all(text).unwrap();
        Self(path)
    }
}
impl Drop for TestFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn encrypted_private_key_authentication_never_falls_back_to_password() {
    let private = PrivateKey::random(&mut rand10::rng(), Algorithm::Ed25519).unwrap();
    let fixture = Fixture::new(Some(private.public_key().clone()));
    let encrypted = private
        .encrypt(&mut rand10::rng(), "local-key-passphrase")
        .unwrap();
    let pem = encrypted
        .to_openssh(russh::keys::ssh_key::LineEnding::LF)
        .unwrap();
    let key_file = TestFile::new(pem.as_bytes());
    let mut options = fixture.options();
    options.private_key = Some(key_file.0.clone());
    options.passphrase = Some(Zeroizing::new("local-key-passphrase".into()));
    // A deliberately wrong password proves the key path is used.
    options.password = Some(Zeroizing::new("must-not-be-used".into()));
    fixture.run(Operation::Connect(options)).unwrap();
    fixture
        .run(Operation::Command(Command::raw("#1p").unwrap()))
        .unwrap();
    assert_eq!(fixture.observed.password_attempts.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.observed.key_attempts.load(Ordering::SeqCst), 1);
}

#[test]
fn rejected_key_does_not_fall_back_to_a_valid_password() {
    let private = PrivateKey::random(&mut rand10::rng(), Algorithm::Ed25519).unwrap();
    let fixture = Fixture::new(None);
    let pem = private
        .to_openssh(russh::keys::ssh_key::LineEnding::LF)
        .unwrap();
    let key_file = TestFile::new(pem.as_bytes());
    let mut options = fixture.options();
    options.private_key = Some(key_file.0.clone());
    let error = fixture.run(Operation::Connect(options)).unwrap_err();
    assert_eq!(error.code, ErrorCode::Authentication);
    assert_eq!(fixture.observed.password_attempts.load(Ordering::SeqCst), 0);
}

#[test]
fn known_hosts_matches_the_exact_host_and_port() {
    let fixture = Fixture::new(None);
    let contents = format!(
        "[127.0.0.1]:{} {}\n",
        fixture.address.port(),
        fixture.host_public.to_openssh().unwrap()
    );
    let hosts = TestFile::new(contents.as_bytes());
    let mut options = fixture.options();
    options.host_key = HostKeyPolicy::KnownHosts(hosts.0.clone());
    fixture.run(Operation::Connect(options)).unwrap();
    assert_eq!(fixture.observed.password_attempts.load(Ordering::SeqCst), 1);
}

#[test]
fn unrelated_known_hosts_entry_cannot_authenticate_the_server() {
    let fixture = Fixture::new(None);
    let contents = format!(
        "[127.0.0.2]:{} {}\n",
        fixture.address.port(),
        fixture.host_public.to_openssh().unwrap()
    );
    let hosts = TestFile::new(contents.as_bytes());
    let mut options = fixture.options();
    options.host_key = HostKeyPolicy::KnownHosts(hosts.0.clone());
    let error = fixture.run(Operation::Connect(options)).unwrap_err();
    assert_eq!(error.code, ErrorCode::HostKey);
    assert_eq!(fixture.observed.password_attempts.load(Ordering::SeqCst), 0);
}

#[test]
fn cancelled_control_command_cannot_deliver_a_late_frame_to_a_later_request() {
    let fixture = Fixture::new(None);
    fixture.connect();
    let request = fixture
        .client
        .submit(
            fixture.runtime.next_id(),
            Operation::Command(Command::raw("#9p").unwrap()),
            RequestOptions {
                timeout_ms: 10_000,
                ..RequestOptions::default()
            },
        )
        .unwrap();
    let observed = fixture.observed.clone();
    let (sent, received) = std::sync::mpsc::channel();
    fixture.runtime.spawn(async move {
        observed.delayed_started.notified().await;
        sent.send(()).unwrap();
    });
    received.recv_timeout(Duration::from_secs(5)).unwrap();
    request.cancel();
    assert!(request.wait(5_000).completed);
    let error = request
        .result()
        .unwrap()
        .as_ref()
        .as_ref()
        .unwrap_err()
        .clone();
    assert_eq!(error.code, ErrorCode::Cancelled);
    assert_eq!(error.certainty, ExecutionCertainty::MayHaveExecuted);
    fixture.observed.delayed_release.notify_one();
    let next = fixture
        .run(Operation::Command(Command::raw("#1p").unwrap()))
        .unwrap_err();
    assert_eq!(next.certainty, ExecutionCertainty::NotSent);
    assert!(!fixture
        .observed
        .commands
        .lock()
        .unwrap()
        .iter()
        .any(|command| command == "#1p"));
}

mod sftp;

#[test]
fn gpascii_stderr_is_preserved_outside_the_response_frame() {
    let fixture = Fixture::new(None);
    fixture.connect();
    let response = fixture
        .run(Operation::Command(Command::raw("#5p").unwrap()))
        .unwrap();
    assert_eq!(response.response, b"5.0");
    assert_eq!(response.stderr, b"diagnostic");
}

#[test]
fn unspecified_host_identity_policy_is_rejected_before_connecting() {
    let fixture = Fixture::new(None);
    let mut options = fixture.options();
    options.host_key = HostKeyPolicy::Unspecified;
    let error = fixture.run(Operation::Connect(options)).unwrap_err();
    assert_eq!(error.code, ErrorCode::HostKey);
    assert_eq!(
        fixture.observed.password_attempts.load(Ordering::Acquire),
        0
    );
    assert_eq!(fixture.observed.key_attempts.load(Ordering::Acquire), 0);
}

#[test]
fn rsa_private_key_loads_and_authenticates_with_a_real_ssh_signature() {
    let keypair =
        russh::keys::ssh_key::private::RsaKeypair::random(&mut rand10::rng(), 2048).unwrap();
    let private = PrivateKey::new(keypair.into(), "ephemeral PPMAC RSA regression key").unwrap();
    let fixture = Fixture::new(Some(private.public_key().clone()));
    let pem = private
        .to_openssh(russh::keys::ssh_key::LineEnding::LF)
        .unwrap();
    let key_file = TestFile::new(pem.as_bytes());
    let mut options = fixture.options();
    options.private_key = Some(key_file.0.clone());
    options.password = Some(Zeroizing::new("must-not-be-used".into()));
    fixture.run(Operation::Connect(options)).unwrap();
    assert_eq!(
        fixture
            .run(Operation::Command(Command::raw("#1p").unwrap()))
            .unwrap()
            .response,
        b"12.5"
    );
    assert_eq!(
        fixture.observed.password_attempts.load(Ordering::Acquire),
        0
    );
    assert_eq!(fixture.observed.key_attempts.load(Ordering::Acquire), 1);
}

#[test]
fn known_hosts_public_key_comments_do_not_change_identity_verification() {
    let fixture = Fixture::new(None);
    let mut public = fixture.host_public.clone();
    public.set_comment("controller host key with an inventory comment");
    let contents = format!(
        "[127.0.0.1]:{} {}\n",
        fixture.address.port(),
        public.to_openssh().unwrap()
    );
    let hosts = TestFile::new(contents.as_bytes());
    let mut options = fixture.options();
    options.host_key = HostKeyPolicy::KnownHosts(hosts.0.clone());
    fixture.run(Operation::Connect(options)).unwrap();
    assert_eq!(
        fixture.observed.password_attempts.load(Ordering::Acquire),
        1
    );
}

fn program_listing() -> Vec<u8> {
    let mut body =
        b"open prog 100\r\n  // retained listing\r\n  while (1) {\r\n    dwell 10\r\n  }\r\n// "
            .to_vec();
    body.resize(223, b'x');
    body.extend(b"\r\nclose");
    body
}

#[test]
fn multiline_program_listing_is_preserved_until_its_complete_ack() {
    let fixture = Fixture::new(None);
    fixture.connect();
    let result = fixture
        .run(Operation::Command(Command::raw("list prog 100").unwrap()))
        .unwrap();
    assert_eq!(result.response, program_listing());
    assert_eq!(result.response.len(), 230);
    assert!(result
        .response
        .windows(15)
        .any(|bytes| bytes == b"    dwell 10\r\n "));
    fixture
        .run(Operation::Command(Command::raw("#1p").unwrap()))
        .unwrap();
}

#[test]
fn reset_reopened_input_without_ack_is_confirmed_by_one_read_only_probe() {
    let fixture = Fixture::new(None);
    fixture.observed.reset_mode.store(1, Ordering::Release);
    let mut options = fixture.options();
    options.control_pool_size = 2;
    fixture.run(Operation::Connect(options)).unwrap();
    let result = fixture
        .run(Operation::Command(Command::raw("$$$***").unwrap()))
        .unwrap();
    assert_eq!(result.response, b"STDIN Open for ASCII Input");
    let commands = fixture.observed.commands.lock().unwrap();
    assert_eq!(
        commands
            .iter()
            .filter(|command| command.as_str() == "$$$***")
            .count(),
        1
    );
    assert_eq!(
        commands
            .iter()
            .filter(|command| command.as_str() == "Sys.ServoCount")
            .count(),
        3,
        "two initial lane checks plus exactly one post-reset probe"
    );
    drop(commands);
    assert_eq!(fixture.client.pool_snapshot().ready, 0);
    assert_eq!(fixture.client.pool_snapshot().failed, 2);
    assert_eq!(
        fixture.observed.password_attempts.load(Ordering::Acquire),
        2,
        "no implicit reconnect"
    );
    let error = fixture
        .run(Operation::Command(Command::raw("#1p").unwrap()))
        .unwrap_err();
    assert_eq!(error.certainty, ExecutionCertainty::NotSent);
}

#[test]
fn reset_eof_without_ack_or_probe_confirmation_is_unknown() {
    let fixture = Fixture::new(None);
    fixture.observed.reset_mode.store(3, Ordering::Release);
    fixture.connect();
    let error = fixture
        .run(Operation::Command(Command::raw("$$$").unwrap()))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Disconnected);
    assert_eq!(error.certainty, ExecutionCertainty::MayHaveExecuted);
    assert!(fixture.client.pool_snapshot().unknown_write);
    assert_eq!(fixture.client.pool_snapshot().ready, 0);
}

#[test]
fn reset_silence_or_unrelated_banner_never_becomes_success() {
    for mode in [2, 5] {
        let fixture = Fixture::new(None);
        fixture.observed.reset_mode.store(mode, Ordering::Release);
        fixture.connect();
        let error = fixture
            .run_with_options(
                Operation::Command(Command::raw("$$$***").unwrap()),
                RequestOptions {
                    timeout_ms: 100,
                    ..RequestOptions::default()
                },
            )
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::ExecTimeout);
        assert_eq!(error.certainty, ExecutionCertainty::MayHaveExecuted);
        assert_eq!(
            *fixture.observed.reset_probe_gate.entered.lock().unwrap(),
            0
        );
        assert_eq!(fixture.client.pool_snapshot().ready, 0);
    }
}

#[test]
fn reset_probe_timeout_and_cancellation_cannot_replay_or_reuse_old_lanes() {
    for cancel in [false, true] {
        let fixture = Fixture::new(None);
        fixture.observed.reset_mode.store(4, Ordering::Release);
        fixture.observed.reset_probe_gate.block();
        fixture.connect();
        let request = fixture
            .client
            .submit(
                fixture.runtime.next_id(),
                Operation::Command(Command::raw("$$$***").unwrap()),
                RequestOptions {
                    timeout_ms: if cancel { 5_000 } else { 150 },
                    ..RequestOptions::default()
                },
            )
            .unwrap();
        fixture.observed.reset_probe_gate.wait(1);
        if cancel {
            request.cancel();
        }
        let error = fixture.run_result(request).unwrap_err();
        fixture.observed.reset_probe_gate.release();
        assert_eq!(
            error.code,
            if cancel {
                ErrorCode::Cancelled
            } else {
                ErrorCode::ExecTimeout
            }
        );
        assert_eq!(error.certainty, ExecutionCertainty::MayHaveExecuted);
        assert!(fixture.client.pool_snapshot().unknown_write);
        assert_eq!(fixture.client.pool_snapshot().ready, 0);
        assert_eq!(
            fixture
                .observed
                .commands
                .lock()
                .unwrap()
                .iter()
                .filter(|command| command.as_str() == "$$$***")
                .count(),
            1
        );
        assert_eq!(
            fixture.observed.password_attempts.load(Ordering::Acquire),
            1
        );
    }
}

#[test]
fn reset_probe_must_return_the_expected_counter_identity() {
    let fixture = Fixture::new(None);
    fixture.observed.reset_mode.store(6, Ordering::Release);
    fixture.connect();
    let error = fixture
        .run(Operation::Command(Command::raw("$$$***").unwrap()))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::ProtocolError);
    assert_eq!(error.certainty, ExecutionCertainty::MayHaveExecuted);
    assert!(fixture.client.pool_snapshot().unknown_write);
    assert_eq!(fixture.client.pool_snapshot().ready, 0);
}

fn parameter_range_response() -> Vec<u8> {
    use std::fmt::Write;
    let mut output = String::new();
    for index in 1_u32..=8192 {
        writeln!(output, "P{index}={}", u64::from(index) * 31 + 7).unwrap();
    }
    output.pop();
    output.into_bytes()
}

#[test]
fn fragmented_ssh_parameter_range_preserves_all_8192_lines_and_request_ownership() {
    let fixture = Fixture::new(None);
    fixture.connect();
    let range = fixture
        .client
        .submit(
            fixture.runtime.next_id(),
            Operation::Command(Command::raw("P1..8192").unwrap()),
            RequestOptions {
                read_pool: true,
                timeout_ms: 10_000,
                ..RequestOptions::default()
            },
        )
        .unwrap();
    let following = fixture
        .client
        .submit(
            fixture.runtime.next_id(),
            Operation::Command(Command::raw("#1p").unwrap()),
            RequestOptions {
                timeout_ms: 10_000,
                ..RequestOptions::default()
            },
        )
        .unwrap();
    let response = fixture.run_result(range.clone()).unwrap();
    let following_response = fixture.run_result(following.clone()).unwrap();
    assert_eq!(response.response, parameter_range_response());
    assert_eq!(following_response.response, b"12.5");
    assert_ne!(range.info().request_id, following.info().request_id);
    assert_eq!(range.info().certainty, ExecutionCertainty::Confirmed);
    let text = std::str::from_utf8(&response.response).unwrap();
    let mut indices = HashSet::new();
    let mut values = HashSet::new();
    for (offset, line) in text.lines().enumerate() {
        let index = offset as u64 + 1;
        let (name, value) = line.split_once('=').unwrap();
        assert_eq!(name, format!("P{index}"));
        let value = value.parse::<u64>().unwrap();
        assert_eq!(value, index * 31 + 7);
        assert!(indices.insert(index), "duplicate parameter index");
        assert!(values.insert(value), "duplicate generated test value");
    }
    assert_eq!(indices.len(), 8192);
    assert_eq!(values.len(), 8192);
    assert_eq!(
        fixture
            .observed
            .commands
            .lock()
            .unwrap()
            .iter()
            .filter(|command| command.as_str() == "P1..8192")
            .count(),
        1
    );
}
