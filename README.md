# pmac：Rust + russh 的 PPMAC 通信库

这是按 [重构实施计划](2026-11-11-rust-russh-rearchitecture.md) 建立的独立仓库。库版本为 **2.0.0 开发版，C ABI 2.1**，使用 `pmac_` 前缀，不兼容旧 API/ABI；dpkg 安装按范围排除。

Windows x64 的 132 项和 x86 此前的 130 项 Rust 测试通过，Release C/C++/Python 与 x64 .NET 回归通过；Linux x64 的 131 项、x86/ARM64 此前的各 129 项 Rust 测试通过，ARMv7/QEMU 为 128 通过、1 项既有 SFTP 时序测试失败，Windows ARM64 已完成 Release 交叉构建。严格线程—GPASCII Channel 亲和、维护屏障、故障隔离、原生完成通知及三语言 SDK 已实现。模拟运行、真实控制器短测和完整工业工况的证据分开登记，见 [验证状态](docs/verification.md)。本轮按用户要求排除 72 小时/7 天长稳，不启动后台长测试。

## 架构

- `pmac-protocol`：不依赖 Tokio 的增量 GPASCII/Shell 解码、完整目标命令、严格数值解析、状态位和字节正则。
- `pmac-core`：共享 Tokio Runtime、有界准入、单次终态发布、取消/关闭、设备维护屏障和显式 GPASCII 池。
- `pmac-transport-russh`：独立 SSH 成员、持续 GPASCII、独立维护 SSH、持久 Shell/隔离 exec、SFTP staging 与发布保护。
- `pmac-ffi`：C ABI、输入复制/指针校验、不透明 ID、请求结果和池管理导出。
- `pmac-testkit`：受控传输、gate/barrier、故障与生命周期回归。
- `bindings/cpp`：C++17 RAII/异步请求及 C++98 同步适配。
- `bindings/dotnet`、`bindings/python`：.NET 8 Task/CancellationToken 和 Python >=3.9 ctypes/asyncio；每 Runtime 一个有界完成泵。

依赖锁定 russh 0.65.1、russh-sftp 3.0.1。为保留 RSA 并修复依赖审计问题，工作区使用带来源、逐文件哈希和完整性门禁的 [russh 最小 vendor 补丁](docs/dependency-patches.md)。Rust 工具链固定为 1.99.1。

## 构建与本机验证

Windows 使用 Rust 1.99.1、MSVC C/C++、PowerShell 7、CMake、.NET 8 和 Python。先配置相应编译器与可执行文件路径。

```powershell
cargo build -p pmac-ffi --locked
cargo test --workspace --locked
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings

cmake -S . -B build/native -A x64
cmake --build build/native --config Debug
ctest --test-dir build/native -C Debug --output-on-failure

$env:PMAC_LIBRARY = (Resolve-Path 'target/debug/pmac.dll').Path
$env:PYTHONPATH = Join-Path $PWD 'bindings/python'
dotnet run --project examples/dotnet/Pmac.Smoke.csproj
python -B -m unittest discover -s bindings/python/tests -v
python -B examples/python/lifecycle.py
```

公开头为 `include/pmac.h`。本机默认产物在 `target/debug`；指定 Rust target 时位于 `target/<triple>/<profile>`。Linux 使用 `libpmac.so`，CMake 配置省略 `-A x64`。SDK 烟测支持 `PMAC_LIBRARY` 指定待测库，避免误加载其他构建。

七个 target 的构建、ABI、SDK 和候选包流程已配置到 [CI](.github/workflows/ci.yml)。配置存在不代表作业已经成功运行。各平台证据见 [平台矩阵](docs/platform-matrix.md)，本地 ABI、SBOM、安全审计和打包命令见 [验证工具](docs/release-validation-tools.md) 与 [候选发布清单](docs/release-checklist.md)。

## 密码、密钥和异步 GetResponse

密码登录设置 `password`；密钥登录设置私钥路径，加密私钥另设 `passphrase`。同时配置私钥和密码时只尝试私钥，不自动兜底密码。主机身份必须选择 known_hosts、SHA-256 固定指纹，或显式的风险选项。实际凭据通过调用方配置或进程环境提供，不写入日志和报告。

```cpp
#include <cstdlib>

pmac::ConnectOptions options;
options.host = "192.0.2.10";
options.username = "root";
if (const char* value = std::getenv("PMAC_PASSWORD")) options.password = value;
if (const char* value = std::getenv("PMAC_PRIVATE_KEY")) options.privateKey = value;
if (const char* value = std::getenv("PMAC_PASSPHRASE")) options.passphrase = value;
options.knownHosts = "known_hosts";
client.connectSync(options);

pmac::Request request = client.getResponseAsync("#1p");
request.wait();
auto response = request.response();
```

