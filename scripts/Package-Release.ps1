[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('i686-pc-windows-msvc','x86_64-pc-windows-msvc','aarch64-pc-windows-msvc','i686-unknown-linux-gnu','x86_64-unknown-linux-gnu','armv7-unknown-linux-gnueabihf','aarch64-unknown-linux-gnu')][string]$Target,
    [ValidateSet('debug','release')][string]$Profile = 'release',
    [string]$ArtifactDirectory = '',
    [string]$OutputDirectory = 'dist',
    [string]$SbomPath = 'build/reports/sbom.spdx.json',
    [string]$EvidenceDirectory = '',
    [switch]$AllowUncommitted
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Push-Location $root
try {
    $workspaceManifest = Get-Content -LiteralPath 'Cargo.toml' -Raw -Encoding UTF8
    $packageSection = [regex]::Match($workspaceManifest, '(?ms)^\[workspace\.package\]\s*(?<body>.*?)(?=^\[|\z)').Groups['body'].Value
    $version = [regex]::Match($packageSection, '(?m)^version\s*=\s*"(?<value>[^"]+)"').Groups['value'].Value
    if (-not $version) { throw 'Workspace version is missing' }
    $header = Get-Content -LiteralPath 'include/pmac.h' -Raw -Encoding UTF8
    $abiMajor = [int][regex]::Match($header, '#define\s+PMAC_ABI_MAJOR\s+(?<value>\d+)').Groups['value'].Value
    $abiMinor = [int][regex]::Match($header, '#define\s+PMAC_ABI_MINOR\s+(?<value>\d+)').Groups['value'].Value
    $commit = (& git rev-parse --verify HEAD 2>$null) -join ''
    $hasCommit = $LASTEXITCODE -eq 0
    $status = @(& git status --porcelain --untracked-files=normal)
    if ((-not $hasCommit -or $status.Count -ne 0) -and -not $AllowUncommitted) {
        throw 'Release packaging requires a committed, clean source tree; -AllowUncommitted produces a development candidate only'
    }
    if (-not $ArtifactDirectory) { $ArtifactDirectory = "target/$Target/$Profile" }
    if (-not [System.IO.Path]::IsPathRooted($ArtifactDirectory)) { $ArtifactDirectory = Join-Path $root $ArtifactDirectory }
    $library = if ($Target -like '*windows*') { 'pmac.dll' } else { 'libpmac.so' }
    $nativePath = Join-Path $ArtifactDirectory $library
    if (-not (Test-Path -LiteralPath $nativePath -PathType Leaf)) { throw "Native artifact does not exist: $nativePath" }
    $nativeIdentity = & (Join-Path $PSScriptRoot 'Test-NativeTarget.ps1') -Library $nativePath -Target $Target
    if (-not (Test-Path -LiteralPath $SbomPath -PathType Leaf)) { throw 'Generate the target SBOM with Write-Sbom.ps1 before packaging' }
    & (Join-Path $PSScriptRoot 'Test-Sbom.ps1') -SbomPath $SbomPath -LockPath (Join-Path $root 'Cargo.lock') -Target $Target
    $destination = [System.IO.Path]::GetFullPath((Join-Path (Join-Path $root $OutputDirectory) "pmac-$version-$Target-$Profile"))
    $rootPrefix = $root.TrimEnd([char]92, [char]47) + [System.IO.Path]::DirectorySeparatorChar
    if (-not $destination.StartsWith($rootPrefix, [StringComparison]::OrdinalIgnoreCase)) { throw 'Output directory must stay inside the workspace' }
    if (Test-Path -LiteralPath $destination) { throw "Refusing to overwrite an existing candidate directory: $destination" }
    New-Item -ItemType Directory -Path (Join-Path $destination 'native') -Force | Out-Null
    Copy-Item -LiteralPath $nativePath -Destination (Join-Path $destination 'native')
    foreach ($optional in @('pmac.dll.lib','pmac.lib','libpmac.a','pmac.pdb')) {
        $file = Join-Path $ArtifactDirectory $optional
        if (Test-Path -LiteralPath $file -PathType Leaf) { Copy-Item -LiteralPath $file -Destination (Join-Path $destination 'native') }
    }
    foreach ($directory in @('include','bindings/cpp','bindings/python/pmac','bindings/dotnet/Pmac')) {
        foreach ($file in Get-ChildItem -LiteralPath (Join-Path $root $directory) -Recurse -File) {
            if ($file.FullName -match '[\\/](bin|obj|__pycache__)[\\/]' -or $file.Extension -notin @('.h','.hpp','.cs','.csproj','.py','.toml','.json')) { continue }
            $relative = [System.IO.Path]::GetRelativePath($root, $file.FullName)
            $copyPath = Join-Path $destination $relative
            New-Item -ItemType Directory -Path (Split-Path $copyPath -Parent) -Force | Out-Null
            Copy-Item -LiteralPath $file.FullName -Destination $copyPath
        }
    }
    foreach ($file in @('LICENSE','THIRD_PARTY_NOTICES.md','Cargo.lock','rust-toolchain.toml')) { Copy-Item -LiteralPath $file -Destination $destination }
    # 白名单复制可独立编译的公开例程，排除内部审核和独立测试 fixture。
    $sdkFiles = @('cmake/PmacConfig.cmake','examples/CMakeLists.txt',
        'examples/c/sdk_smoke.c','examples/c/getresponse.c','examples/c/geterrors.c',
        'examples/cpp/sdk_smoke.cpp','examples/cpp/getresponse.cpp','examples/cpp/geterrors.cpp','examples/cpp/gather.cpp',
        'examples/python/getresponse.py','examples/python/sdk_smoke.py',
        'examples/dotnet/getresponse/Program.cs','examples/dotnet/getresponse/Pmac.GetResponse.csproj',
        'examples/matlab/build_pmac_mex.m','examples/matlab/pmac_mex.cpp','examples/matlab/example_getresponse.m',
        'bindings/python/pyproject.toml','scripts/Test-SdkExamples.ps1',
        'docs/prebuilt-sdk.md','docs/prebuilt-sdk.en.md','docs/gather.md','docs/geterrors.md',
        'docs/abi-contract.md','docs/authentication.md','docs/getresponse-passthrough.md')
    foreach ($relative in $sdkFiles) {
        $copyPath = Join-Path $destination $relative
        New-Item -ItemType Directory -Path (Split-Path $copyPath -Parent) -Force | Out-Null
        Copy-Item -LiteralPath (Join-Path $root $relative) -Destination $copyPath
    }
    Copy-Item -LiteralPath 'docs/prebuilt-sdk.md' -Destination (Join-Path $destination 'README.md')
    Copy-Item -LiteralPath 'docs/prebuilt-sdk.en.md' -Destination (Join-Path $destination 'README.en.md')
    New-Item -ItemType Directory -Path (Join-Path $destination 'third-party/russh') -Force | Out-Null
    Copy-Item -LiteralPath 'vendor/russh/LICENSE' -Destination (Join-Path $destination 'third-party/russh/LICENSE')
    Copy-Item -LiteralPath $SbomPath -Destination (Join-Path $destination 'sbom.spdx.json')
    if (Test-Path -LiteralPath 'docs/dependency-patches.md') { Copy-Item -LiteralPath 'docs/dependency-patches.md' -Destination $destination }
    foreach ($provenanceFile in @('vendor/russh-patch.json','vendor/russh-upstream-files.sha256')) { if (Test-Path -LiteralPath $provenanceFile) { Copy-Item -LiteralPath $provenanceFile -Destination $destination } }
    if ($EvidenceDirectory) {
        if (-not (Test-Path -LiteralPath $EvidenceDirectory -PathType Container)) { throw 'Evidence directory does not exist' }
        Copy-Item -LiteralPath $EvidenceDirectory -Destination (Join-Path $destination 'evidence') -Recurse
    }
    $sourcePaths = @(& git ls-files --cached --others --exclude-standard -- Cargo.toml Cargo.lock rust-toolchain.toml CMakeLists.txt cmake include crates bindings examples scripts vendor docs/dependency-patches.md docs/prebuilt-sdk.md docs/prebuilt-sdk.en.md README.en.md .github)
    if ($LASTEXITCODE -ne 0) { throw 'Unable to enumerate source snapshot' }
    $sourceHashes = @($sourcePaths | Sort-Object -Unique | ForEach-Object {
        if (Test-Path -LiteralPath $_ -PathType Leaf) { "$_`t$((Get-FileHash -LiteralPath $_ -Algorithm SHA256).Hash.ToLowerInvariant())" }
    })
    $sourceBytes = [System.Text.Encoding]::UTF8.GetBytes(($sourceHashes -join "`n"))
    $sourceHash = [Convert]::ToHexString([System.Security.Cryptography.SHA256]::HashData($sourceBytes)).ToLowerInvariant()
    $files = @(Get-ChildItem -LiteralPath $destination -Recurse -File | Sort-Object FullName | ForEach-Object {
        @{ path = [System.IO.Path]::GetRelativePath($destination, $_.FullName).Replace([char]92, [char]47); bytes = $_.Length; sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant() }
    })
    $manifest = [ordered]@{
        schemaVersion = 1; version = $version; abiMajor = $abiMajor; abiMinor = $abiMinor; target = $Target; profile = $Profile; nativeIdentity = $nativeIdentity
        createdUtc = [DateTime]::UtcNow.ToString('o'); commit = $(if ($hasCommit) { $commit } else { $null })
        dirtySource = (-not $hasCommit -or $status.Count -ne 0); sourceSnapshotSha256 = $sourceHash
        sourceFiles = $sourceHashes; cargoLockSha256 = (Get-FileHash -LiteralPath 'Cargo.lock' -Algorithm SHA256).Hash.ToLowerInvariant()
        rustc = ((& rustc -Vv) -join "`n"); cargo = ((& cargo --version) -join '')
        acceptance = 'candidate-only; see evidence and platform sign-off; packaging does not imply hardware acceptance'
        files = $files
    }
    $manifest | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $destination 'manifest.json') -Encoding utf8NoBOM
    Write-Output $destination
} finally { Pop-Location }
