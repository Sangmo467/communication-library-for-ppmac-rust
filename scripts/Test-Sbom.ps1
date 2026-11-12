[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$SbomPath,
    [Parameter(Mandatory)][string]$LockPath,
    [Parameter(Mandatory)][string]$Target
)
$ErrorActionPreference = 'Stop'
$sbom = Get-Content -LiteralPath $SbomPath -Raw -Encoding UTF8 | ConvertFrom-Json
$provenance = [regex]::Match([string]$sbom.creationInfo.comment, '\Apmac:cargo-lock-sha256=(?<lock>[0-9a-f]{64});target=(?<target>[^\s;]+)\z')
if (-not $provenance.Success) { throw 'SBOM is missing Cargo.lock and target provenance' }
$lockHash = (Get-FileHash -LiteralPath $LockPath -Algorithm SHA256).Hash.ToLowerInvariant()
if ($provenance.Groups['lock'].Value -ne $lockHash) { throw 'SBOM Cargo.lock hash mismatch' }
if ($provenance.Groups['target'].Value -cne $Target) { throw 'SBOM target mismatch' }

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

$seen = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
foreach ($package in @($sbom.packages)) {
    $key = "$($package.name)@$($package.versionInfo)"
    if (-not $package.name -or -not $package.versionInfo -or -not $seen.Add($key)) { throw 'SBOM contains an invalid or duplicate dependency identity' }
    if (-not $lockPackages.ContainsKey($key)) { throw "SBOM dependency is absent from Cargo.lock: $key" }
    $sha256 = @($package.checksums | Where-Object { $_.algorithm -eq 'SHA256' })
    $lockedChecksum = [string]$lockPackages[$key]
    if ($lockedChecksum) {
        if ($sha256.Count -ne 1 -or $sha256[0].checksumValue -cne $lockedChecksum) { throw "SBOM dependency checksum mismatch: $key" }
    } elseif ($sha256.Count -ne 0) {
        throw "SBOM dependency has a checksum absent from Cargo.lock: $key"
    }
}
