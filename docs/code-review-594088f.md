# 594088f 固定快照复审

日期：2026-11-12。审核提交：`594088f48370f6de50fde0a8813efa6e24fc11c2`。根树：`ccedcfd77e77edc9283421ce01776a783986b898`。

本次按用户要求先提交修复，后审核。提交包含 19 个文件，456 行新增、40 行删除，提交后工作区干净。此报告为提交后的新增文件，尚未提交；未修改生产源码。

## 方法和范围

- 固定读取上述提交的 Git 对象，并验证每个读取对象的 SHA-1；不使用后续工作区修改作为本轮代码证据。最初读取器曾沿用旧缓存，已丢弃该次源码读取结果，改用独立读取器重新核对。
- 使用 GitNexus MCP detect_changes、context、impact 辅助定位。图索引仍为 f926556，落后本轮提交 5 个提交；新增符号及 vendor 变化以固定快照为准，不把旧图的调用数量当作完整影响范围。
- 未使用 CLI 分析源码或刷新索引；Git 只用于已授权的状态检查、文件清单和提交。未执行测试、Miri、ASan 或 TSan，未连接真实控制器。
- 范围：前轮 5 项修复、跨 Client 预算及恢复、SSH Close 状态机、C++ 生命周期和已报告 UB。
- 业务约束：同一控制器最多 8 个 GPASCII 在线通道；不同线程不共享在线通道；BufferUpload 不占在线通道额度。

结论：4 项待处理问题（2 项 P1、2 项 P2）。第 R1 项为前轮问题的剩余边界，R2–R4 是复核本次共享预算与关闭修复时发现的交互问题。

## R1 / P1：独立创建的连接配置仍可绕过控制器 8 通道总预算

