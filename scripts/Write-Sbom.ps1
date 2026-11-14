[CmdletBinding()]
param([string]$OutputPath = 'build/reports/sbom.spdx.json', [string]$Target = '')
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Push-Location $root
try {
    $arguments = @('metadata', '--format-version', '1', '--locked', '--offline')
    if ($Target) { $arguments += @('--filter-platform', $Target) }
    $metadataText = & cargo @arguments
    if ($LASTEXITCODE -ne 0) { throw 'cargo metadata failed; fetch locked dependencies before generating an SBOM' }
    $metadata = ($metadataText -join "`n") | ConvertFrom-Json
    $lock = Get-Content -LiteralPath 'Cargo.lock' -Raw -Encoding UTF8
    $checksums = @{}
    foreach ($entry in [regex]::Matches($lock, '(?ms)^\[\[package\]\]\s*\n(?<body>.*?)(?=^\[\[package\]\]|\z)')) {
        $body = $entry.Groups['body'].Value
        $name = [regex]::Match($body, '(?m)^name = "(?<value>[^"]+)"').Groups['value'].Value
        $version = [regex]::Match($body, '(?m)^version = "(?<value>[^"]+)"').Groups['value'].Value
        $checksum = [regex]::Match($body, '(?m)^checksum = "(?<value>[^"]+)"').Groups['value'].Value
        if ($checksum) { $checksums["$name@$version"] = $checksum }
    }
    $resolvedIds = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    foreach ($node in $metadata.resolve.nodes) { $null = $resolvedIds.Add($node.id) }
    $resolvedPackages = @($metadata.packages | Where-Object { $resolvedIds.Contains($_.id) })
    $identifiers = @{}
    $index = 0
    foreach ($package in $resolvedPackages) { $identifiers[$package.id] = "SPDXRef-Package-$index"; $index++ }
    $packages = @($resolvedPackages | ForEach-Object {
        $package = $_
        $item = [ordered]@{
            SPDXID = $identifiers[$package.id]
            name = $package.name
            versionInfo = $package.version
            downloadLocation = 'NOASSERTION'
            filesAnalyzed = $false
            licenseConcluded = 'NOASSERTION'
            licenseDeclared = $(if ($package.license) { $package.license -replace '\s*/\s*', ' OR ' } else { 'NOASSERTION' })
            copyrightText = 'NOASSERTION'
            externalRefs = @(@{ referenceCategory = 'PACKAGE-MANAGER'; referenceType = 'purl'; referenceLocator = "pkg:cargo/$($package.name)@$($package.version)" })
        }
        if ($package.source -like 'registry+*crates.io*') {
            $item.downloadLocation = "https://crates.io/api/v1/crates/$($package.name)/$($package.version)/download"
        }
        if ($package.manifest_path.Replace([char]92,[char]47).Contains('/vendor/russh/')) {
            $item.comment = 'Local minimal dependency/type adapter patch; see dependency-patches.md and russh-patch.json in the candidate package. Source snapshot covers vendor content.'
        }
        $key = "$($package.name)@$($package.version)"
        if ($checksums.ContainsKey($key)) { $item.checksums = @(@{ algorithm = 'SHA256'; checksumValue = $checksums[$key] }) }
        $item
    })
    $relationships = [System.Collections.Generic.List[object]]::new()
    foreach ($member in $metadata.workspace_members) {
        $relationships.Add(@{ spdxElementId = 'SPDXRef-DOCUMENT'; relationshipType = 'DESCRIBES'; relatedSpdxElement = $identifiers[$member] })
    }
    foreach ($node in $metadata.resolve.nodes) {
        foreach ($dependency in $node.dependencies) {
            if ($identifiers.ContainsKey($dependency)) {
                $relationships.Add(@{ spdxElementId = $identifiers[$node.id]; relationshipType = 'DEPENDS_ON'; relatedSpdxElement = $identifiers[$dependency] })
            }
        }
    }
    $document = [ordered]@{
        spdxVersion = 'SPDX-2.3'
        dataLicense = 'CC0-1.0'
        SPDXID = 'SPDXRef-DOCUMENT'
        name = $(if ($Target) { "pmac-$Target" } else { 'pmac-all-resolved-targets' })
        documentNamespace = "https://spdx.org/spdxdocs/pmac-$([guid]::NewGuid())"
        creationInfo = @{ creators = @('Tool: pmac-Write-Sbom.ps1'); created = [DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ssZ'); comment = "pmac:cargo-lock-sha256=$((Get-FileHash -LiteralPath 'Cargo.lock' -Algorithm SHA256).Hash.ToLowerInvariant());target=$(if ($Target) { $Target } else { 'all-resolved-targets' })" }
        packages = $packages
        relationships = $relationships.ToArray()
        comment = 'Generated from locked cargo metadata. Includes resolved development/build dependencies; this is not a license clearance or hardware acceptance statement.'
    }
    if (-not [System.IO.Path]::IsPathRooted($OutputPath)) { $OutputPath = Join-Path $root $OutputPath }
    New-Item -ItemType Directory -Path (Split-Path $OutputPath -Parent) -Force | Out-Null
    $document | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $OutputPath -Encoding utf8NoBOM
    Write-Output "SPDX document: $OutputPath ($($packages.Count) packages)"
} finally { Pop-Location }
