# Build and Installation Manual

[中文](build-and-install.md) | [English](build-and-install.en.md)

## 1. Requirements

| Component | Requirement |
|---|---|
| Rust | 1.100.0; pinned through `rust-toolchain.toml`, including rustfmt and Clippy |
| Native linking environment | Architecture-matched MSVC on Windows, GCC/Clang on Linux, and Xcode Command Line Tools on macOS |
| CMake | Optional, >=3.20; builds and installs C/C++ headers and the native library |
| C/C++ callers | C11, C++17, or C++98 |
| .NET callers | .NET 8 |
| Python callers | Python >=3.9; only standard-library ctypes/asyncio dependencies |

The source repository does not store binary libraries. Download ready-to-use SDKs from GitHub Releases, or build for your operating system and CPU architecture. See [prebuilt SDKs](prebuilt-sdk.en.md).

For a clean Windows build, the `aws-lc-sys` assembly implementation requires NASM. If NASM is not installed, explicitly use the prebuilt NASM objects bundled with the locked dependency: set `$env:AWS_LC_SYS_PREBUILT_NASM = '1'` in PowerShell before building. Clippy and tests with Rust 1.100.0 passed using this method. The setting applies only to the current process and does not disable assembly optimization. Toolchain upgrades rebuild dependencies; old build caches may previously have hidden a missing-NASM environment issue.

## 2. Building the native library

Run from the repository root:

```sh
cargo build -p pmac-ffi --release --locked
```

`--locked` ensures use of the repository's `Cargo.lock`. The local russh patch is in `vendor/russh` and is automatically used by normal builds without additional configuration.

Default outputs:

| Platform | Dynamic library | Other outputs |
|---|---|---|
| Windows | `target/release/pmac.dll` | `pmac.dll.lib` import library, `pmac.lib` static library |
| Linux | `target/release/libpmac.so` | `libpmac.a` static library |
| macOS | `target/release/libpmac.dylib` | `libpmac.a` static library |

With an explicit target, outputs move to `target/<triple>/release/`. For example:

```sh
rustup target add x86_64-unknown-linux-gnu
cargo build -p pmac-ffi --release --locked --target x86_64-unknown-linux-gnu
```

Cross-compilation also requires the corresponding linker, system libraries, and target runtime environment. A successful build does not mean acceptance testing has been completed for that operating system, controller firmware, or industrial-network scenario.

## 3. Building and installing with CMake

Single-configuration generators (common on Linux):

```sh
cmake -S . -B build/native -DCMAKE_BUILD_TYPE=Release
cmake --build build/native
cmake --install build/native --prefix /your/install/prefix
```

Windows multi-configuration generators:

```powershell
cmake -S . -B build/native -A x64
cmake --build build/native --config Release
cmake --install build/native --config Release --prefix C:/pmac
```

A separate Rust target may be specified:

```powershell
cmake -S . -B build/arm64 -A ARM64 -DPMAC_RUST_TARGET=aarch64-pc-windows-msvc
cmake --build build/arm64 --config Release
```

Default installation contents:

- `include/pmac.h`;
- `include/pmac/*.hpp`;
- `bin/pmac.dll` and `lib/pmac.dll.lib` on Windows;
- `lib/libpmac.so` on Linux.

Python/.NET projects are not installed; integrate their source directly as described below.

## 4. Language integration

### C and C++

Include the public headers:

```c
#include <pmac.h>
```

```cpp
#include <pmac/pmac.hpp>       // C++17
#include <pmac/pmac_cpp98.hpp> // C++98 synchronous adapter
```

On Windows, link `pmac.dll.lib` and place `pmac.dll` beside the executable or on the system DLL search path at runtime. On Linux, link `libpmac.so` and make it discoverable at runtime through its installation path, rpath, or system dynamic-loader configuration.

Do not mix headers and libraries with different bitness or ABI major versions. Use `pmac_get_runtime_info` to check the ABI and pointer bitness.

### .NET 8

Add a project reference to `bindings/dotnet/Pmac/Pmac.csproj`, or include that directory's source in the solution:

```xml
<ItemGroup>
  <ProjectReference Include="path/to/bindings/dotnet/Pmac/Pmac.csproj" />
</ItemGroup>
```

Place the process-architecture-matched `pmac.dll` or `libpmac.so` on the application's native-library search path. ASP.NET, Windows services, and Linux containers require explicit deployment-directory configuration; do not rely solely on the development machine's build directory.

.NET 8 does not officially support Linux x86. Other ARM platforms require a native library matching the process architecture.

### Python

Install the local package:

```sh
python -m pip install ./bindings/python
```

Explicitly providing the native-library path at runtime is the most reliable approach:

```python
import pmac

runtime = pmac.Runtime(library_path="/absolute/path/to/libpmac.so")
```

On Windows, pass `C:/path/to/pmac.dll`. Without `library_path`, the binding uses system dynamic-library search rules for the name `pmac.dll` or `libpmac.so`. The Python process and native library must have matching bitness.

## 5. Shipping with an application

An application package needs at least:

1. A native dynamic library matching the target architecture;
2. The corresponding language binding or compiled application;
3. Root-level `LICENSE` and `THIRD_PARTY_NOTICES.md`;
4. If redistributing russh source directly, `vendor/russh/LICENSE` and the copyright notices in its source must also be retained.

Do not package passwords, private keys, site addresses from known_hosts, controller export data, or local build directories. Provide credentials through the deployment environment or application security configuration.

## macOS and ARM SDKs

See [the multi-platform SDK guide](prebuilt-sdk.en.md) for native Windows/Linux ARM64, ARMv7 hard-float QEMU validation, and Intel/Apple Silicon macOS builds. macOS uses libpmac.dylib and Xcode Command Line Tools; prebuilt SDK consumers do not need Rust.
