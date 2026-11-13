# C# / .NET GetResponse 接入

GetResponse 支持按请求启用透传：跳过规则表和 online/program 分类，原文提交单条命令。详见 **[GetResponse 透传模式与各语言示例](getresponse-passthrough.md)**。


本指南优先演示 GetResponse：连接 Power PMAC，发送一次命令，取得完整响应或从同一次响应中提取文本。示例项目为 [Pmac.GetResponse.csproj](../examples/dotnet/getresponse/Pmac.GetResponse.csproj)，主程序为 [Program.cs](../examples/dotnet/getresponse/Program.cs)，通过 ProjectReference 引用仓库现有 .NET 绑定，不需要额外 NuGet 包。

## 准备环境

需要 .NET 8 SDK（或能够构建 net8.0 的更新 SDK）、对应架构的原生 pmac 库，以及已核验的 SSH 主机身份文件。先在仓库根目录构建：

```sh
cargo build -p pmac-ffi --release --locked
dotnet build examples/dotnet/getresponse/Pmac.GetResponse.csproj -c Release
```

原生库为 Windows 的 `target/release/pmac.dll` 或 Linux 的 `target/release/libpmac.so`。如果 Cargo 使用 `--target <triple>`，路径改为 `target/<triple>/release`。该项目只构建托管示例和绑定，不会自动构建、复制或升级原生库。

在启动进程前设置以下变量。不要将密码写入源代码、命令历史或版本库；可由当前终端会话或应用的凭据管理方式提供。

| 环境变量 | 用途 |
| --- | --- |
| `PMAC_LIBRARY` | 必填，原生 pmac DLL/so 的文件路径，推荐绝对路径；不能指向托管 `Pmac.Russh.dll` |
| `PMAC_HOST` | 控制器主机名或 IP，由使用者提供 |
| `PMAC_KNOWN_HOSTS` | 已经可信渠道核验的 known_hosts 文件路径 |
| `PMAC_PASSWORD` | 示例传给 SSH 的密码；代码不打印或保存该值 |

本示例的用户名为 `root`，使用密码认证。若现场使用其他用户、私钥或固定 SHA-256 主机指纹，修改示例中的 `ConnectOptions`，字段与设置方法见[认证说明](authentication.md)。不要为了跑通示例启用接受任意主机密钥。

Windows PowerShell，在仓库根目录执行：

```powershell
$env:PMAC_LIBRARY = (Resolve-Path 'target/release/pmac.dll').Path
# 从已有进程环境或可信配置提供 PMAC_HOST、PMAC_KNOWN_HOSTS 和 PMAC_PASSWORD。
# 仅加载库并检查版本、ABI 与位数，不连接控制器：
dotnet run --project examples/dotnet/getresponse/Pmac.GetResponse.csproj -c Release -- --check
```

Linux，终端中执行：

```sh
export PMAC_LIBRARY="$PWD/target/release/libpmac.so"
# 从已有进程环境或可信配置提供 PMAC_HOST、PMAC_KNOWN_HOSTS 和 PMAC_PASSWORD。
dotnet run --project examples/dotnet/getresponse/Pmac.GetResponse.csproj -c Release -- --check
```

`PMAC_LIBRARY` 在绑定第一次调用原生接口时读取并固定；需要换库时重新启动进程。原生库必须与托管进程位数一致，ABI major 为 2。绑定仍兼容 ABI major 2 内的现有输入布局；使用当前仓库构建的 DLL/so 可避免旧库缺少函数入口。`BadImageFormatException` 通常涉及位数/格式不匹配，`DllNotFoundException` 也可能表示依赖库加载失败，`EntryPointNotFoundException` 提示原生库缺少绑定要求的导出。不要通过复制不同版本的 DLL 反复覆盖来掩盖 ABI 不一致。

## 获取完整响应

设置连接变量后，不传参数即执行默认只读命令 `Sys.ServoTime`：

```sh
dotnet run --project examples/dotnet/getresponse/Pmac.GetResponse.csproj -c Release --no-build
```

也可以显式传入命令。**用户提供的命令将原样执行一次，可能改变设备状态。示例不会额外发送运动、写入或测试命令，也不会自动重试失败命令。请先确认目标和命令副作用。** 以下仍为只读查询：

```sh
dotnet run --project examples/dotnet/getresponse/Pmac.GetResponse.csproj -c Release --no-build -- "Sys.ServoTime"
```

主要调用顺序如下；完整可运行实现包含于示例 Program.cs：

```csharp
using var runtime = new Pmac.Runtime();
using var client = runtime.CreateClient();
using (var request = client.Connect(connection))
{
    request.Wait();
}
byte[] response = client.GetResponse(command, pattern);
Console.Write(System.Text.Encoding.UTF8.GetString(response));
```

