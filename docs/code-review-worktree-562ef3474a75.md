# 工作区快照 562ef3474a75 复审

日期：2026-11-12。冻结时间：2026-11-12T10:00:21.674Z。

50 个文件的快照指纹（SHA-256）：`562ef3474a75cb765c9ed74cb7d14abac31c5f1eefc5352771345070ad894dfb`。逐文件读取 UTF-8 并计算哈希，再完整读取一次验证一致；后续审核使用内存冻结内容，不混入工作区后续修改。

使用 GitNexus MCP detect_changes、context、impact、list_repos 定位变更和相关入口；索引元数据指向 594088f，本轮包含未提交的修复，因此以冻结源码为准。未使用 CLI 分析或更新索引，未运行测试、Miri、ASan、TSan 或真实控制器操作。本轮只新增报告，不修改或提交生产代码。

结论：3 项待修复问题（2 项 P1、1 项 P2），均涉及最新修复的生命周期及并发边界。未将上轮已修复触发条件重复列入。

## T1 / P1：关闭产生的旧 owner 不确定配额缺少公开恢复路径

位置：[session.rs:204](../crates/pmac-transport-russh/src/session.rs#L204)、[session.rs:252](../crates/pmac-transport-russh/src/session.rs#L252)、[client.rs:244](../crates/pmac-core/src/client.rs#L244)、[client.rs:693](../crates/pmac-core/src/client.rs#L693)。

正常 Session.close 通过 retire_control 关闭 SSH，然后把仍保存的在线通道 permit 全部计为原 owner 的 uncertain。新的自保活机制会保留这些占用。但 Client.close 已把 Pool.closing 设为 true，confirm_recovery 会直接返回 Closing；释放 Client 后更无法通过原 ID 确认。新 Client 获得新 owner，确认时只处理自己的记录，不能处理旧 owner。

触发场景：同一调用线程反复创建 Client、连接同一控制器、关闭并释放 Client，不额外触发线程退出回收。每个 Session 至少保存一个预建/专属通道，关闭时计为旧 owner uncertain。即使应用已在设备上核实旧进程退出，公开 C ABI/SDK 没有可确认这些孤立记录的入口。单通道场景累计 8 次后，第 9 次连接会因历史占用被拒绝。

影响：上一轮“重建会忘记占用”修复后，现在能保留记录，但连接—关闭—重建流程可能永久耗尽进程内该控制器的额度。底层预算单元测试直接调用 confirm_remote_release(old_owner)，不能证明公开接口可完成这一步。

建议：为关闭后的旧租约提供可追溯、受限且可明确确认的恢复入口，例如保留 Closed Client 的远端释放确认能力，或控制器级的旧 owner/epoch 恢复凭据。正常关闭可优先完成逐 Channel Close 确认，归还已证实释放的额度；不能让新 owner 无条件确认所有历史占用，否则会重新引入跨 Client 误释放。

建议回归：通过实际 C ABI/SDK 反复执行连接、关闭、释放超过 8 轮，包含“设备已核实释放但旧 Client 已关闭/释放”的恢复步骤；不靠内部预算函数或猜测 owner 值清账。

## T2 / P1：used 和自保活引用分开加锁，存在丢失保活的并发窗口

位置：[transport.rs:56](../crates/pmac-core/src/transport.rs#L56)、[transport.rs:89](../crates/pmac-core/src/transport.rs#L89)。

reserve 在 state 锁内递增 used，释放 state 后再设置 keepalive。release_usage 在 state 锁内减计数并计算 idle，释放 state 后才执行 keepalive.take。两段操作不构成统一状态转换。

合法交错如下：

1. 初始 used=1。线程 A 归还最后一个旧 permit，把 used 减到 0，记录 idle=true 后释放 state 锁，尚未清除 keepalive。
2. 线程 B reserve，把 used 增到 1，并将 keepalive 设为 Some(self.clone())，成功取得新 permit。
3. A 继续执行先前 idle=true 分支，清掉 B 所依赖的 keepalive。
4. B 将 permit 转为 uncertain，之后外部 Client/配置引用全部释放。没有自保活引用的账本仍会销毁，注册表 Weak 失效；重建又获得完整额度。

影响：单线程生命周期用例可以通过，但并发归还与新建会破坏“存在不确定占用则账本必定存活”的不变量，仍可能遗失未确认占用并突破 8 通道限制。这是逻辑竞态，不归类为 Rust 数据竞争 UB。

建议：used、uncertain 与自保活状态应在同一个一致的同步协议下更新，例如把保活字段并入同一状态锁，或严格在同一锁保护下完成判断与更新。不能仅根据释放锁前的 idle 布尔值清除引用。

建议回归：控制“最后一次释放”和“新 reserve”的交错，随后把新 permit 转为 uncertain 并释放外部引用，验证 Weak 仍可 upgrade，且额度不能重置。可用并发状态探索验证同一不变量，不只增加顺序执行测试。

## T3 / P2：正常结束的清理监督线程句柄持续累积，没有日常回收

位置：[lib.rs:100](../crates/pmac-ffi/src/lib.rs#L100)、[lib.rs:143](../crates/pmac-ffi/src/lib.rs#L143)、[lib.rs:161](../crates/pmac-ffi/src/lib.rs#L161)。

cleanup_workers 是进程级 Vec<JoinHandle<()>>。每次成功启动 cleanup 都 push 一条；监督者正常结束和释放 Runtime ID 后不会移除其句柄。当前唯一 drain/join 路径是显式 pmac_runtime_cleanup_wait。

触发场景：长期运行进程反复创建/销毁 Runtime，C++ 析构只安排 cleanup；按文档在最终卸载 DLL 时才调用 cleanup_wait。Runtime ID 被及时释放，可以持续创建下一轮，但历史已结束线程的 JoinHandle 仍全部保留。

影响：全局列表及其持有的线程资源随累计清理次数增长，没有受当前 Runtime 数量上限约束。本项是资源保留问题，不声称普通执行立即发生 UB。

建议：在新调度、其他合适的管理阶段或常驻监督者中回收已经完成的句柄，并保留正在运行线程供卸载屏障等待。回收完成线程不应要求等待其他仍有子句柄的 Runtime，避免再次引入析构阻塞。

建议回归：同一进程反复创建和清理大量 Runtime，不在每一轮调用全局等待；待该轮清理完成后，历史句柄数应受限。最后 cleanup_wait 仍须可靠等待全部剩余监督者。

## 上轮 4 项修复核对

| 上轮问题 | 当前冻结源码 |
| --- | --- |
| Weak 注册表忘记不确定占用 | 加入自保活引用，顺序生命周期已覆盖；仍需处理 T1 的公开恢复和 T2 的并发一致性 |
| 跨 Runtime 的预算 owner 冲突 | 改为进程级原子分配、不复用的 owner token，不再直接使用 Runtime 局部 Client ID |
| 清理监督线程不属于卸载屏障 | 保留 JoinHandle 并增加 cleanup_wait；ABI 文档要求卸载前停止新准入并等待监督者退出 |
| cleanup 调度标记失败后不回滚 | 启动失败调用 allow_cleanup_retry；CleanupAttempt 在未完成退出时通过 Drop 回滚 |

顺序预算保活测试、进程唯一 owner 测试以及清理监督者的显式等待测试已经出现在源码中。本轮只读取测试内容，未执行，不能报告本轮通过。

## UB 与范围说明

- 本轮未确认新的 UB。T2 是加锁内存安全仍成立时的逻辑状态竞态，不能与 C++ 数据竞争 UB 混淆。
- 上轮动态卸载风险按新契约处理：停止新 Runtime/cleanup 准入、释放子 ID，再调用 cleanup_wait 等待监督者；未把违反这些前提的并发卸载另算作新缺陷。
- C++98 共享 nativePtr 缓存仍保持删除，BufferUpload 仍不占 GPASCII 在线通道额度。
- 尚未动态复现 T1/T2/T3，也未完成依赖 unsafe 全量审计；结论来自冻结源码的控制流和生命周期推导。
- 本报告尚未提交。没有改动、提交或覆盖用户正在修复的生产源码。源码链接可能显示后续工作文件，所有行号均应按本报告快照指纹核对。
