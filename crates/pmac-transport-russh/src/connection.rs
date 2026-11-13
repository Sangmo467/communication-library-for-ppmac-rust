use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use russh::client::{self, Handle};
use russh::keys::{
    PrivateKey, PrivateKeyWithHashAlg, PublicKey, PublicKeyBase64, PublicKeyOrCertificate,
};
use russh::ChannelId;
use russh::Disconnect;
use sha2::{Digest, Sha256};
use tokio::net::TcpStream;

use pmac_core::{ConnectOptions, Error, ErrorCode, ExecutionContext, HostKeyPolicy};

pub(crate) type Channel = russh::Channel<client::Msg>;
#[derive(Default)]
struct CloseState {
    waiting: HashMap<ChannelId, tokio::sync::oneshot::Sender<()>>,
    closed: HashSet<ChannelId>,
}
type CloseEvents = Arc<Mutex<CloseState>>;

struct CloseEventRegistration {
    channel: ChannelId,
    events: CloseEvents,
}
impl Drop for CloseEventRegistration {
    fn drop(&mut self) {
        self.events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .waiting
            .remove(&self.channel);
    }
}

pub(crate) struct Connection {
    pub(crate) handle: Arc<Handle<ClientHandler>>,
    healthy: Arc<AtomicBool>,
    close_events: CloseEvents,
}

impl Connection {
    pub(crate) fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::Acquire) && !self.handle.is_closed()
    }

    pub(crate) fn retire(&self) {
        self.healthy.store(false, Ordering::Release);
    }

    pub(crate) async fn open_channel(&self) -> Result<Channel, Error> {
        if !self.is_healthy() {
            return Err(disconnected("SSH connection is closed"));
        }
        self.handle
            .channel_open_session()
            .await
            .map_err(transport_error)
    }

    pub(crate) async fn close_channel(&self, mut channel: Channel) -> bool {
        let id = channel.id();
        let (confirmed, mut receiver) = tokio::sync::oneshot::channel();
        {
            let mut events = self
                .close_events
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if events.closed.remove(&id) {
                return true;
            }
            let previous = events.waiting.insert(id, confirmed);
            assert!(previous.is_none(), "duplicate channel close registration");
        }
        let _registration = CloseEventRegistration {
            channel: id,
            events: self.close_events.clone(),
        };
        // Give stdin-driven GPASCII a chance to exit before forcing channel close.
        // This grace period is inside the caller's two-second cleanup deadline.
        let graceful = async {
            if channel.eof().await.is_err() {
                return None;
            }
            loop {
                tokio::select! {
                    confirmed = &mut receiver => return Some(confirmed.is_ok()),
                    message = channel.wait() => match message {
                        Some(russh::ChannelMsg::ExitStatus { .. }
                            | russh::ChannelMsg::ExitSignal { .. }
                            | russh::ChannelMsg::Close) | None => return None,
                        _ => {}
                    }
                }
            }
        };
        if let Ok(Some(confirmed)) =
            tokio::time::timeout(std::time::Duration::from_millis(500), graceful).await
        {
            return confirmed;
        }
        // Exit status alone does not confirm channel release. Always finish the
        // close handshake; timeout/cancellation retains the uncertain permit.
        if channel.close().await.is_err() {
            return receiver.try_recv().is_ok();
        }
        receiver.await.is_ok()
    }

    pub(crate) async fn close(&self) -> Result<(), Error> {
        self.retire();
        let stop = async {
            if !self.handle.is_closed() {
                let _ = self
                    .handle
                    .disconnect(Disconnect::ByApplication, "pmac client shutdown", "en")
                    .await;
            }
            while !self.handle.is_closed() {
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            }
        };
        tokio::time::timeout(std::time::Duration::from_secs(2), stop)
            .await
            .map_err(|_| {
                Error::new(
                    ErrorCode::ShutdownPending,
                    "SSH protocol task has not confirmed shutdown",
                )
            })
    }
}
pub(crate) struct ClientHandler {
    verifier: HostKeyVerifier,
    healthy: Arc<AtomicBool>,
    close_events: CloseEvents,
    track_closes: bool,
}

enum HostKeyVerifier {
    AcceptAnyExplicit,
    Known {
        allowed: Vec<PublicKey>,
        revoked: Vec<PublicKey>,
    },
    Pinned([u8; 32]),
}

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let key = key.public_key();
        let accepted = match &self.verifier {
            HostKeyVerifier::AcceptAnyExplicit => true,
            HostKeyVerifier::Known { allowed, revoked } => {
                !revoked
                    .iter()
                    .any(|known| known.key_data() == key.key_data())
                    && allowed
                        .iter()
                        .any(|known| known.key_data() == key.key_data())
            }
            HostKeyVerifier::Pinned(expected) => {
                Sha256::digest(key.public_key_bytes()).as_slice() == expected
            }
        };
        if !accepted {
            self.healthy.store(false, Ordering::Release);
        }
        Ok(accepted)
    }

    async fn channel_close(
        &mut self,
        channel: ChannelId,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        if self.track_closes {
            let mut events = self
                .close_events
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(confirmed) = events.waiting.remove(&channel) {
                let _ = confirmed.send(());
            } else {
                events.closed.insert(channel);
            }
        }
        Ok(())
    }
}

