# Real-time Continuous Gather

[中文](gather.md) | [English](gather.en.md)

C ABI and C++17 wrappers are currently provided. They reuse the connected Client's SSH authentication and GPASCII requests; no controller credentials need to be supplied again. The reverse data connection must pass controller source-IP verification and a random handshake for this session before sample callbacks begin.

## C++17

```cpp
#include "pmac.hpp"
#include <atomic>
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

        pmac::GatherConfig config;
        config.period = 1;
        config.addresses = {"Motor[1].ActPos.a", "Motor[2].ActPos.a"};
        config.port = 18999; // Change this port, or use 0 for an available port.
        std::atomic<uint64_t> samples{0};
        auto gather = client.startGather(config,
            [&](const pmac::GatherBatch& batch) {
                // batch.at(row, 0/1) contains Motor[1/2].ActPos.a.
                // Copy with batch.copy() before handing values to another thread.
                samples.fetch_add(batch.rows, std::memory_order_relaxed);
            },
            [](const pmac::Error& error) {
                std::cerr << "Gather stopped: " << error.what() << '\n';
            });
        std::cout << "Gather running. Press Enter to stop.\n";
        std::cin.get();
        gather.stop();
        std::cout << "Received " << samples.load() << " sample rows.\n";
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
```

`period` directly sets `Gather.Period`, in controller servo cycles. `Gather.Items` is automatically set to `addresses.size()`, and each address is written to `Gather.Addr[i]` in the original order. Addresses support a single symbol or numeric address expression, such as `Motor[1].ActPos.a`, `Sys.P[8190].a`, or `$100000`; multiple commands, newlines, and arithmetic expressions are rejected. The library accepts at most 128 items. The actual available count still depends on the controller and its configuration; controller rejection returns an error.

`GatherBatch` is a borrowed view: `rows` is the number of sample groups, `items` is the address count, and `data` is a row-major `double` array, where `data[row * items + item]` corresponds to `Gather.Addr[item]`. `firstSample` is the index of the first row, starting at 0 for this session. It is not a controller timestamp; the library does not treat local reception time as sample time.

See [gather.cpp](../examples/cpp/gather.cpp) for a complete integration example.

## Configuration

| C++ field | Default | Meaning |
| --- | --- | --- |
| `period` | `1` | `Gather.Period`; must be greater than 0 |
| `addresses` | Empty | Required address list; its count automatically becomes `Gather.Items` |
| `port` | `0` | Local TCP listening port; specify a port such as 18999, or 0 for automatic allocation |
| `localIp` | Empty | Automatically obtains the local IP seen by the SSH peer from `SSH_CONNECTION`; may be overridden with a local numeric IP |
| `maxSamples` | `2048` | Range 2..=1048576; `Gather.MaxSamples` for the controller's circular acquisition buffer, not the total session sample count |
| `batchRows` | `128` | Range 1..=4096; maximum rows per batch; partial batches are pushed about every 20 ms, subject to scheduling |
| `queueBatches` | `32` | Range 1..=1024; bounded batch queue between the receiver and callback threads |
| `timeoutMs` | `5000` | Timeout for each configuration/cleanup request and waiting for a verified reverse connection; rejected connections do not reset the deadline; does not limit total acquisition duration |

Set `config.port` to change the port. The library successfully starts listening first, then passes the actual port to the controller's `nc`. A port in use or an unbindable local IP causes a connection error at startup. For a fixed port, allow controller access through the firewall. Automatic addresses may be unbindable or unreachable from the controller under NAT or VPN; in that case, set `localIp` to an address that is both locally bindable and reachable by the controller.

## Callbacks and stopping

The receiver thread continuously reads TCP, parses text, and enqueues data into a bounded queue. A separate callback thread delivers batches serially. The queue absorbs temporary slowdowns in callbacks. Sustained inability to keep up terminates acquisition and reports a resource-limit error through the error callback and `stop()`, avoiding silent sample loss. The configurable queue holds at most 64 MiB of numeric data, plus small receive and current-batch buffers. Controller-reported overrun, incorrect column counts, non-finite values, and data-connection closure also terminate acquisition.

