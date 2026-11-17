# Prebuilt SDKs: Windows/Linux/macOS on x86, x64, ARM and LoongArch64

**English** | [简体中文](https://github.com/Sangmo467/communication-library-for-ppmac-rust/blob/main/docs/prebuilt-sdk.md)

Download stable SDKs from [GitHub Releases](https://github.com/Sangmo467/communication-library-for-ppmac-rust/releases). A build workflow or source tag alone does not mean that binary assets have been published.

## Choose the matching package

| Operating system | Process architecture | Target | Native library |
| --- | --- | --- | --- |
| Windows | x86, 32-bit | i686-pc-windows-msvc | native/pmac.dll and pmac.dll.lib |
| Windows | x64, 64-bit | x86_64-pc-windows-msvc | native/pmac.dll and pmac.dll.lib |
| Linux | x86, 32-bit | i686-unknown-linux-gnu | native/libpmac.so |
| Linux | x64, 64-bit | x86_64-unknown-linux-gnu | native/libpmac.so |
| Windows | ARM64 | aarch64-pc-windows-msvc | native/pmac.dll and pmac.dll.lib |
| Linux | ARM64 | aarch64-unknown-linux-gnu | native/libpmac.so |
| Linux | ARMv7 hard-float, 32-bit | armv7-unknown-linux-gnueabihf | native/libpmac.so |
| macOS | Intel x64 | x86_64-apple-darwin | native/libpmac.dylib |
| macOS | Apple Silicon ARM64 | aarch64-apple-darwin | native/libpmac.dylib |
| Linux | LoongArch64 LP64D | loongarch64-unknown-linux-gnu | native/libpmac.so |

Archive names follow pmac-2.0.10-<target>-release.zip, with a matching .zip.sha256 file. Choose by **process architecture**, not only by the operating system: a 32-bit application on 64-bit Windows still needs the x86 SDK. Python and .NET processes must match the library architecture too.

Windows packages use the MSVC ABI. Linux x86/x64/ARM packages are built on Ubuntu 22.04; LoongArch uses the separately documented glibc 2.36 baseline. These are GNU/glibc packages, not musl/Alpine packages. Verify the process ABI and required system-library versions before deployment. Version 2.0.0 has its original four targets, version 2.0.1 has nine, and version 2.0.2 adds LoongArch64 as the tenth target.

The SDK contains native/, include/, bindings/, cmake/PmacConfig.cmake, examples/, licenses, an SPDX SBOM and manifest.json with individual file hashes. Rust is not required on a consumer's machine.

## Build the examples without rebuilding the library

Run these commands from the extracted SDK root. Keep generated output outside that root so that the release manifest remains valid. CMake 3.20 or later and a C/C++ toolchain are required.

Windows, with Visual Studio 2022 Desktop development with C++ and a Windows SDK:

```powershell
# Use -A Win32 for the x86 SDK.
cmake -S examples -B ../pmac-example-build -G "Visual Studio 17 2022" -A x64
cmake --build ../pmac-example-build --config Release
ctest --test-dir ../pmac-example-build -C Release --output-on-failure
```

Linux, with GCC/G++:

```sh
cmake -S examples -B ../pmac-example-build -DCMAKE_BUILD_TYPE=Release
cmake --build ../pmac-example-build --parallel 2
ctest --test-dir ../pmac-example-build --output-on-failure
```

To build against the x86 Linux SDK on an x64 host, install gcc-multilib/g++-multilib and add -DCMAKE_C_FLAGS=-m32 -DCMAKE_CXX_FLAGS=-m32 to configuration.

Six offline tests verify C, C++17 and C++98 ABI/lifecycle integration and GetResponse --check modes. They do not connect to a controller or send motion commands. The Gather/geterrors executables are built too, but require a real controller and are not run by offline CTest. The examples contain detailed Chinese source comments; this guide explains their setup and behavior in English.

The Windows example build copies the DLL next to each executable. Linux build-tree executables use the imported library path. For your own CMake project:

```cmake
find_package(Pmac CONFIG REQUIRED PATHS "/path/to/SDK/cmake" NO_DEFAULT_PATH)
target_link_libraries(my_app PRIVATE Pmac::pmac)
```

Deploy pmac.dll with your Windows executable. On Linux, deploy libpmac.so using an explicit loader path or an appropriate RUNPATH; successfully linking an application does not automatically deploy its runtime dependencies.

PowerShell 7 provides a combined validation entry point:

```powershell
pwsh -File scripts/Test-SdkExamples.ps1 -PackageDirectory .
pwsh -File scripts/Test-SdkExamples.ps1 -PackageDirectory . -PythonExecutable python -DotnetExecutable dotnet
```

The second command additionally requires a matching Python process, a .NET 8 SDK for building and a matching .NET runtime for execution. Linux x86 has no supported .NET 8 x86 runtime, so that target is tested with Rust/C/C++; you can test Python separately if a 32-bit interpreter is available. Windows x86 uses an x86 Python interpreter and an x86 dotnet.exe to execute the managed assembly, even if an x64 SDK compiled it.

## Python and C# offline checks

```sh
python examples/python/sdk_smoke.py
dotnet run --project examples/dotnet/getresponse/Pmac.GetResponse.csproj -- --check
```

The Python smoke example resolves bindings and the native library relative to the extracted SDK. For the GetResponse Python example, install ./bindings/python or add that directory to PYTHONPATH. The C# project references the included binding and copies the matching native library to its output directory. PMAC_LIBRARY can override the C# library location.

For the Windows x86 C# example, build with the available .NET SDK, then run Pmac.GetResponse.dll using the x86 dotnet.exe rather than an x64 dotnet run process. MATLAB MEX examples require a separately installed compatible MATLAB and compiler; they are not part of automatic offline tests.

## Read one response from a controller

Complete the offline checks first. Configure PMAC_HOST, PMAC_USER (default: root), PMAC_KNOWN_HOSTS (absolute known_hosts path) and PMAC_PASSWORD outside the source code. C/C++ examples also accept PMAC_PRIVATE_KEY and PMAC_PASSPHRASE. The Python/C# examples shown here use password authentication. Do not commit or log credentials.

Windows PowerShell:

```powershell
$env:PMAC_HOST = 'controller-address'
$env:PMAC_USER = 'root'
$env:PMAC_KNOWN_HOSTS = 'C:/secure/known_hosts'
# Set PMAC_PASSWORD securely in the local process before continuing.
../pmac-example-build/Release/getresponse_c.exe Sys.ServoTime
../pmac-example-build/Release/getresponse_cpp.exe Sys.ServoTime
$env:PMAC_LIBRARY = (Resolve-Path native/pmac.dll).Path
$env:PYTHONPATH = (Resolve-Path bindings/python).Path
python examples/python/getresponse.py --command Sys.ServoTime
dotnet run --project examples/dotnet/getresponse/Pmac.GetResponse.csproj -- Sys.ServoTime
```

On Linux, use export NAME=value, native/libpmac.so and executable paths without Release/ or .exe. Set PMAC_LIBRARY to an absolute path for the Python GetResponse example.

The default command is Sys.ServoTime. A caller-supplied command can change controller state. A timeout does not prove that the command was not executed: do not automatically replay operations with an unknown result. The C example demonstrates submit/wait, required-length queries, byte copying and reverse-order resource cleanup. C++, Python and C# demonstrate wrapper ownership and original device diagnostics.

Callback examples use library threads, not a UI thread. Copy borrowed data before handing it to another thread. Request stop from a callback, and perform blocking stop/release outside that callback. Gather configuration changes global controller sampling state and requires a reverse TCP route to the configured local port.

## Build and publish a new release

Public source is maintained at [Sangmo467/communication-library-for-ppmac-rust](https://github.com/Sangmo467/communication-library-for-ppmac-rust). Keep internal development history, review records, credentials and site data out of public snapshots.

On each target OS, a PowerShell 7 runner with Rust 1.100.0, CMake and the relevant C/C++ toolchain can execute:

```powershell
./scripts/Build-Release.ps1 -Target x86_64-pc-windows-msvc -PythonExecutable python -DotnetExecutable dotnet
./scripts/Build-Release.ps1 -Target i686-pc-windows-msvc
# Run the next two on a Linux runner.
./scripts/Build-Release.ps1 -Target x86_64-unknown-linux-gnu -PythonExecutable python3 -DotnetExecutable dotnet
./scripts/Build-Release.ps1 -Target i686-unknown-linux-gnu
```

Windows builds need NASM in addition to MSVC. Linux x86 builds need multilib. The script runs target Rust tests, builds the release library, validates the native architecture, generates and checks the SBOM/manifest, builds SDK examples and produces a ZIP plus SHA-256 sidecar. Existing outputs are not overwritten. -AllowUncommitted is for development candidates only; publication rejects dirty source.

The GitHub workflow .github/workflows/release.yml builds ten targets after main/master changes. Actions artifacts are temporary build outputs retained for 30 days. For a stable release, ensure that the Cargo version and tag agree, such as version 2.0.10 and tag v2.0.10, then push the public tag. Only after all required builds and their SDK tests succeed will the publishing job create the GitHub Release and attach ten ZIPs plus ten SHA-256 files.

The publishing job uses GitHub's automatic short-lived token with contents:write; no Gitee token or additional personal token is required. A manual workflow run builds only by default. Select a version tag and enable publish_release to publish manually.

For manual validation/publication, collect all required archives and sidecars in one directory:

```powershell
./scripts/Publish-GitHubRelease.ps1 -AssetDirectory ./release-assets -Tag v2.0.10 -Commit <40-character-public-commit> -ValidateOnly
# Configure GH_TOKEN securely and install the official GitHub CLI before publishing.
./scripts/Publish-GitHubRelease.ps1 -AssetDirectory ./release-assets -Tag v2.0.10 -Commit <40-character-public-commit>
```

Validation checks all ten targets, versions, clean-source flags, identical commits/source snapshots and archive/member hashes. The remote tag must match the manifests. Existing releases are not overwritten; inspect any partial upload before retrying. Do not move a published version tag or silently replace assets already downloaded by users.

If compilation succeeded but publishing failed, manually run **Publish validated SDK artifacts** in Actions. Supply the original build run ID and existing version tag. This workflow downloads the original required packages, verifies their commit against the remote tag and retries publication without recompiling or moving the tag.

## ARM and macOS (version 2.0.1 and later)

scripts/release-targets.json is the single target registry. Linux ARM64 runs on a native ARM64 runner. ARMv7 uses a hard-float cross compiler; Rust and C/C++ tests execute through QEMU with serialized Rust test scheduling. Windows ARM64 uses a native runner, ARM64 Python/.NET and the installed Visual Studio ARM64 compiler. Test-SdkExamples.ps1 detects the Visual Studio generator; VS 2026 requires CMake 4.2 or later.

macOS packages contain separate thin Intel x64 and Apple Silicon ARM64 Mach-O dylibs, not a Universal Binary. The application, interpreter/runtime and library architectures must match. The library deployment target is macOS 11.0; your Python/.NET/MATLAB version can impose a newer OS requirement. Libraries use @rpath/libpmac.dylib and an ad-hoc signature. They are not Developer ID signed or notarized; no Apple distribution certification is claimed.

Install Xcode Command Line Tools and CMake, then run from the extracted SDK:

```sh
cmake -S examples -B ../pmac-macos-build -DCMAKE_BUILD_TYPE=Release
cmake --build ../pmac-macos-build --parallel 2
ctest --test-dir ../pmac-macos-build --output-on-failure
python examples/python/sdk_smoke.py
dotnet run --project examples/dotnet/getresponse/Pmac.GetResponse.csproj -- --check
```

All public guides in docs/ have English .en.md counterparts. The macOS library is native/libpmac.dylib; Python, C# and CMake examples select it automatically. Configure an appropriate runtime search path for @rpath when deploying your own application. The MATLAB MEX build entry accepts maci64/maca64 environments, but must be verified separately with a supported, installed MATLAB/compiler combination; MATLAB is not covered by the CI runtime checks.

## LoongArch64 (version 2.0.2 and later)

LoongArch packages target LP64D new-world ABI and a glibc 2.36 baseline. They are cross-built in an isolated Debian toolchain container, with Rust/C ABI tests executed through QEMU against the baseline libc. C++ examples use the toolchain matching runtime. Kylin/Debian compatibility depends on the actual installed ABI, CPU/kernel and libc, not the distribution name alone. See the [LoongArch64 guide](loongarch-sdk.en.md). No target hardware certification is claimed.
