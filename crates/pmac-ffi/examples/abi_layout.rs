// Generated from include/pmac.h by scripts/Update-AbiProbe.ps1.
#![allow(dead_code)]
#[path = "../src/types.rs"]
mod types;
use std::mem::{align_of, offset_of, size_of};
use types::*;
macro_rules! layout {
    ($ty:ty, $name:literal; $($field:ident),* $(,)?) => {
        println!(concat!($name, ".size={}"), size_of::<$ty>());
        println!(concat!($name, ".align={}"), align_of::<$ty>());
        $(println!(concat!($name, ".", stringify!($field), "={}"), offset_of!($ty, $field));)*
    };
}
fn main() {
    println!("runtime.pointer_width={}", usize::BITS);
    layout!(PmacBytes, "pmac_bytes"; data, len);
    layout!(PmacRuntimeConfig, "pmac_runtime_config"; struct_size, abi_version, worker_threads, reserved);
    layout!(PmacClientConfig, "pmac_client_config"; struct_size, abi_version, max_requests, max_file_tasks, max_retained_bytes);
    layout!(PmacConnectOptions, "pmac_connect_options"; struct_size, abi_version, host, username, password, private_key, public_key, passphrase, known_hosts, pinned_sha256, port, control_pool_size, reserved0, flags, tcp_probe_timeout_ms, keepalive_interval_ms, keepalive_max, reserved);
    layout!(PmacRequestOptions, "pmac_request_options"; struct_size, abi_version, timeout_ms, flags, max_response_bytes);
    layout!(PmacRequestInfo, "pmac_request_info"; struct_size, abi_version, request_id, client_id, state, status, certainty, reserved, connection_epoch, elapsed_us, response_len, stderr_len, exit_code, has_exit_code);
    layout!(PmacErrorInfo, "pmac_error_info"; struct_size, abi_version, status, certainty, request_id, connection_epoch);
    layout!(PmacStateSnapshot, "pmac_state_snapshot"; struct_size, abi_version, state, closing, connection_epoch, last_valid_response_unix_ms, active_requests);
    layout!(PmacRuntimeInfo, "pmac_runtime_info"; struct_size, abi_major, abi_minor, version_number, feature_flags, pointer_width);
    layout!(PmacPoolProfile, "pmac_pool_profile"; struct_size, abi_version, target_size, max_members, min_ready, connect_timeout_ms);
    layout!(PmacPoolState, "pmac_pool_state"; struct_size, abi_version, revision, desired, ready, busy, creating, failed, retiring, release_uncertain, max_members, unknown_write, maintenance_active, maintenance_failed, last_create_error);
    layout!(PmacPoolMember, "pmac_pool_member"; struct_size, abi_version, member_id, epoch, state, control, last_valid_response_unix_ms, last_error, reserved);
    layout!(PmacGetErrorsConfig, "pmac_geterrors_config"; struct_size, abi_version, period_seconds, coord_mask, motor_mask, ring_mask, global_mask, flags, timeout_ms, queue_lines, max_line_bytes);
    layout!(PmacGatherConfig, "pmac_gather_config"; struct_size, abi_version, period, items, addresses, local_ip, port, reserved, max_samples, batch_rows, queue_batches, timeout_ms);
}
