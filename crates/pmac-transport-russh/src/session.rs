use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use async_trait::async_trait;
use pmac_core::{
    ConnectOptions, Error, ErrorCode, ExecutionCertainty, ExecutionContext, OperationResult,
    Session, Transport,
};
use pmac_protocol::{
    Command, GpasciiDecoder, MaintenanceKind, ResetDecoder, ResetEvent, ShellDecoder, ShellEvent,
    ShellMarkers, ShellStderrDecoder, RESET_READINESS_PROBE,
};
use rand::Rng;
use russh::ChannelMsg;
use russh_sftp::client::SftpSession;

use crate::connection::{connect, disconnected, transport_error, Channel, Connection};

#[derive(Debug, Default)]
pub struct RusshTransport;

#[async_trait]
impl Transport for RusshTransport {
    async fn connect(
        &self,
        options: &ConnectOptions,
        context: &ExecutionContext,
    ) -> Result<Arc<dyn Session>, Error> {
        if options.control_pool_size != 1 {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "each pool member uses one SSH connection",
            ));
        }
        let control = connect(options, context).await?;
        let channel = open_gpascii(&control, context).await?;
        Ok(Arc::new(RusshSession {
            options: options.clone(),
            control,
            control_channel: tokio::sync::Mutex::new(Some(channel)),
            control_channels: tokio::sync::Mutex::new(HashMap::new()),
            maintenance: tokio::sync::Mutex::new(None),
            sftp: tokio::sync::Mutex::new(None),
            persistent_shell: tokio::sync::Mutex::new(None),
            nonce: format!("{:016x}", rand::rng().random::<u64>()),
            sequence: Mutex::new(0),
            closed: AtomicBool::new(false),
            maintenance_retired: AtomicBool::new(false),
            owned_staging: Mutex::new(BTreeSet::new()),
        }))
    }
}

const MAX_CONTROL_CHANNELS: usize = 64;
const READINESS_RESPONSE_LIMIT: usize = 4096;

struct ControlChannel {
    channel: Channel,
    owner: Weak<()>,
}

pub(crate) struct RusshSession {
    options: ConnectOptions,
    control: Arc<Connection>,
    control_channel: tokio::sync::Mutex<Option<Channel>>,
    control_channels: tokio::sync::Mutex<HashMap<std::thread::ThreadId, ControlChannel>>,
    maintenance: tokio::sync::Mutex<Option<Arc<Connection>>>,
    sftp: tokio::sync::Mutex<Option<Arc<SftpSession>>>,
    persistent_shell: tokio::sync::Mutex<Option<Channel>>,
    pub(crate) nonce: String,
    sequence: Mutex<u64>,
    closed: AtomicBool,
    maintenance_retired: AtomicBool,
    owned_staging: Mutex<BTreeSet<(bool, String)>>,
}

#[async_trait]
impl Session for RusshSession {
    async fn command(
        &self,
        command: &Command,
        context: &ExecutionContext,
    ) -> Result<OperationResult, Error> {
        let origin = context.origin_thread();
        let owner = context.origin_liveness();
        let mut channels = self.control_channels.lock().await;
        reclaim_exited_channels(&mut channels).await;
        if !channels.contains_key(&origin) && channels.len() >= MAX_CONTROL_CHANNELS {
            return Err(Error::new(
                ErrorCode::ResourceLimit,
                format!("per-member GPASCII thread channel limit reached ({MAX_CONTROL_CHANNELS})"),
            ));
        }
        let result = match channels.entry(origin) {
            std::collections::hash_map::Entry::Occupied(entry) => {
                let channel = &mut entry.into_mut().channel;
                execute_gpascii(
                    &self.control,
                    channel,
                    command,
                    context,
                    context.max_response_bytes(),
                    true,
                )
                .await
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                let channel = match self.control_channel.lock().await.take() {
                    Some(channel) => channel,
                    None => open_gpascii(&self.control, context).await?,
                };
                let channel = &mut entry.insert(ControlChannel { channel, owner }).channel;
                execute_gpascii(
                    &self.control,
                    channel,
                    command,
                    context,
                    context.max_response_bytes(),
                    true,
                )
                .await
            }
        };
        if result.as_ref().is_err_and(|error| error.fatal_transport) {
            self.control.retire();
        }
        result
    }

