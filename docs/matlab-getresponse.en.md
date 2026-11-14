# MATLAB: GetResponse MEX example

[English](matlab-getresponse.en.md) | [简体中文](matlab-getresponse.md)

GetResponse supports per-request passthrough: bypass the rule table and online/program classification and submit one command verbatim. See **[GetResponse passthrough mode and language examples](getresponse-passthrough.en.md)**.

MATLAB can call the native library through this project's C ABI. This C++17 MEX gateway starts with connection and synchronous GetResponse, without requiring MATLAB code to pass native object pointers or maintain 64-bit Request IDs.

Files: [build script](../examples/matlab/build_pmac_mex.m), [MEX source](../examples/matlab/pmac_mex.cpp), and [calling example](../examples/matlab/example_getresponse.m). The MEX maintains one Runtime/Client session at a time for repeated queries before closing.

## 1. Prepare the environment

You need 64-bit MATLAB, a C++17 compiler supported by that MATLAB release, and a native pmac library of the same architecture. The build script supports Windows x64 (MSVC or MATLAB-supported MinGW) and Linux x64. Actual compilation was verified on Windows with MATLAB R2023a and Visual C++ 2022. Other MATLAB releases, compilers, and Linux require validation in the target environment.

First build the Rust native library from a terminal in the repository root:

```sh
cargo build -p pmac-ffi --locked
```

Windows should have `target/debug/pmac.dll` and the MSVC-required `pmac.dll.lib`; Linux should have `target/debug/libpmac.so`. Do not mistake MATLAB managed files for native dynamic libraries.

Then set MATLAB's current directory to the repository root:

```matlab
addpath(fullfile(pwd, 'examples', 'matlab'));
mex -setup C++
build_pmac_mex('debug');
version = pmac_mex('version')
```

For release libraries, first run `cargo build -p pmac-ffi --release --locked`, then `build_pmac_mex('release')` in MATLAB. Use `build_pmac_mex('release', 'C:\pmac\native')` for a custom library directory, or an absolute directory on Linux. The script does not build Rust or download dependencies.

