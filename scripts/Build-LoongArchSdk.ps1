[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
if (-not $IsLinux) { throw 'LoongArch cross builds require a Linux Docker host' }
$root = Split-Path $PSScriptRoot -Parent
$rustHome = ((& rustup show home) -join '').Trim()
if ($LASTEXITCODE -ne 0) { throw 'Cannot resolve Rust toolchain directory' }
$cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:HOME '.cargo' }
$pwshItem = Get-Item -LiteralPath (Get-Command pwsh).Source
$resolvedPwsh = $pwshItem.ResolveLinkTarget($true)
$pwshHome = Split-Path $(if ($resolvedPwsh) { $resolvedPwsh.FullName } else { $pwshItem.FullName }) -Parent
docker build --tag pmac-loongarch-build --file (Join-Path $PSScriptRoot 'loongarch.Dockerfile') $root
if ($LASTEXITCODE -ne 0) { throw 'LoongArch build image failed' }
# 复用 runner 的固定 Rust/PowerShell，容器仅提供隔离的交叉工具链和 QEMU。
docker run --rm --volume "${root}:/workspace" --volume "${cargoHome}:/cargo" --volume "${rustHome}:/rustup" --volume "${pwshHome}:/opt/pwsh:ro" pmac-loongarch-build
if ($LASTEXITCODE -ne 0) { throw 'LoongArch SDK build or validation failed' }
