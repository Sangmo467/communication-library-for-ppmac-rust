import ctypes as C
import os

sizeof = C.sizeof
byref = C.byref

class Bytes(C.Structure):
    _fields_ = [("data", C.POINTER(C.c_uint8)), ("len", C.c_size_t)]
class RuntimeConfig(C.Structure):
    _fields_ = [(name, C.c_uint32) for name in ("struct_size", "abi_version", "worker_threads", "reserved")]
class RuntimeInfo(C.Structure):
    _fields_ = [(name, C.c_uint32) for name in ("struct_size", "abi_major", "abi_minor", "version_number", "feature_flags", "pointer_width")]
class ClientConfig(C.Structure):
    _fields_ = [(name, C.c_uint32) for name in ("struct_size", "abi_version", "max_requests", "max_file_tasks")] + [("max_retained_bytes", C.c_size_t)]
class ConnectOptions(C.Structure):
    _fields_ = [("struct_size", C.c_uint32), ("abi_version", C.c_uint32)] + [(name, Bytes) for name in ("host", "username", "password", "private_key", "public_key", "passphrase", "known_hosts", "pinned_sha256")] + [("port", C.c_uint16), ("control_pool_size", C.c_uint8), ("reserved0", C.c_uint8), ("flags", C.c_uint32), ("tcp_probe_timeout_ms", C.c_uint32), ("keepalive_interval_ms", C.c_uint32), ("keepalive_max", C.c_uint32), ("reserved", C.c_uint64 * 4)]
class RequestOptions(C.Structure):
    _fields_ = [("struct_size", C.c_uint32), ("abi_version", C.c_uint32), ("timeout_ms", C.c_uint32), ("flags", C.c_uint32), ("max_response_bytes", C.c_size_t)]
class RequestInfo(C.Structure):
    _fields_ = [("struct_size", C.c_uint32), ("abi_version", C.c_uint32), ("request_id", C.c_uint64), ("client_id", C.c_uint64), ("state", C.c_uint32), ("status", C.c_int32), ("certainty", C.c_uint32), ("reserved", C.c_uint32), ("connection_epoch", C.c_uint64), ("elapsed_us", C.c_uint64), ("response_len", C.c_size_t), ("stderr_len", C.c_size_t), ("exit_code", C.c_int32), ("has_exit_code", C.c_uint32)]
class ErrorInfo(C.Structure):
    _fields_ = [("struct_size", C.c_uint32), ("abi_version", C.c_uint32), ("status", C.c_int32), ("certainty", C.c_uint32), ("request_id", C.c_uint64), ("connection_epoch", C.c_uint64)]
class StateSnapshot(C.Structure):
    _fields_ = [("struct_size", C.c_uint32), ("abi_version", C.c_uint32), ("state", C.c_uint32), ("closing", C.c_uint32), ("connection_epoch", C.c_uint64), ("last_valid_response_unix_ms", C.c_uint64), ("active_requests", C.c_size_t)]

class PoolProfile(C.Structure):
    _fields_ = [(name, C.c_uint32) for name in ("struct_size", "abi_version", "target_size", "max_members", "min_ready", "connect_timeout_ms")]

class PoolState(C.Structure):
    _fields_ = [("struct_size", C.c_uint32), ("abi_version", C.c_uint32), ("revision", C.c_uint64)] + [(name, C.c_uint32) for name in ("desired", "ready", "busy", "creating", "failed", "retiring", "release_uncertain", "max_members", "unknown_write", "maintenance_active", "maintenance_failed")] + [("last_create_error", C.c_int32)]

class PoolMember(C.Structure):
    _fields_ = [("struct_size", C.c_uint32), ("abi_version", C.c_uint32), ("member_id", C.c_uint64), ("epoch", C.c_uint64), ("state", C.c_uint32), ("control", C.c_uint32), ("last_valid_response_unix_ms", C.c_uint64), ("last_error", C.c_int32), ("reserved", C.c_uint32)]