    async fn shell(
        &self,
        command: &str,
        persistent: bool,
        context: &ExecutionContext,
    ) -> Result<OperationResult, Error> {
        if command.is_empty() || command.len() > 1023 || command.as_bytes().contains(&0) {
            return Err(Error::invalid(
                "shell command must be 1..=1023 bytes without NUL",
            ));
        }
        if persistent {
            self.persistent_shell(command, context).await
        } else {
            self.isolated_exec(command, context).await
        }
    }

    async fn upload(
        &self,
        local: &str,
        remote: &str,
        context: &ExecutionContext,
    ) -> Result<OperationResult, Error> {
        self.upload_file(local, remote, context).await
    }

    async fn download(
        &self,
        remote: &str,
        local: &str,
        context: &ExecutionContext,
    ) -> Result<OperationResult, Error> {
        self.download_file(remote, local, context).await
    }

    async fn file_exists(
        &self,
        remote: &str,
        context: &ExecutionContext,
    ) -> Result<OperationResult, Error> {
        validate_path(remote)?;
        let sftp = self.sftp(context).await?;
        match sftp.metadata(remote).await {
            Ok(_) => Ok(OperationResult::response(b"1".to_vec())),
            Err(error) if is_not_found(&error) => Ok(OperationResult::response(b"0".to_vec())),
            Err(error) => Err(sftp_error(error)),
        }
    }

    async fn file_remove(
        &self,
        remote: &str,
        context: &ExecutionContext,
    ) -> Result<OperationResult, Error> {
        validate_path(remote)?;
        let sftp = self.sftp(context).await?;
        sftp.remove_file(remote).await.map_err(sftp_error)?;
        Ok(OperationResult::default())
    }

    fn is_healthy(&self) -> bool {
        !self.closed.load(Ordering::Acquire) && self.control.is_healthy()
    }

    async fn retire_control(&self) -> Result<(), Error> {
        self.control.close().await
    }

    async fn retire_maintenance(&self) -> Result<(), Error> {
        self.maintenance_retired.store(true, Ordering::Release);
        let sftp = self.sftp.lock().await.clone();
        self.cleanup_owned_staging(sftp.as_deref()).await;
        let connection = self.maintenance.lock().await.take();
        let result = if let Some(connection) = connection {
            connection.close().await
        } else {
            Ok(())
        };
        self.persistent_shell.lock().await.take();
        self.sftp.lock().await.take();
        result
    }

    async fn repair_maintenance(&self, context: &ExecutionContext) -> Result<(), Error> {
        self.retire_maintenance().await?;
        if self.closed.load(Ordering::Acquire) {
            return Err(Error::new(ErrorCode::Closing, "session is closed"));
        }
        let connection = connect(&self.options, context).await?;
        let mut guard = self.maintenance.lock().await;
        if self.closed.load(Ordering::Acquire) {
            drop(guard);
            let _ = connection.close().await;
            return Err(Error::new(
                ErrorCode::Closing,
                "session closed during maintenance repair",
            ));
        }
        *guard = Some(connection);
        self.maintenance_retired.store(false, Ordering::Release);
        Ok(())
    }
    async fn close(&self) -> Result<(), Error> {
        self.closed.store(true, Ordering::Release);
        let (control, maintenance) = tokio::join!(self.retire_control(), self.retire_maintenance());
        control.and(maintenance)
    }
}

impl RusshSession {
    pub(crate) fn check_staging_capacity(&self) -> Result<(), Error> {
        if self
            .owned_staging
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
            >= 64
        {
            return Err(Error::new(ErrorCode::ResourceLimit, "owned staging recovery limit reached; clean up interrupted transfers before retrying"));
        }
        Ok(())
    }
    pub(crate) fn track_staging(&self, path: &str, remote: bool) {
        self.owned_staging
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert((remote, path.into()));
    }