- Data pointers are valid only within the current callback. Call `batch.copy()` to retain data.
- Callbacks may occur before `startGather()` returns; prepare the application state they depend on beforehand.
- Callbacks run on a background thread. Dispatch UI updates to the UI thread, and lengthy disk writes or computation to an application worker thread.
- A callback may call `requestStop()`, which only requests stopping. Do not call blocking `stop()`, destroy Gather, close Client, or close Runtime inside a callback.
- Call `stop()` outside callbacks. It waits for the receiver thread, cleanup task, and running callback to finish. After it returns or throws a terminal error, no further callbacks occur. Explicit stopping discards queued batches not yet delivered.
- Exceptions thrown by C++ callbacks are caught and trigger a stop request, then are rethrown by `stop()`; they do not cross the C ABI.
- The C++ Gather destructor stops and releases the session without throwing. Explicitly call `stop()` to learn about acquisition or cleanup failures.
- Stop and destroy Gather before closing Client and Runtime. Releasing Runtime while a Gather handle remains unreleased returns `PMAC_SHUTDOWN_PENDING`.

## Single-session convention and controller requirements

The implementation follows scanner2 continuous acquisition: over SSH, it starts a pipeline from `gather -W` to `nc` with a session-handshake prefix, writes addresses, Period, Items, and MaxSamples, then executes `Gather.Enable=1`, `Gather.Enable=0`, and `Gather.Enable=3`. There is no cumulative sample limit.

The controller requires executable `gather`, `stdbuf`, `nc`, `nohup`, `sh`, and `killall`, and must be able to connect back to the local TCP port. The data channel is plain TCP from the controller to the local host, suitable for directly reachable controller networks.

The library obtains the controller address from `SSH_CONNECTION` in the authenticated SSH Shell, verifies the reverse TCP source IP, and requires the sender to provide an independently generated 256-bit cryptographically secure random handshake value for this startup. SSH configures the handshake value in the remote pipeline. Wrong sources and incorrect or previous-session handshakes cannot become sample sources: the library closes those connections and continues waiting. Each candidate handshake waits at most 200 ms and cannot exceed the shared startup deadline. Time spent starting the controller pipeline also counts toward that deadline. Silent connections, wrong sources, and repeated incorrect handshakes do not extend startup waiting. The handshake is not delivered to users as sample data.

`localIp` only sets the local listener and reverse-connection destination; it cannot override the expected controller source. Startup fails if `SSH_CONNECTION` is missing or invalid, or if NAT or multi-interface routing makes the reverse source differ from the SSH server address. The library does not fall back to accepting arbitrary sources. Ensure consistent reverse source addresses in deployment.

**Security boundary: source IP and the session handshake prevent unrelated connections that do not know the current handshake from taking over, but the data stream remains unencrypted. The handshake is sent as TCP plaintext; it cannot defend against impersonation after link eavesdropping, data tampering, or a man-in-the-middle, and cannot replace an SSH tunnel. Restrict the listening port to the specified controller. When SSH-equivalent confidentiality and integrity are required, use a trusted encrypted network; the current interface does not provide an SSH data tunnel.**

Gather configuration is controller-global state; only one acquisition session may run on a controller at a time. The library uses `/tmp/pmac-library-gather.lock` to prevent its sessions from overwriting each other. Other software must also honor this exclusivity and must not modify Gather configuration concurrently.

Under the single-session convention, startup and stopping execute `killall gather nc`, so `gather` and `nc` on that controller should be dedicated to acquisition. Normal stopping first executes `Gather.Enable=0`, then terminates processes and removes this session's lock.

