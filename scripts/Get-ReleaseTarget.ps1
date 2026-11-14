[CmdletBinding()]
param([string]$Target = '', [string]$Version = '')
$ErrorActionPreference = 'Stop'
$profiles = @(Get-Content -LiteralPath (Join-Path $PSScriptRoot 'release-targets.json') -Raw -Encoding UTF8 | ConvertFrom-Json)
if ($Version) {
    $releaseVersion = [version]($Version.TrimStart('v') -replace '[-+].*$', '')
    $profiles = @($profiles | Where-Object { [version]$_.sinceVersion -le $releaseVersion })
}
if ($Target) {
    $selected = @($profiles | Where-Object { $_.target -eq $Target })
    if ($selected.Count -ne 1) { throw "Unsupported release target/version: $Target $Version" }
    return $selected[0]
}
$profiles
