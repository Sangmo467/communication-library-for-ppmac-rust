# geterrors 实时状态变化监控

`geterrors` 持续监控 Power PMAC 的状态变化。它报告的文本可能包含错误、警告以及普通状态转变，不能仅因为收到消息就认定设备发生严重故障。库通过已连接 Client 的 SSH 会话打开专用 PTY，运行控制器自带的 `geterrors`，按行回调输出；不需要反向 TCP 监听、`nc` 或单独传输端口。主机身份校验和认证沿用现有 SSH 连接。

当前提供 C ABI 和 C++17 封装。控制器必须已安装可执行的 `geterrors`。本接口保留设备文本，不解析或猜测具体状态位的业务含义；应用应结合对应控制器手册和现场配置决定如何告警。

## 配置与输出

| C++ 字段 | C 字段/标志 | 默认值和范围 |
| --- | --- | --- |
| `periodSeconds` | `period_seconds` | C++ 默认 `0.01`；有限数值，范围 0.01..=1.0 秒；C 零初始化后必须显式填写，0 无效；对应 `-t` |
| `date` | `PMAC_GETERRORS_DATE` | 默认 false；启用时传 `-T`，日期格式由控制器决定 |
| `coordMask` | `coord_mask` / `PMAC_GETERRORS_COORD_MASK` | 默认不指定；指定时传 `-c` 和十六进制掩码 |
| `motorMask` | `motor_mask` / `PMAC_GETERRORS_MOTOR_MASK` | 默认不指定；指定时传 `-m` 和十六进制掩码 |
| `ringMask` | `ring_mask` / `PMAC_GETERRORS_RING_MASK` | 默认不指定；指定时传 `-r` 和十六进制掩码 |
| `globalMask` | `global_mask` / `PMAC_GETERRORS_GLOBAL_MASK` | 默认不指定；指定时传 `-g` 和十六进制掩码 |
| `timeoutMs` | `timeout_ms` | 默认 5000 ms，0 使用默认值；打开专用 PTY 和启动命令的预算，不限制监控总时长 |
| `queueLines` | `queue_lines` | 默认 256，0 使用默认值；最大 4096 条排队消息 |
| `maxLineBytes` | `max_line_bytes` | 默认 65536，0 使用默认值；最大 1048576 字节/行 |

四个掩码均为无符号 64 位值。C++ 使用 `std::optional<uint64_t>`：未赋值时不传相应参数，保留设备工具自身默认值；显式赋值为 0 时仍传该参数。C 使用 flags 区分这两种情况，掩码字段为 0 不代表省略。库不硬编码默认位图，也不推断掩码中每一位的含义。

例如，现场给出的 `-g0x7ff`、`-c0x180000000807ffc`、`-m0x8c007ffc`、`-r0x803e` 可按示例显式填写。它们是本示例的输入值，不是推荐给所有控制器的通用配置。启动时 `-g0x...`、`-c0x...`、`-m0x...`、`-r0x...`、`-t...` 的配置回显以 SETTINGS 消息交付，不应当作设备故障。

| kind | status | 应用处理 |
| --- | --- | --- |
| `PMAC_GETERRORS_SETTINGS` (1) | `PMAC_OK` | 配置回显，记录实际启动设置 |
| `PMAC_GETERRORS_ERROR` (2) | `PMAC_OK` | 原始设备状态变化行；包含普通变化，按内容及设备手册判断含义 |
| `PMAC_GETERRORS_STDERR` (3) | `PMAC_OK` | 远端程序 stderr 行，单独记录程序诊断；不能直接等同于电机/坐标系状态变化 |
| `PMAC_GETERRORS_TERMINAL` (4) | 非零 | 网络、进程、解码/资源或清理终止错误；结束当前监控并交由主线程收尾 |

回调按完整行交付，行终止符不属于正文，空行及仅含 ACK（0x06）的协议确认行不发送；这些确认行不会结束启动设置识别。库保留行内文本及空白，不把状态文本改写为业务错误码。正常主动停止不代表设备状态恢复；没有新状态变化时可能长时间没有回调。

