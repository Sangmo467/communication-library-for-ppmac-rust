use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    #[error("command is empty")]
    EmptyCommand,
    #[error("command length {actual} exceeds {maximum} bytes")]
    CommandTooLong { actual: usize, maximum: usize },
    #[error("command contains a forbidden control character")]
    InvalidCommandCharacter,
    #[error("invalid {kind} target {target}; expected {expected}")]
    InvalidTarget {
        kind: &'static str,
        target: i32,
        expected: &'static str,
    },
    #[error("command has a missing or ambiguous motor/coordinate target: {0}")]
    AmbiguousTarget(String),
    #[error("unsupported or unsafe raw command: {0}")]
    UnsupportedCommand(String),
    #[error("invalid finite floating-point value")]
    InvalidFloat,
    #[error("invalid numeric value: {0}")]
    InvalidNumber(String),
    #[error("response exceeds configured limit of {0} bytes")]
    ResponseTooLarge(usize),
    #[error("unexpected response data: {0}")]
    UnexpectedData(String),
    #[error("response ended before a complete frame was received")]
    IncompleteFrame,
    #[error("remote device returned an error: {0}")]
    RemoteError(String),
    #[error("shell protocol error: {0}")]
    ShellProtocol(String),
    #[error("regular expression error: {0}")]
    Regex(String),
}
