# 22dd905 提交后复审

日期：2026-11-12。基准提交：`22dd90506321f4f8cfde344d1e902901dc7cb3b0`。根树：`46182579e3b494ab88e0fb23da2f7926d90a4055`。

已按用户要求提交 11 个代码/文档改动及 2 份报告，共 13 个文件，343 行新增、22 行删除；提交后工作区干净。本报告在提交后生成，尚未提交。

提交前已运行 GitNexus MCP detect_changes(scope=all)，未见 partial/truncated；随后固定提交并逐对象验证 SHA-1。使用 MCP context/impact 辅助核对，索引落后本轮提交 2 个提交，新方法以固定源码为证据。UNKNOWN 或空调用集合未被当成安全结论。未使用 CLI 分析或刷新索引，未运行测试及动态 UB 检测。

结论：上轮 2 项 P1 的主要修复已落实，本轮确认 1 项 P2 边界问题，涉及 release 与尚未分配 GPASCII 的请求准入。

## C1 / P2：release 未与请求准入同步，已接纳的初始化可能在 ID 删除后分配通道

位置：[lib.rs:274](../crates/pmac-ffi/src/lib.rs#L274)、[client.rs:238](../crates/pmac-core/src/client.rs#L238)、[client.rs:145](../crates/pmac-core/src/client.rs#L145)、[helpers.rs:37](../crates/pmac-ffi/src/helpers.rs#L37)。

has_unreleased_gpascii 检查 Pool.uncertain 和预算当前 owner_usage；没有检查已接纳任务、创建中成员或是否已经停止准入。pmac_client_release 检查后直接删除 Registry ID，也没有像 close 那样设置 state.closing。

触发顺序：

1. pmac_connect_submit 已成功返回请求 ID，初始化任务已被接纳，但尚未运行到第一笔在线通道 reserve（或受控 Transport 初始化在该点前等待）。
2. owner_usage 仍为 0，Pool.uncertain 为空；pmac_client_release 可以返回 Ok，删除 Client ID。
3. 异步任务持有 Client 的 Arc，继续初始化并创建 GPASCII。原 Client ID 已不可查询，也不会出现在按 Registry Client 条目枚举的 recovery_ids 中。

另一个同源窗口是 submit_owned 已取得 Client Arc，release 在其进入 Client.submit 前删除 ID；因为 release 没有关闭准入，该 Arc 随后仍可接纳新请求。

影响：释放检查覆盖了已经存在的在线/回收中/不确定通道，但未覆盖尚未分配通道的已接纳工作，后续资源可能没有可恢复的 Client ID。相关任务持有 Arc，本项不等于悬空内存 UB。

适用边界：要求直接 release 与初始化/提交交错，或在未完成 close 的状态调用 release。按“停止新提交、等待 close 完成、确认必要配额、再 release”的顺序使用时，不触发此窗口；因此本轮按 P2 报告，不重复宣称上轮正常关闭路径的 P1 未修复。

建议：将 release 放在停止准入后的稳定生命周期中，与 Client.submit 使用同一同步协议；已接纳工作尚未结算时拒绝删除或先完成取消收尾。若 API 要求必须先 close，应明确并在入口校验，而不只检查瞬时 owner_usage。允许释放全新未使用 Client 的路径也应原子停止其后续准入。

建议测试：把初始化阻塞在第一次 GPASCII reserve 前，成功提交后尝试 release，应返回 Pending 或完成安全停止，不能删除 ID 后继续创建资源；再控制 submit 已取得 Arc、尚未进入准入锁时与 release 交错，验证删除后不再接受新工作。

## 上轮修复结果

- C++17 和 C++98 reset 现在返回状态，仅在 release 成功时清零；失败时显式对象保留 ID。析构后可通过新增 recovery_ids 接口找到原生保留记录，原先不可发现的 owner 已有恢复途径。
- 原生 release 改为 owner_usage 与 Pool.uncertain 双重检查，覆盖已经预留、在用、回收中及预算不确定的通道；Pool 状态也开始反映预算中的 owner 不确定记录。
- 回收任务首次 poll 前的保护、全局 owner token、同锁计数/保活及已完成监督线程回收仍保持修复。
- 额外检查了恢复 ID 枚举、释放失败状态保留、回收 permit 生命周期和关闭结果传播；未确认其他独立缺陷或新增 UB。

## 验证与交付边界

没有本轮测试通过或控制器实测的新证据。新增的原生恢复 ID 枚举测试已读取核对，但未执行；建议补上 C1 的初始化/提交与释放交错测试。当前结论不能替代动态并发和实际断网关闭验证。

本轮未修改生产源码。本报告尚未提交；所有行号属于 22dd905 固定提交，本地链接可能显示后续工作文件。用户后续提出的 SSH 建连优化建议是独立分析，不作为本轮缺陷条目。
