# GetResponse 多语言接入与示例

[English](getresponse-examples.en.md) | [简体中文](getresponse-examples.md)

GetResponse 支持按请求启用透传：跳过规则表和 online/program 分类，原文提交单条命令。详见 **[GetResponse 透传模式与各语言示例](getresponse-passthrough.md)**。


先通过 GetResponse 完成“连接 → 发送命令 → 等待 → 读取响应 → 关闭”，再接入持续采样和监控。示例默认查询 `Sys.ServoTime`，用户传入的其他命令会实际执行。

| 语言 | 主要接口 | 可运行代码 | 详细步骤 |
| --- | --- | --- | --- |
| C | `pmac_get_response_async` + wait/copy/release | [getresponse.c](../examples/c/getresponse.c) | 本页 |
| C++17 | `Client::getResponse` | [getresponse.cpp](../examples/cpp/getresponse.cpp) | 本页 |
| C# / .NET 8 | `Client.GetResponse` | [Program.cs](../examples/dotnet/getresponse/Program.cs) | [.NET 指南](dotnet-getresponse.md) |
| Python ≥3.9 | `Client.get_response` | [getresponse.py](../examples/python/getresponse.py) | [Python 指南](python-getresponse.md) |
| MATLAB | `pmac_mex('getresponse', ...)` | [example_getresponse.m](../examples/matlab/example_getresponse.m) | [MATLAB MEX 指南](matlab-getresponse.md) |

## 1. 构建与连接配置

从仓库根目录构建原生库：

```sh
cargo build -p pmac-ffi --locked
```

Windows 产物为 `target/debug/pmac.dll`、MSVC 导入库 `pmac.dll.lib`；Linux 为 `target/debug/libpmac.so`。发布版本增加 `--release`，对应改用 `target/release`。进程、编译器目标与原生库架构必须一致；更多安装方式见[构建文档](build-and-install.md)。

C/C++、Python 和 MATLAB 示例读取以下环境变量；私钥环境变量仅由 C/C++ 和 MATLAB 示例读取，Python 示例使用密码认证。.NET 的具体要求见其指南。

| 环境变量 | 含义 |
| --- | --- |
| `PMAC_HOST` | 控制器地址，必填 |
| `PMAC_USER` | SSH 用户，默认 `root` |
| `PMAC_KNOWN_HOSTS` | 已核验的 OpenSSH known_hosts 文件，必填 |
| `PMAC_PASSWORD` | 密码认证所用密码 |
| `PMAC_PRIVATE_KEY` | 可选私钥文件路径，设置后优先使用私钥 |
| `PMAC_PASSPHRASE` | 可选私钥口令 |

密码由当前进程环境或应用的凭据管理机制提供，示例不内置设备密码。主机密钥准备与固定指纹方式见[认证说明](authentication.md)。

```powershell
$env:PMAC_HOST = 'controller-host'
$env:PMAC_USER = 'root'
$env:PMAC_KNOWN_HOSTS = 'C:\pmac\known_hosts'
# 由自己的凭据管理方式设置 PMAC_PASSWORD 或 PMAC_PRIVATE_KEY。
```

## 2. C：异步 ABI 组成一次完整 GetResponse

C ABI 没有同名同步函数。示例封装下列顺序，普通调用线程可用 wait 循环获得同步效果：

1. `pmac_runtime_create`、`pmac_client_create`。
2. `pmac_connect_submit`、`pmac_request_wait`，检查连接结果并释放连接 Request。
3. `pmac_get_response_async` 提交命令，配置 deadline；`pmac_request_wait` 返回 `PMAC_WAIT_PENDING` 时继续等待。
4. 成功后 `pmac_request_copy_response` 先查询长度、再复制字节；需要提取时改用 `pmac_request_copy_match`，不重新发送命令。
5. 释放 Request，关闭并释放 Client，shutdown 并释放 Runtime。此独立进程示例随后退出；嵌入宿主并卸载动态库时，还必须在卸载前等待 `pmac_runtime_cleanup_wait()`。

响应是“地址 + 长度”的字节数组，不保证以 NUL 结尾。示例按实际长度使用 `fwrite`。失败时先复制完整错误，再释放 Request；不能只打印状态数字后继续执行。

Windows 在已配置 x64 MSVC 的开发者终端编译：

