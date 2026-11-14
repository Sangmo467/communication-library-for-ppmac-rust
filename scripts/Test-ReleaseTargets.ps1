[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$work = Join-Path $root ('build/native-header-tests-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $work -Force | Out-Null
$profiles = @(& (Join-Path $PSScriptRoot 'Get-ReleaseTarget.ps1'))
if ($profiles.Count -ne 10) { throw 'Release matrix must include ten targets' }
$legacy = @(& (Join-Path $PSScriptRoot 'Get-ReleaseTarget.ps1') -Version v2.0.0)
if ($legacy.Count -ne 4) { throw 'Legacy release target compatibility regressed' }
$previous = @(& (Join-Path $PSScriptRoot 'Get-ReleaseTarget.ps1') -Version v2.0.1)
if ($previous.Count -ne 9) { throw 'Version 2.0.1 target compatibility regressed' }
foreach ($item in $profiles) {
    $bytes = [byte[]]::new(256)
    if ($item.format -eq 'PE') {
        $bytes[0]=0x4d; $bytes[1]=0x5a
        [BitConverter]::GetBytes([uint32]64).CopyTo($bytes,0x3c)
        [BitConverter]::GetBytes([uint32]0x4550).CopyTo($bytes,64)
        [BitConverter]::GetBytes([uint16]$item.machine).CopyTo($bytes,68)
        [BitConverter]::GetBytes([uint16]0x2000).CopyTo($bytes,86)
        [BitConverter]::GetBytes([uint16]$(if($item.bits -eq 64){0x20b}else{0x10b})).CopyTo($bytes,88)
    } elseif ($item.format -eq 'ELF') {
        $bytes[0]=0x7f; $bytes[1]=0x45; $bytes[2]=0x4c; $bytes[3]=0x46
        $bytes[4]=if($item.bits -eq 64){2}else{1}; $bytes[5]=1
        [BitConverter]::GetBytes([uint16]3).CopyTo($bytes,16)
        [BitConverter]::GetBytes([uint16]$item.machine).CopyTo($bytes,18)
        if ($item.target -eq 'loongarch64-unknown-linux-gnu') { [BitConverter]::GetBytes([uint32]0x43).CopyTo($bytes,48) }
        if ($item.target -like 'armv7-*') { [BitConverter]::GetBytes([uint32]0x400).CopyTo($bytes,36) }
    } else {
        $bytes[0]=0xcf; $bytes[1]=0xfa; $bytes[2]=0xed; $bytes[3]=0xfe
        [BitConverter]::GetBytes([uint32]$item.machine).CopyTo($bytes,4)
        [BitConverter]::GetBytes([uint32]6).CopyTo($bytes,12)
    }
    $path = Join-Path $work $item.target
    [IO.File]::WriteAllBytes($path,$bytes)
    $identity = & (Join-Path $PSScriptRoot 'Test-NativeTarget.ps1') -Library $path -Target $item.target
    if ($identity.pointerBits -ne $item.bits -or $identity.format -ne $item.format) { throw 'Header identity mismatch' }
    if ($item.target -eq 'loongarch64-unknown-linux-gnu') {
        # 相同 machine/位数不能证明 ABI 一致：拒绝旧世界、软浮点、单精度 ABI。
        foreach ($badFlags in @(0x03,0x41,0x42)) {
            [BitConverter]::GetBytes([uint32]$badFlags).CopyTo($bytes,48)
            [IO.File]::WriteAllBytes($path,$bytes)
            $rejected = $false
            try { & (Join-Path $PSScriptRoot 'Test-NativeTarget.ps1') -Library $path -Target $item.target | Out-Null }
            catch { $rejected = $true }
            if (-not $rejected) { throw 'Incompatible LoongArch ELF ABI was accepted' }
        }
        [BitConverter]::GetBytes([uint32]0x43).CopyTo($bytes,48)
        [IO.File]::WriteAllBytes($path,$bytes)
    }
    # 格式和位数相同也不能混用 CPU；验证错误目标必定被拒绝。
    $other = @($profiles | Where-Object { $_.format -eq $item.format -and $_.target -ne $item.target })[0]
    $rejected = $false
    try { & (Join-Path $PSScriptRoot 'Test-NativeTarget.ps1') -Library $path -Target $other.target | Out-Null }
    catch { $rejected = $true }
    if (-not $rejected) { throw 'Wrong-CPU artifact was accepted' }
}
# 拒绝截断的 Mach-O 与 fat/universal 容器，不能把它们误记为薄 dylib。
foreach ($bad in @([byte[]](0xcf,0xfa,0xed,0xfe),[byte[]](0xca,0xfe,0xba,0xbe))) {
    $path = Join-Path $work ([guid]::NewGuid().ToString('N'))
    [IO.File]::WriteAllBytes($path,$bad)
    $rejected=$false
    try { & (Join-Path $PSScriptRoot 'Test-NativeTarget.ps1') -Library $path -Target aarch64-apple-darwin | Out-Null }
    catch { $rejected=$true }
    if (-not $rejected) { throw 'Invalid Mach-O header accepted' }
}
Write-Host 'Ten target headers, wrong-CPU rejection, Mach-O truncation/fat rejection and legacy target coverage passed (synthetic headers only).'
