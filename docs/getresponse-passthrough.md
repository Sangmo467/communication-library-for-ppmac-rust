# GetResponse 透传模式

[English](getresponse-passthrough.en.md) | [简体中文](getresponse-passthrough.md)

GetResponse/command 默认开启透传：跳过 online/program 分类及本地程序上下文检查，原文逐行发送。显式关闭透传可启用规则校验：C 使用 PMAC_REQUEST_CLASSIFY（8），C++ 使用 passthrough=false，C# 使用 Passthrough:false，Python 使用 passthrough=False，MATLAB 使用 struct('passthrough', false)。只读池请求自动启用分类；C 显式 PASSTHROUGH（4）不可与 READ_POOL 或 CLASSIFY 组合。基础帧校验和远端 error # 上报始终生效。

适用于 C/C++、C#、Python 和 MATLAB。设置为 true 只影响当前请求；后续请求不指定时仍默认透传。连接步骤见 [GetResponse 基础指南](getresponse-examples.md)。

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

透传请求统一按可能产生副作用的控制写入调度，保留线程通道亲和；显式 C 透传标志不可与 READ_POOL 组合，否则发送前返回参数错误；各语言绑定指定只读池时自动选择分类模式。请求超时、取消、响应上限、资源配额、错误上报和不自动重放的约定继续生效。任何 `error #N`，包括 #31，仍返回失败和原始诊断；透传模式不会根据程序语义自动补发 close 或修复上下文。

这里的透传仍是一条 GPASCII 帧：最大 1023 字节，拒绝空白命令、NUL、CR/LF、ACK 和 NAK，不能在一个字符串中拼接多行帧。多行程序应逐行请求；这项选项不改变响应正则提取。C ABI 的 RequestOptions 布局不变，透传 flag 为 4，显式分类 flag 为 8；需使用包含此功能的新原生库，旧库会拒绝未知标志。`pmac_command_submit` 也支持同一选项；其他业务接口不使用它绕过规则。


## 完整示例：同一线程逐行提交程序

以下代码假设 Client 已连接。命令会实际写入程序缓冲区，请按应用需要选择程序编号和正文。每条请求都等待成功后才发送下一条；发生异常后停止，不自动重发，不自动补发 close。

```cpp
pmac::RequestOptions options;
options.passthrough = true;
client.getResponse("open prog 100", "", options);
client.getResponse("dwell 10", "", options);
client.getResponse("close", "", options);
```

C 使用原有异步请求与等待流程；NULL 选项或 flags=0 默认透传，下面的 flag 4 也可显式指定透传，flag 8 则关闭透传：

```c
pmac_request_options options = {0};
options.struct_size = sizeof(options);
options.abi_version = PMAC_ABI_VERSION;
options.timeout_ms = 5000;
options.max_response_bytes = 1024 * 1024;
options.flags = PMAC_REQUEST_PASSTHROUGH;
pmac_request_id request = 0;
int32_t status = pmac_get_response_async(client,
    (const uint8_t*)"dwell 10", 8, &options, &request);
/* 检查提交状态，等待 request，检查执行结果，复制响应，再释放 request。
   完整等待、错误复制与收尾代码见 examples/c/getresponse.c。 */
```

## 默认模式与透传模式如何选择

显式关闭透传后，规则校验模式分类命令并维护本地程序上下文；透传是默认模式。透传模式适用于应用希望自行管理控制器状态、发送尚未收录的设备命令或程序正文的场景。设备仍会根据当前实际状态解释和检查命令；本地允许提交不代表设备一定执行成功。

要关闭透传，必须显式设为 false；C 指定 PMAC_REQUEST_CLASSIFY（8）。省略选项仍为透传。不要在一段尚未结束的程序操作中来回切换模式，因为透传不会更新默认模式依赖的本地 open/close 状态。

## 常见问题

- 报未知 flags：检查加载的 DLL/so 是否包含本功能；旧版本原生库可能不支持 flag 4 或 8。
- 透传仍返回 error #：这是设备返回的错误，应用必须处理原文；绕过本地规则不会绕过设备检查。
- read_pool：请求必须通过只读分类；显式 C 透传标志与 READ_POOL 冲突，语言绑定会自动选择分类模式。
- 多行字符串被拒绝：每次提交一条不含 CR/LF 的命令，逐行等待结果。
- `CONFIRMED` 后提取报错：命令可能已执行，提取失败不是可以重发写入命令的依据。

## 独立复位命令

单独一条 `$$$` 或 `$$$***` 即使开启透传，也保留专用复位帧处理和设备级维护屏障：等待完整确认，或启动提示后以 Sys.ServoCount 探测就绪。文本仍原样发送，不自动重发；复合透传命令不在此识别范围内。返回给调用者的命令响应会过滤 STDIN Open for ASCII Input。

设备实测复位可能超过默认5000ms，调用者应显式设置足够的timeout（例如30000ms），库不隐式延长调用者预算。复位成功后旧GPASCII通道失效，需显式repair连接池或创建新Client，不能假定原线程通道仍可直接使用。