```powershell
New-Item -ItemType Directory -Force target/examples | Out-Null
cl /nologo /W4 /std:c11 /I include examples/c/getresponse.c /Fe:target/examples/getresponse_c.exe /Fo:target/examples/getresponse_c.obj /link target/debug/pmac.dll.lib
$env:PATH = (Join-Path $PWD 'target/debug') + ';' + $env:PATH
& ./target/examples/getresponse_c.exe --help
& ./target/examples/getresponse_c.exe
& ./target/examples/getresponse_c.exe 'P1' variable
```

Linux：

```sh
mkdir -p target/examples
cc -std=c11 -Wall -Wextra -Iinclude examples/c/getresponse.c -Ltarget/debug -lpmac -o target/examples/getresponse_c
export LD_LIBRARY_PATH="$PWD/target/debug${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
./target/examples/getresponse_c 'P1' variable
```

## 3. C++17：同步 GetResponse

`pmac.hpp` 是头文件封装，仍需链接同一原生库。`getResponse` 在内部提交、等待、复制结果并释放 Request，返回 `std::vector<uint8_t>`。

```cpp
pmac::RequestOptions options;
options.timeoutMs = 5000;
auto raw = client.getResponse("Sys.ServoTime", "", options);
auto value = client.getResponse("P1", "variable", options);
```

这两行各发送一次命令。若要从同一次响应提取多个值，请保留显式 Request 并多次调用 `match`，不要重复调用 `getResponse`。

Windows x64 MSVC：

```powershell
cl /nologo /EHsc /std:c++17 /W4 /I include /I bindings/cpp examples/cpp/getresponse.cpp /Fe:target/examples/getresponse_cpp.exe /Fo:target/examples/getresponse_cpp.obj /link target/debug/pmac.dll.lib
$env:PATH = (Join-Path $PWD 'target/debug') + ';' + $env:PATH
& ./target/examples/getresponse_cpp.exe --help
& ./target/examples/getresponse_cpp.exe 'P1' variable
```

Linux：

```sh
c++ -std=c++17 -Wall -Wextra -Iinclude -Ibindings/cpp examples/cpp/getresponse.cpp -Ltarget/debug -lpmac -o target/examples/getresponse_cpp
export LD_LIBRARY_PATH="$PWD/target/debug${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
./target/examples/getresponse_cpp 'P1' variable
```

完整示例捕获 `pmac::Error` 并打印状态、执行确定性、Request ID 和诊断正文。长时间运行的应用应复用 Runtime/Client；不要每次查询都重新连接。

## 4. 响应、模式与错误

| pattern | 含义 | 示例 |
| --- | --- | --- |
| 未传或空字符串 | 返回原始完整响应字节 | `P1=3.14` 保留原文 |
| `variable` | 兼容模式：取第一个 `=` 后面的值 | `P1=3.14` → `3.14` |
| `number` | 兼容模式：取首先匹配的数值文本 | `P1=3.14` → `1`，可能取到变量编号 |
| 其他字符串 | Rust 字节正则，默认提取 group 0 | 捕获括号不改变默认 group |

保留字区分大小写；模式不会改变发送到设备的命令。需要具体捕获组时使用显式 Request 的 match 接口指定组号。完整规则见 [GetResponse 模式](pattern-migration.md)。解析数字前应检查错误、匹配结果与格式；不要把空响应或错误文本当成 0。

NAK 或 stdout 中的标准 `error #N` 按远端错误处理，原始诊断必须传给应用。例如 `stdin:1:1: error #31: Buffer closed due to another thread:` 表示原程序缓冲区上下文已经失效。ACK 到达即完成请求；stderr 中的 `error #N` 仅作为异步诊断，可能随当前或后续请求交付，不把 ACK 改判为失败。库不自动补发 `close`、不自动重放原命令。

`CONFIRMED` 表示取得完整协议确认，不能代替成功状态或运动完成判断；`MAY_HAVE_EXECUTED` 表示可能已经执行。失败的写入应由应用核对状态后决定恢复方法，不能直接重试。详见 [ABI 契约](abi-contract.md)。

## 5. 线程与生命周期

同一个 Client 最多占用 16 个在线 GPASCII 通道，多个 Client 独立计数。通道与提交调用线程保持亲和；程序的 `open prog`、正文、`close` 必须在同一调用线程连续完成。其他 Client 或外部程序的 `close all buffers` 仍可能影响当前上下文。