## 默认掩码快速接入

先连接 Client，再启动独立监控。以下代码在已经连接的 `client` 上运行，不覆盖设备默认掩码：

```cpp
pmac::GetErrorsConfig config;
config.periodSeconds = 0.01; // seconds, not milliseconds
// Leave coordMask/motorMask/ringMask/globalMask unset for system defaults.
auto monitor = client.startGetErrors(config,
    [](uint32_t kind, const std::string& text) {
        if (kind == PMAC_GETERRORS_SETTINGS) {
            // Actual masks and period reported by the remote tool.
            std::cout << "[settings] " << text << '\n';
        } else {
            // Status transition or diagnostic text; decide severity in the application.
            std::cout << "[status " << kind << "] " << text << '\n';
        }
    },
    [](const pmac::Error& error) {
        std::cerr << "Monitoring ended: " << error.what() << '\n';
    });
// Continue application work. The monitor keeps listening until stopped or disconnected.
// When finished, outside any monitor callback:
monitor.stop();
```

实际程序应在自己的运行循环结束或收到停止指令时调用 `stop()`；不要紧接启动就停止。如果不需要终止错误回调，可省略第三个参数。监控占用独立 SSH PTY，不会把长时间运行的 `geterrors` 放进普通 `shell()` 请求中；主线程可继续调用命令与状态查询。

### 自定义、零掩码与恢复默认

```cpp
pmac::GetErrorsConfig config;
config.periodSeconds = 0.05;
config.date = true;                 // -T
config.motorMask = UINT64_C(0x8c007ffc); // -m0x8c007ffc
config.globalMask = UINT64_C(0);     // explicitly sends -g0x0
// Coord and ring masks remain unspecified, so their defaults are retained.
// To omit a previously assigned parameter when creating the next monitor:
config.motorMask.reset();
```

C 接口的同等设置如下。先零初始化，再填写必要的结构头和周期；仅标记实际需要发送的掩码：

```c
pmac_geterrors_config config = {0};
config.struct_size = sizeof(config);
config.abi_version = PMAC_ABI_VERSION;
config.period_seconds = 0.05;
config.flags = PMAC_GETERRORS_DATE | PMAC_GETERRORS_MOTOR_MASK | PMAC_GETERRORS_GLOBAL_MASK;
config.motor_mask = UINT64_C(0x8c007ffc);
config.global_mask = 0; /* flag present: explicitly sends -g0x0 */
/* To omit the motor mask for the next start, clear only its flag. */
config.flags &= ~PMAC_GETERRORS_MOTOR_MASK;
```

配置在 `start` 时复制为本次会话的快照。启动后修改配置对象不会改变正在运行的进程；需要修改周期、日期或掩码时，先在回调外停止并释放当前 Monitor，再用新配置启动。C++ 可通过结束对象作用域释放；C 必须显式调用 `pmac_geterrors_release`。每个 Client 最多保留一个 geterrors 句柄，进程总上限 64 个；停止但尚未释放的句柄仍占用名额。

## 启动回显、报警文本与现场验证

典型启动会先返回空行或 ACK，然后报告工具实际使用的掩码与周期。以下回显属于 SETTINGS，不是报警；具体默认值由控制器决定：

```text
-g0x7ff
-c0x180000000807ffc
-m0x8c007ffc
-r0x803e
-t0.010000
```

例如，监控随后可能收到如下状态行，作为独立 ERROR-kind 回调交付：

```text
Coord[1].FeFatal=1
Motor[1].FeFatal=1
```

这两条文本已在联机监听中通过回调收到。工具会在启动时报告当前命中状态，之后在检测到状态转变时继续输出；不能把启动收到的当前状态都解释成刚发生的新报警。回调的 `status=PMAC_OK` 表示传输与解码成功，不表示设备无故障；是否告警应根据 `kind`、文本、控制器手册及业务规则判断。设备没有新变化时保持安静是正常行为。

