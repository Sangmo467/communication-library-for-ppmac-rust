# C# / .NET GetResponse integration

[English](dotnet-getresponse.en.md) | [简体中文](dotnet-getresponse.md)

GetResponse supports per-request passthrough: bypass the rule table and online/program classification and submit one command verbatim. See **[GetResponse passthrough mode and language examples](getresponse-passthrough.en.md)**.

This guide starts with GetResponse: connect to Power PMAC, send a command once, and obtain the complete response or extract text from that same response. The example project is [Pmac.GetResponse.csproj](../examples/dotnet/getresponse/Pmac.GetResponse.csproj), with [Program.cs](../examples/dotnet/getresponse/Program.cs) as its main program. It uses ProjectReference to the repository's existing .NET binding and requires no additional NuGet packages.

## Prepare the environment

You need the .NET 8 SDK (or a newer SDK capable of building net8.0), a native pmac library of the matching architecture, and a verified SSH host identity file. First build from the repository root:

```sh
cargo build -p pmac-ffi --release --locked
dotnet build examples/dotnet/getresponse/Pmac.GetResponse.csproj -c Release
```

The native library is `target/release/pmac.dll` on Windows or `target/release/libpmac.so` on Linux. With Cargo `--target <triple>`, use `target/<triple>/release`. This project builds only the managed example and binding; it does not automatically build, copy, or upgrade the native library.

Set these variables before starting the process. Do not put passwords in source code, command history, or the repository; supply them through the current terminal session or application credential management.

| Environment variable | Purpose |
| --- | --- |
| `PMAC_LIBRARY` | Required native pmac DLL/so file path, preferably absolute; must not point to managed `Pmac.Russh.dll` |
| `PMAC_HOST` | Controller hostname or IP, supplied by the user |
| `PMAC_KNOWN_HOSTS` | Path to a known_hosts file verified through a trusted channel |
| `PMAC_PASSWORD` | Password passed to SSH by the example; the code does not print or save it |

The example uses username `root` and password authentication. For another user, a private key, or a pinned SHA-256 host fingerprint, modify `ConnectOptions` in the example. See [authentication](authentication.en.md) for fields and configuration. Do not accept arbitrary host keys merely to make the example work.

Windows PowerShell, from the repository root:

```powershell
$env:PMAC_LIBRARY = (Resolve-Path 'target/release/pmac.dll').Path
# Supply PMAC_HOST, PMAC_KNOWN_HOSTS, and PMAC_PASSWORD from the existing process environment or trusted configuration.
# Only load the library and check version, ABI, and bitness; do not connect:
dotnet run --project examples/dotnet/getresponse/Pmac.GetResponse.csproj -c Release -- --check
```

Linux terminal:

```sh
export PMAC_LIBRARY="$PWD/target/release/libpmac.so"
# Supply PMAC_HOST, PMAC_KNOWN_HOSTS, and PMAC_PASSWORD from the existing process environment or trusted configuration.
dotnet run --project examples/dotnet/getresponse/Pmac.GetResponse.csproj -c Release -- --check
```

`PMAC_LIBRARY` is read and fixed when the binding first calls the native interface; restart the process to change libraries. The native library and managed process must have the same bitness, with ABI major 2. The binding remains compatible with existing input layouts within ABI major 2; using a DLL/so built from this repository avoids missing functions in older libraries. `BadImageFormatException` usually indicates bitness/format mismatch; `DllNotFoundException` may also mean a dependency failed to load; `EntryPointNotFoundException` indicates a missing export required by the binding. Do not mask ABI mismatches by repeatedly overwriting DLLs with different versions.

## Obtain the full response

After configuring the connection variables, run without arguments to execute the default read-only command `Sys.ServoTime`:

```sh
dotnet run --project examples/dotnet/getresponse/Pmac.GetResponse.csproj -c Release --no-build
```

You can also specify a command. **A user-supplied command is executed verbatim once and may change device state. The example sends no additional motion, write, or test commands and does not automatically retry failures. Confirm the target and command side effects first.** This remains a read-only query:

```sh
dotnet run --project examples/dotnet/getresponse/Pmac.GetResponse.csproj -c Release --no-build -- "Sys.ServoTime"
```

The main call sequence follows; the complete runnable implementation is in Program.cs:

```csharp
using var runtime = new Pmac.Runtime();
using var client = runtime.CreateClient();
using (var request = client.Connect(connection))
{
    request.Wait();
}
byte[] response = client.GetResponse(command, pattern);
Console.Write(System.Text.Encoding.UTF8.GetString(response));
```

`Client.Connect` returns a disposable connection Request; `Wait()` synchronously waits for connection completion before sending a command. `GetResponse` internally submits one request, waits, copies the response, and releases the request. It returns `byte[]` without an additional NUL terminator. The example decodes UTF-8 for display; use the returned array directly for exact byte handling.

