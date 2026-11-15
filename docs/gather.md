# 实时连续 Gather

[中文](gather.md) | [English](gather.en.md)

当前提供 C ABI 和 C++17 封装。复用已连接 Client 的 SSH 认证和 GPASCII 请求，不需要再次传入控制器账号。反向数据连接必须通过控制器来源 IP 检查和本次会话随机握手，验证完成后才交付采样回调。

## C++17

```cpp
#include "pmac.hpp"
#include <atomic>
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

        pmac::GatherConfig config;
        config.period = 1;
        config.addresses = {"Motor[1].ActPos.a", "Motor[2].ActPos.a"};
        config.port = 18999; // Change this port, or use 0 for an available port.
        std::atomic<uint64_t> samples{0};
        auto gather = client.startGather(config,
            [&](const pmac::GatherBatch& batch) {
                // batch.at(row, 0/1) contains Motor[1/2].ActPos.a.
                // Copy with batch.copy() before handing values to another thread.
                samples.fetch_add(batch.rows, std::memory_order_relaxed);
            },
            [](const pmac::Error& error) {
                std::cerr << "Gather stopped: " << error.what() << '\n';
            });
        std::cout << "Gather running. Press Enter to stop.\n";
        std::cin.get();
        gather.stop();
        std::cout << "Received " << samples.load() << " sample rows.\n";
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
```


`period` 直接设置 `Gather.Period`，单位沿用控制器的伺服周期；`Gather.Items` 自动使用 `addresses.size()`，每个地址按原顺序写入 `Gather.Addr[i]`。地址支持单个符号或数值地址表达式，例如 `Motor[1].ActPos.a`、`Sys.P[8190].a`、`$100000`；不接受多条命令、换行或计算表达式。库最多接受 128 项，实际可用项数仍取决于控制器及其配置，控制器拒绝时返回错误。

`GatherBatch` 是借用视图：`rows` 为采样组数，`items` 为地址数量，`data` 为行优先 `double` 数组，`data[row * items + item]` 对应 `Gather.Addr[item]`。`firstSample` 是本次会话从 0 开始的首行序号。它不是控制器时间戳，库不会将本机接收时间当作采样时间。

完整接入示例见 [gather.cpp](../examples/cpp/gather.cpp)。

## 配置

| C++ 字段 | 默认值 | 含义 |
| --- | --- | --- |
| `period` | `1` | `Gather.Period`，必须大于 0 |
| `addresses` | 空 | 必填地址列表；数量自动成为 `Gather.Items` |
| `port` | `0` | 本机 TCP 监听端口；可指定 18999 等端口，0 自动分配 |
| `localIp` | 空 | 自动从 `SSH_CONNECTION` 获取 SSH 对端所见的本机 IP；可覆盖为本机数字 IP |
| `maxSamples` | `2048` | 范围 2..=1048576；控制器环形采集缓冲的 `Gather.MaxSamples`，不是会话采样总数 |
| `batchRows` | `128` | 范围 1..=4096；每批最多行数；本机已收到的数据不足一批时约每 20 ms 推送一次；控制器输出缓冲和调度会增加延迟 |
| `queueBatches` | `32` | 范围 1..=1024；接收线程与回调线程之间的有界批次队列 |
| `timeoutMs` | `5000` | 每个配置/清理请求及等待已验证反向连接的超时；拒绝连接不会重置等待截止时间，不限制采集总时长 |

修改端口只需设置 `config.port`。库先成功监听该端口，再把实际端口传给控制器的 `nc`；端口占用或本机 IP 无法绑定会在启动时返回连接错误。使用固定端口时，应允许控制器通过防火墙访问该端口。自动地址在 NAT、VPN 等情况下可能无法绑定或被控制器访问，此时设置 `localIp` 为控制器可达且本机可绑定的地址。

## 回调和停止

接收线程持续读取 TCP、解析文本、放入有界队列；独立回调线程串行投递批次。回调较慢时由队列吸收短时波动。长期跟不上会终止采集，通过错误回调及 `stop()` 返回资源限制错误，避免静默丢弃采样。队列最多配置为 64 MiB 数值数据，另有少量接收和当前批次缓冲。控制器报告 overrun、列数错误、非有限数值或数据连接关闭同样终止采集。

