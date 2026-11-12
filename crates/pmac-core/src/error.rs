use pmac_protocol::ProtocolError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ErrorCode {
    Ok = 0,
    InvalidArgument = -1,
    InvalidHandle = -2,
    QueueFull = -3,
    QueueTimeout = -4,
    ExecTimeout = -5,
    WaitPending = -6,
    Cancelled = -7,
    Disconnected = -8,
    ProtocolError = -9,
    RemoteError = -10,
    BufferTooSmall = -11,
    ResourceLimit = -12,
    Closing = -13,
    ShutdownPending = -14,
    ConnectError = -15,
    Authentication = -16,
    HostKey = -17,
    FileNotFound = -18,
    FileError = -19,
    ChecksumMismatch = -20,
    PublishUnknown = -21,
    ParseError = -22,
    NoReadyLane = -23,
    PoolWaitTimeout = -24,
    Unsupported = -25,
    Internal = -26,
    RecoveryRequired = -27,
    MaintenanceActive = -28,
    Superseded = -29,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ExecutionCertainty {
    NotSent = 0,
    MayHaveExecuted = 1,
    Confirmed = 2,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub code: ErrorCode,
    pub message: String,
    pub certainty: ExecutionCertainty,
    pub fatal_transport: bool,
}

impl Error {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            certainty: ExecutionCertainty::NotSent,
            fatal_transport: false,
        }
    }

    pub fn sent(mut self) -> Self {
        self.certainty = ExecutionCertainty::MayHaveExecuted;
        self
    }

    pub fn fatal(mut self) -> Self {
        self.fatal_transport = true;
        self
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidArgument, message)
    }
    pub fn disconnected(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Disconnected, message).fatal()
    }
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, message)
    }
}

impl From<ProtocolError> for Error {
    fn from(error: ProtocolError) -> Self {
        match error {
            ProtocolError::RemoteError(message) => Self::new(ErrorCode::RemoteError, message),
            ProtocolError::InvalidNumber(message) => Self::new(ErrorCode::ParseError, message),
            ProtocolError::ResponseTooLarge(limit) => Self::new(
                ErrorCode::ResourceLimit,
                format!("response exceeds {limit} bytes"),
            ),
            ProtocolError::IncompleteFrame | ProtocolError::UnexpectedData(_) => {
                Self::new(ErrorCode::ProtocolError, error.to_string()).fatal()
            }
            ProtocolError::Regex(message) => Self::new(ErrorCode::InvalidArgument, message),
            other => Self::new(ErrorCode::InvalidArgument, other.to_string()),
        }
    }
}
