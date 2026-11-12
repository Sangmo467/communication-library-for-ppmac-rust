# Power PPMAC 通信库使用手册

适用于库版本 2.0.0、C ABI 2.1。本文说明公开 SDK 的日常使用；完整函数和结构定义分别以 `include/pmac.h`、各语言绑定源码及 [C ABI 契约](abi-contract.md) 为准。

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

常用操作：

- `abort`：中止；网络请求不能替代硬件急停。
- `coord_enable`、`coord_exec_prog`、`coord_hold`、`coord_run`。
- `plc_enable`、`plc_disable`。
- `motor_kill`、`motor_enable`、`motor_jog_stop`、`motor_abs_jog`、`motor_inc_jog`。

常用查询：

- `coord_status`、`motor_status`；
- `motor_position`、`motor_desired_position`、`motor_velocity`、`motor_position_error`；
- `servo_error_count`、`phase_error_count`、`servo_count`、`phase_time`、`servo_time`；
- `abort_all_flag`、`plc_running`、`coord_prog_running`、`motor_closed_loop`、`coord_motor_enabled`。

C++ 使用 camelCase，.NET 使用 PascalCase，Python 使用 snake_case。C++ 和 Python 的状态对象提供命名字段；.NET 的 `MotorStatus` / `CoordStatus` 值对象也提供对应属性。数值 helper 会拒绝 NaN、Inf、溢出和歧义目标。

GPASCII ACK 只说明命令交互得到确认。运动到位、程序结束、PLC 实际状态和故障消除必须读取相应控制器状态，并由应用安全逻辑判断。

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