- 数据指针只在当前回调中有效。需要保留时调用 `batch.copy()`。
- 回调可能在 `startGather()` 返回前发生，因此应提前创建回调依赖的业务状态。
- 回调运行在后台线程。UI 更新应转交 UI 线程；长时间存盘或计算应交给业务线程。
- 回调内可以调用 `requestStop()`；它只发出停止请求。不要在回调内调用阻塞的 `stop()`、销毁 Gather、关闭 Client 或关闭 Runtime。
- 在回调外调用 `stop()`，它等待接收线程、清理任务和正在执行的回调结束，返回或抛出终止错误后均不会再回调。主动停止会丢弃尚未投递的排队批次。
- C++ 回调抛出的异常会被捕获，并请求停止，之后在 `stop()` 中重新抛出，不跨越 C ABI。
- C++ Gather 析构时停止并释放会话；析构不抛异常。需要获知采集或清理失败时显式调用 `stop()`。
- 先停止并销毁 Gather，再关闭 Client 和 Runtime。Gather 句柄未释放时，Runtime 的释放返回 `PMAC_SHUTDOWN_PENDING`。

## 单会话约定和控制器要求

先写入地址、Period、Items、MaxSamples，并执行 `Gather.Enable=1`、`Gather.Enable=0` 初始化；再经 SSH 启动包含会话握手前缀的 `gather -w` 到 `nc` 的管道，验证反向连接后执行 `Gather.Enable=3`。小写 `-w` 按控制器手册表示等待采集开始，停止采集后继续等待，进程由停止清理任务终止。采集没有累计样本上限。

控制器需要可执行 `gather`、`stdbuf`、`nc`、`nohup`、`sh`、`mkfifo`、`killall`、`pgrep` 和 `sleep`，并能够回连本机 TCP 端口。数据通道是控制器到本机的普通 TCP，适用于可直连的控制器网络。

库从已认证 SSH Shell 的 `SSH_CONNECTION` 获取控制器地址，检查反向 TCP 的来源 IP，并要求发送端提供本次启动独立生成的 256 位安全随机握手值。握手值经 SSH 配置到远端管道；错误来源、错误或旧会话握手不会成为采样数据源，库关闭该连接并继续等待。每个候选连接的握手最多等待 200 ms，且不得超过同一个启动截止时间；控制器启动管道所用时间也计入该截止时间。沉默连接、错误来源或重复错误握手均不会延长启动等待。握手不作为采样数据交给用户。

`localIp` 仅设置本机监听及回连目的地址，不能覆盖预期控制器来源。`SSH_CONNECTION` 缺失、地址非法，或 NAT、多网卡路由导致回连源地址与 SSH 服务端地址不一致时，启动会失败；库不会退回接受任意来源。部署时应保证回连来源地址一致。

**安全边界：来源 IP 和会话握手可阻止不知道当前握手值的无关连接抢占，数据流仍未加密。握手会随 TCP 明文发送，不能防御链路监听后的冒用、数据篡改或中间人，也不能替代 SSH 隧道。应限制监听端口只允许指定控制器访问；需要与 SSH 等价的数据机密性和完整性时，应在受信任的加密网络中使用，当前接口不提供 SSH 数据隧道。**

Gather 配置为控制器全局状态，一台控制器同一时间只能运行一个采集会话。库使用 `/tmp/pmac-library-gather.lock` 防止本库会话互相覆盖。其他软件也必须遵守独占约定，不应同时改写 Gather 配置。

按照单会话约定，启动前及停止时执行 `killall gather nc`，因此该控制器上的 `gather` 和 `nc` 应专用于采集。正常停止先执行 `Gather.Enable=0`，再结束进程和移除本次会话的锁。

如果进程崩溃或 SSH 中断，库可能无法确认远端清理结果，不会擅自接管尚有锁的会话。确认旧会话已失效后，可通过现有命令/Shell 接口执行 `Gather.Enable=0` 以及以下恢复命令，再重新启动：