After a process crash or SSH interruption, the library may be unable to confirm remote cleanup and will not take over a session whose lock remains. After confirming that the old session is no longer valid, use the existing command/Shell APIs to execute `Gather.Enable=0` and the following recovery commands, then restart:

```sh
killall gather nc >/dev/null 2>&1 || true
rm -f /tmp/pmac-library-gather.lock/owner
rmdir /tmp/pmac-library-gather.lock
```

A remote lock conflict returns a startup error; cleanup failures are reported as terminal errors. `stop()` does not restore the Gather configuration that existed before startup.

## C ABI

1. Zero-initialize `pmac_gather_config`, then set `struct_size`, `abi_version`, `period`, `items`, and `addresses`. Other fields may remain 0 to use defaults. `port` may specify any bindable listening port.
2. Call `pmac_gather_start(client, &config, callback, user, &id)`. Configuration and address strings may be freed after the function returns; `user` must remain alive until stopping completes.
3. For `status == PMAC_OK`, read `rows * items` doubles in the callback. Nonzero `status` indicates a terminal error; `message` is length-delimited UTF-8 text. All pointers are valid only during that callback. Return nonzero to request stopping.
4. Inside callbacks, use `pmac_gather_request_stop(id)`; outside callbacks, wait for stopping with `pmac_gather_stop(id)`, then release the handle with `pmac_gather_release(id)`.

`stop` is idempotent and retains the terminal error for repeated queries. Even when `release` returns a previous terminal error, stopping and handle destruction are complete; do not release again. `stop/release` return `PMAC_INVALID_ARGUMENT` inside their own callback and do not destroy the handle in that case. Uncaught C/C++ exceptions must not cross the native callback boundary.

The new APIs preserve existing ABI structure layouts. Other languages can integrate through the C ABI; the newly added convenience wrapper is currently C++17.

### Complete C11 example

Save the following as `gather.c`. The example gets connection parameters from environment variables and stops when Enter is pressed on the main thread; no Gather ID needs to be shared between threads. `PMAC_KNOWN_HOSTS` points to a verified known_hosts file, and `PMAC_PASSWORD` may provide the password. For private keys, configure connection options as described in [Authentication](authentication.en.md).

