#![forbid(unsafe_code)]

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use async_trait::async_trait;
use pmac_core::{ConnectOptions, Error, ExecutionContext, OperationResult, Session, Transport};
use pmac_protocol::Command;
use tokio::sync::{watch, Notify};

#[derive(Debug)]
pub struct MockTransport {
    pub session: Arc<MockSession>,
    pub connects: AtomicUsize,
}

impl Default for MockTransport {
    fn default() -> Self {
        Self {
            session: Arc::new(MockSession::default()),
            connects: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl Transport for MockTransport {
    async fn connect(
        &self,
        _options: &ConnectOptions,
        _context: &ExecutionContext,
    ) -> Result<Arc<dyn Session>, Error> {
        self.connects.fetch_add(1, Ordering::AcqRel);
        self.session.healthy.store(true, Ordering::Release);
        Ok(self.session.clone())
    }
}

#[derive(Debug)]
pub struct MockSession {
    healthy: AtomicBool,
    blocked: watch::Sender<bool>,
    commands: Mutex<Vec<String>>,
    shell_commands: Mutex<Vec<String>>,
    ssh_connection: Mutex<String>,
    started: AtomicUsize,
    started_notify: Notify,
}

impl Default for MockSession {
    fn default() -> Self {
        let (blocked, _) = watch::channel(false);
        Self {
            healthy: AtomicBool::new(true),
            blocked,
            commands: Mutex::new(Vec::new()),
            shell_commands: Mutex::new(Vec::new()),
            ssh_connection: Mutex::new("127.0.0.1 12345 127.0.0.1 22".into()),
            started: AtomicUsize::new(0),
            started_notify: Notify::new(),
        }
    }
}

impl MockSession {
    pub fn block(&self, blocked: bool) {
        self.blocked.send_replace(blocked);
    }
    pub fn commands(&self) -> Vec<String> {
        self.lock_commands().clone()
    }
    pub fn shell_commands(&self) -> Vec<String> {
        self.shell_commands
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
    pub fn set_ssh_connection(&self, value: &str) {
        *self
            .ssh_connection
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = value.into();
    }
    pub fn started(&self) -> usize {
        self.started.load(Ordering::Acquire)
    }

    pub async fn wait_started(&self, count: usize) {
        loop {
            let notified = self.started_notify.notified();
            if self.started() >= count {
                return;
            }
            notified.await;
        }
    }

    fn lock_commands(&self) -> MutexGuard<'_, Vec<String>> {
        self.commands
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[async_trait]
impl Session for MockSession {
    async fn command(
        &self,
        command: &Command,
        _context: &ExecutionContext,
    ) -> Result<OperationResult, Error> {
        if !self.healthy.load(Ordering::Acquire) {
            return Err(Error::disconnected("mock disconnected").sent());
        }
        if command.text() == "#13p" {
            panic!("injected command task panic");
        }
        let mut gate = self.blocked.subscribe();
        if *gate.borrow_and_update() {
            self.started.fetch_add(1, Ordering::AcqRel);
            self.started_notify.notify_waiters();
            while *gate.borrow() {
                gate.changed()
                    .await
                    .map_err(|_| Error::disconnected("mock gate closed").sent())?;
            }
        }
        self.lock_commands().push(command.text().into());
        let response: Vec<u8> = match command.text() {
            "Sys.ServoCount" => b"100".to_vec(),
            "Sys.ServoErrorCtr" | "Sys.PhaseErrorCtr" => b"0".to_vec(),
            "Sys.ServoTime" | "Sys.PhaseTime" => b"1.25".to_vec(),
            "Sys.AbortAll" => b"0".to_vec(),
            text if text.starts_with('#') && text.ends_with('p') => text[1..text.len() - 1]
                .parse::<f64>()
                .unwrap_or(0.0)
                .to_string()
                .into_bytes(),
            text if text.starts_with('#') && text.ends_with('?') => b"$8000000000000000".to_vec(),
            text if text.starts_with('&') && text.ends_with('?') => b"$3".to_vec(),
            text if text.starts_with("Motor[") && text.ends_with("].ClosedLoop") => b"1".to_vec(),
            text if text.starts_with("Motor[") && text.ends_with("].AmpEna") => b"1".to_vec(),
            text if text.starts_with("Plc[") => b"1".to_vec(),
            text if text.starts_with("Coord[") => b"0".to_vec(),
            _ => Vec::new(),
        };
        Ok(OperationResult::response(response))
    }

    async fn shell(
        &self,
        command: &str,
        _persistent: bool,
        _context: &ExecutionContext,
    ) -> Result<OperationResult, Error> {
        self.shell_commands
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(command.into());
        if command.contains("$SSH_CONNECTION") {
            return Ok(OperationResult {
                response: self
                    .ssh_connection
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .as_bytes()
                    .to_vec(),
                stderr: Vec::new(),
                exit_code: Some(0),
            });
        }
        if command.starts_with("gpascii -2 -i") {
            Ok(OperationResult {
                response: b"EOF".to_vec(),
                stderr: Vec::new(),
                exit_code: Some(0),
            })
        } else {
            Ok(OperationResult {
                response: command.as_bytes().to_vec(),
                stderr: Vec::new(),
                exit_code: Some(0),
            })
        }
    }

    async fn upload(
        &self,
        _local: &str,
        _remote: &str,
        _context: &ExecutionContext,
    ) -> Result<OperationResult, Error> {
        Ok(OperationResult::default())
    }
    async fn download(
        &self,
        _remote: &str,
        _local: &str,
        _context: &ExecutionContext,
    ) -> Result<OperationResult, Error> {
        Ok(OperationResult::default())
    }
    async fn file_exists(
        &self,
        remote: &str,
        _context: &ExecutionContext,
    ) -> Result<OperationResult, Error> {
        Ok(OperationResult::response(if remote == "missing" {
            b"0".to_vec()
        } else {
            b"1".to_vec()
        }))
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
    async fn close(&self) -> Result<(), Error> {
        self.healthy.store(false, Ordering::Release);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pmac_core::{
        Client, ClientConfig, ClientState, ErrorCode, ExecutionCertainty, Operation, Request,
        RequestOptions, Runtime,
    };

    async fn connected(
        runtime: &Arc<Runtime>,
        mock: &Arc<MockTransport>,
        config: ClientConfig,
    ) -> Arc<Client> {
        let client = runtime.create_client(mock.clone(), config).unwrap();
        let options = ConnectOptions::new("mock", "tester");
        let request = client
            .submit(
                runtime.next_id(),
                Operation::Connect(options),
                RequestOptions::default(),
            )
            .unwrap();
        assert_eq!(wait(request).await.info.code, ErrorCode::Ok);
        client
    }

    async fn wait(request: Arc<Request>) -> pmac_core::WaitOutcome {
        tokio::task::spawn_blocking(move || request.wait(5_000))
            .await
            .unwrap()
    }

    fn command(client: &Arc<Client>, runtime: &Runtime, text: &str) -> Arc<Request> {
        client
            .submit(
                runtime.next_id(),
                Operation::Command(Command::raw(text).unwrap()),
                RequestOptions::default(),
            )
            .unwrap()
    }

    async fn shutdown(runtime: Arc<Runtime>, client: Arc<Client>) {
        client.close(1000).await.unwrap();
        tokio::task::spawn_blocking(move || drop(runtime))
            .await
            .unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_results_keep_request_ownership() {
        let runtime = Runtime::new(2).unwrap();
        let mock = Arc::new(MockTransport::default());
        let client = connected(&runtime, &mock, ClientConfig::default()).await;
        let first = command(&client, &runtime, "#1p");
        let second = command(&client, &runtime, "#2p");
        let (first_outcome, second_outcome) =
            tokio::join!(wait(first.clone()), wait(second.clone()));
        assert_eq!(first_outcome.info.code, ErrorCode::Ok);
        assert_eq!(second_outcome.info.code, ErrorCode::Ok);
        assert_eq!(
            first.result().unwrap().as_ref().as_ref().unwrap().response,
            b"1"
        );
        assert_eq!(
            second.result().unwrap().as_ref().as_ref().unwrap().response,
            b"2"
        );
        shutdown(runtime, client).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn thirty_two_mixed_requests_do_not_change_ownership() {
        let runtime = Runtime::new(2).unwrap();
        let mock = Arc::new(MockTransport::default());
        let client = connected(&runtime, &mock, ClientConfig::default()).await;
        let requests = (0..32)
            .map(|index| {
                command(
                    &client,
                    &runtime,
                    if index % 2 == 0 { "#1p" } else { "#2p" },
                )
            })
            .collect::<Vec<_>>();
        for (index, request) in requests.into_iter().enumerate() {
            assert_eq!(wait(request.clone()).await.info.code, ErrorCode::Ok);
            let expected = if index % 2 == 0 {
                b"1".to_vec()
            } else {
                b"2".to_vec()
            };
            assert_eq!(
                request
                    .result()
                    .unwrap()
                    .as_ref()
                    .as_ref()
                    .unwrap()
                    .response,
                expected
            );
        }
        shutdown(runtime, client).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn queued_cancel_never_reaches_transport() {
        let runtime = Runtime::new(2).unwrap();
        let mock = Arc::new(MockTransport::default());
        let client = connected(&runtime, &mock, ClientConfig::default()).await;
        mock.session.block(true);
        let running = command(&client, &runtime, "#1p");
        mock.session.wait_started(1).await;
        let queued = command(&client, &runtime, "#2p");
        queued.cancel();
        assert_eq!(queued.info().certainty, ExecutionCertainty::NotSent);
        mock.session.block(false);
        assert_eq!(wait(running).await.info.code, ErrorCode::Ok);
        assert_eq!(wait(queued).await.info.code, ErrorCode::Cancelled);
        assert_eq!(mock.session.commands(), vec!["#1p"]);
        shutdown(runtime, client).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn execution_timeout_is_unknown_and_retires_connection() {
        let runtime = Runtime::new(2).unwrap();
        let mock = Arc::new(MockTransport::default());
        let client = connected(&runtime, &mock, ClientConfig::default()).await;
        mock.session.block(true);
        let request = client
            .submit(
                runtime.next_id(),
                Operation::Command(Command::raw("#1p").unwrap()),
                RequestOptions {
                    timeout_ms: 20,
                    ..RequestOptions::default()
                },
            )
            .unwrap();
        mock.session.wait_started(1).await;
        let outcome = wait(request).await;
        assert_eq!(outcome.info.code, ErrorCode::ExecTimeout);
        assert_eq!(outcome.info.certainty, ExecutionCertainty::MayHaveExecuted);
        assert_eq!(client.snapshot().state, ClientState::Disconnected);
        mock.session.block(false);
        assert_eq!(mock.session.commands().len(), 0);
        shutdown(runtime, client).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn wait_pending_does_not_cancel_execution() {
        let runtime = Runtime::new(2).unwrap();
        let mock = Arc::new(MockTransport::default());
        let client = connected(&runtime, &mock, ClientConfig::default()).await;
        mock.session.block(true);
        let request = command(&client, &runtime, "#1p");
        mock.session.wait_started(1).await;
        assert!(!request.wait(0).completed);
        mock.session.block(false);
        assert_eq!(wait(request).await.info.code, ErrorCode::Ok);
        shutdown(runtime, client).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn close_linearizes_against_new_submission() {
        let runtime = Runtime::new(2).unwrap();
        let mock = Arc::new(MockTransport::default());
        let client = connected(&runtime, &mock, ClientConfig::default()).await;
        mock.session.block(true);
        let running = command(&client, &runtime, "#1p");
        mock.session.wait_started(1).await;
        let closing_client = client.clone();
        let closing = tokio::spawn(async move { closing_client.close(1000).await });
        while !client.snapshot().closing {
            tokio::task::yield_now().await;
        }
        assert_eq!(
            client
                .submit(
                    runtime.next_id(),
                    Operation::Command(Command::raw("#2p").unwrap()),
                    RequestOptions::default()
                )
                .unwrap_err()
                .code,
            ErrorCode::Closing
        );
        while !running.is_cancelled() {
            tokio::task::yield_now().await;
        }
        mock.session.block(false);
        closing.await.unwrap().unwrap();
        assert_eq!(wait(running).await.info.code, ErrorCode::Cancelled);
        tokio::task::spawn_blocking(move || drop(runtime))
            .await
            .unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn task_panic_completes_request_and_quarantines_client() {
        let runtime = Runtime::new(2).unwrap();
        let mock = Arc::new(MockTransport::default());
        let client = connected(&runtime, &mock, ClientConfig::default()).await;
        let request = command(&client, &runtime, "#13p");
        assert_eq!(wait(request).await.info.code, ErrorCode::Internal);
        assert_eq!(client.snapshot().state, ClientState::Disconnected);
        shutdown(runtime, client).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn completed_result_uses_budget_until_released() {
        let runtime = Runtime::new(2).unwrap();
        let mock = Arc::new(MockTransport::default());
        let client = connected(
            &runtime,
            &mock,
            ClientConfig {
                max_requests: 1,
                ..ClientConfig::default()
            },
        )
        .await;
        let request = command(&client, &runtime, "#1p");
        wait(request.clone()).await;
        assert_eq!(
            client
                .submit(
                    runtime.next_id(),
                    Operation::Command(Command::raw("#2p").unwrap()),
                    RequestOptions::default()
                )
                .unwrap_err()
                .code,
            ErrorCode::QueueFull
        );
        drop(request);
        let next = command(&client, &runtime, "#2p");
        assert_eq!(wait(next).await.info.code, ErrorCode::Ok);
        shutdown(runtime, client).await;
    }
}

#[cfg(test)]
mod pool_tests;
