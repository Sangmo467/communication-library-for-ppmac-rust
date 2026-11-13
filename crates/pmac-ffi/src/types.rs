use pmac_core::RequestInfo;

pub const ABI_MAJOR: u32 = 2;
pub const ABI_MINOR: u32 = 1;
pub const ABI_VERSION: u32 = ABI_MAJOR * 1000 + ABI_MINOR;
pub const CONNECT_TCP_PROBE: u32 = 1;
pub const CONNECT_ACCEPT_ANY_HOST_KEY: u32 = 2;
pub const REQUEST_INFINITE: u32 = 1;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PmacBytes {
    pub data: *const u8,
    pub len: usize,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PmacRuntimeConfig {
    pub struct_size: u32,
    pub abi_version: u32,
    pub worker_threads: u32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PmacClientConfig {
    pub struct_size: u32,
    pub abi_version: u32,
    pub max_requests: u32,
    pub max_file_tasks: u32,
    pub max_retained_bytes: usize,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PmacConnectOptions {
    pub struct_size: u32,
    pub abi_version: u32,
    pub host: PmacBytes,
    pub username: PmacBytes,
    pub password: PmacBytes,
    pub private_key: PmacBytes,
    pub public_key: PmacBytes,
    pub passphrase: PmacBytes,
    pub known_hosts: PmacBytes,
    pub pinned_sha256: PmacBytes,
    pub port: u16,
    pub control_pool_size: u8,
    pub reserved0: u8,
    pub flags: u32,
    pub tcp_probe_timeout_ms: u32,
    pub keepalive_interval_ms: u32,
    pub keepalive_max: u32,
    pub reserved: [u64; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PmacRequestOptions {
    pub struct_size: u32,
    pub abi_version: u32,
    pub timeout_ms: u32,
    pub flags: u32,
    pub max_response_bytes: usize,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PmacRequestInfo {
    pub struct_size: u32,
    pub abi_version: u32,
    pub request_id: u64,
    pub client_id: u64,
    pub state: u32,
    pub status: i32,
    pub certainty: u32,
    pub reserved: u32,
    pub connection_epoch: u64,
    pub elapsed_us: u64,
    pub response_len: usize,
    pub stderr_len: usize,
    pub exit_code: i32,
    pub has_exit_code: u32,
}

impl PmacRequestInfo {
    pub fn from_request(
        info: RequestInfo,
        response_len: usize,
        stderr_len: usize,
        exit_code: Option<i32>,
    ) -> Self {
        Self {
            struct_size: size_of::<Self>() as u32,
            abi_version: ABI_VERSION,
            request_id: info.request_id,
            client_id: info.client_id,
            state: info.state as u32,
            status: info.code as i32,
            certainty: info.certainty as u32,
            reserved: 0,
            connection_epoch: info.connection_epoch,
            elapsed_us: info.elapsed_us,
            response_len,
            stderr_len,
            exit_code: exit_code.unwrap_or(0),
            has_exit_code: u32::from(exit_code.is_some()),
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PmacErrorInfo {
    pub struct_size: u32,
    pub abi_version: u32,
    pub status: i32,
    pub certainty: u32,
    pub request_id: u64,
    pub connection_epoch: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PmacStateSnapshot {
    pub struct_size: u32,
    pub abi_version: u32,
    pub state: u32,
    pub closing: u32,
    pub connection_epoch: u64,
    pub last_valid_response_unix_ms: u64,
    pub active_requests: usize,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PmacRuntimeInfo {
    pub struct_size: u32,
    pub abi_major: u32,
    pub abi_minor: u32,
    pub version_number: u32,
    pub feature_flags: u32,
    pub pointer_width: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PmacPoolProfile {
    pub struct_size: u32,
    pub abi_version: u32,
    pub target_size: u32,
    pub max_members: u32,
    pub min_ready: u32,
    pub connect_timeout_ms: u32,
}
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PmacPoolState {
    pub struct_size: u32,
    pub abi_version: u32,
    pub revision: u64,
    pub desired: u32,
    pub ready: u32,
    pub busy: u32,
    pub creating: u32,
    pub failed: u32,
    pub retiring: u32,
    pub release_uncertain: u32,
    pub max_members: u32,
    pub unknown_write: u32,
    pub maintenance_active: u32,
    pub maintenance_failed: u32,
    pub last_create_error: i32,
}
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PmacPoolMember {
    pub struct_size: u32,
    pub abi_version: u32,
    pub member_id: u64,
    pub epoch: u64,
    pub state: u32,
    pub control: u32,
    pub last_valid_response_unix_ms: u64,
    pub last_error: i32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PmacGatherConfig {
    pub struct_size: u32,
    pub abi_version: u32,
    pub period: u32,
    pub items: u32,
    pub addresses: *const PmacBytes,
    pub local_ip: PmacBytes,
    pub port: u16,
    pub reserved: u16,
    pub max_samples: u32,
    pub batch_rows: u32,
    pub queue_batches: u32,
    pub timeout_ms: u32,
}
