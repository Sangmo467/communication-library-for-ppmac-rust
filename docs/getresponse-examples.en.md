# GetResponse integration and examples across languages

[English](getresponse-examples.en.md) | [简体中文](getresponse-examples.md)

GetResponse supports per-request passthrough: bypass the rule table and online/program classification and submit one command verbatim. See **[GetResponse passthrough mode and language examples](getresponse-passthrough.en.md)**.

First complete “connect → send command → wait → read response → close” with GetResponse, then add continuous sampling and monitoring. The examples query `Sys.ServoTime` by default; other user-supplied commands are actually executed.

| Language | Main interface | Runnable code | Detailed steps |
| --- | --- | --- | --- |
| C | `pmac_get_response_async` + wait/copy/release | [getresponse.c](../examples/c/getresponse.c) | This page |
| C++17 | `Client::getResponse` | [getresponse.cpp](../examples/cpp/getresponse.cpp) | This page |
| C# / .NET 8 | `Client.GetResponse` | [Program.cs](../examples/dotnet/getresponse/Program.cs) | [.NET guide](dotnet-getresponse.en.md) |
| Python ≥3.9 | `Client.get_response` | [getresponse.py](../examples/python/getresponse.py) | [Python guide](python-getresponse.en.md) |
| MATLAB | `pmac_mex('getresponse', ...)` | [example_getresponse.m](../examples/matlab/example_getresponse.m) | [MATLAB MEX guide](matlab-getresponse.en.md) |

## 1. Build and connection configuration

Build the native library from the repository root:

```sh
cargo build -p pmac-ffi --locked
```

Windows produces `target/debug/pmac.dll` and the MSVC import library `pmac.dll.lib`; Linux produces `target/debug/libpmac.so`. Add `--release` for release builds and use `target/release`. Process, compiler target, and native library architectures must match. See [build documentation](build-and-install.en.md) for other installation options.

The C/C++, Python, and MATLAB examples read these environment variables. Private-key variables are read only by the C/C++ and MATLAB examples; the Python example uses password authentication. See the .NET guide for its requirements.

| Environment variable | Meaning |
| --- | --- |
| `PMAC_HOST` | Required controller address |
| `PMAC_USER` | SSH user, default `root` |
| `PMAC_KNOWN_HOSTS` | Required verified OpenSSH known_hosts file |
| `PMAC_PASSWORD` | Password for password authentication |
| `PMAC_PRIVATE_KEY` | Optional private-key file path; takes precedence when set |
| `PMAC_PASSPHRASE` | Optional private-key passphrase |

Supply passwords through the current process environment or application credential management; examples contain no device password. See [authentication](authentication.en.md) for host key preparation and pinned fingerprints.

```powershell
$env:PMAC_HOST = 'controller-host'
$env:PMAC_USER = 'root'
$env:PMAC_KNOWN_HOSTS = 'C:\pmac\known_hosts'
# Set PMAC_PASSWORD or PMAC_PRIVATE_KEY using your credential management method.
```

## 2. C: compose a complete GetResponse using the asynchronous ABI

The C ABI has no synchronous function of this name. The example wraps this sequence; ordinary calling threads can use a wait loop for synchronous behavior:

1. `pmac_runtime_create`, `pmac_client_create`.
2. `pmac_connect_submit`, `pmac_request_wait`; check the connection result and release the connection Request.
3. Submit the command with `pmac_get_response_async` and configure its deadline; keep waiting when `pmac_request_wait` returns `PMAC_WAIT_PENDING`.
4. After success, query the length with `pmac_request_copy_response`, then copy bytes. For extraction, use `pmac_request_copy_match` without resending the command.
5. Release the Request, close/release Client, then shut down/release Runtime. This standalone example then exits; embedded hosts unloading a dynamic library must additionally wait for `pmac_runtime_cleanup_wait()` before unloading.

Responses are byte arrays represented by address + length, with no guaranteed NUL terminator. The example uses `fwrite` with the actual length. On failure, copy the complete error before releasing Request; do not print only a status number and continue.

Compile on Windows in a developer terminal configured for x64 MSVC:

