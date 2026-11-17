# 当前工作区代码审核（2026-11-13）

范围：E:\PMAC\comm-rust-russh，master，HEAD 4cd443c，包含已有未提交和未跟踪源码。此次只审核，未修复业务代码、未提交或推送。按协议、请求/连接池、传输、FFI/Gather 和语言绑定分区检查；不代表穷尽全部路径。

## 确认问题

### 1. P1：复合 close all buffers 不会清除本地程序上下文

位置：crates/pmac-core/src/client.rs:651–657（整条文本比较）、746–773（上下文清除/失效）。

触发：线程 A 成功确认 open prog 1；线程 B 提交并确认 P1;close all buffers 或 close all buffers;P1；线程 A 继续提交程序正文。

Command::raw 支持分号分段，两个复合形式都被正确分类为 Maintenance 并允许执行。但 execute_data 的 closes_all 标志只对整条文本分词后严格等于 close/all/buffers 的情况成立。复合命令的远端全局关闭与本地状态跟踪因此分离，旧程序上下文未清除；错误分支也不会进入对应的全局退役处理。

受控 MockTransport 实验结果：

```text
close="close all buffers", effect=Maintenance
stale dwell accepted as program: false
close="P1;close all buffers", effect=Maintenance
stale dwell accepted as program: true
close="close all buffers;P1", effect=Maintenance
stale dwell accepted as program: true
```

实验确认了本地状态缺陷，未在真实控制器上执行全局关闭或运动。远端全局关闭成功后，旧正文可能被发送到已退出程序模式的 Channel，发生拒绝或按在线语义执行。该行为违反 docs/online-commands.md 所述全局关闭清除全部上下文的契约。

建议：在协议解析结果中保留全局关闭语义，或按与 Command::raw 一致的在线分段规则检测；处理 cpx/cx 正文不透明边界，避免把正文里的字面量当成维护命令。也可明确拒绝复合全局关闭。增加成功/失败及跨线程回归。

复现程序：target/audit-compound-close/main.rs（忽略目录，使用当前生产 crate）。

```powershell
$env:PATH='C:\Users\15503\.cargo\bin;'+$env:PATH
cargo run --manifest-path target/audit-compound-close/Cargo.toml --offline --target-dir target --quiet
```

### 2. P2：Python 请求与连接参数仍被 ctypes 静默截断

位置：bindings/python/pmac/core.py:60–65、459–461。

现有 _checked_integer 已覆盖部分池管理和业务参数，但 options() 和 _connect_options() 仍直接构造原生整数。Python 的任意精度整数超出目标字段范围后被取模转换，原生校验看到的是转换后的值。

Python 3.12 本地实测：

```text
timeout_ms=4294967297 -> 1
max_response_bytes=-1 -> 18446744073709551615
port=65558 -> 22
control_pool_size=257 -> 1
```

影响：请求可能意外在 1ms 超时；负响应上限变成极大预算并引起资源错误；连接可能访问与调用者指定不同的端口，池大小也发生变化。没有实际连接错误端口。

建议：构造 ctypes 结构之前，对全部整数配置字段执行类型和范围校验，统一使用 _checked_integer；测试范围上界、负数和非整数输入。

## 未计入确认 bug 的观察

Gather 的 TCP accept 在 crates/pmac-ffi/src/gather.rs:438 忽略 peer 地址，首个连接即成为数据源。可访问监听端口的其他主机可能抢占并注入格式有效的数字。文档已声明这是普通 TCP 链路，因此此项单独作为数据来源完整性边界记录；未进行攻击或真实控制器验证，不混入已复现的缺陷数量。未确认正常约定内的 Gather 停止/释放生命周期缺陷。
## GitNexus 重建与影响分析

执行 node .gitnexus/run.cjs analyze --force --index-only，使用 GitNexus 1.6.12。首次运行重建了图但 FTS checkpoint 失败，且用户目录 registry.json 更新被沙箱拒绝；通过权限审查后完整重试成功，恢复了上次未完成状态。最终结果：3,862 节点、10,598 边、133 clusters、339 已枚举流程。status 显示覆盖的 130 个文件全部匹配当前内容，up-to-date。--index-only 未更新仓库 AGENTS.md/CLAUDE.md。

query/context 用于定位请求执行、程序缓冲区和绑定调用；execute_data 的 upstream impact：LOW，直接调用者 execute，影响 1 个模块，当前枚举流程关联为 0。该结果仅反映已记录图，不能将未枚举流程理解为不受影响。

detect_changes(scope=all)：262 个变更符号、110 个受影响流程、37 个已跟踪变更文件，risk_level=critical；返回未标记 partial/truncated。它描述审核开始前已有的整体未提交差异，不是此次引入的代码修改或缺陷数。未跟踪源码另通过源码检查与编译测试覆盖。

索引明确提示覆盖限制：59 处跨语言字段访问未关联；流程候选有 630 个未入选、68 个未展开、123 个去重流程因上限丢弃，361 个 callee 未遍历、14 个遍历因预算截断。因此 339 是已枚举数量，不能称为完整流程总数。此次没有将 UNKNOWN 或空图结果作为安全证据，也没有进行需 PDG 层支持的完整 taint/security 审核。

## 验证结果

- cargo test --workspace --locked --offline --quiet：201 项通过（11+28+57+46+59）。
- cargo fmt --all --check：通过。
- cargo clippy --workspace --all-targets --locked --offline -- -D warnings：通过。
- Python 3.12 unittest discover：24 项，23 通过、1 项 POSIX fork 测试在 Windows 跳过。
- scripts/Test-DependencyPatch.ps1：通过，91 个 vendor 文件校验一致。
- git diff --check：通过（Git 输出既有文件的换行转换提示）。
- 复合全局关闭：受控 MockTransport 实验确认本地上下文残留；未模拟控制器实际执行正文。
- Python 参数截断：直接构造当前绑定结构复现；不需要连接设备。

正式测试全部通过仍未覆盖上述两个边界。未执行真实控制器运动、物理断网、长时间压力测试、Linux/其他平台运行或完整跨语言 ABI 运行测试。新增审核报告及忽略目录内复现 harness；业务源码与已有修改保持原状。所有本轮主动文本写入使用 PowerShell，UTF-8 无 BOM。
## 后续修复

已按用户要求修复上述两个确认问题。全局关闭检测按在线分段执行，并保留 cpx/cx 的正文边界；成功后清除上下文，失败后退役成员。Python 在 ctypes 转换前校验请求及连接配置整数字段。

新增回归先复现旧行为，再验证修复；Rust workspace 203项通过；Python 26项（25通过、1跳过）；fmt、Clippy和diff检查通过。Gather未认证TCP来源观察仍未修改。未提交或推送。