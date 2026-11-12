# 工作区快照 50dd3443ac23 复审

日期：2026-11-12。冻结时间：2026-11-12T10:54:10.574Z。

46 个文件的快照指纹（SHA-256）：`50dd3443ac237b2584184afa40149e48a71ca4e85b97504654697ab922bc82d3`。逐文件读取 UTF-8 并哈希，再完整读取一次确认一致，后续只分析冻结内容。

使用 GitNexus MCP detect_changes、context、impact 辅助定位；本轮变更映射返回 5 个文件，未报告 partial/truncated。图索引落后 f8d5922 一个提交，未索引的新 helper 以冻结源码为准。未使用 CLI 分析或更新索引，未执行测试或动态 UB 检测，未连接真实控制器。

结论：2 项 P1，分别是原生拒绝释放时 C++ 丢失句柄，以及原生释放检查尚未覆盖完整回收生命周期。两项发生在不同分支，不重复计数。上轮未执行任务取消保护已修复。

## R1 / P1：C++ reset/析构忽略新的释放拒绝，丢弃仍须恢复的 Client ID

位置：[pmac.hpp:277](../bindings/cpp/pmac.hpp#L277)、[pmac_cpp98.hpp:88](../bindings/cpp/pmac_cpp98.hpp#L88)、[lib.rs:274](../crates/pmac-ffi/src/lib.rs#L274)。

新 pmac_client_release 会在 Pool.release_uncertain 非零时返回 ShutdownPending 并保留原生 ID，这是必要保护。但 C++17 Client.reset 无条件忽略 close 和 release 的返回值，随后将 id_ 设为 0、释放 runtime_；C++98 析构同样忽略失败。

触发场景：Channel Close 未确认，新关闭路径将成员计入 Pool.uncertain；应用调用 C++ reset 或销毁 Client。原生层正确拒绝释放，包装层却放弃 ID。若应用没有另行保存原生 ID，SDK 对象不再提供确认该 owner 或重试释放的入口。

影响：Client 注册表条目和不确定预算被保留，却丢失了可操作的 C++ 所有者。随后 Runtime 清理监督者会持续等 has_children 归零，cleanup_wait 也可能一直等待。相比原先直接删除 ID，当前保护避免静默清账，但包装层必须同步处理拒绝。

建议：显式 reset/close/release 应报告 Pending 并保留 id_ 和 Runtime 所有者，直到恢复确认和释放成功。析构中的失败不能简单丢弃，应移交给可查询且可恢复的持有者；不能自动确认远端释放，也不能无限阻塞析构线程。

建议测试：让原生 release 返回 ShutdownPending，验证显式 reset 不把 id 清零，恢复确认后能重试成功；另测析构、移动赋值触发 reset 以及 C++98 析构的失败所有权移交。

## R2 / P1：原生释放只看 Pool 当前计数，仍可能在回收未完成时删除 owner

位置：[lib.rs:276](../crates/pmac-ffi/src/lib.rs#L276)、[client.rs:317](../crates/pmac-core/src/client.rs#L317)、[pool.rs:229](../crates/pmac-core/src/pool.rs#L229)、[session.rs:579](../crates/pmac-transport-russh/src/session.rs#L579)、[session.rs:664](../crates/pmac-transport-russh/src/session.rs#L664)。

释放保护只读取 PoolSnapshot.release_uncertain，即 Pool.uncertain.len，没有检查关闭是否已完成、owner 是否还有回收中的通道，或预算中是否已有该 owner 的 uncertain。检查与 Registry.remove 也不组成统一生命周期操作。

路径 A：close 仍在等待 Channel Close，调用方的 1000ms 等待先返回 Pending；Pool.uncertain 要等 session.close 返回后才登记。此时 release_uncertain 仍为 0，pmac_client_release 可以返回 Ok 并删除 ID；稍后 2 秒关闭确认超时，再登记不确定占用时，恢复 ID 已经消失。C++ reset 和部分 finalizer 具有这种 close 后立即 release 的调用顺序。

路径 B：线程退出回收先将 Channel 从 control_channels 移除，再通过 Drop 派生后台回收任务。该任务确认失败时只把 permit 转为预算 uncertain，不反馈 Pool.uncertain；其 bool 结果被丢弃。后续 Client.close 已看不到被移出的通道，可能关闭成功且公开计数仍为 0，release 同样删除仍有预算占用的 owner。

影响：即便已增加 Closed Client 手动确认能力，在这两条路径中原生释放仍可能提前删除 ID，遗留无法通过公开接口确认的旧 owner 额度。R1 是原生拒绝后 C++ 丢 ID，本项则是原生本身错误允许删除，须分别修复。

建议：建立覆盖当前、回收中及不确定通道的 owner 生命周期状态；关闭尚未结算或预算仍有该 owner 未确认资源时，应保留 ID。后台回收结果也须进入公开状态。释放判定和删除要基于稳定的状态/所有权，不能只在某一瞬间采样 Pool 的成员计数。

建议测试：控制 close 先返回 Pending、随后 Channel 确认失败的顺序，在中间尝试 release 必须保留 ID；再测线程退出后台回收超时后，Client 关闭、公开计数和释放结果保持一致。恢复步骤仍须可通过保留的 ID 完成。

## 上轮修复核对与测试覆盖

- 首次 poll 前取消：关闭 helper 现在接收 UncertainPermitOnDrop，Drop 路径在创建/spawn future 前构造保护；原先提前归还额度的触发条件已消除。
- 显式 Session.close 的确认失败：现在返回 ShutdownPending，Client 关闭任务将该成员登记进 Pool.uncertain；这一主路径已补上。
- 释放保护：原生层开始拒绝已有 Pool.uncertain 的 ID；仍需修复 R1 的包装层状态保留和 R2 的后台/未完成回收覆盖。
- 新 unpolled 测试使用 shutdown_background 且丢弃 JoinHandle，没有等待取消完成。此时预算仍被尚未销毁的 permit 占用，也能令部分断言通过；建议 abort 后等待任务完成，再验证 uncertain 与确认后的完整额度，避免调度时序导致假通过。此为测试缺口，不另计生产缺陷。
- 本轮未确认新增 UB。Rust FFI 通过 Arc 保活已取得的对象引用，ID 丢失/提前删除在上述路径中属于配额与恢复所有权问题，不能直接等同于悬空内存。
- BufferUpload 仍不占在线 GPASCII 通道，不同线程的在线通道隔离规则未被本轮修改。

## 交付边界

本轮仅审核和新建本报告，没有提交、修改或覆盖生产源码，报告尚未提交。未重跑测试、动态 UB 检测或控制器验证；新增结论均为冻结源码静态推导。所有行号属于指纹 50dd3443ac23，本地链接可能显示用户之后修改的工作文件。