The example uses synchronous Main without switching calling threads between connection and commands. It releases the connection Request first, then Client on exit, and finally Runtime. On failure, it records the original operation error and reports cleanup errors separately so device diagnostics are not overwritten.

## pattern: full response, variable, number, and regex

The second command-line argument is an optional pattern. Omitted or empty means the full response. Extraction reuses the completed response without resending the command. Reserved patterns are case-sensitive; see [pattern rules](pattern-migration.en.md).

| pattern | Result |
| --- | --- |
| Omitted / empty string | Complete response bytes; empty responses are allowed |
| `variable` | Skip whitespace after the first equals sign, then extract until the next whitespace or response end; suitable for extracting `3.14` from `P1=3.14` |
| `number` | Extract a numeric fragment starting at the first digit, sign, or decimal-point candidate; returns the `1` in the variable name for `P1=3.14`, unsuitable for general named-variable value extraction |
| Other string | Rust byte regex; synchronous GetResponse returns group 0, the complete match, rather than automatically selecting the first parenthesized group |

Use `number` when the first numeric candidate is known to be the desired value. Do not use it to extract the value after `=` from responses such as `P1=...`:

```sh
dotnet run --project examples/dotnet/getresponse/Pmac.GetResponse.csproj -c Release --no-build -- "Sys.ServoTime" "number"
```

For example, use `variable` for a controller variable response containing an equals sign:

```sh
dotnet run --project examples/dotnet/getresponse/Pmac.GetResponse.csproj -c Release --no-build -- "Sys.ServoTime" "variable"
```

Regex example matching a particular response line:

```sh
dotnet run --project examples/dotnet/getresponse/Pmac.GetResponse.csproj -c Release --no-build -- "Sys.ServoTime" '(?m)^Sys[.]ServoTime=([^\r\n]+)'
```

Although this pattern has parentheses, `GetResponse(command, pattern)` still returns the entire `Sys.ServoTime=...` match. Use an explicit Request for group 1. This is an alternative call sequence: do not call it again after sending the same command to “retrieve” the original response:

```csharp
using var request = client.GetResponseAsync(command);
request.Wait();
byte[] value = request.Match(@"(?m)^Sys[.]ServoTime=([^\r\n]+)", 1);
Console.Write(System.Text.Encoding.UTF8.GetString(value));
```

Regexes run in the native Rust `regex::bytes` engine; do not assume all .NET Regex syntax is supported. Lookaround and backreferences are unsupported. Commands, response formats, and exact matches depend on the actual device. `number` is compatible text extraction, not strict numeric parsing: loose exponent fragments, for example, may not convert to valid floating-point values. Validate further in the application when numbers are required.

## Errors, threads, and shutdown

The example catches `PmacException`, reports the original `Message`, `Status`, `Certainty`, and RequestId, and exits nonzero on failure; loading and configuration exceptions also produce nonzero exits. Do not discard device `error#...` text by printing only “call failed”. NAK or an `error#` in stdout produces a synchronous remote error. ACK completes the request immediately; an `error#` in stderr is only an asynchronous diagnostic and does not reclassify ACK as failure. Only a diagnostic whose `stdin` line number matches the current command enters that request's `Stderr()`; late diagnostics for earlier commands are logged rather than attached to later requests. Following native protocol/remote errors, this example does not print the response as a normal GetResponse result, send an extra `close`, or replay the command.

`Certainty=MayHaveExecuted` means the command may have executed and is not grounds for automatically retrying commands with side effects. Some errors clear local program context and retire the member. The application must verify device state before explicit recovery; retrying the same GetResponse alone cannot restore controller resources or program state.

A Client occupies at most 16 GPASCII online channels in total, including all pool members, prebuilt members, members in use/recycling, and resources with uncertain release. Different Clients have separate quotas; the controller may still refuse new resources. Each submitting thread exclusively owns a fixed channel; `open prog` / program lines / `close` must be submitted on the same thread. Do not let `await`, Task.Run, or UI scheduling switch threads within this sequence. Ordinary single queries can use the synchronous approach shown here.

If closing reports `PMAC_SHUTDOWN_PENDING` or resources still have uncertain release, retain objects/IDs and follow the [user manual](user-manual.en.md) to verify and recover before retrying. A timeout does not mean unloading is safe. This example is a standalone console application; long-lived hosts should integrate objects and recovery into the application lifecycle.

Gather and geterrors are follow-up topics without current C# convenience wrappers. Wrap the C ABI yourself if needed and observe callbacks and stop barriers: [real-time Gather](gather.en.md), [geterrors state-change monitoring](geterrors.en.md).

GetResponse also supports an optional passthrough request setting, enabled by default, to send one command verbatim while bypassing online/program classification. For language syntax, single-frame limits, and error semantics, see [passthrough mode](getresponse-examples.en.md#optional-passthrough-mode-bypass-onlineprogram-rules).