```powershell
New-Item -ItemType Directory -Force target/examples | Out-Null
cl /nologo /W4 /std:c11 /I include examples/c/getresponse.c /Fe:target/examples/getresponse_c.exe /Fo:target/examples/getresponse_c.obj /link target/debug/pmac.dll.lib
$env:PATH = (Join-Path $PWD 'target/debug') + ';' + $env:PATH
& ./target/examples/getresponse_c.exe --help
& ./target/examples/getresponse_c.exe
& ./target/examples/getresponse_c.exe 'P1' variable
```

Linux:

```sh
mkdir -p target/examples
cc -std=c11 -Wall -Wextra -Iinclude examples/c/getresponse.c -Ltarget/debug -lpmac -o target/examples/getresponse_c
export LD_LIBRARY_PATH="$PWD/target/debug${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
./target/examples/getresponse_c 'P1' variable
```

## 3. C++17: synchronous GetResponse

`pmac.hpp` is a header wrapper and still requires linking the same native library. `getResponse` internally submits, waits, copies the result, and releases Request, returning `std::vector<uint8_t>`.

```cpp
pmac::RequestOptions options;
options.timeoutMs = 5000;
auto raw = client.getResponse("Sys.ServoTime", "", options);
auto value = client.getResponse("P1", "variable", options);
```

Each of these two calls sends a command once. To extract multiple values from the same response, retain an explicit Request and call `match` repeatedly rather than calling `getResponse` again.

Windows x64 MSVC:

```powershell
cl /nologo /EHsc /std:c++17 /W4 /I include /I bindings/cpp examples/cpp/getresponse.cpp /Fe:target/examples/getresponse_cpp.exe /Fo:target/examples/getresponse_cpp.obj /link target/debug/pmac.dll.lib
$env:PATH = (Join-Path $PWD 'target/debug') + ';' + $env:PATH
& ./target/examples/getresponse_cpp.exe --help
& ./target/examples/getresponse_cpp.exe 'P1' variable
```

Linux:

```sh
c++ -std=c++17 -Wall -Wextra -Iinclude -Ibindings/cpp examples/cpp/getresponse.cpp -Ltarget/debug -lpmac -o target/examples/getresponse_cpp
export LD_LIBRARY_PATH="$PWD/target/debug${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
./target/examples/getresponse_cpp 'P1' variable
```

The complete example catches `pmac::Error` and prints status, execution certainty, Request ID, and diagnostic text. Long-running applications should reuse Runtime/Client rather than reconnect for each query.

## 4. Responses, patterns, and errors

| pattern | Meaning | Example |
| --- | --- | --- |
| Omitted or empty string | Return original complete response bytes | Preserve `P1=3.14` verbatim |
| `variable` | Compatibility mode: value after the first `=` | `P1=3.14` → `3.14` |
| `number` | Compatibility mode: first matching numeric text | `P1=3.14` → `1`; may select the variable number |
| Other string | Rust byte regex; group 0 by default | Capture parentheses do not change the default group |

Reserved words are case-sensitive; patterns do not change the command sent to the device. For a specific capture group, use an explicit Request's match interface and specify the group number. See [GetResponse patterns](pattern-migration.en.md) for complete rules. Check errors, matches, and format before parsing numbers; do not treat empty responses or error text as 0.

Every standard `error #N` is a remote error, including errors accompanied by ACK; the original diagnostic must reach the application. For example, `stdin:1:1: error #31: Buffer closed due to another thread:` means the original program buffer context is invalid. The library does not automatically send an extra `close` or replay the original command.

`CONFIRMED` means complete protocol acknowledgment; it cannot replace success status or motion completion checks. `MAY_HAVE_EXECUTED` means execution may already have occurred. After failed writes, the application must check state before choosing recovery rather than retry directly. See the [ABI contract](abi-contract.en.md).

## 5. Threads and lifetime

A Client occupies at most 16 online GPASCII channels; multiple Clients count independently. Channels retain affinity to the submitting calling thread. Program `open prog`, body, and `close` must run consecutively on the same calling thread. A `close all buffers` from other Clients or external programs may still affect the current context.

