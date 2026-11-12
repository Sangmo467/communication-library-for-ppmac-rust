# 本机 Windows x86 验证记录

记录日期：2026-11-12（Asia/Shanghai）。本轮为 Windows x64 主机上执行的 Win32/WoW64 开发验证；不代表所有最低 Windows 版本、物理 32 位系统或其他平台已经签收。按用户要求，本轮排除 72 小时/7 天长稳，没有启动后台长测试。

依赖锁 SHA-256：`22325345ec78e56bbe09fdc376d8411b970916e7a26feb1a7bf9325f0d5a2f67`。工具链文件固定 Rust 1.99.1，本机 stable 工具链实际版本相同。目标 `i686-pc-windows-msvc`，MSVC 19.43.34810.0 / Visual Studio 2022。

| 检查 | 最终源码实际结果 | 证据 |
|---|---|---|
| Rust workspace x86 | 126 通过，0 失败：core 5、FFI 11、protocol 40、testkit 35、transport 35 | `build/reports/windows-x86/rust-tests-final.log` |
| CMake Win32 Debug/Release | 均通过，生成实际 PE I386/32 位 DLL | `cmake-build-final.log`、`cmake-build-release-native-final.log` |
| C / C++17 / C++98 / NoThrow / ABI 探针 | Debug 与 Release 各 6/6 CTest 通过 | `ctest-final.log`、`ctest-release-final.log` |
| C/Rust ABI | 122 个 size/align/offset/位宽值逐项一致 | `abi-c-final.txt`、`abi-rust-final.txt` |
| Python x86 | 17 项 unittest；Debug 与 Release 生命周期烟测均通过 | `python-unittest-final.log`、`python-lifecycle-final.log`、`python-lifecycle-release-final.log` |
| C# x86 | 现有 x86 .NET 运行时执行 net8.0 程序；Debug 与 Release 生命周期均通过 | `dotnet-lifecycle-final.log`、`dotnet-lifecycle-release-final.log` |
| Release 原生身份 | 32 位 PE I386；5,954,560 字节；SHA-256 `0a812c8df59c44a1472d1250644bd70f20d00c40057721a6b803285e648b83be` | `build/reports/final-native-artifacts.json` |

SDK 生命周期烟测包含每语言 768 个同步失败请求、32 个异步提交、连接池管理负测、原生完成通知、可重试 release 和 GC；这些计数不是控制器上成功执行的业务命令数。真实 SSH/SFTP/RSA 签名认证的自动化覆盖包含在 35 项 transport 测试中。

## 依赖安全

`build/reports/security/security.json` 为同一锁文件的真实检查结果：cargo-audit 0.23.0 通过，漏洞数 0；cargo-deny 0.20.2 的 advisories、bans、licenses、sources 均通过。重复版本仅按既定策略警告，没有忽略安全公告或放宽 wildcard 门禁。内部 path 依赖已加精确 `=2.0.0` 版本。

原有 RSA 告警通过 [最小依赖补丁](dependency-patches.md) 修复：稳定 RSA/PKCS1、上游 ssh-key rc.12，以及一个借用 ASN.1 类型别名适配。保留 RSA 功能，真实 RSA 签名认证已回归。来源自检确认 90 个上游文件中仅记录的两个文件改变；其余文件与官方归档一致。

## 剩余边界

最终源码的 Linux x64/x86 本机/WSL、ARMv7/ARM64 QEMU 和 Windows ARM64 交叉构建证据见 [验证状态](verification.md)；这些结果不是 GitHub CI 作业已运行的证明。Linux x86 没有 .NET 8 官方运行时路径，ARM 的 .NET 需原生设备，不能用 QEMU 代签。Windows x86 大地址空间压力、最低 OS 范围及各设备档位的性能仍需独立签收。现场只读短测不包含在本机模拟服务计数中。