geterrors 覆盖活动电机状态、坐标系状态、全局状态、活动坐标系程序及 PLC 的执行状态，以及活动 Macro ring 状态。工具输出可包含 `Backup Motor[n].Status`、`Backup Coord[n].Status`、`Backup Sys.Status`、`Backup Macro.Status[n]` 对应的状态字符串。掩码位与固件的状态消息顺序有关，本库只把用户提供的数值格式化为 `0x` 十六进制参数，不发送十进制或 `$` 格式，本库也不会额外发送 Backup 命令来设置掩码。

现场验收可按以下顺序进行：连接并核验 SSH 主机身份 → 以默认掩码启动 → 记录 SETTINGS → 由现场人员在既有安全流程下触发已知状态变化 → 核对回调文本 → 调用 stop/release → 确认没有本次新增的 geterrors 进程。库只向本次专用 PTY 发送 Ctrl+C；不要为了停止监控而终止设备上其他既有 geterrors 进程。联机测试已验证 0.01 秒默认掩码、0.05 秒日期/显式零掩码、持续消息回调及正常 Ctrl+C 清理；不意味着对所有固件和故障场景完成验证。

可直接使用仓库中的 [C++17 示例](../examples/cpp/geterrors.cpp)和 [C11 示例](../examples/c/geterrors.c)。以下保留完整代码及逐项说明。

## 完整 C++17 示例

将以下代码保存为 `geterrors.cpp`。先设置 `PMAC_HOST`、`PMAC_KNOWN_HOSTS`，按认证方式提供 `PMAC_PASSWORD` 或修改连接选项使用私钥。known_hosts 必须预先通过可信渠道核验，详见[认证说明](authentication.md)。

```cpp
#include "pmac.hpp"
#include <cstdlib>
#include <iostream>
#include <stdexcept>

static std::string requiredEnv(const char* name) {
    const char* value = std::getenv(name);
    if (!value || !*value) throw std::runtime_error(std::string("Set ") + name);
    return value;
}
int main() {
    try {
        pmac::Runtime runtime;
        auto client = runtime.createClient();
        pmac::ConnectOptions connection;
        connection.host = requiredEnv("PMAC_HOST");
        connection.username = "root";
        connection.knownHosts = requiredEnv("PMAC_KNOWN_HOSTS");
        if (const char* password = std::getenv("PMAC_PASSWORD")) connection.password = password;
        client.connectSync(connection);
        pmac::GetErrorsConfig config;
        config.periodSeconds = 0.01;
        config.date = true;
        config.globalMask = UINT64_C(0x7ff);
        config.coordMask = UINT64_C(0x180000000807ffc);
        config.motorMask = UINT64_C(0x8c007ffc);
        config.ringMask = UINT64_C(0x803e);
        auto monitor = client.startGetErrors(config,
            [](uint32_t kind, const std::string& text) {
                // SETTINGS: startup echo; ERROR: device status transitions;
                // STDERR: remote program diagnostics. This callback owns no UI thread.
                std::cout << '[' << kind << "] " << text << '\n';
                // Copy text into an owned queue entry before another thread uses it.
            },
            [](const pmac::Error& error) {
                std::cerr << "Monitor terminated: " << error.what() << '\n';
            });
        std::cout << "Monitoring. Press Enter to stop.\n";
        std::cin.get();
        monitor.stop(); // waits for the process cleanup and callback lifetime barrier
        // monitor destructor releases its ID before client/runtime destructors run.
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
```

`OnMessage` 接收 `kind` 和 `const std::string&`；字符串仅在当前回调期间有效，需要跨线程处理时复制。用户回调抛出的异常被封装捕获、请求停止，并在回调外调用 `stop()` 时重新抛出；异常不会跨越 C ABI。析构停止并释放句柄且不抛异常，需要取得终止原因时显式调用 `stop()`。

## 完整 C11 示例

保存为 `geterrors.c`，环境变量同上。本例的回调只使用库提供的借用文本并立即输出，不保留指针。

