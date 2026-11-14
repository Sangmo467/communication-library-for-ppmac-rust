# GetResponse passthrough mode

[English](getresponse-passthrough.en.md) | [简体中文](getresponse-passthrough.md)

GetResponse/command defaults to passthrough, bypassing online/program classification and local program-context checks while forwarding each line unchanged. Disable it explicitly for rule validation: C uses PMAC_REQUEST_CLASSIFY (8), C++ uses passthrough=false, C# uses Passthrough:false, Python uses passthrough=False, and MATLAB uses struct('passthrough', false). Read-pool requests automatically enable classification; explicit C PASSTHROUGH (4) cannot be combined with READ_POOL or CLASSIFY. Basic framing checks and remote error # reporting always apply.

Applies to C/C++, C#, Python, and MATLAB. Setting true affects only the current request; subsequent requests default to passthrough unless explicitly disabled. For connection steps, see the [GetResponse basic guide](getresponse-examples.en.md).

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

## Complete example: submit a program line by line on one thread

The following code assumes an already connected Client. These commands actually write to a program buffer; choose the program number and body for your application. Each request must succeed before the next is sent. Stop on an exception, without automatically resending or sending close.

```cpp
pmac::RequestOptions options;
options.passthrough = true;
client.getResponse("open prog 100", "", options);
client.getResponse("dwell 10", "", options);
client.getResponse("close", "", options);
```

C uses the existing asynchronous request/wait flow; simply set the flag before submission:

```c
pmac_request_options options = {0};
options.struct_size = sizeof(options);
options.abi_version = PMAC_ABI_VERSION;
options.timeout_ms = 5000;
options.max_response_bytes = 1024 * 1024;
options.flags = PMAC_REQUEST_PASSTHROUGH;
pmac_request_id request = 0;
int32_t status = pmac_get_response_async(client,
    (const uint8_t*)"dwell 10", 8, &options, &request);
/* Check submission status, wait for request, check the execution result,
   copy the response, then release request. See examples/c/getresponse.c
   for complete waiting, error-copying, and cleanup code. */
```

## Choosing default or passthrough mode

Explicit classification mode applies command rules and maintains local program context; passthrough is the default. Passthrough is intended for applications that manage controller state themselves or send device commands/program body lines not yet covered by the rules. The device still interprets and validates commands against its actual state; local permission to submit does not guarantee successful device execution.

To disable passthrough, explicitly set it to false, or use PMAC_REQUEST_CLASSIFY (8) in C. Omitting the option retains passthrough. Do not switch modes repeatedly within an unfinished program operation: passthrough does not update the local open/close state used by classification mode.

## Common questions

- Unknown flags: check whether the loaded DLL/so contains this feature; older native libraries may not support flags 4 or 8.
- Passthrough still returns error #: this is a device error whose original text the application must handle; bypassing local rules does not bypass device checks.
- read_pool requires read-only classification. Explicit C PASSTHROUGH conflicts with READ_POOL; language bindings automatically select classification.
- Multiline string rejected: submit one command without CR/LF at a time and wait for each result.
- Extraction fails after `CONFIRMED`: the command may already have executed; extraction failure is not grounds for resending a write command.
