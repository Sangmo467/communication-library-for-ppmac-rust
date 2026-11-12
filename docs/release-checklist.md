# 候选包与发布验收

本仓库的打包脚本只生成候选产物，不自动发布、部署或宣告工业可用。正式签收必须同时有实现、测试和目标平台证据。

## 可追溯候选包

使用 PowerShell 7 执行；先在明确 target 上构建并运行验证：

```powershell
cargo build -p pmac-ffi --release --locked --target x86_64-pc-windows-msvc
./scripts/Write-Sbom.ps1 -Target x86_64-pc-windows-msvc
$package = ./scripts/Package-Release.ps1 -Target x86_64-pc-windows-msvc -EvidenceDirectory build/reports
./scripts/Test-Release.ps1 -PackageDirectory $package
```

默认要求存在提交且工作区干净。开发快照可以显式加 `-AllowUncommitted`；清单会记录 `dirtySource=true` 和独立源码快照 SHA-256，不能仅靠提交号识别这种产物。输出目录已存在时拒绝覆盖。

包中包括原生库、公开头文件、SDK 源码、许可证、依赖锁、工具链声明、SPDX 2.3 SBOM、验证证据及 SHA-256 清单。打包时检查 PE/ELF 机器类型和位宽，防止把 x64 库标成 x86/ARM。`Test-Release.ps1` 检查文件集合、长度、哈希、依赖清单引用以及原生目标；校验和检测意外损坏，不代替发布签名或可信分发。

SBOM 从 `cargo metadata --locked --offline` 和 Cargo.lock 生成，包含解析到的开发/构建依赖；`--filter-platform` 用于目标过滤。工作区本地包没有 crates.io 归档校验和，因此其完整性由源码快照和候选包哈希覆盖。SBOM 的许可证声明不等于法律清查结论。

## 正式发布的证据

每一个承诺 target 都需要构建、C/Rust 逐字段 ABI 比对、SDK 功能/生命周期、真实控制器只读烟测、故障恢复、并发及性能报告。额外记录：

- OS、内核/glibc、CPU/ARM ABI、内存、电源模式、网卡、控制器型号/固件、SSH 算法及 SFTP 扩展。
- 1/2/8/32 线程、单设备/多设备、控制与维护并行时的错误、数据归属、p50/p95/p99/p99.9、最大延迟、CPU/RSS、队列与设备负载。
- RST/FIN/单向及双向 DROP、抖动、半开、rekey、服务名额耗尽、20 秒创建、迟到帧、取消/关闭交错及发布中断。
- 本轮用户明确排除 72 小时/7 天长稳，不启动后台长测试，也不将该排除项作为候选发布的单独阻断条件。后续若另行要求长稳，需要新的执行窗口和独立证据。
- `cargo audit`、`cargo deny`、持续模糊测试及可用范围内的 Miri/Sanitizer/Loom。缺少工具、网络或硬件时记录为未执行。

安全门禁用 `./scripts/Invoke-Security.ps1`。工具必须先安装；工具缺失和检查失败都会生成失败报告并使门禁失败。显式跳过会在报告中标记，不能作为完整签收。依赖检查依据 [RustSec](https://rustsec.org/) 与 [cargo-deny 配置文档](https://embarkstudios.github.io/cargo-deny/checks/cfg.html)。

## 切换与回退

先固定旧库、worker 候选和本库三个版本的源码/二进制哈希，再进行限流只读比对。一次只让一个客户端拥有设备控制权；运动、写入、加载和复位不得双发比较。

切换前关闭旧客户端并确认其连接/任务退出，核对设备状态后由新客户端接管。结果未知的写操作先核对，显式解除恢复闸门后才继续冲突动作；不得把连接 ready 当成动作成功。回退同样需要停止新客户端、核对未知操作和设备状态，再恢复旧版本。任何错目标、结果错归、双完成、重复副作用、UAF、数据竞争或静默损坏都阻断发布。


## 本轮开发候选清单（2026-11-12）

七个 Release 开发候选已生成并通过 `Test-Release.ps1` 的文件集合、长度、SHA-256、SPDX 引用和原生机器类型检查。它们都使用 Cargo.lock SHA-256 `22325345ec78e56bbe09fdc376d8411b970916e7a26feb1a7bf9325f0d5a2f67`，源码快照 SHA-256 `2e57adfe549e00b38811489cb328031835c0873379bc44530c14f28ff05322b4`。仓库当前没有提交，因此各 manifest 均明确记录 `dirtySource=true`，不能把该快照当成已提交正式发布。

| target | 候选目录（均位于 `dist/`） | 清单文件数 | SPDX依赖包数 | 本地证据边界 |
|---|---|---:|---:|---|
| x86_64-pc-windows-msvc | `pmac-2.0.0-x86_64-pc-windows-msvc-release` | 49 | 219 | 本机原生/SDK与既有真机短测 |
| i686-pc-windows-msvc | `pmac-2.0.0-i686-pc-windows-msvc-release` | 58 | 218 | WoW64原生/SDK |
| aarch64-pc-windows-msvc | `pmac-2.0.0-aarch64-pc-windows-msvc-release` | 41 | 218 | 仅交叉构建，未原生运行 |
| x86_64-unknown-linux-gnu | `pmac-2.0.0-x86_64-unknown-linux-gnu-release` | 53 | 213 | Ubuntu 24.04 WSL原生/SDK |
| i686-unknown-linux-gnu | `pmac-2.0.0-i686-unknown-linux-gnu-release` | 40 | 212 | WSL multilib原生/C++，无SDK |
| armv7-unknown-linux-gnueabihf | `pmac-2.0.0-armv7-unknown-linux-gnueabihf-release` | 42 | 210 | QEMU/C/C++/ABI，无物理板卡 |
| aarch64-unknown-linux-gnu | `pmac-2.0.0-aarch64-unknown-linux-gnu-release` | 42 | 212 | QEMU/C/ABI，本地无C++样例与SDK |

汇总校验见 `build/reports/final-candidate-validation.json`。每个候选只收集对应目标的日志与摘要，不因同包包含证据而扩大验收范围。候选生成后的文档说明更新不改变manifest记录的源码快照；若Rust、C ABI、SDK、构建脚本、CI或锁文件再变更，必须重新同步验证并重建候选。
