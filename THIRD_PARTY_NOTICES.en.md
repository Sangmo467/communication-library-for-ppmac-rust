# Third-party Components and Licenses

[中文](THIRD_PARTY_NOTICES.md) | [English](THIRD_PARTY_NOTICES.en.md)

This project itself uses the root-level MIT License. Third-party components retain their own licenses; this project's MIT license does not change their licensing terms.

## Components distributed directly with source

### russh 0.65.1

- Upstream: https://github.com/warp-tech/russh
- Crate: https://crates.io/crates/russh/0.65.1
- License: Apache License 2.0
- Local directory: `vendor/russh/`
- Full license: `vendor/russh/LICENSE`

The local version retains russh copyright and license notices and includes these adaptations:

1. Changes RSA and PKCS1 dependencies to stable versions and adapts to borrowed ASN.1 types and non-deprecated RSA type names;
2. Adjusts the SSH Channel Close lifecycle to retain channel records until peer close acknowledgment is received;
3. Extends known_hosts parsing to preserve revocation markers and reject unsupported security markers and revocation patterns.

The public source retains the upstream source, examples, benchmarks and tests needed for building and verification. Binary SDKs include the native library, bindings, documentation and license notices required by consumers. The complete changes are defined by comparing this directory with upstream 0.65.1.

## Cargo dependencies

Other Rust dependencies are obtained through Cargo, with names, versions, and source checksums locked in `Cargo.lock`. Their licenses are provided by the respective packages. When redistributing binaries containing these dependencies, users should meet the applicable licensing and notice obligations for the actual build outputs.

Common dependencies include Tokio, russh-sftp, regex, thiserror, zeroize, sha2, and md5. The dependency list may change with lockfile updates; refer to `Cargo.lock` in the corresponding release commit.