def load(path=None):
    if path is None:
        path = "pmac.dll" if os.name == "nt" else "libpmac.so"
    lib = C.CDLL(path)
    lib.pmac_version_string.restype = C.c_char_p
    lib.pmac_version_number.restype = C.c_uint32
    lib.pmac_get_runtime_info.argtypes = [C.POINTER(RuntimeInfo)]
    lib.pmac_abi_version.argtypes = []
    lib.pmac_abi_version.restype = C.c_uint32
    if lib.pmac_abi_version() // 1000 != 2:
        raise RuntimeError("native library ABI major is incompatible with this SDK")
    lib.pmac_runtime_create.argtypes = [C.POINTER(RuntimeConfig), C.POINTER(C.c_uint64)]
    lib.pmac_runtime_shutdown.argtypes = [C.c_uint64, C.c_uint32]
    if hasattr(lib, "pmac_runtime_wait_completion"):
        lib.pmac_runtime_wait_completion.argtypes = [C.c_uint64, C.POINTER(C.c_uint64), C.c_uint32]
        lib.pmac_runtime_wake.argtypes = [C.c_uint64]
    lib.pmac_runtime_release.argtypes = [C.c_uint64]
    lib.pmac_client_create.argtypes = [C.c_uint64, C.POINTER(ClientConfig), C.POINTER(C.c_uint64)]
    lib.pmac_client_close.argtypes = [C.c_uint64, C.c_uint32, C.c_uint32]
    lib.pmac_client_release.argtypes = [C.c_uint64]
    lib.pmac_client_get_state.argtypes = [C.c_uint64, C.POINTER(StateSnapshot)]
    lib.pmac_connect_submit.argtypes = [C.c_uint64, C.POINTER(ConnectOptions), C.POINTER(C.c_uint64)]
    submit = [C.c_uint64, C.POINTER(C.c_uint8), C.c_size_t, C.POINTER(RequestOptions), C.POINTER(C.c_uint64)]
    lib.pmac_command_submit.argtypes = submit
    lib.pmac_get_response_async.argtypes = submit
    lib.pmac_shell_submit.argtypes = submit[:3] + [C.c_uint32] + submit[3:]
    two_paths = [C.c_uint64, C.POINTER(C.c_uint8), C.c_size_t, C.POINTER(C.c_uint8), C.c_size_t, C.POINTER(RequestOptions), C.POINTER(C.c_uint64)]
    lib.pmac_upload_submit.argtypes = two_paths
    lib.pmac_download_submit.argtypes = two_paths
    lib.pmac_buffer_upload_submit.argtypes = two_paths[:5] + [C.c_uint32] + two_paths[5:]
    lib.pmac_file_exists_submit.argtypes = submit
    lib.pmac_file_remove_submit.argtypes = submit
    lib.pmac_request_wait.argtypes = [C.c_uint64, C.c_uint32, C.POINTER(RequestInfo)]
    if hasattr(lib, "pmac_request_get_route"):
        lib.pmac_request_get_route.argtypes = [C.c_uint64, C.POINTER(C.c_uint64), C.POINTER(C.c_uint64)]
    lib.pmac_request_cancel.argtypes = [C.c_uint64]
    copy = [C.c_uint64, C.POINTER(C.c_uint8), C.c_size_t, C.POINTER(C.c_size_t)]
    lib.pmac_request_copy_response.argtypes = copy
    lib.pmac_request_copy_stderr.argtypes = copy
    lib.pmac_request_copy_error.argtypes = [C.c_uint64, C.POINTER(ErrorInfo), C.POINTER(C.c_uint8), C.c_size_t, C.POINTER(C.c_size_t)]
    lib.pmac_request_copy_match.argtypes = [C.c_uint64, C.POINTER(C.c_uint8), C.c_size_t, C.c_uint32] + copy[1:]
    lib.pmac_request_release.argtypes = [C.c_uint64]
    unit = [C.c_uint64, C.POINTER(RequestOptions)]
    target = [C.c_uint64, C.c_int32, C.POINTER(RequestOptions)]
    for name in ("abort",): getattr(lib, "pmac_" + name).argtypes = unit
    for name in ("coord_enable", "coord_hold", "coord_run", "plc_enable", "plc_disable", "motor_kill", "motor_enable", "motor_jog_stop"):
        getattr(lib, "pmac_" + name).argtypes = target
    lib.pmac_coord_exec_prog.argtypes = [C.c_uint64, C.c_int32, C.c_int32, C.POINTER(RequestOptions)]
    for name in ("motor_abs_jog", "motor_inc_jog"):
        getattr(lib, "pmac_" + name).argtypes = [C.c_uint64, C.c_int32, C.c_double, C.POINTER(RequestOptions)]
    for name in ("coord_status", "motor_status"):
        getattr(lib, "pmac_" + name).argtypes = target + [C.POINTER(C.c_uint64)]
    for name in ("motor_position", "motor_desired_position", "motor_velocity", "motor_position_error"):
        getattr(lib, "pmac_" + name).argtypes = target + [C.POINTER(C.c_double)]
    for name in ("servo_error_count", "phase_error_count", "servo_count"):
        getattr(lib, "pmac_" + name).argtypes = unit + [C.POINTER(C.c_uint32)]
    for name in ("phase_time", "servo_time"):
        getattr(lib, "pmac_" + name).argtypes = unit + [C.POINTER(C.c_double)]
    lib.pmac_abort_all_flag.argtypes = unit + [C.POINTER(C.c_uint32)]
    for name in ("plc_running", "coord_prog_running", "motor_closed_loop", "coord_motor_enabled"):
        getattr(lib, "pmac_" + name).argtypes = target + [C.POINTER(C.c_uint32)]
    pool_functions = {
        "pmac_pool_prepare_async": [C.c_uint64, C.POINTER(ConnectOptions), C.POINTER(PoolProfile), C.POINTER(C.c_uint64)],
        "pmac_pool_resize_async": [C.c_uint64, C.POINTER(PoolProfile), C.POINTER(C.c_uint64)],
        "pmac_pool_repair_async": [C.c_uint64, C.c_uint64, C.POINTER(RequestOptions), C.POINTER(C.c_uint64)],
        "pmac_pool_drain_async": [C.c_uint64, C.c_uint64, C.POINTER(RequestOptions), C.POINTER(C.c_uint64)],
        "pmac_pool_repair_maintenance_async": [C.c_uint64, C.POINTER(RequestOptions), C.POINTER(C.c_uint64)],
        "pmac_pool_get_state": [C.c_uint64, C.POINTER(PoolState)],
        "pmac_pool_get_members": [C.c_uint64, C.POINTER(PoolMember), C.c_size_t, C.POINTER(C.c_size_t)],
        "pmac_pool_confirm_recovery": [C.c_uint64, C.c_uint64, C.c_uint32],
    }
    for name, arguments in pool_functions.items():
        if hasattr(lib, name):
            getattr(lib, name).argtypes = arguments
    return lib

def view(data):
    if data is None:
        return Bytes(None, 0), None
    data = data.encode("utf-8") if isinstance(data, str) else bytes(data)
    buffer = C.create_string_buffer(data, len(data)) if data else None
    return Bytes(C.cast(buffer, C.POINTER(C.c_uint8)) if buffer else None, len(data)), buffer
