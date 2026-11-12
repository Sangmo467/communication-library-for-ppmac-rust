# Linux x64 SDK 本机验证

2026-11-12在Windows x64主机的Ubuntu 24.04 WSL中实际执行。此记录是Linux x64原生库与托管SDK的开发验证，不代替物理Linux主机、最低内核/glibc或其他CPU架构的验收。

| 检查 | 当前结果 | 本地工件 |
|---|---|---|
| Python 3.12.3单元测试 | 当前Release全19项通过，包含模拟PID及真实POSIX fork子进程GC；exit0 | 本次WSL命令结果；旧日志仅对应历史17项 |
| Python原生生命周期 | 当前Release的768同步终态负测、32异步提交、pool、完成cursor、retryable release、GC通过 | 本次WSL命令结果 |
| .NET原生生命周期 | 本轮未重跑；历史Debug/Release、并发Dispose记录不代表当前源码 | 既有 `dotnet-lifecycle-*.log` 仅为历史证据 |
| 当前Rust | 129项Rust通过：core 5、FFI 12、protocol 42、testkit 36、transport 34 | 本次WSL命令结果 |
| C/C++/ABI | 本轮未重跑；历史Release五种样例和122项ABI对照保留 | `build/reports/linux-c-cpp-abi-final.log` 仅为历史证据 |

当前Release库为 `target/x86_64-unknown-linux-gnu/release/libpmac.so`，7,943,416字节，SHA-256为 `0060ca738c1493260f5b54707b8a237600a8468e17618f58137bd75f30c9000e`。全部七个Release产物的机器类型和哈希见 `build/reports/final-native-artifacts.json`。此前Debug SDK摘要仍保存在 `build/reports/linux-sdk/summary.json`，不与当前Release哈希混用。

.NET使用Windows端已编译的AnyCPU `Pmac.Smoke.dll`，通过 `PMAC_LIBRARY` 显式加载Linux原生库。只下载官方便携.NET 8.0.34 Linux x64 runtime，没有安装.NET SDK或修改系统配置。下载来源为[微软8.0发布元数据](https://builds.dotnet.microsoft.com/dotnet/release-metadata/8.0/releases.json)，实际归档的SHA-512与该条runtime文件元数据一致：

```text
3949c533b04b0f2b8ed7f6a595409806857891de07eca5ff2795c3ffecc5c373c8977ce296ea1124cef0890f8afeb3365a3bb8514d11a91ba79adb5c47c27ddb
```

运行时位于忽略目录 `build/tools/dotnet-runtime-8.0.34-linux-x64`，独立校验记录为其中的 `download-verification.json`。测试没有连接真实控制器，没有启动72小时/7天长稳。
