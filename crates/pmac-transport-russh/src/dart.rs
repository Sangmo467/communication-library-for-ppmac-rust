use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Weak};

use pmac_core::{Error, ErrorCode, ExecutionContext, OperationResult, Session};
use pmac_protocol::{response_text, Command, DartTransfer, DART_WRITE_ACK};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::session::RusshSession;

const MAX_DART_CONNECTIONS: usize = 10;

struct DartBudget {
    connections: Arc<Semaphore>,
}

struct DartConnectionPermit {
    _budget: Arc<DartBudget>,
    _permit: OwnedSemaphorePermit,
}

impl DartBudget {
    fn shared(host: &str) -> Arc<Self> {
        static REGISTRY: std::sync::OnceLock<
            std::sync::Mutex<HashMap<String, Weak<DartBudget>>>,
        > = std::sync::OnceLock::new();
        let registry = REGISTRY.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
        let key = host.to_ascii_lowercase();
        let mut registry = registry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(existing) = registry.get(&key).and_then(Weak::upgrade) {
            return existing;
        }
        let budget = Arc::new(Self {
            connections: Arc::new(Semaphore::new(MAX_DART_CONNECTIONS)),
        });
        registry.insert(key, Arc::downgrade(&budget));
        budget
    }

    fn reserve(self: &Arc<Self>) -> Result<DartConnectionPermit, Error> {
        let permit = self
            .connections
            .clone()
            .try_acquire_owned()
            .map_err(|_| {
                Error::new(
                    ErrorCode::ResourceLimit,
                    "controller DART TCP connection limit reached (10)",
                )
            })?;
        Ok(DartConnectionPermit {
            _budget: self.clone(),
            _permit: permit,
        })
    }
}

pub(crate) struct DartConnection {
    stream: TcpStream,
    _permit: DartConnectionPermit,
    pub(crate) port: u16,
}

pub(crate) async fn connect(
    session: &RusshSession,
    host: &str,
    context: &ExecutionContext,
) -> Result<DartConnection, Error> {
    let port = discover(session, context).await?;
    connect_port(host, port).await
}

pub(crate) async fn connect_port(host: &str, port: u16) -> Result<DartConnection, Error> {
    let budget = DartBudget::shared(host);
    let permit = budget.reserve()?;
    let stream = TcpStream::connect((host, port)).await.map_err(|error| {
        Error::new(
            ErrorCode::ConnectError,
            format!("DART TCP connection to port {port} failed: {error}"),
        )
    })?;
    stream.set_nodelay(true).map_err(|error| {
        Error::new(
            ErrorCode::ConnectError,
            format!("cannot configure DART TCP_NODELAY: {error}"),
        )
    })?;
    Ok(DartConnection {
        stream,
        _permit: permit,
        port,
    })
}

pub(crate) async fn disconnect(mut connection: DartConnection) -> Result<(), Error> {
    connection.stream.shutdown().await.map_err(|error| {
        Error::new(
            ErrorCode::Disconnected,
            format!("DART TCP shutdown failed: {error}"),
        )
    })
}

pub(crate) fn transfer(
    connection: &mut DartConnection,
    transfer: DartTransfer,
) -> impl Future<Output = Result<OperationResult, Error>> + '_ {
    let header = transfer.wire_header();
    let command = transfer.command();
    async move {
        connection
            .stream
            .write_all(&header)
            .await
            .map_err(|error| dart_io_error("DART header write failed", error))?;

        if command.is_write() {
            let data = transfer.into_wire_data();
            connection
                .stream
                .write_all(&data)
                .await
                .map_err(|error| dart_io_error("DART data write failed", error))?;
            let mut acknowledgement = [0];
            connection
                .stream
                .read_exact(&mut acknowledgement)
                .await
                .map_err(|error| dart_io_error("DART acknowledgement read failed", error))?;
            if acknowledgement[0] != DART_WRITE_ACK {
                return Err(Error::new(
                    ErrorCode::ProtocolError,
                    format!(
                        "invalid DART write acknowledgement 0x{:02x}; expected 0xba",
                        acknowledgement[0]
                    ),
                )
                .sent());
            }
            Ok(OperationResult::default())
        } else {
            let mut response = Vec::new();
            response.try_reserve_exact(transfer.len()).map_err(|_| {
                Error::new(
                    ErrorCode::ResourceLimit,
                    "cannot preallocate the DART response buffer",
                )
            })?;
            response.resize(transfer.len(), 0);
            connection
                .stream
                .read_exact(&mut response)
                .await
                .map_err(|error| dart_io_error("DART response read failed", error))?;
            Ok(OperationResult::response(command.wire_to_native(response)))
        }
    }
}

async fn discover(session: &RusshSession, context: &ExecutionContext) -> Result<u16, Error> {
    let enabled = session
        .command(&Command::raw("Dart.Enable")?, context)
        .await
        .map_err(discovery_error)?;
    if discovery_value(&enabled.response, "Dart.Enable")? != 1 {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "DART is disabled or unsupported because Dart.Enable did not return 1",
        ));
    }
    let port = session
        .command(&Command::raw("Dart.Port")?, context)
        .await
        .map_err(discovery_error)?;
    let port = discovery_value(&port.response, "Dart.Port")?;
    u16::try_from(port).ok().filter(|port| *port != 0).ok_or_else(|| {
        Error::new(
            ErrorCode::Unsupported,
            "DART service returned an invalid TCP port",
        )
    })
}

fn discovery_value(response: &[u8], expected_name: &str) -> Result<u64, Error> {
    let text = response_text(response).map_err(|_| {
        Error::new(
            ErrorCode::Unsupported,
            format!("{expected_name} did not return a UTF-8 numeric value"),
        )
    })?;
    if let Some((name, value)) = text.split_once('=') {
        if name.trim() != expected_name || value.contains('=') {
            return Err(Error::new(
                ErrorCode::Unsupported,
                format!("DART discovery returned the wrong response for {expected_name}"),
            ));
        }
    }
    pmac_protocol::parse_u64(response).map_err(|_| {
        Error::new(
            ErrorCode::Unsupported,
            format!("{expected_name} did not return a valid unsigned integer"),
        )
    })
}

fn discovery_error(error: Error) -> Error {
    if error.fatal_transport
        || !matches!(
            error.code,
            ErrorCode::RemoteError | ErrorCode::ParseError | ErrorCode::ProtocolError
        )
    {
        return error;
    }
    Error::new(
        ErrorCode::Unsupported,
        "controller does not provide the required DART discovery response",
    )
}

fn dart_io_error(operation: &str, error: std::io::Error) -> Error {
    Error::new(
        ErrorCode::Disconnected,
        format!("{operation}: {error}"),
    )
    .sent()
}
