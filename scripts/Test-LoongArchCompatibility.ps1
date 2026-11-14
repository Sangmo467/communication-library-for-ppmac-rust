[CmdletBinding()]
param([Parameter(Mandatory)][string]$Library)
$ErrorActionPreference = 'Stop'
$report = & loongarch64-linux-gnu-readelf --version-info --wide $Library
if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect LoongArch ELF symbol versions' }
$text = $report -join "`n"
if ($text -match 'GLIBC_PRIVATE') { throw 'Release library depends on private glibc ABI' }
$versions = @([regex]::Matches($text,'GLIBC_(\d+\.\d+)') | ForEach-Object { [version]$_.Groups[1].Value } | Sort-Object -Unique)
if (-not $versions.Count) { throw 'No glibc version requirements found' }
$required = $versions[-1]
if ($required -gt [version]'2.36') { throw "LoongArch library exceeds glibc 2.36 baseline: requires $required" }
[pscustomobject]@{ target='loongarch64-unknown-linux-gnu'; requiredGlibc=$required.ToString(); maximumGlibc='2.36'; abi='LP64D ELF ABI v1'; validation='ELF symbol versions; QEMU runtime tests are reported separately' }
