# Python: start with GetResponse

[English](python-getresponse.en.md) | [简体中文](python-getresponse.md)

GetResponse supports per-request passthrough: bypass the rule table and online/program classification and submit one command verbatim. See **[GetResponse passthrough mode and language examples](getresponse-passthrough.en.md)**.

The Python binding is in [`bindings/python`](../bindings/python). It calls the native library through `ctypes`, without requiring an extension module. Start with `Client.get_response()` for ordinary command requests: it creates a Request, waits for completion, copies the response, and releases the Request. The runnable example is [`examples/python/getresponse.py`](../examples/python/getresponse.py); by default it only reads `Sys.ServoTime`.

## 1. Prepare the native library and Python package

Python 3.9 or later is required. The Python process and native library must have the same architecture, such as Windows x64 Python with x64 `pmac.dll`. Build from the repository root:

```text
cargo build -p pmac-ffi --locked
```

Debug libraries are `target/debug/pmac.dll` on Windows and `target/debug/libpmac.so` on Linux. For release builds add `--release` and use `target/release` paths. See [build instructions](build-and-install.en.md) for platform dependencies and installation requirements.

Import the Python package using either method:

- Install into the current Python environment: `python -m pip install ./bindings/python`. The distribution name is `pmac-russh`; the import name is `pmac`.
- During development, point `PYTHONPATH` to the repository's `bindings/python`; installation is unnecessary.

`PMAC_LIBRARY` is an environment variable read by the example, which resolves it to an absolute path and passes it to `Runtime(library_path=...)`. The binding itself does not read this variable. Use a native library matching the current binding version to prevent a search path from loading an old DLL.

## 2. Configure the connection and run

Run the following commands from the repository root. `PMAC_KNOWN_HOSTS` must point to a known_hosts file containing a verified controller host key; do not bypass this configuration by disabling host verification. See [authentication](authentication.en.md) for host identity and key preparation. The default account is `root`, configurable through `PMAC_USER`. The password is read only from the environment and is not stored in source code.

PowerShell:

```powershell
$env:PYTHONPATH = Join-Path $PWD 'bindings/python'
$env:PMAC_LIBRARY = (Resolve-Path 'target/debug/pmac.dll').Path
$env:PMAC_HOST = 'controller-host'
$env:PMAC_USER = 'root'
$env:PMAC_KNOWN_HOSTS = 'C:\pmac\known_hosts'
# Set PMAC_PASSWORD for this process using your credential management method.
python examples/python/getresponse.py --help
python examples/python/getresponse.py --native-version
python examples/python/getresponse.py
python examples/python/getresponse.py --command 'P1' --pattern variable
```

Linux:

```sh
export PYTHONPATH="$PWD/bindings/python${PYTHONPATH:+:$PYTHONPATH}"
export PMAC_LIBRARY="$PWD/target/debug/libpmac.so"
export PMAC_HOST='controller-host'
export PMAC_USER='root'
export PMAC_KNOWN_HOSTS="$HOME/.ssh/known_hosts"
# Set PMAC_PASSWORD for this process using your credential management method.
python3 examples/python/getresponse.py --help
python3 examples/python/getresponse.py --native-version
python3 examples/python/getresponse.py
python3 examples/python/getresponse.py --command 'P1' --pattern variable
```

`--help` does not import the binding, load the native library, or connect. `--native-version` only loads the native library and reports its version and ABI; no controller or connection credentials are needed. Other invocations require all connection environment variables. `--command` sends its argument once as a controller command; when supplying write or motion commands, the application must confirm their effects.

## 3. Synchronous GetResponse and returned bytes

The minimal call sequence follows. Connection and command failures can both raise `PmacError`:

```python
import os
from pmac import Runtime

with Runtime(library_path=os.environ['PMAC_LIBRARY']) as runtime:
    with runtime.create_client() as client:
        client.connect(
            os.environ['PMAC_HOST'], os.environ.get('PMAC_USER', 'root'),
            os.environ['PMAC_PASSWORD'], known_hosts=os.environ['PMAC_KNOWN_HOSTS'],
        )
        data = client.get_response('Sys.ServoTime', timeout_ms=5000)
        print(data.decode('utf-8', errors='replace'))
```

The return value is `bytes`, not necessarily a number or text. By default, the example decodes UTF-8 with replacement characters for invalid bytes, adds a newline only when the response lacks a final newline, and does not trim content. Pass `--raw` to preserve exact bytes; this mode adds no newline. Do not use decoded string length as the byte count.

`--timeout-ms` sets the command request budget: the example allows 1..4294967295 milliseconds, default 5000. This differs from the SSH connection budget and continuous monitoring period. A timeout does not prove non-execution and does not trigger automatic retry. Exit code 0 means completion; 1 means native-library or controller request failure; 2 means configuration, import, or local I/O failure; 130 means user interruption.

## 4. Pattern extraction and capture groups

The `pattern` in `get_response(command, pattern=None, **kwargs)` extracts from the same response after request completion, without resending the command. See [pattern rules](pattern-migration.en.md).

