# geterrors Real-time Status-change Monitoring

[中文](geterrors.md) | [English](geterrors.en.md)

`geterrors` continuously monitors Power PMAC status changes. Its text can include errors, warnings, and ordinary status transitions; receiving a message alone does not establish a severe device fault. The library opens a dedicated PTY through the connected Client's SSH session, runs the controller-provided `geterrors`, and delivers output lines through callbacks. No reverse TCP listener, `nc`, or separate transport port is needed. Host identity verification and authentication use the existing SSH connection.

C ABI and C++17 wrappers are currently provided. The controller must have an executable `geterrors` installed. This API preserves device text without parsing or guessing the application meaning of individual status bits; applications should determine alerts using the relevant controller manual and site configuration.

## Configuration and output

| C++ field | C field/flag | Default and range |
| --- | --- | --- |
| `periodSeconds` | `period_seconds` | C++ default `0.01`; finite, 0.01..=1.0 seconds; C must explicitly set it after zero-initialization, as 0 is invalid; corresponds to `-t` |
| `date` | `PMAC_GETERRORS_DATE` | Default false; passes `-T` when enabled; date format is controller-defined |
| `coordMask` | `coord_mask` / `PMAC_GETERRORS_COORD_MASK` | Omitted by default; when specified, passes `-c` and a hexadecimal mask |
| `motorMask` | `motor_mask` / `PMAC_GETERRORS_MOTOR_MASK` | Omitted by default; when specified, passes `-m` and a hexadecimal mask |
| `ringMask` | `ring_mask` / `PMAC_GETERRORS_RING_MASK` | Omitted by default; when specified, passes `-r` and a hexadecimal mask |
| `globalMask` | `global_mask` / `PMAC_GETERRORS_GLOBAL_MASK` | Omitted by default; when specified, passes `-g` and a hexadecimal mask |
| `timeoutMs` | `timeout_ms` | Default 5000 ms; 0 selects the default; budget for opening the dedicated PTY and starting the command, not total monitoring duration |
| `queueLines` | `queue_lines` | Default 256; 0 selects the default; at most 4096 queued messages |
| `maxLineBytes` | `max_line_bytes` | Default 65536; 0 selects the default; at most 1048576 bytes per line |

All four masks are unsigned 64-bit values. C++ uses `std::optional<uint64_t>`: an unset value omits the argument and preserves the device tool's own default; an explicitly assigned 0 still passes the argument. C uses flags to distinguish these cases: a zero mask field does not mean omission. The library neither hardcodes default bitmaps nor infers the meaning of each mask bit.

For example, site-provided `-g0x7ff`, `-c0x180000000807ffc`, `-m0x8c007ffc`, and `-r0x803e` can be entered explicitly as in the example. These are inputs for this example, not universal configurations recommended for all controllers. Startup configuration echoes for `-g0x...`, `-c0x...`, `-m0x...`, `-r0x...`, and `-t...` are delivered as SETTINGS messages and should not be treated as device faults.

| kind | status | Application handling |
| --- | --- | --- |
| `PMAC_GETERRORS_SETTINGS` (1) | `PMAC_OK` | Configuration echo; record the actual startup settings |
| `PMAC_GETERRORS_ERROR` (2) | `PMAC_OK` | Original device status-change line, including ordinary changes; interpret using the content and device manual |
| `PMAC_GETERRORS_STDERR` (3) | `PMAC_OK` | Remote program stderr line; record program diagnostics separately rather than equating them directly with motor/coordinate-system status changes |
| `PMAC_GETERRORS_TERMINAL` (4) | Nonzero | Network, process, decoding/resource, or cleanup terminal error; end this monitor and let the main thread finish cleanup |

Callbacks deliver complete lines without line terminators. Empty lines and protocol acknowledgment lines containing only ACK (0x06) are not delivered; these acknowledgment lines do not end startup-settings recognition. The library preserves in-line text and whitespace rather than rewriting status text into application error codes. Normal explicit stopping does not mean device status has recovered; callbacks may remain absent for a long time when there are no new status changes.

