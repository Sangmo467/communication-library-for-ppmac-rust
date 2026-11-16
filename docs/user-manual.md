# Power PPMAC 通信库使用手册

[English](user-manual.en.md)

GetResponse 默认开启透传，跳过规则表和 online/program 分类，原文提交单条命令；显式关闭后启用规则校验。详见 **[GetResponse 透传模式与各语言示例](getresponse-passthrough.md)**。


适用于库版本 2.0.7、C ABI 2.1。本文说明公开 SDK 的日常使用；完整函数和结构定义分别以 `include/pmac.h`、各语言绑定源码及 [C ABI 契约](abi-contract.md) 为准。

首次接入建议先阅读 [GetResponse 多语言示例](getresponse-examples.md)，包含 C/C++、[C#](dotnet-getresponse.md)、[Python](python-getresponse.md) 和 [MATLAB MEX](matlab-getresponse.md) 的构建、连接与完整调用步骤；Gather 和 geterrors 作为后续专题。

## 1. 对象模型与基本顺序

所有语言共享同一套原生对象模型：

- **Runtime**：管理 Tokio worker、完成通知和所属对象。一个进程可以创建多个 Runtime，但通常复用一个即可。
- **Client**：一个控制器的逻辑客户端，持有连接池、请求容量和维护连接。
- **Request**：一次已接纳操作的 ID、终态和结果。读取结果不会重新发送命令。

基本顺序：

1. 创建 Runtime；
2. 创建 Client；
3. 配置认证和主机身份校验并连接；
4. 提交命令或业务操作；
5. 等待请求并读取响应、stderr、退出码或错误；
6. 释放 Request；
7. 关闭并释放 Client；
8. shutdown 并释放 Runtime。

C++/Python 的 RAII 或 `with`、.NET 的 `using`/`await using` 可辅助收尾，但不能把垃圾回收当成确定的网络关闭流程。

## 2. 连接配置

### 2.1 常用参数

| 参数 | 默认值 | 说明 |
|---|---:|---|
| host、username | 无 | 必填；host 使用控制器地址，username 通常为控制器 SSH 用户 |
| port | 22 | SSH 端口 |
| password | 空 | 密码认证 |
| private_key / privateKey / PrivateKeyPath | 空 | 私钥文件路径；设置后优先使用私钥，不自动兜底密码 |
| passphrase | 空 | 加密私钥的口令 |
| known_hosts / knownHosts / KnownHostsPath | 空 | OpenSSH known_hosts 文件 |
| pinned_sha256 / pinnedSha256 / PinnedSha256 | 空 | 主机公钥的 32 字节 SHA-256 指纹 |
| accept_any_host_key / acceptAnyHostKey / AcceptAnyHostKey | false | 显式跳过主机身份校验；不建议生产使用 |
| tcp_probe / tcpProbe / TcpProbe | true | 为建连过程增加 TCP 超时，不是额外建立第二条连接 |
| keepalive_interval_ms 等 | 1000 / 3 | SSH 保活间隔和最大失败次数 |
| control_pool_size / controlPoolSize / ControlPoolSize | 1 | 初始控制连接成员数 |

主机身份校验与登录凭据是两个独立配置。必须选择 known_hosts、固定 SHA-256 指纹，或明确接受 `accept_any_host_key` 的风险。详见[认证说明](authentication.md)。

### 2.2 C++17

```cpp
#include "pmac.hpp"
#include <cstdlib>
#include <string>

pmac::Runtime runtime;
pmac::Client client = runtime.createClient();

pmac::ConnectOptions options;
options.host = std::getenv("PMAC_HOST");
options.username = "root";
options.knownHosts = std::getenv("PMAC_KNOWN_HOSTS");
if (const char* value = std::getenv("PMAC_PASSWORD")) options.password = value;
client.connectSync(options);

auto bytes = client.getResponse("Sys.ServoTime");
std::string text(bytes.begin(), bytes.end());
```

C++17 的 `getResponse` 返回 `std::vector<uint8_t>`，不是自动追加 NUL 的字符串。`Request` 不可复制，可以移动。

### 2.3 C++98

C++98 适配层提供同步 `Runtime`、`Client` 和 `Options`：

```cpp
#include "pmac_cpp98.hpp"

pmac::Runtime runtime;
pmac::Client client(runtime);
pmac::ConnectOptions options;
options.host = "192.0.2.10";
options.username = "root";
options.password = "from-secure-configuration";
options.knownHosts = "known_hosts";
client.connect(options);
std::vector<uint8_t> response = client.getResponse("Sys.ServoTime");
```

该适配层不依赖 C++11；异步和精细终态控制可使用 C ABI 或 C++17 绑定。

### 2.4 .NET 8

```csharp
using Pmac;

using var runtime = new Pmac.Runtime();
using var client = runtime.CreateClient();
await client.ConnectAsync(new ConnectOptions
{
    Host = Environment.GetEnvironmentVariable("PMAC_HOST")!,
    Username = "root",
    Password = Environment.GetEnvironmentVariable("PMAC_PASSWORD"),
    KnownHostsPath = Environment.GetEnvironmentVariable("PMAC_KNOWN_HOSTS"),
});

byte[] response = await Task.Run(() => client.GetResponse("Sys.ServoTime"));
```

真正的异步等待可直接使用 Request：

```csharp
using var request = client.GetResponseAsync("Sys.ServoTime");
RequestInfo info = await request.WaitAsync(cancellationToken);
byte[] response = request.Response();
```

取消 `WaitAsync` 的 CancellationToken 会请求取消原生操作，不只是停止托管等待。

### 2.5 Python

```python
import os
import pmac

with pmac.Runtime(library_path=os.environ.get("PMAC_LIBRARY")) as runtime:
    with runtime.create_client() as client:
        client.connect(
            os.environ["PMAC_HOST"],
            os.environ.get("PMAC_USER", "root"),
            password=os.environ.get("PMAC_PASSWORD"),
            known_hosts=os.environ.get("PMAC_KNOWN_HOSTS"),
        )
        response = client.get_response("Sys.ServoTime")
        print(response.decode("ascii").strip())
```

`PMAC_LIBRARY` 在这里由应用读取并传给 `library_path`，不是绑定隐式定义的配置项。异步接口返回 Request：

```python
request = client.get_response_async("P1", timeout_ms=5000)
try:
    await request.wait_async()
    value = request.response()
finally:
    request.release()
```

Request 也支持 `with request:`。不要在 `fork` 后的子进程继承并使用父进程 Runtime；应在子进程中重新创建。

### 2.6 C ABI 核心顺序

C 调用方需要填写每个输入结构的 `struct_size` 和 `abi_version`，并在每次调用后检查状态：

```c
pmac_runtime_config runtime_options = {0};
runtime_options.struct_size = sizeof(runtime_options);
runtime_options.abi_version = PMAC_ABI_VERSION;
runtime_options.worker_threads = 2;
pmac_runtime_id runtime = 0;
int32_t status = pmac_runtime_create(&runtime_options, &runtime);

pmac_client_config client_options = {0};
client_options.struct_size = sizeof(client_options);
client_options.abi_version = PMAC_ABI_VERSION;
client_options.max_requests = 64;
client_options.max_file_tasks = 1;
client_options.max_retained_bytes = 80u * 1024u * 1024u;
pmac_client_id client = 0;
if (status == PMAC_OK)
    status = pmac_client_create(runtime, &client_options, &client);
```

连接字符串使用 `pmac_bytes {data, len}`，长度不含 NUL。提交后保存 Request ID：

```c
pmac_request_options request_options = {0};
request_options.struct_size = sizeof(request_options);
request_options.abi_version = PMAC_ABI_VERSION;
request_options.timeout_ms = 5000;
request_options.max_response_bytes = 1024u * 1024u;

const char command[] = "Sys.ServoTime";
pmac_request_id request = 0;
status = pmac_get_response_async(
    client, (const uint8_t*)command, sizeof(command) - 1u,
    &request_options, &request);
```

使用 `pmac_request_wait` 等待，再两次调用 `pmac_request_copy_response`：第一次用空缓冲取得 `required`，分配空间后第二次复制。最后调用 `pmac_request_release`。所有公开函数和结构见 `include/pmac.h`。

## 3. 请求、超时和结果

### 3.1 请求参数

默认总执行超时为 5000 ms，默认最大响应为 1 MiB。可按调用设置：

| C++17 | .NET | Python | 含义 |
|---|---|---|---|
| `timeoutMs` | `TimeoutMs` | `timeout_ms` | 从提交开始计算的总 deadline，包含排队 |
| `infinite` | `Infinite` | `infinite=True` | 移除总 deadline；仍可取消或关闭 |
| `maxResponseBytes` | `MaxResponseBytes` | `max_response_bytes` | 单请求最大保留响应 |
| `readPool` | `ReadPool` | `read_pool=True` | 允许分类器认可的只读命令使用连接池规则 |

`request_wait` 的等待预算只限制本次等待。`WAIT_PENDING` 不表示请求失败，也不会取消它；可继续等待、轮询或取消。

### 3.2 命令与提取

- `command` / `Command`：提交 GPASCII 命令并返回原始响应字节。
- `get_response` / `GetResponse`：同一请求核心，额外支持模式提取。
- `Request.response()` / `Response()`：取得完整响应。
- `Request.match(...)` / `Match(...)`：按模式和捕获组复制结果。
- `Request.stderr()` / `Stderr()`：取得 Shell 标准错误。
- `Request.cancel()` / `Cancel()`：请求取消；结果取决于操作所处阶段。

精确空模式、`number` 和 `variable` 使用兼容提取规则，其他模式使用 `regex::bytes` 字节正则。详见[模式说明](pattern-migration.md)。

### 3.3 执行确定性

| 值 | 含义 | 应用处理 |
|---|---|---|
| `NOT_SENT` | 可证明尚未发送 | 排除其他准入问题后可重新提交 |
| `MAY_HAVE_EXECUTED` | 已开始发送或失去确认，远端可能已执行 | 不得盲目重试；先核对设备状态 |
| `CONFIRMED` | 取得完整协议确认 | 只代表协议完成，不代表机械动作完成 |

C++ 异常提供 `status()`、`requestId()`、`certainty()`；.NET 的 `PmacException` 提供 `Status`、`RequestId`、`Certainty`；Python 的 `PmacError` 提供 `status`、`request_id`、`certainty`。

## 4. 连接池与线程顺序

普通 `connect` 使用初始控制配置。需要显式管理时使用：

- C++：`preparePoolAsync`、`resizePoolAsync`、`repairPoolAsync`、`drainPoolAsync`；
- .NET：`PreparePoolAsync`、`ResizePoolAsync`、`RepairPoolAsync`、`DrainPoolAsync`；
- Python：`pool_prepare_async`、`pool_resize_async`、`pool_repair_async`、`pool_drain_async`；
- C：`pmac_pool_*_async`。

池参数满足 `1 <= min_ready <= target_size <= max_members <= 255`。成员是独立 SSH/GPASCII 会话，不是无边界的自动扩容机制。
同一 Client 最多占用 **16 个 GPASCII 在线通道**，合计其所有池成员的预建、打开中、在用、回收中以及释放不确定的通道。第 17 个通道申请在发送前返回 `RESOURCE_LIMIT`；不同 Client 各自拥有 16 个额度，即使连接到同一控制器也不共用一个 16 通道上限。设备自身仍可能有额外资源限制。profile 的成员数范围不代表允许突破在线通道预算。

只有已确认释放的通道才归还额度；释放不确定的旧通道继续占用所属 Client 的额度，应用核实远端释放后按 revision 显式确认恢复。一个 Client 的恢复声明不能释放其他 Client 的占用。维护面的 BufferUpload 不占用 GPASCII 在线通道额度，在线通道满额时仍可通过维护面执行。

关键规则：

1. 每个调用线程首次提交 GPASCII 时固定一个健康成员，并独占一个常驻 Channel。
2. 同线程后续普通命令和 `read_pool` 请求不迁移成员；不同线程不共享 Channel。
3. 跨线程或跨成员不提供隐式写后读因果顺序或同时刻快照。
4. `open prog`、程序行和 `close` 必须从同一线程提交。
5. 成员故障后不会自动把线程迁移到其他成员，也不会自动重放旧请求。
6. 维护面的修复使用 `repairMaintenanceAsync` / `RepairMaintenanceAsync` / `pool_repair_maintenance_async`。

通过 `poolState` / `PoolState()` / `pool_state()` 查看池 revision 和成员计数，通过 `poolMembers` / `PoolMembers()` / `pool_members()` 查看成员状态，通过 `Request.route` / `Route` 查看请求的成员 ID 和 epoch。

出现未知写结果时，后续冲突操作可能返回 `RECOVERY_REQUIRED`。应用核对设备后，以匹配 revision 调用 `confirmRecovery` / `ConfirmRecovery` / `pool_confirm_recovery`。确认未知写结果与确认远端连接释放是两个独立声明。

## 5. Shell 与文件

### Shell

- `shell(command, persistent=True)` 使用持久 Shell，保留工作目录和环境等会话状态。
- `persistent=False` 使用隔离 exec，不继承另一调用的 Shell 状态。
- 等待完成后读取 stdout、stderr 和可选退出码；非零退出码不等于传输失败。
- 长任务仍需设置合理 deadline，或保留取消和关闭能力。

### 文件

| 功能 | C++17 | .NET | Python |
|---|---|---|---|
| 上传 | `upload` | `Upload` / `UploadAsync` | `upload` |
| 下载 | `download` | `Download` / `DownloadAsync` | `download` |
| buffer 上传 | `uploadBuffer` | `BufferUpload` / `BufferUploadAsync` | `upload_buffer` |
| 判断存在 | `fileExists` | `FileExists` | `file_exists` |
| 删除 | `removeFile` | `RemoveFile` | `remove_file` |

上传采用同目录 staging 和 MD5 校验后发布。`PUBLISH_UNKNOWN` 表示最终发布结果不确定，不能直接当成未发布并重试。删除、覆盖和程序加载属于有副作用操作，应在受控维护窗口使用。

## 6. 运动、PLC 与状态查询

本节函数通过已连接的 Client 同步发送 GPASCII 命令。控制操作成功表示协议已确认；运动是否到位、程序是否完成和 PLC 是否处于期望状态，需要继续查询。以下使用 Python 名称；C ABI 加 `pmac_` 前缀，C++17 使用 camelCase，.NET 使用 PascalCase，详细 C 签名见第 10.6、10.7 节。

### 6.1 参数、返回和错误

| 参数 | 说明 |
|---|---|
| `n` / `motor` | 电机编号。电机命令和 `#n` 状态查询由库校验为 0～255；设备实际可用编号取决于配置 |
| `n` / `coord` | 坐标系编号。`&n` 命令和状态查询由库校验为 0～127；实际范围取决于设备配置 |
| `n` / `plc` | PLC 编号。启用与禁用命令由库校验为 0～31 |
| `p` / `program` | 要启动的运动程序编号，必须为正整数 |
| `value` / `position` / `distance` | `double` 浮点值；绝对 jog 为目标位置，增量 jog 为相对位移；须为有限值，单位和缩放由控制器配置决定 |
| 请求选项 | Python 可传 `timeout_ms=5000`；C++、.NET 和 C 使用对应的请求选项对象或结构 |

控制操作的 Python 返回值为 `None`，C++17/.NET 返回 `void`；C 返回 `PMAC_OK` 或负状态码。查询在 Python/C++/.NET 返回解析后的数值或布尔值；C 返回状态码并通过 `out` 输出值。失败会在高级绑定抛出异常。超时或取消后应检查执行确定性；如果可能已经执行，先查询设备，不要盲目重复运动命令。

### 6.2 电机命令

| Python 方法 | 对应 GPASCII | 含义、参数与结果 |
|---|---|---|
| `motor_enable(n)` | `#n j/`（实际格式 `#nj/`） | 对电机 `n` 发送使能命令；成功无数据返回 |
| `motor_kill(n)` | `#nkill` | 对电机 `n` 发送 kill；成功无数据返回 |
| `motor_jog_stop(n)` | `#njog/` | 停止电机 `n` 的 jog；成功无数据返回 |
| `motor_abs_jog(n, value)` | `#njog=value` | 以 `value` 为绝对目标位置，发送 jog 命令 |
| `motor_inc_jog(n, value)` | `#njog:value` | 以 `value` 为相对位移，发送增量 jog 命令 |
| `abort()` | `&*adisable` | 发送全局 abort/disable 命令，成功无数据返回 |

例如 `motor_abs_jog(1, 25.0)` 针对电机 1，`motor_inc_jog(1, -2.0)` 请求负方向增量 2 个控制器位置单位。命令的速度、加减速、限位及允许的运动范围取决于控制器配置。网络 Abort、Kill 和 JogStop 不能替代硬件安全链路。

### 6.3 坐标系和 PLC 命令

| Python 方法 | 对应 GPASCII | 含义与结果 |
|---|---|---|
| `coord_enable(n)` | `&nenable` | 对坐标系 `n` 发送 enable；成功无数据返回 |
| `coord_exec_prog(n, p)` | `&n begin p run` | 在坐标系 `n` 启动程序 `p`；成功表示命令已确认，不表示程序已运行结束 |
| `coord_hold(n)` | `&nhold` | 向坐标系 `n` 发送 hold；成功无数据返回 |
| `coord_run(n)` | `&nrun` | 向坐标系 `n` 发送 run；成功无数据返回 |
| `plc_enable(n)` | `enable plc n` | 启用 PLC `n`；成功无数据返回 |
| `plc_disable(n)` | `disable plc n` | 禁用 PLC `n`；成功无数据返回 |

例如 `coord_exec_prog(1, 10)` 发送 `&1 begin 10 run`。可使用 `coord_prog_running(1)` 查询程序活动标志；`plc_running(2)` 查询 PLC 2 的活动标志。查询值是一次采样，实际任务的完成条件还应由应用定义。

### 6.4 电机位置、速度和状态

| Python 方法 | 查询 | 返回值和用途 |
|---|---|---|
| `motor_position(n)` | `#np` | `float`：实际位置 |
| `motor_desired_position(n)` | `#nd` | `float`：期望位置 |
| `motor_velocity(n)` | `#nv` | `float`：速度 |
| `motor_position_error(n)` | `#nf` | `float`：位置误差 |
| `motor_status(n)` | `#n?` | Python 返回 `MotorStatus`；C ABI 返回 `uint64_t` 位字段；C++17/.NET 方法返回原始 64 位值 |
| `motor_closed_loop(n)` | 先读 `Motor[n].ClosedLoop`，为真时再读 `Motor[n].AmpEna` | 布尔值：两项均为真时才为真；两个读取不是原子快照 |

例如位置和误差可分别用于显示和监测跟随情况；速度和位置的单位由设备配置决定。不能把单次读取当作多轴同时刻快照。`motor_closed_loop` 的两个读取可能发生在不同时间；如状态正在变化，应在应用中考虑这一点。

### 6.5 坐标系、PLC 和系统状态

| Python 方法 | 查询 | 返回值和用途 |
|---|---|---|
| `coord_status(n)` | `&n?` | Python 返回 `CoordStatus`；C ABI、C++17 和 .NET 返回原始 64 位状态值 |
| `coord_prog_running(n)` | `Coord[n].ProgActive` | `bool`：坐标系程序活动标志 |
| `coord_motor_enabled(n)` | `Coord[n].AmpEna` | `bool`：坐标系电机使能标志 |
| `plc_running(n)` | `Plc[n].Active` | `bool`：PLC 活动标志 |
| `abort_all_flag()` | `Sys.AbortAll` | `bool`：全局 abort 标志 |
| `servo_error_count()` | `Sys.ServoErrorCtr` | 整数：伺服错误计数 |
| `phase_error_count()` | `Sys.PhaseErrorCtr` | 整数：相位错误计数 |
| `servo_count()` | `Sys.ServoCount` | 整数：伺服计数 |
| `servo_time()` | `Sys.ServoTime` | `float`：伺服周期时间，单位以控制器定义为准 |
| `phase_time()` | `Sys.PhaseTime` | `float`：相位周期时间，单位以控制器定义为准 |

数值解析失败会返回解析错误，不会把无效响应当作零。状态标志接口分别查询，不构成原子快照。

### 6.6 状态位解释

`motor_status` 和 `coord_status` 保留完整 64 位。下面列出常用位，位号从最低位 0 开始；实际含义应结合控制器版本及配置核对。

| 位 | 名称 | 含义 |
|---:|---|---|
| 63 | `trigger_move` | 触发运动相关状态 |
| 62 | `home_in_progress` | 回零进行中 |
| 61 / 60 | `minus_limit` / `plus_limit` | 负 / 正硬限位状态 |
| 59 / 58 | `fe_warn` / `fe_fatal` | 跟随误差警告 / 严重错误 |
| 57 / 56 | `limit_stop` / `amp_fault` | 限位停止 / 放大器故障 |
| 55 / 54 | `soft_minus_limit` / `soft_plus_limit` | 负 / 正软限位状态 |
| 53 / 50 | `i2t_fault` / `enc_loss` | I²t 故障 / 编码器丢失 |
| 47 / 46 | `home_complete` / `des_vel_zero` | 回零完成 / 期望速度为零 |
| 45 / 44 | `closed_loop` / `amp_ena` | 闭环 / 放大器使能状态位 |
| 43 / 42 | `in_pos` / `interlock_stop` | 到位 / 联锁停止状态位 |

Python 状态对象的命名字段及两位字段定义见 `pmac/status.py`；C++ 的状态辅助类型见 `pmac_status.hpp`。坐标系还有 `feed_hold`、`move_mode` 等两位字段，不能按单个布尔位处理。不同绑定的状态方法返回类型以第 6.4、6.5 节为准。

### 6.7 调用示例

以下代码假定 `client` 已连接，且应用已确认目标、单位和运行条件。示例使用只读查询：

```python
position = client.motor_position(1, timeout_ms=5000)
velocity = client.motor_velocity(1, timeout_ms=5000)
closed_loop = client.motor_closed_loop(1, timeout_ms=5000)
plc_active = client.plc_running(2, timeout_ms=5000)
program_active = client.coord_prog_running(1, timeout_ms=5000)
print(position, velocity, closed_loop, plc_active, program_active)
```

需要控制设备时，方法调用形式如下；实际执行前由应用确认设备状态、行程和运动许可：

```python
# client.motor_enable(1, timeout_ms=5000)
# client.motor_abs_jog(1, 25.0, timeout_ms=5000)
# client.motor_inc_jog(1, -2.0, timeout_ms=5000)
# client.motor_jog_stop(1, timeout_ms=5000)
# client.motor_kill(1, timeout_ms=5000)
# client.coord_enable(1, timeout_ms=5000)
# client.coord_exec_prog(1, 10, timeout_ms=5000)
# client.coord_hold(1, timeout_ms=5000)
# client.coord_run(1, timeout_ms=5000)
# client.plc_enable(2, timeout_ms=5000)
# client.plc_disable(2, timeout_ms=5000)
# client.abort(timeout_ms=5000)
```

C ABI 调用需检查状态码后再使用输出：

```c
double position = 0.0;
uint32_t active = 0;
int32_t status = pmac_motor_position(client, 1, &request_options, &position);
if (status == PMAC_OK)
    status = pmac_plc_running(client, 2, &request_options, &active);
/* 仅在 status == PMAC_OK 时使用 position 和 active。 */
```

## 7. 常见状态与处理

| 状态 | 含义与处理 |
|---|---|
| `INVALID_ARGUMENT` | 检查结构大小、ABI、必填字符串、数值和缓冲参数 |
| `INVALID_HANDLE` | 对象已释放或 ID 不属于当前有效对象；不要复用旧 ID |
| `QUEUE_FULL` / `RESOURCE_LIMIT` | 降低并发、释放已完成请求或调整有界容量 |
| `EXEC_TIMEOUT` | 总执行期限到期；结合 certainty 判断，不要自动重放写操作 |
| `WAIT_PENDING` | 本次等待到期；请求仍可能继续 |
| `CANCELLED` | 请求已取消；结合 certainty 判断是否可能执行 |
| `DISCONNECTED` | 对应连接失效；检查网络、成员状态和显式修复流程 |
| `AUTHENTICATION` | 检查用户名、密码/私钥、口令和私钥格式 |
| `HOST_KEY` | 主机身份不匹配或不受信；不要通过关闭校验掩盖变化 |
| `BUFFER_TOO_SMALL` | 按 `required` 分配缓冲后，用同一 Request 重取 |
| `SHUTDOWN_PENDING` | 关闭或释放尚未完成，ID 保留，可处理子对象后重试 |
| `RECOVERY_REQUIRED` | 存在未知写结果；先核对设备并显式确认恢复 |
| `MAINTENANCE_ACTIVE` | 维护屏障正在阻止相关新操作 |
| `SUPERSEDED` | 管理操作或 recovery revision 已被更新状态取代 |
| `PUBLISH_UNKNOWN` | 文件发布结果不确定；先检查远端状态 |

完整状态值见 `include/pmac.h`。C++ 还可使用 `statusName`、`errorCategory` 和 `isTransportError` 做诊断分类；分类本身不是自动重试策略。

## 8. 关闭与故障恢复

推荐显式收尾：

1. 停止提交新业务；
2. 取消不再需要的请求并等待终态；
3. 读取仍需保留的结果，然后释放 Request；
4. 调用 Client close，成功完成后再 release；
5. 调用 Runtime shutdown；
6. 释放剩余 Client/Request ID，再 release Runtime。

`SHUTDOWN_PENDING` 不是对象已丢失：保留 ID，处理未完成工作或子对象后重试。`pmac_client_release` 现在要求 Client 已完成 close，且 GPASCII 资源全部归还。

如果关闭后仍存在远端释放不确定记录，可通过 `pmac_client_recovery_ids` 找到保留 Client。只有应用核实远端会话确已释放后，才确认 `REMOTE_RELEASE`；本地 socket 关闭本身不构成证明。

卸载 DLL/so 前，除上述步骤外，还要确保没有线程仍在执行 C 入口。使用原生清理监督者时调用 `pmac_runtime_cleanup_wait` 等待监督线程退出。

## 9. 安全与故障排查

- 凭据通过安全配置或进程环境注入，不写入源码、日志、崩溃转储或发布包。
- 优先 known_hosts 或固定指纹；主机密钥变化时先核实控制器是否被替换或重装。
- 私钥与密码同时配置时只尝试私钥。需要密码登录时移除私钥配置。
- 首次建连较慢时分别测量 TCP、SSH 认证和首个 GPASCII 响应，不要仅通过缩短 timeout 调整性能。
- 出现 `NO_READY_LANE` 时检查成员状态和池容量，不要让多个线程绕过 Channel 配额。
- 发生写操作超时、断线或取消交错时，先保存 request ID、route、status 和 certainty，再决定恢复动作。
- 网络 Abort/Kill/JogStop 不是安全认证功能；机械安全必须由独立、经过验收的安全链路保证。

## 10. API 函数参考

本节以 `include/pmac.h` 的 C ABI 名称为基准，覆盖公开函数。C++17、.NET 和 Python 将同一操作封装为对象方法；对应名称及示例见前文和第 10.8 节。C++98 只提供该适配层实际声明的同步接口。签名中的精确类型、结构布局和平台导出约定以头文件为准。

### 10.1 通用参数和返回约定

| 参数 | 含义 |
|---|---|
| `id` / `runtime` | 对应 Runtime、Client 或 Request 的有效 ID，具体类型由函数签名决定 |
| `config` | 配置结构；按头文件设置 `struct_size` 和 `abi_version`，其余保留字段置零 |
| `options` | 连接或请求选项；请求选项的超时、无限等待、响应上限和只读池标志见第 3.1 节 |
| `out_id` | 输出新创建对象或已提交请求的 ID；成功后由调用方负责释放 |
| `command`、`command_len` | 命令字节指针及字节数，不依赖 NUL 终止 |
| `local`、`remote` 及对应 `_len` | 本地或控制器路径的字节指针和字节数；上传和下载的参数顺序不同 |
| `out`、`capacity`、`required` | 输出缓冲区、容量和实际所需长度；按各函数的单位分配空间，字节复制接口使用字节数 |
| `wait_ms` | 本次等待上限，单位毫秒；请求执行期限由请求选项单独控制 |
| `motor` / `coord` / `plc` / `program` | 控制器中的电机、坐标系、PLC 或程序编号 |

除版本查询函数外，表中 C 函数直接返回 `int32_t` 状态：`PMAC_OK` 表示该次调用成功；负值表示错误或尚未完成，详见第 7 节。异步提交返回成功只表示请求已接纳，最终操作状态须等待 Request。调用前检查指针与长度、结构大小和 ABI，调用后只在成功或相应契约允许时使用输出。C++/.NET/Python 通常把失败转换为异常，实际数据作为返回值；同步无结果操作通常返回 `void` / `None`。

### 10.2 Runtime 和 Client

| 函数 | 用途和参数 | 输出 / 返回 |
|---|---|---|
| `pmac_runtime_create(config, out_id)` | 创建运行时；`worker_threads` 指定工作线程数 | 状态；`out_id` 为 Runtime ID |
| `pmac_runtime_wait_completion(id, cursor, wait_ms)` | 等待完成通知；`cursor` 是调用方保存的通知游标 | 状态；游标由函数更新，通知后仍需查询具体请求 |
| `pmac_runtime_wake(id)` | 唤醒完成通知等待者 | 状态 |
| `pmac_runtime_shutdown(id, wait_ms)` | 请求关闭并在给定时间内等待 | 状态；未完成可返回 `SHUTDOWN_PENDING` 并重试 |
| `pmac_runtime_release(id)` | 释放已关闭且无未释放子对象的 Runtime | 状态 |
| `pmac_runtime_cleanup(id)` | 将 Runtime 清理交给清理监督者 | 状态；按关闭契约处理仍存在的对象 |
| `pmac_runtime_cleanup_wait()` | 等待清理监督线程退出 | 状态 |
| `pmac_get_runtime_info(out_info)` | 查询运行时 ABI、版本、能力和指针宽度 | 状态及 `pmac_runtime_info` |
| `pmac_version_string()` | 查询版本字符串 | 指向库管理的只读字符串指针；调用方不释放 |
| `pmac_version_number()` / `pmac_abi_version()` | 查询数值版本 / ABI 版本 | `uint32_t` 版本号 |
| `pmac_client_create(runtime, config, out_id)` | 在 Runtime 下创建客户端；配置请求、文件任务及保留字节上限 | 状态及 Client ID |
| `pmac_client_close(id, close_mode, wait_ms)` | 关闭客户端；`close_mode` 按头文件和 ABI 契约使用 | 状态；未完成时保留 ID 并重试 |
| `pmac_client_release(id)` | 释放已完成关闭的客户端 ID | 状态 |
| `pmac_client_recovery_ids(out, capacity, required)` | 枚举仍有待确认释放记录的客户端；容量单位为 ID 数 | 状态及所需 ID 数 |
| `pmac_client_get_state(id, out_state)` | 查询连接、关闭状态、epoch、最后响应时间和活动请求数 | 状态及 `pmac_state_snapshot` |

### 10.3 请求和结果

| 函数 | 用途和参数 | 输出 / 返回 |
|---|---|---|
| `pmac_connect_submit(id, options, out_id)` | 以 `pmac_connect_options` 提交 SSH 和 GPASCII 连接 | 状态及 Request ID |
| `pmac_command_submit(id, command, command_len, options, out_id)` | 提交 GPASCII 命令 | 状态及 Request ID；响应由 Request 读取 |
| `pmac_get_response_async(id, command, command_len, options, out_id)` | 提交 GPASCII 响应请求；模式提取在结果阶段执行 | 状态及 Request ID |
| `pmac_request_wait(id, wait_ms, out_info)` | 等待或轮询请求；`out_info` 接收状态、确定性和结果长度 | 状态；`WAIT_PENDING` 表示本次等待结束但请求未终止 |
| `pmac_request_cancel(id)` | 请求取消；不保证远端未执行 | 状态；仍需等待终态并检查 certainty |
| `pmac_request_copy_response(id, out, capacity, required)` | 复制标准响应；可先查询所需大小 | 状态及所需字节数 |
| `pmac_request_copy_stderr(id, out, capacity, required)` | 复制 Shell stderr | 状态及所需字节数 |
| `pmac_request_copy_error(id, out_info, out, capacity, required)` | 获取结构化错误及错误文本 | 状态、错误信息及所需文本字节数 |
| `pmac_request_copy_match(id, pattern, pattern_len, group, out, capacity, required)` | 用 `pattern` 和从 0 开始的捕获组 `group` 提取已完成响应 | 状态及所需结果字节数；规则见[模式说明](pattern-migration.md) |
| `pmac_request_get_route(id, member_id, epoch)` | 获取请求使用的连接池成员及连接 epoch | 状态及两个 `uint64_t` 输出 |
| `pmac_request_release(id)` | 释放 Request ID 和结果资源 | 状态；释放后不能继续读取该 ID |

典型流程：提交并检查状态 → 保存 ID → `pmac_request_wait` 直到终态 → 查询需要的结果或错误 → `pmac_request_release`。输出缓冲不足时按 `required` 重新分配，再对同一个 Request 复制；这不会重新发送设备命令。
### 10.4 DART 二进制接口

先调用 `pmac_connect_submit` 并等待 SSH/GPASCII 连接成功，再建立 DART。命令、元素类型、地址和字节序见 [DART 接口指南](dart-api.md)。每个函数的直接返回值均是提交状态，操作结果通过 `out_id` 对应的 Request 取得。

| 函数 | 参数含义 | Request 结果 |
|---|---|---|
| `pmac_dart_connect_async(id, options, out_id)` | `id` 为 Client；`options` 为请求选项 | 等待成功后 DART 可用 |
| `pmac_dart_disconnect_async(id, options, out_id)` | 关闭 DART 连接 | 等待关闭结果 |
| `pmac_dart_get_async(id, command, address, data_len, options, out_id)` | `command` 为读取命令；`address` 为 USHM 字节偏移或 P 编号；`data_len` 为字节数 | 响应是主机原生字节序的二进制数据 |
| `pmac_dart_set_async(id, command, address, data, data_len, options, out_id)` | `command` 为写入命令；`data` 指向主机原生字节序数据，`data_len` 为字节数 | 完整确认后成功；不确定写入需核对，不能盲目重放 |

例：读取 P1～P100 使用 `PMAC_DART_GET_P_VARIABLE`、`address=1`、`data_len=100 * sizeof(double)`。长度非零、不得超过 `PMAC_DART_MAX_DATA_BYTES`，且须为元素字节宽度的整数倍；USHM 范围和 P 编号还须满足头文件常量限制。数据通道故障后后续新请求可重连；显式断开后需重新连接。

### 10.5 Shell、文件和连接池

以下 `*_submit` / `*_async` 均返回提交状态，并通过 `out_id` 输出 Request ID；等待后通过 Request 取操作结果。

| 函数 | 用途、参数和完成结果 |
|---|---|
| `pmac_shell_submit(id, command, command_len, persistent, options, out_id)` | 执行 Shell；`persistent` 非零使用持久会话，零使用隔离 exec。Request 含 stdout、stderr 和可选退出码 |
| `pmac_upload_submit(id, local, local_len, remote, remote_len, options, out_id)` | 本地文件上传到远端路径；等待发布结果 |
| `pmac_download_submit(id, remote, remote_len, local, local_len, options, out_id)` | 远端文件下载到本地路径；注意路径参数顺序与上传相反 |
| `pmac_buffer_upload_submit(id, local, local_len, remote, remote_len, cleanup_remote, options, out_id)` | 上传并加载程序 buffer；`cleanup_remote` 非零时加载后清理远端文件 |
| `pmac_file_exists_submit(id, remote, remote_len, options, out_id)` | 判断远端文件是否存在；响应为单字节 ASCII `0` 或 `1` |
| `pmac_file_remove_submit(id, remote, remote_len, options, out_id)` | 删除指定远端文件；等待操作结果 |
| `pmac_pool_prepare_async(id, connect, profile, out_id)` | 根据连接选项和池配置准备池；等待管理请求完成 |
| `pmac_pool_resize_async(id, profile, out_id)` | 按新配置调整目标池大小 |
| `pmac_pool_repair_async(id, member_id, options, out_id)` | 修复指定的失败或已排空成员；`member_id=0` 选择目标范围内所有符合条件的成员 |
| `pmac_pool_drain_async(id, member_id, options, out_id)` | 排空指定成员；`member_id=0` 选择除控制成员外的所有成员 |
| `pmac_pool_repair_maintenance_async(id, options, out_id)` | 修复维护连接 |
| `pmac_pool_get_state(id, out_state)` | 同步取得池 revision、成员计数和恢复状态；直接返回状态码 |
| `pmac_pool_get_members(id, out_members, capacity, required)` | 同步复制成员快照；容量和所需数量以成员个数计 |
| `pmac_pool_confirm_recovery(id, expected_revision, flags)` | 核对设备后按当前 revision 确认恢复；`flags` 选择未知写入和/或远端释放确认；直接返回状态码 |

`pmac_pool_profile` 包含 `target_size`、`max_members`、`min_ready` 和 `connect_timeout_ms`。文件路径由指针和对应长度共同指定；上传发布不确定时先检查远端文件，不应自动重试。

### 10.6 运动控制

本节函数为同步 C helper：`id` 是 Client，`options` 是请求选项，直接返回状态码，不返回 Request ID。编号参数分别表示目标坐标系、电机、PLC 或程序；`position` 和 `distance` 是控制器单位下的有限浮点值。

| 函数 | 作用和附加参数 | 成功返回 |
|---|---|---|
| `pmac_abort(id, options)` | 发送全局 abort | `PMAC_OK` |
| `pmac_coord_enable(id, coord, options)` | 启用指定坐标系 | `PMAC_OK` |
| `pmac_coord_exec_prog(id, coord, program, options)` | 在 `coord` 执行 `program` | `PMAC_OK` |
| `pmac_coord_hold(id, coord, options)` / `pmac_coord_run(id, coord, options)` | 暂停 / 运行指定坐标系 | `PMAC_OK` |
| `pmac_plc_enable(id, plc, options)` / `pmac_plc_disable(id, plc, options)` | 启用 / 禁用 PLC | `PMAC_OK` |
| `pmac_motor_kill(id, motor, options)` | 对电机发送 kill | `PMAC_OK` |
| `pmac_motor_enable(id, motor, options)` | 启用电机 | `PMAC_OK` |
| `pmac_motor_jog_stop(id, motor, options)` | 停止指定电机 jog | `PMAC_OK` |
| `pmac_motor_abs_jog(id, motor, position, options)` | 绝对位置 jog，`position` 为目标位置 | `PMAC_OK` |
| `pmac_motor_inc_jog(id, motor, distance, options)` | 增量 jog，`distance` 为位移 | `PMAC_OK` |

返回 `PMAC_OK` 表示协议交互完成；实际动作是否完成应通过状态查询及设备安全逻辑确认。
### 10.7 状态与数值查询

下列 C 函数同步执行；共同参数为 Client `id`、请求 `options` 和输出指针 `out`。直接返回 `PMAC_OK` 或错误状态码，成功时将结果写入 `out`。`motor`、`coord`、`plc` 为目标编号；浮点值单位由控制器配置决定。

| 函数 | 附加输入 | `out` 类型及含义 |
|---|---|---|
| `pmac_coord_status` | `coord` | `uint64_t`，坐标系状态位 |
| `pmac_motor_status` | `motor` | `uint64_t`，电机状态位 |
| `pmac_motor_position` | `motor` | `double`，实际位置 |
| `pmac_motor_desired_position` | `motor` | `double`，期望位置 |
| `pmac_motor_velocity` | `motor` | `double`，速度 |
| `pmac_motor_position_error` | `motor` | `double`，位置误差 |
| `pmac_servo_error_count` | 无 | `uint32_t`，伺服错误计数 |
| `pmac_phase_error_count` | 无 | `uint32_t`，相位错误计数 |
| `pmac_servo_count` | 无 | `uint32_t`，伺服计数 |
| `pmac_phase_time` | 无 | `double`，相位周期时间 |
| `pmac_servo_time` | 无 | `double`，伺服周期时间 |
| `pmac_abort_all_flag` | 无 | `uint32_t`，全局 abort 标志，0 为假，非零为真 |
| `pmac_plc_running` | `plc` | `uint32_t`，PLC 运行标志 |
| `pmac_coord_prog_running` | `coord` | `uint32_t`，坐标系程序运行标志 |
| `pmac_motor_closed_loop` | `motor` | `uint32_t`，电机闭环标志 |
| `pmac_coord_motor_enabled` | `coord` | `uint32_t`，坐标系电机使能标志 |

例如：

```c
double position = 0.0;
int32_t status = pmac_motor_position(client, 1, &request_options, &position);
if (status == PMAC_OK) {
    /* position 是电机 1 的实际位置。 */
}
```

### 10.8 C++17、.NET 和 Python 方法对照

下表按常用方法列出名称、参数和结果；可选的请求选项分别为 C++ `RequestOptions`、.NET `RequestOptions?`、Python 关键字选项。每种绑定暴露的方法范围不完全相同，以对应源码声明为准。

| 操作 | C++17 | .NET | Python | 参数与结果 |
|---|---|---|---|---|
| 连接 | `connectSync(options)` | `ConnectAsync(options, cancellationToken)` | `connect(host, username, password=None, **kwargs)` | 连接配置见第 2 节；成功无数据返回 |
| 异步提交连接 | `connect(options)` | `Connect(options)` | `connect_async(...)` | 返回 `Request`，使用者等待并释放 |
| GPASCII | `getResponse(command, pattern, options)` | `GetResponse(command, pattern, options)` | `get_response(command, pattern=None, **kwargs)` | `command` 为命令，`pattern` 可选；返回响应或匹配的字节 |
| 提交 GPASCII | `getResponseAsync(command, options)` | `GetResponseAsync(command, options)` | `get_response_async(command, **kwargs)` | 返回 Request；结果通过 Request 读取 |
| 直接命令 | `command(command, options)` | `Command(command, options)` | `command(command, **kwargs)` | 返回原始响应字节；相应 `Async` / `_async` 返回 Request |
| Shell | `shell(command, persistent, options)` | `Shell(command, persistent, options)` | `shell(command, persistent=True, **kwargs)` | 返回 stdout 字节；需 stderr 或退出码时使用异步 Request |
| DART 连接 / 断开 | `dartConnect` / `dartDisconnect` | `DartConnect` / `DartDisconnect` | `dart_connect` / `dart_disconnect` | 成功无数据返回；对应异步方法返回 Request |
| DART 读取 | `dartGet(command, address, length, options)` | `DartGet(command, address, length, options)` | `dart_get(command, address, length, **kwargs)` | `length` 为字节数，返回主机字节序的字节序列 |
| DART 写入 | `dartSet(command, address, data, options)` | `DartSet(command, address, data, options)` | `dart_set(command, address, data, **kwargs)` | `data` 为主机字节序字节序列；成功无数据返回 |
| 上传 / 下载 | `upload(local, remote)` / `download(remote, local)` | `Upload(local, remote)` / `Download(remote, local)` | `upload(local, remote)` / `download(remote, local)` | 路径为字符串；成功无数据返回 |
| 文件存在 / 删除 | `fileExists(remote)` / `removeFile(remote)` | `FileExists(remote)` / `RemoveFile(remote)` | `file_exists(remote)` / `remove_file(remote)` | 存在查询返回布尔值；删除成功无数据返回 |
| 状态查询 | `state()`、`isConnected()` | `State()`、`IsConnected` | `state()`、`connected` | 返回状态快照或布尔值 |
| 关闭 | `close(waitMs)` | `Close(waitMs)` | `close(wait_ms)` | 等待关闭；资源释放另遵循对象生命周期 |

C++ 的 `Request.wait()` 成功后无返回数据，`waitNoThrow(waitMs)` 返回请求信息；`.NET Request.Wait()` / `WaitAsync()` 和 Python `Request.wait()` / `wait_async()` 用于等待，响应使用 `Response()` / `response()` 获取。`Request` 的匹配、stderr、取消和释放方法见第 3.2 节和对应绑定声明。

Python DART 示例：

```python
import struct

client.dart_connect(timeout_ms=5000)
try:
    data = client.dart_get(0x90, 1, 8, timeout_ms=5000)
    print("P1 =", struct.unpack("=d", data)[0])
    # 仅在应用确认 P1 可以写入时执行：
    client.dart_set(0x91, 1, struct.pack("=d", 100.0), timeout_ms=5000)
finally:
    client.dart_disconnect(timeout_ms=5000)
```

C++ 和 .NET 的完整 DART 示例及多元素传输规则见 [DART 接口指南](dart-api.md)。
## 实时连续 Gather

C 和 C++17 可使用连续采样接口，按批次接收 `Gather.Addr` 对应的行优先数值。完整接入代码、构建命令、参数范围、回调与内存规则、停止/释放、网络部署与错误恢复见[Gather 详细使用方法](gather.md)。

启动前校验 SSH 主机身份并确保控制器能够回连指定本机地址/端口；库验证来源 IP 和本次会话随机握手，数据流仍是明文 TCP。先停止并释放 Gather，再关闭 Client 和 Runtime。回调线程只处理或复制本批数据，阻塞停止与销毁应交回主线程。
## geterrors 实时状态监控

先连接 Client，再通过 C 的 `pmac_geterrors_start` 或 C++17 的 `client.startGetErrors()` 打开持续监听。周期为 0.01～1.0 秒；四类掩码可选，省略使用系统默认，显式零掩码与省略不同。启动设置和设备状态变化由不同消息类型交付；停止时向本次 SSH 终端发送 Ctrl+C，停止屏障结束后释放 Monitor，再关闭 Client/Runtime。

完整默认/自定义掩码示例、C/C++ 程序、编译命令、回调内存约定、状态判读及现场排查见 [geterrors 详细使用方法](geterrors.md)。
