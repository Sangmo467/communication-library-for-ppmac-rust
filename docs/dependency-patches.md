# russh 0.65.1 的稳定 RSA 依赖补丁

[中文](dependency-patches.md) | [English](dependency-patches.en.md)

2026-11-11 的真实 `cargo audit` 首次审计发现 RUSTSEC-2023-0071：锁文件的 `rsa 0.10.0-rc.18` 不满足公告修复范围 `>=0.10.0`。官方 [RustSec 公告](https://rustsec.org/advisories/RUSTSEC-2023-0071.html) 说明了 Marvin 定时侧信道问题及稳定 0.10 系列修复。

当日 crates.io 最新 russh 仍为 0.65.1，它精确锁定旧 RSA/PKCS1/ssh-key 预发布版。单独 `cargo update -p rsa --precise 0.10.0` 因 `=0.10.0-rc.18` 约束失败。本仓库保留 RSA 功能，使用可审计的本地最小适配；不改算法、参数校验、签名或加密运算实现。

## 来源与完整性

- 官方包：[russh 0.65.1](https://crates.io/crates/russh/0.65.1)，归档下载地址 `https://crates.io/api/v1/crates/russh/0.65.1/download`。
- `.crate` SHA-256：`94be3d86c71b9ed220a675c405e35c7a02565a009bf8da2e9cc13a700320a5c5`，与原 Cargo.lock 的 crates.io checksum 相同。
- 包内 `.cargo_vcs_info.json` 上游提交：`b33092aad1e51c66258a12e8b83fc473a68d9d34`，路径 `russh`。
- 复制前，逐一比较原始 tar.gz 中全部 90 个文件与本机缓存的 SHA-256，全部一致。复制保留 LICENSE、源码和原包元数据，排除 Cargo 生成的 `.cargo-ok`、`.cargo-checksum.json` 缓存标记。
- `vendor/russh-upstream-files.sha256` 记录原有 90 个文件以及后来补入的 LICENSE；`vendor/russh-patch.json` 记录归档、RSA 补丁和 known_hosts 解析补丁的哈希。

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

`vendor/russh/src/keys/format/pkcs8.rs` 包含借用 ASN.1 类型适配：

```diff
-let sk = &pkcs1::RsaPrivateKey::try_from(pki.private_key)?;
+let sk = &pkcs1::RsaPrivateKeyRef::try_from(pki.private_key)?;
```

另外，将 `src/keys/format/mod.rs` 中一处及 `src/keys/format/pkcs8.rs` 中两处 `rsa::RsaPrivateKey` 替换为其非弃用的等价名称 `rsa::PrivateKey`；RSA 类型和运算语义保持不变。

稳定 PKCS1 将 `RsaPrivateKey` 泛型化，并提供 `RsaPrivateKeyRef<'a> = RsaPrivateKey<UintRef<'a>>`。该变化明确原来的借用整数表示，其他解码、数值构造和验证路径不变。russh 自身版本保持 0.65.1，不冒充上游新版本。

| 文件 | 上游 SHA-256 | 补丁后 SHA-256 |
|---|---|---|
| Cargo.toml | `00f0172e07ff5a538feeb9fb37448f2769045060bd8ae799bc2cfee333596ab0` | `1da2a0a51aac0fede21418e4884fb9003cf51b4dab951ffa0fb9220a2bb0374e` |
| src/keys/format/mod.rs | `867e0971d5b396acff2b7857dbc322bc5b26225a9dec3048d9e7b533db1b323e` | `fc1afb28803e09a736c871971c43584ef4d313192379adcc7d89ec98ec5e796a` |
| src/keys/format/pkcs8.rs | `69db6690bcedeaabac613859967b6ddc8ea156b41c9a0a4ca0b36614487c99d3` | `1d781fd78da9a45714b3bc96cdb32054e1f2c1691b63cdc08f72aa8f9e134772` |
| src/lib_inner.rs | `226342a2066612f1b1affc059a7eeb44f0716943fa8613e49cf91e28074bf485` | `7c8d96b0567c0b2f33b2e6c8b3d304c87b23117b8f55f538baf30ba25762367d` |
| src/session.rs | `fa836a54dd8ecd73fe68d76a3e7f2bf878c38e6cb511a986982c5f972e279e9e` | `1d27d2230a76c73aede5e2a2b5021f89ecedd7ad72c4bc1400019e388937da6b` |
| src/client/encrypted.rs | `4366e6b81d60f8a124b5865bd8f86b8dbf028d1b6639d12566a4184ff71eb2a7` | `50131d8637dff20b861302ca4c6ebbbd316a05150c8646ad4676333bc75be986` |
| src/server/encrypted.rs | `4c9e355ed91aad60225a6e17cf8ea5a9ded4b118cb711050f135b72e82cd6b0f` | `b8015c081505b7b9d866353b4d5b2e6d78d7b70a856e20da9769d88234eced89` |

`src/keys/known_hosts.rs` 的本地补丁保留匹配的撤销密钥，并拒绝未支持的安全 marker 和撤销通配模式；调用方先判定撤销，再判定普通允许条目。补丁后哈希见 `vendor/russh-patch.json`。

Close 生命周期补丁记录 `close_sent`，本端实际发送后保留通道记录直到对端 Close 到达；client/server 只在本端尚未发送时回复，再移除记录。这样可避免重复 Close，并让 `channel_close` 回调作为远端释放确认，而不是把本地消息入队误当成 GPASCII 已退出。

根 Cargo.toml 通过 `[patch.crates-io] russh = { path = "vendor/russh" }` 使用补丁；根 Cargo.lock 锁定稳定 RSA/PKCS1 和上游 ssh-key rc.12。vendor 中原包 Cargo.lock 保持原样用于来源核对，不是本工作区构建或审计使用的锁文件。

## 门禁与撤销

`./scripts/Test-DependencyPatch.ps1` 检查全部 91 个记录文件及记录的补丁后哈希，并将依赖版本、借用类型和 RSA 类型名称反向恢复后核对上游原始哈希。CI 在构建和依赖审计前执行，防止补丁范围悄然扩大。

补丁必须通过保留 RSA 功能的构建、私钥加载/签名认证回归、整个工作区测试、C ABI/SDK 烟测以及 `cargo audit`/`cargo deny`。审计结果以同一根 Cargo.lock 哈希的报告为准；不能用忽略公告或禁用 RSA 代替修复。

上游发布兼容稳定 RSA 的 russh 后，评估 API/固件算法差异，完整运行这些门禁，再移除 path patch、vendor 和本适配。任何新增源码修改都需重新记录完整 diff、来源和哈希。
