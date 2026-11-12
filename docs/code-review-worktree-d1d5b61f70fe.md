# 工作区快照 d1d5b61f70fe 复审

日期：2026-11-12。冻结时间：2026-11-12T09:38:30.750Z。

快照指纹（SHA-256）：`d1d5b61f70fe4b617e8cfd67e705d1c83ce1f85b90e321d05643508346791f6e`。

冻结 49 个源码及相关文件，先逐文件读取 UTF-8 并计算哈希，再次读取全部文件确认哈希一致。后续分析只使用内存中的冻结内容，不跟随工作区继续变化。本轮未提交或修改生产源码，仅新增本报告。

GitNexus MCP detect_changes 报告当前有 17 个变更文件；MCP 索引元数据指向 594088f，索引时间 2026-11-12T09:27:05.664Z。图用于结构辅助，当前未提交修复的结论以冻结源码为准。未使用 CLI 分析或刷新索引，未执行测试、Miri、ASan、TSan 或真实控制器操作。

结论：4 项待修复问题（3 项 P1、1 项 P2），均是本次共享预算及后台清理修复的边界问题。上轮 4 项主要路径已有对应修复，见后表。

## W1 / P1：最后一个预算所有者释放后，不确定占用随 Weak 注册表丢失

位置：[transport.rs:19](../crates/pmac-core/src/transport.rs#L19)、[transport.rs:27](../crates/pmac-core/src/transport.rs#L27)、[session.rs:252](../crates/pmac-transport-russh/src/session.rs#L252)。

控制器注册表只保留 Weak<GpasciiBudget>。permit.retain_uncertain 将占用计入预算后消费 permit，不会单独保活预算；BudgetState 中的不确定计数也不能保活其自身。

触发场景：Client 关闭时旧通道被记为远端释放不确定；随后应用释放最后一个 Client、Session 及连接配置引用。注册表中的 Weak 失效，预算连同不确定记录销毁。再创建同一 host/port 的 Client 时，shared 无法 upgrade，直接新建 used=0 的预算。

影响：用户尚未确认远端旧通道释放，本地却重新获得全部 8 个名额。关闭并重建 Client 可以绕过控制器占用保护，影响 C ABI/SDK 和 Rust 核心路径。

建议：控制器占用账本必须独立于 Client 的生命期；至少在存在在用或不确定记录时保持其状态，并提供明确的重连后恢复/确认途径。只有确认占用为零时才能回收账本。不能简单改为永久强引用而不处理已确认空闲记录的清理和容量策略。

建议测试：将 8 个 permit 全部转为 uncertain，释放所有 Client/配置强引用，再用同一控制器键获取预算，第一个新 reserve 应仍被拒绝，直到完成有效释放确认。

## W2 / P1：Rust 核心跨 Runtime 的 Client ID 碰撞导致 owner 隔离失效

位置：[runtime.rs:41](../crates/pmac-core/src/runtime.rs#L41)、[runtime.rs:59](../crates/pmac-core/src/runtime.rs#L59)、[client.rs:693](../crates/pmac-core/src/client.rs#L693)、[transport.rs:64](../crates/pmac-core/src/transport.rs#L64)。

预算现在以 owner 区分不确定记录，但 owner 直接取 Client.id。每个 Rust Runtime 自己的 next_id 都从 1 开始，公开的 Runtime.create_client 在各自 Runtime 内分配 ID，因此两个 Runtime 的首个 Client 都可以是 1。同一控制器的预算注册表却是进程级共享。

触发场景：两个 Rust Runtime 各创建第一个 Client，连接同一控制器；两个 Client 的 gpascii_owner 均为 1。A、B 分别产生不确定通道后，只确认 A 的恢复，会移除 uncertain[1] 中合并的 A、B 记录，再次误归还 B 的额度。

影响：同一 Runtime 内 owner 区分已修复，但跨 Runtime 仍可能越界确认并突破 8 通道限制。范围是 Rust 核心公开创建路径；C ABI 使用进程级 Registry ID，不能把该碰撞直接泛化为所有 C SDK 调用都会发生。

建议：预算使用独立、进程级唯一且不复用的所有者 token，或包含 Runtime 唯一身份的复合键；不要直接将调用方/Runtime 作用域的 Client ID 用作全局所有者。

建议测试：两个 Runtime 的首个 Client 连接同一控制器，分别保留不确定占用；确认其中一个时，另一个的占用必须保持，且预算 owner 身份必须不同。

## W3 / P1：清理监督线程未纳入卸载屏障，存在执行已卸载代码的条件性 UB 风险

位置：[lib.rs:83](../crates/pmac-ffi/src/lib.rs#L83)、[lib.rs:63](../crates/pmac-ffi/src/lib.rs#L63)、[runtime.rs:15](../crates/pmac-core/src/runtime.rs#L15)、[abi-contract.md:25](abi-contract.md#L25)。

pmac_runtime_cleanup 创建的 std::thread 直接丢弃 JoinHandle。Runtime 仅保存 cleanup_scheduled 标记，没有保存该监督线程的句柄或完成状态。现有 shutdown/join 负责 Tokio Runtime 和原来的 pmac-shutdown 线程，不包含新 pmac-cleanup 线程；pmac_runtime_release 也没有等待该线程退出。

触发条件：应用安排后台 cleanup，释放子 ID，然后另一个线程显式完成 runtime_shutdown/runtime_release，随后卸载 DLL/so。监督线程可以仍处于 shutdown 返回与下一次 pmac_runtime_release 调用之间，或在释放 ID 后尚未完成自己的返回和析构。原生 Runtime ID 已不可查询，不构成监督线程完全停止的证明。

影响：应用如果把现有 shutdown/release 成功当作卸载屏障，监督线程可能在模块卸载后继续执行其代码或析构逻辑，存在未定义行为风险。仅在卸载动态库的条件下成立；保持模块加载的普通使用不据此判定发生 UB。本轮未进行卸载竞态复现。

建议：把监督线程纳入可等待的库级收尾协议，保留并 join 其句柄；或者明确为监督者执行期持有动态库加载引用并提供安全释放机制。必须有调用方可观察的、涵盖全部库内线程的完成点，不能仅以删除 Runtime ID 表示安全卸载。

建议测试：在监督线程的 shutdown 返回后、release 调用前设置可控门闩；并行执行显式 shutdown/release，证明卸载完成点必须等待监督者退出。实际动态卸载测试应在独立子进程中执行。

## W4 / P2：清理线程启动失败或异常退出后，scheduled 标记不回滚

位置：[runtime.rs:56](../crates/pmac-core/src/runtime.rs#L56)、[lib.rs:85](../crates/pmac-ffi/src/lib.rs#L85)、[lib.rs:110](../crates/pmac-ffi/src/lib.rs#L110)。

schedule_cleanup 在尝试创建监督线程前将 cleanup_scheduled 置为 true。spawn 失败后只返回 Internal，没有清除标记；线程启动成功但 shutdown_clients 遇到非 Pending 错误并提前 return 时也没有恢复可重试状态。

触发场景：系统暂时不能创建线程，第一次 cleanup 返回 Internal；系统恢复后再次 cleanup，schedule_cleanup 看到 true，直接返回 Ok，不再创建线程。或者监督者内部的 shutdown joiner 创建失败，监督者退出，后续 cleanup 同样误报已安排成功。

影响：一次临时失败永久阻断该 Runtime 的自动清理重试。C++ 析构只调用一次 cleanup 并忽略其返回结果，无法据返回 Ok 判断清理确实仍有所有者执行，可能遗留原生 ID。

建议：使用可回滚的调度状态或 RAII 调度令牌，区分未调度、启动中、运行中、完成与失败。启动失败及未完成的异常退出应恢复可重试状态，并保留明确的清理所有者/错误诊断。

建议测试：注入外层线程启动失败、内部 shutdown joiner 启动失败，恢复后再次 cleanup 必须真正启动并最终释放，而不是只返回 Ok。

## 上轮修复核对

| 上轮项目 | 当前冻结源码状态 |
| --- | --- |
| 独立 C ABI/SDK 配置各建预算 | prepare 现在按规范化 host/port 取得共享账本；重建后丢失占用的生命周期缺口见 W1 |
| 一个 Client 确认释放其他 Client 的额度 | uncertain 改为按 owner 记录，常规 C ABI ID 路径已区分；Rust 跨 Runtime owner 冲突见 W2 |
| 重复 Close 报文 | 增加 close_sent，本地关闭及接收 Close 分支已检查状态，不再无条件重复发送 |
| C++ 析构持续循环 | 改为调用后台 cleanup，当前调用线程不再等子句柄；后台清理退出与重试缺口见 W3、W4 |
| 早先 C++98 nativePtr 条件性数据竞争 | 共享缓存/helper 仍保持删除，没有恢复该触发点 |
| BufferUpload 占用在线额度 | 仍保持不计入在线 GPASCII 通道预算 |

上述是源码核对，不表示本轮测试通过。本轮未确认新增的合规缓冲区调用下 Rust FFI 越界或悬空引用；W3 是独立的动态卸载生命周期风险。第三方依赖 unsafe 未做完整逐行证明。

## 范围及交付

- 同一控制器目前按小写 host 字符串加 port 识别；不同地址别名是否映射同一物理控制器仍需要明确的设备标识策略，本轮未将地址别名问题额外计为缺陷。
- 未用人为无效指针、未对齐输出或调用方并发释放输入等违反 ABI 前提的场景充当库自身 UB 证据。
- 所有行号来自冻结工作区快照。链接打开时可能显示用户后来继续修改的文件，应以快照指纹为准。
- 本报告尚未提交。没有改动、提交或覆盖生产源码，也没有把后续工作区变化混入本轮结论。
