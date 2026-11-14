[CmdletBinding()]
param([Parameter(Mandatory)][string]$Library, [Parameter(Mandatory)][string]$Target)
$ErrorActionPreference = 'Stop'
$nativeProfile = & (Join-Path $PSScriptRoot 'Get-ReleaseTarget.ps1') -Target $Target
$stream = [IO.File]::OpenRead((Resolve-Path -LiteralPath $Library).Path)
$reader = [IO.BinaryReader]::new($stream, [Text.Encoding]::UTF8, $true)
try {
    $abiFlags = $null
    $machFileType = $null
    $magic = $reader.ReadBytes(4)
    if ($magic.Length -ne 4) { throw 'Native artifact is truncated' }
    if ($magic[0] -eq 0x4d -and $magic[1] -eq 0x5a) {
        $format = 'PE'
        $stream.Position = 0x3c
        $peOffset = $reader.ReadUInt32()
        if ($peOffset -gt $stream.Length - 26) { throw 'PE header is outside artifact bounds' }
        $stream.Position = $peOffset
        if ($reader.ReadUInt32() -ne 0x00004550) { throw 'Invalid PE signature' }
        $machine = $reader.ReadUInt16()
        $stream.Position = $peOffset + 22
        if (($reader.ReadUInt16() -band 0x2000) -eq 0) { throw 'PE artifact is not a DLL' }
        $stream.Position = $peOffset + 24
        $optionalMagic = $reader.ReadUInt16()
        $bits = if ($optionalMagic -eq 0x20b) { 64 } elseif ($optionalMagic -eq 0x10b) { 32 } else { throw 'Unknown PE optional header' }
    } elseif ($magic[0] -eq 0x7f -and $magic[1] -eq 0x45 -and $magic[2] -eq 0x4c -and $magic[3] -eq 0x46) {
        $format = 'ELF'
        $class = $reader.ReadByte()
        if ($reader.ReadByte() -ne 1) { throw 'Only little-endian release targets are supported' }
        $bits = if ($class -eq 1) { 32 } elseif ($class -eq 2) { 64 } else { throw 'Unknown ELF class' }
        $stream.Position = 16
        if ($reader.ReadUInt16() -ne 3) { throw 'ELF artifact is not a shared object' }
        $machine = $reader.ReadUInt16()
        $stream.Position = if ($bits -eq 32) { 36 } else { 48 }
        $abiFlags = $reader.ReadUInt32()
    } elseif ([BitConverter]::ToUInt32($magic, 0) -eq 4277009103) {
        # 当前分发 Intel/Apple Silicon 独立的薄 Mach-O 动态库，不把 fat/universal 文件误判为单架构。
        $format = 'Mach-O'
        $bits = 64
        if ($stream.Length -lt 32) { throw 'Mach-O header is truncated' }
        $machine = $reader.ReadUInt32()
        $null = $reader.ReadUInt32() # cpusubtype
        $machFileType = $reader.ReadUInt32()
        if ($machFileType -ne 6) { throw 'Mach-O artifact is not MH_DYLIB' }
    } else { throw 'Artifact is not a supported PE, ELF or thin little-endian Mach-O library' }
    if ($format -ne $nativeProfile.format -or $machine -ne $nativeProfile.machine -or $bits -ne $nativeProfile.bits) {
        throw "Artifact format/machine/width $format/$machine/$bits does not match $Target"
    }
    if ($Target -eq 'armv7-unknown-linux-gnueabihf' -and ($abiFlags -band 0x400) -eq 0) { throw 'ARMv7 candidate does not declare the hard-float ABI' }
    [pscustomobject]@{ target=$Target; format=$format; machine=$machine; pointerBits=$bits; elfFlags=$abiFlags; machFileType=$machFileType }
} finally { $reader.Dispose(); $stream.Dispose() }
