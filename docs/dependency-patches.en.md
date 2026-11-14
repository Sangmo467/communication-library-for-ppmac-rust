# Stable RSA Dependency Patch for russh 0.65.1

[中文](dependency-patches.md) | [English](dependency-patches.en.md)

The first actual `cargo audit` on 2026-11-11 found RUSTSEC-2023-0071: the lockfile's `rsa 0.10.0-rc.18` did not satisfy the advisory's fixed range `>=0.10.0`. The official [RustSec advisory](https://rustsec.org/advisories/RUSTSEC-2023-0071.html) describes the Marvin timing side channel and its fix in the stable 0.10 series.

The latest russh on crates.io that day was still 0.65.1, which pinned older RSA/PKCS1/ssh-key prereleases exactly. Running `cargo update -p rsa --precise 0.10.0` alone failed because of the `=0.10.0-rc.18` constraint. This repository retains RSA functionality using a minimal, auditable local adaptation; algorithms, parameter validation, signature operations, and encryption implementations are unchanged.

## Origin and integrity

- Official package: [russh 0.65.1](https://crates.io/crates/russh/0.65.1); archive download URL: `https://crates.io/api/v1/crates/russh/0.65.1/download`.
- `.crate` SHA-256: `94be3d86c71b9ed220a675c405e35c7a02565a009bf8da2e9cc13a700320a5c5`, matching the crates.io checksum in the original Cargo.lock.
- Upstream commit in the package's `.cargo_vcs_info.json`: `b33092aad1e51c66258a12e8b83fc473a68d9d34`, path `russh`.
- Before copying, SHA-256 hashes of all 90 files in the original tar.gz were compared individually against the local cache; all matched. The copy retains LICENSE, source, and original package metadata, excluding Cargo-generated `.cargo-ok` and `.cargo-checksum.json` cache markers.
- `vendor/russh-upstream-files.sha256` records the original 90 files and the LICENSE added later; `vendor/russh-patch.json` records hashes for the archive, RSA patch, and known_hosts parsing patch.

## Complete patch scope

`vendor/russh/Cargo.toml` contains only three exact dependency-version changes:

```diff
 [dependencies.pkcs1]
-version = "=0.8.0-rc.4"
+version = "=0.8.0"
 [dependencies.rsa]
-version = "=0.10.0-rc.18"
+version = "=0.10.0"
 [dependencies.ssh-key]
-version = "=0.7.0-rc.11"
+version = "=0.7.0-rc.12"
```

Stable RSA requires stable PKCS1. Upstream ssh-key rc.12 already uses `rsa ^0.10` and the `pkcs1v15` feature, resolving the incompatibility caused by rc.11 requesting the removed `rsa/sha2` feature. SHA-2 is a mandatory dependency of stable RSA; digest support has not been removed.

`vendor/russh/src/keys/format/pkcs8.rs` includes an adaptation for borrowed ASN.1 types:

```diff
-let sk = &pkcs1::RsaPrivateKey::try_from(pki.private_key)?;
+let sk = &pkcs1::RsaPrivateKeyRef::try_from(pki.private_key)?;
```

Additionally, one occurrence of `rsa::RsaPrivateKey` in `src/keys/format/mod.rs` and two in `src/keys/format/pkcs8.rs` are replaced with the equivalent non-deprecated name `rsa::PrivateKey`; the RSA type and operation semantics remain unchanged.

Stable PKCS1 makes `RsaPrivateKey` generic and provides `RsaPrivateKeyRef<'a> = RsaPrivateKey<UintRef<'a>>`. This change makes the original borrowed-integer representation explicit; other decoding, numeric construction, and validation paths remain unchanged. russh itself remains version 0.65.1 and is not presented as a new upstream version.

| File | Upstream SHA-256 | Patched SHA-256 |
|---|---|---|
| Cargo.toml | `00f0172e07ff5a538feeb9fb37448f2769045060bd8ae799bc2cfee333596ab0` | `1da2a0a51aac0fede21418e4884fb9003cf51b4dab951ffa0fb9220a2bb0374e` |
| src/keys/format/mod.rs | `867e0971d5b396acff2b7857dbc322bc5b26225a9dec3048d9e7b533db1b323e` | `fc1afb28803e09a736c871971c43584ef4d313192379adcc7d89ec98ec5e796a` |
| src/keys/format/pkcs8.rs | `69db6690bcedeaabac613859967b6ddc8ea156b41c9a0a4ca0b36614487c99d3` | `1d781fd78da9a45714b3bc96cdb32054e1f2c1691b63cdc08f72aa8f9e134772` |
| src/lib_inner.rs | `226342a2066612f1b1affc059a7eeb44f0716943fa8613e49cf91e28074bf485` | `7c8d96b0567c0b2f33b2e6c8b3d304c87b23117b8f55f538baf30ba25762367d` |
| src/session.rs | `fa836a54dd8ecd73fe68d76a3e7f2bf878c38e6cb511a986982c5f972e279e9e` | `1d27d2230a76c73aede5e2a2b5021f89ecedd7ad72c4bc1400019e388937da6b` |
| src/client/encrypted.rs | `4366e6b81d60f8a124b5865bd8f86b8dbf028d1b6639d12566a4184ff71eb2a7` | `50131d8637dff20b861302ca4c6ebbbd316a05150c8646ad4676333bc75be986` |
| src/server/encrypted.rs | `4c9e355ed91aad60225a6e17cf8ea5a9ded4b118cb711050f135b72e82cd6b0f` | `b8015c081505b7b9d866353b4d5b2e6d78d7b70a856e20da9769d88234eced89` |

The local patch to `src/keys/known_hosts.rs` preserves matching revoked keys and rejects unsupported security markers and revocation wildcard patterns. Callers check revocation before ordinary allowed entries. See `vendor/russh-patch.json` for the patched hash.

The Close lifecycle patch records `close_sent` and retains the channel record after the local Close is actually sent until the peer's Close arrives. The client/server replies only if the local side has not already sent Close, then removes the record. This avoids duplicate Close messages and makes the `channel_close` callback confirmation of remote release, rather than treating local message enqueueing as proof that GPASCII has exited.

The root Cargo.toml uses the patch through `[patch.crates-io] russh = { path = "vendor/russh" }`; the root Cargo.lock pins stable RSA/PKCS1 and upstream ssh-key rc.12. The original package Cargo.lock in vendor remains unchanged for provenance checks; it is not the lockfile used to build or audit this workspace.

## Gates and removal

`./scripts/Test-DependencyPatch.ps1` checks all 91 recorded files and the recorded patched hashes. It also reverses dependency-version, borrowed-type, and RSA-type-name changes and verifies the upstream original hashes. CI runs this before builds and dependency audits to prevent silent expansion of patch scope.

The patch must pass builds retaining RSA functionality, private-key loading/signature-authentication regressions, full workspace tests, C ABI/SDK smoke tests, and `cargo audit`/`cargo deny`. Audit results must come from reports with the same root Cargo.lock hash. Ignoring the advisory or disabling RSA is not a substitute for the fix.

Once upstream releases russh compatible with stable RSA, evaluate API and firmware algorithm differences, run all these gates, then remove the path patch, vendor, and this adaptation. Any additional source changes require a newly recorded complete diff, origin, and hashes.