```c
#include "pmac.h"
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct { uint64_t rows; } samples;

static pmac_bytes bytes(const char *s) {
    pmac_bytes b = { (const uint8_t *)s, s ? strlen(s) : 0 };
    return b;
}
static int32_t on_data(void *user, int32_t status, const double *data,
                       size_t rows, size_t items, uint64_t first_sample,
                       const uint8_t *message, size_t message_len) {
    samples *result = (samples *)user;
    if (status != PMAC_OK) {
        fprintf(stderr, "Gather terminal status=%d: ", (int)status);
        if (message_len) fwrite(message, 1, message_len, stderr);
        fputc('\n', stderr);
        return 0;
    }
    /* data[row * items + item], row < rows, item < items.
       first_sample is the zero-based sample index of this batch's first row. */
    if (first_sample == 0 && rows && items)
        printf("first batch: rows=%zu items=%zu first_sample=%" PRIu64
               " first_value=%.17g\n", rows, items, first_sample, data[0]);
    result->rows += rows;
    /* For another thread, copy all rows * items doubles into an owned buffer
       and enqueue it with the metadata. Never enqueue data/message/user pointers
       without separately preserving their ownership and lifetime. */
    return 0; /* nonzero requests stop; do not call stop/release here */
}
int main(void) {
    pmac_runtime_id runtime = 0;
    pmac_client_id client = 0;
    pmac_request_id request = 0;
    pmac_gather_id gather = 0;
    pmac_runtime_config rc = {0};
    pmac_client_config cc = {0};
    pmac_connect_options connection = {0};
    pmac_gather_config config = {0};
    pmac_request_info info = {0};
    pmac_bytes addresses[2];
    samples result = {0}; /* used by callback only until stop barrier */
    const char *host = getenv("PMAC_HOST");
    const char *known_hosts = getenv("PMAC_KNOWN_HOSTS");
    int32_t status = PMAC_OK, stopped, released;
    int failed = 0;
    if (!host || !*host || !known_hosts || !*known_hosts) {
        fputs("Set PMAC_HOST and PMAC_KNOWN_HOSTS\n", stderr);
        return 1;
    }
    rc.struct_size = sizeof(rc); rc.abi_version = PMAC_ABI_VERSION;
    rc.worker_threads = 2;
    status = pmac_runtime_create(&rc, &runtime);
    if (status != PMAC_OK) goto cleanup;
    cc.struct_size = sizeof(cc); cc.abi_version = PMAC_ABI_VERSION;
    cc.max_requests = 64; cc.max_file_tasks = 1;
    cc.max_retained_bytes = 80u * 1024u * 1024u;
    status = pmac_client_create(runtime, &cc, &client);
    if (status != PMAC_OK) goto cleanup;
    connection.struct_size = sizeof(connection);
    connection.abi_version = PMAC_ABI_VERSION;
    connection.host = bytes(host); connection.username = bytes("root");
    connection.password = bytes(getenv("PMAC_PASSWORD"));
    connection.known_hosts = bytes(known_hosts);
    connection.port = 22; connection.control_pool_size = 1;
    connection.flags = PMAC_CONNECT_TCP_PROBE;
    connection.tcp_probe_timeout_ms = 600;
    connection.keepalive_interval_ms = 1000; connection.keepalive_max = 3;
    status = pmac_connect_submit(client, &connection, &request);
    if (status != PMAC_OK) goto cleanup;
    do { status = pmac_request_wait(request, UINT32_MAX, &info); }
    while (status == PMAC_WAIT_PENDING);
    (void)pmac_request_release(request); request = 0;
    if (status != PMAC_OK) goto cleanup;
    addresses[0] = bytes("Motor[1].ActPos.a");
    addresses[1] = bytes("Motor[2].ActPos.a");
    config.struct_size = sizeof(config); config.abi_version = PMAC_ABI_VERSION;
    config.period = 1; config.items = 2; config.addresses = addresses;
    config.port = 18999; /* 0 selects an available local TCP port */
    status = pmac_gather_start(client, &config, on_data, &result, &gather);
    if (status != PMAC_OK) goto cleanup;
    puts("Gather running. Press Enter to stop.");
    (void)getchar();
cleanup:
    if (status != PMAC_OK) {
        fprintf(stderr, "operation failed: %d\n", (int)status); failed = 1;
    }
    if (gather) {
        stopped = pmac_gather_stop(gather); /* callback lifetime barrier */
        released = pmac_gather_release(gather); gather = 0;
        /* A terminal acquisition/cleanup error can be returned again by release;
           the valid handle has nevertheless been destroyed. Do not retry it. */
        if (stopped != PMAC_OK || released != PMAC_OK) {
            fprintf(stderr, "Gather stop=%d release=%d\n", (int)stopped, (int)released);
            failed = 1;
        }
        printf("Received %" PRIu64 " sample rows\n", result.rows);
    }
    if (client) {
        status = pmac_client_close(client, 0, 5000);
        if (status == PMAC_OK) status = pmac_client_release(client);
        if (status != PMAC_OK) {
            fprintf(stderr, "Client cleanup pending/failed: %d; client=%" PRIu64
                    " runtime=%" PRIu64 " retained\n", (int)status, client, runtime);
            return 1; /* standalone process exits; see retry guidance below */
        }
        client = 0;
    }
    if (runtime) {
        status = pmac_runtime_shutdown(runtime, 5000);
        if (status == PMAC_OK) status = pmac_runtime_release(runtime);
        if (status != PMAC_OK) {
            fprintf(stderr, "Runtime cleanup pending/failed: %d; id=%" PRIu64
                    " retained\n", (int)status, runtime);
            return 1;
        }
    }
    return failed;
}
```

