# DART 接口使用指南

DART 通过独立 TCP 连接传输 P 变量和用户共享内存（USHM）的二进制数据。先完成 SSH 连接，再调用 DART 连接接口；库通过 GPASCII 读取 `Dart.Enable` 和 `Dart.Port`，要求服务已启用。完成操作后关闭 DART 连接。控制器对 DART 客户端数量有限制，应用应及时释放连接。

## 命令与地址

| 数据 | 读取命令 | 写入命令 | 元素字节数 | 地址 |
|---|---:|---:|---:|---|
| USHM char | `0x10` | `0x11` | 1 | USHM 字节偏移；对应 `Sys.Cdata[i]` |
| USHM int | `0x40` | `0x41` | 4 | 字节偏移，`Sys.Idata[i]` 对应 `i * 4` |
| USHM float | `0xf0` | `0xf1` | 4 | 字节偏移，`Sys.Fdata[i]` 对应 `i * 4` |
| USHM double | `0x80` | `0x81` | 8 | 字节偏移，`Sys.Ddata[i]` 对应 `i * 8` |
| P 变量 | `0x90` | `0x91` | 8 | 首个 P 变量编号，例如 P1 为 1 |

不提供 short 命令。长度始终以字节计，必须非零、为元素大小的整数倍，且不超过 524288。USHM 偏移加长度不得超过 1048576；P 变量范围为 P0～P65535，末尾不能越界。数据缓冲区使用调用方主机的原生字节序；库负责转换为网络传输格式。

## C ABI

声明及常量位于 `include/pmac.h`。四个接口均提交异步请求：成功提交返回 `PMAC_OK` 并在 `out_id` 写入请求 ID；操作结果需等待请求获取，最后释放请求。`options` 为请求选项，可使用库的默认选项约定；超时由 `timeout_ms` 指定。

- `pmac_dart_connect_async(client, options, out_id)`：发现服务端口并建立 DART 连接。
- `pmac_dart_disconnect_async(client, options, out_id)`：关闭 DART 连接。
- `pmac_dart_get_async(client, command, address, data_len, options, out_id)`：提交读取，通过 `pmac_request_wait` 等待，再用 `pmac_request_copy_response` 取得 `data_len` 字节。
- `pmac_dart_set_async(client, command, address, data, data_len, options, out_id)`：提交写入；提交时复制数据。完成确认后释放请求。

请求 ID 必须通过 `pmac_request_release` 释放。错误详情与执行确定性可通过请求错误接口读取。写入发送后失去确认时，操作可能已执行；先读取控制器状态，再决定是否重试。

## C++17

`pmac::Client` 提供 `dartConnect`、`dartDisconnect`、`dartGet`、`dartSet`，以及对应的 `dartConnectAsync`、`dartDisconnectAsync`、`dartGetAsync`、`dartSetAsync`。异步方法返回 `Request`，调用 `wait()` 后读取 `response()`。

```cpp
client.dartConnect();
auto bytes = client.dartGet(PMAC_DART_GET_P_VARIABLE, 1, sizeof(double));
double p1;
std::memcpy(&p1, bytes.data(), sizeof(p1)); // 需包含 <cstring>
client.dartDisconnect();
```

写入时将主机字节序的值复制到 `std::vector<uint8_t>`，再调用 `dartSet(PMAC_DART_SET_P_VARIABLE, 1, bytes)`。各方法均可传入 `RequestOptions`。

## Python

`Client` 提供 `dart_connect`、`dart_disconnect`、`dart_get`、`dart_set` 及同名 `_async` 方法。同步读返回 `bytes`；异步方法返回 `Request`，可使用 `with` 管理并调用 `wait()`、`response()`。选项通过关键字参数传递，例如 `timeout_ms=5000`。

```python
import struct

client.dart_connect(timeout_ms=5000)
try:
    raw = client.dart_get(0x90, 1, 8, timeout_ms=5000)
    p1 = struct.unpack('=d', raw)[0]
    # 确认 P1 可写后：
    client.dart_set(0x91, 1, struct.pack('=d', 100.0), timeout_ms=5000)
finally:
    client.dart_disconnect(timeout_ms=5000)
```

## .NET

`Client` 提供 `DartConnect`、`DartDisconnect`、`DartGet`、`DartSet` 及对应的 `Async` 方法。这里的 `Async` 方法返回 `Request`；使用 `using` 释放请求，调用 `Wait()` 或 `WaitAsync()` 等待。`DartGet` 返回主机字节序的 `byte[]`。

```csharp
client.DartConnect();
try
{
    byte[] data = client.DartGet(0x90, 1, 8);
    double p1 = BitConverter.ToDouble(data, 0);
    // 确认 P1 可写后：
    client.DartSet(0x91, 1, BitConverter.GetBytes(100.0));
}
finally { client.DartDisconnect(); }
```

## 并发与错误处理

同一客户端可从多个线程提交请求；应用应协调对同一地址的并发写入，避免依赖不同请求之间未经同步的顺序。DART 写入确认表示协议层收到确认，不代表机械运动完成。超时、断连或取消后的写入结果可能不确定，不应盲目重放。重新连接前应结束并释放原请求，必要时关闭并重建客户端。