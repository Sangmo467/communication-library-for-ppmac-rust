# Changelog

[中文](CHANGELOG.md) | [English](CHANGELOG.en.md)

This project follows semantic versioning. The public C ABI major/minor versions are recorded independently; existing input structure layouts remain compatible within ABI major 2.

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