```sh
killall gather nc >/dev/null 2>&1 || true
rm -f /tmp/pmac-library-gather.lock/stdin /tmp/pmac-library-gather.lock/owner
rmdir /tmp/pmac-library-gather.lock
```

远端锁冲突返回启动错误；清理失败通过终止错误报告。`stop()` 不恢复启动前的 Gather 配置。

## C ABI

1. 用零初始化的 `pmac_gather_config` 设置 `struct_size`、`abi_version`、`period`、`items`、`addresses`，其余字段可保持 0 使用默认值。`port` 可指定任意可绑定的监听端口。
2. 调用 `pmac_gather_start(client, &config, callback, user, &id)`。配置和地址字符串在函数返回后即可释放，`user` 必须保留到停止完成。
3. 回调收到 `status == PMAC_OK` 时读取 `rows * items` 个 `double`；非零 `status` 表示终止错误，`message` 是带长度的 UTF-8 文本。所有指针只在本次回调期间有效。返回非零值请求停止。
4. 回调内可调用 `pmac_gather_request_stop(id)`；回调外用 `pmac_gather_stop(id)` 等待停止，再用 `pmac_gather_release(id)` 释放句柄。

`stop` 幂等，并保留终止错误供重复查询。`release` 即使返回此前的终止错误，也已完成停止和句柄销毁；不要再次释放。`stop/release` 在自身回调内返回 `PMAC_INVALID_ARGUMENT`，此时不销毁句柄。未捕获的 C/C++ 异常不得跨越原生回调边界。

新增接口保持已有 ABI 结构布局不变。其他语言可通过 C ABI 接入；当前新增的便捷封装为 C++17。

### 完整 C11 示例

将以下代码保存为 `gather.c`。示例用环境变量提供连接参数，主线程按 Enter 后停止；不需要在线程间共享 Gather ID。`PMAC_KNOWN_HOSTS` 指向已核验的 known_hosts 文件，密码可由 `PMAC_PASSWORD` 提供。使用私钥时可按[认证说明](authentication.md)填写连接选项。