```c
#include "pmac.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <inttypes.h>

static pmac_bytes bytes(const char *s) {
    pmac_bytes b = {(const uint8_t *)s, s ? strlen(s) : 0};
    return b;
}
static int32_t on_message(void *user, int32_t status, uint32_t kind,
                          const uint8_t *text, size_t length) {
    FILE *output = status == PMAC_OK ? stdout : stderr;
    (void)user;
    fprintf(output, "[kind=%u status=%d] ", (unsigned)kind, (int)status);
    if (length) fwrite(text, 1, length, output); /* length-delimited, not NUL-terminated */
    fputc('\n', output);
    return 0; /* nonzero requests stop; never call stop/release from this callback */
}
int main(void) {
    pmac_runtime_id runtime = 0;
    pmac_client_id client = 0;
    pmac_request_id request = 0;
    pmac_geterrors_id monitor = 0;
    pmac_runtime_config rc = {0};
    pmac_client_config cc = {0};
    pmac_connect_options connection = {0};
    pmac_geterrors_config config = {0};
    pmac_request_info info = {0};
    const char *host = getenv("PMAC_HOST");
    const char *known_hosts = getenv("PMAC_KNOWN_HOSTS");
    int32_t status = PMAC_OK, stopped, released;
    int failed = 0;
    if (!host || !*host || !known_hosts || !*known_hosts) {
        fputs("Set PMAC_HOST and PMAC_KNOWN_HOSTS\n", stderr);
        return 1;
    }
    rc.struct_size = sizeof(rc); rc.abi_version = PMAC_ABI_VERSION; rc.worker_threads = 2;
    status = pmac_runtime_create(&rc, &runtime);
    if (status != PMAC_OK) goto cleanup;
    cc.struct_size = sizeof(cc); cc.abi_version = PMAC_ABI_VERSION;
    cc.max_requests = 64; cc.max_file_tasks = 1; cc.max_retained_bytes = 80u * 1024u * 1024u;
    status = pmac_client_create(runtime, &cc, &client);
    if (status != PMAC_OK) goto cleanup;
    connection.struct_size = sizeof(connection); connection.abi_version = PMAC_ABI_VERSION;
    connection.host = bytes(host); connection.username = bytes("root");
    connection.password = bytes(getenv("PMAC_PASSWORD")); connection.known_hosts = bytes(known_hosts);
    connection.port = 22; connection.control_pool_size = 1;
    connection.flags = PMAC_CONNECT_TCP_PROBE; connection.tcp_probe_timeout_ms = 600;
    connection.keepalive_interval_ms = 1000; connection.keepalive_max = 3;
    status = pmac_connect_submit(client, &connection, &request);
    if (status != PMAC_OK) goto cleanup;
    do { status = pmac_request_wait(request, UINT32_MAX, &info); }
    while (status == PMAC_WAIT_PENDING);
    (void)pmac_request_release(request); request = 0;
    if (status != PMAC_OK) goto cleanup;
    config.struct_size = sizeof(config); config.abi_version = PMAC_ABI_VERSION;
    config.period_seconds = 0.01; /* required: zero does NOT select a default */
    config.flags = PMAC_GETERRORS_DATE | PMAC_GETERRORS_GLOBAL_MASK |
                   PMAC_GETERRORS_COORD_MASK | PMAC_GETERRORS_MOTOR_MASK | PMAC_GETERRORS_RING_MASK;
    config.global_mask = UINT64_C(0x7ff); config.coord_mask = UINT64_C(0x180000000807ffc);
    config.motor_mask = UINT64_C(0x8c007ffc); config.ring_mask = UINT64_C(0x803e);
    status = pmac_geterrors_start(client, &config, on_message, NULL, &monitor);
    if (status != PMAC_OK) goto cleanup;
    puts("Monitoring. Press Enter to stop.");
    (void)getchar();
cleanup:
    if (status != PMAC_OK) { fprintf(stderr, "operation failed: %d\n", (int)status); failed = 1; }
    if (monitor) {
        stopped = pmac_geterrors_stop(monitor);
        released = pmac_geterrors_release(monitor); monitor = 0;
        /* A recorded terminal error can be returned by release after ID destruction. */
        if (stopped != PMAC_OK || released != PMAC_OK) {
            fprintf(stderr, "Monitor stop=%d release=%d\n", (int)stopped, (int)released);
            failed = 1;
        }
    }
    if (client) {
        status = pmac_client_close(client, 0, 5000);
        if (status == PMAC_OK) status = pmac_client_release(client);
        if (status != PMAC_OK) {
            fprintf(stderr, "Client cleanup failed/pending: %d; client=%" PRIu64
                    " runtime=%" PRIu64 " retained\n", (int)status, client, runtime);
            return 1; /* standalone process exits; embedding applications retain and retry IDs */
        }
    }
    if (runtime) {
        status = pmac_runtime_shutdown(runtime, 5000);
        if (status == PMAC_OK) status = pmac_runtime_release(runtime);
        if (status != PMAC_OK) {
            fprintf(stderr, "Runtime cleanup failed/pending: %d; id=%" PRIu64
                    " retained\n", (int)status, runtime);
            return 1;
        }
    }
    return failed;
}
```
## 回调、停止和释放

