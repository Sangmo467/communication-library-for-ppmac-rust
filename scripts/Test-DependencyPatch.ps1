[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$vendor = Join-Path $root 'vendor/russh'
$patch = Get-Content -LiteralPath (Join-Path $root 'vendor/russh-patch.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$expected = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
foreach ($line in Get-Content -LiteralPath (Join-Path $root 'vendor/russh-upstream-files.sha256') -Encoding UTF8) {
    if ($line -notmatch '^(?<hash>[a-f0-9]{64})  (?<path>.+)$') { throw 'Invalid upstream file-hash list' }
    $relative = $Matches.path
    $originalHash = $Matches.hash
    $null = $expected.Add($relative)
    $actualHash = (Get-FileHash -LiteralPath (Join-Path $vendor $relative) -Algorithm SHA256).Hash.ToLowerInvariant()
    $expectedHash = if ($relative -eq 'Cargo.toml') { $patch.patchedCargoTomlSha256 } elseif ($relative -eq 'src/keys/format/pkcs8.rs') { $patch.patchedPkcs8Sha256 } else { $originalHash }
    if ($actualHash -ne $expectedHash) { throw "Unexpected vendor source change: $relative" }
}
$actualFiles = @(Get-ChildItem -LiteralPath $vendor -Recurse -File -Force)
if ($actualFiles.Count -ne $expected.Count) { throw 'Vendor source has an unexpected file count' }
foreach ($file in $actualFiles) {
    $relative = [IO.Path]::GetRelativePath($vendor,$file.FullName).Replace([char]92,[char]47)
    if (-not $expected.Contains($relative)) { throw "Unrecorded vendor source: $relative" }
}
$patched = Get-Content -LiteralPath (Join-Path $vendor 'Cargo.toml') -Raw -Encoding UTF8
$restored = $patched.Replace('version = "=0.10.0"','version = "=0.10.0-rc.18"').Replace('version = "=0.8.0"','version = "=0.8.0-rc.4"').Replace('version = "=0.7.0-rc.12"','version = "=0.7.0-rc.11"')
$restoredHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($restored))).ToLowerInvariant()
if ($restoredHash -ne $patch.upstreamCargoTomlSha256) { throw 'Vendor manifest has changes beyond the documented dependency pins' }
$pkcs8 = Get-Content -LiteralPath (Join-Path $vendor 'src/keys/format/pkcs8.rs') -Raw -Encoding UTF8
$restoredPkcs8 = $pkcs8.Replace('let sk = &pkcs1::RsaPrivateKeyRef::try_from(pki.private_key)?;', 'let sk = &pkcs1::RsaPrivateKey::try_from(pki.private_key)?;')
$restoredPkcs8Hash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($restoredPkcs8))).ToLowerInvariant()
if ($restoredPkcs8Hash -ne $patch.upstreamPkcs8Sha256) { throw 'Unexpected change to RSA key decoding source' }
Write-Output "Vendored russh provenance passed: $($expected.Count) recorded files; manifest, ASN.1 and channel-close patches match"
