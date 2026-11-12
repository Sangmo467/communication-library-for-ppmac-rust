# 本轮收尾状态

更新：2026-11-12。用户要求持续完成实现和测试；72 小时/7 天长稳验收已明确排除。

## 代码审核问题与线程亲和修复结果

原审核报告中的 5 项问题均已修复并转为持久回归，原始触发条件和复现证据保留在 [审核报告](code-review-2026-11-12.md)：

- [x] R1 / P1：持久 Shell 分别等待带请求序号的 stdout/stderr 完成边界；stderr 标记不写尾随换行，避免分包后污染下一请求。受控 SSH 和 POSIX 多请求分帧回归通过。
- [x] R2 / P1：.NET 未完成 Request 在 Dispose 后保留 finalizer，直到原生 ID 释放；真实 DLL 的 Dispose/GC/完成泵交错回归通过。
- [x] R3 / P2：创建前校验 `Semaphore::MAX_PERMITS`，Client 预留 ID 由 RAII 在错误/unwind 时回滚；x86 真实 DLL 连续 1025 次拒绝后仍能创建合法 Client。
- [x] R4 / P2：Python Request/Client/Runtime finalizer 均核对创建 PID；模拟 PID 和 Linux Python 3.12 原生 fork 子进程 GC/退出回归通过，父进程可正常释放。
- [x] R5 / P2：SBOM 记录 Cargo.lock 哈希和目标，打包/验包共同核对依赖名称、版本及 SHA-256；错误版本、checksum、锁哈希和目标的负测均通过。
- [x] 按用户新增要求实现严格调用线程亲和：每线程固定 member 并独占常驻 GPASCII Channel，`read_pool` 不再迁移同一线程；`open prog 1` 与另一线程 `Motor[1].ServoCtrl=1` 的 member 和真实 russh Channel ID 隔离回归通过。

## 当前源码验证与候选

- Windows x64：132 项 Rust 测试通过；Windows x86：此前 130 项通过。格式、Clippy、Release C/C++ 六项 CTest、Python 和 x64 .NET 生命周期通过。x86 .NET 本轮未重跑，原有运行时目录当前不存在。
- Linux x64：131 项 Rust 测试通过；Linux x86：此前 129 项通过。Linux x64 的 Python 3.12 共 19 项通过（含真实 fork），原生生命周期通过。
- 依据在线指令计划新增受控 MT01/MT06/MT08：程序回读隔离、open/close 响应交错及 Channel 耗尽零借用均通过，报告为 `build/reports/mt-controlled-validation.json`。
- Linux ARM64/QEMU：此前 129 项 Rust 测试通过。
- Linux ARMv7/QEMU：128 项通过；既有 `deadline_during_publish_reports_publish_unknown_without_replaying` 的 250ms 预算在进入 publish gate 前到期，1 项失败。该 SFTP 用例与本次 GPASCII 改动无关，未修改它来掩盖失败，也不将 ARMv7 记为全量通过。
- Windows ARM64：Release DLL 交叉重建和 PE 目标校验通过；没有原生运行证据。
- 七个 Release 候选均已重建，通过文件集合、长度、SHA-256、SPDX 内部引用、锁文件/目标来源、逐依赖版本/checksum 和 PE/ELF 身份检查；每个候选的 SBOM 负测也通过。
- 当前源码快照：`c89b623bb469efb2ce4e2e1c48aba0839b21b004ad78e13a47b19011843a7707`；Cargo.lock：`22325345ec78e56bbe09fdc376d8411b970916e7a26feb1a7bf9325f0d5a2f67`。汇总见 `build/reports/final-candidate-validation.json` 和 `build/reports/final-native-artifacts.json`。
- 仓库已有首次提交 `f926556d15972ac3f6e53de91b874b62b2905f19`；最新 MT 测试及文档改动尚未提交，七个 manifest 均记录该提交、源码快照及 `dirtySource=true`。候选完整性不代表实际 CI 已运行或现场验收完成。

## 联机与现场边界

真实 PPMAC 只读短测已有：单 lane 597 次零错；8 线程、2 lane 600 次零错。`P1..8192` 完整返回 8192 项，`list prog 100`、`&1P` 正常，`ecat reset` 单次完整确认。详见 [验证状态](verification.md)。这些是旧路由实现的真机证据，不冒充严格线程亲和版本的联机复测。

`$$$***` 原测试只提交一次并在 60 秒后超时，未重发；重连后的计数变化符合复位现象，但不构成原命令协议确认。修复后的状态机未再次执行真机复位。

当前进程未配置密码或私钥，不能完成新 Release 的联机只读复测。需要在进程环境中提供 `PMAC_PASSWORD` 或 `PMAC_PRIVATE_KEY`/`PMAC_PASSPHRASE` 后才能继续；认证条件缺失不写成控制器功能失败。

尚未执行真实运动、程序加载、删除用户文件或破坏性网络故障。Windows ARM64 原生运行、物理 ARM 板卡、各平台最低 OS、工业网络故障和性能验收仍需对应硬件与现场窗口；本地构建、WSL 和 QEMU 结果不能替代这些证据。

## 后续待办与所需条件

- [ ] 提供进程环境认证配置后，完成严格线程亲和 Release 的只读联机复测并保存结果。
- [ ] 在设备维护窗口验证修复后的无 ACK 复位状态机；保留原超时记录，复位不自动重发。
- [ ] 在台架验证同线程 `open prog`/程序行/`close` 与其他线程控制命令的真实设备隔离；本地受控 Channel 回归已通过，但真机未执行程序加载。
- [ ] 处理或单独签收 ARMv7 QEMU 的上述 SFTP 时序用例，保留当前失败记录。
- [ ] 补齐 Windows ARM64 原生运行、物理 ARM 板卡、Linux ARM64 C++ 样例、x86 .NET 复测及各平台 SDK/最低 OS 的缺失证据，具体范围见 [平台矩阵](platform-matrix.md)。
- [ ] 在相应台架和现场窗口补齐运动/PLC/程序加载、文件发布、工业网络故障及同条件性能验收，详见 [验证状态](verification.md)。
- [ ] 落实可用范围内的 Loom/Miri/Sanitizer、持续模糊测试和更广泛 GC/进程退出覆盖，并取得实际 CI 成功记录。

dpkg 与 72 小时/7 天长稳不属于本轮待办，不启动后台长测试。