    pub(crate) fn untrack_staging(&self, path: &str, remote: bool) {
        self.owned_staging
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&(remote, path.into()));
    }

    pub(crate) async fn remove_owned_remote_staging(&self, sftp: &SftpSession, path: &str) {
        match sftp.remove_file(path).await {
            Ok(()) => self.untrack_staging(path, true),
            Err(error) if is_not_found(&error) => self.untrack_staging(path, true),
            Err(error) => tracing::warn!(path, %error, "owned remote staging could not be removed"),
        }
    }
    async fn cleanup_owned_staging(&self, sftp: Option<&SftpSession>) {
        let pending = self
            .owned_staging
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        let cleanup = async {
            for (remote, path) in pending {
                let result = if remote {
                    match sftp {
                        Some(sftp) => match sftp.remove_file(&path).await {
                            Ok(()) => Ok(()),
                            Err(error) if is_not_found(&error) => Ok(()),
                            Err(error) => Err(error.to_string()),
                        },
                        None => {
                            Err("no SFTP session is available for owned staging cleanup".into())
                        }
                    }
                } else {
                    match tokio::fs::remove_file(&path).await {
                        Ok(()) => Ok(()),
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                        Err(error) => Err(error.to_string()),
                    }
                };
                match result {
                    Ok(()) => self.untrack_staging(&path, remote),
                    Err(error) => tracing::warn!(
                        path,
                        remote,
                        error,
                        "owned staging remains after interrupted transfer"
                    ),
                }
            }
        };
        if tokio::time::timeout(std::time::Duration::from_millis(250), cleanup)
            .await
            .is_err()
        {
            let pending = self
                .owned_staging
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for (remote, path) in pending.iter() {
                tracing::warn!(
                    path,
                    remote,
                    "owned staging cleanup budget expired; path retained for explicit recovery"
                );
            }
        }
    }
    async fn persistent_shell(
        &self,
        command: &str,
        context: &ExecutionContext,
    ) -> Result<OperationResult, Error> {
        let mut guard = self.persistent_shell.lock().await;
        if guard.is_none() {
            let connection = self.maintenance_connection(context).await?;
            let channel = connection.open_channel().await?;
            channel.request_shell(true).await.map_err(transport_error)?;
            *guard = Some(channel);
        }
        let channel = guard.as_mut().expect("shell channel initialized");
        let sequence = self.next_sequence()?;
        let markers = ShellMarkers::new(&self.nonce, sequence);
        let wire = markers.command(command)?;
        channel.data_bytes(wire).await.map_err(transport_error)?;
        let framing_allowance = markers
            .begin
            .len()
            .saturating_add(markers.end.len())
            .saturating_add(markers.stderr_end.len())
            .saturating_add(64);
        let mut stdout_received = 0_usize;
        let mut stderr_received = 0_usize;
        let mut stderr_decoder =
            ShellStderrDecoder::new(markers.stderr_end.clone(), context.max_response_bytes());
        let mut decoder = ShellDecoder::new(markers, context.max_response_bytes());
        let mut stdout_result = None;
        let mut stderr_result = None;
        loop {
            let Some(message) = channel.wait().await else {
                guard.take();
                return Err(disconnected(
                    "persistent shell closed before its completion markers",
                ));
            };
            match message {
                ChannelMsg::Data { data } => {
                    stdout_received = stdout_received.saturating_add(data.len());
                    if stdout_received.saturating_add(stderr_received)
                        > context
                            .max_response_bytes()
                            .saturating_add(framing_allowance)
                    {
                        guard.take();
                        return Err(output_limit());
                    }
                    for event in decoder.push(&data).map_err(protocol_error)? {
                        if let ShellEvent::Complete { stdout, exit_code } = event {
                            stdout_result = Some((stdout, exit_code));
                        }
                    }
                }
                ChannelMsg::ExtendedData { data, .. } => {
                    stderr_received = stderr_received.saturating_add(data.len());
                    if stdout_received.saturating_add(stderr_received)
                        > context
                            .max_response_bytes()
                            .saturating_add(framing_allowance)
                    {
                        guard.take();
                        return Err(output_limit());
                    }
                    if let Some(stderr) = stderr_decoder.push(&data).map_err(protocol_error)? {
                        stderr_result = Some(stderr);
                    }
                }
                ChannelMsg::Failure => {
                    guard.take();
                    return Err(disconnected("server rejected persistent shell request"));
                }
                ChannelMsg::Eof | ChannelMsg::Close => {
                    guard.take();
                    return Err(disconnected(
                        "persistent shell ended before its completion markers",
                    ));
                }
                _ => {}
            }
            if stdout_result.is_some() && stderr_result.is_some() {
                let (stdout, exit_code) = stdout_result.take().expect("stdout result checked");
                let stderr = stderr_result.take().expect("stderr result checked");
                if stdout.len().saturating_add(stderr.len()) > context.max_response_bytes() {
                    guard.take();
                    return Err(output_limit());
                }
                return Ok(OperationResult {
                    response: stdout,
                    stderr,
                    exit_code: Some(exit_code),
                });
            }
        }
    }

    async fn isolated_exec(
        &self,
        command: &str,
        context: &ExecutionContext,
    ) -> Result<OperationResult, Error> {
        let connection = self.maintenance_connection(context).await?;
        let mut channel = connection.open_channel().await?;
        channel.exec(true, command).await.map_err(transport_error)?;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut exit_code = None;
        loop {
            let Some(message) = channel.wait().await else {
                return Err(disconnected(
                    "exec stream ended without a confirmed exit status",
                ));
            };
            match message {
                ChannelMsg::Data { data } => append_bounded(
                    &mut stdout,
                    &data,
                    context.max_response_bytes().saturating_sub(stderr.len()),
                )?,
                ChannelMsg::ExtendedData { data, .. } => append_bounded(
                    &mut stderr,
                    &data,
                    context.max_response_bytes().saturating_sub(stdout.len()),
                )?,
                ChannelMsg::ExitStatus { exit_status } => {
                    exit_code = Some(i32::try_from(exit_status).map_err(|_| {
                        disconnected("exec returned an exit status outside the public i32 range")
                    })?);
                }
                ChannelMsg::Close => {
                    return match exit_code {
                        Some(exit_code) => Ok(OperationResult {
                            response: stdout,
                            stderr,
                            exit_code: Some(exit_code),
                        }),
                        None => Err(disconnected(
                            "exec channel closed without a confirmed exit status",
                        )),
                    };
                }
                ChannelMsg::Failure => return Err(disconnected("server rejected exec request")),
                // EOF only closes the output stream; RFC 4254 permits exit-status afterwards.
                ChannelMsg::Eof => {}
                _ => {}
            }
        }
    }
    pub(crate) async fn maintenance_connection(
        &self,
        context: &ExecutionContext,
    ) -> Result<Arc<Connection>, Error> {
        if self.closed.load(Ordering::Acquire) || self.maintenance_retired.load(Ordering::Acquire) {
            return Err(disconnected(
                "maintenance connection requires explicit repair",
            ));
        }
        let mut guard = self.maintenance.lock().await;
        if let Some(connection) = guard.as_ref().filter(|connection| connection.is_healthy()) {
            return Ok(connection.clone());
        }
        *guard = None;
        let connection = connect(&self.options, context).await?;
        if self.closed.load(Ordering::Acquire) || self.maintenance_retired.load(Ordering::Acquire) {
            let _ = connection.close().await;
            return Err(disconnected(
                "maintenance connection retired during creation",
            ));
        }
        *guard = Some(connection.clone());
        Ok(connection)
    }

    pub(crate) async fn sftp(&self, context: &ExecutionContext) -> Result<Arc<SftpSession>, Error> {
        let mut guard = self.sftp.lock().await;
        if let Some(session) = guard.as_ref() {
            return Ok(session.clone());
        }
        let connection = self.maintenance_connection(context).await?;
        let channel = connection.open_channel().await?;
        channel
            .request_subsystem(true, "sftp")
            .await
            .map_err(transport_error)?;
        let session = SftpSession::new(channel.into_stream())
            .await
            .map_err(sftp_error)?;
        *guard = Some(Arc::new(session));
        Ok(guard.as_ref().expect("SFTP initialized").clone())
    }

    pub(crate) fn next_sequence(&self) -> Result<u64, Error> {
        let mut sequence = self.lock_sequence();
        *sequence = sequence
            .checked_add(1)
            .ok_or_else(|| Error::internal("shell marker sequence exhausted"))?;
        Ok(*sequence)
    }

    fn lock_sequence(&self) -> MutexGuard<'_, u64> {
        self.sequence
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