## Quick start with default masks

Connect Client first, then start independent monitoring. The following runs on an already connected `client` and preserves device default masks:

```cpp
pmac::GetErrorsConfig config;
config.periodSeconds = 0.01; // seconds, not milliseconds
// Leave coordMask/motorMask/ringMask/globalMask unset for system defaults.
auto monitor = client.startGetErrors(config,
    [](uint32_t kind, const std::string& text) {
        if (kind == PMAC_GETERRORS_SETTINGS) {
            // Actual masks and period reported by the remote tool.
            std::cout << "[settings] " << text << '\n';
        } else {
            // Status transition or diagnostic text; decide severity in the application.
            std::cout << "[status " << kind << "] " << text << '\n';
        }
    },
    [](const pmac::Error& error) {
        std::cerr << "Monitoring ended: " << error.what() << '\n';
    });
// Continue application work. The monitor keeps listening until stopped or disconnected.
// When finished, outside any monitor callback:
monitor.stop();
```

In a real program, call `stop()` when its run loop ends or a stop instruction arrives; do not stop immediately after startup. Omit the third argument if no terminal-error callback is needed. Monitoring occupies a separate SSH PTY instead of placing long-running `geterrors` in an ordinary `shell()` request. The main thread can continue issuing commands and status queries.

### Custom masks, zero masks, and restoring defaults

```cpp
pmac::GetErrorsConfig config;
config.periodSeconds = 0.05;
config.date = true;                 // -T
config.motorMask = UINT64_C(0x8c007ffc); // -m0x8c007ffc
config.globalMask = UINT64_C(0);     // explicitly sends -g0x0
// Coord and ring masks remain unspecified, so their defaults are retained.
// To omit a previously assigned parameter when creating the next monitor:
config.motorMask.reset();
```

The equivalent C settings follow. Zero-initialize first, then fill in the required structure header and period. Flag only the masks that should actually be sent:

```c
pmac_geterrors_config config = {0};
config.struct_size = sizeof(config);
config.abi_version = PMAC_ABI_VERSION;
config.period_seconds = 0.05;
config.flags = PMAC_GETERRORS_DATE | PMAC_GETERRORS_MOTOR_MASK | PMAC_GETERRORS_GLOBAL_MASK;
config.motor_mask = UINT64_C(0x8c007ffc);
config.global_mask = 0; /* flag present: explicitly sends -g0x0 */
/* To omit the motor mask for the next start, clear only its flag. */
config.flags &= ~PMAC_GETERRORS_MOTOR_MASK;
```

Configuration is copied into a session snapshot at `start`. Modifying the configuration object afterward does not change the running process. To change period, date, or masks, stop and release the current Monitor outside callbacks before starting one with the new configuration. C++ can release by ending the object's scope; C must explicitly call `pmac_geterrors_release`. Each Client may retain at most one geterrors handle, with a process-wide limit of 64. Stopped but unreleased handles still occupy a slot.

## Startup echoes, alert text, and site verification

A typical startup first returns empty lines or ACK, then reports the masks and period actually used by the tool. The following is SETTINGS output, not an alert; concrete defaults are controller-defined:

```text
-g0x7ff
-c0x180000000807ffc
-m0x8c007ffc
-r0x803e
-t0.010000
```

For example, monitoring may subsequently receive the following status lines, delivered as separate ERROR-kind callbacks:

```text
Coord[1].FeFatal=1
Motor[1].FeFatal=1
```

Both lines have been received through callbacks in live monitoring. The tool reports current matching states at startup and continues producing output when it detects transitions. Do not interpret every initial current-state report as a newly occurring alert. Callback `status=PMAC_OK` means successful transport and decoding, not a fault-free device. Decide alerts using `kind`, text, the controller manual, and application rules. Silence when the device has no new changes is normal.

