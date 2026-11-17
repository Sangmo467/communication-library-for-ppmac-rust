# Online Commands and Program-body Passthrough

[中文](online-commands.md) | [English](online-commands.en.md)

The command catalog follows the Online Command Specification in `Power_PMAC_Software_Reference_Manual` (manual pages 1119–1319). See [online-command-rules.tsv](../crates/pmac-protocol/src/online-command-rules.tsv) for catalog rules, including syntax families, scope, read/write/maintenance classification, and representative examples. Rules cover the manual's Online command families; the controller ultimately determines parameter validity and device resource state.

The local classification and program-context rules on this page apply when passthrough is explicitly disabled. Default passthrough retains framing checks; Rust Command::raw remains an explicit rule parser.

## Online Commands

- All commands are case-insensitive; the library preserves the original text when sending.
- Motor-specific commands require an explicit `#` motor target in the same command; coordinate-system-specific commands require an explicit `&` coordinate-system target. Valid single numbers, lists, ranges, and `*` are supported.
- Selecting a default target with `#1` or `&1` first does not let subsequent bare target-dependent commands bypass these checks. For example, `kill` must still be `#1kill`, and `abort` must still be `&1abort`.
- The manual defines `k/kill` as motor commands, so `&1k` and `&1kill` are rejected; use `#1k`, `#1kill`, or the corresponding motor-list forms.
- At user request, `&nresume` (including lists, ranges, and `&*resume`) is additionally supported as a coordinate-system control-write compatibility command. The rule table explicitly marks this extension rather than presenting it as a manual Online entry.
- `cpx` and `cx` require an explicit coordinate system. Their following single-line program body is preserved, including spaces, case, quotes, and semicolons. The entire request is a control write; variables in the body are not analyzed to reduce its classification.
- Following the manual, `start` accepts an optional nonnegative program constant, such as `&1start`, `&1start5`, or `&1start 5`. Use `cpx/cx` or a program buffer for arbitrary bodies.

Read-only and write requests are distinguished using complete syntax. Examples: `&1%` queries while `&1%100` writes; `&1Q1` queries while `&1Q1=17.5` writes; `M0->` queries a definition while `M0->Sys.ServoCount` changes it. `rotfree/rotfreeall`, `backup`, and `ecat slaves` are queries. `macro…read` copies data to local variables and cannot be treated as read-only. Complex dynamic indexing is classified as a control write when absence of side effects cannot be established.

Only verified read-only syntax may set `read_pool`; control writes and maintenance commands cannot enter through the read-only pool. Normal calling-thread and GPASCII Channel affinity rules still apply.

## Data-structure variables

Root names and delimiter boundaries are checked against the manual catalog, case-insensitively. Subsequent members and multidimensional indices are not checked against an allowlist. See the [structure catalog](data-structures.en.md) for roots and manual pages. Examples include Motor[1].ActPos, CompTable[0].Data[1][2][3] and BrickAC.Chan[0].I2tWarnOnly. The controller validates fields and index ranges.

Custom variables support StimulusType, StimulusType(11), MotorState_Mtr(11) and assignments. Custom array queries must not use StimulusType[11]. Original text is preserved; custom-name queries conservatively use Control scheduling and cannot enter the read pool. Only complete, statically recognized numeric structure-member queries are ReadOnly.

## Modbus online commands

Client and Server online commands from Power PMAC On-Line Modbus Command Specification 2014-01.pdf are supported; the document title page is marked JULY-2015. These include ModbusAscii, Close/CloseAll/CloseThread, Coil/CoilRead/CoilWrite, Connect, Discrete/DiscreteRead, ExceptionCode, Float/FloatRead/FloatWrite, Holding/HoldingRead, LinuxError, Ping, Query, Register/RegisterRead/RegisterWrite, Response, SocketError and State, plus ModbusServerLinuxError/Query/Response/State. All shortened client names in this list carry the Modbus prefix.

Names are case-insensitive; explicit socket numbers are 0–7. The controller validates remaining parameters. The remaining single-line ModbusAscii payload is preserved, including semicolons. Structure access such as Modbus[0].Config.ServerIPAddr[3], ModbusServer[0].State and Sys.ModbusServerBuffer[0] is also supported. These are PMAC Modbus commands sent over the existing GPASCII transport; this does not add a host-side Modbus TCP transport API.

Read commands such as ModbusRegisterRead write local PMAC variables, and CloseThread has communication-thread semantics. All Modbus online commands therefore use Control scheduling on the control member and cannot set read_pool. Use the same Client and calling thread for related socket commands.

Example: submit ModbusConnect 0, then ModbusRegisterRead 0,10,2,P100, then ModbusServerState, waiting for each response.

## Program Commands

`GetResponse` / `pmac_command_submit` automatically use the calling thread's confirmed program-buffer context without new parameters:

```python
client.get_response("open prog 100", passthrough=False)  # Wait for successful confirmation
client.get_response("dwell 10", passthrough=False)       # Program body, same calling thread
client.get_response("close", passthrough=False)          # Submit separately and wait for confirmation
```

The application chooses the program number. Each call transfers one line under the existing command-length limit; NUL, CR/LF, and ACK/NAK control bytes remain forbidden in command text.

