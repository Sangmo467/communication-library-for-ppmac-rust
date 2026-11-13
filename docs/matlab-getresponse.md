# MATLAB 调用：GetResponse MEX 示例

MATLAB 可以通过本项目 C ABI 调用原生库。这里提供 C++17 MEX 网关，优先实现连接和同步 GetResponse，不需要在 MATLAB 中传递原生对象指针或维护 64 位 Request ID。

文件： [构建脚本](../examples/matlab/build_pmac_mex.m)、[MEX 源码](../examples/matlab/pmac_mex.cpp)、[调用示例](../examples/matlab/example_getresponse.m)。MEX 每次维护一个 Runtime/Client 会话，可连续查询后再关闭。

## 1. 准备环境

需要 64 位 MATLAB、受该 MATLAB 版本支持的 C++17 编译器、相同架构的原生 pmac 库。构建脚本支持 Windows x64（MSVC 或 MATLAB 支持的 MinGW）与 Linux x64；本次在 Windows、MATLAB R2023a、Visual C++ 2022 上完成实际编译验证。其他 MATLAB 版本、编译器与 Linux 需要在目标环境验证。

先在仓库根目录的终端构建 Rust 原生库：

```sh
cargo build -p pmac-ffi --locked
```

Windows 应存在 `target/debug/pmac.dll` 和 MSVC 所需 `pmac.dll.lib`，Linux 应存在 `target/debug/libpmac.so`。不要把 MATLAB 托管文件误当作原生动态库。

再在 MATLAB 中将当前目录切换到仓库根目录：

```matlab
addpath(fullfile(pwd, 'examples', 'matlab'));
mex -setup C++
build_pmac_mex('debug');
version = pmac_mex('version')
```

发布库先执行 `cargo build -p pmac-ffi --release --locked`，再在 MATLAB 执行 `build_pmac_mex('release')`。自定义库目录使用 `build_pmac_mex('release', 'C:\pmac\native')`；Linux 对应绝对目录。脚本不会替你构建 Rust 或下载依赖。