async fn reclaim_exited_channels(channels: &mut HashMap<std::thread::ThreadId, ControlChannel>) {
    let exited = channels
        .iter()
        .filter(|(_, entry)| entry.owner.strong_count() == 0)
        .map(|(origin, _)| *origin)
        .collect::<Vec<_>>();
    for origin in exited {
        if let Some(entry) = channels.remove(&origin) {
            let _ = entry.channel.close().await;
        }
    }
}

fn channel_initialization_error(mut error: Error) -> Error {
    error.certainty = ExecutionCertainty::NotSent;
    error.fatal_transport = false;
    error
}

async fn open_gpascii(
    connection: &Arc<Connection>,
    context: &ExecutionContext,
) -> Result<Channel, Error> {
    let mut channel = connection
        .open_channel()
        .await
        .map_err(channel_initialization_error)?;
    let initialized = async {
        channel
            .exec(true, "gpascii -2")
            .await
            .map_err(transport_error)?;
        let ready = Command::raw("Sys.ServoCount")?;
        let verified = execute_gpascii(
            connection,
            &mut channel,
            &ready,
            context,
            READINESS_RESPONSE_LIMIT,
            false,
        )
        .await?;
        let readiness_value = verified
            .response
            .split(|byte| matches!(byte, b'\r' | b'\n'))
            .rfind(|line| !line.is_empty())
            .ok_or_else(|| disconnected("GPASCII readiness query returned no value"))?;
        pmac_protocol::parse_u64(readiness_value)
            .map_err(|_| disconnected("GPASCII readiness query returned no valid counter"))?;
        Ok(())
    }
    .await;
    if let Err(error) = initialized {
        let _ = channel.close().await;
        return Err(channel_initialization_error(error));
    }
    Ok(channel)
}

