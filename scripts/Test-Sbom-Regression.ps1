[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$PackageDirectory,
    [string]$WorkDirectory = ''
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$packageRoot = (Resolve-Path -LiteralPath $PackageDirectory).Path
$manifest = Get-Content -LiteralPath (Join-Path $packageRoot 'manifest.json') -Raw -Encoding UTF8 | ConvertFrom-Json
if (-not $WorkDirectory) { $WorkDirectory = "build/sbom-regression-$([guid]::NewGuid().ToString('N'))" }
if (-not [System.IO.Path]::IsPathRooted($WorkDirectory)) { $WorkDirectory = Join-Path $root $WorkDirectory }
New-Item -ItemType Directory -Path $WorkDirectory -Force | Out-Null
Push-Location $root
try {
    $sourceSbom = Join-Path $packageRoot 'sbom.spdx.json'
    $lock = Join-Path $packageRoot 'Cargo.lock'
    $validator = Join-Path $PSScriptRoot 'Test-Sbom.ps1'
    & $validator -SbomPath $sourceSbom -LockPath $lock -Target $manifest.target

    function Assert-Rejected([string]$Case, [string]$Expected, [scriptblock]$Operation) {
        $error = $null
        try { & $Operation } catch { $error = $_ }
        if (-not $error -or $error.Exception.Message -notlike "*$Expected*") {
            if ($error) { throw "SBOM regression '$Case' failed for the wrong reason: $($error.Exception.Message)" }
            throw "SBOM regression '$Case' was accepted"
        }
    }
    function Select-CheckedPackage($sbom) {
        return @($sbom.packages | Where-Object { @($_.checksums | Where-Object { $_.algorithm -eq 'SHA256' }).Count -eq 1 })[0]
    }

    $stale = Join-Path $WorkDirectory 'stale-version.json'
    $sbom = Get-Content -LiteralPath $sourceSbom -Raw -Encoding UTF8 | ConvertFrom-Json
    (Select-CheckedPackage $sbom).versionInfo = '0.0.0-review-stale'
    $sbom | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $stale -Encoding utf8NoBOM
    Assert-Rejected 'stale version' 'absent from Cargo.lock' { & $validator -SbomPath $stale -LockPath $lock -Target $manifest.target }

    $badChecksum = Join-Path $WorkDirectory 'bad-checksum.json'
    $sbom = Get-Content -LiteralPath $sourceSbom -Raw -Encoding UTF8 | ConvertFrom-Json
    @((Select-CheckedPackage $sbom).checksums | Where-Object { $_.algorithm -eq 'SHA256' })[0].checksumValue = '0' * 64
    $sbom | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $badChecksum -Encoding utf8NoBOM
    Assert-Rejected 'wrong checksum' 'checksum mismatch' { & $validator -SbomPath $badChecksum -LockPath $lock -Target $manifest.target }

    $badLock = Join-Path $WorkDirectory 'bad-lock.json'
    $sbom = Get-Content -LiteralPath $sourceSbom -Raw -Encoding UTF8 | ConvertFrom-Json
    $sbom.creationInfo.comment = $sbom.creationInfo.comment -replace 'cargo-lock-sha256=[0-9a-f]{64}', "cargo-lock-sha256=$('0' * 64)"
    $sbom | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $badLock -Encoding utf8NoBOM
    Assert-Rejected 'wrong lock provenance' 'Cargo.lock hash mismatch' { & $validator -SbomPath $badLock -LockPath $lock -Target $manifest.target }

    $badTarget = Join-Path $WorkDirectory 'bad-target.json'
    $sbom = Get-Content -LiteralPath $sourceSbom -Raw -Encoding UTF8 | ConvertFrom-Json
    $sbom.creationInfo.comment = $sbom.creationInfo.comment -replace 'target=[^\s;]+$', 'target=wrong-target'
    $sbom | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $badTarget -Encoding utf8NoBOM
    Assert-Rejected 'wrong target' 'target mismatch' { & $validator -SbomPath $badTarget -LockPath $lock -Target $manifest.target }

    Assert-Rejected 'packaging stale SBOM' 'absent from Cargo.lock' {
        ./scripts/Package-Release.ps1 -Target $manifest.target -ArtifactDirectory (Join-Path $packageRoot 'native') -OutputDirectory (Join-Path $WorkDirectory 'package-rejected') -SbomPath $stale -AllowUncommitted | Out-Null
    }

    $candidate = Join-Path $WorkDirectory 'candidate'
    Copy-Item -LiteralPath $packageRoot -Destination $candidate -Recurse
    $candidateSbom = Join-Path $candidate 'sbom.spdx.json'
    Copy-Item -LiteralPath $stale -Destination $candidateSbom
    $candidateManifestPath = Join-Path $candidate 'manifest.json'
    $candidateManifest = Get-Content -LiteralPath $candidateManifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
    $entry = @($candidateManifest.files | Where-Object { $_.path -eq 'sbom.spdx.json' })[0]
    $item = Get-Item -LiteralPath $candidateSbom
    $entry.bytes = $item.Length
    $entry.sha256 = (Get-FileHash -LiteralPath $candidateSbom -Algorithm SHA256).Hash.ToLowerInvariant()
    $candidateManifest | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $candidateManifestPath -Encoding utf8NoBOM
    Assert-Rejected 'candidate stale SBOM' 'absent from Cargo.lock' {
        ./scripts/Test-Release.ps1 -PackageDirectory $candidate | Out-Null
    }
    Write-Output 'SBOM provenance and dependency regression passed'
} finally {
    Pop-Location
    Remove-Item -LiteralPath $WorkDirectory -Recurse -Force -ErrorAction SilentlyContinue
}