```c
#include "pmac.h"
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct { uint64_t rows; } samples;

static pmac_bytes bytes(const char *s) {
    pmac_bytes b = { (const uint8_t *)s, s ? strlen(s) : 0 };
    return b;
}
static int32_t on_data(void *user, int32_t status, const double *data,
                       size_t rows, size_t items, uint64_t first_sample,
                       const uint8_t *message, size_t message_len) {
    samples *result = (samples *)user;
    if (status != PMAC_OK) {
        fprintf(stderr, "Gather terminal status=%d: ", (int)status);
        if (message_len) fwrite(message, 1, message_len, stderr);
        fputc('\n', stderr);
        return 0;
    }
    /* data[row * items + item], row < rows, item < items.
       first_sample is the zero-based sample index of this batch's first row. */
    if (first_sample == 0 && rows && items)
        printf("first batch: rows=%zu items=%zu first_sample=%" PRIu64
               " first_value=%.17g\n", rows, items, first_sample, data[0]);
    result->rows += rows;
    /* For another thread, copy all rows * items doubles into an owned buffer
       and enqueue it with the metadata. Never enqueue data/message/user pointers
       without separately preserving their ownership and lifetime. */
    return 0; /* nonzero requests stop; do not call stop/release here */
}
int main(void) {
    pmac_runtime_id runtime = 0;
    pmac_client_id client = 0;
    pmac_request_id request = 0;
    pmac_gather_id gather = 0;
    pmac_runtime_config rc = {0};
    pmac_client_config cc = {0};
    pmac_connect_options connection = {0};
    pmac_gather_config config = {0};
    pmac_request_info info = {0};
    pmac_bytes addresses[2];
    samples result = {0}; /* used by callback only until stop barrier */
    const char *host = getenv("PMAC_HOST");
    const char *known_hosts = getenv("PMAC_KNOWN_HOSTS");
    int32_t status = PMAC_OK, stopped, released;
    int failed = 0;
    if (!host || !*host || !known_hosts || !*known_hosts) {
        fputs("Set PMAC_HOST and PMAC_KNOWN_HOSTS\n", stderr);
        return 1;
    }
    rc.struct_size = sizeof(rc); rc.abi_version = PMAC_ABI_VERSION;
    rc.worker_threads = 2;
    status = pmac_runtime_create(&rc, &runtime);
    if (status != PMAC_OK) goto cleanup;
    cc.struct_size = sizeof(cc); cc.abi_version = PMAC_ABI_VERSION;
    cc.max_requests = 64; cc.max_file_tasks = 1;
    cc.max_retained_bytes = 80u * 1024u * 1024u;
    status = pmac_client_create(runtime, &cc, &client);
    if (status != PMAC_OK) goto cleanup;
    connection.struct_size = sizeof(connection);
    connection.abi_version = PMAC_ABI_VERSION;
    connection.host = bytes(host); connection.username = bytes("root");
    connection.password = bytes(getenv("PMAC_PASSWORD"));
    connection.known_hosts = bytes(known_hosts);
    connection.port = 22; connection.control_pool_size = 1;
    connection.flags = PMAC_CONNECT_TCP_PROBE;
    connection.tcp_probe_timeout_ms = 600;
    connection.keepalive_interval_ms = 1000; connection.keepalive_max = 3;
    status = pmac_connect_submit(client, &connection, &request);
    if (status != PMAC_OK) goto cleanup;
    do { status = pmac_request_wait(request, UINT32_MAX, &info); }
    while (status == PMAC_WAIT_PENDING);
    (void)pmac_request_release(request); request = 0;
    if (status != PMAC_OK) goto cleanup;
    addresses[0] = bytes("Motor[1].ActPos.a");
    addresses[1] = bytes("Motor[2].ActPos.a");
    config.struct_size = sizeof(config); config.abi_version = PMAC_ABI_VERSION;
    config.period = 1; config.items = 2; config.addresses = addresses;
    config.port = 18999; /* 0 selects an available local TCP port */
    status = pmac_gather_start(client, &config, on_data, &result, &gather);
    if (status != PMAC_OK) goto cleanup;
    puts("Gather running. Press Enter to stop.");
    (void)getchar();
cleanup:
    if (status != PMAC_OK) {
        fprintf(stderr, "operation failed: %d\n", (int)status); failed = 1;
    }
    if (gather) {
        stopped = pmac_gather_stop(gather); /* callback lifetime barrier */
        released = pmac_gather_release(gather); gather = 0;
        /* A terminal acquisition/cleanup error can be returned again by release;
           the valid handle has nevertheless been destroyed. Do not retry it. */
        if (stopped != PMAC_OK || released != PMAC_OK) {
            fprintf(stderr, "Gather stop=%d release=%d\n", (int)stopped, (int)released);
            failed = 1;
        }
        printf("Received %" PRIu64 " sample rows\n", result.rows);
    }
    if (client) {
        status = pmac_client_close(client, 0, 5000);
        if (status == PMAC_OK) status = pmac_client_release(client);
        if (status != PMAC_OK) {
            fprintf(stderr, "Client cleanup pending/failed: %d; client=%" PRIu64
                    " runtime=%" PRIu64 " retained\n", (int)status, client, runtime);
            return 1; /* standalone process exits; see retry guidance below */
        }
        client = 0;
    }
    if (runtime) {
        status = pmac_runtime_shutdown(runtime, 5000);
        if (status == PMAC_OK) status = pmac_runtime_release(runtime);
        if (status != PMAC_OK) {
            fprintf(stderr, "Runtime cleanup pending/failed: %d; id=%" PRIu64
                    " retained\n", (int)status, runtime);
            return 1;
        }
    }
    return failed;
}
```

本例为独立程序；如果集成到长驻进程，关闭失败时应保存输出的 Client/Runtime ID，完成恢复后继续重试关闭/释放，不能把返回 `PMAC_SHUTDOWN_PENDING` 当成已释放。先结束并释放 Gather，再关闭/释放 Client，最后 shutdown/release Runtime。连接请求已结束后立即释放 Request ID。动态卸载原生库前还须遵守 [ABI 生命周期契约](abi-contract.md)并等待清理监督线程，不能让后台工作继续使用已卸载的 DLL/so。