async fn execute_gpascii(
    connection: &Arc<Connection>,
    channel: &mut Channel,
    command: &Command,
    context: &ExecutionContext,
    response_limit: usize,
    channel_failure_retires_connection: bool,
) -> Result<OperationResult, Error> {
    if matches!(
        command.maintenance(),
        MaintenanceKind::Reset | MaintenanceKind::GlobalReset
    ) {
        return execute_reset(connection, channel, command, context).await;
    }
    let mut wire = command.text().as_bytes().to_vec();
    wire.push(b'\n');
    channel.data_bytes(wire).await.map_err(transport_error)?;
    let mut decoder = GpasciiDecoder::new(command.text().as_bytes(), response_limit);
    let mut stderr = Vec::new();
    loop {
        let Some(message) = channel.wait().await else {
            if channel_failure_retires_connection {
                connection.retire();
            }
            return Err(disconnected(
                "GPASCII channel closed before a complete response",
            ));
        };
        match message {
            ChannelMsg::Data { data } => {
                let frames = decoder.push(&data).map_err(protocol_error)?;
                if let Some(frame) = frames.into_iter().next() {
                    if frame.remote_error {
                        let mut error = Error::new(
                            ErrorCode::RemoteError,
                            format!(
                                "controller rejected command: {}",
                                String::from_utf8_lossy(&frame.body)
                            ),
                        );
                        error.certainty = ExecutionCertainty::Confirmed;
                        return Err(error);
                    }
                    if frame.body.len().saturating_add(stderr.len()) > response_limit {
                        return Err(Error::new(
                            ErrorCode::ResourceLimit,
                            "combined GPASCII output limit exceeded",
                        )
                        .sent()
                        .fatal());
                    }
                    return Ok(OperationResult {
                        response: frame.body,
                        stderr,
                        exit_code: None,
                    });
                }
            }
            ChannelMsg::ExtendedData { data, .. } => {
                append_bounded(&mut stderr, &data, response_limit)?;
            }
            ChannelMsg::Failure => {
                return Err(disconnected("server rejected GPASCII exec request"))
            }
            ChannelMsg::Eof | ChannelMsg::Close => {
                if channel_failure_retires_connection {
                    connection.retire();
                }
                return Err(disconnected(
                    "GPASCII channel ended with an incomplete response",
                ));
            }
            _ => {}
        }
    }
}

