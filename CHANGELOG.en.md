# Changelog

[中文](CHANGELOG.md) | [English](CHANGELOG.en.md)

This project follows semantic versioning. The public C ABI major/minor versions are recorded independently; existing input structure layouts remain compatible within ABI major 2.

## 2.0.10

- Explicitly request a PTY for GPASCII channels so the controller merges and flushes diagnostics before ACK. Invalid commands now return their `stdin`-numbered error and `0` in the same synchronous response without delays or extra queries. Initialization fails if PTY is unavailable rather than falling back to non-PTY stream mixing.
- Retain `stdin`-line isolation for asynchronous stderr. Add PTY-rejection, error-body and large-response regressions. Verify 250 invalid/valid polling rounds, a 1000-line response and 10 reconnections on the controller. C ABI remains 2.1.
- Record a targeted `RUSTSEC-2026-0543` exception: chrono is only a transitive russh-sftp timestamp dependency, the notice is informational rather than a vulnerability, and no patched upgrade exists. All other security checks remain enabled.

## 2.0.9

- Attribute asynchronous GPASCII stderr by each command's `stdin` line number. Late diagnostics for completed commands are logged rather than written into a later valid request's `stderr` field.
- Preserve immediate ACK completion without added waits. Add mixed-line and delayed-pollution regressions plus 250 rounds of real-controller verification. C ABI remains 2.1.

## 2.0.8

- Complete GPASCII requests immediately on ACK without an artificial quiet-period delay. NAK or an `error #N` in stdout still fails synchronously; stderr is now asynchronous diagnostic output and no longer rejects a later valid response.
- Retain asynchronous stderr under an independent limit, dropping excess bytes with a warning instead of consuming the stdout response budget or failing later requests and retiring their connections.
- Remote errors now distinguish the original command from the response body. Add delayed-diagnostic, passthrough and real-controller polling regressions. C ABI remains 2.1.

## 2.0.7

- Include previously omitted reset recovery: keep healthy SSH transports after confirmed $$$/$$$***, close stale GPASCII channels and reopen them on demand on the same connections. The same Client can query again; unknown resets are never replayed.
- Use a separate bounded recovery budget. Omitted command options select 30 seconds for standalone resets across bindings; explicit budgets are preserved.
- Preserve session ownership and recoverable pool state if a custom session close panics during repair or drain.
- Validate Python shutdown/close wait budgets before narrowing to native u32.
- Accept CRLF at the exact geterrors payload line limit. Add panic, fragmentation and SSH reuse regressions. C ABI remains 2.1.
## 2.0.6

- Preserve reset framing and maintenance barriers for standalone $$$/$$$*** in passthrough mode, avoiding ordinary ACK waits for reset responses.
- Filter STDIN Open for ASCII Input from command responses while retaining internal readiness verification.
- Clear stale release uncertainty only when the original session confirms retirement or closure; explicit pool repair restores querying after reset.
- Add reset framing, passthrough and release-confirmation regressions and document timeout/recovery requirements. C ABI remains 2.1.

## 2.0.5

- Fix C#/Python native cleanup after GC, failed C++ Client move assignment, pool initialization rollback and release confirmation.
- Enforce stream queue payload limits, verify remote process cleanup, and fix long-path upload diagnostics, stale business errors and Shell output limits.
- Optimize fragmented GPASCII parsing, selected regex captures, request completion cleanup and stream allocations; prune stale DART budget entries.
- C++ Client move assignment may now throw cleanup errors. C ABI remains 2.1.

## 2.0.4

- Fix continuous Gather delivery with lowercase gather -w and a session-private FIFO that keeps stdin open without a PTY. Configure acquisition, start the reader, then set Gather.Enable=3.
- Clean up the FIFO and session lock on stop; update the native numeric version to 20004. C ABI remains 2.1.

## 2.0.3

- Derive the native version string from Cargo and add a C-export version consistency regression test.

- Default GetResponse/command to passthrough; explicit opt-out enables online/program validation and read-pool requests classify automatically. Add PMAC_REQUEST_CLASSIFY (8), retaining C ABI 2.1.
- Check 66 case-insensitive structure roots against the software reference and independent DART/Modbus specifications. Delegate subsequent members and multidimensional indices to the controller; custom arrays use parentheses.
- Support Modbus Client/Server online commands and verify exact-wire upload of the motion program and 16 jog/jogret body forms, with bilingual catalogs and guides.

## 2.0.2