回调中的 `result` 在停止屏障后才由主线程读取，因此示例不需要跨线程原子计数。若主线程在采集中读取统计值，应添加原子变量或互斥保护。示例只在第一批输出少量信息；实际高频数据应复制后投递给有界业务队列，避免逐行控制台打印拖慢回调。
## 编译和运行

以下命令在仓库根目录执行；C 示例保存为根目录的 `gather.c`，C++ 使用仓库已有 `examples/cpp/gather.cpp`。应用和原生库必须使用相同位数与 ABI。仓库的 CMake 配置定义 Windows 动态库 `pmac.dll` 和 MSVC 导入库 `pmac.dll.lib`，Linux 动态库为 `libpmac.so`；当前 CMake 示例目标不包含 Gather 示例，需要按下面命令单独编译。

Windows（x64 Native Tools Command Prompt for VS 2022，MSVC）：

```bat
cargo build -p pmac-ffi --release --locked
cl /nologo /W4 /TC /std:c11 /Iinclude gather.c target\release\pmac.dll.lib /Fe:gather_c.exe
cl /nologo /W4 /EHsc /std:c++17 /Iinclude /Ibindings\cpp examples\cpp\gather.cpp target\release\pmac.dll.lib /Fe:gather_cpp.exe
set "PATH=%CD%\target\release;%PATH%"
set "PMAC_HOST=192.168.0.200"
set "PMAC_KNOWN_HOSTS=C:\pmac\known_hosts"
rem 如使用密码认证，请在当前环境设置 PMAC_PASSWORD，避免写入脚本或版本库。
gather_cpp.exe
rem 或运行 gather_c.exe
```

Linux（GCC/G++，终端）：

```sh
cargo build -p pmac-ffi --release --locked
cc -std=c11 -Wall -Wextra -Iinclude gather.c -Ltarget/release -lpmac \
  -Wl,-rpath,'$ORIGIN/target/release' -o gather_c
c++ -std=c++17 -Wall -Wextra -Iinclude -Ibindings/cpp examples/cpp/gather.cpp \
  -Ltarget/release -lpmac -Wl,-rpath,'$ORIGIN/target/release' -o gather_cpp
export PMAC_HOST=192.168.0.200
export PMAC_KNOWN_HOSTS="$HOME/.ssh/known_hosts"
# 如使用密码认证，在当前环境提供 PMAC_PASSWORD。
./gather_cpp
# 或运行 ./gather_c
```

若 Cargo 显式指定 `--target <triple>`，请将编译链接路径改为 `target/<triple>/release`，并相应修改 Windows PATH 或 Linux rpath。安装包的链接路径见[构建与安装](build-and-install.md)。运行前应由运维通过可信渠道核验控制器主机密钥，并准备 known_hosts；不要为了通过示例而开启接受任意主机密钥。

## 配置边界与容量规划

C 的 `period`、`items`、`addresses` 必填；`struct_size=sizeof(pmac_gather_config)`，`abi_version=PMAC_ABI_VERSION`，`reserved=0`。C/C++ 的 `max_samples/maxSamples`、`batch_rows/batchRows`、`queue_batches/queueBatches`、`timeout_ms/timeoutMs` 传 0 时使用默认值；默认替换后再检查范围。`period` 可取正 `uint32_t`，实际支持范围以控制器为准。地址数量为 1..=128，每项 UTF-8 字节长度 1..=256，只允许 ASCII 字母、数字及 `_.$[]`。控制器还会检查地址是否存在或适用于 Gather。

`local_ip/localIp` 为空时自动发现；手动设置必须为本机可绑定的数字 IPv4/IPv6 单播地址，不能使用主机名、通配地址、组播地址或 IPv4 广播地址。端口为 0 或 1..=65535。采样周期约为 `Gather.Period × 控制器伺服周期`，不能把 `period` 当作毫秒。

