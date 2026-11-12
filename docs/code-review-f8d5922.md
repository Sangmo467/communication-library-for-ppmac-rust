# f8d5922 提交后复审

日期：2026-11-12。基准提交：`f8d59227641239419b0ed3d814f34f1f79ecb7d8`。根树：`0f8e3ed83ed670786491d1da085a642bd5794123`。

已按用户要求提交修复、3 份审核报告及新增仓库指引，共 22 个文件，732 行新增、97 行删除；提交后工作区干净。本报告在提交后生成，尚未提交，不属于上述快照。

## 审核方法

新增 AGENTS.md 要求提交前做图变更分析。本轮已在提交前运行 GitNexus MCP detect_changes(scope=all)，未见 partial/truncated 标记；提交后再以 compare/594088f 映射差异，并用 context/impact 辅助核对。MCP 索引落后本轮提交 1 个提交，新关闭 helper 未能在图中定位，实际代码证据来自直接读取并校验 SHA-1 的 Git 对象。

未使用 CLI 分析源码或刷新索引，Git 只用于用户授权的状态检查和提交。未运行测试、Miri、ASan、TSan 或真实控制器操作；以下是静态源码结论，未宣称动态复现。

结论：2 项待修复问题（P1 两项）。上轮计数/自保活竞态和已完成清理句柄积累已有对应修复；正常关闭及 Closed Client 的手动确认也已补上，本轮重点发现关闭异常与回收任务首次执行前取消的缺口。

## F1 / P1：关闭确认失败未进入公开状态，SDK 可能释放仍有不确定占用的 owner

位置：[session.rs:609](../crates/pmac-transport-russh/src/session.rs#L609)、[session.rs:251](../crates/pmac-transport-russh/src/session.rs#L251)、[pool.rs:229](../crates/pmac-core/src/pool.rs#L229)、[pmac.hpp:277](../bindings/cpp/pmac.hpp#L277)。

close_channel_confirmed 超时或未取得远端 Close 时，只通过 UncertainPermitOnDrop 在预算里记录 uncertain，返回类型仍为 ()。close_gpascii_channels 同样不返回关闭结果；retire_control 随后只返回整个 SSH 连接的关闭结果。

如果 Channel 确认失败，但 SSH 本地关闭成功，Client.close 仍能返回 Ok 并将成员标为 Drained。Pool.snapshot.release_uncertain 只取 Pool.uncertain.len，不读取预算中新增的 owner 不确定占用；因此一个此前健康的 Pool 可以返回 release_uncertain=0，而控制器预算实际仍保留占用。

Closed Client 现在可以在 ID 保留时确认释放，这是有效修复；剩余问题是调用方没有可靠公开信号知道必须保留 ID。C++ reset 会关闭后无条件 release；Python with 的 close 成功后随即 release，.NET Dispose 也在 Close 成功后释放。ID 一旦释放，新 Client 的新 owner 不能确认旧 owner 的记录。

触发场景：在线 Channel 的远端 Close 确认丢失或延迟超过 2 秒，随后 SSH 本地关闭成功；应用按 SDK 正常关闭/释放流程操作，并在控制器上核实旧进程已退出。预算仍占用旧 owner 的名额，而恢复入口已随 ID 丢失。累计这类占用会使后续连接被 ResourceLimit 拒绝。

建议：将 Channel 关闭结果和 owner 的不确定占用传播到 Client/Pool 公开状态；SDK 在需要恢复时保留或移交相应所有者，不能把 SSH 关闭成功直接当作整个额度回收成功。若允许释放 Client ID，应有范围明确的旧租约恢复机制，不应靠新 Client 无条件清除所有历史记录。

建议测试：受控服务端延迟/丢弃 Channel Close 确认，但允许 SSH 关闭成功；通过实际 C ABI/SDK 检查返回结果、release_uncertain 和 ID 保留情况。随后用受限恢复步骤归还额度并重新连接，不使用内部预算函数清账。

## F2 / P1：回收 future 首次 poll 前被取消，会把未确认资源当作已释放

位置：[session.rs:609](../crates/pmac-transport-russh/src/session.rs#L609)、[session.rs:652](../crates/pmac-transport-russh/src/session.rs#L652)、[transport.rs:123](../crates/pmac-core/src/transport.rs#L123)。

close_channel_confirmed 是 async fn，接收原始 GpasciiChannelPermit；UncertainPermitOnDrop 在函数体首行构造。CloseChannelOnDrop::drop 调用 runtime.spawn(close_channel_confirmed(..., permit))，把原始 permit 放进尚未执行的 future。

async 函数体只有在 future 被 poll 时执行。若 Runtime 正在退出，或调度出的任务在首次 poll 前被取消，UncertainPermitOnDrop 尚未构造；丢弃 future 会直接调用原始 permit 的 Drop，走 release_one，而不是 retain_uncertain。

影响：尚未等待远端确认，甚至可能尚未发送 Close，额度就被归还。初始化请求取消、线程退出回收与 Runtime 退出交错时存在此窗口，违背未确认资源应保留占用的规则；重建 Runtime/Client 后可能再次放行控制器第 9 个通道。这是资源状态逻辑问题，不归类为 Rust 数据竞争 UB。

建议：在构造或 spawn 异步 future 之前就创建不确定占用保护，并让 future 捕获该保护；或让关闭 helper 接收已经受保护的 permit。任务即使一次都没有执行，也必须通过保护对象的 Drop 保留不确定记录。

建议测试：使用不会在 spawn 当下执行任务的受控 Runtime，创建回收任务后在第一次调度前立即 abort/drop，断言未取得远端确认时额度仍被占用；同时保留运行中取消、确认成功和确认超时的用例。

## 上轮修复核对

| 上轮问题 | 当前提交状态 |
| --- | --- |
| 旧 owner 在正常关闭后不能确认 | 正常关闭先等逐 Channel Close；Closed Client 保留 ID 时可确认自身 owner。异常关闭与 SDK 生命周期剩余缺口见 F1 |
| used 与 keepalive 的并发更新不一致 | keepalive 已并入 GpasciiBudgetState，计数和自保活更新由同一把锁保护；前轮交错窗口已移除 |
| 已完成清理线程句柄一直积累 | 每次 cleanup 前增加 reap_finished_cleanup_workers，回收已完成线程；最终 cleanup_wait 仍等待运行中线程 |

## 其他核对与验证边界

- 8 通道预算、进程唯一 owner、按 owner 确认和 BufferUpload 不占在线通道的规则仍保留。
- C++98 共享 nativePtr 缓存仍保持删除；没有确认新的未定义行为。动态卸载仍须遵守停止新准入、释放子句柄并调用 cleanup_wait 的契约。
- 上轮要求的公开状态/SDK 真实生命周期及首次 poll 前取消场景，需要补充针对性测试；读取既有测试代码不等于本轮执行通过。
- 不因 MCP 的 UNKNOWN 或空调用集合认定相关函数无调用者；新 helper 未索引的范围已用固定源码核对。图的影响评分不替代本报告逐项结论。
- 未修改、提交或覆盖任何生产源码。此报告尚未提交；所有行号以 f8d5922 快照为准，本地链接可能显示用户后续修改。