| pattern | Result |
| --- | --- |
| `None` or empty string | Complete response bytes; an empty response is valid |
| `number` | Legacy-compatible loose numeric fragment, not strict numeric parsing |
| `variable` | First non-whitespace fragment after the first `=` |
| Other string | Group 0 of a Rust byte regex: the entire match |

`number` and `variable` are case-sensitive. For `P1=3.14`, `number` returns `b'1'` while `variable` returns `b'3.14'`; do not treat `number` as an extractor for the right-hand value of a named variable. `number` may also accept fragments such as `1e+`; validate further when a numeric value is required. `variable` requires an equals sign; do not assume every command returns the same format.

```powershell
python examples/python/getresponse.py --command 'P1' --pattern variable
python examples/python/getresponse.py --command 'P1' --pattern '^P[0-9]+=(.+)$'
```

The second command extracts group 0, still including `P1=`. To obtain capture group 1 while also inspecting the response and separate stderr, use an explicit Request:

```python
# client is already connected as in the preceding example.
with client.get_response_async('P1', timeout_ms=5000) as request:
    request.wait()
    raw_response = request.response()
    stderr_bytes = request.stderr()
    value = request.match(r'^P[0-9]+=(.+)$', group=1)
    print(value.decode('utf-8', errors='replace'))
```

The regex must match actual controller output, including any newlines; this expression illustrates capture groups. Byte regexes do not support lookaround or backreferences. No match must not be treated as a valid numeric value; invalid expressions report parse errors. Perform all extraction within the current Request lifetime without repeating the command.

## 5. Pass complete errors to the application

The runnable example catches `PmacError`, writes `status`, `certainty`, and `request_id` to stderr, then outputs the full `str(error)` verbatim and exits nonzero. Application code should retain this information rather than return an empty string or print only “communication failed” and continue.

```python
from pmac import PmacError

try:
    data = client.get_response('Sys.ServoTime', timeout_ms=5000)
except PmacError as error:
    # Pass these fields and the complete text to application logs, UI, or exception handlers.
    print(error.status, error.certainty, error.request_id)
    print(str(error))
    raise
```

NAK or a remote `error #` diagnostic in stdout is a synchronous failure, including `stdin:1:1: error #31: Buffer closed due to another thread:`; the library preserves the original diagnostic. ACK completes the request immediately. An `error #` in stderr is only an asynchronous diagnostic: it does not reclassify ACK as failure and may be delivered through the current or a later request's `stderr()`. Do not automatically send `close` or resend commands. Operations requiring business-level success, such as program upload, must verify device state rather than relying on ACK alone. To read separate stdout/stderr, retain an explicit Request and obtain bytes with `response()`/`stderr()` rather than issuing another command.

Execution certainty describes transmission and acknowledgment state: 0 is `NOT_SENT`, 1 is `MAY_HAVE_EXECUTED`, and 2 is `CONFIRMED`. `CONFIRMED` means complete protocol acknowledgment, not application success or completed mechanical motion. `MAY_HAVE_EXECUTED` requires the application to check device state; recovery involving writes requires explicit application confirmation. See the [ABI contract](abi-contract.en.md).

## 6. Lifetime, threads, and program buffers

Nest `with Runtime(...)` → `with runtime.create_client()`. Explicit Requests should have their own `with` and finish waiting/copying results before exit; synchronous `get_response()` manages its Request internally. On exit, close the Client and release its resources before closing Runtime. Do not unload the native library early or depend on interpreter shutdown destructor ordering. A `Runtime` cannot be reused after `fork` as a new connection in the child process.

A Client can occupy at most **16 GPASCII online channels** simultaneously, counting prebuilt pool members, those being created, in use, being recycled, and those with uncertain release. The 17th allocation reports a resource limit; Python's GIL does not bypass this limit. Each Client has its own quota, but external programs and other Clients can still affect global controller buffer state.

For buffer operations such as `open prog`, opening, writing the body, and `close` must run on the same calling thread and original channel context; do not distribute the steps across a thread pool. After another thread, Client, or external program executes `close all buffers`, the original channel may report error #31 on its next write. The application must receive the error and decide how to recover; this example does not send an extra `close`.

## 7. Gather and geterrors (follow-up features)

After ordinary requests work, add [Gather continuous sampling](gather.en.md) and [geterrors real-time state-change monitoring](geterrors.en.md) as needed. They are continuous sessions with a different lifecycle from a single `get_response()` request/response. The current Python convenience binding does not wrap these callback interfaces. Detailed guides provide C ABI and C++17 usage; Python integrations written with `ctypes` must manage callback references, callback threads, and stop/release ordering.

`geterrors` requires a dedicated continuous listening session and exits the remote program using Ctrl+C. The detection period is configurable from 0.01..1.0 seconds; unspecified monitoring masks retain controller defaults. Do not run it as an ordinary single GetResponse command that indefinitely occupies a GPASCII channel.

GetResponse also supports an optional passthrough request setting, enabled by default, to send one command verbatim while bypassing online/program classification. For language syntax, single-frame limits, and error semantics, see [passthrough mode](getresponse-examples.en.md#optional-passthrough-mode-bypass-onlineprogram-rules).
