# 构建与安装手册

## 1. 环境要求

| 组件 | 要求 |
|---|---|
| Rust | 1.99.1；仓库通过 `rust-toolchain.toml` 固定，包含 rustfmt 和 Clippy |
| 原生链接环境 | Windows 使用 MSVC 对应架构工具链；Linux 使用 GCC/Clang 对应架构工具链 |
| CMake | 可选，>=3.20；用于构建、安装 C/C++ 头文件及原生库 |
| C/C++ 调用方 | C11、C++17 或 C++98 |
| .NET 调用方 | .NET 8 |
| Python 调用方 | Python >=3.9，仅依赖标准库 ctypes/asyncio |

公开仓库不包含预编译库。使用方需要针对自己的操作系统和 CPU 架构构建。

## 2. 构建原生库

在仓库根目录执行：

```sh
cargo build -p pmac-ffi --release --locked
```

`--locked` 确保使用随仓库提供的 `Cargo.lock`。russh 的本地补丁位于 `vendor/russh`，正常构建会自动使用，无需额外配置。

默认产物：

| 平台 | 动态库 | 其他产物 |
|---|---|---|
| Windows | `target/release/pmac.dll` | `pmac.dll.lib` 导入库、`pmac.lib` 静态库 |
| Linux | `target/release/libpmac.so` | `libpmac.a` 静态库 |

显式指定 target 后，产物改在 `target/<triple>/release/`。例如：

```sh
rustup target add x86_64-unknown-linux-gnu
cargo build -p pmac-ffi --release --locked --target x86_64-unknown-linux-gnu
```

交叉构建还需要对应的链接器、系统库和目标平台运行环境。构建成功不等于该操作系统、控制器固件或工业网络场景已经验收。

## 3. 使用 CMake 构建和安装

单配置生成器（常见于 Linux）：

```sh
cmake -S . -B build/native -DCMAKE_BUILD_TYPE=Release
cmake --build build/native
cmake --install build/native --prefix /your/install/prefix
```

Windows 多配置生成器：

```powershell
cmake -S . -B build/native -A x64
cmake --build build/native --config Release
cmake --install build/native --config Release --prefix C:/pmac
```

可指定独立 Rust target：

```powershell
cmake -S . -B build/arm64 -A ARM64 -DPMAC_RUST_TARGET=aarch64-pc-windows-msvc
cmake --build build/arm64 --config Release
```

默认安装内容：

- `include/pmac.h`；
- `include/pmac/*.hpp`；
- Windows 的 `bin/pmac.dll` 与 `lib/pmac.dll.lib`；
- Linux 的 `lib/libpmac.so`。

不会安装 Python/.NET 项目；它们按下一节直接集成源码。

## 4. 各语言集成

### C 和 C++

包含公开头：

```c
#include <pmac.h>
```

```cpp
#include <pmac/pmac.hpp>       // C++17
#include <pmac/pmac_cpp98.hpp> // C++98 同步适配
```

Windows 链接 `pmac.dll.lib`，运行时让 `pmac.dll` 位于可执行文件目录或系统 DLL 搜索路径。Linux 链接 `libpmac.so`，运行时通过安装路径、rpath 或系统动态加载器配置找到它。

不要混用不同位宽或不同 ABI major 的头文件和库。可通过 `pmac_get_runtime_info` 检查 ABI 与指针位宽。

### .NET 8

将 `bindings/dotnet/Pmac/Pmac.csproj` 作为项目引用，或把该目录源码纳入解决方案：

```xml
<ItemGroup>
  <ProjectReference Include="path/to/bindings/dotnet/Pmac/Pmac.csproj" />
</ItemGroup>
```

把匹配进程架构的 `pmac.dll` 或 `libpmac.so` 放到应用原生库搜索路径。ASP.NET、Windows 服务或 Linux 容器需要显式配置对应部署目录，不能只依赖开发机上的构建目录。

.NET 8 官方不支持 Linux x86；其他 ARM 平台需使用与进程架构一致的原生库。

### Python

安装本地包：

```sh
python -m pip install ./bindings/python
```

运行时显式传入原生库路径最可靠：

```python
import pmac

runtime = pmac.Runtime(library_path="/absolute/path/to/libpmac.so")
```

Windows 可传入 `C:/path/to/pmac.dll`。不传 `library_path` 时，绑定按名称 `pmac.dll` 或 `libpmac.so` 使用系统动态库搜索规则。Python 进程与原生库必须位宽一致。

## 5. 发布到应用程序

应用包至少需要：

1. 与目标架构匹配的原生动态库；
2. 对应语言绑定或编译后的应用；
3. 根目录 `LICENSE` 和 `THIRD_PARTY_NOTICES.md`；
4. 如直接再分发 russh 源码，还需保留 `vendor/russh/LICENSE` 和源码中的版权说明。

不要把密码、私钥、known_hosts 中的现场地址、控制器导出数据或本地构建目录打入应用包。凭据由部署环境或应用安全配置提供。
