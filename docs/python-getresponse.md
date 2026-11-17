# Python 调用：优先使用 GetResponse

[English](python-getresponse.en.md) | [简体中文](python-getresponse.md)

GetResponse 支持按请求启用透传：跳过规则表和 online/program 分类，原文提交单条命令。详见 **[GetResponse 透传模式与各语言示例](getresponse-passthrough.md)**。


Python 绑定位于 [`bindings/python`](../bindings/python)，通过 `ctypes` 调用本机库，无需编写扩展模块。先使用 `Client.get_response()` 完成普通命令请求；它会创建 Request、等待完成、复制响应并释放 Request。可运行示例见 [`examples/python/getresponse.py`](../examples/python/getresponse.py)，默认只读取 `Sys.ServoTime`。

## 1. 准备本机库与 Python 包

要求 Python 3.9 或更高版本。Python 进程与本机库必须使用相同架构，例如 Windows x64 Python 对应 x64 `pmac.dll`。先在仓库根目录构建：

```text
cargo build -p pmac-ffi --locked
```

调试库为 Windows `target/debug/pmac.dll`、Linux `target/debug/libpmac.so`。发布构建使用 `--release`，并相应改用 `target/release` 路径。平台依赖与安装要求见[构建说明](build-and-install.md)。

任选一种方式导入 Python 包：

- 安装到当前 Python 环境：`python -m pip install ./bindings/python`，发布包名为 `pmac-russh`，导入名为 `pmac`。
- 开发时设置 `PYTHONPATH` 指向仓库的 `bindings/python`，无需安装。

`PMAC_LIBRARY` 是示例读取的环境变量；示例把它解析为绝对路径并传给 `Runtime(library_path=...)`。绑定不会自行读取此变量。请使用与当前绑定版本匹配的本机库，避免搜索路径加载到旧 DLL。

## 2. 配置连接并运行

以下命令均从仓库根目录执行。`PMAC_KNOWN_HOSTS` 必须指向已核验控制器主机密钥的 known_hosts 文件；不要通过关闭主机校验绕过此配置。主机身份和密钥准备见[认证说明](authentication.md)。账号默认为 `root`，可通过 `PMAC_USER` 修改。密码仅从环境读取，不写进源码。

PowerShell：

```powershell
$env:PYTHONPATH = Join-Path $PWD 'bindings/python'
$env:PMAC_LIBRARY = (Resolve-Path 'target/debug/pmac.dll').Path
$env:PMAC_HOST = 'controller-host'
$env:PMAC_USER = 'root'
$env:PMAC_KNOWN_HOSTS = 'C:\pmac\known_hosts'
# 通过自己的凭据管理方式设置当前进程的 PMAC_PASSWORD。
python examples/python/getresponse.py --help
python examples/python/getresponse.py --native-version
python examples/python/getresponse.py
python examples/python/getresponse.py --command 'P1' --pattern variable
```

Linux：

```sh
export PYTHONPATH="$PWD/bindings/python${PYTHONPATH:+:$PYTHONPATH}"
export PMAC_LIBRARY="$PWD/target/debug/libpmac.so"
export PMAC_HOST='controller-host'
export PMAC_USER='root'
export PMAC_KNOWN_HOSTS="$HOME/.ssh/known_hosts"
# 通过自己的凭据管理方式设置当前进程的 PMAC_PASSWORD。
python3 examples/python/getresponse.py --help
python3 examples/python/getresponse.py --native-version
python3 examples/python/getresponse.py
python3 examples/python/getresponse.py --command 'P1' --pattern variable
```

`--help` 不导入绑定、不加载本机库、不建立连接。`--native-version` 仅加载本机库并报告版本和 ABI，无需控制器和连接凭据。其他调用需要全部连接环境变量。`--command` 会将参数作为控制器命令发送一次；自行传入修改或运动命令时，应由应用确认其影响。

## 3. 同步 GetResponse 与返回字节

最小调用关系如下，连接失败和命令失败均可能抛出 `PmacError`：

```python
import os
from pmac import Runtime

with Runtime(library_path=os.environ['PMAC_LIBRARY']) as runtime:
    with runtime.create_client() as client:
        client.connect(
            os.environ['PMAC_HOST'], os.environ.get('PMAC_USER', 'root'),
            os.environ['PMAC_PASSWORD'], known_hosts=os.environ['PMAC_KNOWN_HOSTS'],
        )
        data = client.get_response('Sys.ServoTime', timeout_ms=5000)
        print(data.decode('utf-8', errors='replace'))
```

返回值是 `bytes`，不保证一定是数字或文本。示例默认用 UTF-8 解码并用替换字符显示非法字节；只在末尾缺少换行时补换行，不裁剪响应内容。需要保存精确字节时，传 `--raw`，此模式不添加换行。不要对字节数使用解码后的字符串长度。

`--timeout-ms` 设置命令请求预算，示例允许 1..4294967295 毫秒，默认 5000；它与 SSH 建连预算、持续监控周期是不同概念。超时不代表命令一定没有执行，也不触发自动重试。退出码 0 表示示例完成，1 表示本机库或控制器请求失败，2 表示配置、导入或本地 I/O 失败，130 表示用户中断。

## 4. pattern 提取与捕获组

`get_response(command, pattern=None, **kwargs)` 的 `pattern` 在请求完成后从同一份响应提取结果，不重新发送命令。完整规则见[模式说明](pattern-migration.md)。

