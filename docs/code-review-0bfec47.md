# GPASCII 并发与配额统一审核报告

审核日期：2026-11-12。源码基准：`0bfec4738f08d73e2d2f48ff313fb74dd5e9089c`。

结论：当前范围内有 3 项待修复问题（P1 一项、P2 两项）。第 1、2 项延续上轮结论，第 3 项依据用户补充的 BufferUpload 规则更正后确认。本报告只修改文档，未修改生产代码。

## 审核依据与限制

- 不同调用线程不得共用一个 GPASCII 在线通道；同一线程的 open、赋值、close 必须保留通道亲和关系。
- 按同一控制器合计最多 8 个 GPASCII 在线通道审核；所有池成员的预建、打开中、在用及释放不确定的在线通道应使用同一预算。
- 用户明确确认：BufferUpload 不占用 GPASCII 在线通道。批处理命令名称含有 gpascii，不能据此把它计入在线通道额度。
- 使用 GitNexus MCP 的 list_repos、detect_changes、context、impact 分析结构和调用关系，并以显式 UTF-8 读取的最新源码核对结论。未使用 GitNexus CLI 或其他 CLI 进行源码分析。
- MCP 索引仍为 `f926556`，落后基准 3 个提交；图中新增符号和调用关系可能缺失，不能据图中“没有调用者”断言没有影响。
- 本轮未执行测试，未连接真实控制器。下列场景为静态推导，未宣称已动态复现。

## 待修复问题

### U1 / P1：在线通道配额按 Session 分开，不能限制控制器总数

证据：每次建立 Session 都先调用 open_gpascii，再创建独立的 `gpascii_usage: Mutex::new(1)`；后续 reserve 只检查当前 Session 的计数。

