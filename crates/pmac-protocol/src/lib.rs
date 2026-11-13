#![forbid(unsafe_code)]

mod command;
mod dart;
mod error;
mod gpascii;
mod online;
mod parse;
mod reset;
mod shell;
mod status;

pub use command::{Command, CommandEffect, CommandScope, MaintenanceKind, MAX_COMMAND_LEN};
pub use dart::{
    DartCommand, DartTransfer, DART_MAX_DATA_BYTES, DART_MAX_P_INDEX, DART_USHM_BYTES,
    DART_WRITE_ACK,
};
pub use error::ProtocolError;
pub use gpascii::{GpasciiDecoder, GpasciiFrame};
pub use parse::{
    parse_bool, parse_f64, parse_u32, parse_u64, regex_match, response_text, RegexMatch,
};
pub use shell::{ShellDecoder, ShellEvent, ShellMarkers, ShellStderrDecoder};
pub use status::{CoordStatus, MotorStatus};

pub use reset::{ResetDecoder, ResetEvent, RESET_READINESS_PROBE};