geterrors covers active motor status, coordinate-system status, global status, execution status of active coordinate-system programs and PLCs, and active Macro ring status. Output may include status strings corresponding to `Backup Motor[n].Status`, `Backup Coord[n].Status`, `Backup Sys.Status`, and `Backup Macro.Status[n]`. Mask bits depend on the firmware's status-message ordering. This library only formats supplied values as `0x` hexadecimal arguments, never decimal or `$` format, and does not send additional Backup commands to configure masks.

For site acceptance, proceed in this order: connect and verify SSH host identity → start with default masks → record SETTINGS → have site personnel trigger a known status change under existing safety procedures → check callback text → call stop/release → verify that no geterrors process newly created by this session remains. The library sends Ctrl+C only to its dedicated PTY. Do not terminate other existing geterrors processes on the device to stop this monitor. Live tests verified 0.01-second default masks, 0.05-second date/explicit-zero masks, continuous message callbacks, and normal Ctrl+C cleanup; this does not establish validation across every firmware version and fault scenario.

Use the repository's [C++17 example](../examples/cpp/geterrors.cpp) and [C11 example](../examples/c/geterrors.c) directly. Complete code and explanations are retained below.

## Complete C++17 example

Save the following as `geterrors.cpp`. Set `PMAC_HOST` and `PMAC_KNOWN_HOSTS` first, then provide `PMAC_PASSWORD` or modify connection options for a private key according to the authentication method. Verify known_hosts through a trusted channel beforehand; see [Authentication](authentication.en.md).

```cpp
#include "pmac.hpp"
#include <cstdlib>
#include <iostream>
#include <stdexcept>

static std::string requiredEnv(const char* name) {
    const char* value = std::getenv(name);
    if (!value || !*value) throw std::runtime_error(std::string("Set ") + name);
    return value;
}
int main() {
    try {
        pmac::Runtime runtime;
        auto client = runtime.createClient();
        pmac::ConnectOptions connection;
        connection.host = requiredEnv("PMAC_HOST");
        connection.username = "root";
        connection.knownHosts = requiredEnv("PMAC_KNOWN_HOSTS");
        if (const char* password = std::getenv("PMAC_PASSWORD")) connection.password = password;
        client.connectSync(connection);
        pmac::GetErrorsConfig config;
        config.periodSeconds = 0.01;
        config.date = true;
        config.globalMask = UINT64_C(0x7ff);
        config.coordMask = UINT64_C(0x180000000807ffc);
        config.motorMask = UINT64_C(0x8c007ffc);
        config.ringMask = UINT64_C(0x803e);
        auto monitor = client.startGetErrors(config,
            [](uint32_t kind, const std::string& text) {
                // SETTINGS: startup echo; ERROR: device status transitions;
                // STDERR: remote program diagnostics. This callback owns no UI thread.
                std::cout << '[' << kind << "] " << text << '\n';
                // Copy text into an owned queue entry before another thread uses it.
            },
            [](const pmac::Error& error) {
                std::cerr << "Monitor terminated: " << error.what() << '\n';
            });
        std::cout << "Monitoring. Press Enter to stop.\n";
        std::cin.get();
        monitor.stop(); // waits for the process cleanup and callback lifetime barrier
        // monitor destructor releases its ID before client/runtime destructors run.
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
```

`OnMessage` receives `kind` and `const std::string&`. The string is valid only during the current callback; copy it for cross-thread processing. The wrapper catches user callback exceptions, requests stopping, and rethrows them when `stop()` is called outside callbacks. Exceptions do not cross the C ABI. The destructor stops and releases the handle without throwing; explicitly call `stop()` to obtain the termination reason.

## Complete C11 example

Save as `geterrors.c`, using the same environment variables. This callback only uses the library's borrowed text and immediately prints it without retaining pointers.