pub(crate) async fn connect(
    options: &ConnectOptions,
    _context: &ExecutionContext,
    track_closes: bool,
) -> Result<Arc<Connection>, Error> {
    validate_options(options)?;
    let verifier = load_verifier(options).await?;
    let private_key = load_private_key(options).await?;

    let address = (options.host.as_str(), options.port);
    let stream = if options.tcp_probe {
        tokio::time::timeout(options.tcp_probe_timeout, TcpStream::connect(address))
            .await
            .map_err(|_| Error::new(ErrorCode::ConnectError, "TCP connect probe timed out"))?
            .map_err(|error| {
                Error::new(
                    ErrorCode::ConnectError,
                    format!("TCP connect failed: {error}"),
                )
            })?
    } else {
        TcpStream::connect(address).await.map_err(|error| {
            Error::new(
                ErrorCode::ConnectError,
                format!("TCP connect failed: {error}"),
            )
        })?
    };
    stream.set_nodelay(true).map_err(|error| {
        Error::new(
            ErrorCode::ConnectError,
            format!("failed to enable TCP_NODELAY: {error}"),
        )
    })?;

    let config = Arc::new(client::Config {
        nodelay: true,
        keepalive_interval: Some(options.keepalive_interval),
        keepalive_max: options.keepalive_max as usize,
        inactivity_timeout: None,
        channel_buffer_size: 64,
        ..client::Config::default()
    });
    let healthy = Arc::new(AtomicBool::new(true));
    let close_events = CloseEvents::default();
    let handler = ClientHandler {
        verifier,
        healthy: healthy.clone(),
        close_events: close_events.clone(),
        track_closes,
    };
    let mut handle = client::connect_stream(config, stream, handler)
        .await
        .map_err(|error| {
            if healthy.load(Ordering::Acquire) {
                Error::new(
                    ErrorCode::ConnectError,
                    format!("SSH handshake failed: {error}"),
                )
            } else {
                Error::new(
                    ErrorCode::HostKey,
                    format!("SSH handshake or host-key verification failed: {error}"),
                )
            }
        })?;

    let authenticated = if let Some(key) = private_key {
        let hash = if key.algorithm().is_rsa() {
            handle
                .best_supported_rsa_hash()
                .await
                .map_err(transport_error)?
                .flatten()
        } else {
            None
        };
        handle
            .authenticate_publickey(
                options.username.clone(),
                PrivateKeyWithHashAlg::new(Arc::new(key), hash),
            )
            .await
            .map_err(authentication_error)?
            .success()
    } else if let Some(password) = &options.password {
        handle
            .authenticate_password(options.username.clone(), password.as_str())
            .await
            .map_err(authentication_error)?
            .success()
    } else {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "password or private key is required",
        ));
    };
    if !authenticated {
        return Err(Error::new(
            ErrorCode::Authentication,
            "SSH authentication was rejected",
        ));
    }
    Ok(Arc::new(Connection {
        handle: Arc::new(handle),
        healthy,
        close_events,
    }))
}

async fn load_verifier(options: &ConnectOptions) -> Result<HostKeyVerifier, Error> {
    match &options.host_key {
        HostKeyPolicy::Unspecified => Err(Error::new(
            ErrorCode::HostKey,
            "host-key policy must be explicitly configured",
        )),
        HostKeyPolicy::AcceptAnyExplicit => Ok(HostKeyVerifier::AcceptAnyExplicit),
        HostKeyPolicy::PinnedSha256(digest) => Ok(HostKeyVerifier::Pinned(*digest)),
        HostKeyPolicy::KnownHosts(path) => {
            let host = options.host.clone();
            let port = options.port;
            let path = path.clone();
            tokio::task::spawn_blocking(move || {
                russh::keys::known_hosts::known_host_key_entries_path(&host, port, path)
            })
            .await
            .map_err(|error| Error::internal(error.to_string()))?
            .map(|keys| {
                let mut allowed = Vec::new();
                let mut revoked = Vec::new();
                for (_, key, is_revoked) in keys {
                    if is_revoked {
                        revoked.push(key);
                    } else {
                        allowed.push(key);
                    }
                }
                HostKeyVerifier::Known { allowed, revoked }
            })
            .map_err(|error| {
                Error::new(
                    ErrorCode::HostKey,
                    format!("failed to read known_hosts: {error}"),
                )
            })
        }
    }
}

async fn load_private_key(options: &ConnectOptions) -> Result<Option<PrivateKey>, Error> {
    let Some(path) = options.private_key.clone() else {
        return Ok(None);
    };
    let passphrase = options.passphrase.clone();
    tokio::task::spawn_blocking(move || {
        russh::keys::load_secret_key(path, passphrase.as_deref().map(String::as_str))
    })
    .await
    .map_err(|error| Error::internal(error.to_string()))?
    .map(Some)
    .map_err(|error| {
        Error::new(
            ErrorCode::Authentication,
            format!("failed to load private key: {error}"),
        )
    })
}

fn validate_options(options: &ConnectOptions) -> Result<(), Error> {
    if options.host.is_empty()
        || options.host.as_bytes().contains(&0)
        || options.username.is_empty()
        || options.username.as_bytes().contains(&0)
    {
        return Err(Error::invalid(
            "host and username must be non-empty UTF-8 without NUL",
        ));
    }
    if options.port == 0 || options.control_pool_size == 0 {
        return Err(Error::invalid(
            "port and control_pool_size must be non-zero",
        ));
    }
    Ok(())
}

pub(crate) fn transport_error(error: russh::Error) -> Error {
    disconnected(format!("SSH transport failed: {error}"))
}
fn authentication_error(error: russh::Error) -> Error {
    Error::new(
        ErrorCode::Authentication,
        format!("SSH authentication failed: {error}"),
    )
}
pub(crate) fn disconnected(message: impl Into<String>) -> Error {
    Error::disconnected(message).sent()
}