- 回调运行在独立线程，同一 Monitor 的回调串行执行，且可能在 start 返回前到达；提前准备好 `user` 或 C++ 捕获的业务对象。
- C 的 `text` 是带长度的 UTF-8 字节视图，不保证以 NUL 结尾，当前回调返回后即失效。跨线程投递必须复制文本、kind 和 status；业务队列也应有容量限制。
- 自身回调内只能调用 `pmac_geterrors_request_stop` / `monitor.requestStop()` 或返回非零来请求停止。不要调用阻塞的 stop/release、销毁 Monitor、关闭 Client 或关闭 Runtime。
- 主线程调用 `stop()` 后，库向本次 Monitor 的专用 PTY 发送 Ctrl+C，并给进程至多 2 秒退出；不能正常结束时报告清理错误。实现不执行 `killall`，不终止其他 geterrors/Shell 会话。
- stop 是回调生命期屏障：在回调外返回或抛出终止错误后，不会再调用该 Monitor 的回调。停止时不保证投递完所有排队消息。
- C 的 stop 幂等，保留终态错误供再次查询。对有效句柄调用 release，即使返回此前的终态或清理错误，也已销毁句柄，不要重试 release；在自身回调内返回 `PMAC_INVALID_ARGUMENT` 时不会销毁。
- start 失败时输出 ID 为 0，库等待已启动的回调停止后才返回。启动成功后，应用必须在关闭 Client/Runtime 前停止并释放 Monitor。C++ Monitor 持有 RuntimeOwner，防止其依赖被提前析构。
- C++ 回调捕获的对象、C 的 user 内存以及应用处理队列应至少保留至停止屏障完成。显式停止后再销毁句柄，最后关闭 Client、释放子 ID 并关闭 Runtime。

C 示例是独立程序。如果嵌入长期运行的应用，关闭返回 `PMAC_SHUTDOWN_PENDING` 时必须保留 Client/Runtime ID，处理恢复条件并重试，不能卸载仍有后台工作使用的 DLL/so。详细生命周期和卸载屏障见 [ABI 契约](abi-contract.md)。监控不会自动重启或重放；重连后创建新的 Monitor。

## 编译与运行

在仓库根目录保存上述两个源文件，先构建原生库。也可直接使用仓库示例，将下列命令中的 `geterrors.c` 替换为 `examples/c/geterrors.c`、`geterrors.cpp` 替换为 `examples/cpp/geterrors.cpp`（Windows 使用反斜杠路径）。C++ 需要具有完整 C++17 标准库支持的编译器（包括 `<optional>`）。原生库和应用的位数、目标平台必须一致。CMake 定义 Windows 动态库为 `pmac.dll`、MSVC 导入库为 `pmac.dll.lib`；Linux 使用 `libpmac.so`。

Windows，x64 Native Tools Command Prompt for VS 2022：