```c
#include "pmac.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <inttypes.h>

static pmac_bytes bytes(const char *s) {
    pmac_bytes b = {(const uint8_t *)s, s ? strlen(s) : 0};
    return b;
}
static int32_t on_message(void *user, int32_t status, uint32_t kind,
                          const uint8_t *text, size_t length) {
    FILE *output = status == PMAC_OK ? stdout : stderr;
    (void)user;
    fprintf(output, "[kind=%u status=%d] ", (unsigned)kind, (int)status);
    if (length) fwrite(text, 1, length, output); /* length-delimited, not NUL-terminated */
    fputc('\n', output);
    return 0; /* nonzero requests stop; never call stop/release from this callback */
}
int main(void) {
    pmac_runtime_id runtime = 0;
    pmac_client_id client = 0;
    pmac_request_id request = 0;
    pmac_geterrors_id monitor = 0;
    pmac_runtime_config rc = {0};
    pmac_client_config cc = {0};
    pmac_connect_options connection = {0};
    pmac_geterrors_config config = {0};
    pmac_request_info info = {0};
    const char *host = getenv("PMAC_HOST");
    const char *known_hosts = getenv("PMAC_KNOWN_HOSTS");
    int32_t status = PMAC_OK, stopped, released;
    int failed = 0;
    if (!host || !*host || !known_hosts || !*known_hosts) {
        fputs("Set PMAC_HOST and PMAC_KNOWN_HOSTS\n", stderr);
        return 1;
    }
    rc.struct_size = sizeof(rc); rc.abi_version = PMAC_ABI_VERSION; rc.worker_threads = 2;
    status = pmac_runtime_create(&rc, &runtime);
    if (status != PMAC_OK) goto cleanup;
    cc.struct_size = sizeof(cc); cc.abi_version = PMAC_ABI_VERSION;
    cc.max_requests = 64; cc.max_file_tasks = 1; cc.max_retained_bytes = 80u * 1024u * 1024u;
    status = pmac_client_create(runtime, &cc, &client);
    if (status != PMAC_OK) goto cleanup;
    connection.struct_size = sizeof(connection); connection.abi_version = PMAC_ABI_VERSION;
    connection.host = bytes(host); connection.username = bytes("root");
    connection.password = bytes(getenv("PMAC_PASSWORD")); connection.known_hosts = bytes(known_hosts);
    connection.port = 22; connection.control_pool_size = 1;
    connection.flags = PMAC_CONNECT_TCP_PROBE; connection.tcp_probe_timeout_ms = 600;
    connection.keepalive_interval_ms = 1000; connection.keepalive_max = 3;
    status = pmac_connect_submit(client, &connection, &request);
    if (status != PMAC_OK) goto cleanup;
    do { status = pmac_request_wait(request, UINT32_MAX, &info); }
    while (status == PMAC_WAIT_PENDING);
    (void)pmac_request_release(request); request = 0;
    if (status != PMAC_OK) goto cleanup;
    config.struct_size = sizeof(config); config.abi_version = PMAC_ABI_VERSION;
    config.period_seconds = 0.01; /* required: zero does NOT select a default */
    config.flags = PMAC_GETERRORS_DATE | PMAC_GETERRORS_GLOBAL_MASK |
                   PMAC_GETERRORS_COORD_MASK | PMAC_GETERRORS_MOTOR_MASK | PMAC_GETERRORS_RING_MASK;
    config.global_mask = UINT64_C(0x7ff); config.coord_mask = UINT64_C(0x180000000807ffc);
    config.motor_mask = UINT64_C(0x8c007ffc); config.ring_mask = UINT64_C(0x803e);
    status = pmac_geterrors_start(client, &config, on_message, NULL, &monitor);
    if (status != PMAC_OK) goto cleanup;
    puts("Monitoring. Press Enter to stop.");
    (void)getchar();
cleanup:
    if (status != PMAC_OK) { fprintf(stderr, "operation failed: %d\n", (int)status); failed = 1; }
    if (monitor) {
        stopped = pmac_geterrors_stop(monitor);
        released = pmac_geterrors_release(monitor); monitor = 0;
        /* A recorded terminal error can be returned by release after ID destruction. */
        if (stopped != PMAC_OK || released != PMAC_OK) {
            fprintf(stderr, "Monitor stop=%d release=%d\n", (int)stopped, (int)released);
            failed = 1;
        }
    }
    if (client) {
        status = pmac_client_close(client, 0, 5000);
        if (status == PMAC_OK) status = pmac_client_release(client);
        if (status != PMAC_OK) {
            fprintf(stderr, "Client cleanup failed/pending: %d; client=%" PRIu64
                    " runtime=%" PRIu64 " retained\n", (int)status, client, runtime);
            return 1; /* standalone process exits; embedding applications retain and retry IDs */
        }
    }
    if (runtime) {
        status = pmac_runtime_shutdown(runtime, 5000);
        if (status == PMAC_OK) status = pmac_runtime_release(runtime);
        if (status != PMAC_OK) {
            fprintf(stderr, "Runtime cleanup failed/pending: %d; id=%" PRIu64
                    " retained\n", (int)status, runtime);
            return 1;
        }
    }
    return failed;
}
```

