# SSH 建连性能建议

依据：22dd905 提交及其 russh 0.65.1 本地 vendor。日期：2026-11-12。本文件只给建议，未修改生产配置、未连接控制器、未进行性能实测；下述收益均需在目标控制器验证。

## 先确认连接时间花在哪里

当前路径是：读取主机校验/私钥配置 → TCP 连接（可能含 DNS）→ russh connect_stream → 私钥算法扩展查询（私钥认证分支）→ 用户认证 → 打开 Channel → exec gpascii -2 → Sys.ServoCount 就绪确认。

建议分别记录每个调用阶段的毫秒耗时，以及实际协商的 KEX、主机密钥、Cipher、MAC。日志只记算法名、阶段和耗时。SSH 认证成功的时间与 GPASCII 首次有效响应的时间必须分别呈现；若等待主要在最后一步，调整加密算法不会消除该等待。

## 最值得先处理的具体代码点

[connection.rs:216](../crates/pmac-transport-russh/src/connection.rs#L216) 对所有私钥均调用 best_supported_rsa_hash，再进入 authenticate_publickey。

本地 [client/mod.rs:781](../vendor/russh/src/client/mod.rs#L781) 的扩展查询会等待 server-sig-algs，超时为 1 秒；[client/mod.rs:802](../vendor/russh/src/client/mod.rs#L802) 也说明了这项等待。非 RSA 私钥不需要选择 RSA 签名散列。

建议只对 key.algorithm().is_rsa() 的密钥调用这一查询；Ed25519 等其他密钥直接使用 None 的 RSA hash 参数。该条件分支已在当前工作区实现，相关 transport 认证回归通过，但尚未用真实私钥测量收益。

```rust
let hash = if key.algorithm().is_rsa() {
    handle.best_supported_rsa_hash().await?.flatten()
} else {
    None
};
```

如果使用非 RSA 私钥、控制器没有及时发送扩展信息，这项改动可能省掉接近 1 秒的无用等待。若扩展已经收到，收益较小；密码认证不走此分支，不受此优化影响。RSA 本身的算法探测不能无条件取消，应根据已经验证的服务端能力选择 RSA-SHA2 或保留自动协商。

## 可配置的握手与加密参数

[client::Config](../vendor/russh/src/client/mod.rs#L2724) 的 preferred 支持 KEX、主机密钥算法、Cipher、MAC 和压缩优先级，定义见 [negotiation.rs:72](../vendor/russh/src/negotiation.rs#L72)。当前项目未在 Config 中设置 preferred，使用库默认顺序。

| 项目 | 建议 | 适用边界 |
| --- | --- | --- |
| KEX | 记录实际选择；若当前使用较重的 DH/GEX，在设备支持和安全策略允许时与 curve25519-sha256 做 A/B 测试 | 当前默认已包含 Curve25519，且优先包含 ML-KEM 混合交换，不能假定强制改成单一算法一定更好 |
| 主机密钥/用户密钥 | 设备支持时比较 Ed25519 与现有 RSA-SHA2；两种身份密钥不是同一个配置对象 | 受控制器固件和服务端密钥配置约束；不为提速切换成 SHA-1 签名 |
| Cipher | 在两端支持的算法中比较 ChaCha20-Poly1305 与 AES-GCM | 更主要影响会话数据处理，握手总耗时的改善取决于实际瓶颈，不给固定提升比例 |
| MAC | 优先沿用现有受支持的安全组合；AEAD 的认证语义按库协商处理 | 不把关闭完整性校验作为优化项 |
| Compression | PMAC 短指令场景保留 none 优先 | 默认已经 none 优先，通常无需新增压缩配置 |

调整时在默认算法集合基础上修改优先级，保留严格 KEX 和扩展协商标记；避免用单元素列表覆盖并丢掉这些标记。原始默认列表见 [negotiation.rs:165](../vendor/russh/src/negotiation.rs#L165)。算法名的公开支持说明可参考 [russh README](https://docs.rs/crate/russh/latest/source/README.md)，实际版本以本仓库 vendor 为准。

## 连接复用和预热

1. 优先复用已认证的 SSH 连接，避免在每次指令前重新握手。多个线程仍各自独占 GPASCII Channel；复用 SSH 不等于允许线程共用 GPASCII 通道。
2. 可以在业务启动前完成 Connect，降低用户发起操作时的等待。预热改变等待发生的位置，不等于降低冷启动成本，应分别统计冷连接和热连接。
3. 初期只准备所需的最小成员数，额外成员按实际并行需求创建；每个成员本身会预建一个在线通道，同一控制器总在线通道仍不得超过 8。
4. 需要时预热独立维护 SSH，避免第一次文件/Shell 操作再承担一轮握手。BufferUpload 按用户规则不计入在线 GPASCII 通道额度，但仍有维护面连接成本。
5. 不要用无关临时线程发送普通查询来预占业务线程的在线通道。初始 Connect 的未分配预建通道和首次业务调用线程的亲和关系，应继续遵守现有所有权设计。

## 现有参数中哪些无需再调

- [connection.rs:178](../crates/pmac-transport-russh/src/connection.rs#L178) 和 Config 已开启 TCP_NODELAY，Nagle 相关优化已做。
- tcp_probe=true 仅给同一次 TcpStream::connect 增加超时，并将该 stream 继续交给 connect_stream；不是先探测后再建立第二条 TCP 连接。关闭它不会省掉一次 TCP 握手。
- keepalive_interval/max 主要用于会话存活检测，不是首次建连加速参数。
- 缩短 timeout 只会更早报告失败，不能让成功握手自动变快；就绪确认仍需完整执行。
- 私钥和 known_hosts 当前按连接读取。多成员频繁建连时，可以在安全生命周期内缓存解析后的密钥和主机校验材料，配置更新时明确失效；先确认这部分耗时是否显著。

## 控制器服务端条件

若测量定位到认证前的长等待，可由控制器维护方检查 DNS 和认证后端。客户端用固定地址可排除正向解析变量；同一控制器必须统一地址表示，不能同时混用主机名/IP 而意外形成不同的预算键。

如果固件使用 OpenSSH，可检查 UseDNS。现代 OpenSSH 默认 no；仅在实际配置启用了反向解析、且测量确认其造成延迟时才评估调整。该设置还影响基于主机名的 authorized_keys from / Match Host 规则，不能只按速度更改。依据：[OpenSSH sshd_config](https://man.openbsd.org/sshd_config#UseDNS)。控制器若使用其他 SSH 实现，应采用对应实现的配置。

主机指纹校验、加密和就绪验证继续保留。本建议不要求修改控制器服务、算法或认证方式；实际变更需要先确认固件支持与既有使用方兼容。

## 推荐实施顺序

1. 加入各阶段耗时和实际协商算法记录。
2. 条件化非 RSA 密钥分支，消除不必要的 RSA 扩展等待；优先验证这个明确代码点。
3. 确认 SSH 复用、最小成员数和维护面首次连接的实际行为。
4. 如果 KEX/认证计算确实占主要时间，再对已支持算法做单变量 A/B 测试。

每组记录冷连接总耗时、中位数、P95、最大值，以及 GPASCII 第一次有效响应时间；每轮按确认关闭和释放流程完成收尾，出现不确定占用时先处理恢复，不通过更换地址或跳过配额继续压测。本轮没有这些实测结果，无法给出固定加速比例。