- Program-body passthrough begins only after successful confirmation of `open prog/plc/subprog` or `open forward/inverse/rotary` with an explicit coordinate system.
- Bodies are not subject to the Online action allowlist and are always treated as writes. `system`, `pread`, `dread`, and other Program Commands may pass through in this context for controller compilation/interpretation; this does not make them standalone Online commands.
- Context is bound to the original calling thread, member, and connection epoch. Other threads cannot inherit it; disconnection, member repair, or connection reestablishment invalidates the old context.
- Submit `close` separately. Once closing is admitted, subsequent bodies are no longer accepted, preventing bare program commands from executing as Online commands after the program buffer has been exited.
- A body execution error invalidates the local program context and retires the corresponding member. The library does not automatically resend program lines. Applications must choose recovery according to error certainty and device state.
- Global `close all buffers` uses a maintenance barrier and clears all program contexts for this Client.

Direct Rust callers use `Client::parse_command_for_current_thread(text)` before submitting `Operation::Command`. `Command::raw` only parses Online Commands; `Command::program_line` constructs a marked body, but Client still verifies valid buffer context before admission and sending.

## Regression coverage

The [command corpus](../crates/pmac-protocol/tests/online-command-corpus.tsv) and rule-table examples cover case handling, read/write classification, explicit scope, and original-text preservation. FFI tests use a controlled Transport to verify normal admission and read-only-pool rejection of writes. Core and local SSH tests cover same-thread bodies, cross-thread isolation, queued closing, cancellation, repair, and global buffer closing.

These checks establish local acceptance, classification, and routing behavior; they do not establish that motion, resets, or configuration changes have been executed individually on real controllers.

## Cross-Client global closing and controller errors

`close all buffers` affects buffers opened by other Clients or external programs. Even if another GPASCII channel locally still records `open prog`, its next submission may receive:

```text
stdin:1:1: error #31: Buffer closed due to another thread:
```

GPASCII channel initialization requires a PTY so the controller merges diagnostics into stdout and flushes errors before the ACK response. If the PTY request fails, initialization fails rather than falling back to a non-PTY mode with delayed stderr. The GPASCII stdout response frame determines the synchronous result: NAK or a standard `error #<number>` diagnostic in stdout returns `REMOTE_ERROR`. ACK completes the request immediately without waiting for or being reclassified by SSH ExtendedData (stderr). Stderr is asynchronous diagnostic output. Only diagnostics received before request completion with a `stdin` line number matching the current command are retained in that request's `stderr` field. Late diagnostics for earlier commands are logged as warnings, never cached or attached to later requests; unnumbered diagnostics that cannot be attributed are also only logged. Diagnostics have an independent retention limit, with excess bytes dropped and a warning logged. Stdout error details retain the original command, response body, and received diagnostics. Synchronous C callers must immediately read them with `pmac_copy_last_error`; asynchronous requests use `pmac_request_copy_error` and retain the Request ID until copying completes. C++/Python/.NET bindings carry diagnostics in their errors/exceptions. `CONFIRMED` only means that the full protocol response was received, not that statement execution succeeded.

After error #31 in stdout, the library invalidates the affected old program context and retires its member. It neither automatically sends a compensating `close` nor resends the original statement, and never rewrites recovery or cleanup results as original-request success. Applications must read the original error, then repair through the pool-recovery API or reconnect, and explicitly reopen required program buffers after confirming device state. A member may be shared by multiple calling threads; those threads observe the unusable old epoch and must coordinate recovery. Other independent Clients are not proactively closed by the library, but the global command's controller-side effects still exist.

When the same Client issues `close all buffers` through the library, known local program contexts are cleared after the maintenance barrier. Old bodies may be rejected during local admission, while valid Online commands may still receive a delayed remote-channel error #31. In stdout this diagnostic returns a synchronous error; in asynchronous stderr it is delivered only as a diagnostic and does not trigger failure or member retirement, so the application must verify program-buffer state separately. Each Client has a total quota of 16 live GPASCII channels, including prebuilt channels and occupancy with uncertain release. New threads or pool repair must not bypass counts whose release has not been confirmed.

GetResponse/command defaults to passthrough, bypassing online/program classification and local program-context checks while forwarding each line unchanged. Disable it explicitly for rule validation: C uses PMAC_REQUEST_CLASSIFY (8), C++ uses passthrough=false, C# uses Passthrough:false, Python uses passthrough=False, and MATLAB uses struct('passthrough', false). Read-pool requests automatically enable classification; explicit C PASSTHROUGH (4) cannot be combined with READ_POOL or CLASSIFY. Basic framing checks and remote error # reporting always apply.

## Program-buffer jog / jogret

After a successful buffer open, jog / jogret bodies are forwarded line by line, preserving lists, ranges, wildcards, P/Q expressions, directions, return positions and trailing semicolons. Examples include jog1..3,5..7:0;, jog=10000^-50;, jogret25..27=*, jog+ 1..5,8 and jog-1,2,3;. These lines are not reclassified as standalone Online commands and require no request-level PASSTHROUGH flag. Use the same Client, calling thread and confirmed buffer. Mock tests verify exact uploaded text and member affinity; the controller determines actual compilation and execution validity.
