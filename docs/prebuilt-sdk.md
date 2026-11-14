# Windows / Linux / macOS 多架构预编译 SDK 与中文调用例程

**简体中文** | [English](prebuilt-sdk.en.md)

下载入口为 [GitHub Releases](https://github.com/Sangmo467/communication-library-for-ppmac-rust/releases)。正式版本需要在公开源码 tag 上完成构建与附件上传后才可下载；仓库中的脚本本身不代表已经发布二进制。

## 选择下载包

| 系统 | 进程架构 | Rust target | 库 |
| --- | --- | --- | --- |
| Windows | x86（32 位） | i686-pc-windows-msvc | native/pmac.dll + pmac.dll.lib |
| Windows | x64（64 位） | x86_64-pc-windows-msvc | native/pmac.dll + pmac.dll.lib |
| Linux | x86（32 位） | i686-unknown-linux-gnu | native/libpmac.so |
| Linux | x64（64 位） | x86_64-unknown-linux-gnu | native/libpmac.so |
| Windows | ARM64 | aarch64-pc-windows-msvc | native/pmac.dll + pmac.dll.lib |
| Linux | ARM64 | aarch64-unknown-linux-gnu | native/libpmac.so |
| Linux | ARMv7 hard-float（32 位） | armv7-unknown-linux-gnueabihf | native/libpmac.so |
| macOS | Intel x64 | x86_64-apple-darwin | native/libpmac.dylib |
| macOS | Apple Silicon ARM64 | aarch64-apple-darwin | native/libpmac.dylib |
| Linux | LoongArch64 LP64D（新世界 ABI） | loongarch64-unknown-linux-gnu | native/libpmac.so |

附件命名：`pmac-2.0.2-<target>-release.zip`，每个 ZIP 配套 `.zip.sha256`。32 位程序选择 x86 包，即使操作系统是 64 位。DLL 位数必须与 C/C++、Python、.NET 进程一致。Windows 使用 MSVC；Linux 包为 GNU/glibc，不适用于 musl/Alpine。构建于较新 Linux 发行版的包不能承诺在更旧 glibc 上运行；如需旧工业 Linux，必须在目标兼容环境构建和验收。

包内包括：`native/`、`include/pmac.h`、`bindings/`、`cmake/PmacConfig.cmake`、`examples/`、`scripts/Test-SdkExamples.ps1`、许可证、依赖 SBOM 和带逐文件 SHA-256 的 `manifest.json`。使用者无需安装 Rust。

## 解压后编译和离线检查

下面命令在 SDK 根目录执行，构建目录放在包外。示例含中文注释；离线测试不需要控制器、密码或 PMAC_HOST，也不会发送运动命令。

Windows（VS 2022 C++ 桌面开发组件及 Windows SDK、CMake 3.20+）：

```powershell
# x64 使用 -A x64；x86 使用 -A Win32。
cmake -S examples -B ../pmac-example-build -G "Visual Studio 17 2022" -A x64
cmake --build ../pmac-example-build --config Release
ctest --test-dir ../pmac-example-build -C Release --output-on-failure
```

Linux（GCC/G++ 与 CMake 3.20+）：

```sh
cmake -S examples -B ../pmac-example-build -DCMAKE_BUILD_TYPE=Release
# x86 包在 x64 主机编译：安装 gcc-multilib、g++-multilib，并在上条命令增加：
# -DCMAKE_C_FLAGS=-m32 -DCMAKE_CXX_FLAGS=-m32
cmake --build ../pmac-example-build --parallel 2
ctest --test-dir ../pmac-example-build --output-on-failure
```

六项离线测试包含 C、C++17、C++98 的 ABI/生命周期与独立 C ABI 布局探针，以及 GetResponse 的 `--check`。CMake 自动在 Windows 可执行文件旁复制 DLL；Linux 构建树使用导入库路径加载 so。链接器找不到符号时先核对是否混用了架构或旧版头文件。

自己的 CMake 工程可直接引用：

```cmake
find_package(Pmac CONFIG REQUIRED PATHS "/path/to/SDK/cmake" NO_DEFAULT_PATH)
target_link_libraries(my_app PRIVATE Pmac::pmac)
```

Windows 运行时将 native/pmac.dll 复制到程序旁；Linux 部署时将 libpmac.so 放入应用明确配置的库路径或设置适当 RUNPATH。仅链接 .lib 不会自动部署 DLL。

PowerShell 7 的统一测试入口（会按 manifest 自动选择 x86/x64 编译参数）：

```powershell
pwsh -File scripts/Test-SdkExamples.ps1 -PackageDirectory .
# 同时验证 Python/.NET：使用与库相同架构的解释器与 runtime；dotnet build 还需 .NET 8 SDK。
pwsh -File scripts/Test-SdkExamples.ps1 -PackageDirectory . -PythonExecutable python -DotnetExecutable dotnet
```

Linux x86 不提供 .NET 8 的官方 x86 runtime，因此只验证 C/C++，Python 可用自行安装的 32 位解释器。Windows x86 的 .NET 例程可以用 x64 SDK 编译 IL，再用 x86 dotnet.exe 执行。MATLAB MEX 需另装匹配架构的 MATLAB 与支持的编译器，参见 examples/matlab/build_pmac_mex.m；不属于上述自动离线测试。

## Python / C# 直接调用

```sh
# Python：无第三方运行时依赖，自动按 SDK 相对路径载入绑定和动态库。
python examples/python/sdk_smoke.py
# C#：项目引用包内绑定，构建时将 native/ 下的库复制到输出目录。
dotnet run --project examples/dotnet/getresponse/Pmac.GetResponse.csproj -- --check
```

Python getresponse.py 可通过 `PYTHONPATH=bindings/python` 引用，或先 `pip install ./bindings/python`。C# 的 PMAC_LIBRARY 可覆盖默认库位置；Windows x86 请按上文用 x86 runtime 执行编译后的 DLL。

## 连接设备读取一次

先完成离线检查，再配置环境变量：PMAC_HOST、PMAC_USER（默认 root）、PMAC_KNOWN_HOSTS（known_hosts 文件绝对路径）、PMAC_PASSWORD。C/C++ 还支持 PMAC_PRIVATE_KEY 和 PMAC_PASSPHRASE。Python/C# 此处示例采用密码登录。不要把凭据写进源码、命令参数或版本库。

```powershell
$env:PMAC_HOST = '控制器地址'
$env:PMAC_USER = 'root'
$env:PMAC_KNOWN_HOSTS = 'C:/secure/known_hosts'
# 在本机安全设置 PMAC_PASSWORD 后运行，不在日志里打印它。
../pmac-example-build/Release/getresponse_c.exe Sys.ServoTime
../pmac-example-build/Release/getresponse_cpp.exe Sys.ServoTime
$env:PMAC_LIBRARY = (Resolve-Path native/pmac.dll).Path
$env:PYTHONPATH = (Resolve-Path bindings/python).Path
python examples/python/getresponse.py --command Sys.ServoTime
# C# 默认使用输出目录的库，也可读取上述 PMAC_LIBRARY。
dotnet run --project examples/dotnet/getresponse/Pmac.GetResponse.csproj -- Sys.ServoTime
```

Linux 使用 `export NAME=value`；库路径换为 native/libpmac.so，可执行文件没有 Release 子目录和 .exe 后缀。示例默认命令为 Sys.ServoTime；自定义命令可能改变控制器状态。超时不代表命令没有执行，禁止自动重放未知结果的写操作。C 例程展示 Request 提交、等待、长度查询、响应复制和逆序释放；C++/.NET/Python 展示包装层资源所有权及错误正文处理。

## 维护者：十平台构建和 GitHub 附件发布

仅在公开源码分支的干净提交/tag 上制作正式包。内部开发分支与公开 GitHub 历史独立；不要直接推送内部 master。将适合公开的源码、cmake、examples、scripts 和本文逐文件同步到公开工作区，再提交、打 tag。

各平台 runner 需 Rust 1.100.0、PowerShell 7、CMake；Windows 需 MSVC/NASM，Linux 需 GCC/G++/make，x86 构建需 multilib。该脚本可供任意 Windows/Linux 服务器或 CI 调用：

```powershell
# 在 Windows runner 分别执行（可分别使用 x64/x86 Python 和 .NET runtime）。
./scripts/Build-Release.ps1 -Target x86_64-pc-windows-msvc -PythonExecutable python -DotnetExecutable dotnet
./scripts/Build-Release.ps1 -Target i686-pc-windows-msvc
# 在 Linux runner 分别执行。
./scripts/Build-Release.ps1 -Target x86_64-unknown-linux-gnu -PythonExecutable python3 -DotnetExecutable dotnet
./scripts/Build-Release.ps1 -Target i686-unknown-linux-gnu
```

脚本执行目标架构 Rust 测试、Release 编译、架构验证、SBOM/清单验证和解压包结构下的 C/C++ 示例测试，最后生成 ZIP 与校验文件。`-AllowUncommitted` 仅供本地开发验证；发布脚本拒绝 dirtySource 包。输出目录已有同名包时拒绝覆盖，重建可指定新的 `-OutputDirectory`。

将全部目标平台 ZIP 及其校验文件汇集到同一目录：

```powershell
# 此命令不联网；检查全部目标包版本、公开 commit、源码摘要、逐文件完整性及 SHA-256。
./scripts/Publish-GitHubRelease.ps1 -AssetDirectory ./release-assets -Tag v2.0.2 -Commit <公开tag的40位提交号> -ValidateOnly
# 将 GH_TOKEN 配为 runner 的私密环境变量后执行；不要在聊天、源码或日志中粘贴 Token。
./scripts/Publish-GitHubRelease.ps1 -AssetDirectory ./release-assets -Tag v2.0.2 -Commit <公开tag的40位提交号>
```

发布脚本先确认 GitHub 上同名 tag 的提交与全部目标包清单一致，再创建/选择 Release 并上传附件。不覆盖同名附件；如果上传中断，先检查已上传附件和校验值后再人工处理，不可声称部分上传已发布完整十平台版本。使用 GitHub 官方 CLI，且默认拒绝覆盖已存在的 Release。

## GitHub 自动构建与发布

仓库：[Sangmo467/communication-library-for-ppmac-rust](https://github.com/Sangmo467/communication-library-for-ppmac-rust)。`.github/workflows/release.yml` 在 main/master 更新时自动构建全部目标平台，产生十组独立 ZIP 与校验文件（Actions artifacts 保留 30 天）。

正式发布：公开分支干净提交的 Cargo 版本与 tag 一致，例如 Cargo version=2.0.2 对应 v2.0.2。执行 `git tag v2.0.2`、`git push github v2.0.2` 后，十平台构建与示例测试全部成功才创建 GitHub Release 并上传十个 ZIP 及十个 SHA-256 校验附件。无需设置 Gitee Token 或额外私人 Token；只有发布 job 使用 GitHub 提供的短期 Token 并获得 contents:write。手动运行默认只构建；选择版本 tag 并勾选 publish_release 才会发布。

Linux 使用 Ubuntu 22.04 runner（glibc 2.35 环境），不承诺向更旧 glibc 兼容；Windows 使用 Windows Server 2022/MSVC。Linux x86 测试 Rust/C/C++，不运行不支持该架构的 .NET 8。Windows x86 使用独立 x86 Python 和 .NET runtime。

分支构建的 Actions artifacts 用于评估；正式稳定下载入口是 GitHub Releases。发布脚本拒绝缺包、校验失败、脏源码、不同提交/不同源码摘要的全部目标包组合，且不覆盖已发布的同名 Release。

发布阶段失败、十平台构建已成功时，可在 Actions 手动运行 **Publish validated SDK artifacts**，填写原构建 run ID 和已有版本 tag。该入口只下载原有全部目标包、验证其提交与远端 tag 后重试发布，不重新编译，也不移动标签。

## ARM 与 macOS（2.0.1 起）

目标清单统一保存在 scripts/release-targets.json。Linux ARM64 使用原生 ARM64 runner；ARMv7 使用 hard-float GCC 交叉工具链，Rust 及 C/C++ 测试均实际通过 QEMU 执行，采用单测试线程降低模拟调度的影响。Windows ARM64 使用原生 runner、ARM64 Python/.NET 与 Visual Studio ARM64 编译器。

macOS 提供两个独立的薄 Mach-O dylib 包，不是 Universal Binary。运行进程、Python/.NET/MATLAB 和库的架构必须一致。构建部署下限设为 macOS 11.0；还需满足所用 Python/.NET/MATLAB 自身的系统版本要求。发布库使用 @rpath/libpmac.dylib install name 并带 ad-hoc 签名，未使用 Apple Developer ID 或公证服务；不要把它描述为已完成 Apple 分发认证。

macOS 需 Xcode Command Line Tools 与 CMake，解压包后：

```sh
cmake -S examples -B ../pmac-macos-build -DCMAKE_BUILD_TYPE=Release
cmake --build ../pmac-macos-build --parallel 2
ctest --test-dir ../pmac-macos-build --output-on-failure
python examples/python/sdk_smoke.py
dotnet run --project examples/dotnet/getresponse/Pmac.GetResponse.csproj -- --check
```

本机双语使用文档位于 docs/，目录下每份公开指南均有 .en.md 英文对应文件。动态库名在 macOS 为 native/libpmac.dylib；Python、C# 和 CMake 示例会自动选择。自行部署时需为 @rpath 设置适当的运行时搜索路径。MATLAB MEX 构建入口支持选择 maci64/maca64，但须在已安装、受支持的 MATLAB/编译器环境中单独验证，不属于 CI 验证结论。

Windows 例程验证脚本会检测实际安装的 Visual Studio 生成器；若使用 VS 2026，CMake 须为 4.2 或更高版本。

## LoongArch64（2.0.2 起）

LoongArch 包使用 LP64D 新世界 ABI、glibc 2.36 基线。隔离的 Debian 交叉工具链构建后，Rust/C ABI 测试通过 QEMU 在基线 libc 上运行，C++ 例程使用工具链匹配的运行环境。麒麟/Debian 是否兼容须以实际 ABI、CPU/内核及 libc 为准，不按发行版名称承诺通用兼容。详见 [LoongArch64 指南](loongarch-sdk.md)；不宣称完成目标机实机认证。
