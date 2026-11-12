# 构建平台与验收范围

版本固定以 `rust-toolchain.toml`、`Cargo.lock` 和候选包 `manifest.json` 为准。CI 的七个 target 是待验证交付目标；工作流文件存在、交叉编译通过或 QEMU 通过都不能替代对应系统和控制器的现场签收。

| Rust target | CI 承载方式 | 自动验证范围 | 尚需证据 |
|---|---|---|---|
| x86_64-pc-windows-msvc | Windows Server 2022 x64 | Rust、C ABI/C++17/C++98、Python、.NET | 最低 Windows 版本与真机 |
| i686-pc-windows-msvc | Windows Server 2022 WoW64 | 32 位 Rust、C ABI/C++17/C++98、32 位 Python/独立 x86 .NET | 地址空间压力、真机 |
| aarch64-pc-windows-msvc | Windows 11 ARM64 | 原生 ARM64 Rust、C ABI/C++、Python、.NET | 工控机/板卡、驱动与性能 |
| x86_64-unknown-linux-gnu | Ubuntu 24.04 x64 | Rust、C ABI/C++、Python、.NET | 最低内核/glibc 与真机 |
| i686-unknown-linux-gnu | Ubuntu 24.04 x64 + multilib | 32 位 Rust 与 C ABI/C++执行 | 32 位 Linux 镜像、SDK 与真机 |
| armv7-unknown-linux-gnueabihf | Ubuntu 24.04 + 交叉工具链/QEMU | hard-float 构建、Rust/C ABI/C++模拟执行 | ARMv7 板卡、SDK、真实尾延迟 |
| aarch64-unknown-linux-gnu | Ubuntu 24.04 ARM64 | 原生 ARM64 Rust、C ABI/C++、Python、.NET | ARM64 板卡与真实控制器 |

最终源码的本机验证已覆盖：Windows x64/x86各126项Rust及Debug/Release原生和SDK回归；Linux x64/x86各125项Rust、Release C/C++与ABI；ARMv7/ARM64 QEMU各125项Rust及Release ABI；Windows ARM64 Release交叉构建。ARMv7本地QEMU还执行了C/C++，ARM64本地QEMU执行了C；Windows ARM64没有原生运行证据。逐项范围和日志见 [验证状态](verification.md)、[Windows x86记录](platform-local-verification.md)及[Linux SDK记录](linux-sdk-verification.md)。

本轮按用户要求排除 72 小时/7 天长稳；不以缺少长稳报告单独阻断候选包。上述是本地开发验证，不是七个GitHub CI作业均已成功的证明。实际CI结果应保存在CI artifact，并关联源码提交、工具链和Cargo.lock哈希。Windows ARM32、ARM soft-float、ARMv6、musl和大端不在该矩阵。

C#当前使用.NET 8。其[官方支持表](https://github.com/dotnet/core/blob/main/release-notes/8.0/supported-os.md)不包含Linux x86，并明确不支持通过QEMU运行.NET；因此Linux x86的C# SDK尚无当前运行时路径，ARMv7的C#需原生板卡验证，不能由QEMU作业代签。Windows x86可以使用独立x86 .NET运行时验证。

最低系统版本、控制器固件、SSH算法交集、PTY要求、SFTP扩展、会话配额以及各板卡的性能档位仍需要P0台架实测冻结。不要把Ubuntu 24.04的构建成功解释成兼容任意旧glibc；如需旧系统，应增加固定sysroot/构建镜像及该系统的运行测试。ARMv7本次交叉链接器来自Ubuntu 18.04，QEMU运行使用其sysroot，这同样不构成旧系统完整验收。

GitHub runner标签依据[官方runner清单](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)。Rust自身的target等级见[Rust平台支持表](https://doc.rust-lang.org/rustc/platform-support.html)；它不构成本通信库及其依赖已通过平台验收的证明。