先完成 Request、复制结果并释放，再关闭 Client，最后关闭 Runtime。C++/C#/Python 的自动资源管理不能替代对关闭失败的处理。插件、MEX 或宿主程序卸载 DLL 前还应完成原生清理屏障，不能留下正在运行的回调或清理线程。

## 可选透传模式：跳过 online/program 规则

GetResponse/command 默认开启透传：跳过 online/program 分类及本地程序上下文检查，原文逐行发送。显式关闭透传可启用规则校验：C 使用 PMAC_REQUEST_CLASSIFY（8），C++ 使用 passthrough=false，C# 使用 Passthrough:false，Python 使用 passthrough=False，MATLAB 使用 struct('passthrough', false)。只读池请求自动启用分类；C 显式 PASSTHROUGH（4）不可与 READ_POOL 或 CLASSIFY 组合。基础帧校验和远端 error # 上报始终生效。

GetResponse 默认开启透传。透传时字符串不经过 `online-command-rules.tsv` 分类，也不要求本地确认 `open prog`，不区分 online command 与 program command。发送时保留空格、分号、大小写等原始文本；仍由 GPASCII 添加正常的帧结束符。

| 语言 | 启用方法 |
| --- | --- |
| C | `options.flags |= PMAC_REQUEST_PASSTHROUGH`，再调用 `pmac_get_response_async` |
| C++17 | `options.passthrough = true`，传给 `getResponse(command, pattern, options)` |
| C++98 | `pmac98::Options` 的 `passthrough = true`，传给 `getResponse` |
| C# | `new RequestOptions(Passthrough: true)` |
| Python | `client.get_response(command, passthrough=True)` |
| MATLAB MEX | 第四参数 `struct('passthrough', true)` |

```cpp
pmac::RequestOptions options;
options.passthrough = true;
auto raw = client.getResponse("dwell 10", "", options);
```

```csharp
var raw = client.GetResponse("dwell 10", options: new RequestOptions(Passthrough: true));
```

```python
raw = client.get_response('dwell 10', passthrough=True, timeout_ms=5000)
```

```matlab
raw = pmac_mex('getresponse', 'dwell 10', '', struct('passthrough', true));
```

这些调用会发送命令，设备是否接受由其当前状态决定；示例中的 `dwell 10` 通常需要设备处于相应程序缓冲区状态。应用需要在同一调用线程依次发送 open、正文和 close。透传模式不解析或维护程序缓冲区状态，也不为 reset/save 等命令自动添加维护屏障；建议一段程序操作始终使用同一种模式，避免与依赖本地上下文的规则校验模式混用。

透传请求统一按可能产生副作用的控制写入调度，保留线程通道亲和；显式 C 透传标志不可与 READ_POOL 组合，否则发送前返回参数错误；各语言绑定指定只读池时自动选择分类模式。请求超时、取消、响应上限、资源配额、错误上报和不自动重放的约定继续生效。stdout 中的任何 `error #N`，包括 #31，仍返回失败和原始诊断；stderr 中的 `error #N` 仅作为异步诊断，不把 ACK 改判为失败。透传模式不会根据程序语义自动补发 close 或修复上下文。

这里的透传仍是一条 GPASCII 帧：最大 1023 字节，拒绝空白命令、NUL、CR/LF、ACK 和 NAK，不能在一个字符串中拼接多行帧。多行程序应逐行请求；这项选项不改变响应正则提取。C ABI 的 RequestOptions 布局不变，透传 flag 为 4，显式分类 flag 为 8；需使用包含此功能的新原生库，旧库会拒绝未知标志。`pmac_command_submit` 也支持同一选项；其他业务接口不使用它绕过规则。

## 6. 后续：Gather 与 geterrors

GetResponse 接通后，再查看 [Gather 连续采样](gather.md)和 [geterrors 实时状态监控](geterrors.md)。两者为持续会话，不能通过一次普通 GetResponse 获取完整生命周期。现有 C ABI/C++17 支持专用接口与回调；Python、.NET 和本 MATLAB MEX 示例未新增这些回调封装。

geterrors 通过专用 PTY 持续监听，停止时向远端发送 Ctrl+C；周期为 0.01～1.0 秒，未指定掩码时使用设备默认值。扩展 MATLAB 回调时，应在原生线程复制数据并排队，由 MATLAB 调用线程主动取出；详见 MATLAB 指南的扩展边界。
