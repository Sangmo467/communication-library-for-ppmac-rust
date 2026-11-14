[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$SbomPath,
    [Parameter(Mandatory)][string]$LockPath,
    [Parameter(Mandatory)][string]$Target
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$sbom = Get-Content -LiteralPath $SbomPath -Raw -Encoding UTF8 | ConvertFrom-Json
$provenance = [regex]::Match([string]$sbom.creationInfo.comment, '\Apmac:cargo-lock-sha256=(?<lock>[0-9a-f]{64});target=(?<target>[^\s;]+)\z')
if (-not $provenance.Success) { throw 'SBOM is missing Cargo.lock and target provenance' }
$lockHash = (Get-FileHash -LiteralPath $LockPath -Algorithm SHA256).Hash.ToLowerInvariant()
if ($provenance.Groups['lock'].Value -ne $lockHash) { throw 'SBOM Cargo.lock hash mismatch' }
if ($provenance.Groups['target'].Value -cne $Target) { throw 'SBOM target mismatch' }
$workspaceLock = Join-Path $root 'Cargo.lock'
if ((Get-FileHash -LiteralPath $workspaceLock -Algorithm SHA256).Hash.ToLowerInvariant() -ne $lockHash) {
    throw 'Dependency resolution requires the validated Cargo.lock to match the workspace Cargo.lock'
}

Push-Location $root
try {
    $metadataText = & cargo metadata --format-version 1 --locked --offline --filter-platform $Target
    if ($LASTEXITCODE -ne 0) { throw 'cargo metadata failed; fetch locked dependencies before validating an SBOM' }
    $metadata = ($metadataText -join "`n") | ConvertFrom-Json
} finally { Pop-Location }

$lockPackages = @{}
$lock = Get-Content -LiteralPath $LockPath -Raw -Encoding UTF8
foreach ($entry in [regex]::Matches($lock, '(?ms)^\[\[package\]\]\s*\n(?<body>.*?)(?=^\[\[package\]\]|\z)')) {
    $body = $entry.Groups['body'].Value
    $name = [regex]::Match($body, '(?m)^name = "(?<value>[^"]+)"').Groups['value'].Value
    $version = [regex]::Match($body, '(?m)^version = "(?<value>[^"]+)"').Groups['value'].Value
    $checksum = [regex]::Match($body, '(?m)^checksum = "(?<value>[^"]+)"').Groups['value'].Value
    if (-not $name -or -not $version) { throw 'Cargo.lock contains a package without a name or version' }
    $key = "$name@$version"
    if ($lockPackages.ContainsKey($key)) { throw "Cargo.lock contains an ambiguous duplicate package: $key" }
    $lockPackages[$key] = $checksum
}

$resolvedIds = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
foreach ($node in $metadata.resolve.nodes) { $null = $resolvedIds.Add($node.id) }
$identities = @{}
$expectedPackages = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
foreach ($package in $metadata.packages) {
    if (-not $resolvedIds.Contains($package.id)) { continue }
    $key = "$($package.name)@$($package.version)"
    if (-not $expectedPackages.Add($key)) { throw "Resolved dependency set contains an ambiguous package: $key" }
    $identities[$package.id] = $key
}
$expectedRelationships = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
foreach ($node in $metadata.resolve.nodes) {
    foreach ($dependency in $node.dependencies) {
        if (-not $identities.ContainsKey($node.id) -or -not $identities.ContainsKey($dependency)) { throw 'Resolved dependency graph references an unknown package' }
        $edge = "$($identities[$node.id])`t$($identities[$dependency])"
        $null = $expectedRelationships.Add($edge)
    }
}

$seen = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
$packageIds = @{}
foreach ($package in @($sbom.packages)) {
    $key = "$($package.name)@$($package.versionInfo)"
    if (-not $package.name -or -not $package.versionInfo -or -not $seen.Add($key)) { throw 'SBOM contains an invalid or duplicate dependency identity' }
    if (-not $package.SPDXID -or $packageIds.ContainsKey($package.SPDXID)) { throw 'SBOM contains an invalid or duplicate SPDX package identifier' }
    $packageIds[$package.SPDXID] = $key
    if (-not $lockPackages.ContainsKey($key)) { throw "SBOM dependency is absent from Cargo.lock: $key" }
    $sha256 = @($package.checksums | Where-Object { $_.algorithm -eq 'SHA256' })
    $lockedChecksum = [string]$lockPackages[$key]
    if ($lockedChecksum) {
        if ($sha256.Count -ne 1 -or $sha256[0].checksumValue -cne $lockedChecksum) { throw "SBOM dependency checksum mismatch: $key" }
    } elseif ($sha256.Count -ne 0) {
        throw "SBOM dependency has a checksum absent from Cargo.lock: $key"
    }
}
foreach ($key in $expectedPackages) {
    if (-not $seen.Contains($key)) { throw "SBOM is missing resolved dependency: $key" }
}
foreach ($key in $seen) {
    if (-not $expectedPackages.Contains($key)) { throw "SBOM dependency is not resolved for target ${Target}: $key" }
}

$relationships = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
foreach ($relationship in @($sbom.relationships | Where-Object { $_.relationshipType -eq 'DEPENDS_ON' })) {
    if (-not $packageIds.ContainsKey($relationship.spdxElementId) -or -not $packageIds.ContainsKey($relationship.relatedSpdxElement)) {
        throw 'SBOM dependency relationship references an absent package'
    }
    $edge = "$($packageIds[$relationship.spdxElementId])`t$($packageIds[$relationship.relatedSpdxElement])"
    if (-not $relationships.Add($edge)) { throw "SBOM contains a duplicate dependency relationship: $edge" }
}
foreach ($edge in $expectedRelationships) {
    if (-not $relationships.Contains($edge)) { throw "SBOM is missing resolved dependency relationship: $edge" }
}
foreach ($edge in $relationships) {
    if (-not $expectedRelationships.Contains($edge)) { throw "SBOM dependency relationship is not resolved for target ${Target}: $edge" }
}
