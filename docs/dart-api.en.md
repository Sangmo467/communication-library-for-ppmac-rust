# DART API Guide

[中文](dart-api.md) | [English](dart-api.en.md)

DART transfers binary P-variable and user shared memory (USHM) data over a separate TCP connection. Establish the SSH connection before calling the DART connection API. The library reads `Dart.Enable` and `Dart.Port` through GPASCII and requires the service to be enabled. Close the DART connection after completing operations. The controller limits the number of DART clients, so applications should release connections promptly.

## Commands and addresses

| Data | Read command | Write command | Element bytes | Address |
|---|---:|---:|---:|---|
| USHM char | `0x10` | `0x11` | 1 | USHM byte offset; corresponds to `Sys.Cdata[i]` |
| USHM int | `0x40` | `0x41` | 4 | Byte offset; `Sys.Idata[i]` corresponds to `i * 4` |
| USHM float | `0xf0` | `0xf1` | 4 | Byte offset; `Sys.Fdata[i]` corresponds to `i * 4` |
| USHM double | `0x80` | `0x81` | 8 | Byte offset; `Sys.Ddata[i]` corresponds to `i * 8` |
| P variable | `0x90` | `0x91` | 8 | Index of the first P variable; for example, P1 is 1 |

No short commands are provided. Length is always in bytes, must be nonzero and an integer multiple of the element size, and must not exceed 524288. The USHM offset plus length must not exceed 1048576. P variables range from P0 to P65535; the end must remain in range. Data buffers use the caller host's native byte order; the library converts them to the network transfer format.

## C ABI

Declarations and constants are in `include/pmac.h`. All four APIs submit asynchronous requests: successful submission returns `PMAC_OK` and writes the request ID to `out_id`. Wait for the request to obtain the operation result, then release it. `options` specifies request options and may follow the library's default-options convention; `timeout_ms` specifies the timeout.

- `pmac_dart_connect_async(client, options, out_id)`: discovers the server port and establishes the DART connection.
- `pmac_dart_disconnect_async(client, options, out_id)`: closes the DART connection.
- `pmac_dart_get_async(client, command, address, data_len, options, out_id)`: submits a read; wait with `pmac_request_wait`, then obtain `data_len` bytes with `pmac_request_copy_response`.
- `pmac_dart_set_async(client, command, address, data, data_len, options, out_id)`: submits a write and copies the data at submission. Release the request after completion is acknowledged.

Request IDs must be released with `pmac_request_release`. Read error details and execution certainty through the request error APIs. If acknowledgment is lost after a write is sent, the operation may already have executed; read controller state before deciding whether to retry.

## C++17

`pmac::Client` provides `dartConnect`, `dartDisconnect`, `dartGet`, and `dartSet`, along with their `dartConnectAsync`, `dartDisconnectAsync`, `dartGetAsync`, and `dartSetAsync` counterparts. Asynchronous methods return a `Request`; call `wait()` before reading `response()`.

```cpp
client.dartConnect();
auto bytes = client.dartGet(PMAC_DART_GET_P_VARIABLE, 1, sizeof(double));
double p1;
std::memcpy(&p1, bytes.data(), sizeof(p1)); // Requires <cstring>
client.dartDisconnect();
```

For writes, copy host-byte-order values into a `std::vector<uint8_t>`, then call `dartSet(PMAC_DART_SET_P_VARIABLE, 1, bytes)`. All methods accept `RequestOptions`.

## Python

`Client` provides `dart_connect`, `dart_disconnect`, `dart_get`, and `dart_set`, and `_async` methods with the same names. Synchronous reads return `bytes`; asynchronous methods return a `Request`, which can be managed with `with` and queried using `wait()` and `response()`. Pass options as keyword arguments, for example `timeout_ms=5000`.

```python
import struct

client.dart_connect(timeout_ms=5000)
try:
    raw = client.dart_get(0x90, 1, 8, timeout_ms=5000)
    p1 = struct.unpack('=d', raw)[0]
    # After confirming that P1 is writable:
    client.dart_set(0x91, 1, struct.pack('=d', 100.0), timeout_ms=5000)
finally:
    client.dart_disconnect(timeout_ms=5000)
```

## .NET

`Client` provides `DartConnect`, `DartDisconnect`, `DartGet`, and `DartSet`, and corresponding `Async` methods. These `Async` methods return a `Request`; release it with `using`, and wait using `Wait()` or `WaitAsync()`. `DartGet` returns a host-byte-order `byte[]`.

```csharp
client.DartConnect();
try
{
    byte[] data = client.DartGet(0x90, 1, 8);
    double p1 = BitConverter.ToDouble(data, 0);
    // After confirming that P1 is writable:
    client.DartSet(0x91, 1, BitConverter.GetBytes(100.0));
}
finally { client.DartDisconnect(); }
```

## Concurrency and error handling

Multiple threads may submit requests to the same client. Applications should coordinate concurrent writes to the same address and avoid relying on unsynchronized ordering between requests. DART write acknowledgment means acknowledgment at the protocol level, not completion of mechanical motion. Write outcomes after a timeout, disconnection, or cancellation may be uncertain; do not blindly replay them. After a data-channel timeout, cancellation, or disconnection, the library discards the connection. Subsequent new requests reconnect using the discovered port without replaying failed requests. When a write outcome is uncertain, read back the actual state before using the recovery acknowledgment API to clear write protection. After an explicit disconnect, call the connection API again.
