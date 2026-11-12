[CmdletBinding()]
param([Parameter(Mandatory)][string]$PackageDirectory)
$ErrorActionPreference = 'Stop'
$packageRoot = (Resolve-Path -LiteralPath $PackageDirectory).Path
$prefix = $packageRoot.TrimEnd([char]92,[char]47) + [System.IO.Path]::DirectorySeparatorChar
$manifest = Get-Content -LiteralPath (Join-Path $packageRoot 'manifest.json') -Raw -Encoding UTF8 | ConvertFrom-Json
if ($manifest.schemaVersion -ne 1 -or -not $manifest.files -or -not $manifest.target) { throw 'Invalid candidate manifest' }
$seen = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
foreach ($file in $manifest.files) {
    $fullPath = [System.IO.Path]::GetFullPath((Join-Path $packageRoot $file.path))
    if (-not $fullPath.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) { throw 'Manifest path escapes package directory' }
    if (-not $seen.Add($fullPath)) { throw "Duplicate artifact path: $($file.path)" }
    $item = Get-Item -LiteralPath $fullPath
    if ($item.Length -ne $file.bytes -or (Get-FileHash -LiteralPath $fullPath -Algorithm SHA256).Hash -ne $file.sha256) { throw "Artifact integrity check failed: $($file.path)" }
}
if (@(Get-ChildItem -LiteralPath $packageRoot -Recurse -File).Count -ne $manifest.files.Count + 1) { throw 'Package contains files that are absent from the manifest' }
$lockHash = (Get-FileHash -LiteralPath (Join-Path $packageRoot 'Cargo.lock') -Algorithm SHA256).Hash
if ($lockHash -ne $manifest.cargoLockSha256) { throw 'Dependency lock hash mismatch' }
& (Join-Path $PSScriptRoot 'Test-Sbom.ps1') -SbomPath (Join-Path $packageRoot 'sbom.spdx.json') -LockPath (Join-Path $packageRoot 'Cargo.lock') -Target $manifest.target
$sbom = Get-Content -LiteralPath (Join-Path $packageRoot 'sbom.spdx.json') -Raw -Encoding UTF8 | ConvertFrom-Json
if ($sbom.spdxVersion -ne 'SPDX-2.3' -or $sbom.dataLicense -ne 'CC0-1.0' -or -not $sbom.packages) { throw 'Missing SPDX package inventory' }
$identifiers = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
$null = $identifiers.Add('SPDXRef-DOCUMENT')
foreach ($package in $sbom.packages) {
    if (-not $identifiers.Add($package.SPDXID) -or -not $package.name -or -not $package.versionInfo) { throw 'Invalid or duplicate SPDX package' }
}
foreach ($relationship in $sbom.relationships) {
    if (-not $identifiers.Contains($relationship.spdxElementId) -or -not $identifiers.Contains($relationship.relatedSpdxElement)) { throw 'SPDX relationship references an absent package' }
}
$library = if ($manifest.target -like '*windows*') { 'pmac.dll' } else { 'libpmac.so' }
$identity = & (Join-Path $PSScriptRoot 'Test-NativeTarget.ps1') -Library (Join-Path $packageRoot "native/$library") -Target $manifest.target
[pscustomobject]@{ integrity = 'passed'; target = $identity.target; files = $manifest.files.Count; dependencyPackages = $sbom.packages.Count; acceptance = $manifest.acceptance }
