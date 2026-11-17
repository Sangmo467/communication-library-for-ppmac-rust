# C ABI Contract Summary

[简体中文](abi-contract.md)

Updated 2026-11-12. [include/pmac.h](../include/pmac.h) is authoritative for public definitions. All exports use `extern "C"` and the `pmac_` prefix; Windows x86 uses `__cdecl`.

## Versions and structures

- The library version is 2.0.9, with ABI major/minor **2.1**; `pmac_abi_version()` returns **2001**.
- Existing input structure layouts remain unchanged. ABI major 2 inputs, including version value 2000, are accepted; new pool/notification interfaces require the corresponding library exports.
- Public aggregates use `repr(C)` with default C layout; pack(1) is prohibited. `size_t` varies with target bitness; x64 layout checks do not replace 32-bit verification.
- Input structures carry `struct_size` and `abi_version`. The common 8-byte header is read first to check minimum size and ABI major, then the complete input structure is copied. Callers must allocate full output structures using the current public types.
- `pmac_get_runtime_info` returns the ABI, library version, feature mask, and native pointer width. Check ABI major and pointer width; do not mix headers and dynamic libraries of different architectures.

## IDs, closing, and release

The high 4 bits of Runtime, Client, and Request IDs are type tags; the low bits are sequence numbers that never wrap and are never reused. Released IDs remain invalid permanently. Internal Arc references keep acquired objects alive; C calls do not dereference object addresses.

- `pmac_client_close(id, 0, wait_ms)` stops admission to that Client, cancels admitted requests, and performs cleanup. Only close_mode=0 is currently supported.
- `pmac_client_release` removes the ID only after Client close has completed and all GPASCII resources owned by that owner, including resources in use, being reclaimed, and of uncertain release status, have been returned. Otherwise it returns `SHUTDOWN_PENDING` and retains the ID. After verifying the device, call `pmac_pool_confirm_recovery`, then retry release.
- `pmac_client_recovery_ids` enumerates Client IDs that are closed/closing and still have resources outstanding. Even after a C++ destructor transfers ownership, this retrieves retained IDs for restricted recovery based on device verification.
- `pmac_runtime_shutdown` stops admission of new Clients and Requests to that Runtime, closes existing Clients, and waits under supervised cleanup for Tokio and blocking tasks to stop.
- A shutdown timeout returns `SHUTDOWN_PENDING`. The Runtime ID remains valid and may be waited on again. A timeout does not mean background tasks have exited.
- After successful shutdown, local state/completion results remain readable; callers must still release all Client/Request IDs belonging to that Runtime.
- `pmac_runtime_release` succeeds only after shutdown has completed, all Clients are closed, and all child IDs have been released. Otherwise it returns `SHUTDOWN_PENDING` and retains the ID.
- `pmac_runtime_cleanup` starts a one-time cleanup supervisor and returns immediately. The supervisor holds the Runtime and completes release after background work stops and child IDs are subsequently released. C++ destructors use this entry point instead of waiting indefinitely on the calling thread.
- `pmac_runtime_cleanup_wait` joins scheduled cleanup supervisor threads. Before dynamically unloading a DLL/so, stop admission of new Runtimes/cleanup work and call this interface to wait for all supervisor threads to exit. Merely deleting Runtime IDs is not an unload barrier.
- SDK Dispose/shutdown also retains pending Runtime IDs. Release request results/Clients still held, then retry; GC/finalizers do not replace explicit completion of shutdown.

Typical C cleanup order: shutdown and wait for success → read needed terminal states/results → release Requests/Clients → release Runtime. Do not submit new business operations after closing. Complete these steps before unloading the DLL/so and ensure that no calling thread is still executing a C entry point.

## Requests, waiting, and results

`pmac_get_response_async`, `pmac_command_submit`, and other submit functions share the same request core. Successful submission returns a Request ID; failed admission does not return a pending request. `QUEUE_FULL` is an admission rejection: do not treat the timeout of an admitted operation as a queue-full condition that permits resubmission.

