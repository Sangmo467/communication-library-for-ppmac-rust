use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use pmac_protocol::Command;
use zeroize::Zeroizing;

use crate::{Error, ErrorCode, OperationResult, Request};

const MAX_GPASCII_CHANNELS: usize = 8;

#[derive(Default)]
struct GpasciiBudgetState {
    used: usize,
    uncertain: usize,
}

#[doc(hidden)]
#[derive(Default)]
pub struct GpasciiBudget {
    state: std::sync::Mutex<GpasciiBudgetState>,
}
impl GpasciiBudget {
    pub(crate) fn new() -> Self {
        Self::default()
    }
    pub fn reserve(self: &Arc<Self>) -> Result<GpasciiChannelPermit, Error> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.used >= MAX_GPASCII_CHANNELS {
            return Err(Error::new(
                ErrorCode::ResourceLimit,
                "controller GPASCII channel limit reached (8)",
            ));
        }
        state.used += 1;
        drop(state);
        Ok(GpasciiChannelPermit {
            budget: self.clone(),
            release_on_drop: true,
        })
    }
    pub fn confirm_remote_release(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.used = state
            .used
            .checked_sub(state.uncertain)
            .expect("GPASCII release confirmation exceeds uncertain usage");
        state.uncertain = 0;
    }
}

pub struct GpasciiChannelPermit {
    budget: Arc<GpasciiBudget>,
    release_on_drop: bool,
}
impl GpasciiChannelPermit {
    pub fn release(mut self) {
        self.release_one();
        self.release_on_drop = false;
    }
    pub fn retain_uncertain(mut self) {
        let mut state = self
            .budget
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.uncertain += 1;
        self.release_on_drop = false;
    }
    fn release_one(&self) {
        let mut state = self
            .budget
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.used = state
            .used
            .checked_sub(1)
            .expect("GPASCII channel budget underflow");
    }
}
impl Drop for GpasciiChannelPermit {
    fn drop(&mut self) {
        if self.release_on_drop {
            self.release_one();
        }
    }
}

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
    #[doc(hidden)]
    pub gpascii_budget: Arc<GpasciiBudget>,
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
            gpascii_budget: Arc::new(GpasciiBudget::new()),
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
        self.request.origin_liveness()
    }
    pub fn begin_control_channel_initialization(&self) {
        self.request.begin_control_channel_initialization();
    }
    pub fn finish_control_channel_initialization(&self) {
        self.request.finish_control_channel_initialization();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ExecutionCertainty;

    #[test]
    fn gpascii_budget_is_bounded_and_uncertain_release_requires_confirmation() {
        let budget = Arc::new(GpasciiBudget::new());
        let mut permits = (0..8)
            .map(|_| budget.reserve().unwrap())
            .collect::<Vec<_>>();
        let error = budget.reserve().err().unwrap();
        assert_eq!(error.code, ErrorCode::ResourceLimit);
        assert_eq!(error.certainty, ExecutionCertainty::NotSent);
        drop(permits.pop());
        permits.push(budget.reserve().unwrap());
        permits.pop().unwrap().retain_uncertain();
        assert!(budget.reserve().is_err());
        budget.confirm_remote_release();
        permits.push(budget.reserve().unwrap());
        assert_eq!(permits.len(), 8);
    }
}
