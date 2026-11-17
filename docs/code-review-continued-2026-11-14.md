# 当前工作区继续审核（2026-11-14）

本轮只审核，未修改业务源码，未提交或推送；保留全部既有未提交改动。重点复查 Rust Client 关闭、FFI 释放条件、Python 请求及完成通知，并阅读 geterrors/Gather、SFTP 和部分 C++/.NET 绑定。不是全仓库无缺陷证明。新建报告和忽略目录内的复现文件均通过 PowerShell 直接写入，UTF-8 无 BOM。

## 确认问题

### R1 / P2：关闭重试成功后仍保留本次关闭产生的不确定记录

位置：crates/pmac-core/src/client.rs:576–596；释放保护 crates/pmac-ffi/src/lib.rs:337–348。

Client.close 在一次 Session.close 返回 ShutdownPending（包括被捕获的 close panic）时，将 (member_id, epoch) 放入 pool.uncertain。随后重试即使成功，也只清空 member.session 并设置 Drained，没有消除本次失败引入、随后已被确认关闭的记录。

复现复用现有 pool_tests.rs 的 Fixture 和一次性 close panic 注入，执行 shutdown_clients(5000)。结果：

```text
state=Closed, healthy=false, uncertain=1, unreleased=true
```

shutdown 已成功，模拟 Session 的后续 close 也返回成功，但 has_unreleased_gpascii 仍为 true。pmac_client_release 明确以该条件拒绝释放，因此 C ABI 生命周期仍需额外恢复确认，无法仅凭成功的关闭重试结束。此前新增的 panic 回归只断言 Closing 阶段 uncertain=1 和最终 Closed，未断言最终可释放。该实验使用公开自定义 Transport 场景，没有证明内置 russh 会发生同样的一次性 panic。

建议：按成员和 epoch 区分本次关闭尚待确认的记录与历史遗留记录；后续确认成功时只消除确实已获释放证明的记录，保留其他历史通道和预算不确定占用。增加“先 Pending/panic，后成功，最终允许释放”的断言，不能简单清空全部 uncertain。

复现：target/audit-continued-review/audit.rs；运行以下命令，预期断言失败（用于展示缺陷）：

```powershell
& C:/Users/15503/.cargo/bin/cargo.exe test --manifest-path target/audit-continued-review/Cargo.toml --offline --target-dir target audit_confirmed_close_retains_stale_uncertainty -- --nocapture
```

### R2 / P2：Python 捕获组编号和等待预算仍发生整数截断

位置：bindings/python/pmac/core.py:107–110、150–155；native.py 为相应参数声明 c_uint32。

options() 和连接参数的整数检查已修复，但 Request.match(group) 和 Request.wait(wait_ms) 仍将任意 Python 整数直接传给 ctypes：

```text
group=4294967296 -> C ABI group=0
wait_ms=4294967296 -> C ABI wait_ms=0
```

前者把不存在的捕获组变成完整匹配组，可能返回看似成功但错误的数据；后者把约 49.7 天的等待预算变成立即轮询。负数也会取模；例如 -1 会变成 0xffffffff（原生无限等待标志），因此若要兼容该写法，应明确规定，不能依赖隐式转换。

离线探针使用真实 CFUNCTYPE 的 uint32 参数转换，经过生产 Request.match/wait，记录进入 C ABI 的值；没有连接控制器。建议复用 _checked_integer，或对明确支持的 sentinel 单独处理，并增加越界、负数及类型检查。

复现：target/audit-continued-review/integer_probe.py。
## 优化建议（未做性能基准）

Python _CompletionPump.run（core.py:300–314）逐项遍历 pending[:]，又对每个完成项执行 pending.remove。批量完成时最坏为 O(n²)，并每轮复制列表。可用一趟遍历构建仍未完成的列表，同时为已完成/已回收项各归还一次 semaphore slot。容量已有上限，建议按并发量评估收益；不要牺牲弱引用、释放与异常处理语义。

关闭重试还可以只对尚未确认成功的 Session 重试，避免一个慢成员使其他已关闭成员每 50ms 再次收到 close。这需要保留逐成员确认状态，适合与 R1 的状态管理一起处理。

## 验证与图分析边界

- cargo test -p pmac-core -p pmac-testkit --locked --offline --quiet：62 项通过，文档测试通过。
- Python unittest discover：27 项，26 通过，1 项平台相关测试跳过。
- R1 独立反例：新增的“最终可释放”断言失败，复现结果见上；该失败是发现证据，不算既有测试回归。
- R2 CFUNCTYPE 参数探针：确认两个 2**32 参数均以 0 到达 C ABI。
- 未做控制器连机、Linux 验证或完整 workspace 重测；此次没有业务代码补丁。

GitNexus query/context 用于定位监控、文件和请求调用流程；Client.close upstream impact 返回 UNKNOWN（没有解析到调用者），不能视为低风险。已用文本与源码补证 Runtime.shutdown_clients 和 pmac_client_close 对 Client.close 的调用，以及 pmac_client_release 对 has_unreleased_gpascii 的检查。后续修复至少需要覆盖 Rust shutdown、C ABI client/runtime release 及各语言关闭路径。

GitNexus status 显示 HEAD 与分析器身份一致，但索引因 6 个文档修改、2 个文档新增而 stale；列出的差异没有本轮审查的源码文件。本轮未重建索引，图结果仅作定位线索，确认问题以当前源码与离线反例为准。无源码修改、无提交，未运行提交前 detect_changes。