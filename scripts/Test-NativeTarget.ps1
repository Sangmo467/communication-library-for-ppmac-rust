[CmdletBinding()]
param([Parameter(Mandatory)][string]$Library, [Parameter(Mandatory)][string]$Target)
$ErrorActionPreference = 'Stop'
$expected = @{
    'i686-pc-windows-msvc' = @{ format = 'PE'; machine = 0x014c; bits = 32 }
    'x86_64-pc-windows-msvc' = @{ format = 'PE'; machine = 0x8664; bits = 64 }
    'aarch64-pc-windows-msvc' = @{ format = 'PE'; machine = 0xaa64; bits = 64 }
    'i686-unknown-linux-gnu' = @{ format = 'ELF'; machine = 3; bits = 32 }
    'x86_64-unknown-linux-gnu' = @{ format = 'ELF'; machine = 62; bits = 64 }
    'armv7-unknown-linux-gnueabihf' = @{ format = 'ELF'; machine = 40; bits = 32 }
    'aarch64-unknown-linux-gnu' = @{ format = 'ELF'; machine = 183; bits = 64 }
}
if (-not $expected.ContainsKey($Target)) { throw "Unsupported release target: $Target" }
$stream = [System.IO.File]::OpenRead((Resolve-Path -LiteralPath $Library).Path)
$reader = [System.IO.BinaryReader]::new($stream, [System.Text.Encoding]::UTF8, $true)
try {
    $abiFlags = $null
    $magic = $reader.ReadBytes(4)
    if ($magic.Length -lt 4) { throw 'Native artifact is truncated' }
    if ($magic[0] -eq 0x4d -and $magic[1] -eq 0x5a) {
        $format = 'PE'
        $stream.Position = 0x3c
        $peOffset = $reader.ReadUInt32()
        if ($peOffset -gt $stream.Length - 26) { throw 'PE header is outside artifact bounds' }
        $stream.Position = $peOffset
        if ($reader.ReadUInt32() -ne 0x00004550) { throw 'Invalid PE signature' }
        $machine = $reader.ReadUInt16()
        $stream.Position = $peOffset + 24
        $optionalMagic = $reader.ReadUInt16()
        $bits = if ($optionalMagic -eq 0x20b) { 64 } elseif ($optionalMagic -eq 0x10b) { 32 } else { throw 'Unknown PE optional header' }
    } elseif ($magic[0] -eq 0x7f -and $magic[1] -eq 0x45 -and $magic[2] -eq 0x4c -and $magic[3] -eq 0x46) {
        $format = 'ELF'
        $class = $reader.ReadByte()
        if ($reader.ReadByte() -ne 1) { throw 'Only little-endian release targets are supported' }
        $bits = if ($class -eq 1) { 32 } elseif ($class -eq 2) { 64 } else { throw 'Unknown ELF class' }
        $stream.Position = 18
        $machine = $reader.ReadUInt16()
        $stream.Position = if ($bits -eq 32) { 36 } else { 48 }
        $abiFlags = $reader.ReadUInt32()
    } else { throw 'Artifact is neither PE nor ELF' }
    $profile = $expected[$Target]
    if ($format -ne $profile.format -or $machine -ne $profile.machine -or $bits -ne $profile.bits) {
        throw "Artifact format/machine/width $format/$machine/$bits does not match $Target"
    }
    if ($Target -eq 'armv7-unknown-linux-gnueabihf' -and ($abiFlags -band 0x400) -eq 0) { throw 'ARMv7 candidate does not declare the hard-float ABI' }
    [pscustomobject]@{ target = $Target; format = $format; machine = $machine; pointerBits = $bits; elfFlags = $abiFlags }
} finally { $reader.Dispose(); $stream.Dispose() }