```bat
cargo build -p pmac-ffi --release --locked
cl /nologo /W4 /TC /std:c11 /Iinclude geterrors.c target\release\pmac.dll.lib /Fe:geterrors_c.exe
cl /nologo /W4 /EHsc /std:c++17 /Iinclude /Ibindings\cpp geterrors.cpp target\release\pmac.dll.lib /Fe:geterrors_cpp.exe
set "PATH=%CD%\target\release;%PATH%"
set "PMAC_HOST=192.168.0.200"
set "PMAC_KNOWN_HOSTS=C:\pmac\known_hosts"
rem 按认证方式在当前环境提供 PMAC_PASSWORD；不要将凭据写入版本库。
geterrors_cpp.exe
rem 或运行 geterrors_c.exe
```

Linux，GCC/G++：

```sh
cargo build -p pmac-ffi --release --locked
cc -std=c11 -Wall -Wextra -Iinclude geterrors.c -Ltarget/release -lpmac \
  -Wl,-rpath,'$ORIGIN/target/release' -o geterrors_c
c++ -std=c++17 -Wall -Wextra -Iinclude -Ibindings/cpp geterrors.cpp \
  -Ltarget/release -lpmac -Wl,-rpath,'$ORIGIN/target/release' -o geterrors_cpp
export PMAC_HOST=192.168.0.200
export PMAC_KNOWN_HOSTS="$HOME/.ssh/known_hosts"
# 按认证方式在当前环境提供 PMAC_PASSWORD。
./geterrors_cpp
# 或运行 ./geterrors_c
```

显式使用 Cargo `--target <triple>` 时，链接路径改为 `target/<triple>/release`，并同步修改 PATH/rpath。发布安装路径见[构建与安装](build-and-install.md)。当前其他语言可通过此 C ABI 自行接入，尚无 Python/.NET 便捷封装。

## 容量和故障排查

默认队列配置的容量预算为 `queueLines × maxLineBytes`，上限 64 MiB；默认 256 × 65536 = 16 MiB，这是按最大行长计算的容量上界，不意味着每条短消息都实际占用 64 KiB。另有当前行、SSH 和业务复制缓冲。超长行或持续落后的回调会终止监控并报告资源错误，库不会默默截断状态文本。不要在回调线程执行耗时磁盘写入、UI 等待或无界队列增长。

| 现象 | 处理 |
| --- | --- |
| 配置被拒绝 / `PMAC_INVALID_ARGUMENT` | 显式填写 C 的 period_seconds，排除 0、NaN、无穷及超出 0.01..=1.0 的值；核对结构大小/版本、flags 和队列限制 |
| 启动失败或远端进程退出 | 确认控制器安装 geterrors、SSH 账号有执行权限，查看 STDERR 及终态消息；确认设备工具支持配置的选项 |
| 只看到 SETTINGS，没有 ERROR | SETTINGS 是启动参数回显；没有状态转变时没有新消息是正常情况，检查掩码和期望变化是否匹配，不要仅据此认定网络断开 |
| ERROR 收到普通变化 | 此工具监控状态转变；kind 名称不代表严重性，按设备文档和应用规则分类 |
| 收到 STDERR | 记录远端程序诊断，与设备状态变化文本分开处理；结合后续终态判断程序是否结束 |
| 队列溢出或超长行 | 将耗时工作移出回调，检查消息频率和行格式，在上限内调整 queueLines/maxLineBytes；两者乘积不得超过 64 MiB |
| SSH 断开 / Client 关闭 | 收尾并释放原 Monitor，恢复已认证连接后重新启动；不存在独立 TCP 数据连接需要放行 |
| stop 返回清理超时 | 本次专用 PTY 进程未在停止预算内正常退出；记录终态，核实控制器会话状态。不要执行全局 killall 干扰其他监控 |
| release 返回非零 | 有效句柄可能已携带终态错误完成销毁；只有自身回调拒绝或无效 ID 等入口错误不代表完成正常释放，遵守前述句柄规则 |

geterrors 的事件到达不能替代专用硬件安全链路，主机时间、日期选项或回调顺序也不能用来证明机械动作已经停止。监控采用 SSH 加密通道，其认证与安全配置应与命令接口保持一致。