`Client.Connect` 返回可释放的连接 Request，`Wait()` 同步等待连接完成后才发送命令。`GetResponse` 内部提交一个请求、等待完成、复制响应并释放请求；返回 `byte[]`，没有额外 NUL 终止符。示例用 UTF-8 解码显示，完整字节处理应直接使用返回数组。

示例使用同步 Main，不在连接/命令之间切换调用线程。它先释放连接 Request，再在退出时释放 Client、最后释放 Runtime。失败时记录原始操作错误，清理错误另外输出，避免覆盖设备返回的诊断。

## pattern：完整响应、variable、number 与正则

第二个命令行参数为可选 pattern，省略或空字符串返回完整响应；提取操作复用已完成的响应，不会再次发送命令。保留模式精确区分大小写，详见[模式说明](pattern-migration.md)。

| pattern | 返回内容 |
| --- | --- |
| 省略 / 空字符串 | 完整响应字节，允许空响应 |
| `variable` | 从首个等号后跳过空白，提取至下一个空白或响应末尾；适合 `P1=3.14` 中取 `3.14` |
| `number` | 从首个数字、正负号或小数点候选提取数字片段；对 `P1=3.14` 得到的是变量名中的 `1`，不适合通用的带变量名响应取值 |
| 其他字符串 | Rust 字节正则；同步 GetResponse 取 group 0，即完整匹配，而不是自动取第一个括号捕获组 |

当已确认响应中首个数字候选就是要提取的值时，可传入 `number`；不要把它用于从 `P1=...` 这类响应中取等号后的值：

```sh
dotnet run --project examples/dotnet/getresponse/Pmac.GetResponse.csproj -c Release --no-build -- "Sys.ServoTime" "number"
```
例如，当控制器返回带等号的变量响应时，使用 `variable`：

```sh
dotnet run --project examples/dotnet/getresponse/Pmac.GetResponse.csproj -c Release --no-build -- "Sys.ServoTime" "variable"
```

只匹配某一响应行的正则示例：

```sh
dotnet run --project examples/dotnet/getresponse/Pmac.GetResponse.csproj -c Release --no-build -- "Sys.ServoTime" '(?m)^Sys[.]ServoTime=([^\r\n]+)'
```

该模式包含括号，但 `GetResponse(command, pattern)` 仍返回整段 `Sys.ServoTime=...` 匹配。需要 group 1 时，使用显式 Request；以下是替代调用方式，不要在已经发送同一命令后再调用一次来“获取”原响应：

```csharp
using var request = client.GetResponseAsync(command);
request.Wait();
byte[] value = request.Match(@"(?m)^Sys[.]ServoTime=([^\r\n]+)", 1);
Console.Write(System.Text.Encoding.UTF8.GetString(value));
```

正则由原生 Rust `regex::bytes` 引擎执行，不能按 .NET Regex 的全部语法理解；不支持环视和反向引用。命令、响应格式及具体匹配应以实际设备为准。`number` 是兼容文本提取而非严格数值解析，例如宽松指数片段不保证能转换成有效浮点数；需要数值时应在应用中继续校验。

## 错误、线程和关闭

示例捕获 `PmacException`，输出原始 `Message`、`Status`、`Certainty` 与 RequestId，失败退出码为非零；加载和配置异常也以非零退出。不要只打印“调用失败”而丢弃设备的 `error#...` 文本，也不能将有 ACK 的 `error#` 响应当成功。收到原生协议/远端错误后，本例不打印其为正常 GetResponse 结果，不自动补发 `close` 或重放命令。

`Certainty=MayHaveExecuted` 表示命令可能已执行，不能据此自动重试带副作用的命令。某些错误会清除本地程序上下文并使成员退役，需要应用核实设备状态后显式恢复；单纯重试相同 GetResponse 不能恢复控制器资源或程序状态。

同一 Client 最多占用 16 个 GPASCII 在线通道，合计全部池成员、预建、在用、回收中和释放不确定的占用。不同 Client 各自计算额度；控制器自身仍可能拒绝新资源。每个提交线程独占固定通道，`open prog` / 程序行 / `close` 必须在同一线程提交；不要让 `await`、Task.Run 或 UI 调度在该序列中切换线程。普通单次查询可保持如本例的同步调用方式。

关闭出现 `PMAC_SHUTDOWN_PENDING` 或仍有释放不确定的资源时，应保留对象/ID并按[使用手册](user-manual.md)完成核实和恢复后重试，不能把超时当作已安全卸载。此例是独立控制台程序；长驻宿主应将对象和恢复流程纳入应用生命周期。

Gather 与 geterrors 属于后续专题，当前没有 C# 便捷封装；需要时通过 C ABI 自行封装，并遵守回调和停止屏障：[实时 Gather](gather.md)、[geterrors 状态变化监控](geterrors.md)。
GetResponse 也支持可选的 passthrough 请求选项，以原文发送单条命令并跳过 online/program 分类；默认关闭。各语言写法、单帧限制与错误语义见[透传模式说明](getresponse-examples.md#可选透传模式跳过-onlineprogram-规则)。