The script uses the `-R2018a` Matrix API, enables C++17, and retains MATLAB's default exception handling options. Do not add `/EHsc` to override MSVC's `/EHs`, as this may affect MEX error propagation. Windows automatically adds the native library directory to the current MATLAB process PATH and verifies that the DLL is x86-64. Linux uses rpath to the selected library directory; rebuild when distributing to a different path, or set `LD_LIBRARY_PATH` before starting MATLAB. Compiler options follow the [MathWorks mex documentation](https://www.mathworks.com/help/matlab/ref/mex.html).

`pmac_mex('version')` loads the library without connecting to a controller. Complete this check first; correct the loading path if DLLs/dependencies are missing or architectures differ.

## 2. Run the complete example with environment variables

Configure the target and verified known_hosts in MATLAB. Supply the password through your credential management mechanism using `PMAC_PASSWORD`, or use `PMAC_PRIVATE_KEY` with optional `PMAC_PASSPHRASE`.

```matlab
setenv('PMAC_HOST', 'controller-host');
setenv('PMAC_USER', 'root');
setenv('PMAC_KNOWN_HOSTS', 'C:\pmac\known_hosts');
response = example_getresponse();               % Decode the raw Sys.ServoTime response as UTF-8 text
value = example_getresponse('P1', 'variable');  % Extract the value after the equals sign
```

`example_getresponse` handles connection, querying, UTF-8 decoding, and `onCleanup` closure; every call creates an independent session. For repeated ordinary variable collection, use the persistent session below to avoid repeated handshakes. See [authentication](authentication.en.md) for private keys and host identity settings.

## 3. Repeat GetResponse after connecting

```matlab
options = struct;
options.host = getenv('PMAC_HOST');
options.username = 'root';
options.password = getenv('PMAC_PASSWORD');
options.knownHosts = getenv('PMAC_KNOWN_HOSTS');
pmac_mex('connect', options);
cleanup = onCleanup(@() pmac_mex('close'));

try
    raw = pmac_mex('getresponse', 'Sys.ServoTime');
    assert(isa(raw, 'uint8'));
    text = native2unicode(raw, 'UTF-8');
    fprintf('%s\n', text);

    rawValue = pmac_mex('getresponse', 'P1', 'variable');
    valueText = native2unicode(rawValue, 'UTF-8');
    value = str2double(valueText);
    if isnan(value), error('example:InvalidNumber', 'Unexpected numeric response: %s', valueText); end
catch exception
    fprintf(2, '%s: %s\n', exception.identifier, exception.message);
    rethrow(exception);  % Preserve the original diagnostic for the caller
end
clear cleanup          % Close the session now on the normal path
```

Scripts can also call `pmac_mex('close')` explicitly; closing an already closed session is allowed to succeed. During reuse, the entire connection/request sequence runs on the MATLAB calling thread. The MEX does not return native pointers or convert IDs to doubles that lose precision.

## 4. Gateway commands and configuration

| Call | Return or behavior |
| --- | --- |
| `pmac_mex('version')` | Returns a version string with an output; prints version and ABI without an output |
| `pmac_mex('connect', options)` | Creates a session; an existing session must first be closed; can return logical true |
| `raw = pmac_mex('getresponse', command)` | One synchronous request, returning a uint8 row vector |
| `raw = pmac_mex('getresponse', command, pattern)` | Extracts pattern group 0 from the same response |
| `pmac_mex('close')` | Waits for closure and releases the session and cleanup tasks; can return logical true |

Each call accepts zero or one output. GetResponse still executes the command when no output is requested and discards its result. Command and pattern accept char row vectors or string scalars, with no NUL; this example limits each text argument to 64 KiB.

All connect fields are optional unless required below; unknown fields cause an argument error.

| Field | Default and meaning |
| --- | --- |
| `host` | Required, nonempty address |
| `username` | `root`, nonempty |
| `port` | 22, range 1–65535 |
| `password` | Empty; password authentication |
| `privateKey` / `publicKey` / `passphrase` | Empty; key paths and private-key passphrase |
| `knownHosts` | Empty; trusted known_hosts path; required by the example |
| `pinnedSha256` | Optional SHA-256 host fingerprint as 32 uint8 bytes, not a hexadecimal string |
| `acceptAnyHostKey` | false; trusted knownHosts or a pinned fingerprint is required |
| `tcpProbe` / `tcpProbeTimeoutMs` | true / 600 milliseconds |
| `keepaliveIntervalMs` / `keepaliveMax` | 1000 milliseconds / 3 attempts |
| `workers` | 2, range 1–256; Runtime worker count |

Integer settings require nonnegative integer scalars; bool settings accept logical or numeric 0/1. The example fixes the initial control pool to 1 and does not yet expose per-request timeout, cancellation, or asynchronous interfaces; extend the MEX if needed. Native worker count is not the GPASCII channel count.

## 5. Errors, patterns, and closing

An empty pattern preserves the full response; `variable` extracts the value after `=`; legacy `number` returns `1` first for `P1=3.14`. Ordinary regexes return group 0 by default; capture parentheses do not automatically select group 1. See [pattern rules](pattern-migration.en.md). The MEX preserves response bytes; only the example script decodes them as UTF-8. Keep binary data as uint8.

Native errors become MATLAB exceptions with identifier `pmac:Native`; the message contains the complete diagnostic, status, certainty, and Request ID. Argument errors use `pmac:Argument`. Remote errors, including `error #31`, must not be ignored as ordinary responses; commands are not automatically resent and close is not automatically sent. `CONFIRMED` is not application success; `MAY_HAVE_EXECUTED` requires the application to verify device state.

The MEX calls mexLock when creating a session and calls mexUnlock only after successfully closing all native resources and waiting for cleanup. Run `pmac_mex('close')` before `clear pmac_mex` or recompilation. If closing fails, the session and lock remain for retry; do not unload the DLL directly. The MATLAB exit callback also attempts native cleanup; persistent failure may extend exit waiting. `onCleanup` is an aid; applications must still record and explicitly handle close errors. See MathWorks [mexLock](https://www.mathworks.com/help/matlab/apiref/mexlock.html), [mexUnlock](https://www.mathworks.com/help/matlab/apiref/mexunlock.html), and [mexAtExit](https://www.mathworks.com/help/matlab/apiref/mexatexit.html).

## 6. loadlibrary and future callback extensions

MATLAB `loadlibrary` / `calllib` can work with C shared libraries, but `loadlibrary` does not support function-pointer input arguments. The project's complete header also contains callback definitions, so this guide uses a compiled MEX to avoid requiring handwritten parseable headers and pointer marshaling. This does not mean every C interface is unusable through calllib. See the restrictions in [MathWorks loadlibrary documentation](https://www.mathworks.com/help/matlab/ref/loadlibrary.html).

This example supports only GetResponse, without Gather/geterrors interfaces. In future extensions, native callbacks should only copy data into a bounded queue, polled actively by MATLAB calls to the MEX. Overflow policy, stop barriers, and release ordering are also required. Do not call mx/mex APIs or mexPrintf directly from native callback threads; see [MEX/Matrix API thread restrictions](https://www.mathworks.com/help/matlab/matlab_external/mex-api-is-not-thread-safe.html).

After ordinary requests work, see the dedicated C/C++ interfaces for [Gather](gather.en.md) and [geterrors](geterrors.en.md). geterrors listens continuously until stopped with Ctrl+C, supports detection periods of 0.01–1.0 seconds, and uses system defaults when masks are unspecified.

GetResponse also supports an optional passthrough request setting, disabled by default, to send one command verbatim while bypassing online/program classification. For language syntax, single-frame limits, and error semantics, see [passthrough mode](getresponse-examples.en.md#optional-passthrough-mode-bypass-onlineprogram-rules).