位置：[transport.rs:145](../crates/pmac-core/src/transport.rs#L145)、[lib.rs:567](../crates/pmac-ffi/src/lib.rs#L567)。

prepare 已不再覆盖传入 budget，克隆同一 ConnectOptions 的两个 Rust Client 可以共享额度；但 ConnectOptions::new 每次仍创建新的预算。C ABI parse_connect_options 每次提交连接配置都会调用这个构造函数，C/C++/.NET/Python 接口也没有传入该 Arc 的字段。

触发场景：通过公开 C ABI 或其 SDK 创建两个 Client，分别配置同一控制器地址并连接，合计持有 8 个通道后，再申请第 9 个。两个独立预算分别小于 8，仍会放行。现有 cloned_connect_options_share_one_budget_across_clients 用例覆盖的是手动共享 Arc 的 Rust 路径，不能验证公开 SDK 的默认行为。

影响：日常多 Client 使用仍可能超出控制器限制；新 Client 也无法继承同一控制器已有的不确定占用。

建议：在可识别的控制器作用域内共享预算，或增加所有公开接口都可使用的控制器预算所有权机制；不能只依赖 Rust 调用方克隆 ConnectOptions。不同控制器应独立计数，修改克隆配置中的 host/port 时也不应意外混用预算。

## R2 / P1：一个 Client 的恢复确认会释放其他 Client 的不确定额度

位置：[client.rs:262](../crates/pmac-core/src/client.rs#L262)、[transport.rs:46](../crates/pmac-core/src/transport.rs#L46)、[session.rs:249](../crates/pmac-transport-russh/src/session.rs#L249)。

当前已支持跨 Client 共享 budget。Client.confirm_recovery 只校验自身 Pool 的 revision、running 和 creating，却调用 budget.confirm_remote_release；后者一次扣除整个共享预算的 uncertain，而不是当前 Client 所属的记录。预算只有 used/uncertain 两个总数，没有所有者、成员 epoch 或通道标识。

触发场景：A、B 克隆相同配置而共享预算，两者都有远端释放不确定的通道。应用只核对 A 的远端通道，并对 A 的 revision 确认释放；代码同时归还 B 的不确定额度，而 B 的 Pool.uncertain 仍保留原记录。随后 A 或 B 可以使用这些本不应归还的额度建立新通道。

影响：跨 Client 恢复确认越过 Pool 边界，可能在 B 的旧通道仍存在时突破控制器 8 通道限制。B 的状态报告与实际预算也不一致。

建议：释放不确定记录须带所有者/epoch/通道租约标识，Pool 级确认只归还被当前确认覆盖的记录。如果设计为控制器级确认，应提供覆盖所有 Client 状态及版本的独立确认流程，不能只校验其中一个 Pool 的 revision。

## R3 / P2：russh 收到 Close 时无条件再次发送，产生重复关闭报文

位置：[session.rs:287](../vendor/russh/src/session.rs#L287)、[client/encrypted.rs:532](../vendor/russh/src/client/encrypted.rs#L532)、[server/encrypted.rs:1450](../vendor/russh/src/server/encrypted.rs#L1450)。

本次补丁让发送 Close 后的通道记录保留到对端 Close 到达；但未增加 close_sent 状态。客户端和服务端的接收分支只要通道记录仍存在，都会调用 enc.byte(CHANNEL_CLOSE)，随后移除记录。

报文顺序：A 主动发送 Close → B 回复 Close 并移除通道 → A 收到回复时，因保留的通道仍是 established，再发一份 Close。双端同时主动关闭时也会出现多发。

[SSH RFC 4254 §5.3](https://www.rfc-editor.org/rfc/rfc4254.html#section-5.3) 规定：收到 Close 时，尚未发送过 Close 的一方才需要回复；双方均已发送和接收 Close 后，通道即被视为关闭，编号可以重用。

影响：重复报文发往已经关闭、甚至可能被重用的通道编号；与严格对端的兼容风险是依据协议状态机作出的推断，本轮未在真实控制器复现断连。现有双端都使用补丁的 russh 测试可能因忽略未知通道编号而掩盖重复报文。

建议：显式记录 close_sent 和 close_received，在实际输出 Close 时标记（包括 pending_data 排空后的发送路径）；收到 Close 只在本端尚未发送时回复。收到确认、通知关闭和移除记录的顺序仍须保证关闭等待者能被唤醒。

## R4 / P2：C++ 析构清理在未释放子句柄时可能永久循环

位置：[pmac_error.hpp:83](../bindings/cpp/pmac_error.hpp#L83)、[pmac_cpp98.hpp:65](../bindings/cpp/pmac_cpp98.hpp#L65)、[lib.rs:66](../crates/pmac-ffi/src/lib.rs#L66)。

runtimeCleanup 会反复 shutdown、release，直到结果不是 SHUTDOWN_PENDING。shutdown 已成功但 Registry 仍有 Client/Request 子句柄时，release 将持续返回 Pending；循环不会释放这些子句柄，也没有退避或其他所有者接管。shutdown 进入 joined 状态后会立即返回，因而此时循环还会占用 CPU。

触发条件：Runtime 析构时仍有原生子句柄，而且释放这些句柄的代码必须等该析构返回后才能执行。C++98 Client 只接收 Runtime 引用、未共享其所有者，可构造 Runtime 对象先于 Client 对象销毁的顺序；混用 C API 保存子句柄时也存在这一条件。该结论不表示正常按子对象先销毁的顺序也必然死锁。

影响：原先超时遗留 ID 的问题被改成析构永久等待；若同一线程负责后续子句柄释放，则无法自行进展。

建议：完善 Runtime 与子句柄的所有权，使 Runtime 所有者至少活到全部子句柄释放；或将未完成清理交给能够在子句柄释放后继续工作的监督者。不能在当前析构线程上无限重试一个它本身无法消除的条件。显式关闭应能返回 Pending 并保留所有权与诊断信息。

## 前轮修复核对

| 项目 | 本轮快照状态 |
| --- | --- |
| 克隆 ConnectOptions 的共享预算被覆盖 | 已删除覆盖逻辑并加入跨 Client 克隆配置用例；独立配置与 SDK 路径仍有 R1 |
| Close 入本地队列便释放额度 | 改为等待 channel_close 回调，有 2 秒超时与释放不确定保护；新增协议状态机问题见 R3 |
| TCP 阶段失败也记远端释放不确定 | 新增逐次创建重置的 gpascii_resource_attempted 标记，纯前置连接失败不再无条件登记 |
| C++98 nativePtr 的条件性数据竞争 UB | 已删除该 helper 和共享 mutable 缓存 |
| C++ Runtime 超时后遗留 ID | 增加重试清理 helper 与回归；未释放子句柄导致的循环问题见 R4 |
| BufferUpload 错误占在线通道额度 | 仍保持移出在线配额及额外 GPASCII 服务槽位预算的修复 |

以上是源码核对结果，不表示本轮执行了新增或既有测试。当前改动未新增自有裸指针操作，未确认新的合规调用下 UB；未进行完整依赖 unsafe 审计。

## 建议回归

1. 使用 C ABI 或各 SDK 的独立连接配置创建两个 Client，连接同一控制器；合计第 9 个在线通道应在发送前拒绝。另测不同控制器不会互相消耗额度。
2. 两个共享预算的 Client 同时存在不确定占用，确认 A 后 B 的不确定额度和记录必须保留；A 的 Pool revision 不能授权释放 B 的资源。
3. 报文层验证客户端主动关闭、服务端主动关闭、同时关闭和 pending_data 后关闭：每一端对同一通道最多发送一次 Close。不能只检查 handler 回调次数或只依赖双端同一补丁实现。
4. 服务端延迟 Close、主动关闭早于注册等待、关闭任务超时和 Runtime 退出时，确认额度既不提前释放，也有正确的恢复诊断。
5. C++ 清理测试包括持续 release Pending、子句柄只能在析构返回后释放的顺序，以及正常关闭耗时超过 1 秒；区分可等待的后台进度和当前线程永远无法解除的条件。
6. 保留在线 8 通道满额时 BufferUpload 成功，以及 open plc / 赋值 / close / list plc 与其他线程赋值隔离的测试。

本报告只描述 594088f 快照，不覆盖用户后续修复进度。源码链接可能打开已修改的工作文件，行号应以报告标注提交为准。