This example is a standalone program. When integrating into a long-running process, preserve the printed Client/Runtime IDs if closing fails, complete recovery, and retry closing/releasing. Do not treat `PMAC_SHUTDOWN_PENDING` as a completed release. Stop and release Gather first, close/release Client next, and finally shutdown/release Runtime. Release the Request ID immediately after the connection request completes. Before dynamically unloading the native library, also follow the [ABI lifecycle contract](abi-contract.en.md) and wait for the cleanup supervisor thread; background work must not continue using an unloaded DLL/so.

The main thread reads callback `result` only after the stop barrier, so this example needs no cross-thread atomic counter. Add atomics or a mutex if the main thread reads statistics during acquisition. The example prints only a small amount of information in the first batch. For actual high-frequency data, copy it into a bounded application queue; printing each row to the console would slow callbacks.

## Building and running

Run the following commands from the repository root. Save the C example as root-level `gather.c`; for C++, use the existing `examples/cpp/gather.cpp`. The application and native library must use matching bitness and ABI. The repository CMake configuration defines `pmac.dll` and MSVC import library `pmac.dll.lib` on Windows, and `libpmac.so` on Linux. The current CMake example targets do not include Gather; compile it separately with the commands below.

Windows (x64 Native Tools Command Prompt for VS 2022, MSVC):

```bat
cargo build -p pmac-ffi --release --locked
cl /nologo /W4 /TC /std:c11 /Iinclude gather.c target\release\pmac.dll.lib /Fe:gather_c.exe
cl /nologo /W4 /EHsc /std:c++17 /Iinclude /Ibindings\cpp examples\cpp\gather.cpp target\release\pmac.dll.lib /Fe:gather_cpp.exe
set "PATH=%CD%\target\release;%PATH%"
set "PMAC_HOST=192.168.0.200"
set "PMAC_KNOWN_HOSTS=C:\pmac\known_hosts"
rem For password authentication, set PMAC_PASSWORD in this environment; keep it out of scripts and version control.
gather_cpp.exe
rem Or run gather_c.exe
```

Linux (GCC/G++, terminal):

```sh
cargo build -p pmac-ffi --release --locked
cc -std=c11 -Wall -Wextra -Iinclude gather.c -Ltarget/release -lpmac \
  -Wl,-rpath,'$ORIGIN/target/release' -o gather_c
c++ -std=c++17 -Wall -Wextra -Iinclude -Ibindings/cpp examples/cpp/gather.cpp \
  -Ltarget/release -lpmac -Wl,-rpath,'$ORIGIN/target/release' -o gather_cpp
export PMAC_HOST=192.168.0.200
export PMAC_KNOWN_HOSTS="$HOME/.ssh/known_hosts"
# For password authentication, provide PMAC_PASSWORD in this environment.
./gather_cpp
# Or run ./gather_c
```

If Cargo explicitly uses `--target <triple>`, change the compile/link path to `target/<triple>/release` and adjust Windows PATH or Linux rpath accordingly. See [Build and installation](build-and-install.en.md) for package linking paths. Before running, operations personnel should verify the controller host key through a trusted channel and prepare known_hosts; do not enable arbitrary host-key acceptance just to run an example.

## Configuration limits and capacity planning

C requires `period`, `items`, and `addresses`; set `struct_size=sizeof(pmac_gather_config)`, `abi_version=PMAC_ABI_VERSION`, and `reserved=0`. In C/C++, 0 for `max_samples/maxSamples`, `batch_rows/batchRows`, `queue_batches/queueBatches`, or `timeout_ms/timeoutMs` selects the default. Range validation follows default substitution. `period` accepts positive `uint32_t` values; actual support depends on the controller. Address count is 1..=128; each address is 1..=256 UTF-8 bytes and allows only ASCII letters, digits, and `_.$[]`. The controller also checks whether addresses exist and are suitable for Gather.

