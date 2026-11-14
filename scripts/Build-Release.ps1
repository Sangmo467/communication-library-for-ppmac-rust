[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('i686-pc-windows-msvc','x86_64-pc-windows-msvc','i686-unknown-linux-gnu','x86_64-unknown-linux-gnu')][string]$Target,
    [string]$OutputDirectory = 'dist',
    [string]$PythonExecutable = '',
    [string]$DotnetExecutable = '',
    [switch]$AllowUncommitted,
    [switch]$SkipRustTests
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Push-Location $root
try {
    if (($IsWindows -and $Target -notlike '*windows*') -or ($IsLinux -and $Target -notlike '*linux*')) {
        throw 'Run this script on the target operating system (Windows or Linux)'
    }
    # 构建 runner 需预装 Rust 1.100、CMake、对应 C/C++ 工具链；Linux x86 还需 multilib。
    rustup target add $Target
    if ($LASTEXITCODE -ne 0) { throw 'Rust target installation failed' }
    if (-not $SkipRustTests) {
        cargo test --workspace --locked --target $Target
        if ($LASTEXITCODE -ne 0) { throw 'Target Rust tests failed' }
    }
    cargo build -p pmac-ffi --release --locked --target $Target
    if ($LASTEXITCODE -ne 0) { throw 'Target release build failed' }
    $evidence = "build/release-$Target"
    New-Item -ItemType Directory -Path $evidence -Force | Out-Null
    $sbom = Join-Path $evidence 'sbom.spdx.json'
    & (Join-Path $PSScriptRoot 'Write-Sbom.ps1') -Target $Target -OutputPath $sbom | Out-Host
    $arguments = @{ Target=$Target; OutputDirectory=$OutputDirectory; SbomPath=$sbom; EvidenceDirectory=$evidence; AllowUncommitted=$AllowUncommitted }
    $package = & (Join-Path $PSScriptRoot 'Package-Release.ps1') @arguments
    & (Join-Path $PSScriptRoot 'Test-Release.ps1') -PackageDirectory $package | Out-Host
    & (Join-Path $PSScriptRoot 'Test-SdkExamples.ps1') -PackageDirectory $package -PythonExecutable $PythonExecutable -DotnetExecutable $DotnetExecutable
    # 测试过的包保持不可变；散列覆盖最终 ZIP，远端附件可独立校验。
    $archive = "$package.zip"
    if (Test-Path -LiteralPath $archive) { throw 'Refusing to overwrite an existing SDK archive' }
    Compress-Archive -LiteralPath $package -DestinationPath $archive -CompressionLevel Optimal
    $hash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
    "$hash  $([IO.Path]::GetFileName($archive))" | Set-Content -LiteralPath "$archive.sha256" -Encoding utf8NoBOM
    Write-Output $archive
} finally { Pop-Location }
