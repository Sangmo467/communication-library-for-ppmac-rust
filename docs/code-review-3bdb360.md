# 3bdb360 代码复审与扩展检查

日期：2026-11-12。审核基准：`3bdb36087c008f075ed619e8ce6ed7e2cb7f1b00`。

结论：本轮确认 3 项待修复问题（P1 一项、P2 两项）。共享预算和关闭清理的主要实现已更新，但控制器预算仍存在跨 Client 边界，关闭确认也仍有缺口；扩展检查新增一项连接失败后的恢复阻塞问题。

## 提交和审核范围

已按用户要求提交 7 个代码文件和上一轮报告，共 8 个文件，348 行新增、226 行删除；提交后工作区干净。本报告为提交后的新增审核文档，不属于上述提交。

审核方式：GitNexus MCP 的 detect_changes、context、impact、list_repos，加上显式 UTF-8 读取最新源码。MCP 索引为 f926556，落后 4 个提交；新增符号和调用关系不能依赖旧图完整呈现。未通过 CLI 分析代码或更新索引，未重跑测试，未连接真实控制器。Git 仅用于用户授权的状态检查和提交。

范围包括本次共享预算、通道关闭、BufferUpload、成员创建与恢复；另外抽查 FFI 参数校验及句柄回滚、.NET Request 释放、Python fork 保护和持久 Shell 输出边界。以下为静态源码结论，未宣称动态复现或全项目验收通过。

约束：同一控制器最多 8 个 GPASCII 在线通道；不同线程不得共用在线通道；BufferUpload 不占在线通道额度。

## 待修复问题

### N1 / P1：同一控制器的多个 Client 仍分别获得完整 8 通道预算