## Callbacks, stopping, and releasing

- Callbacks run on a separate thread and are serialized for each Monitor. They may arrive before start returns; prepare `user` or C++ captured application objects in advance.
- C `text` is a length-delimited UTF-8 byte view, not necessarily NUL-terminated, and expires when the current callback returns. Copy text, kind, and status before dispatching across threads; the application queue should also have a capacity limit.
- Inside the monitor's own callback, only call `pmac_geterrors_request_stop` / `monitor.requestStop()` or return nonzero to request stopping. Do not call blocking stop/release, destroy Monitor, close Client, or close Runtime.
- When the main thread calls `stop()`, the library sends Ctrl+C to this Monitor's dedicated PTY and gives the process up to 2 seconds to exit; failure to exit normally reports a cleanup error. The implementation does not execute `killall` or terminate other geterrors/Shell sessions.
- stop is a callback lifetime barrier: after it returns or throws a terminal error outside callbacks, this Monitor receives no further callbacks. Stopping does not guarantee delivery of all queued messages.
- C stop is idempotent and retains terminal errors for repeated queries. Calling release on a valid handle destroys it even when returning a prior terminal or cleanup error; do not retry release. A call from its own callback that returns `PMAC_INVALID_ARGUMENT` does not destroy it.
- If start fails, the output ID is 0, and the library waits for already started callbacks to stop before returning. After successful startup, the application must stop and release Monitor before closing Client/Runtime. C++ Monitor holds a RuntimeOwner to prevent early destruction of its dependencies.
- Objects captured by C++ callbacks, C user memory, and application processing queues must remain alive at least until the stop barrier completes. Stop explicitly, then destroy the handle, and finally close Client, release child IDs, and close Runtime.

The C example is standalone. When embedding in a long-running application, preserve Client/Runtime IDs if closing returns `PMAC_SHUTDOWN_PENDING`, address recovery conditions, and retry. Do not unload a DLL/so still in use by background work. See the [ABI contract](abi-contract.en.md) for lifecycle details and unload barriers. Monitoring does not restart or replay automatically; create a new Monitor after reconnecting.

## Building and running

Save the two source files above in the repository root and build the native library first. Alternatively, use repository examples directly by replacing `geterrors.c` with `examples/c/geterrors.c` and `geterrors.cpp` with `examples/cpp/geterrors.cpp` in the following commands (use backslashes on Windows). C++ requires a compiler with complete C++17 standard-library support, including `<optional>`. Native-library and application bitness and target platform must match. CMake defines Windows dynamic library `pmac.dll` and MSVC import library `pmac.dll.lib`; Linux uses `libpmac.so`.

Windows, x64 Native Tools Command Prompt for VS 2022:

