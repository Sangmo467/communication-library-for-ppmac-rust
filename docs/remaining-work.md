# 本轮收尾状态

更新：2026-11-12。用户要求持续完成实现和测试；72 小时/7 天长稳验收已明确排除。

## 代码审核后的新增待办（2026-11-12）

本次发现 5 项待修复问题，详细触发条件、源码位置和复现证据见 [审核报告](code-review-2026-11-12.md)。主要实现和既有回归已完成，当前仍需完成以下修复与补充回归：

- [ ] R1 / P1：持久 Shell 的 stderr 会错归下一请求；已受控 SSH 复现。
- [ ] R2 / P1：.NET 未完成 Request 在 Dispose 后 GC 会泄漏原生 ID，Runtime release 持续 pending；已真实 DLL 复现。
- [ ] R3 / P2：32 位超大 max_file_tasks 触发 panic 后不回滚 Reserved ID，1024 次后合法 Client 创建也失败；已 x86 DLL 复现。
- [ ] R4 / P2：Python finalizer 未检查 PID，绕过 fork 保护；已模拟 PID 验证，实际 POSIX fork 后果待验。
- [ ] R5 / P2：SBOM 依赖版本与 Cargo.lock 不一致仍可通过打包校验；已独立探针候选复现。
- [ ] 将复现转为持久回归；修复后重跑受影响测试、重建候选并更新源码/产物哈希。

本次重跑的 Windows x64 Rust 126 项、Python 17 项、格式、Clippy 及 vendor 补丁完整性检查均通过。审核发现的额外反例仍失败，未修复生产源码。完整现场证据之外，还需落实可用范围内的 Loom/Miri/Sanitizer、持续模糊测试、更广泛 GC/进程退出覆盖，以及首次 Git 提交和实际 CI 运行记录。dpkg 与 72 小时/7 天长稳继续排除。

## 前次进度核对（2026-11-12，代码审核前）

主要实现、既有本地回归和七个平台 Release 开发候选已完成；本次审核新增的修复仍待完成，联机只读复测受认证条件限制，现场与硬件验收按下方待办分别记录。

- [x] 本次重新执行 `scripts/Test-Release.ps1`，七个候选包全部通过文件集合、长度、SHA-256、SPDX 依赖引用及 PE/ELF 目标身份检查。候选位置见 [汇总记录](../build/reports/final-candidate-validation.json)。
- [x] 重新计算当前源码快照，与七个候选记录一致：`2e57adfe549e00b38811489cb328031835c0873379bc44530c14f28ff05322b4`。
- [x] 当前 `Cargo.lock` SHA-256 与候选及验证记录一致：`22325345ec78e56bbe09fdc376d8411b970916e7a26feb1a7bf9325f0d5a2f67`。
- [x] 前次核对各平台汇总与最近联机失败记录，当时未重跑 Rust/SDK 回归，未连接或操作控制器；本次审核的重跑结果见上节。
- [ ] 最终 Release 联机只读复测：当前进程未设置 `PMAC_PASSWORD`、`PMAC_PRIVATE_KEY`、`PMAC_PASSPHRASE`；最近尝试记录为 `PmacError/status=-1`，见 [联机记录](../build/reports/online/final-readonly-release.jsonl)。

仓库当前尚无首次 Git 提交；七个候选均记录 `dirtySource=true`，使用源码快照哈希追溯。本次进度文档更新不在打包脚本的源码快照范围内，不改变上述源码快照匹配结果。

## 已完成的最终源码验证

- [x] Windows x64/x86 各 126 项 Rust 测试；Debug/Release 原生、C/C++、Python/.NET 生命周期回归。
- [x] Linux x64/x86 各 125 项 Rust 测试；Release C/C++17/C++98、NoThrow 和 122 项 ABI 对照；Linux x64 Python/.NET Debug/Release 回归。
- [x] Linux ARMv7/ARM64 在 QEMU 下各 125 项 Rust 测试及 Release ABI 对照；ARMv7 另通过五种 C/C++ 样例，ARM64 通过 C 样例。
- [x] Windows ARM64 Release DLL 交叉构建与 PE ARM64 身份检查；没有伪称原生运行。
- [x] 无 ACK 复位的精确 readiness 标记与只读探测状态机；不重发复位，不以静默或 EOF 冒充确认。修复仅完成模拟回归，未再次真机复位。
- [x] `&1P`、批量 P 变量读取、`list prog 100` 和旧提取模式回归。
- [x] 并发取消、错误快照、句柄释放、完成通知、池管理和维护屏障回归。
- [x] Rust Clippy、格式检查、cargo-audit 零漏洞和 cargo-deny 全类别通过。
- [x] 七个 Release 开发候选的 SBOM、源码/锁文件快照、逐文件 SHA-256 和原生目标校验，见 [候选清单](release-checklist.md)。
- [x] 文档、功能矩阵与实际平台证据同步，区分构建、模拟、原生和真实设备证据。

## 联机与现场边界

真实 PPMAC 只读短测已有：单 lane 597 次零错；8 线程、2 lane 600 次零错。`P1..8192` 完整返回 8192 项，`list prog 100`、`&1P` 正常，`ecat reset` 单次完整确认。详见 [验证状态](verification.md)。

`$$$***` 原测试只提交一次并在 60 秒后超时，未重发；重连后的计数变化符合复位现象，但不构成原命令协议确认。修复后的状态机未再次执行真机复位。

用户已允许本轮联机测试。最终 Release 的只读联机复测已尝试，设备 SSH 端口可达，但当前进程未配置密码或私钥，认证返回 `-1`，没有执行设备命令。需要在进程环境中提供 `PMAC_PASSWORD` 或 `PMAC_PRIVATE_KEY`/`PMAC_PASSPHRASE` 后才能继续；不把这次认证失败写成控制器功能失败。

尚未执行真实运动、程序加载、删除用户文件或破坏性网络故障。Windows ARM64 原生运行、物理 ARM 板卡、各平台最低 OS、工业网络故障和性能验收仍需对应硬件与现场窗口；本地构建、WSL 和 QEMU 结果不能替代这些证据。

## 后续待办与所需条件

- [ ] 提供进程环境认证配置后，完成最终 Release 的只读联机复测并保存结果。
- [ ] 在设备维护窗口验证修复后的无 ACK 复位状态机；保留原超时记录，复位不自动重发。
- [ ] 补齐 Windows ARM64 原生运行、物理 ARM 板卡、Linux ARM64 C++ 样例及各平台 SDK/最低 OS 的缺失证据，具体范围见 [平台矩阵](platform-matrix.md)。
- [ ] 在相应台架和现场窗口补齐运动/PLC/程序加载、文件发布、工业网络故障及同条件性能验收，详见 [验证状态](verification.md)。

72 小时/7 天长稳不属于本轮待办，不启动后台长测试。