位置：[client.rs:690](../crates/pmac-core/src/client.rs#L690)、[transport.rs:145](../crates/pmac-core/src/transport.rs#L145)。

prepare 每次为 Client 新建 GpasciiBudget，覆盖传入 ConnectOptions 中已有的 budget。当前共享范围是单个 Client 的连接池，不是控制器；即使调用方克隆同一份 ConnectOptions 试图共享预算，也会被覆盖。

触发场景：同一进程中两个 Client 连接相同控制器，分别持有 5 个和 3 个通道；下一次在任一 Client 内申请通道，其局部预算仍有余量，会放行控制器的第 9 个通道。旧 Client 尚有释放不确定额度时，新 Client 也能获得全新额度。

影响：多个 Client 可以绕过控制器 8 通道限制，导致连接失败；本次的单 Client、多个成员用例覆盖不到这个边界。

建议：同一控制器的 Client 应复用预算或由 API 明确绑定同一控制器预算；不要在 prepare 中无条件覆盖已共享的 budget。远端释放确认也应按预算所有者和具体释放记录处理，避免一个 Client 的确认释放另一个 Client 的额度。进程外工具占用不受本地计数器管理，不能宣称本地预算可消除所有外部争用。

### N2 / P2：Close 消息入队即归还额度，早于远端通道释放确认

位置：[session.rs:602](../crates/pmac-transport-russh/src/session.rs#L602)、[channels/mod.rs:416](../vendor/russh/src/channels/mod.rs#L416)、[channels/mod.rs:652](../vendor/russh/src/channels/mod.rs#L652)。

新关闭任务正确持有 Channel 和 permit，解决了原先取消业务请求导致关闭保护丢失的问题。但 channel.close().await 只是把 Close 消息发送到本地 SSH 消息队列；返回成功不表示服务端已处理 Close 或释放 GPASCII。代码此时立即 permit.release。

触发场景：8 个通道满额，旧线程退出后触发回收；Close 入队后 SSH 网络发送或服务端处理被延迟，本地额度已归还，另一个成员可以立即从另一条 SSH 连接申请新通道。旧进程仍占额度时便可能触发第 9 个连接失败。

影响：计数器只能限制本地排队后的名义占用，不能可靠限制远端同时在用通道。正常成功路径也存在此窗口，不仅限于关闭失败。

建议：等待远端 Channel Close 等可确认的关闭事件，再归还额度；超时、连接异常和关闭任务未确认退出时保留释放不确定记录。关闭任务仍须独立于业务请求取消。不能把队列发送成功当作远端资源释放成功。

### N3 / P2：未创建远端通道的连接失败也被计入释放不确定，阻塞显式恢复

位置：[client.rs:889](../crates/pmac-core/src/client.rs#L889)、[client.rs:778](../crates/pmac-core/src/client.rs#L778)、[connection.rs:110](../crates/pmac-transport-russh/src/connection.rs#L110)。

create_member 的所有 Err 分支都无条件向 s.uncertain 添加 (id, epoch)，没有区分是否实际分配了远端 GPASCII 资源。TCP 拒绝、TCP 探测超时等发生在创建 SSH/GPASCII 通道之前；新 GpasciiBudget 的 permit 会正常释放，但成员层的 uncertain 记录仍被留下。

触发场景：配置 target_size=max_members=1，控制器端口暂时不可达。首次 prepare 返回 ConnectError，记录一个释放不确定成员；控制器恢复后执行 PoolRepair，occupied=0、uncertain=1，仍满足满额判断，返回 ResourceLimit。必须执行远端释放确认才能修复，尽管此前根本没有创建远端通道。

影响：明确无远端资源的瞬时网络失败消耗成员配额，阻止合法恢复；用户被要求确认不存在的远端释放。默认更大配额也会在重复失败后受到影响。

建议：按实际资源分配阶段或租约状态决定是否记录远端释放不确定；纯 TCP/认证前失败不应添加 GPASCII 服务占用。不要简单以业务请求的 NotSent 分类代替资源生命周期判断：GPASCII 初始化取消虽然业务未发送，仍可能需要清理已创建的远端进程。

## 上轮问题的处理结果

| 上轮问题 | 当前源码状态 |
| --- | --- |
| 每个 Session 独立计数 | 同一 Client 的成员现在共享 budget，预建通道也先预留；跨 Client 缺口见 N1 |
| 关闭等待被业务取消后丢失保护 | 已改为独立关闭任务持有 Channel 和 permit；释放时机缺口见 N2 |
| BufferUpload 错误占在线通道和额外服务槽位 | 已移除两处错误预留及临时服务槽位记账 |

新测试包含一个/两个成员场景、在线通道满额时实际 BufferUpload 成功，以及 max_members=1 时 BufferUpload 无需额外槽位。这些测试内容已读取核对，但本轮未执行，不能报告为本轮通过。

## 扩展抽查结果

以下历史问题的对应修复仍在当前源码中，未因本轮改动回退：

- FFI 使用 ClientReservation 的 Drop 回滚未发布句柄；Runtime 在创建 Semaphore 前检查 MAX_PERMITS，覆盖 32 位非法配置边界。
- .NET 未完成 Request.Dispose 仅在原生 ID 已释放时 SuppressFinalize，保留最终释放机会。
- Python Request finalizer 保存并核对创建 PID，避免 fork 子进程释放继承句柄。
- 持久 Shell 分别等待 stdout 和 stderr 完成标记后发布结果。

这属于源码抽查，不替代平台、GC/fork 或真实网络交错测试。未在这些抽查点确认新的独立缺陷。

## 建议补测

1. 两个 Client 连接同一控制器，合计 8 个在线通道后，第 9 个在发送前拒绝；克隆 ConnectOptions、重新创建 Client 也不能绕过预算。不同控制器应分别计算。
2. 服务端延迟确认 Close，阻止原通道释放；本地不能提前归还额度。验证跨成员、不同 SSH 连接情况下的远端同时在线通道峰值不超过 8。
3. 关闭任务遭遇错误、超时或 Runtime 退出时，未确认的通道进入释放不确定状态，不因 permit 默认 Drop 被当作已经释放。
4. target_size=max_members=1：先制造 TCP 拒绝，再恢复服务；显式 PoolRepair 应成功，失败阶段没有分配通道时 release_uncertain 应为 0。
5. 将 TCP 失败与 GPASCII 初始化后取消分开测试；后者需要实际关闭确认，不能因为业务 NotSent 就忽略远端通道占用。
6. 在线 8 通道保持占用时执行 BufferUpload，验证完整上传/加载成功、在线计数不变；保留 open plc、close、list 与另一线程在线赋值不串入缓冲区的检查。

## 交付状态

代码及上一轮报告已提交为 3bdb360。本次仅生成本报告，未修改生产源码；本报告尚未提交。所有问题均以本轮读取的最新源码位置为证据，MCP 旧图只用于结构辅助。没有本轮测试通过或控制器联机验证的新增证据。
