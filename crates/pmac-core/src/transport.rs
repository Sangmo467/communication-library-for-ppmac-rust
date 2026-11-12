use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use pmac_protocol::Command;
use zeroize::Zeroizing;

use crate::{Error, ErrorCode, OperationResult, Request};

#[derive(Clone)]
pub struct ConnectOptions {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: Option<Zeroizing<String>>,
    pub private_key: Option<PathBuf>,
    public_key: Option<PathBuf>,
    pub passphrase: Option<Zeroizing<String>>,
    pub host_key: HostKeyPolicy,
    pub tcp_probe: bool,
    pub tcp_probe_timeout: Duration,
    pub keepalive_interval: Duration,
    pub keepalive_max: u32,
    pub control_pool_size: u8,
}

impl std::fmt::Debug for ConnectOptions {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConnectOptions")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("private_key", &self.private_key)
            .field("host_key", &self.host_key)
            .finish_non_exhaustive()
    }
}

impl ConnectOptions {
    pub fn new(host: impl Into<String>, username: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            port: 22,
            username: username.into(),
            password: None,
            private_key: None,
            public_key: None,
            passphrase: None,
            host_key: HostKeyPolicy::Unspecified,
            tcp_probe: true,
            tcp_probe_timeout: Duration::from_millis(600),
            keepalive_interval: Duration::from_secs(1),
            keepalive_max: 3,
            control_pool_size: 1,
        }
    }

    pub(crate) fn owned_bytes(&self) -> usize {
        self.host
            .len()
            .saturating_add(self.username.len())
            .saturating_add(self.password.as_ref().map_or(0, |v| v.len()))
            .saturating_add(self.passphrase.as_ref().map_or(0, |v| v.len()))
            .saturating_add(match &self.host_key {
                HostKeyPolicy::KnownHosts(path) => path.as_os_str().len().saturating_mul(4),
                _ => 32,
            })
            .saturating_add(
                self.private_key
                    .as_ref()
                    .map_or(0, |v| v.as_os_str().len().saturating_mul(4)),
            )
            .saturating_add(
                self.public_key
                    .as_ref()
                    .map_or(0, |v| v.as_os_str().len().saturating_mul(4)),
            )
    }
    pub fn public_key_path(&self) -> Option<&PathBuf> {
        self.public_key.as_ref()
    }
    pub fn set_public_key_path(&mut self, path: Option<PathBuf>) {
        self.public_key = path;
    }
}

#[derive(Debug, Clone)]
pub enum HostKeyPolicy {
    Unspecified,
    KnownHosts(PathBuf),
    PinnedSha256([u8; 32]),
    AcceptAnyExplicit,
}

pub struct ExecutionContext {
    request: Arc<Request>,
}

impl ExecutionContext {
    pub(crate) fn new(request: Arc<Request>) -> Self {
        Self { request }
    }
    pub fn is_cancelled(&self) -> bool {
        self.request.is_cancelled()
    }
    pub async fn cancelled(&self) {
        self.request.cancelled().await
    }
    pub fn deadline(&self) -> Option<tokio::time::Instant> {
        self.request.deadline()
    }
    pub fn max_response_bytes(&self) -> usize {
        self.request.max_response_bytes()
    }
    pub fn origin_thread(&self) -> std::thread::ThreadId {
        self.request.origin_thread()
    }
    pub fn origin_liveness(&self) -> std::sync::Weak<()> {
        self.request.origin_liveness().clone()
    }

    pub fn mark_publish_started(&self) {
        self.request.mark_publish_started();
    }
    pub fn mark_publish_confirmed(&self) {
        self.request.mark_publish_confirmed();
    }
    pub fn has_unconfirmed_publish(&self) -> bool {
        self.request.has_unconfirmed_publish()
    }
    pub fn timeout_error(&self) -> Error {
        Error::new(
            ErrorCode::ExecTimeout,
            "operation execution deadline expired",
        )
        .sent()
        .fatal()
    }
}

#[async_trait]
pub trait Transport: Send + Sync + 'static {
    async fn connect(
        &self,
        options: &ConnectOptions,
        context: &ExecutionContext,
    ) -> Result<Arc<dyn Session>, Error>;
}

#[async_trait]
pub trait Session: Send + Sync + 'static {
    async fn command(
        &self,
        command: &Command,
        context: &ExecutionContext,
    ) -> Result<OperationResult, Error>;
    async fn shell(
        &self,
        command: &str,
        persistent: bool,
        context: &ExecutionContext,
    ) -> Result<OperationResult, Error>;
    async fn upload(
        &self,
        local: &str,
        remote: &str,
        context: &ExecutionContext,
    ) -> Result<OperationResult, Error>;
    async fn download(
        &self,
        remote: &str,
        local: &str,
        context: &ExecutionContext,
    ) -> Result<OperationResult, Error>;
    async fn file_exists(
        &self,
        remote: &str,
        context: &ExecutionContext,
    ) -> Result<OperationResult, Error>;
    async fn file_remove(
        &self,
        remote: &str,
        context: &ExecutionContext,
    ) -> Result<OperationResult, Error>;
    fn is_healthy(&self) -> bool;
    async fn retire_control(&self) -> Result<(), Error> {
        self.close().await
    }
    async fn retire_maintenance(&self) -> Result<(), Error> {
        Ok(())
    }
    async fn repair_maintenance(&self, _context: &ExecutionContext) -> Result<(), Error> {
        Ok(())
    }
    async fn close(&self) -> Result<(), Error>;
}
