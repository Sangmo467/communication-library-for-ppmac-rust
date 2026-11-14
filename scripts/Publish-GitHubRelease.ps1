[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$AssetDirectory,
    [Parameter(Mandatory)][ValidatePattern('^v\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$')][string]$Tag,
    [Parameter(Mandatory)][ValidatePattern('^[0-9a-fA-F]{40}$')][string]$Commit,
    [string]$Owner = 'Sangmo467',
    [string]$Repository = 'communication-library-for-ppmac-rust',
    [switch]$ValidateOnly
)
$ErrorActionPreference = 'Stop'
# Token 只从进程环境读取，不接受命令行明文，不写入 URL 或构建产物。
$assets = (Resolve-Path -LiteralPath $AssetDirectory).Path
$targets = @('i686-pc-windows-msvc','x86_64-pc-windows-msvc','i686-unknown-linux-gnu','x86_64-unknown-linux-gnu')
$files = @()
$snapshot = $null
foreach ($target in $targets) {
    $name = "pmac-$($Tag.Substring(1))-$target-release.zip"
    $path = Join-Path $assets $name
    $sidecar = "$path.sha256"
    if (-not (Test-Path -LiteralPath $path) -or -not (Test-Path -LiteralPath $sidecar)) { throw "Missing release asset or checksum: $name" }
    $expected = (Get-Content -LiteralPath $sidecar -Raw -Encoding UTF8).Trim()
    $actual = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($expected -ne "$actual  $name") { throw "Checksum mismatch: $name" }
    $zip = [IO.Compression.ZipFile]::OpenRead($path)
    try {
        $entries = @($zip.Entries | Where-Object { $_.FullName -match '^[^/]+/manifest\.json$' })
        if ($entries.Count -ne 1) { throw "Missing unique root manifest: $name" }
        $reader = [IO.StreamReader]::new($entries[0].Open(), [Text.Encoding]::UTF8)
        try { $manifest = $reader.ReadToEnd() | ConvertFrom-Json } finally { $reader.Dispose() }
        if ($manifest.schemaVersion -ne 1 -or $manifest.dirtySource -ne $false -or
            $manifest.target -ne $target -or $manifest.profile -ne 'release' -or
            $manifest.version -ne $Tag.Substring(1) -or $manifest.commit -ne $Commit) {
            throw "Asset provenance/version/target does not match the clean public release commit: $name"
        }
        if (-not $snapshot) { $snapshot = $manifest.sourceSnapshotSha256 }
        if (-not $snapshot -or $manifest.sourceSnapshotSha256 -ne $snapshot) { throw 'Four platform assets must have identical source snapshots' }
        $prefix = $entries[0].FullName.Substring(0, $entries[0].FullName.Length - 'manifest.json'.Length)
        $seen = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
        foreach ($entry in $manifest.files) {
            if ($entry.path -match '(^/|\\|(^|/)\.\.(/|$))' -or -not $seen.Add($entry.path)) { throw 'Invalid manifest path' }
            $member = $zip.GetEntry($prefix + $entry.path)
            if (-not $member -or $member.Length -ne $entry.bytes) { throw "Missing or truncated archive member: $($entry.path)" }
            $stream = $member.Open()
            try { $sha = [Security.Cryptography.SHA256]::HashData($stream) } finally { $stream.Dispose() }
            if ([Convert]::ToHexString($sha).ToLowerInvariant() -ne $entry.sha256) { throw "Archive member checksum mismatch: $($entry.path)" }
        }
        if (@($zip.Entries | Where-Object { $_.Name }).Count -ne $manifest.files.Count + 1) { throw 'Archive has unlisted files' }
    } finally { $zip.Dispose() }
    $files += @($path, $sidecar)
}
if ($ValidateOnly) { Write-Host 'All four release archives passed validation; no network request made.'; return }
if (-not $env:GH_TOKEN -and -not $env:GITHUB_TOKEN) { throw 'Configure GH_TOKEN or GITHUB_TOKEN before publishing' }
$repo = "$Owner/$Repository"
# --verify-tag 防止意外创建与源码清单不一致的版本标签。
$referenceText = & gh api "repos/$repo/git/ref/tags/$Tag"
if ($LASTEXITCODE -ne 0) { throw 'Version tag must already exist on GitHub' }
$reference = ($referenceText -join "`n") | ConvertFrom-Json
$object = $reference.object
for ($depth = 0; $object.type -eq 'tag' -and $depth -lt 8; $depth++) {
    $annotated = & gh api "repos/$repo/git/tags/$($object.sha)"
    if ($LASTEXITCODE -ne 0) { throw 'Cannot resolve annotated release tag' }
    $object = (($annotated -join "`n") | ConvertFrom-Json).object
}
if ($object.type -ne 'commit' -or $object.sha -ne $Commit) { throw 'GitHub tag commit does not match the four SDK manifests' }
& gh release view $Tag --repo $repo *> $null
if ($LASTEXITCODE -eq 0) { throw 'Release already exists; refusing to overwrite published artifacts' }
$notes = "Prebuilt Windows/Linux x86/x64 SDKs, buildable examples and SHA-256 checksums. English guides are attached to this release. Windows/Linux x86/x64 SDK。包含预编译库、C/C++/Python/.NET 绑定、详细中文注释调用例程、独立 CMake 工程及 SHA-256 校验。全部目标已完成 Rust 和 C/C++ 离线测试；支持的架构另验证 Python/.NET。"
 # Existing tag was verified above; do not pass target_commitish for an older workflow revision.
& gh release create $Tag @files --repo $repo --verify-tag --title "PMAC $Tag — Windows/Linux x86/x64 SDK" --notes $notes
if ($LASTEXITCODE -ne 0) { throw 'GitHub release upload failed; inspect remote state before retrying' }
# 英文文档可作为独立附件补充，不修改已经验证的二进制 ZIP 或版本标签。
$sourceRoot = Split-Path $PSScriptRoot -Parent
foreach ($relative in @('README.en.md','docs/prebuilt-sdk.en.md')) {
    $document = Join-Path $sourceRoot $relative
    if (Test-Path -LiteralPath $document) {
        & gh release upload $Tag $document --repo $repo
        if ($LASTEXITCODE -ne 0) { throw 'English release documentation upload failed' }
    }
}
$publishedText = & gh release view $Tag --repo $repo --json assets
if ($LASTEXITCODE -ne 0) { throw 'Cannot verify GitHub release attachments' }
$published = ($publishedText -join "`n") | ConvertFrom-Json
foreach ($file in $files) {
    if (-not @($published.assets | Where-Object { $_.name -eq [IO.Path]::GetFileName($file) }).Count) { throw 'GitHub release attachment verification failed' }
}
Write-Host "GitHub Release: https://github.com/$repo/releases/tag/$Tag"
