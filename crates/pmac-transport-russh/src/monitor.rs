//! Independent PTY-backed geterrors stream. It never owns the ordinary shell lane.
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use pmac_core::{Error, ErrorCode, ErrorMonitorStream, GetErrorsOptions, MonitorChunk};
use russh::{ChannelMsg, Pty};

use crate::connection::{transport_error, Channel, Connection};

const CHUNK_LIMIT: usize = 32 * 1024;
const BUFFER_LIMIT: usize = 256 * 1024;

pub(crate) async fn open(
    connection: Arc<Connection>,
    options: &GetErrorsOptions,
) -> Result<Box<dyn ErrorMonitorStream>, Error> {
    options.validate()?;
    let channel = tokio::time::timeout(Duration::from_secs(5), connection.open_channel())
        .await
        .map_err(|_| Error::new(ErrorCode::ExecTimeout, "geterrors channel open timed out"))??;
    let mut stream = GetErrorsStream {
        channel: Some(channel),
        connection,
        runtime: tokio::runtime::Handle::current(),
        pending: VecDeque::new(),
        ended: false,
        exec_requested: false,
        exit_confirmed: false,
        close_confirmed: false,
        stop_error: None,
    };
    let opened = tokio::time::timeout(Duration::from_secs(5), stream.initialize(options)).await;
    let result = opened.unwrap_or_else(|_| {
        Err(Error::new(
            ErrorCode::ExecTimeout,
            "geterrors initialization timed out",
        ))
    });
    if let Err(mut error) = result {
        if let Err(cleanup) = stream.stop().await {
            error
                .message
                .push_str(&format!("; geterrors cleanup failed: {}", cleanup.message));
        }
        return Err(error);
    }
    Ok(Box::new(stream))
}
fn command(options: &GetErrorsOptions) -> String {
    let mut text = format!("geterrors -t{}", options.period_seconds);
    if options.append_date {
        text.push_str(" -T");
    }
    for (flag, mask) in [
        ('c', options.coord_mask),
        ('m', options.motor_mask),
        ('r', options.ring_mask),
        ('g', options.global_mask),
    ] {
        if let Some(mask) = mask {
            use std::fmt::Write;
            let _ = write!(text, " -{flag}0x{mask:x}");
        }
    }
    text
}
struct GetErrorsStream {
    channel: Option<Channel>,
    connection: Arc<Connection>,
    runtime: tokio::runtime::Handle,
    pending: VecDeque<MonitorChunk>,
    ended: bool,
    exec_requested: bool,
    exit_confirmed: bool,
    close_confirmed: bool,
    stop_error: Option<Error>,
}
impl GetErrorsStream {
    async fn initialize(&mut self, options: &GetErrorsOptions) -> Result<(), Error> {
        self.channel
            .as_ref()
            .unwrap()
            .request_pty(
                true,
                "dumb",
                80,
                24,
                0,
                0,
                &[
                    (Pty::ISIG, 1),
                    (Pty::VINTR, 3),
                    (Pty::ECHO, 0),
                    (Pty::ECHONL, 0),
                ],
            )
            .await
            .map_err(transport_error)?;
        self.reply("PTY").await?;
        self.exec_requested = true;
        self.channel
            .as_ref()
            .unwrap()
            .exec(true, command(options))
            .await
            .map_err(transport_error)?;
        self.reply("exec").await
    }
    async fn reply(&mut self, stage: &str) -> Result<(), Error> {
        loop {
            match self.channel.as_mut().unwrap().wait().await {
                Some(ChannelMsg::Success) => return Ok(()),
                Some(ChannelMsg::Failure) => {
                    self.exec_requested = false;
                    return Err(Error::new(
                        ErrorCode::RemoteError,
                        format!("server rejected geterrors {stage} request"),
                    ));
                }
                Some(ChannelMsg::Data { data }) => self.enqueue(data.to_vec(), false)?,
                Some(ChannelMsg::ExtendedData { data, .. }) => self.enqueue(data.to_vec(), true)?,
                Some(ChannelMsg::ExitStatus { .. } | ChannelMsg::ExitSignal { .. }) => {
                    self.exit_confirmed = true;
                    return Err(Error::new(
                        ErrorCode::RemoteError,
                        "geterrors exited before startup confirmation",
                    ));
                }
                Some(ChannelMsg::Close) => {
                    self.close_confirmed = true;
                    return Err(Error::disconnected(
                        "geterrors channel closed during startup",
                    ));
                }
                Some(ChannelMsg::Eof) | None => {
                    return Err(Error::disconnected("geterrors stream ended during startup"))
                }
                _ => {}
            }
        }
    }
    fn enqueue(&mut self, data: Vec<u8>, stderr: bool) -> Result<(), Error> {
        let retained: usize = self.pending.iter().map(|chunk| chunk.data.len()).sum();
        if data.len().saturating_add(retained) > BUFFER_LIMIT {
            return Err(Error::new(
                ErrorCode::ResourceLimit,
                "geterrors buffered output exceeds 256 KiB",
            ));
        }
        for bytes in data.chunks(CHUNK_LIMIT) {
            self.pending.push_back(MonitorChunk {
                data: bytes.to_vec(),
                stderr,
            });
        }
        Ok(())
    }
    async fn cleanup(&mut self) -> Result<(), Error> {
        let channel = self.channel.as_mut().unwrap();
        if self.exec_requested && !self.exit_confirmed && !self.close_confirmed {
            // A dedicated PTY with ISIG/VINTR turns ETX into SIGINT for this process.
            channel.data(&b"\x03"[..]).await.map_err(transport_error)?;
        }
        if !self.exec_requested || self.exit_confirmed {
            channel.close().await.map_err(transport_error)?;
        }
        while !self.close_confirmed {
            match channel.wait().await {
                Some(ChannelMsg::ExitStatus { .. } | ChannelMsg::ExitSignal { .. }) => {
                    self.exit_confirmed = true;
                    channel.close().await.map_err(transport_error)?;
                }
                Some(ChannelMsg::Close) => {
                    self.close_confirmed = true;
                    channel.close().await.map_err(transport_error)?;
                }
                None => {
                    return Err(Error::disconnected(
                        "geterrors cleanup ended without channel close confirmation",
                    ))
                }
                _ => {} // Drain pending output while waiting for the interrupted process.
            }
        }
        if self.exec_requested && !self.exit_confirmed {
            return Err(Error::disconnected(
                "geterrors process exit was not confirmed",
            ));
        }
        Ok(())
    }
}
#[async_trait]
impl ErrorMonitorStream for GetErrorsStream {
    async fn read(&mut self) -> Result<Option<MonitorChunk>, Error> {
        loop {
            if let Some(chunk) = self.pending.pop_front() {
                return Ok(Some(chunk));
            }
            if self.ended || self.channel.is_none() {
                return Ok(None);
            }
            match self.channel.as_mut().unwrap().wait().await {
                Some(ChannelMsg::Data { data }) => self.enqueue(data.to_vec(), false)?,
                Some(ChannelMsg::ExtendedData { data, .. }) => self.enqueue(data.to_vec(), true)?,
                Some(ChannelMsg::ExitStatus { exit_status }) => {
                    self.exit_confirmed = true;
                    self.ended = true;
                    if exit_status != 0 {
                        return Err(Error::new(
                            ErrorCode::RemoteError,
                            format!("geterrors exited with status {exit_status}"),
                        ));
                    }
                    return Ok(None);
                }
                Some(ChannelMsg::ExitSignal { signal_name, .. }) => {
                    self.exit_confirmed = true;
                    self.ended = true;
                    return Err(Error::new(
                        ErrorCode::RemoteError,
                        format!("geterrors exited on signal {signal_name:?}"),
                    ));
                }
                Some(ChannelMsg::Eof) => {
                    self.ended = true;
                    return Ok(None);
                }
                Some(ChannelMsg::Close) => {
                    self.close_confirmed = true;
                    self.ended = true;
                    return Err(Error::disconnected("geterrors channel closed unexpectedly"));
                }
                Some(ChannelMsg::Failure) | None => {
                    self.ended = true;
                    return Err(Error::disconnected("geterrors monitor connection ended"));
                }
                _ => {}
            }
        }
    }
    async fn stop(&mut self) -> Result<(), Error> {
        if self.channel.is_some() {
            let mut result = tokio::time::timeout(Duration::from_secs(2), self.cleanup())
                .await
                .unwrap_or_else(|_| {
                    Err(Error::new(
                        ErrorCode::ExecTimeout,
                        "geterrors process shutdown timed out; remote exit is unconfirmed",
                    ))
                });
            if result.is_err() {
                if let Some(channel) = &self.channel {
                    let _ = tokio::time::timeout(Duration::from_millis(100), channel.close()).await;
                }
            }
            if let Some(channel) = self.channel.take() {
                // Consume the connection's close-event bookkeeping too. A monitor
                // must not leave one cached ChannelId behind on every normal stop.
                let closed = tokio::time::timeout(
                    Duration::from_millis(100),
                    self.connection.close_channel(channel),
                )
                .await;
                if result.is_ok() && !matches!(closed, Ok(true)) {
                    result = Err(Error::new(
                        ErrorCode::ShutdownPending,
                        "geterrors close handshake is unconfirmed",
                    ));
                }
            }
            self.pending.clear();
            self.ended = true;
            self.stop_error = result.err();
        }
        self.stop_error.clone().map_or(Ok(()), Err)
    }
}
impl Drop for GetErrorsStream {
    fn drop(&mut self) {
        if let Some(channel) = self.channel.take() {
            let mut cleanup = Self {
                channel: Some(channel),
                connection: self.connection.clone(),
                runtime: self.runtime.clone(),
                pending: VecDeque::new(),
                ended: true,
                exec_requested: self.exec_requested,
                exit_confirmed: self.exit_confirmed,
                close_confirmed: self.close_confirmed,
                stop_error: None,
            };
            self.runtime.spawn(async move {
                let _ = cleanup.stop().await;
            });
        }
    }
}