位置：[session.rs:36](../crates/pmac-transport-russh/src/session.rs#L36)、[session.rs:48](../crates/pmac-transport-russh/src/session.rs#L48)、[session.rs:69](../crates/pmac-transport-russh/src/session.rs#L69)。

触发场景：两个成员分别持有 5 个和 3 个在线通道后，再向任一尚未达到本地 8 个的成员申请通道。局部检查会放行第 9 个总通道。池预建通道也未经过同一个全局计数器。

影响：本地无法在触达控制器前拒绝超额请求，仍可能出现用户说明的无法连接。成员修复、重建和释放不确定的旧连接也不能依赖新 Session 的零起点重新获得完整额度。

修复要求：同一控制器的成员共享在线通道预算，在创建预建通道前预留；新线程通道、初始化中及释放不确定通道都计入该预算。额度满时返回 ResourceLimit/NotSent，保持已有线程的通道，不借用其他线程的通道。BufferUpload 不计入此预算。

### U2 / P2：关闭等待被取消时失去关闭保护，同时归还额度

证据：回收时先从映射移除 Channel，并创建一个退出即释放额度的 permit；CloseChannelOnDrop.close 内部先 take 掉 Channel，再等待 channel.close。底层 close 等待向 SSH 消息队列发送 Close，并非同步完成操作。

位置：[session.rs:601](../crates/pmac-transport-russh/src/session.rs#L601)、[session.rs:617](../crates/pmac-transport-russh/src/session.rs#L617)、[channels/mod.rs:416](../vendor/russh/src/channels/mod.rs#L416)。

触发场景：SSH 发送队列阻塞，旧通道回收暂停在 channel.close().await；此时新业务请求被取消或超时。关闭 future 被丢弃，保护对象的 Option 已为空，Drop 无法重新安排关闭；permit 则归还额度。

影响：远端旧 GPASCII 通道可能仍存在，而本地已认为额度可用，后续继续建立通道，破坏 8 通道限制。前轮 NotSent 修复避免了整成员退役，但没有保证这次回收最终完成。

修复要求：让独立于业务请求取消的关闭任务同时持有 Channel 和配额，完成关闭或记录释放不确定之后才处理额度；关闭失败或状态不明时不能直接当作已释放。初始化失败和取消时的异步关闭也应使用同样的生命周期规则。

### U3 / P2：BufferUpload 被错误纳入 GPASCII 在线通道和额外服务槽位预算

本项依据用户确认的“BufferUpload 不占用 GPASCII 通道”规则判定。

证据与位置：

- [client.rs:385](../crates/pmac-core/src/client.rs#L385) 对 BufferUpload 调用 reserve_temporary。
- [pool.rs:347](../crates/pmac-core/src/pool.rs#L347) 将其作为额外 GPASCII service slot；成员数达到 max_members 时直接拒绝。
- [session.rs:482](../crates/pmac-transport-russh/src/session.rs#L482) 对 `gpascii -2 -i...` 再次预留在线通道额度。
- [tests.rs:1130](../crates/pmac-transport-russh/src/tests.rs#L1130) 要求在线通道满额时批量加载命令失败；[pool_tests.rs:963](../crates/pmac-testkit/src/pool_tests.rs#L963) 要求无额外成员槽位时 BufferUpload 失败。这些断言与用户确认的规则不符。

触发场景 A：8 个在线通道正常在用，通过维护连接发起 BufferUpload。现有逻辑会因在线通道配额用尽而拒绝本不消耗该额度的操作。

触发场景 B：准备一个成员并配置 max_members=1，即使在线通道尚未满额，BufferUpload 也会因为“额外服务槽位”预留失败而被拒绝。

影响：合法 BufferUpload 被错误拒绝；错误预留还混淆在线通道的使用量及释放不确定记录。现有测试将这种拒绝视为正确结果，不能证明符合用户约束。

修复要求：将 BufferUpload 从在线通道计数及额外 GPASCII 服务槽位预留中移除；保留必要的维护屏障、文件并发限制及写入结果不确定保护。用“在线通道满额仍允许 BufferUpload”替换相反的测试断言。

更正记录：撤回先前“BufferUpload 应在上传前预留 GPASCII 通道”的建议，不再将检查在线通道额度过晚列为它的问题。远端文件发布与加载失败的语义属于另一项设计议题，本报告未据此新增缺陷。

## 前轮已修复项的源码核对

以下仅表示实现中已看到对应修复，不等于本轮测试通过。

| 项目 | 当前状态与证据 |
| --- | --- |
| 线程退出后已接纳请求仍需保持通道 | Request 持有存活标记的强引用，完成后释放；已新增退出线程的 open plc / 赋值 / close / list plc 用例 |
| 退出线程亲和记录无限累积 | Pool.register 调用 reclaim_exited_affinity 清理无存活引用的亲和记录 |
| 初始化探测占用业务响应预算 | readiness 使用独立 4096 字节预算 |
| 初始化期间取消导致整个成员退役 | 初始化状态用于返回 NotSent，已有取消与超时用例 |
| 回收阶段取消导致整个成员退役 | command 在锁定和回收之前设置初始化状态；仍需解决 U2 的资源生命周期问题 |
| 本地单成员通道上限为 64 | 已改为 8；仍需解决 U1 的跨成员总预算问题 |
| SBOM 遗漏依赖仍被接受 | 前轮已增加解析后包集合和依赖关系集合的完整性核对；本轮未重跑脚本 |

## 建议补充的回归测试

1. 两个及以上成员合计持有 8 个在线通道，第 9 个请求在发送前被拒绝；已有 open prog / open plc 通道持续可用，绝不跨线程借用。
2. 预建成员、线程专属通道、初始化取消、成员修复及释放不确定通道使用同一在线预算；不能通过创建新 Session 重置额度。
3. 人为阻塞 SSH 关闭消息发送，取消或超时回收请求；确认关闭任务继续执行，关闭完成前额度不归还，失败时保留释放不确定状态。
4. 8 个在线通道满额时，实际 BufferUpload API 仍可完成上传和加载；max_members 等于现有成员数时也不能仅因缺少额外 GPASCII 槽位失败。
5. PLC 交错测试中，A 执行 open plc 1、赋值、close、list plc 1；B 同时发送另一条赋值。分别断言 A 的内容正确、B 的赋值在线执行且未进入 A 缓冲区。
6. 模拟服务端分别存储 prog 和 plc，以及不同编号的缓冲区；现有测试将 open/list prog 1 与 plc 1 共用 program_lines，无法检出缓冲区类型或编号混淆。

## 报告边界

本报告统一当前线程亲和、在线通道预算、关闭取消和 BufferUpload 路径的结论。历史报告 code-review-2026-11-12.md 涉及其他时间点和更广范围，其旧问题不能未经复核直接计入本次未解决项。本轮未发现足够证据再增加第 4 项生产代码缺陷，也不能据此保证全项目没有其他问题。
