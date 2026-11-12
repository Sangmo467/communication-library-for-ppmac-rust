# 验证工具运行说明

## ABI 与本机构建

CMake 会通过 PowerShell 从 `include/pmac.h` 生成 C 布局探针，输出所有公共结构的 `size`、`align` 和逐字段 `offset`。Rust 例子引用真实 `src/types.rs`。新增 ABI 声明后重新生成 Rust 例子：

```powershell
./scripts/Update-AbiProbe.ps1 -RustOutputPath crates/pmac-ffi/examples/abi_layout.rs
cmake -S . -B build/native -A x64
cmake --build build/native --config Debug
ctest --test-dir build/native -C Debug --output-on-failure
ctest --test-dir build/native -C Debug -V -R pmac_abi_layout | Set-Content -Encoding utf8NoBOM build/reports/abi-c.txt
cargo run -p pmac-ffi --example abi_layout --locked | Set-Content -Encoding utf8NoBOM build/reports/abi-rust.txt
./scripts/Compare-Abi.ps1 -CLayoutPath build/reports/abi-c.txt -RustLayoutPath build/reports/abi-rust.txt
```

Linux 配置不传 `-A x64`。显式 Rust target 用 `-DPMAC_RUST_TARGET=<triple>`；32 位目标必须同时选择 32 位 C/C++ 编译器。ARMv7 的 QEMU 设置可参考 CI，但 QEMU 结果只用于功能和布局验证。

## 可选只读短时基准

本轮用户明确排除 72 小时/7 天长稳，不自动启动后台长测试。保留工具用于显式调用的短时功能/并发和基准测试。

`Invoke-Soak.ps1` 驱动 `soak.py`，只提交 `Sys.ServoCount`、`Sys.PhaseTime`、`Sys.ServoTime` 三个只读查询。工具不执行运动、写入、Shell、文件发布或复位，不自动重连/重放。第一个错误会终止运行并返回失败。

凭据只通过进程环境传递。设置 `PMAC_HOST`、`PMAC_USER`、`PMAC_PASSWORD` 或 `PMAC_PRIVATE_KEY`/`PMAC_PASSPHRASE`，再设置 `PMAC_KNOWN_HOSTS` 或 64 位十六进制的 `PMAC_PINNED_SHA256`。设置 `PMAC_LIBRARY` 为待测 DLL/so 的绝对路径。报告不保存密码、私钥路径、主机配置或原始响应。

```powershell
# 初次只读基准：连接完成后测量 30 秒，总请求率约 20/s。
./scripts/Invoke-Soak.ps1 -DurationSeconds 30 -Threads 1 -Rate 20

# 明确验证可用会话配额后，显式创建两条 lane 并选择读池。
./scripts/Invoke-Soak.ps1 -DurationSeconds 60 -Threads 8 -PoolSize 2 -Rate 100

# 无原生库、无网络自检。
./scripts/Invoke-Soak.ps1 -SelfTest
```

可选 `-PythonExecutable` 指定 Python，避免 WindowsApps 占位程序；Python >=3.9。可配置 1/2/8/32 线程、pool 大小、请求期限、速率和输出路径。速率限制采用每工作线程的间隔，启动时允许至多线程数的突发。池大小大于 1 时请求明确使用读池，不把多线程排队冒充多 lane 并行。

JSONL 报告包含库版本、连接用时、成功数/错误数、最大延迟、CPU 时间、RSS 和固定 4096 样本 reservoir 的分位数估计。分位数是估计值，最大值为全量观测。单请求延迟用 `perf_counter_ns()` 高分辨率时钟，报告附实现和分辨率；低分辨率 `monotonic()` 仅用于运行期限和限速。报告每 30 秒追加一次；采样内存不会随请求数增长。Ctrl+C 会标记中断，不能将中断报告当成请求时段已经完整执行。

该工具核对请求 ID、严格数值语法，以及响应携带名称时的名称匹配。它不能单独证明机械动作完成、所有电机/坐标系目标归属、网络故障恢复或安全链路正确；这些仍由确定性模拟、故障注入和台架用例补齐。比较不同库时需额外统一 RTT、控制器负载、电源模式及查询组合。
