[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Target,
    [string]$OutputDirectory = 'dist',
    [string]$PythonExecutable = '',
    [string]$DotnetExecutable = '',
    [switch]$AllowUncommitted,
    [switch]$SkipRustTests
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$targetProfile = & (Join-Path $PSScriptRoot 'Get-ReleaseTarget.ps1') -Target $Target
$hostSystem = if ($IsWindows) { 'windows' } elseif ($IsMacOS) { 'macos' } elseif ($IsLinux) { 'linux' } else { 'unsupported' }
if ($hostSystem -ne $targetProfile.system) { throw "Run $Target on a matching operating system; host is $hostSystem" }
Push-Location $root
try {
    rustup target add $Target
    if ($LASTEXITCODE -ne 0) { throw 'Rust target installation failed' }
    $sdkCmakeArguments = @()
    if ($targetProfile.qemu) {
        # ARMv7 hard-float 在 x64 runner 交叉编译，测试通过 QEMU 执行，不把交叉编译成功当作运行通过。
        $env:CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABIHF_LINKER = 'arm-linux-gnueabihf-gcc'
        $env:CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABIHF_RUNNER = 'qemu-arm -L /usr/arm-linux-gnueabihf'
        $env:CC_armv7_unknown_linux_gnueabihf = 'arm-linux-gnueabihf-gcc'
        $sdkCmakeArguments = @('-DCMAKE_SYSTEM_NAME=Linux','-DCMAKE_SYSTEM_PROCESSOR=arm',
            '-DCMAKE_C_COMPILER=arm-linux-gnueabihf-gcc','-DCMAKE_CXX_COMPILER=arm-linux-gnueabihf-g++',
            '-DCMAKE_CROSSCOMPILING_EMULATOR=qemu-arm;-L;/usr/arm-linux-gnueabihf')
    }
    $runnerExecutable = 'qemu-arm'
    $runnerSysroot = '/usr/arm-linux-gnueabihf'
    if ($Target -eq 'loongarch64-unknown-linux-gnu') {
        if (-not $env:PMAC_LOONGARCH_SYSROOT -or -not $env:PMAC_LOONGARCH_LINKER) { throw 'Use Build-LoongArchSdk.ps1 to prepare the validated LoongArch toolchain/sysroot' }
        $runnerExecutable = 'qemu-loongarch64'
        $runnerSysroot = $env:PMAC_LOONGARCH_SYSROOT
        $env:CARGO_TARGET_LOONGARCH64_UNKNOWN_LINUX_GNU_LINKER = $env:PMAC_LOONGARCH_LINKER
        $env:CARGO_TARGET_LOONGARCH64_UNKNOWN_LINUX_GNU_RUNNER = "$runnerExecutable -L $runnerSysroot"
        $env:CC_loongarch64_unknown_linux_gnu = $env:PMAC_LOONGARCH_LINKER
        $env:CFLAGS_loongarch64_unknown_linux_gnu = '-std=gnu11'
        # SDK C++ 例程使用 Debian 交叉工具链及匹配的 libstdc++；库本身按 glibc 2.36 构建。
        $sdkCmakeArguments = @('-DCMAKE_SYSTEM_NAME=Linux','-DCMAKE_SYSTEM_PROCESSOR=loongarch64',
            '-DCMAKE_C_COMPILER=loongarch64-linux-gnu-gcc-15','-DCMAKE_CXX_COMPILER=loongarch64-linux-gnu-g++-15',
            '-DCMAKE_CROSSCOMPILING_EMULATOR=qemu-loongarch64;-L;/usr/loongarch64-linux-gnu')
    }
    if ($IsMacOS) { $env:MACOSX_DEPLOYMENT_TARGET = $targetProfile.deployment }
    if (-not $SkipRustTests) {
        $arguments = @('test','--workspace','--locked','--target',$Target)
        if ($targetProfile.qemu -or $targetProfile.emulated) { $arguments += @('--','--test-threads=1') }
        cargo @arguments
        if ($LASTEXITCODE -ne 0) { throw 'Target Rust tests failed' }
    }
    cargo build -p pmac-ffi --release --locked --target $Target
    if ($LASTEXITCODE -ne 0) { throw 'Target release build failed' }
    if ($IsMacOS) {
        $library = "target/$Target/release/libpmac.dylib"
        # 可移动的 install name；修改 Mach-O 后重新做 ad-hoc 签名，避免 Apple Silicon 拒载。
        install_name_tool -id '@rpath/libpmac.dylib' $library
        if ($LASTEXITCODE -ne 0) { throw 'Cannot set relocatable macOS install name' }
        codesign --force --sign - $library
        if ($LASTEXITCODE -ne 0) { throw 'macOS ad-hoc library signing failed' }
        codesign --verify $library
        if ($LASTEXITCODE -ne 0) { throw 'macOS library signature verification failed' }
    }
    $evidence = "build/release-$Target"
    New-Item -ItemType Directory -Path $evidence -Force | Out-Null
    if ($Target -eq 'loongarch64-unknown-linux-gnu') {
        $compatibility = & (Join-Path $PSScriptRoot 'Test-LoongArchCompatibility.ps1') -Library "target/$Target/release/libpmac.so"
        $compatibility | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $evidence 'loongarch-compatibility.json') -Encoding utf8NoBOM
        $compatibility | Format-List | Out-Host
    }
    $sbom = Join-Path $evidence 'sbom.spdx.json'
    & (Join-Path $PSScriptRoot 'Write-Sbom.ps1') -Target $Target -OutputPath $sbom | Out-Host
    $arguments = @{ Target=$Target; OutputDirectory=$OutputDirectory; SbomPath=$sbom; EvidenceDirectory=$evidence; AllowUncommitted=$AllowUncommitted }
    $package = & (Join-Path $PSScriptRoot 'Package-Release.ps1') @arguments
    & (Join-Path $PSScriptRoot 'Test-Release.ps1') -PackageDirectory $package | Out-Host
    $sdkBuild = Join-Path $root "build/sdk-$Target"
    & (Join-Path $PSScriptRoot 'Test-SdkExamples.ps1') -PackageDirectory $package -BuildDirectory $sdkBuild -PythonExecutable $PythonExecutable -DotnetExecutable $DotnetExecutable -CmakeArguments $sdkCmakeArguments
    # 在各目标实际执行 C 和 Rust 探针，逐字段核对大小/对齐/偏移。
    $cProbe = Join-Path $sdkBuild $(if ($IsWindows) { 'Release/sdk_abi.exe' } else { 'sdk_abi' })
    $cLayout = Join-Path $evidence 'abi-c.txt'
    $rustLayout = Join-Path $evidence 'abi-rust.txt'
    if ($targetProfile.qemu -or $targetProfile.emulated) { & $runnerExecutable -L $runnerSysroot $cProbe | Set-Content -LiteralPath $cLayout -Encoding utf8NoBOM }
    else { & $cProbe | Set-Content -LiteralPath $cLayout -Encoding utf8NoBOM }
    if ($LASTEXITCODE -ne 0) { throw 'C ABI probe execution failed' }
    cargo run -p pmac-ffi --example abi_layout --release --locked --target $Target | Set-Content -LiteralPath $rustLayout -Encoding utf8NoBOM
    if ($LASTEXITCODE -ne 0) { throw 'Rust ABI probe execution failed' }
    & (Join-Path $PSScriptRoot 'Compare-Abi.ps1') -CLayoutPath $cLayout -RustLayoutPath $rustLayout
    $archive = "$package.zip"
    if (Test-Path -LiteralPath $archive) { throw 'Refusing to overwrite an existing SDK archive' }
    Compress-Archive -LiteralPath $package -DestinationPath $archive -CompressionLevel Optimal
    $hash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
    "$hash  $([IO.Path]::GetFileName($archive))" | Set-Content -LiteralPath "$archive.sha256" -Encoding utf8NoBOM
    Write-Output $archive
} finally { Pop-Location }