- The total execution deadline is established during submission and includes queueing. The wait_ms argument to `pmac_request_wait` limits only that wait. `WAIT_PENDING` neither cancels the request nor changes its execution state.
- `PMAC_REQUEST_INFINITE` removes only the total execution deadline; cancel/close remain available.
- Requests own copies of commands/paths/configuration and retain their own results. Late events cannot overwrite a completed result; multiple threads may copy it into their own buffers.
- Copying responses, stderr, errors, or regex captures sends no new command. A small buffer returns `BUFFER_TOO_SMALL` and the required byte count. Retrieve again using the same Request ID.
- Nonempty output requires a valid buffer; an empty byte result permits null output. `required` must be non-null. Results are length-delimited and need not end in NUL.
- Text input is length-delimited UTF-8 without NUL; raw responses may contain arbitrary bytes.
- Exactly empty, `number`, and `variable` patterns retain legacy helper extraction. Other patterns use `regex::bytes` and return real captures/syntax errors. Older libraries returned the raw response directly for other patterns; see [Pattern migration](pattern-migration.en.md) for the explicit new API semantics.
- Synchronous APIs wait on the same request core; they do not maintain a second SSH or parsing implementation.
- After a synchronous business call fails, call `pmac_copy_last_error` on the same thread, before any other PMAC entry point, to copy error details. This interface does not itself clear the details and supports querying the required length before copying the message. Details include execution certainty, connection epoch, and a diagnostic request ID. The synchronous call has already released that ID, so it cannot be used to query the request.
- A new PMAC call returning a status replaces the previous error on that thread; threads do not affect one another. If synchronous error details are unavailable, do not infer that the command was not sent.

Cancellation/timeout while queued guarantees no send. Send initiation and cancellation are arbitrated by the same request state. Terminal state and execution certainty are published together; insufficient local copy buffer space does not alter the facts of remote completion.

## Completion broadcast

`pmac_runtime_wait_completion(runtime, cursor, wait_ms)` returns `OK` and updates cursor when the Runtime completion sequence changes. Exhausting the wait budget returns `WAIT_PENDING` without changing any request state. Each waiter uses its own uint64_t cursor; do not write the same cursor concurrently.

The sequence broadcasts wakeups instead of consuming a result queue; one waiter cannot take another waiter's notification. `pmac_runtime_wake` also advances the sequence, so after waking, check your own tracked requests rather than interpreting a notification as success of a particular request. Scan requests before waiting on the sequence; completions arriving between poll and wait remain observable.

.NET/Python use one bounded completion pump per Runtime. Tracking a new request and exiting actively wake it; normal completion is signaled by the native sequence. Completion object capacity and result memory are bounded. Results remain until release, so slow consumers cannot cause unlimited growth.

## Pool management, routing, and fault recovery

The pool contains fixed, ordered control member 1 and extra members explicitly created by the application. Each member is an independent SSH/GPASCII session.
Each Client has a budget of **16** online GPASCII channels, covering precreated, opening, in-use, reclaiming, and release-uncertain occupancy across all of its members. Quotas are isolated by a process-unique owner; multiple Clients on the same controller count separately, and the library imposes no combined cross-Client limit of 16. A 17th allocation is rejected with `RESOURCE_LIMIT`, `NOT_SENT`. Quota is returned only after confirmed remote release or explicit recovery confirmation by the owning owner. Unknown release cannot return quota early, nor can confirmation by another Client offset it. The BufferUpload maintenance path does not consume online channel quota.

| Entry point | Contract |
|---|---|
| `pmac_pool_prepare_async` | Initially supplies connection configuration and profile; returns a management Request without blocking healthy data paths |
| `pmac_pool_resize_async` | Explicitly adjusts target/max/min_ready; retains the configured target; failures/partial availability are reported through state and request errors |
| `pmac_pool_repair_async` | Repairs specified failed members; member_id=0 selects members needing repair within the current target range; healthy members are not rebuilt |
| `pmac_pool_drain_async` | Stops new dispatch to selected members and cleans up; member_id=0 selects extra read members; fixed control member 1 cannot be drained |
| `pmac_pool_repair_maintenance_async` | Explicitly repairs the maintenance plane; ordinary requests do not reconnect it implicitly |
| `pmac_pool_get_state` | revision, desired, ready/busy/creating/failed/retiring, release_uncertain, recovery gates, and last creation error |
| `pmac_pool_get_members` | Per-member ID, epoch, state, control role, and last valid response time; capacity/required are measured in members |
| `pmac_request_get_route` | Returns the assigned member_id and connection epoch; member ID=0 means unassigned |
| `pmac_pool_confirm_recovery` | Accepts application confirmation only in a quiescent state matching expected_revision; confirmation flags must be nonzero |

