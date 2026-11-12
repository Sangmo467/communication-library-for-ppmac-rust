#![forbid(unsafe_code)]

mod budget;
mod client;
mod completion;
mod error;
mod pool;
mod request;
mod runtime;
mod transport;

pub use client::{Client, ClientConfig, ClientState, StateSnapshot};
pub use error::{Error, ErrorCode, ExecutionCertainty};
pub use request::{
    Operation, OperationResult, Request, RequestInfo, RequestOptions, RequestSnapshot,
    RequestState, WaitOutcome,
};
pub use runtime::Runtime;
pub use transport::{
    ConnectOptions, ExecutionContext, GpasciiBudget, GpasciiChannelPermit, HostKeyPolicy, Session,
    Transport,
};

pub use pool::{MemberSnapshot, MemberState, PoolProfile, PoolSnapshot};
