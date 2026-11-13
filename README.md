# Communication Library for Power PPMAC

基于 Rust 与 russh 的 Power PPMAC 通信库，提供统一的 C ABI，以及 C++17、C++98、.NET 8 和 Python 绑定。

当前版本：**2.0.0**；C ABI：**2.1**；开源协议：**MIT**。

## 功能

- GPASCII 命令、同步/异步 GetResponse、响应与正则捕获。
- 严格调用线程与 GPASCII Channel 亲和，支持显式连接池和故障隔离。
- 持久 Shell、隔离 exec、SFTP 上传/下载和程序 buffer 上传。
- 电机、坐标系、PLC 常用操作及状态查询。
- 请求取消、执行 deadline、有界队列、完成通知和确定性错误状态。
- known_hosts、SHA-256 主机指纹、密码和私钥认证。

## 仓库内容

公开仓库仅包含核心源码、公开头文件、语言绑定、构建配置、依赖锁和使用文档。不包含编译产物、独立测试程序、烟测程序、内部审核记录或现场验证数据。

核心 crate：

- `pmac-protocol`：GPASCII/Shell 协议、数值解析、状态位与模式提取。
- `pmac-core`：Runtime、Client、Request、连接池、取消和生命周期。
- `pmac-transport-russh`：SSH、GPASCII、Shell 与 SFTP 传输。
- `pmac-ffi`：稳定的 `pmac_` C ABI，生成 `pmac.dll` 或 `libpmac.so`。

## 构建

需要 Rust 1.99.1（已由 `rust-toolchain.toml` 固定）和对应平台的 C/C++ 链接环境。

```sh
cargo build -p pmac-ffi --release --locked
```

产物位于 `target/release/`；显式指定 Rust target 时位于 `target/<triple>/release/`。

也可以使用 CMake 构建并安装头文件、C++ 绑定和原生库：

```sh
cmake -S . -B build/native -DCMAKE_BUILD_TYPE=Release
cmake --build build/native --config Release
cmake --install build/native --config Release --prefix /your/install/prefix
```

Windows 使用 Visual Studio 生成器时可省略 `CMAKE_BUILD_TYPE`，保留 `--config Release`。详细步骤见[构建与安装手册](docs/build-and-install.md)。

## 快速使用

Python >=3.9：

```python
import os
import pmac

with pmac.Runtime(library_path="target/release/pmac.dll") as runtime:
    with runtime.create_client() as client:
        client.connect(
            os.environ["PMAC_HOST"],
            os.environ.get("PMAC_USER", "root"),
            password=os.environ.get("PMAC_PASSWORD"),
            known_hosts=os.environ.get("PMAC_KNOWN_HOSTS"),
        )
        print(client.get_response("Sys.ServoTime").decode("ascii").strip())
```

Linux 将 `library_path` 改为 `target/release/libpmac.so`，也可传入原生库的绝对路径。

C++17：

```cpp
#include "pmac.hpp"
#include <cstdlib>
#include <iostream>

int main() {
    pmac::Runtime runtime;
    pmac::Client client = runtime.createClient();
    pmac::ConnectOptions options;
    options.host = std::getenv("PMAC_HOST");
    options.username = "root";
    options.knownHosts = std::getenv("PMAC_KNOWN_HOSTS");
    if (const char* password = std::getenv("PMAC_PASSWORD")) options.password = password;
    client.connectSync(options);
    auto response = client.getResponse("Sys.ServoTime");
    std::cout.write(reinterpret_cast<const char*>(response.data()), response.size());
}
```

## 文档

- [DART 接口指南](docs/dart-api.md)：连接、数据类型、C/C++、Python 与 .NET 使用。
- [使用手册](docs/user-manual.md)：四种语言接入、请求、连接池、业务操作、恢复和关闭。
- [构建与安装](docs/build-and-install.md)：原生库、CMake 安装和各语言加载方式。
- [认证与主机身份校验](docs/authentication.md)：密码、私钥、known_hosts 与固定指纹。
- [C ABI 契约](docs/abi-contract.md)：结构布局、ID、状态码、请求结果与恢复语义。
- [GetResponse 模式](docs/pattern-migration.md)：兼容提取和字节正则规则。
- [第三方组件与许可证](THIRD_PARTY_NOTICES.md)。
- [版本变更](CHANGELOG.md)。

## 重要使用边界

- `CONFIRMED` 表示取得完整协议确认，不表示机械运动到位或程序执行完成。
- 已发送后失去确认的操作返回 `MAY_HAVE_EXECUTED`；库不会自动重放可能有副作用的命令。
- Abort、Kill、JogStop 等网络命令不能替代独立硬件安全链路。
- 生产环境必须校验 SSH 主机身份；不要记录密码、私钥内容或私钥口令。
- 不同线程或连接池成员之间不保证隐式写后读顺序或同一时刻快照。
- 卸载 DLL/so 前必须完成 Runtime 关闭及所有子对象释放。

## 许可证

本项目采用 [MIT License](LICENSE)。项目包含修改后的 russh 0.65.1；该组件保留 Apache-2.0 许可和原始声明，详见[第三方组件说明](THIRD_PARTY_NOTICES.md)。