- Add a LoongArch64 Linux SDK with LP64D new-world ABI and glibc 2.36 baseline checks.
- Isolate the cross toolchain and execute QEMU Rust/C/C++ tests, 149-value ABI comparisons and target-host preflight checks.
- Add English/Chinese Kylin and Debian integration guides with explicit ABI/runtime compatibility conditions; C ABI remains 2.1.

## 2.0.1

- Add Linux ARM64/ARMv7, Windows ARM64 and macOS Intel/Apple Silicon build/release paths alongside the existing four targets.
- Centralize targets and native-library names; verify PE, ELF and Mach-O architecture plus C/Rust ABI parity on each target.
- Provide full English public user documentation and bilingual navigation, retaining Chinese comments in buildable examples.
- Keep C ABI 2.1; ship separate x64 and ARM64 macOS dylib packages.

## Unreleased

- Default GetResponse/command to passthrough; explicit opt-out enables online/program rules, while read-pool requests classify automatically. Add PMAC_REQUEST_CLASSIFY (8) without changing request layouts.
- Check 66 case-insensitive structure roots by prefix and boundary only; custom arrays use parentheses.
- Support Modbus Client/Server online commands and add exact-wire motion and jog/jogret buffer tests plus bilingual catalogs.

- Fixed lost request-extraction diagnostics and incorrect execution-certainty reporting in bindings, inability to retry after a custom Session close panic, and lingering cached late SSH Close after cancellation of a wait; reduced repeated allocations in Gather line parsing; added Rust 1.100 documentation-table checks to CI.

- Added optional GetResponse passthrough mode: sends a single GPASCII command unchanged, bypassing the rule table, online/program classification, and local open checks. Available in C/C++, C#, Python, and MATLAB; enabled by default with an explicit opt-out, preserving error reporting.

- Fixed repository-metadata inheritance in five crates following new Cargo manifest diagnostics in Rust 1.100.0; documented upgrade review and Windows NASM build requirements.

- Upgraded the build toolchain and CI to Rust 1.100.0; the minimum supported version remains Rust 1.99.

- Added runnable C/C++, C#, Python, and MATLAB examples focused on GetResponse, with detailed integration documentation; added a synchronous C++17 MEX gateway for MATLAB. Gather/geterrors topics follow basic requests.

- Changed each Client's GPASCII channel limit to 16, retaining occupancy with uncertain release. Standard error # diagnostics are reported as remote errors even with ACK. Error #31 invalidates old program contexts without hidden resends or compensating close commands.

- Added geterrors C ABI and C++17 real-time status-change monitoring: a dedicated SSH PTY, optional date and masks, classified line callbacks, bounded queues, and Ctrl+C stop/release barriers. See the [guide](docs/geterrors.en.md).

- Added C ABI and C++17 wrappers for real-time continuous Gather, supporting addresses, sample periods, listening ports, bounded batch callbacks, and stop/release barriers.
- Gather admits reverse TCP connections using the controller source IP and a per-session random handshake; incorrect or silent connections share the startup deadline. Data remains plaintext TCP; see the [detailed guide](docs/gather.en.md) for deployment boundaries.
- Fixed compound `close all buffers` failing to invalidate local program contexts.
- Python validates integer ranges in request and connection configurations before ctypes conversion, rejecting silent truncation.

## 2.0.0 - 2026-11-12

First standalone Rust + russh release, C ABI 2.1.

### Added

- Incremental GPASCII/Shell protocol handling, strict numeric parsing, status bits, and byte-regex extraction.
- Runtime/Client/Request opaque IDs, synchronous and asynchronous requests, cancellation, deadlines, and completion broadcast.
- Explicit SSH connection pools, strict calling-thread–GPASCII Channel affinity, maintenance barriers, and member-level fault isolation.
- Password and private-key authentication, known_hosts, and SHA-256 host-fingerprint verification.
- Persistent Shell, isolated exec, SFTP file transfer, and program-buffer upload.
- C ABI, C++17/C++98, .NET 8, and Python >=3.9 bindings.
- Common motor, coordinate-system, and PLC commands and status queries.
- MIT license, third-party license notices, build/installation documentation, and complete user manuals.

### Compatibility

- Uses the `pmac_` prefix and is incompatible with the old 1.x API/ABI.
- Exact empty, `number`, and `variable` GetResponse patterns preserve legacy extraction semantics; other patterns execute as byte regexes. See the [pattern guide](docs/pattern-migration.en.md).
- Writes that may already have executed are not automatically retried; applications must recover according to execution certainty and device state.
