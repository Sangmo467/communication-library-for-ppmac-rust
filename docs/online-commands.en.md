# Online Commands and Program-body Passthrough

[中文](online-commands.md) | [English](online-commands.en.md)

The command catalog follows the Online Command Specification in `Power_PMAC_Software_Reference_Manual` (manual pages 1119–1319). See [online-command-rules.tsv](../crates/pmac-protocol/src/online-command-rules.tsv) for catalog rules, including syntax families, scope, read/write/maintenance classification, and representative examples. Rules cover the manual's Online command families; the controller ultimately determines parameter validity and device resource state.

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

Structure members use general syntax rather than a fixed structure-name prefix allowlist. `Sys`, `EncTable[n]`, `Motor[n]`, `ECAT[n]`, `Gate1/2/3[n]`, `Ldata`, `Macro`, `Eip[n]`, `Coord[n]`, `BufIO[n]`, `Gather`, `Tdata[n]`, `UserAlgo`, and others may all be followed by complete member chains and array indices. `Sys.` and `Motor[n].` illustrate structure prefixes; they are not complete member queries themselves.

Valid user-declared scalar and array names may also be sent directly, such as `MyVariable`, `MyArray[3]`, `MyArray[P1]`, and corresponding assignments. Standalone-name queries whose declarations have not been locally verified are conservatively routed as control requests without rewriting the text; this does not turn a query into an assignment. Names containing `Jog`, `Kill`, or `System` are not blocked merely by substring matching.

Complete numeric-index member queries and `.a` address queries are classified as read-only; assignments, compound assignments, and increment/decrement operations are writes. Dynamic-index member access is also allowed, but is classified as a control write and excluded from the read-only pool because expressions may have side effects. The relevant controller firmware determines whether fields exist, are writable, and accept particular ranges. Local syntax acceptance does not mean every field supports writes.

## Program Commands

`GetResponse` / `pmac_command_submit` automatically use the calling thread's confirmed program-buffer context without new parameters:

```python
client.get_response("open prog 100")  # Wait for successful confirmation
client.get_response("dwell 10")       # Program body, same calling thread
client.get_response("close")          # Submit separately and wait for confirmation
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

The library returns any standard `error #<number>` diagnostic as `REMOTE_ERROR`, even if the controller ends the response with ACK rather than NAK. Received raw stdout/stderr diagnostics are preserved in error details. Synchronous C callers must immediately read them with `pmac_copy_last_error`; asynchronous requests use `pmac_request_copy_error` and retain the Request ID until copying completes. C++/Python/.NET bindings carry diagnostics in their errors/exceptions. `CONFIRMED` only means that the full protocol response was received, not that statement execution succeeded.

After error #31, the library invalidates the affected old program context and retires its member. It neither automatically sends a compensating `close` nor resends the original statement, and never rewrites recovery or cleanup results as original-request success. Applications must read the original error, then repair through the pool-recovery API or reconnect, and explicitly reopen required program buffers after confirming device state. A member may be shared by multiple calling threads; those threads observe the unusable old epoch and must coordinate recovery. Other independent Clients are not proactively closed by the library, but the global command's controller-side effects still exist.

When the same Client issues `close all buffers` through the library, known local program contexts are cleared after the maintenance barrier. Old bodies may be rejected during local admission, while valid Online commands may still receive a delayed remote-channel error #31, which is likewise returned to the application. Each Client has a total quota of 16 live GPASCII channels, including prebuilt channels and occupancy with uncertain release. New threads or pool repair must not bypass counts whose release has not been confirmed.

GetResponse/command may use PMAC_REQUEST_PASSTHROUGH (4) to skip online/program classification and local program-context checks. It is disabled by default and cannot be combined with READ_POOL. Single-frame validation still applies, and remote error # diagnostics continue to be reported unchanged. See [Passthrough mode](getresponse-examples.en.md#optional-passthrough-mode-bypass-onlineprogram-rules).