| pattern | 返回内容 |
| --- | --- |
| `None` 或空字符串 | 完整响应字节，空响应也有效 |
| `number` | 旧版兼容的宽松数字片段，不是严格数值解析 |
| `variable` | 首个 `=` 后的首个非空白片段 |
| 其他字符串 | Rust 字节正则的第 0 组，即整个匹配 |

`number` 和 `variable` 区分大小写。例如对 `P1=3.14`，`number` 返回 `b'1'`，`variable` 返回 `b'3.14'`；因此读取带变量名的响应时不能直接把 `number` 当作右侧数值提取器。`number` 也可能接受 `1e+` 这样的片段，业务需要数值时必须进一步验证。`variable` 要求响应确实包含等号，不能假设所有命令均返回同一格式。

```powershell
python examples/python/getresponse.py --command 'P1' --pattern variable
python examples/python/getresponse.py --command 'P1' --pattern '^P[0-9]+=(.+)$'
```

第二条命令提取第 0 组，仍包含 `P1=`。要得到捕获组 1、同时查看响应和独立 stderr，使用显式 Request：

```python
# 这里的 client 已按前例连接。
with client.get_response_async('P1', timeout_ms=5000) as request:
    request.wait()
    raw_response = request.response()
    stderr_bytes = request.stderr()
    value = request.match(r'^P[0-9]+=(.+)$', group=1)
    print(value.decode('utf-8', errors='replace'))
```

正则需要匹配控制器的实际输出格式，包括可能存在的换行；示例表达式用于说明捕获组。字节正则不支持环视和反向引用；无匹配不应作为有效数值继续处理，非法表达式报告解析错误。所有提取均在当前 Request 生命周期内完成，不重复发命令。

## 5. 将错误完整交给应用

可运行示例捕获 `PmacError`，向 stderr 输出 `status`、`certainty`、`request_id`，随后原样输出完整 `str(error)`；进程以非零状态退出。业务代码也应保留这些信息，不要仅返回空字符串或打印“通信失败”后继续执行。

```python
from pmac import PmacError

try:
    data = client.get_response('Sys.ServoTime', timeout_ms=5000)
except PmacError as error:
    # 将这些字段及完整文本交给应用日志、UI 或上层异常处理。
    print(error.status, error.certainty, error.request_id)
    print(str(error))
    raise
```

NAK 或 stdout 中的远端 `error #` 诊断按同步失败处理，包括 `stdin:1:1: error #31: Buffer closed due to another thread:`；库会保留原始诊断。ACK 到达即完成请求，stderr 中的 `error #` 仅作为异步诊断，不把 ACK 改判为失败，并可能随当前或后续请求的 `stderr()` 交付。不要自动补发 `close` 或重发命令；程序上传等需要业务成功保证的操作，应核对设备状态，不能只依赖 ACK。对于独立 stdout/stderr 的读取需求，应保留显式 Request，再用 `response()`/`stderr()` 获取字节，而不是另发一次命令。

执行确定性字段用于说明发送与确认状态：0 为 `NOT_SENT`，1 为 `MAY_HAVE_EXECUTED`，2 为 `CONFIRMED`。`CONFIRMED` 表示收到完整协议确认，不代表业务结果成功或机械动作已经结束。`MAY_HAVE_EXECUTED` 需要应用核对设备状态；涉及写入的恢复必须由应用明确确认，参见 [ABI 契约](abi-contract.md)。

## 6. 生命周期、线程与程序缓冲区

使用 `with Runtime(...)` → `with runtime.create_client()` 的嵌套顺序。显式 Request 应使用自己的 `with` 并在退出前完成等待与结果复制；同步 `get_response()` 已在内部处理 Request。退出时先关闭 Client、释放其资源，再关闭 Runtime；不要提前卸载本机库或依赖解释器退出时的析构顺序。`Runtime` 不能在 `fork` 后作为子进程的新连接复用。

同一个 Client 最多同时占用 **16 个 GPASCII 在线通道**，包含池预建、正在创建、使用中、回收中和释放不确定的占用。第 17 个申请会报告资源限制，Python 的 GIL 不会绕过此上限。多个 Client 各有自己的额度，但外部程序和其他 Client 仍能影响控制器的全局缓冲区状态。

在 `open prog` 等缓冲区操作中，打开、写入正文与 `close` 必须在同一调用线程和原有通道上下文中完成，不能把各步骤分散到线程池。另一个线程、Client 或外部程序执行 `close all buffers` 后，原通道可能在下一次写入报告 error #31。应用必须获知错误，再决定如何恢复；此示例不补发 `close`。

## 7. Gather 与 geterrors（后续功能）

普通请求接通后，再按需要接入 [Gather 连续采样](gather.md)和 [geterrors 实时状态变化监控](geterrors.md)。它们是持续会话，与单次 `get_response()` 的请求/响应周期不同。当前 Python 便捷绑定未封装这两类回调接口；现有详细指南提供 C ABI 与 C++17 用法，Python 若通过 `ctypes` 自行接入，必须管理回调引用、回调线程与停止/释放顺序。

`geterrors` 需要专用持续监听会话、以 Ctrl+C 退出远端程序；检测周期可配置为 0.01..1.0 秒，未指定的监控掩码沿用控制器默认值。不要把它当作普通单次 GetResponse 命令长期阻塞在 GPASCII 通道上。

GetResponse 也支持可选的 passthrough 请求选项，以原文发送单条命令并跳过 online/program 分类；默认开启。各语言写法、单帧限制与错误语义见[透传模式说明](getresponse-examples.md#可选透传模式跳过-onlineprogram-规则)。