An empty `local_ip/localIp` enables automatic discovery. A manual value must be a locally bindable numeric IPv4/IPv6 unicast address; hostnames, wildcard addresses, multicast, and IPv4 broadcast addresses are rejected. Ports are 0 or 1..=65535. The sample interval is approximately `Gather.Period × controller servo cycle`; do not interpret `period` as milliseconds.

Numeric queue capacity is `batchRows × queueBatches × items × sizeof(double)` and must not exceed 64 MiB. For example, 128 rows, 32 batches, and 3 items require 98,304 bytes, plus receive buffers, the current batch, and application-owned data copies. Batch data consists of contiguous `rows × items` doubles; `rows` may be smaller than `batchRows`, and queued batches not yet delivered are discarded when stopping. `maxSamples` controls the controller circular buffer; it does not limit cumulative samples across the session.

`timeoutMs` is the budget for each SSH/GPASCII configuration or cleanup operation. Startup involves several operations, so it is not an overall `startGather()` duration limit. The reverse-connection stage uses a separate shared deadline, consumed jointly by pipeline startup, candidate connections, and handshakes. Each candidate handshake waits at most 200 ms and cannot exceed that shared deadline. Continuous acquisition has no total session duration limit.

## Common errors and troubleshooting

| Symptom or status | Checks and action |
| --- | --- |
| `PMAC_INVALID_ARGUMENT` | Check structure size/version, reserved fields, address characters, configuration ranges, and the 64 MiB queue limit; stop/release inside their own callback also return this error; use request_stop or return nonzero instead |
| `PMAC_CONNECT_ERROR` or startup wait timeout | Check local IP bindability, port conflicts, firewall, and controller-to-listener routing; ensure gather/stdbuf/nc/nohup/sh/killall are available on the controller |
| Source-IP / handshake verification failure | Check SSH_CONNECTION, whether the reverse source matches the SSH server address, and NAT/VPN/multi-interface routing; localIp only changes the listener and reverse destination, not source verification |
| `PMAC_AUTHENTICATION` / `PMAC_HOST_KEY` | Fix SSH authentication, private-key permissions, or known_hosts/pinned fingerprints before starting Gather; see the authentication document |
| Remote lock conflict or `PMAC_REMOTE_ERROR` | Check for other acquisition processes or failed controller commands; recover the lock as described below only after confirming that the old session is no longer valid; do not take over a running session |
| `PMAC_RESOURCE_LIMIT` / overrun | Determine whether the controller buffer overflowed or the local callback queue filled; reduce addresses, increase the sample period, adjust circular-buffer/queue sizes within limits, and move slow operations outside callbacks |
| `PMAC_PARSE_ERROR` / `PMAC_PROTOCOL_ERROR` | Check controller gather output, address count, columns per row, and non-finite values; preserve error messages for diagnosis and do not continue using invalid data as normal samples |
| `PMAC_DISCONNECTED` / `PMAC_CLOSING` | Check whether data TCP, SSH, Client, or Runtime closed; after termination, stop/release, reconnect, and create a new Gather; sessions are not automatically replayed |
| stop/release returns a cleanup error | Local callbacks have stopped, but remote shutdown still needs verification; a valid handle is destroyed even when release returns a terminal error and must not be released again |
| Runtime release returns `PMAC_SHUTDOWN_PENDING` | Release Gather, Request, and other child IDs first and finish closing/releasing Client; if resource state is uncertain, perform the explicit recovery required by the ABI document before retrying |

Manual remote-lock recovery is only appropriate after confirming that the old acquisition has stopped and that the current application exclusively owns the controller. First execute `Gather.Enable=0` through authenticated GPASCII, then execute the earlier `killall` and lock-directory cleanup commands through Shell. Check return statuses and verify that processes and the lock are gone before creating a new Gather. If SSH is disconnected, restore a usable control connection first; deleting local handles does not substitute for verifying remote state. Recovery stops gather/nc processes on that controller and does not restore previous Gather configuration.