```bat
cargo build -p pmac-ffi --release --locked
cl /nologo /W4 /TC /std:c11 /Iinclude geterrors.c target\release\pmac.dll.lib /Fe:geterrors_c.exe
cl /nologo /W4 /EHsc /std:c++17 /Iinclude /Ibindings\cpp geterrors.cpp target\release\pmac.dll.lib /Fe:geterrors_cpp.exe
set "PATH=%CD%\target\release;%PATH%"
set "PMAC_HOST=192.168.0.200"
set "PMAC_KNOWN_HOSTS=C:\pmac\known_hosts"
rem Provide PMAC_PASSWORD in this environment as required; do not commit credentials.
geterrors_cpp.exe
rem Or run geterrors_c.exe
```

Linux, GCC/G++:

```sh
cargo build -p pmac-ffi --release --locked
cc -std=c11 -Wall -Wextra -Iinclude geterrors.c -Ltarget/release -lpmac \
  -Wl,-rpath,'$ORIGIN/target/release' -o geterrors_c
c++ -std=c++17 -Wall -Wextra -Iinclude -Ibindings/cpp geterrors.cpp \
  -Ltarget/release -lpmac -Wl,-rpath,'$ORIGIN/target/release' -o geterrors_cpp
export PMAC_HOST=192.168.0.200
export PMAC_KNOWN_HOSTS="$HOME/.ssh/known_hosts"
# Provide PMAC_PASSWORD in this environment as required by authentication.
./geterrors_cpp
# Or run ./geterrors_c
```

When explicitly using Cargo `--target <triple>`, change linking paths to `target/<triple>/release` and adjust PATH/rpath. See [Build and installation](build-and-install.en.md) for release installation paths. Other languages can currently integrate through this C ABI; Python/.NET convenience wrappers are not yet available.

## Capacity and troubleshooting

The queue's default capacity budget is `queueLines × maxLineBytes`, capped at 64 MiB. The default is 256 × 65536 = 16 MiB. This is an upper bound calculated using maximum line length, not an allocation of 64 KiB for every short message. Current-line, SSH, and application-copy buffers require additional memory. Oversized lines or persistently slow callbacks terminate monitoring with a resource error; the library never silently truncates status text. Do not perform slow disk writes, UI waits, or unbounded queue growth on the callback thread.

| Symptom | Action |
| --- | --- |
| Configuration rejected / `PMAC_INVALID_ARGUMENT` | Explicitly set C period_seconds; exclude 0, NaN, infinity, and values outside 0.01..=1.0; verify structure size/version, flags, and queue limits |
| Startup failure or remote process exits | Ensure geterrors is installed and the SSH account has execution permission; inspect STDERR and terminal messages; verify that the device tool supports the configured options |
| SETTINGS only, no ERROR | SETTINGS echoes startup arguments; no new messages when there are no transitions is normal; check masks against expected changes rather than concluding that the network disconnected |
| ERROR contains ordinary changes | The tool monitors transitions; the kind name does not indicate severity; classify according to device documentation and application rules |
| STDERR received | Record remote program diagnostics separately from device status-change text; use subsequent terminal status to determine whether the program ended |
| Queue overflow or oversized line | Move slow work out of callbacks, check message frequency and line format, and adjust queueLines/maxLineBytes within limits; their product must not exceed 64 MiB |
| SSH disconnected / Client closed | Finish cleanup and release the old Monitor; restart after restoring an authenticated connection; there is no separate TCP data connection to allow through a firewall |
| stop returns cleanup timeout | This dedicated PTY process did not exit normally within the stop budget; record terminal status and verify controller session state; do not use global killall and disrupt other monitors |
| release returns nonzero | A valid handle may already have been destroyed while returning a terminal error; entry errors such as rejection inside its own callback or an invalid ID do not mean normal release completed; follow the handle rules above |

geterrors event arrival cannot replace a dedicated hardware safety circuit. Host time, the date option, and callback order cannot prove that mechanical motion has stopped. Monitoring uses an SSH-encrypted channel; keep its authentication and security settings consistent with the command interface.
