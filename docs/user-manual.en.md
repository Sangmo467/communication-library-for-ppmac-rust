# Power PPMAC Communication Library User Manual

[简体中文](user-manual.md)

GetResponse supports per-request passthrough: skip the rule table and online/program classification, and submit a single command unchanged. See **[GetResponse Passthrough Mode and Language Examples](getresponse-passthrough.en.md)**.


Applies to library version 2.0.6 and C ABI 2.1. This document covers everyday use of the public SDKs. For complete function and structure definitions, refer to `include/pmac.h`, the language binding sources, and the [C ABI Contract](abi-contract.en.md).

For initial integration, start with [GetResponse Multilanguage Examples](getresponse-examples.en.md), covering build, connection, and complete call sequences for C/C++, [C#](dotnet-getresponse.en.md), [Python](python-getresponse.en.md), and [MATLAB MEX](matlab-getresponse.en.md). Gather and geterrors are follow-up topics.

## 1. Object Model and Basic Sequence

All languages share the same native object model:

- **Runtime**: manages Tokio workers, completion notifications, and owned objects. A process can create multiple Runtimes, but usually one can be reused.
- **Client**: a logical client for one controller, holding the connection pool, request capacity, and maintenance connection.
- **Request**: the ID, terminal state, and result of an admitted operation. Reading results does not resend commands.

Basic sequence:

1. Create a Runtime;
2. Create a Client;
3. Configure authentication and host identity verification, then connect;
4. Submit commands or application operations;
5. Wait for requests and read responses, stderr, exit codes, or errors;
6. Release Requests;
7. Close and release the Client;
8. Shut down and release the Runtime.

C++/Python RAII or `with`, and .NET `using`/`await using`, assist cleanup. Garbage collection is not a deterministic network shutdown procedure.

## 2. Connection Configuration

### 2.1 Common parameters

| Parameter | Default | Description |
|---|---:|---|
| host, username | None | Required; host is the controller address, username is usually the controller SSH user |
| port | 22 | SSH port |
| password | Empty | Password authentication |
| private_key / privateKey / PrivateKeyPath | Empty | Private key file path; takes priority when set, without automatic password fallback |
| passphrase | Empty | Passphrase for an encrypted private key |
| known_hosts / knownHosts / KnownHostsPath | Empty | OpenSSH known_hosts file |
| pinned_sha256 / pinnedSha256 / PinnedSha256 | Empty | 32-byte SHA-256 fingerprint of the host public key |
| accept_any_host_key / acceptAnyHostKey / AcceptAnyHostKey | false | Explicitly skip host identity verification; not recommended for production |
| tcp_probe / tcpProbe / TcpProbe | true | Adds a TCP timeout to connection establishment; does not establish a second connection |
| keepalive_interval_ms and related settings | 1000 / 3 | SSH keepalive interval and maximum failure count |
| control_pool_size / controlPoolSize / ControlPoolSize | 1 | Initial number of control connection members |

Host identity verification and login credentials are independent settings. Choose known_hosts, a pinned SHA-256 fingerprint, or explicitly accept the risk of `accept_any_host_key`. See [Authentication](authentication.en.md).

### 2.2 C++17

```cpp
#include "pmac.hpp"
#include <cstdlib>
#include <string>

pmac::Runtime runtime;
pmac::Client client = runtime.createClient();

pmac::ConnectOptions options;
options.host = std::getenv("PMAC_HOST");
options.username = "root";
options.knownHosts = std::getenv("PMAC_KNOWN_HOSTS");
if (const char* value = std::getenv("PMAC_PASSWORD")) options.password = value;
client.connectSync(options);

auto bytes = client.getResponse("Sys.ServoTime");
std::string text(bytes.begin(), bytes.end());
```

C++17 `getResponse` returns `std::vector<uint8_t>`, not a string with an automatically appended NUL. `Request` is movable but not copyable.

### 2.3 C++98

The C++98 adapter provides synchronous `Runtime`, `Client`, and `Options`:

```cpp
#include "pmac_cpp98.hpp"

pmac::Runtime runtime;
pmac::Client client(runtime);
pmac::ConnectOptions options;
options.host = "192.0.2.10";
options.username = "root";
options.password = "from-secure-configuration";
options.knownHosts = "known_hosts";
client.connect(options);
std::vector<uint8_t> response = client.getResponse("Sys.ServoTime");
```

This adapter does not depend on C++11. Use the C ABI or C++17 binding for asynchronous operations and detailed terminal-state control.

### 2.4 .NET 8

```csharp
using Pmac;

using var runtime = new Pmac.Runtime();
using var client = runtime.CreateClient();
await client.ConnectAsync(new ConnectOptions
{
    Host = Environment.GetEnvironmentVariable("PMAC_HOST")!,
    Username = "root",
    Password = Environment.GetEnvironmentVariable("PMAC_PASSWORD"),
    KnownHostsPath = Environment.GetEnvironmentVariable("PMAC_KNOWN_HOSTS"),
});

byte[] response = await Task.Run(() => client.GetResponse("Sys.ServoTime"));
```

Use Request directly for truly asynchronous waiting:

```csharp
using var request = client.GetResponseAsync("Sys.ServoTime");
RequestInfo info = await request.WaitAsync(cancellationToken);
byte[] response = request.Response();
```

Cancelling the CancellationToken of `WaitAsync` requests cancellation of the native operation, rather than merely stopping the managed wait.

### 2.5 Python

```python
import os
import pmac

with pmac.Runtime(library_path=os.environ.get("PMAC_LIBRARY")) as runtime:
    with runtime.create_client() as client:
        client.connect(
            os.environ["PMAC_HOST"],
            os.environ.get("PMAC_USER", "root"),
            password=os.environ.get("PMAC_PASSWORD"),
            known_hosts=os.environ.get("PMAC_KNOWN_HOSTS"),
        )
        response = client.get_response("Sys.ServoTime")
        print(response.decode("ascii").strip())
```

Here the application reads `PMAC_LIBRARY` and passes it to `library_path`; it is not an implicitly defined binding configuration setting. Asynchronous interfaces return Request:

```python
request = client.get_response_async("P1", timeout_ms=5000)
try:
    await request.wait_async()
    value = request.response()
finally:
    request.release()
```

Request also supports `with request:`. Do not inherit and use the parent Runtime in a child after `fork`; create a new Runtime in the child.

### 2.6 Core C ABI sequence

C callers must set `struct_size` and `abi_version` in every input structure and check status after every call:

```c
pmac_runtime_config runtime_options = {0};
runtime_options.struct_size = sizeof(runtime_options);
runtime_options.abi_version = PMAC_ABI_VERSION;
runtime_options.worker_threads = 2;
pmac_runtime_id runtime = 0;
int32_t status = pmac_runtime_create(&runtime_options, &runtime);

pmac_client_config client_options = {0};
client_options.struct_size = sizeof(client_options);
client_options.abi_version = PMAC_ABI_VERSION;
client_options.max_requests = 64;
client_options.max_file_tasks = 1;
client_options.max_retained_bytes = 80u * 1024u * 1024u;
pmac_client_id client = 0;
if (status == PMAC_OK)
    status = pmac_client_create(runtime, &client_options, &client);
```

Connection strings use `pmac_bytes {data, len}`, with a length excluding NUL. Save the Request ID after submission:

```c
pmac_request_options request_options = {0};
request_options.struct_size = sizeof(request_options);
request_options.abi_version = PMAC_ABI_VERSION;
request_options.timeout_ms = 5000;
request_options.max_response_bytes = 1024u * 1024u;

const char command[] = "Sys.ServoTime";
pmac_request_id request = 0;
status = pmac_get_response_async(
    client, (const uint8_t*)command, sizeof(command) - 1u,
    &request_options, &request);
```

Wait with `pmac_request_wait`, then call `pmac_request_copy_response` twice: first with an empty buffer to obtain `required`, then again after allocating space to copy the data. Finally call `pmac_request_release`. See `include/pmac.h` for all public functions and structures.

## 3. Requests, Timeouts, and Results

### 3.1 Request parameters

The default total execution timeout is 5000 ms, and the default maximum response is 1 MiB. Set these per call:

| C++17 | .NET | Python | Meaning |
|---|---|---|---|
| `timeoutMs` | `TimeoutMs` | `timeout_ms` | Total deadline from submission, including queueing |
| `infinite` | `Infinite` | `infinite=True` | Remove the total deadline; cancellation and closing remain available |
| `maxResponseBytes` | `MaxResponseBytes` | `max_response_bytes` | Maximum retained response per request |
| `readPool` | `ReadPool` | `read_pool=True` | Allow classifier-approved read-only commands to use pool rules |

The wait budget of `request_wait` limits only that wait. `WAIT_PENDING` does not mean request failure and does not cancel it; continue waiting, poll, or cancel.

### 3.2 Commands and extraction

- `command` / `Command`: submit a GPASCII command and return raw response bytes.
- `get_response` / `GetResponse`: use the same request core, with additional pattern extraction support.
- `Request.response()` / `Response()`: retrieve the complete response.
- `Request.match(...)` / `Match(...)`: copy results by pattern and capture group.
- `Request.stderr()` / `Stderr()`: retrieve Shell standard error.
- `Request.cancel()` / `Cancel()`: request cancellation; the outcome depends on the operation phase.

Exactly empty, `number`, and `variable` patterns use compatible extraction rules. Other patterns use `regex::bytes` byte regular expressions. See [Pattern semantics](pattern-migration.en.md).

### 3.3 Execution certainty

| Value | Meaning | Application handling |
|---|---|---|
| `NOT_SENT` | Provably not sent | May resubmit after resolving other admission issues |
| `MAY_HAVE_EXECUTED` | Sending began or confirmation was lost; the remote operation may have executed | Do not retry blindly; check device state first |
| `CONFIRMED` | Complete protocol confirmation obtained | Protocol completion only; does not mean mechanical motion has completed |

C++ exceptions provide `status()`, `requestId()`, and `certainty()`; .NET `PmacException` provides `Status`, `RequestId`, and `Certainty`; Python `PmacError` provides `status`, `request_id`, and `certainty`.

## 4. Connection Pools and Thread Ordering

Ordinary `connect` uses the initial control configuration. For explicit management, use:

- C++：`preparePoolAsync`、`resizePoolAsync`、`repairPoolAsync`、`drainPoolAsync`；
- .NET：`PreparePoolAsync`、`ResizePoolAsync`、`RepairPoolAsync`、`DrainPoolAsync`；
- Python：`pool_prepare_async`、`pool_resize_async`、`pool_repair_async`、`pool_drain_async`；
- C：`pmac_pool_*_async`。

Pool parameters must satisfy `1 <= min_ready <= target_size <= max_members <= 255`. Members are independent SSH/GPASCII sessions, not an unbounded automatic scaling mechanism.
A single Client can occupy at most **16 online GPASCII channels**, counting precreated, opening, in-use, reclaiming, and release-uncertain channels across all pool members. A 17th channel allocation returns `RESOURCE_LIMIT` before sending. Different Clients each have 16 slots; even when connected to the same controller, they do not share a single 16-channel limit. The device itself may impose additional resource limits. The profile member-count range does not permit exceeding the online channel budget.

Only confirmed channel release returns quota. Old channels with uncertain release continue occupying the owning Client's quota; after verifying remote release, the application explicitly confirms recovery against the revision. One Client's recovery declaration cannot release another Client's occupancy. Maintenance-plane BufferUpload does not consume online GPASCII channel quota and remains available through the maintenance plane when online channels are full.

Key rules:

1. Each calling thread is pinned to a healthy member on its first GPASCII submission and exclusively owns a persistent Channel.
2. Subsequent ordinary commands and `read_pool` requests from that thread do not migrate members; different threads do not share Channels.
3. There is no implicit read-after-write causal ordering or simultaneous snapshot across threads or members.
4. Submit `open prog`, program lines, and `close` from the same thread.
5. After a member fails, threads are not automatically migrated to other members, and old requests are not automatically replayed.
6. Repair the maintenance plane with `repairMaintenanceAsync` / `RepairMaintenanceAsync` / `pool_repair_maintenance_async`.

Use `poolState` / `PoolState()` / `pool_state()` for the pool revision and member counts, `poolMembers` / `PoolMembers()` / `pool_members()` for member states, and `Request.route` / `Route` for the request member ID and epoch.

After an unknown write result, subsequent conflicting operations may return `RECOVERY_REQUIRED`. After checking the device, call `confirmRecovery` / `ConfirmRecovery` / `pool_confirm_recovery` with the matching revision. Confirmation of unknown write results and confirmation of remote connection release are independent declarations.

## 5. Shell and Files

### Shell

- `shell(command, persistent=True)` uses a persistent Shell, retaining session state such as the working directory and environment.
- `persistent=False` uses isolated exec and does not inherit another call's Shell state.
- After completion, read stdout, stderr, and the optional exit code. A nonzero exit code is not a transport failure.
- Long tasks still need a reasonable deadline, or the ability to cancel and close.

### Files

| Feature | C++17 | .NET | Python |
|---|---|---|---|
| Upload | `upload` | `Upload` / `UploadAsync` | `upload` |
| Download | `download` | `Download` / `DownloadAsync` | `download` |
| Buffer upload | `uploadBuffer` | `BufferUpload` / `BufferUploadAsync` | `upload_buffer` |
| Existence check | `fileExists` | `FileExists` | `file_exists` |
| Delete | `removeFile` | `RemoveFile` | `remove_file` |

Uploads stage in the same directory and publish after MD5 verification. `PUBLISH_UNKNOWN` means the final publication outcome is uncertain; do not treat it as unpublished and retry. Deletion, overwriting, and program loading have side effects and should be used in controlled maintenance windows.

## 6. Motion, PLCs, and Status Queries

Functions in this section synchronously send GPASCII commands through a connected Client. Successful control operations mean protocol confirmation; query further to determine whether motion is in position, a program has completed, or a PLC is in the expected state. Python names are used below; add the `pmac_` prefix for the C ABI, use camelCase for C++17 and PascalCase for .NET. Detailed C signatures are in sections 10.6 and 10.7.

### 6.1 Parameters, returns, and errors

| Parameter | Description |
|---|---|
| `n` / `motor` | Motor number. The library validates motor commands and `#n` status queries as 0–255; available device numbers depend on configuration |
| `n` / `coord` | Coordinate system number. The library validates `&n` commands and status queries as 0–127; the actual range depends on device configuration |
| `n` / `plc` | PLC number. The library validates enable and disable commands as 0–31 |
| `p` / `program` | Motion program number to start; must be a positive integer |
| `value` / `position` / `distance` | `double` floating-point value: target position for absolute jog, relative displacement for incremental jog; must be finite. Units and scaling depend on controller configuration |
| Request options | Python accepts `timeout_ms=5000`; C++, .NET, and C use their corresponding request option objects or structures |

Control operations return `None` in Python and `void` in C++17/.NET; C returns `PMAC_OK` or a negative status code. Queries return parsed numeric or Boolean values in Python/C++/.NET; C returns a status code and writes the value through `out`. Higher-level bindings throw exceptions on failure. After timeout or cancellation, check execution certainty. If execution may have occurred, query the device before repeating motion commands.

### 6.2 Motor commands

| Python method | GPASCII command | Meaning, parameters, and result |
|---|---|---|
| `motor_enable(n)` | `#n j/` (actual format `#nj/`) | Send enable to motor `n`; no data returned on success |
| `motor_kill(n)` | `#nkill` | Send kill to motor `n`; no data returned on success |
| `motor_jog_stop(n)` | `#njog/` | Stop jog on motor `n`; no data returned on success |
| `motor_abs_jog(n, value)` | `#njog=value` | Send jog with `value` as the absolute target position |
| `motor_inc_jog(n, value)` | `#njog:value` | Send incremental jog with `value` as relative displacement |
| `abort()` | `&*adisable` | Send global abort/disable; no data returned on success |

For example, `motor_abs_jog(1, 25.0)` targets motor 1, while `motor_inc_jog(1, -2.0)` requests a negative increment of 2 controller position units. Speed, acceleration/deceleration, limits, and permitted motion range depend on controller configuration. Network Abort, Kill, and JogStop cannot replace hardware safety circuits.

### 6.3 Coordinate system and PLC commands

| Python method | GPASCII command | Meaning and result |
|---|---|---|
| `coord_enable(n)` | `&nenable` | Send enable to coordinate system `n`; no data returned on success |
| `coord_exec_prog(n, p)` | `&n begin p run` | Start program `p` in coordinate system `n`; success confirms the command, not program completion |
| `coord_hold(n)` | `&nhold` | Send hold to coordinate system `n`; no data returned on success |
| `coord_run(n)` | `&nrun` | Send run to coordinate system `n`; no data returned on success |
| `plc_enable(n)` | `enable plc n` | Enable PLC `n`; no data returned on success |
| `plc_disable(n)` | `disable plc n` | Disable PLC `n`; no data returned on success |

For example, `coord_exec_prog(1, 10)` sends `&1 begin 10 run`. Use `coord_prog_running(1)` to query the program-active flag and `plc_running(2)` to query the PLC 2 active flag. Each value is one sample; the application must also define the actual task completion criteria.

### 6.4 Motor position, velocity, and status

| Python method | Query | Return value and use |
|---|---|---|
| `motor_position(n)` | `#np` | `float`: actual position |
| `motor_desired_position(n)` | `#nd` | `float`: desired position |
| `motor_velocity(n)` | `#nv` | `float`: velocity |
| `motor_position_error(n)` | `#nf` | `float`: position error |
| `motor_status(n)` | `#n?` | Python returns `MotorStatus`; C ABI returns a `uint64_t` bit field; C++17/.NET methods return the raw 64-bit value |
| `motor_closed_loop(n)` | Read `Motor[n].ClosedLoop` first, then `Motor[n].AmpEna` if true | Boolean: true only if both are true; the two reads are not an atomic snapshot |

Position and error can be used for display and following monitoring respectively; velocity and position units depend on device configuration. A single read is not a simultaneous multi-axis snapshot. The two reads in `motor_closed_loop` may occur at different times; account for this in the application when state is changing.

### 6.5 Coordinate system, PLC, and system status

| Python method | Query | Return value and use |
|---|---|---|
| `coord_status(n)` | `&n?` | Python returns `CoordStatus`; C ABI, C++17, and .NET return the raw 64-bit status value |
| `coord_prog_running(n)` | `Coord[n].ProgActive` | `bool`: coordinate system program-active flag |
| `coord_motor_enabled(n)` | `Coord[n].AmpEna` | `bool`: coordinate system motor-enable flag |
| `plc_running(n)` | `Plc[n].Active` | `bool`: PLC active flag |
| `abort_all_flag()` | `Sys.AbortAll` | `bool`: global abort flag |
| `servo_error_count()` | `Sys.ServoErrorCtr` | Integer: servo error count |
| `phase_error_count()` | `Sys.PhaseErrorCtr` | Integer: phase error count |
| `servo_count()` | `Sys.ServoCount` | Integer: servo count |
| `servo_time()` | `Sys.ServoTime` | `float`: servo cycle time, in controller-defined units |
| `phase_time()` | `Sys.PhaseTime` | `float`: phase cycle time, in controller-defined units |

Numeric parsing failures return a parse error rather than treating invalid responses as zero. Status-flag interfaces query separately and do not form an atomic snapshot.

### 6.6 Interpreting status bits

`motor_status` and `coord_status` preserve all 64 bits. Common bits are listed below, numbered from least significant bit 0. Verify their actual meanings against controller version and configuration.

| Bit | Name | Meaning |
|---:|---|---|
| 63 | `trigger_move` | Triggered-motion-related state |
| 62 | `home_in_progress` | Homing in progress |
| 61 / 60 | `minus_limit` / `plus_limit` | Negative / positive hardware limit state |
| 59 / 58 | `fe_warn` / `fe_fatal` | Following error warning / fatal error |
| 57 / 56 | `limit_stop` / `amp_fault` | Limit stop / amplifier fault |
| 55 / 54 | `soft_minus_limit` / `soft_plus_limit` | Negative / positive software limit state |
| 53 / 50 | `i2t_fault` / `enc_loss` | I²t fault / encoder loss |
| 47 / 46 | `home_complete` / `des_vel_zero` | Homing complete / desired velocity zero |
| 45 / 44 | `closed_loop` / `amp_ena` | Closed-loop / amplifier-enable status bits |
| 43 / 42 | `in_pos` / `interlock_stop` | In-position / interlock-stop status bits |

See `pmac/status.py` for Python status object named fields and two-bit field definitions; see `pmac_status.hpp` for C++ status helpers. Coordinate systems also have two-bit fields such as `feed_hold` and `move_mode`; do not treat these as single Boolean bits. Status method return types for each binding are described in sections 6.4 and 6.5.

### 6.7 Call examples

The following assumes `client` is connected and the application has verified the target, units, and operating conditions. These examples use read-only queries:

```python
position = client.motor_position(1, timeout_ms=5000)
velocity = client.motor_velocity(1, timeout_ms=5000)
closed_loop = client.motor_closed_loop(1, timeout_ms=5000)
plc_active = client.plc_running(2, timeout_ms=5000)
program_active = client.coord_prog_running(1, timeout_ms=5000)
print(position, velocity, closed_loop, plc_active, program_active)
```

To control the device, use the following method forms. Before actual execution, the application must verify device state, travel, and motion permission:

```python
# client.motor_enable(1, timeout_ms=5000)
# client.motor_abs_jog(1, 25.0, timeout_ms=5000)
# client.motor_inc_jog(1, -2.0, timeout_ms=5000)
# client.motor_jog_stop(1, timeout_ms=5000)
# client.motor_kill(1, timeout_ms=5000)
# client.coord_enable(1, timeout_ms=5000)
# client.coord_exec_prog(1, 10, timeout_ms=5000)
# client.coord_hold(1, timeout_ms=5000)
# client.coord_run(1, timeout_ms=5000)
# client.plc_enable(2, timeout_ms=5000)
# client.plc_disable(2, timeout_ms=5000)
# client.abort(timeout_ms=5000)
```

Check C ABI status codes before using outputs:

```c
double position = 0.0;
uint32_t active = 0;
int32_t status = pmac_motor_position(client, 1, &request_options, &position);
if (status == PMAC_OK)
    status = pmac_plc_running(client, 2, &request_options, &active);
/* Use position and active only when status == PMAC_OK. */
```

## 7. Common Statuses and Handling

| Status | Meaning and handling |
|---|---|
| `INVALID_ARGUMENT` | Check structure size, ABI, required strings, numeric values, and buffer arguments |
| `INVALID_HANDLE` | Object released or ID not associated with a currently valid object; do not reuse old IDs |
| `QUEUE_FULL` / `RESOURCE_LIMIT` | Reduce concurrency, release completed requests, or adjust bounded capacity |
| `EXEC_TIMEOUT` | Total execution deadline expired; assess certainty and do not automatically replay writes |
| `WAIT_PENDING` | This wait expired; the request may still continue |
| `CANCELLED` | Request cancelled; use certainty to assess whether execution may have occurred |
| `DISCONNECTED` | The corresponding connection failed; check the network, member states, and explicit repair procedure |
| `AUTHENTICATION` | Check username, password/private key, passphrase, and private key format |
| `HOST_KEY` | Host identity mismatch or untrusted host; do not hide changes by disabling verification |
| `BUFFER_TOO_SMALL` | Allocate according to `required`, then retrieve using the same Request |
| `SHUTDOWN_PENDING` | Close or release is incomplete; ID is retained, so handle child objects and retry |
| `RECOVERY_REQUIRED` | Unknown write result exists; check the device and explicitly confirm recovery |
| `MAINTENANCE_ACTIVE` | A maintenance barrier is blocking related new operations |
| `SUPERSEDED` | A management operation or recovery revision was superseded by newer state |
| `PUBLISH_UNKNOWN` | File publication outcome is uncertain; check remote state first |

See `include/pmac.h` for all status values. C++ also provides `statusName`, `errorCategory`, and `isTransportError` for diagnostic classification; classification alone is not an automatic retry strategy.

## 8. Shutdown and Fault Recovery

Explicit cleanup is recommended:

1. Stop submitting new operations;
2. Cancel requests no longer needed and wait for terminal state;
3. Read results still needed, then release Requests;
4. Call Client close and release only after it completes successfully;
5. Call Runtime shutdown;
6. Release remaining Client/Request IDs, then release Runtime.

`SHUTDOWN_PENDING` does not mean the object is lost: retain its ID, handle unfinished work or child objects, then retry. `pmac_client_release` now requires completed Client close and return of all GPASCII resources.

If uncertain remote-release records remain after closing, find the retained Client through `pmac_client_recovery_ids`. Confirm `REMOTE_RELEASE` only after the application verifies that remote sessions have actually been released; closing a local socket is not proof.

Before unloading a DLL/so, also ensure that no thread is still executing a C entry point. When using native cleanup supervisors, call `pmac_runtime_cleanup_wait` to wait for supervisor threads to exit.

## 9. Security and Troubleshooting

- Inject credentials through secure configuration or the process environment; do not include them in source code, logs, crash dumps, or release packages.
- Prefer known_hosts or a pinned fingerprint. If a host key changes, first verify whether the controller was replaced or reinstalled.
- When both a private key and password are configured, only the private key is attempted. Remove the private key configuration to use password login.
- If initial connection is slow, measure TCP, SSH authentication, and the first GPASCII response separately. Do not tune performance solely by shortening timeouts.
- On `NO_READY_LANE`, check member state and pool capacity; do not allow multiple threads to bypass Channel quotas.
- When write timeouts, disconnects, or cancellation races occur, preserve request ID, route, status, and certainty before deciding on recovery.
- Network Abort/Kill/JogStop are not safety-certified functions. Mechanical safety requires an independent, validated safety circuit.

## 10. API Function Reference

This section covers public functions using the C ABI names in `include/pmac.h`. C++17, .NET, and Python expose the same operations as object methods; see earlier sections and section 10.8 for names and examples. C++98 provides only the synchronous interfaces actually declared by that adapter. The header is authoritative for exact signature types, structure layouts, and platform export conventions.

### 10.1 Common parameter and return conventions

| Parameter | Meaning |
|---|---|
| `id` / `runtime` | Valid ID of the corresponding Runtime, Client, or Request; the function signature determines its type |
| `config` | Configuration structure; set `struct_size` and `abi_version` per the header, and zero other reserved fields |
| `options` | Connection or request options; see section 3.1 for request timeout, infinite waiting, response limit, and read-only pool flags |
| `out_id` | ID of a newly created object or submitted request; the caller owns release after success |
| `command`, `command_len` | Command byte pointer and byte count; do not rely on NUL termination |
| `local`, `remote`, and corresponding `_len` | Byte pointer and byte count for a local or controller path; argument order differs between upload and download |
| `out`, `capacity`, `required` | Output buffer, capacity, and actual required length; allocate in each function's units, with byte-copy interfaces measured in bytes |
| `wait_ms` | Limit for this wait in milliseconds; request options separately control the execution deadline |
| `motor` / `coord` / `plc` / `program` | Controller motor, coordinate system, PLC, or program number |

Except for version queries, the C functions in these tables directly return an `int32_t` status: `PMAC_OK` means that call succeeded; negative values indicate errors or incompletion, as described in section 7. Successful asynchronous submission means only that a request was admitted; wait on the Request for the final operation status. Before calling, check pointers and lengths, structure size, and ABI. Use outputs only on success or where the corresponding contract allows. C++/.NET/Python generally convert failures to exceptions and return actual data; synchronous operations without results usually return `void` / `None`.

### 10.2 Runtime and Client

| Function | Purpose and parameters | Output / return |
|---|---|---|
| `pmac_runtime_create(config, out_id)` | Create a runtime; `worker_threads` specifies worker count | Status; `out_id` is the Runtime ID |
| `pmac_runtime_wait_completion(id, cursor, wait_ms)` | Wait for completion notification; `cursor` is the caller-maintained notification cursor | Status; updates the cursor, but individual requests still need checking after notification |
| `pmac_runtime_wake(id)` | Wake completion notification waiters | Status |
| `pmac_runtime_shutdown(id, wait_ms)` | Request shutdown and wait within the given time | Status; may return `SHUTDOWN_PENDING` if unfinished, allowing retry |
| `pmac_runtime_release(id)` | Release a shut-down Runtime with no unreleased child objects | Status |
| `pmac_runtime_cleanup(id)` | Transfer Runtime cleanup to a cleanup supervisor | Status; handle remaining objects according to the shutdown contract |
| `pmac_runtime_cleanup_wait()` | Wait for cleanup supervisor threads to exit | Status |
| `pmac_get_runtime_info(out_info)` | Query runtime ABI, version, capabilities, and pointer width | Status and `pmac_runtime_info` |
| `pmac_version_string()` | Query version string | Pointer to library-managed read-only string; caller must not release it |
| `pmac_version_number()` / `pmac_abi_version()` | Query numeric version / ABI version | `uint32_t` version number |
| `pmac_client_create(runtime, config, out_id)` | Create a client under a Runtime; configure request, file-task, and retained-byte limits | Status and Client ID |
| `pmac_client_close(id, close_mode, wait_ms)` | Close a client; use `close_mode` per header and ABI contract | Status; retain ID and retry if unfinished |
| `pmac_client_release(id)` | Release a client ID after closing completes | Status |
| `pmac_client_recovery_ids(out, capacity, required)` | Enumerate clients with release records still awaiting confirmation; capacity is in IDs | Status and required ID count |
| `pmac_client_get_state(id, out_state)` | Query connection/closing state, epoch, last response time, and active request count | Status and `pmac_state_snapshot` |

### 10.3 Requests and results

| Function | Purpose and parameters | Output / return |
|---|---|---|
| `pmac_connect_submit(id, options, out_id)` | Submit SSH and GPASCII connection using `pmac_connect_options` | Status and Request ID |
| `pmac_command_submit(id, command, command_len, options, out_id)` | Submit a GPASCII command | Status and Request ID; read response through Request |
| `pmac_get_response_async(id, command, command_len, options, out_id)` | Submit a GPASCII response request; pattern extraction occurs at result retrieval | Status and Request ID |
| `pmac_request_wait(id, wait_ms, out_info)` | Wait for or poll a request; `out_info` receives status, certainty, and result lengths | Status; `WAIT_PENDING` means this wait ended but the request is not terminal |
| `pmac_request_cancel(id)` | Request cancellation; does not guarantee no remote execution | Status; still wait for terminal state and check certainty |
| `pmac_request_copy_response(id, out, capacity, required)` | Copy standard response; may query size first | Status and required byte count |
| `pmac_request_copy_stderr(id, out, capacity, required)` | Copy Shell stderr | Status and required byte count |
| `pmac_request_copy_error(id, out_info, out, capacity, required)` | Retrieve structured error and error text | Status, error information, and required text byte count |
| `pmac_request_copy_match(id, pattern, pattern_len, group, out, capacity, required)` | Extract completed response with `pattern` and zero-based capture `group` | Status and required result byte count; see [Pattern semantics](pattern-migration.en.md) |
| `pmac_request_get_route(id, member_id, epoch)` | Retrieve the pool member and connection epoch used by the request | Status and two `uint64_t` outputs |
| `pmac_request_release(id)` | Release Request ID and result resources | Status; the ID cannot be read after release |

Typical sequence: submit and check status → save ID → `pmac_request_wait` until terminal state → retrieve needed results or errors → `pmac_request_release`. If output space is insufficient, reallocate according to `required` and copy from the same Request; this does not resend the device command.
### 10.4 DART binary interface

First call `pmac_connect_submit` and wait for successful SSH/GPASCII connection, then establish DART. See the [DART API Guide](dart-api.en.md) for commands, element types, addresses, and byte order. Each function directly returns submission status; retrieve operation results from the Request identified by `out_id`.

| Function | Parameter meaning | Request result |
|---|---|---|
| `pmac_dart_connect_async(id, options, out_id)` | `id` is a Client; `options` contains request options | DART is available after successful wait |
| `pmac_dart_disconnect_async(id, options, out_id)` | Close the DART connection | Wait for close result |
| `pmac_dart_get_async(id, command, address, data_len, options, out_id)` | `command` is a read command; `address` is a USHM byte offset or P number; `data_len` is bytes | Response contains binary data in host native byte order |
| `pmac_dart_set_async(id, command, address, data, data_len, options, out_id)` | `command` is a write command; `data` points to host-native-order data, with `data_len` in bytes | Success after full confirmation; verify uncertain writes instead of replaying blindly |

Example: read P1–P100 using `PMAC_DART_GET_P_VARIABLE`, `address=1`, and `data_len=100 * sizeof(double)`. Length must be nonzero, no greater than `PMAC_DART_MAX_DATA_BYTES`, and a multiple of element byte width; USHM ranges and P numbers must also meet header constant limits. New requests may reconnect after a data-channel failure; an explicit disconnect requires connecting again.

### 10.5 Shell, files, and connection pools

The following `*_submit` / `*_async` functions return submission status and write a Request ID through `out_id`. After waiting, retrieve operation results through Request.

| Function | Purpose, parameters, and completion result |
|---|---|
| `pmac_shell_submit(id, command, command_len, persistent, options, out_id)` | Execute Shell; nonzero `persistent` uses a persistent session, zero uses isolated exec. Request contains stdout, stderr, and an optional exit code |
| `pmac_upload_submit(id, local, local_len, remote, remote_len, options, out_id)` | Upload local file to remote path; wait for publication result |
| `pmac_download_submit(id, remote, remote_len, local, local_len, options, out_id)` | Download remote file to local path; path argument order is the reverse of upload |
| `pmac_buffer_upload_submit(id, local, local_len, remote, remote_len, cleanup_remote, options, out_id)` | Upload and load a program buffer; nonzero `cleanup_remote` removes the remote file after loading |
| `pmac_file_exists_submit(id, remote, remote_len, options, out_id)` | Check remote file existence; response is one ASCII byte `0` or `1` |
| `pmac_file_remove_submit(id, remote, remote_len, options, out_id)` | Delete the specified remote file; wait for operation result |
| `pmac_pool_prepare_async(id, connect, profile, out_id)` | Prepare pool using connection options and pool configuration; wait for management request completion |
| `pmac_pool_resize_async(id, profile, out_id)` | Adjust target pool size to the new configuration |
| `pmac_pool_repair_async(id, member_id, options, out_id)` | Repair specified failed or drained members; `member_id=0` selects all eligible members within the target range |
| `pmac_pool_drain_async(id, member_id, options, out_id)` | Drain specified members; `member_id=0` selects all except the control member |
| `pmac_pool_repair_maintenance_async(id, options, out_id)` | Repair the maintenance connection |
| `pmac_pool_get_state(id, out_state)` | Synchronously retrieve pool revision, member counts, and recovery state; directly returns status |
| `pmac_pool_get_members(id, out_members, capacity, required)` | Synchronously copy member snapshots; capacity and required count are measured in members |
| `pmac_pool_confirm_recovery(id, expected_revision, flags)` | After checking the device, confirm recovery against the current revision; `flags` selects unknown-write and/or remote-release confirmation; directly returns status |

`pmac_pool_profile` contains `target_size`, `max_members`, `min_ready`, and `connect_timeout_ms`. File paths are specified by a pointer and corresponding length. If upload publication is uncertain, check the remote file first rather than retrying automatically.

### 10.6 Motion control

These functions are synchronous C helpers: `id` is a Client and `options` contains request options. They directly return status, not a Request ID. Number arguments identify the target coordinate system, motor, PLC, or program; `position` and `distance` are finite floating-point values in controller units.

| Function | Purpose and additional parameters | Success return |
|---|---|---|
| `pmac_abort(id, options)` | Send global abort | `PMAC_OK` |
| `pmac_coord_enable(id, coord, options)` | Enable specified coordinate system | `PMAC_OK` |
| `pmac_coord_exec_prog(id, coord, program, options)` | Execute `program` in `coord` | `PMAC_OK` |
| `pmac_coord_hold(id, coord, options)` / `pmac_coord_run(id, coord, options)` | Hold / run specified coordinate system | `PMAC_OK` |
| `pmac_plc_enable(id, plc, options)` / `pmac_plc_disable(id, plc, options)` | Enable / disable PLC | `PMAC_OK` |
| `pmac_motor_kill(id, motor, options)` | Send kill to motor | `PMAC_OK` |
| `pmac_motor_enable(id, motor, options)` | Enable motor | `PMAC_OK` |
| `pmac_motor_jog_stop(id, motor, options)` | Stop jog on specified motor | `PMAC_OK` |
| `pmac_motor_abs_jog(id, motor, position, options)` | Absolute-position jog; `position` is target position | `PMAC_OK` |
| `pmac_motor_inc_jog(id, motor, distance, options)` | Incremental jog; `distance` is displacement | `PMAC_OK` |

`PMAC_OK` means protocol interaction completed; confirm actual action completion through status queries and device safety logic.
### 10.7 Status and numeric queries

These C functions execute synchronously. Common parameters are Client `id`, request `options`, and output pointer `out`. They directly return `PMAC_OK` or an error status and write results into `out` on success. `motor`, `coord`, and `plc` identify targets. Floating-point units depend on controller configuration.

| Function | Additional input | `out` type and meaning |
|---|---|---|
| `pmac_coord_status` | `coord` | `uint64_t`, coordinate system status bits |
| `pmac_motor_status` | `motor` | `uint64_t`, motor status bits |
| `pmac_motor_position` | `motor` | `double`, actual position |
| `pmac_motor_desired_position` | `motor` | `double`, desired position |
| `pmac_motor_velocity` | `motor` | `double`, velocity |
| `pmac_motor_position_error` | `motor` | `double`, position error |
| `pmac_servo_error_count` | None | `uint32_t`, servo error count |
| `pmac_phase_error_count` | None | `uint32_t`, phase error count |
| `pmac_servo_count` | None | `uint32_t`, servo count |
| `pmac_phase_time` | None | `double`, phase cycle time |
| `pmac_servo_time` | None | `double`, servo cycle time |
| `pmac_abort_all_flag` | None | `uint32_t`, global abort flag; 0 is false, nonzero is true |
| `pmac_plc_running` | `plc` | `uint32_t`, PLC active flag |
| `pmac_coord_prog_running` | `coord` | `uint32_t`, coordinate system program-active flag |
| `pmac_motor_closed_loop` | `motor` | `uint32_t`, motor closed-loop flag |
| `pmac_coord_motor_enabled` | `coord` | `uint32_t`, coordinate system motor-enable flag |

For example:

```c
double position = 0.0;
int32_t status = pmac_motor_position(client, 1, &request_options, &position);
if (status == PMAC_OK) {
    /* position is the actual position of motor 1. */
}
```

### 10.8 C++17, .NET, and Python method mapping

The following table lists names, parameters, and results for common methods. Optional request options are C++ `RequestOptions`, .NET `RequestOptions?`, and Python keyword options. Bindings do not expose exactly the same method sets; their source declarations are authoritative.

| Operation | C++17 | .NET | Python | Parameters and result |
|---|---|---|---|---|
| Connect | `connectSync(options)` | `ConnectAsync(options, cancellationToken)` | `connect(host, username, password=None, **kwargs)` | See section 2 for connection configuration; no data returned on success |
| Submit connection asynchronously | `connect(options)` | `Connect(options)` | `connect_async(...)` | Returns `Request`; caller waits and releases |
| GPASCII | `getResponse(command, pattern, options)` | `GetResponse(command, pattern, options)` | `get_response(command, pattern=None, **kwargs)` | `command` is the command, `pattern` is optional; returns response or matched bytes |
| Submit GPASCII | `getResponseAsync(command, options)` | `GetResponseAsync(command, options)` | `get_response_async(command, **kwargs)` | Returns Request; read results through Request |
| Direct command | `command(command, options)` | `Command(command, options)` | `command(command, **kwargs)` | Returns raw response bytes; corresponding `Async` / `_async` returns Request |
| Shell | `shell(command, persistent, options)` | `Shell(command, persistent, options)` | `shell(command, persistent=True, **kwargs)` | Returns stdout bytes; use asynchronous Request for stderr or exit code |
| DART connect / disconnect | `dartConnect` / `dartDisconnect` | `DartConnect` / `DartDisconnect` | `dart_connect` / `dart_disconnect` | No data returned on success; corresponding asynchronous methods return Request |
| DART read | `dartGet(command, address, length, options)` | `DartGet(command, address, length, options)` | `dart_get(command, address, length, **kwargs)` | `length` is bytes; returns bytes in host byte order |
| DART write | `dartSet(command, address, data, options)` | `DartSet(command, address, data, options)` | `dart_set(command, address, data, **kwargs)` | `data` is a host-byte-order byte sequence; no data returned on success |
| Upload / download | `upload(local, remote)` / `download(remote, local)` | `Upload(local, remote)` / `Download(remote, local)` | `upload(local, remote)` / `download(remote, local)` | Paths are strings; no data returned on success |
| File existence / delete | `fileExists(remote)` / `removeFile(remote)` | `FileExists(remote)` / `RemoveFile(remote)` | `file_exists(remote)` / `remove_file(remote)` | Existence query returns Boolean; no data returned on successful deletion |
| State query | `state()`, `isConnected()` | `State()`, `IsConnected` | `state()`, `connected` | Returns state snapshot or Boolean |
| Close | `close(waitMs)` | `Close(waitMs)` | `close(wait_ms)` | Waits for closing; resource release separately follows object lifetime rules |

C++ `Request.wait()` returns no data on success, while `waitNoThrow(waitMs)` returns request information. `.NET Request.Wait()` / `WaitAsync()` and Python `Request.wait()` / `wait_async()` wait for completion; retrieve responses with `Response()` / `response()`. See section 3.2 and each binding's declarations for Request match, stderr, cancellation, and release methods.

Python DART example:

```python
import struct

client.dart_connect(timeout_ms=5000)
try:
    data = client.dart_get(0x90, 1, 8, timeout_ms=5000)
    print("P1 =", struct.unpack("=d", data)[0])
    # Execute only after the application confirms that P1 may be written:
    client.dart_set(0x91, 1, struct.pack("=d", 100.0), timeout_ms=5000)
finally:
    client.dart_disconnect(timeout_ms=5000)
```

See the [DART API Guide](dart-api.en.md) for complete C++ and .NET DART examples and multielement transfer rules.
## Real-Time Continuous Gather

C and C++17 provide continuous sampling interfaces that receive row-major values corresponding to `Gather.Addr` in batches. See [Detailed Gather Usage](gather.en.md) for complete integration code, build commands, parameter ranges, callback and memory rules, stopping/release, network deployment, and error recovery.

Before starting, verify SSH host identity and ensure that the controller can connect back to the specified local address/port. The library validates the source IP and a random handshake for this session, but the data stream remains plaintext TCP. Stop and release Gather before closing Client and Runtime. The callback thread should only process or copy the current batch; return blocking stop and destruction to the main thread.
## geterrors Real-Time Status Monitoring

Connect the Client first, then start continuous monitoring with C `pmac_geterrors_start` or C++17 `client.startGetErrors()`. The period is 0.01–1.0 seconds. Four mask classes are optional; omission uses system defaults, while an explicit zero mask differs from omission. Startup settings and device state changes arrive as different message types. To stop, send Ctrl+C to this SSH terminal; release the Monitor after the stop barrier completes, then close Client/Runtime.

See [Detailed geterrors Usage](geterrors.en.md) for complete default/custom mask examples, C/C++ programs, compile commands, callback memory conventions, status interpretation, and field troubleshooting.
