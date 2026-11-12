# Linux x64 SDK 本机验证

2026-11-12在Windows x64主机的Ubuntu 24.04 WSL中实际执行。此记录是Linux x64原生库与托管SDK的开发验证，不代替物理Linux主机、最低内核/glibc或其他CPU架构的验收。

| 检查 | 实际结果 | 本地工件 |
|---|---|---|
| Python 3.12.3单元测试 | Debug与Release均17通过，exit0 | `build/reports/linux-sdk/python-unittest.log`、`python-unittest-release-final.log` |
| Python原生生命周期 | 768同步终态负测、32异步提交、pool、完成cursor、retryable release、GC通过；Debug与Release均exit0 | `python-lifecycle.log`、`python-lifecycle-release-final.log` |
| .NET原生生命周期 | 同规模负测与异步提交、认证参数、pool、完成通知、准入、retryable release、GC通过；Debug与Release均exit0 | `dotnet-lifecycle.log`、`dotnet-lifecycle-release-final.log` |
| .NET并发Dispose | 64个Client，各8工作者每人16次Close/Dispose/State；8路Runtime.Dispose通过 | 与上行同一smoke和日志 |
| 最终Rust/C/C++/ABI | 125项Rust通过；Release的C、C++17、C++98及两种NoThrow样例通过；122项C/Rust ABI值一致 | `build/reports/linux-x64/rust-tests-final.log`、`build/reports/linux-c-cpp-abi-final.log` |

最终Release库为 `build/linux-artifacts/x86_64-unknown-linux-gnu/release/libpmac.so`，7,928,840字节，SHA-256为 `537e3fcab00192fb088c1c194da9fdda04ec5120ff2c14f20fcffa5067a923b0`。全部七个Release产物的机器类型和哈希见 `build/reports/final-native-artifacts.json`。此前Debug SDK摘要仍保存在 `build/reports/linux-sdk/summary.json`，不与最终Release哈希混用。

.NET使用Windows端已编译的AnyCPU `Pmac.Smoke.dll`，通过 `PMAC_LIBRARY` 显式加载Linux原生库。只下载官方便携.NET 8.0.34 Linux x64 runtime，没有安装.NET SDK或修改系统配置。下载来源为[微软8.0发布元数据](https://builds.dotnet.microsoft.com/dotnet/release-metadata/8.0/releases.json)，实际归档的SHA-512与该条runtime文件元数据一致：

```text
3949c533b04b0f2b8ed7f6a595409806857891de07eca5ff2795c3ffecc5c373c8977ce296ea1124cef0890f8afeb3365a3bb8514d11a91ba79adb5c47c27ddb
```

运行时位于忽略目录 `build/tools/dotnet-runtime-8.0.34-linux-x64`，独立校验记录为其中的 `download-verification.json`。测试没有连接真实控制器，没有启动72小时/7天长稳。