Profiles require `1 <= min_ready <= target_size <= max_members <= 255`, and a nonzero connection budget. The total creation/wait budget is separate from ordinary operation deadlines. Management requests establish a new revision; stale tasks cannot install old members or overwrite new targets. Superseded management operations report `SUPERSEDED`.

The first time each submitting thread uses GPASCII, it is pinned to a healthy member and exclusively owns a persistent Channel there. Subsequent ordinary commands and `PMAC_REQUEST_READ_POOL` requests from that thread do not migrate. `PMAC_REQUEST_READ_POOL` only relaxes admission for commands classified and confirmed as read-only; commands with side effects are rejected before sending. Threads do not share GPASCII Channels. There is no implicit read-after-write ordering or simultaneous sampling guarantee across threads/members. Submit `open prog` through `close` from the same thread.

The maintenance barrier waits for the boundary of admitted work. Pending/running maintenance blocks admission of related new operations with `MAINTENANCE_ACTIVE`. A control failure retires only the affected member; a maintenance failure isolates the maintenance plane. Default recovery is manual application repair: traffic/failures do not automatically expand the pool, reconnect, or replay.

Writes with `MAY_HAVE_EXECUTED` retain a recovery gate; subsequent conflicting operations report `RECOVERY_REQUIRED`. Set `PMAC_RECOVERY_CONFIRM_UNKNOWN_WRITES` only after the application checks device state. `PMAC_RECOVERY_CONFIRM_REMOTE_RELEASE` is a separate application declaration that uncertain remote sessions have been released, used to return quota; closing a local socket is not proof. Normal Session closure waits for remote Close on each Channel first. If uncertain records remain, a Closed Client may confirm its owner's old channels while its own ID remains unreleased, but cannot confirm another Client's records.

Member state values: Ready=0, Busy=1, Failed=2, Retiring=3, ReleaseUncertain=4, Creating=5, Drained=6. Do not automatically clear unknown write results merely because ready_count recovers.

## Execution certainty and identity

- `NOT_SENT`: provably not sent.
- `MAY_HAVE_EXECUTED`: sending has begun or confirmation was lost; the remote action may have occurred. The caller must check device state.
- `CONFIRMED`: complete, valid protocol confirmation was obtained; it does not mean mechanical motion or an application program has finished.

`PUBLISH_UNKNOWN` means file publication has an uncertain result. The library does not automatically retry operations that may have side effects, and a complete ACK does not replace mechanical state checks.

Host identity is independent of login method. Choose known_hosts, a 32-byte SHA-256 fingerprint, or explicit `PMAC_CONNECT_ACCEPT_ANY_HOST_KEY`; use the latter only where the caller explicitly accepts the risk. Passwords/private keys/passphrases become library-owned configuration on asynchronous submission and are not included in error text or test reports. Configuring both a private key and password does not enable automatic password fallback.

- C++ headers default to compatibility with older ABI 2.1 libraries lacking the pmac_copy_last_error export, using conservative MayHaveExecuted on failure. After confirming that the linked library provides the export, define PMAC_ENABLE_LAST_ERROR_DETAILS before including the header to enable complete error details; all translation units must use this option consistently.

GetResponse/command defaults to passthrough, bypassing online/program classification and local program-context checks while forwarding each line unchanged. Disable it explicitly for rule validation: C uses PMAC_REQUEST_CLASSIFY (8), C++ uses passthrough=false, C# uses Passthrough:false, Python uses passthrough=False, and MATLAB uses struct('passthrough', false). Read-pool requests automatically enable classification; explicit C PASSTHROUGH (4) cannot be combined with READ_POOL or CLASSIFY. Basic framing checks and remote error # reporting always apply.
