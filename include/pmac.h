#ifndef PMAC_H
#define PMAC_H

#include <stddef.h>
#include <stdint.h>

#if defined(_WIN32) && defined(PMAC_STATIC)
#  define PMAC_EXPORT
#elif defined(_WIN32) && defined(PMAC_BUILD_SHARED)
#  define PMAC_EXPORT __declspec(dllexport)
#elif defined(_WIN32)
#  define PMAC_EXPORT __declspec(dllimport)
#else
#  define PMAC_EXPORT __attribute__((visibility("default")))
#endif

#ifdef __cplusplus
extern "C" {
#endif

#define PMAC_ABI_MAJOR 2u
#define PMAC_ABI_MINOR 1u
#define PMAC_ABI_VERSION 2001u
#define PMAC_CONNECT_TCP_PROBE 1u
#define PMAC_CONNECT_ACCEPT_ANY_HOST_KEY 2u
#define PMAC_REQUEST_INFINITE 1u
#define PMAC_REQUEST_READ_POOL 2u
/* GetResponse/command: passthrough is the default; CLASSIFY opts into local rules. */
#define PMAC_REQUEST_PASSTHROUGH 4u
#define PMAC_REQUEST_CLASSIFY 8u
#define PMAC_RECOVERY_CONFIRM_UNKNOWN_WRITES 1u
#define PMAC_RECOVERY_CONFIRM_REMOTE_RELEASE 2u

#define PMAC_DART_GET_USHM_CHAR 0x10u
#define PMAC_DART_SET_USHM_CHAR 0x11u
#define PMAC_DART_GET_USHM_INT 0x40u
#define PMAC_DART_SET_USHM_INT 0x41u
#define PMAC_DART_GET_USHM_FLOAT 0xf0u
#define PMAC_DART_SET_USHM_FLOAT 0xf1u
#define PMAC_DART_GET_USHM_DOUBLE 0x80u
#define PMAC_DART_SET_USHM_DOUBLE 0x81u
#define PMAC_DART_GET_P_VARIABLE 0x90u
#define PMAC_DART_SET_P_VARIABLE 0x91u
#define PMAC_DART_MAX_DATA_BYTES 524288u
#define PMAC_DART_USHM_BYTES 1048576u
#define PMAC_DART_MAX_P_INDEX 65535u

#define PMAC_OK 0
#define PMAC_INVALID_ARGUMENT -1
#define PMAC_INVALID_HANDLE -2
#define PMAC_QUEUE_FULL -3
#define PMAC_QUEUE_TIMEOUT -4
#define PMAC_EXEC_TIMEOUT -5
#define PMAC_WAIT_PENDING -6
#define PMAC_CANCELLED -7
#define PMAC_DISCONNECTED -8
#define PMAC_PROTOCOL_ERROR -9
#define PMAC_REMOTE_ERROR -10
#define PMAC_BUFFER_TOO_SMALL -11
#define PMAC_RESOURCE_LIMIT -12
#define PMAC_CLOSING -13
#define PMAC_SHUTDOWN_PENDING -14
#define PMAC_CONNECT_ERROR -15
#define PMAC_AUTHENTICATION -16
#define PMAC_HOST_KEY -17
#define PMAC_FILE_NOT_FOUND -18
#define PMAC_FILE_ERROR -19
#define PMAC_CHECKSUM_MISMATCH -20
#define PMAC_PUBLISH_UNKNOWN -21
#define PMAC_PARSE_ERROR -22
#define PMAC_NO_READY_LANE -23
#define PMAC_POOL_WAIT_TIMEOUT -24
#define PMAC_UNSUPPORTED -25
#define PMAC_INTERNAL -26
#define PMAC_RECOVERY_REQUIRED -27
#define PMAC_MAINTENANCE_ACTIVE -28
#define PMAC_SUPERSEDED -29

typedef uint64_t pmac_runtime_id;
typedef uint64_t pmac_client_id;
typedef uint64_t pmac_request_id;
typedef struct { const uint8_t *data; size_t len; } pmac_bytes;
typedef struct { uint32_t struct_size, abi_version, worker_threads, reserved; } pmac_runtime_config;
typedef struct { uint32_t struct_size, abi_version, max_requests, max_file_tasks; size_t max_retained_bytes; } pmac_client_config;
typedef struct {
    uint32_t struct_size, abi_version;
    pmac_bytes host, username, password, private_key, public_key, passphrase, known_hosts, pinned_sha256;
    uint16_t port; uint8_t control_pool_size, reserved0; uint32_t flags;
    uint32_t tcp_probe_timeout_ms, keepalive_interval_ms, keepalive_max;
    uint64_t reserved[4];
} pmac_connect_options;
typedef struct { uint32_t struct_size, abi_version, timeout_ms, flags; size_t max_response_bytes; } pmac_request_options;
typedef struct {
    uint32_t struct_size, abi_version; uint64_t request_id, client_id;
    uint32_t state; int32_t status; uint32_t certainty, reserved;
    uint64_t connection_epoch, elapsed_us; size_t response_len, stderr_len;
    int32_t exit_code; uint32_t has_exit_code;
} pmac_request_info;
typedef struct { uint32_t struct_size, abi_version; int32_t status; uint32_t certainty; uint64_t request_id, connection_epoch; } pmac_error_info;
typedef struct { uint32_t struct_size, abi_version, state, closing; uint64_t connection_epoch, last_valid_response_unix_ms; size_t active_requests; } pmac_state_snapshot;
typedef struct { uint32_t struct_size, abi_major, abi_minor, version_number, feature_flags, pointer_width; } pmac_runtime_info;

PMAC_EXPORT int32_t pmac_runtime_create(const pmac_runtime_config *config, pmac_runtime_id *out_id);
PMAC_EXPORT int32_t pmac_runtime_wait_completion(pmac_runtime_id id, uint64_t *cursor, uint32_t wait_ms);
PMAC_EXPORT int32_t pmac_runtime_wake(pmac_runtime_id id);
PMAC_EXPORT int32_t pmac_runtime_shutdown(pmac_runtime_id id, uint32_t wait_ms);
PMAC_EXPORT int32_t pmac_runtime_release(pmac_runtime_id id);
PMAC_EXPORT int32_t pmac_runtime_cleanup(pmac_runtime_id id);
PMAC_EXPORT int32_t pmac_runtime_cleanup_wait(void);
PMAC_EXPORT int32_t pmac_get_runtime_info(pmac_runtime_info *out_info);
PMAC_EXPORT const char *pmac_version_string(void);
PMAC_EXPORT uint32_t pmac_version_number(void);
PMAC_EXPORT uint32_t pmac_abi_version(void);

PMAC_EXPORT int32_t pmac_client_create(pmac_runtime_id runtime, const pmac_client_config *config, pmac_client_id *out_id);
PMAC_EXPORT int32_t pmac_client_close(pmac_client_id id, uint32_t close_mode, uint32_t wait_ms);
PMAC_EXPORT int32_t pmac_client_release(pmac_client_id id);
PMAC_EXPORT int32_t pmac_client_recovery_ids(pmac_client_id *out, size_t capacity, size_t *required);
PMAC_EXPORT int32_t pmac_client_get_state(pmac_client_id id, pmac_state_snapshot *out_state);

PMAC_EXPORT int32_t pmac_connect_submit(pmac_client_id id, const pmac_connect_options *options, pmac_request_id *out_id);
PMAC_EXPORT int32_t pmac_command_submit(pmac_client_id id, const uint8_t *command, size_t command_len, const pmac_request_options *options, pmac_request_id *out_id);
/* Asynchronous GetResponse; use pmac_request_wait/copy_response or copy_match. */
PMAC_EXPORT int32_t pmac_get_response_async(pmac_client_id id, const uint8_t *command, size_t command_len, const pmac_request_options *options, pmac_request_id *out_id);
/* DART payload values use host-native endian; the transport encodes big-endian wire data. */
PMAC_EXPORT int32_t pmac_dart_connect_async(pmac_client_id id, const pmac_request_options *options, pmac_request_id *out_id);
PMAC_EXPORT int32_t pmac_dart_disconnect_async(pmac_client_id id, const pmac_request_options *options, pmac_request_id *out_id);
PMAC_EXPORT int32_t pmac_dart_get_async(pmac_client_id id, uint32_t command, uint32_t address, size_t data_len, const pmac_request_options *options, pmac_request_id *out_id);
PMAC_EXPORT int32_t pmac_dart_set_async(pmac_client_id id, uint32_t command, uint32_t address, const uint8_t *data, size_t data_len, const pmac_request_options *options, pmac_request_id *out_id);
PMAC_EXPORT int32_t pmac_shell_submit(pmac_client_id id, const uint8_t *command, size_t command_len, uint32_t persistent, const pmac_request_options *options, pmac_request_id *out_id);
PMAC_EXPORT int32_t pmac_upload_submit(pmac_client_id id, const uint8_t *local, size_t local_len, const uint8_t *remote, size_t remote_len, const pmac_request_options *options, pmac_request_id *out_id);
PMAC_EXPORT int32_t pmac_download_submit(pmac_client_id id, const uint8_t *remote, size_t remote_len, const uint8_t *local, size_t local_len, const pmac_request_options *options, pmac_request_id *out_id);
PMAC_EXPORT int32_t pmac_buffer_upload_submit(pmac_client_id id, const uint8_t *local, size_t local_len, const uint8_t *remote, size_t remote_len, uint32_t cleanup_remote, const pmac_request_options *options, pmac_request_id *out_id);
PMAC_EXPORT int32_t pmac_file_exists_submit(pmac_client_id id, const uint8_t *remote, size_t remote_len, const pmac_request_options *options, pmac_request_id *out_id);
PMAC_EXPORT int32_t pmac_file_remove_submit(pmac_client_id id, const uint8_t *remote, size_t remote_len, const pmac_request_options *options, pmac_request_id *out_id);

PMAC_EXPORT int32_t pmac_request_wait(pmac_request_id id, uint32_t wait_ms, pmac_request_info *out_info);
PMAC_EXPORT int32_t pmac_request_cancel(pmac_request_id id);
PMAC_EXPORT int32_t pmac_request_copy_response(pmac_request_id id, uint8_t *out, size_t capacity, size_t *required);
PMAC_EXPORT int32_t pmac_request_copy_stderr(pmac_request_id id, uint8_t *out, size_t capacity, size_t *required);
/* Copy the immediately preceding error on this thread; request_id is diagnostic and already released. */
PMAC_EXPORT int32_t pmac_copy_last_error(pmac_error_info *out_info, char *out, size_t capacity, size_t *required);
PMAC_EXPORT int32_t pmac_request_copy_error(pmac_request_id id, pmac_error_info *out_info, char *out, size_t capacity, size_t *required);
PMAC_EXPORT int32_t pmac_request_copy_match(pmac_request_id id, const uint8_t *pattern, size_t pattern_len, uint32_t group, uint8_t *out, size_t capacity, size_t *required);
PMAC_EXPORT int32_t pmac_request_release(pmac_request_id id);

typedef struct {
    uint32_t struct_size, abi_version, target_size, max_members, min_ready, connect_timeout_ms;
} pmac_pool_profile;
typedef struct {
    uint32_t struct_size, abi_version; uint64_t revision;
    uint32_t desired, ready, busy, creating, failed, retiring, release_uncertain, max_members;
    uint32_t unknown_write, maintenance_active, maintenance_failed; int32_t last_create_error;
} pmac_pool_state;
typedef struct {
    uint32_t struct_size, abi_version; uint64_t member_id, epoch;
    uint32_t state, control; uint64_t last_valid_response_unix_ms;
    int32_t last_error; uint32_t reserved;
} pmac_pool_member;

PMAC_EXPORT int32_t pmac_pool_prepare_async(pmac_client_id id, const pmac_connect_options *connect, const pmac_pool_profile *profile, pmac_request_id *out_id);
PMAC_EXPORT int32_t pmac_pool_resize_async(pmac_client_id id, const pmac_pool_profile *profile, pmac_request_id *out_id);
PMAC_EXPORT int32_t pmac_pool_repair_async(pmac_client_id id, uint64_t member_id, const pmac_request_options *options, pmac_request_id *out_id);
PMAC_EXPORT int32_t pmac_pool_drain_async(pmac_client_id id, uint64_t member_id, const pmac_request_options *options, pmac_request_id *out_id);
PMAC_EXPORT int32_t pmac_pool_repair_maintenance_async(pmac_client_id id, const pmac_request_options *options, pmac_request_id *out_id);
PMAC_EXPORT int32_t pmac_pool_get_state(pmac_client_id id, pmac_pool_state *out_state);
/* capacity and required are member counts, not byte lengths. No partial copy. */
PMAC_EXPORT int32_t pmac_pool_get_members(pmac_client_id id, pmac_pool_member *out_members, size_t capacity, size_t *required);
/* Explicit application acknowledgement, never inferred from a successful reconnect. */
PMAC_EXPORT int32_t pmac_pool_confirm_recovery(pmac_client_id id, uint64_t expected_revision, uint32_t flags);
PMAC_EXPORT int32_t pmac_request_get_route(pmac_request_id id, uint64_t *member_id, uint64_t *epoch);

PMAC_EXPORT int32_t pmac_abort(pmac_client_id id, const pmac_request_options *options);
PMAC_EXPORT int32_t pmac_coord_enable(pmac_client_id id, int32_t coord, const pmac_request_options *options);
PMAC_EXPORT int32_t pmac_coord_exec_prog(pmac_client_id id, int32_t coord, int32_t program, const pmac_request_options *options);
PMAC_EXPORT int32_t pmac_coord_hold(pmac_client_id id, int32_t coord, const pmac_request_options *options);
PMAC_EXPORT int32_t pmac_coord_run(pmac_client_id id, int32_t coord, const pmac_request_options *options);
PMAC_EXPORT int32_t pmac_plc_enable(pmac_client_id id, int32_t plc, const pmac_request_options *options);
PMAC_EXPORT int32_t pmac_plc_disable(pmac_client_id id, int32_t plc, const pmac_request_options *options);
PMAC_EXPORT int32_t pmac_motor_kill(pmac_client_id id, int32_t motor, const pmac_request_options *options);
PMAC_EXPORT int32_t pmac_motor_enable(pmac_client_id id, int32_t motor, const pmac_request_options *options);
PMAC_EXPORT int32_t pmac_motor_jog_stop(pmac_client_id id, int32_t motor, const pmac_request_options *options);
PMAC_EXPORT int32_t pmac_motor_abs_jog(pmac_client_id id, int32_t motor, double position, const pmac_request_options *options);
PMAC_EXPORT int32_t pmac_motor_inc_jog(pmac_client_id id, int32_t motor, double distance, const pmac_request_options *options);
PMAC_EXPORT int32_t pmac_coord_status(pmac_client_id id, int32_t coord, const pmac_request_options *options, uint64_t *out);
PMAC_EXPORT int32_t pmac_motor_status(pmac_client_id id, int32_t motor, const pmac_request_options *options, uint64_t *out);
PMAC_EXPORT int32_t pmac_motor_position(pmac_client_id id, int32_t motor, const pmac_request_options *options, double *out);
PMAC_EXPORT int32_t pmac_motor_desired_position(pmac_client_id id, int32_t motor, const pmac_request_options *options, double *out);
PMAC_EXPORT int32_t pmac_motor_velocity(pmac_client_id id, int32_t motor, const pmac_request_options *options, double *out);
PMAC_EXPORT int32_t pmac_motor_position_error(pmac_client_id id, int32_t motor, const pmac_request_options *options, double *out);
PMAC_EXPORT int32_t pmac_servo_error_count(pmac_client_id id, const pmac_request_options *options, uint32_t *out);
PMAC_EXPORT int32_t pmac_phase_error_count(pmac_client_id id, const pmac_request_options *options, uint32_t *out);
PMAC_EXPORT int32_t pmac_servo_count(pmac_client_id id, const pmac_request_options *options, uint32_t *out);
PMAC_EXPORT int32_t pmac_phase_time(pmac_client_id id, const pmac_request_options *options, double *out);
PMAC_EXPORT int32_t pmac_servo_time(pmac_client_id id, const pmac_request_options *options, double *out);
PMAC_EXPORT int32_t pmac_abort_all_flag(pmac_client_id id, const pmac_request_options *options, uint32_t *out);
PMAC_EXPORT int32_t pmac_plc_running(pmac_client_id id, int32_t plc, const pmac_request_options *options, uint32_t *out);
PMAC_EXPORT int32_t pmac_coord_prog_running(pmac_client_id id, int32_t coord, const pmac_request_options *options, uint32_t *out);
PMAC_EXPORT int32_t pmac_motor_closed_loop(pmac_client_id id, int32_t motor, const pmac_request_options *options, uint32_t *out);
PMAC_EXPORT int32_t pmac_coord_motor_enabled(pmac_client_id id, int32_t coord, const pmac_request_options *options, uint32_t *out);


/* Continuous geterrors status-transition monitor over an authenticated SSH PTY.
 * period_seconds is finite, in [0.01, 1.0]. Masks are sent only when their flag is set.
 * Text is borrowed UTF-8 without a terminating NUL; callbacks are serialized.
 * Return nonzero or use request_stop from the callback. stop/release are barriers
 * and must run outside the callback. Stop sends Ctrl+C only to this monitor's PTY.
 */
#define PMAC_GETERRORS_DATE 1u
#define PMAC_GETERRORS_COORD_MASK 2u
#define PMAC_GETERRORS_MOTOR_MASK 4u
#define PMAC_GETERRORS_RING_MASK 8u
#define PMAC_GETERRORS_GLOBAL_MASK 16u
#define PMAC_GETERRORS_SETTINGS 1u
#define PMAC_GETERRORS_ERROR 2u
#define PMAC_GETERRORS_STDERR 3u
#define PMAC_GETERRORS_TERMINAL 4u
typedef uint64_t pmac_geterrors_id;
typedef struct {
    uint32_t struct_size, abi_version;
    double period_seconds;
    uint64_t coord_mask, motor_mask, ring_mask, global_mask;
    uint32_t flags, timeout_ms, queue_lines, max_line_bytes;
} pmac_geterrors_config;
typedef int32_t (*pmac_geterrors_callback)(void *user, int32_t status,
    uint32_t kind, const uint8_t *text, size_t length);
PMAC_EXPORT int32_t pmac_geterrors_start(pmac_client_id client, const pmac_geterrors_config *config,
    pmac_geterrors_callback callback, void *user, pmac_geterrors_id *out);
PMAC_EXPORT int32_t pmac_geterrors_request_stop(pmac_geterrors_id id);
/* stop waits for the PTY process and callbacks; returns any terminal/cleanup error. */
PMAC_EXPORT int32_t pmac_geterrors_stop(pmac_geterrors_id id);
/* release destroys a valid handle even when returning a recorded terminal error.
 * Called from its own callback, it returns INVALID_ARGUMENT without destroying it. */
PMAC_EXPORT int32_t pmac_geterrors_release(pmac_geterrors_id id);

/* Continuous Gather. All values are row-major doubles in Gather.Addr order.
 * callback: status == PMAC_OK for data; otherwise a terminal error event.
 * Pointers are borrowed until callback returns. Return nonzero to request stop.
 * Callbacks run serially on a dedicated thread and may start before start returns.
 * Do not call stop/release from the callback; use request_stop or return nonzero.
 * One session per controller. gather and nc processes are reserved for Gather.
 * Reverse TCP requires controller source IP and a per-session random handshake.
 * TCP data remains unencrypted; use a trusted network and restrict incoming peers.
 */
typedef uint64_t pmac_gather_id;
typedef int32_t (*pmac_gather_callback)(void *user, int32_t status,
    const double *data, size_t rows, size_t items, uint64_t first_sample,
    const uint8_t *message, size_t message_len);
typedef struct {
    uint32_t struct_size, abi_version;
    uint32_t period, items;
    const pmac_bytes *addresses;
    pmac_bytes local_ip; /* empty: discover SSH_CONNECTION client IP; numeric local IP override */
    uint16_t port, reserved; /* port 0: auto; reserved must be zero */
    uint32_t max_samples; /* 0: 2048 */
    uint32_t batch_rows; /* 0: 128, maximum 4096; flush approximately every 20 ms */
    uint32_t queue_batches; /* 0: 32; queue overflow stops acquisition */
    uint32_t timeout_ms; /* 0: 5000 per startup/cleanup request */
} pmac_gather_config;
PMAC_EXPORT int32_t pmac_gather_start(pmac_client_id client, const pmac_gather_config *config,
    pmac_gather_callback callback, void *user, pmac_gather_id *out);
PMAC_EXPORT int32_t pmac_gather_request_stop(pmac_gather_id id);
/* stop waits for threads/callbacks; returns the terminal acquisition/cleanup error, if any. */
PMAC_EXPORT int32_t pmac_gather_stop(pmac_gather_id id);
/* release stops and destroys the handle even when it reports a terminal error. */
PMAC_EXPORT int32_t pmac_gather_release(pmac_gather_id id);
#ifdef __cplusplus
}
#endif
#endif
