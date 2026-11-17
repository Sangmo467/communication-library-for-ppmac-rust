# Gather 数据来源审核（2026-11-13）

结论：存在可验证的数据来源验证缺失缺陷。能访问监听地址/端口的无关连接若先到达，就能成为唯一数据源；格式有效的伪造数值以成功回调交付。普通 TCP 的文档说明界定了加密边界，但不能替代来源和会话绑定。

## 代码证据

- gather.rs:405 只绑定本机地址，并不限制来源；local_ip 是目的/监听地址。
- gather.rs:438–449 的 accept 丢弃 peer 地址，接受第一个连接后关闭 listener。
- gather.rs:473 在未验证数据来源、甚至未收到首行数据时宣告 ready。
- gather.rs:550–567 只校验列数、可解析和有限值，不验证来源、会话或真实性。
- gather.rs:612 起接收循环没有数据空闲截止；连接保持但不发送时可持续等待读取，直到调用方停止或 Client/Runtime 状态异常。这不是独立的采样超时契约违背，而是抢占后可造成无数据会话的后果。

GitNexus query/context 确认 run 的直接入口为 pmac_gather_start，数据经过 receive/Decoder 到回调。SSH 控制命令和裸 TCP 数据流之间没有认证连接。远端目录锁及其 owner token 防止配置争用，token 没有在数据流中用于握手。

## 本地实验

在忽略目录 target/gather-source-audit 复制当前 FFI 源码，保留生产 Gather 函数不变，只在测试模块追加实验；核心/协议/传输依赖指向当前生产 crate。控制器命令使用现有 MockTransport fixture，实际数据使用本地 TCP。没有真实控制器连接，也没有实际 SSH 认证实验。

独立本地连接抢先发送两列数值，调用生产 pmac_gather_start 及其 coordinator/dispatcher：

```text
unrelated peer = 127.0.0.1:13439
start status=0; callback status=0; forged values=[900001.0, 900002.0, 900003.0, 900004.0]
later legitimate-source connection refused
```

后续连接拒绝仅实测为后续本地连接；真实控制器届时无法接入是由 listener 已关闭推导。实验确认无关连接的载荷可得到成功回调。曾尝试绑定 127.0.0.2 作为不同来源地址，但该环境建连未及时完成，该实验不作为跨主机验证证据。跨主机可达性仍取决于实际路由/防火墙；跨主机无来源过滤由源码确认。

```powershell
$env:PATH='C:\Users\15503\.cargo\bin;'+$env:PATH
cargo test --manifest-path target/gather-source-audit/Cargo.toml --offline --target-dir target unrelated_source_injects -- --nocapture
```

## 影响和分级

前提是能连接该监听地址和端口，并抢在控制器前完成 TCP 连接。固定端口更容易被定位，临时端口不能替代认证。攻击者不需要控制器 SSH 账号。正确列数的有限数字可污染测量；保持空闲连接可阻断正常数据；错误文本或关闭连接会终止采集。没有证据表明本缺陷直接获得代码执行或改写控制器指令，库也不会自动把 Gather 数据转成运动命令。

一般遥测/显示场景建议 P2；若业务将该数据用于自动决策、闭环或保护，优先级应提高到 P1。完全隔离且受信任的控制器专网、防火墙仅允许指定控制器访问，会降低暴露，但不是库实现了来源验证。

## 建议

1. 最低限度核对 peer IP，并绑定当前控制器/会话的允许来源；不匹配者关闭后继续等待，沿用原有绝对截止时间，避免用无关连接续期。NAT、多网卡、IPv6需显式定义预期来源，不能简单假设主机名首次解析地址一定等于回连源。
2. 增加由安全随机数产生、通过已认证SSH配置到发送端的会话握手，在验证前不交付采样；这能限制不知道秘密的抢先连接，但明文传送token不能防止链路监听后的重放或中间人。
3. 要获得与SSH身份一致的数据真实性和保密性，优先将数据放入SSH认证加密通道，或采用等价的双向认证加密数据协议。仅增加IP白名单不构成密码学认证。
4. 回归覆盖：无关连接先到、错误握手、连接后沉默、合法连接后到、deadline不续期，以及来源地址/NAT策略。采样空闲超时应作为明确可配置策略，避免误伤合法暂停。

此次只做分析和本地验证，没有修改生产源码或重建索引；新增本报告和忽略目录实验文件。所有主动文本写入为PowerShell UTF-8无BOM。
## 已实施修复

保留现有nc反向TCP数据接口，增加两项接纳条件：从认证SSH Shell的SSH_CONNECTION取得控制器IP并核对peer（含IPv4映射IPv6归一），以及每次启动重新生成的256位安全随机握手。握手经SSH配置到发送管道，验证前不发布ready或采样。无关来源和错误/旧握手关闭后继续等待，单peer握手最多200ms，全部peer共享绝对启动截止时间；等待支持停止和Client/Runtime关闭，失败时沿用回滚清理。握手完整有界读取后比较，不吞掉同包的首批数据，错误消息不包含握手值。

这是来源与会话绑定修复，仍不提供TCP加密或抵御链路监听/中间人；IP与随机握手不替代SSH隧道。SSH_CONNECTION缺失或地址非法，以及NAT/多网卡导致实际来源与发现地址不一致时失败关闭，不做接受任意来源的回退。详见gather.md的部署边界。

验证：Rust workspace 208项全部通过（11+33+57+48+59）；Gather相关14项通过，涵盖错误/旧/raw数据抢先连接后合法连接、沉默和连续错误连接的绝对deadline、停止/关闭中断、来源不符、IPv4映射和首包合并、发现失败时local override不绕过校验。cargo fmt --all --check、cargo clippy --workspace --all-targets --offline --locked -- -D warnings、git diff --check均通过。MockTransport增加SSH端点和Shell记录支持；未改ABI结构布局。未验证真实控制器上的命令执行、NAT、多网卡或中间人场景。未提交或推送。