数值队列容量按 `batchRows × queueBatches × items × sizeof(double)` 计算，不得超过 64 MiB。例如 128 行、32 批、3 项为 98,304 字节；另需接收缓冲、当前批次和业务自行复制数据的内存。批次内的数据为连续 `rows × items` 个 double；`rows` 可能小于 `batchRows`，停止时未投递的排队批次会被丢弃。`maxSamples` 调节控制器环形缓冲，不会限制整个会话的累计样本数。

`timeoutMs` 是每次 SSH/GPASCII 配置或清理操作的预算，启动涉及多个操作，因此不是 `startGather()` 整体耗时上限。反向连接阶段另使用一个共同截止时间，管道启动、候选连接和握手共同消耗该预算；每个候选握手最多等待 200 ms，也不能越过共同截止时间。持续采样没有会话总时长限制。

## 常见错误与排查

| 现象或状态 | 检查和处理 |
| --- | --- |
| `PMAC_INVALID_ARGUMENT` | 检查结构大小/版本、保留字段、地址字符、配置范围以及队列是否超过 64 MiB；在自身回调中调用 stop/release 也会返回此错误，改用 request_stop 或返回非零 |
| `PMAC_CONNECT_ERROR` 或启动等待超时 | 检查本机 IP 是否可绑定、端口占用、防火墙、控制器到监听端口的路由；确认控制器具备 gather/stdbuf/nc/nohup/sh/killall |
| 来源 IP / 握手验证失败 | 检查 SSH_CONNECTION 是否有效、控制器回连源地址是否与 SSH 服务端地址一致，以及 NAT/VPN/多网卡路由；localIp 只改变监听和回连目的地址，不能放宽来源验证 |
| `PMAC_AUTHENTICATION` / `PMAC_HOST_KEY` | 在启动 Gather 前先修复 SSH 认证、私钥权限或 known_hosts/固定指纹，见认证文档 |
| 远端锁冲突或 `PMAC_REMOTE_ERROR` | 排查其他采集进程或控制器命令失败；确认旧会话已失效后再按下文恢复锁，不要抢占仍运行的会话 |
| `PMAC_RESOURCE_LIMIT` / overrun | 判断是控制器缓冲溢出还是本机回调队列满；减少地址数量、增大采样周期、在允许范围调整环形缓冲/队列，并把慢操作移出回调 |
| `PMAC_PARSE_ERROR` / `PMAC_PROTOCOL_ERROR` | 检查控制器 gather 输出格式、地址数量、每行列数和非有限数值；保留错误消息用于诊断，不要把异常数据当正常采样继续使用 |
| `PMAC_DISCONNECTED` / `PMAC_CLOSING` | 检查数据 TCP、SSH、Client/Runtime 是否已关闭；终止后先 stop/release，再重连并创建新 Gather，会话不会自动重放 |
| stop/release 返回清理错误 | 本机回调已停止，但远端是否关闭仍需核实；release 返回终态错误时有效句柄已销毁，不能再次释放 |
| Runtime 释放返回 `PMAC_SHUTDOWN_PENDING` | 先释放 Gather、Request 等子 ID，并完成 Client 关闭/释放；如果资源状态不确定，完成 ABI 文档规定的显式恢复后重试 |

手工恢复远端锁只适用于已确认旧采集停止、且控制器由当前应用独占的情况。先通过已认证 GPASCII 连接执行 `Gather.Enable=0`，再通过 Shell 执行前文的 `killall` 和锁目录清理命令；检查返回状态并确认进程和锁已消失，之后创建新 Gather。SSH 断线时先恢复可用的控制连接再处理，不要以删除本机句柄代替远端状态核实。恢复会停止该控制器上的 gather/nc 进程，不恢复此前 Gather 配置。
实机验证（2026-11-14）：无需 PTY。控制器上的 gather -w 在 stdin 为 /dev/null（EOF）时会重复输出等待提示而不交付数值。库在会话锁目录创建权限为 0600 的 FIFO，由后台 Shell 以读写方式打开，并作为 gather 的 stdin，保持输入打开且不发送数据；停止时终止 gather/nc 并移除 FIFO 和锁。无需额外常驻辅助进程。