Complete Requests, copy results, and release them before closing Client, then Runtime. Automatic resource management in C++/C#/Python does not replace handling close failures. Plugins, MEX gateways, and hosts must also complete native cleanup barriers before unloading a DLL, leaving no running callbacks or cleanup threads.

## Optional passthrough mode: bypass online/program rules

GetResponse/command defaults to passthrough, bypassing online/program classification and local program-context checks while forwarding each line unchanged. Disable it explicitly for rule validation: C uses PMAC_REQUEST_CLASSIFY (8), C++ uses passthrough=false, C# uses Passthrough:false, Python uses passthrough=False, and MATLAB uses struct('passthrough', false). Read-pool requests automatically enable classification; explicit C PASSTHROUGH (4) cannot be combined with READ_POOL or CLASSIFY. Basic framing checks and remote error # reporting always apply.

GetResponse defaults to passthrough, bypassing classification by `online-command-rules.tsv`. It does not require a locally confirmed `open prog`, and makes no distinction between online commands and program commands. Original spaces, semicolons, and letter case are preserved; GPASCII still adds its normal frame terminator.

| Language | How to enable |
| --- | --- |
| C | Set `options.flags |= PMAC_REQUEST_PASSTHROUGH`, then call `pmac_get_response_async` |
| C++17 | Set `options.passthrough = true` and pass it to `getResponse(command, pattern, options)` |
| C++98 | Set `passthrough = true` in `pmac98::Options` and pass it to `getResponse` |
| C# | `new RequestOptions(Passthrough: true)` |
| Python | `client.get_response(command, passthrough=True)` |
| MATLAB MEX | Fourth argument: `struct('passthrough', true)` |

```cpp
pmac::RequestOptions options;
options.passthrough = true;
auto raw = client.getResponse("dwell 10", "", options);
```

```csharp
var raw = client.GetResponse("dwell 10", options: new RequestOptions(Passthrough: true));
```

```python
raw = client.get_response('dwell 10', passthrough=True, timeout_ms=5000)
```

```matlab
raw = pmac_mex('getresponse', 'dwell 10', '', struct('passthrough', true));
```

These calls send commands; whether the device accepts them depends on its current state. The example `dwell 10` normally requires the corresponding program buffer state on the device. The application must send open, body, and close sequentially on the same calling thread. Passthrough does not parse or maintain program buffer state, or automatically add maintenance barriers for commands such as reset/save. Use the same mode throughout a program operation to avoid mixing it with classification mode, which depends on local context.

Passthrough requests are always scheduled as control writes with potential side effects and retain thread/channel affinity. Explicit C PASSTHROUGH cannot be combined with READ_POOL; that combination fails before transmission. Language bindings automatically select classification when the read pool is requested. Request timeouts, cancellation, response limits, resource quotas, error reporting, and the no-automatic-replay contract remain in effect. Every `error #N`, including #31, still returns failure with the original diagnostic; passthrough does not automatically send close or repair context based on program semantics.

Passthrough still represents a single GPASCII frame: at most 1023 bytes. Blank commands, NUL, CR/LF, ACK, and NAK are rejected; multiple lines/frames cannot be combined in one string. Submit multiline programs one line per request. This option does not change response regex extraction. The C ABI RequestOptions layout is unchanged; passthrough uses flag 4 and explicit classification uses flag 8. A native library containing this feature is required; older libraries reject unknown flags. `pmac_command_submit` supports the same option; other business interfaces do not use it to bypass rules.

## 6. Next: Gather and geterrors

After GetResponse works, see [Gather continuous sampling](gather.en.md) and [geterrors real-time state monitoring](geterrors.en.md). Both are continuous sessions whose full lifecycle cannot be obtained through one ordinary GetResponse. The current C ABI/C++17 provides dedicated interfaces and callbacks; Python, .NET, and this MATLAB MEX example do not add these callback wrappers.

geterrors listens continuously through a dedicated PTY and sends Ctrl+C to the remote when stopped. Its period is 0.01–1.0 seconds; unspecified masks use device defaults. When extending MATLAB callbacks, copy and queue data on native threads and retrieve it actively from the MATLAB calling thread; see the extension boundaries in the MATLAB guide.
