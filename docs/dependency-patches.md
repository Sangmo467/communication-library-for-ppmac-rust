# russh 0.65.1 的稳定 RSA 依赖补丁

2026-11-11 的真实 `cargo audit` 首次审计发现 RUSTSEC-2023-0071：锁文件的 `rsa 0.10.0-rc.18` 不满足公告修复范围 `>=0.10.0`。官方 [RustSec 公告](https://rustsec.org/advisories/RUSTSEC-2023-0071.html) 说明了 Marvin 定时侧信道问题及稳定 0.10 系列修复。

当日 crates.io 最新 russh 仍为 0.65.1，它精确锁定旧 RSA/PKCS1/ssh-key 预发布版。单独 `cargo update -p rsa --precise 0.10.0` 因 `=0.10.0-rc.18` 约束失败。本仓库保留 RSA 功能，使用可审计的本地最小适配；不改算法、参数校验、签名或加密运算实现。

## 来源与完整性

- 官方包：[russh 0.65.1](https://crates.io/crates/russh/0.65.1)，归档下载地址 `https://crates.io/api/v1/crates/russh/0.65.1/download`。
- `.crate` SHA-256：`94be3d86c71b9ed220a675c405e35c7a02565a009bf8da2e9cc13a700320a5c5`，与原 Cargo.lock 的 crates.io checksum 相同。
- 包内 `.cargo_vcs_info.json` 上游提交：`b33092aad1e51c66258a12e8b83fc473a68d9d34`，路径 `russh`。
- 复制前，逐一比较原始 tar.gz 中全部 90 个文件与本机缓存的 SHA-256，全部一致。复制保留 LICENSE、源码和原包元数据，排除 Cargo 生成的 `.cargo-ok`、`.cargo-checksum.json` 缓存标记。
- `vendor/russh-upstream-files.sha256` 保存这 90 个上游文件的原始哈希；`vendor/russh-patch.json` 保存归档及两个修改文件的原始/修改后哈希。

## 完整补丁范围

`vendor/russh/Cargo.toml` 仅三处精确依赖版本变化：

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

稳定 RSA 要求稳定 PKCS1。上游 ssh-key rc.12 已使用 `rsa ^0.10` 和 `pkcs1v15` feature，解决 rc.11 请求已删除 `rsa/sha2` feature 的不兼容；SHA-2 是稳定 RSA 的必选依赖，没有删除摘要能力。

`vendor/russh/src/keys/format/pkcs8.rs` 仅一个借用 ASN.1 类型别名适配：

```diff
-let sk = &pkcs1::RsaPrivateKey::try_from(pki.private_key)?;
+let sk = &pkcs1::RsaPrivateKeyRef::try_from(pki.private_key)?;
```

稳定 PKCS1 将 `RsaPrivateKey` 泛型化，并提供 `RsaPrivateKeyRef<'a> = RsaPrivateKey<UintRef<'a>>`。该变化明确原来的借用整数表示，其他解码、数值构造和验证路径不变。russh 自身版本保持 0.65.1，不冒充上游新版本。

| 文件 | 上游 SHA-256 | 补丁后 SHA-256 |
|---|---|---|
| Cargo.toml | `00f0172e07ff5a538feeb9fb37448f2769045060bd8ae799bc2cfee333596ab0` | `1da2a0a51aac0fede21418e4884fb9003cf51b4dab951ffa0fb9220a2bb0374e` |
| src/keys/format/pkcs8.rs | `69db6690bcedeaabac613859967b6ddc8ea156b41c9a0a4ca0b36614487c99d3` | `7ccc19246d8cfa769b6e8adfe975c23e2e7eafd3b3e58da76299c8592bd03ded` |

根 Cargo.toml 通过 `[patch.crates-io] russh = { path = "vendor/russh" }` 使用补丁；根 Cargo.lock 锁定稳定 RSA/PKCS1 和上游 ssh-key rc.12。vendor 中原包 Cargo.lock 保持原样用于来源核对，不是本工作区构建或审计使用的锁文件。

## 门禁与撤销

`./scripts/Test-DependencyPatch.ps1` 检查全部文件集合、88 个未改文件、两个补丁后哈希，并将三处版本与借用类型别名反向恢复后核对上游原始哈希。CI 在构建和依赖审计前执行，防止补丁范围悄然扩大。

补丁必须通过保留 RSA 功能的构建、私钥加载/签名认证回归、整个工作区测试、C ABI/SDK 烟测以及 `cargo audit`/`cargo deny`。审计结果以同一根 Cargo.lock 哈希的报告为准；不能用忽略公告或禁用 RSA 代替修复。

上游发布兼容稳定 RSA 的 russh 后，评估 API/固件算法差异，完整运行这些门禁，再移除 path patch、vendor 和本适配。任何新增源码修改都需重新记录完整 diff、来源和哈希。