C 入口为 `pmac_get_response_async`，C# 为 `GetResponseAsync`，Python 为 `get_response_async`。同步 GetResponse 只等待同一个请求；响应、stderr、错误和正则捕获可重复复制，不会二次发送。认证细节见 [认证说明](docs/authentication.md)。

## 显式连接池与顺序

应用通过 `pmac_pool_prepare_async`、`pmac_pool_resize_async`、`pmac_pool_drain_async`、`pmac_pool_repair_async` 管理成员；`pmac_connect_submit` 是初次 prepare 的便捷入口。健康成员不会因瞬时并发而自动扩缩容，故障后也不自动补池或重放旧请求。

每个调用线程首次提交 GPASCII 命令时固定一个健康 member，并在该 member 内独占一个常驻 Channel；之后该线程的普通命令和 `read_pool` 请求都保持这一亲和关系，不同线程绝不共用同一个 GPASCII Channel。同一 member 内按接纳顺序执行，不同线程/member 可以并行；跨线程或跨 member 不提供隐式写后读因果顺序或同时刻快照。亲和标识捕获自提交 API 时所在的 OS/Rust 调用线程，不随 Tokio worker 或异步等待迁移。

`PMAC_REQUEST_READ_POOL`（SDK 对应 `RequestOptions.readPool`、`RequestOptions.ReadPool`、`read_pool=True`）仍只允许分类器认可的只读命令，但不再让同一线程跨 member 迁移。`open prog`、程序行和 `close` 必须从同一线程提交；另一线程的电机写入等命令使用自己的 Channel，不会进入该程序缓冲区。成员故障时不把该线程静默迁移到其他 member；修复使用新 epoch/Channel，显式 drain 后才重新分配。线程 Channel 随所属 Session 保留并在关闭时释放。

`pmac_pool_get_state` 和 `pmac_pool_get_members` 返回 revision、ready/busy/creating、失败、退役及远端释放不确定的配额。`pmac_request_get_route` 返回每请求的成员 ID/epoch。故障仅隔离相关成员；结果未知的写操作触发恢复闸门，应用核对后再用匹配 revision 的 `pmac_pool_confirm_recovery` 明确确认。确认远端释放与确认未知写结果是两个独立标志，不能仅凭连接重新 ready 就解除。

save、复位和 buffer 加载经过设备级维护屏障；维护待执行/执行中停止相关新准入，并等待已接纳任务的边界。维护连接故障不自动关闭健康控制成员。维护修复使用 `pmac_pool_repair_maintenance_async`；运行期恢复当前由应用显式发起。

## 生命周期与完成通知

Runtime、Client、Request 都是不复用的非零 64 位 ID。请求拥有输入和结果，`request_wait` 的等待预算与请求总 deadline 分离。排队取消保证 `NOT_SENT`；发送后失去确认返回 `MAY_HAVE_EXECUTED`，库不自动重试可能有副作用的操作。

原生完成通知采用 Runtime 广播序列：`pmac_runtime_wait_completion` 使用每个等待者自己的 cursor；`pmac_runtime_wake` 唤醒订阅/退出检查。通知不消费其他等待者的事件，也不直接替代每请求结果读取。当前 .NET/Python 完成泵由该序列唤醒，不为每个请求创建等待线程；每 Runtime 总跟踪量有界。

`runtime_shutdown` 停止 Client/Request 准入，取消并关闭现有工作，等待 Tokio 和阻塞任务退出。超时返回 `SHUTDOWN_PENDING` 并保留 Runtime ID。完成 shutdown 后仍需释放该 Runtime 的全部 Client/Request ID，`runtime_release` 才能成功。SDK Dispose/shutdown 遇 pending 同样保留 ID，调用方释放子对象后重试；卸载 DLL/so 前必须完成关闭、释放并确保没有 C 调用仍在执行。

## 业务与验证边界

- 电机、坐标系、PLC helper 生成完整目标；NaN/Inf、溢出和歧义原始目标在发送前拒绝。
- 文件采用同目录 staging、流式 MD5 校验和发布保护；发布结果不确定返回 `PUBLISH_UNKNOWN`。
- 持久 Shell 保留 cwd/env，隔离 exec 分别返回 stdout/stderr/退出码；无限执行仍可取消和关闭。
- GPASCII ACK 表示协议交互完整，不等于运动到位或程序完成。Abort/Kill/JogStop 的网络请求不承担硬件安全链路。
- 旧空模式/`number`/`variable` 提取已用旧 C helper 的32例边界样本核对保留；其他模式使用 Rust `regex::bytes`，语义见 [模式迁移](docs/pattern-migration.md)。真实固件维护行为、平台最低 OS/ABI、会话配额和工业网络故障指标仍需对应证据。

逐项进度见 [功能矩阵](docs/feature-matrix.md)、[62 个旧 C / 176 个旧 C++ 声明迁移证据](docs/legacy-symbol-matrix.md) 和 [C ABI 契约](docs/abi-contract.md)。