脚本使用 `-R2018a` Matrix API，启用 C++17，并保留 MATLAB 默认异常处理选项；不要额外用 `/EHsc` 覆盖 MSVC 的 `/EHs`，否则可能影响 MEX 错误传播。Windows 自动将原生库目录加入当前 MATLAB 进程的 PATH，并验证 DLL 为 x86-64。Linux 通过 rpath 指向所选库目录，分发到其他路径时应重新构建或在启动 MATLAB 前设置 `LD_LIBRARY_PATH`。编译参数依据 [MathWorks mex 文档](https://www.mathworks.com/help/matlab/ref/mex.html)。

`pmac_mex('version')` 只加载库，不连接控制器。先完成这个检查，遇到缺失 DLL/依赖或架构不匹配时修正加载路径。

## 2. 用环境变量运行完整示例

在 MATLAB 中配置目标及已核验的 known_hosts；密码通过自己的凭据管理机制提供给环境变量 `PMAC_PASSWORD`，也可使用 `PMAC_PRIVATE_KEY` 和可选 `PMAC_PASSPHRASE`。

```matlab
setenv('PMAC_HOST', 'controller-host');
setenv('PMAC_USER', 'root');
setenv('PMAC_KNOWN_HOSTS', 'C:\pmac\known_hosts');
response = example_getresponse();               % Sys.ServoTime 原始响应转 UTF-8 文本
value = example_getresponse('P1', 'variable');  % 提取等号后的值
```

`example_getresponse` 负责连接、查询、UTF-8 解码和 `onCleanup` 关闭；每次调用建立独立会话。需要重复采集普通变量时，使用下一节的持久会话，避免每次重新握手。私钥与主机身份设置见[认证说明](authentication.md)。

## 3. 连接后重复 GetResponse

```matlab
options = struct;
options.host = getenv('PMAC_HOST');
options.username = 'root';
options.password = getenv('PMAC_PASSWORD');
options.knownHosts = getenv('PMAC_KNOWN_HOSTS');
pmac_mex('connect', options);
cleanup = onCleanup(@() pmac_mex('close'));

try
    raw = pmac_mex('getresponse', 'Sys.ServoTime');
    assert(isa(raw, 'uint8'));
    text = native2unicode(raw, 'UTF-8');
    fprintf('%s\n', text);

    rawValue = pmac_mex('getresponse', 'P1', 'variable');
    valueText = native2unicode(rawValue, 'UTF-8');
    value = str2double(valueText);
    if isnan(value), error('example:InvalidNumber', 'Unexpected numeric response: %s', valueText); end
catch exception
    fprintf(2, '%s: %s\n', exception.identifier, exception.message);
    rethrow(exception);  % 保留原始诊断，由上层处理
end
clear cleanup          % 正常路径现在关闭会话
```

脚本中也可显式 `pmac_mex('close')`；重复关闭已关闭会话允许成功。复用时整个连接和请求顺序在 MATLAB 调用线程执行。MEX 不返回原生指针，也不把 ID 转换为会丢失精度的 double。

## 4. 网关命令与配置

| 调用 | 返回或行为 |
| --- | --- |
| `pmac_mex('version')` | 有输出时返回版本字符串；无输出时打印版本和 ABI |
| `pmac_mex('connect', options)` | 建立会话；已有会话时要求先 close；可返回 logical true |
| `raw = pmac_mex('getresponse', command)` | 一次同步请求，返回 uint8 行向量 |
| `raw = pmac_mex('getresponse', command, pattern)` | 在同一次响应上提取 pattern 的 group 0 |
| `pmac_mex('close')` | 等待关闭并释放会话与清理任务；可返回 logical true |

每次调用接受零或一个输出。GetResponse 不接收输出时仍会执行命令，结果被丢弃。命令与 pattern 支持 char 行向量或 string 标量，不支持 NUL；本示例将单个文本参数限制为 64 KiB。

connect 的字段均可省略，除非下面要求必填；未知字段报参数错误。

| 字段 | 默认值与含义 |
| --- | --- |
| `host` | 必填，非空地址 |
| `username` | `root`，非空 |
| `port` | 22，范围 1～65535 |
| `password` | 空，密码认证 |
| `privateKey` / `publicKey` / `passphrase` | 空，密钥路径与私钥口令 |
| `knownHosts` | 空，可信 known_hosts 路径；示例要求提供 |
| `pinnedSha256` | 可选，32 个 uint8 字节的 SHA-256 主机指纹，不是十六进制字符串 |
| `acceptAnyHostKey` | false；需要可信 knownHosts 或固定指纹 |
| `tcpProbe` / `tcpProbeTimeoutMs` | true / 600 毫秒 |
| `keepaliveIntervalMs` / `keepaliveMax` | 1000 毫秒 / 3 次 |
| `workers` | 2，范围 1～256，Runtime worker 数 |

整数配置要求非负整数标量；bool 配置接受 logical 或数值 0/1。示例固定初始控制池为 1，暂未暴露每次请求 timeout、取消或异步接口，需要时扩展 MEX。原生 worker 数不等于 GPASCII 通道数。

## 5. 错误、模式和关闭

空 pattern 保留完整响应；`variable` 提取 `=` 后面的值；兼容 `number` 对 `P1=3.14` 会先得到 `1`。普通正则默认返回 group 0，捕获括号不会自动取组 1。详见[模式规则](pattern-migration.md)。MEX 保留响应字节，只有示例脚本将其按 UTF-8 解码；二进制数据应保留 uint8。

原生错误抛为 MATLAB 异常 `pmac:Native`，message 包含完整诊断、status、certainty 和 Request ID；参数错误为 `pmac:Argument`。包括 `error #31` 在内的远端错误不能作为正常响应忽略，也不自动重发或补发 close。`CONFIRMED` 不等于业务成功，`MAY_HAVE_EXECUTED` 需要应用核对设备状态。

MEX 建立会话时调用 mexLock；成功关闭所有原生资源并等待清理后才 mexUnlock。请先 `pmac_mex('close')`，再 `clear pmac_mex` 或重新编译。若关闭报错，会保留会话和锁以便重试；此时不能直接卸载 DLL。MATLAB 退出回调也会尝试完成原生清理，持续失败时可能延长退出等待。`onCleanup` 是辅助机制，应用仍需记录关闭错误并显式处理。相关行为见 MathWorks 的 [mexLock](https://www.mathworks.com/help/matlab/apiref/mexlock.html)、[mexUnlock](https://www.mathworks.com/help/matlab/apiref/mexunlock.html) 和 [mexAtExit](https://www.mathworks.com/help/matlab/apiref/mexatexit.html)。

## 6. loadlibrary 与后续回调扩展

MATLAB 的 `loadlibrary` / `calllib` 可用于 C 共享库，但 `loadlibrary` 不支持函数指针输入参数。本项目完整头文件还包含回调定义，因此这里使用编译式 MEX，避免要求使用者手写可解析头文件和指针封送；这不表示全部 C 接口都无法通过 calllib 调用。限制见 [MathWorks loadlibrary 文档](https://www.mathworks.com/help/matlab/ref/loadlibrary.html)。

本示例仅支持 GetResponse，不提供 Gather/geterrors 接口。后续扩展时，原生回调只复制数据到有界队列，由 MATLAB 调用 MEX 主动轮询取得数据；还需实现溢出策略、停止屏障与释放顺序。不要直接从原生回调线程调用 mx/mex API 或 mexPrintf，依据 [MEX/Matrix API 线程限制](https://www.mathworks.com/help/matlab/matlab_external/mex-api-is-not-thread-safe.html)。

接通普通请求后再参阅 [Gather](gather.md)和 [geterrors](geterrors.md)的 C/C++ 专用接口。geterrors 持续监听直到 Ctrl+C 停止，检测周期支持 0.01～1.0 秒，掩码未指定时使用系统默认值。

GetResponse 也支持可选的 passthrough 请求选项，以原文发送单条命令并跳过 online/program 分类；默认关闭。各语言写法、单帧限制与错误语义见[透传模式说明](getresponse-examples.md#可选透传模式跳过-onlineprogram-规则)。
