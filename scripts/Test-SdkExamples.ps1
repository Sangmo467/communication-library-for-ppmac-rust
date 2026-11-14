[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$PackageDirectory,
    [string]$BuildDirectory = '',
    [string]$PythonExecutable = '',
    [string]$DotnetExecutable = '',
    [string[]]$CmakeArguments = @()
)
$ErrorActionPreference = 'Stop'
$package = (Resolve-Path -LiteralPath $PackageDirectory).Path
$manifest = Get-Content -LiteralPath (Join-Path $package 'manifest.json') -Raw -Encoding UTF8 | ConvertFrom-Json
if (-not $BuildDirectory) { $BuildDirectory = Join-Path ([IO.Path]::GetTempPath()) "pmac-sdk-$([guid]::NewGuid().ToString('N'))" }
# 构建目录必须在包外，避免破坏包内清单校验；保留目录便于查看编译输出。
$BuildDirectory = [IO.Path]::GetFullPath($BuildDirectory)
if ($BuildDirectory.StartsWith($package.TrimEnd([char]92,[char]47) + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) -or $BuildDirectory -eq $package) {
    throw 'BuildDirectory must be outside the immutable SDK package'
}
$arguments = @('-S',(Join-Path $package 'examples'),'-B',$BuildDirectory,'-DCMAKE_BUILD_TYPE=Release')
if ($IsWindows) {
    $arch = if ($manifest.target -like 'i686-*') { 'Win32' } elseif ($manifest.target -like 'aarch64-*') { 'ARM64' } else { 'x64' }
    $arguments += @('-G','Visual Studio 17 2022','-A',$arch)
} elseif ($manifest.target -like 'i686-*') {
    $arguments += @('-DCMAKE_C_FLAGS=-m32','-DCMAKE_CXX_FLAGS=-m32')
}
cmake @arguments @CmakeArguments
if ($LASTEXITCODE -ne 0) { throw 'SDK example configuration failed' }
cmake --build $BuildDirectory --config Release --parallel 2
if ($LASTEXITCODE -ne 0) { throw 'SDK example compilation failed' }
ctest --test-dir $BuildDirectory -C Release --output-on-failure
if ($LASTEXITCODE -ne 0) { throw 'SDK C/C++ offline tests failed' }
$library = Join-Path $package $(if ($manifest.target -like '*windows*') { 'native/pmac.dll' } else { 'native/libpmac.so' })
$oldLibrary = $env:PMAC_LIBRARY
$oldPythonPath = $env:PYTHONPATH
try {
    $env:PMAC_LIBRARY = $library
    $env:PYTHONPATH = Join-Path $package 'bindings/python'
    if ($PythonExecutable) {
        # 解释器必须与库同位数；不使用当前机器默认 Python 猜测目标位数。
        & $PythonExecutable -B (Join-Path $package 'examples/python/sdk_smoke.py')
        if ($LASTEXITCODE -ne 0) { throw 'SDK Python lifecycle smoke failed' }
        & $PythonExecutable -B (Join-Path $package 'examples/python/getresponse.py') --native-version
        if ($LASTEXITCODE -ne 0) { throw 'SDK Python GetResponse offline check failed' }
    }
    if ($DotnetExecutable) {
        $project = Join-Path $package 'examples/dotnet/getresponse/Pmac.GetResponse.csproj'
        # .NET 8 的 --artifacts-path 把 obj/bin 留在包外。x86 使用匹配的 runtime 执行 IL。
        dotnet build $project -c Release --artifacts-path (Join-Path $BuildDirectory 'dotnet') --nologo
        if ($LASTEXITCODE -ne 0) { throw 'SDK .NET example build failed' }
        $assembly = Join-Path $BuildDirectory 'dotnet/bin/Pmac.GetResponse/release/Pmac.GetResponse.dll'
        & $DotnetExecutable $assembly --check
        if ($LASTEXITCODE -ne 0) { throw 'SDK .NET offline check failed' }
    }
} finally {
    $env:PMAC_LIBRARY = $oldLibrary
    $env:PYTHONPATH = $oldPythonPath
}
Write-Host "SDK examples passed: $($manifest.target)"
