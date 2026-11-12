# 当前源码审核（2026-11-12）

结论：发现 5 项待修复问题（2 项 P1、3 项 P2）。现有回归通过不能覆盖这些新场景；主要实现和开发候选已具备，但不应把本轮收尾标记为全部完成。此次仅审核和构造复现，未修改生产源码，未连接真实控制器。

## 范围与验证

按核心调度/池、SSH/SFTP/协议、C ABI、C++/.NET/Python SDK 和发布脚本分区审核当前源码；仓库尚无 Git 提交，因此没有基于提交的差异审核。索引时间早于当前源码，本次结论以直接读取源码和本地复现为依据。

本次重新执行并通过：Windows x64 Rust workspace 126 项测试、Python 17 项单元测试、cargo fmt、workspace/all-targets Clippy（-D warnings）和 vendor 补丁完整性检查。vendor/russh 仍输出 3 条弃用类型别名警告，不影响上述检查退出状态。未重跑七个平台全部回归，未更新在线依赖安全公告库。

复现程序位于忽略目录 build/，不会进入生产源码快照。以下故意失败的新增断言用于证明缺陷，不表示既有 126 项回归失败。

## R1 / P1：持久 Shell 的 stderr 归属错误

位置：[session.rs:340](../crates/pmac-transport-russh/src/session.rs#L340)，返回结果见 347–351 行；[shell.rs:23](../crates/pmac-protocol/src/shell.rs#L23) 的结束标记仅写 stdout。

持久 Shell 读到 stdout 完成标记便发布结果，但 stderr 没有对应完成边界。SSH 服务端先转发 stdout 标记、再转发同一命令的 stderr 时，错误输出会留给下一请求。stdout/stderr 经独立管道读取时，这种顺序合法。

已在生产传输源码的独立副本中，仅调整受控 SSH 服务端发送顺序复现：第一请求实际 stderr 为空；第二请求服务端没有发送 stderr，却得到 `warn-from-first`。主审核者独立重跑，断言按预期失败。

- 日志：[跨请求复现](../build/review-transport/stderr-cross-request-repro.log)。
- 命令：`cargo test --manifest-path build/review-transport/Cargo.toml --target-dir target --offline -p pmac-transport-russh review_persistent_stderr_crosses_request_boundary -- --nocapture`。
- 缺少覆盖：stderr 晚于 stdout 完成标记到达，以及连续请求之间的 stderr 隔离。
- 修复方向：为两个输出流分别建立带请求标识的完成边界，两个边界均确认后再发布；不能依赖固定 sleep 或一次 drain。

## R2 / P1：.NET 未完成请求 Dispose 后可能永久泄漏原生句柄

位置：[Request.cs:94](../bindings/dotnet/Pmac/Request.cs#L94)；完成泵弱引用见 [Runtime.cs:151](../bindings/dotnet/Pmac/Runtime.cs#L151)。

Dispose 取消请求，只有 completed 才释放 ID，却无条件 SuppressFinalize。完成泵只持弱引用；泵处理终态前对象被 GC 后，没有对象负责释放原生 ID，Runtime 最终 release 会持续返回 SHUTDOWN_PENDING（-14）。

通过本地临时 TCP 监听器和受控完成泵交错复现，主审核者独立重跑结果相同：`collected=True`、原生 cancel 返回 0（句柄仍有效）、Shutdown 返回 -14；手动释放保存的泄漏 ID 后 Shutdown 成功。

- 程序：[Program.cs](../build/review-bindings/Program.cs)；日志：[Dispose/GC 复现](../build/review-bindings/dotnet-dispose-gc.log)。
- 命令：`dotnet run --project build/review-bindings/Review.csproj -c Release --no-restore`。
- 缺少覆盖：未完成 Request 的 Dispose、丢弃引用、GC 与完成泵交错。
- 修复方向：原生 ID 释放前保留 finalizer，或由完成泵持有待释放对象的强引用。

## R3 / P2：32 位非法配置会永久耗尽 Client 句柄配额

位置：[helpers.rs:205](../crates/pmac-ffi/src/helpers.rs#L205)、[client.rs:109](../crates/pmac-core/src/client.rs#L109)、[lib.rs:25](../crates/pmac-ffi/src/lib.rs#L25)。

max_file_tasks 只校验非零，32 位传入 0xffffffff 超过 Tokio Semaphore 上限。创建 Client 发生 panic，外层将其转换为 INTERNAL（-26），但此前 reserve 的 ID 没有回滚，调用方得到的 ID 仍为 0。

使用最终 x86 DLL 和 32 位 Python ctypes 复现，主审核者独立重跑：1024 次非法配置均返回 -26；之后默认合法配置也返回 RESOURCE_LIMIT（-12）。Runtime shutdown/release 均成功也不归还这些 Reserved 配额。

- 完整复现代码与结果：[FFI 审核记录](../build/reports/review-ffi-20261112.txt)。
- 缺少覆盖：x86 超出 Semaphore 上限的配置、panic 路径的预留 ID 回滚和重复失败后恢复。
- 修复方向：预留前校验 Semaphore::MAX_PERMITS，并通过 RAII 确保预留在错误及 unwind 时回滚。

## R4 / P2：Python finalizer 绕过 fork 的 PID 保护

位置：[core.py:47](../bindings/python/pmac/core.py#L47)、[core.py:311](../bindings/python/pmac/core.py#L311)、[core.py:406](../bindings/python/pmac/core.py#L406)。

显式 API 检查 PID，三个 weakref finalizer 却直接取消/关闭/释放原生句柄。POSIX fork 子进程中 GC 或解释器正常退出仍会进入继承的 Rust 锁和 Runtime 清理路径，存在阻塞或干扰共享连接的风险。

现有 FakeNative 配合模拟 PID 变化验证：显式 Request.id 返回 -2，GC 却仍释放继承的 Request 11 和 Client 2。此证据证明 finalizer 绕过保护；尚未在 POSIX 原生 fork 环境复现死锁或连接干扰。

- 日志：[PID/finalizer 复现](../build/review-bindings/python-finalizer-pid.log)。
- 缺少覆盖：PID 变化后的 Request/Client/Runtime finalizer，以及实际 POSIX fork 子进程退出。
- 修复方向：注册 finalizer 时保存创建 PID，回调在 PID 不匹配时直接返回；增加原生 Linux fork 验证。

## R5 / P2：错误 SBOM 仍可通过候选打包与验证

位置：[Package-Release.ps1:34](../scripts/Package-Release.ps1#L34)、[Test-Release.ps1:19](../scripts/Test-Release.ps1#L19)。

打包只检查指定 SBOM 存在；验证只检查 SPDX 元数据和内部引用，不核对 SBOM 中版本/校验和与包内 Cargo.lock、目标平台的关联。SBOM 本身被哈希，只能证明打包后未变，不能证明打包时内容正确。

将现有 SBOM 的 async-trait 从 0.1.92 改为 0.0.0-review-stale 后，实际调用打包脚本生成独立探针候选，再执行 Test-Release，仍返回 integrity=passed。主审核者复核该候选内容并重跑校验，结果相同。现有 dist 候选未被修改；该复现不表示它们实际含有错误 SBOM。

- 代码与结果：[FFI/发布审核记录](../build/reports/review-ffi-20261112.txt)。
- 探针候选：`build/review-ffi-sbom/pmac-2.0.0-x86_64-pc-windows-msvc-release`，仅用于反例验证，不作交付。
- 缺少覆盖：过期/错误版本、checksum 和目标不匹配的 SBOM 负测。
- 修复方向：记录并验证 lock/target 来源，核对解析到的依赖版本和 checksum；如需精确目标集合，应与锁定的 cargo metadata 结果比对。

## 仍未完成的工作

1. 修复 R1–R5，加入能覆盖上述交错与非法输入的持久回归，再重跑受影响平台和 SDK 检查；源码改变后重建候选并更新哈希。
2. 为最终 Release 配置进程认证环境并完成只读联机复测；当前缺少凭据，最近认证失败不算控制器功能失败。
3. 在维护窗口验证修复后的无 ACK 复位状态机；原 $$$*** 超时记录保留，不能由重连后的计数变化改记成功。
4. 补齐 Windows ARM64 原生执行、物理 ARM 板卡、Linux ARM64 C++/SDK、最低 OS/glibc 和固件/SSH算法/PTY/SFTP扩展兼容证据。
5. 补齐实际运动/PLC/坐标程序/加载、save、文件权限/磁盘满/发布中断及现场网络 RST/FIN/DROP/rekey 等验证。
6. 补齐约 20 秒初始化、会话名额耗尽/释放、外部工具争用、维护并行负载，以及旧库/worker/新库同条件性能对比。
7. 尚无 Loom/Miri/Sanitizer、持续模糊测试及更广泛 GC/进程退出覆盖的执行证据；需按可用平台落实。
8. 建立首次 Git 提交与后续 CI 成功记录；已有开发候选靠源码哈希追溯，CI 配置存在不等于七个平台 CI 已运行成功。

dpkg 安装和 72 小时/7 天长稳按现有用户范围排除，不重新列为本轮待办。

本次没有把命名 readiness 响应列为缺陷：已用 Sys.ServoCount=123 的受控响应验证通过。其余未报告区域只代表本次未发现已证实问题，不代表全部状态空间和所有平台均已验收。