async fn execute_reset(
    connection: &Arc<Connection>,
    channel: &mut Channel,
    command: &Command,
    context: &ExecutionContext,
) -> Result<OperationResult, Error> {
    let mut wire = command.text().as_bytes().to_vec();
    wire.push(b'\n');
    channel.data_bytes(wire).await.map_err(transport_error)?;
    let mut decoder = ResetDecoder::new(command.text().as_bytes(), context.max_response_bytes());
    let mut stderr = Vec::new();
    loop {
        match channel.wait().await {
            Some(ChannelMsg::Data { data }) => {
                for event in decoder.push(&data).map_err(protocol_error)? {
                    match event {
                        ResetEvent::ProbeRequired => {
                            let mut probe = RESET_READINESS_PROBE.as_bytes().to_vec();
                            probe.push(b'\n');
                            channel.data_bytes(probe).await.map_err(transport_error)?;
                            decoder.probe_sent().map_err(protocol_error)?;
                        }
                        ResetEvent::Complete(response) => {
                            connection.retire();
                            if response.len().saturating_add(stderr.len())
                                > context.max_response_bytes()
                            {
                                return Err(Error::new(
                                    ErrorCode::ResourceLimit,
                                    "combined reset output limit exceeded",
                                )
                                .sent()
                                .fatal());
                            }
                            return Ok(OperationResult {
                                response,
                                stderr,
                                exit_code: None,
                            });
                        }
                        ResetEvent::Rejected(response) => {
                            let mut error = Error::new(
                                ErrorCode::RemoteError,
                                format!(
                                    "controller rejected reset: {}",
                                    String::from_utf8_lossy(&response)
                                ),
                            );
                            error.certainty = ExecutionCertainty::Confirmed;
                            return Err(error);
                        }
                    }
                }
            }
            Some(ChannelMsg::ExtendedData { data, .. }) => {
                append_bounded(&mut stderr, &data, context.max_response_bytes())?
            }
            Some(ChannelMsg::Failure) => return Err(disconnected("server rejected reset request")),
            Some(ChannelMsg::Eof | ChannelMsg::Close) | None => {
                connection.retire();
                return Err(disconnected(
                    "reset channel ended without a complete ACK or verified reopened-input probe",
                ));
            }
            _ => {}
        }
    }
}
fn output_limit() -> Error {
    Error::new(
        ErrorCode::ResourceLimit,
        "combined shell output limit exceeded",
    )
    .sent()
    .fatal()
}
fn append_bounded(output: &mut Vec<u8>, bytes: &[u8], maximum: usize) -> Result<(), Error> {
    if output.len().saturating_add(bytes.len()) > maximum {
        return Err(output_limit());
    }
    output.extend_from_slice(bytes);
    Ok(())
}

pub(crate) fn validate_path(path: &str) -> Result<(), Error> {
    if path.is_empty() || path.as_bytes().contains(&0) || path.contains(['\r', '\n']) {
        Err(Error::invalid(
            "path must be non-empty UTF-8 without NUL or newlines",
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn shell_quote(path: &str) -> Result<String, Error> {
    validate_path(path)?;
    Ok(format!("'{}'", path.replace('\'', "'\\''")))
}

pub(crate) fn protocol_error(error: pmac_protocol::ProtocolError) -> Error {
    let mut mapped = Error::from(error);
    mapped.certainty = ExecutionCertainty::MayHaveExecuted;
    if mapped.code == ErrorCode::ProtocolError || mapped.code == ErrorCode::ResourceLimit {
        mapped.fatal_transport = true;
    }
    mapped
}

pub(crate) fn sftp_error(error: russh_sftp::client::error::Error) -> Error {
    let code = if is_not_found(&error) {
        ErrorCode::FileNotFound
    } else {
        ErrorCode::FileError
    };
    let fatal = match &error {
        russh_sftp::client::error::Error::Status(status) => matches!(
            status.status_code,
            russh_sftp::protocol::StatusCode::ConnectionLost
                | russh_sftp::protocol::StatusCode::NoConnection
                | russh_sftp::protocol::StatusCode::BadMessage
        ),
        russh_sftp::client::error::Error::Limited(_) => false,
        _ => true,
    };
    let mapped = Error::new(code, format!("SFTP operation failed: {error}")).sent();
    if fatal {
        mapped.fatal()
    } else {
        mapped
    }
}

pub(crate) fn is_not_found(error: &russh_sftp::client::error::Error) -> bool {
    matches!(error, russh_sftp::client::error::Error::Status(status) if status.status_code == russh_sftp::protocol::StatusCode::NoSuchFile)
}
