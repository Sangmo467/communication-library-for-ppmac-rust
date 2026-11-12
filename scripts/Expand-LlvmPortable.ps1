[CmdletBinding()]
param([Parameter(Mandatory)][string]$Archive, [Parameter(Mandatory)][string]$ExpectedSha256, [string]$OutputDirectory='build/tools/llvm-23.1.5', [string]$SevenZip='C:/Program Files/7-Zip/7z.exe')
$ErrorActionPreference='Stop'
$root=Split-Path $PSScriptRoot -Parent
$archivePath=(Resolve-Path -LiteralPath $Archive).Path
if ((Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash -ne $ExpectedSha256) { throw 'LLVM archive SHA256 mismatch' }
$outputPath=[IO.Path]::GetFullPath((Join-Path $root $OutputDirectory))
$workspacePrefix=$root.TrimEnd([char]92,[char]47)+[IO.Path]::DirectorySeparatorChar
if (-not $outputPath.StartsWith($workspacePrefix,[StringComparison]::OrdinalIgnoreCase)) { throw 'Portable LLVM output must remain inside the workspace' }
$outputPrefix=$outputPath.TrimEnd([char]92,[char]47)+[IO.Path]::DirectorySeparatorChar
New-Item -ItemType Directory -Path $outputPath -Force | Out-Null
$info=[Diagnostics.ProcessStartInfo]::new()
$info.FileName=$SevenZip
$info.ArgumentList.Add('x')
$info.ArgumentList.Add('-so')
$info.ArgumentList.Add($archivePath)
$info.UseShellExecute=$false
$info.CreateNoWindow=$true
$info.RedirectStandardOutput=$true
$info.RedirectStandardError=$true
$process=[Diagnostics.Process]::Start($info)
$errors=$process.StandardError.ReadToEndAsync()
$reader=[System.Formats.Tar.TarReader]::new($process.StandardOutput.BaseStream)
$aliases=[System.Collections.Generic.List[object]]::new()
$copied=0
try {
    while ($null -ne ($entry=$reader.GetNextEntry())) {
        $separator=$entry.Name.IndexOf('/')
        if ($separator -lt 0) { continue }
        $relative=$entry.Name.Substring($separator+1)
        $selected=$relative -match '^bin/(clang(?:-cl|\+\+|-\d+)?|lld(?:-link)?|llvm-(?:ar|lib|readobj|objdump|objcopy))\.exe$|^bin/[^/]+\.dll$|^lib/clang/|^LICENSE(?:\.TXT)?$'
        if (-not $selected -or $entry.EntryType -eq [System.Formats.Tar.TarEntryType]::Directory) { continue }
        $destination=[IO.Path]::GetFullPath((Join-Path $outputPath $relative))
        if (-not $destination.StartsWith($outputPrefix,[StringComparison]::OrdinalIgnoreCase)) { throw 'LLVM archive entry escapes output directory' }
        New-Item -ItemType Directory -Path (Split-Path $destination -Parent) -Force | Out-Null
        if ($null -ne $entry.DataStream) {
            $target=[IO.File]::Open($destination,[IO.FileMode]::Create,[IO.FileAccess]::Write)
            try { $entry.DataStream.CopyTo($target,81920) } finally { $target.Dispose() }
            $copied++
        } elseif ($entry.EntryType -in @([System.Formats.Tar.TarEntryType]::HardLink,[System.Formats.Tar.TarEntryType]::SymbolicLink)) {
            $aliases.Add(@{destination=$destination;link=$entry.LinkName;relative=$relative})
        } else { throw "Unsupported selected LLVM entry: $relative" }
    }
    if (-not $process.WaitForExit(15000)) { throw 'LLVM decompressor did not exit after archive EOF' }
    if ($process.ExitCode -ne 0) { throw "LLVM decompression failed: $($errors.GetAwaiter().GetResult())" }
    foreach ($alias in $aliases) {
        $link=$alias.link
        if ($link.Contains('/')) { $link=$link.Substring($link.IndexOf('/')+1) } else { $link=Join-Path (Split-Path $alias.relative -Parent) $link }
        $source=[IO.Path]::GetFullPath((Join-Path $outputPath $link))
        if (-not $source.StartsWith($outputPrefix,[StringComparison]::OrdinalIgnoreCase)) { throw 'LLVM link escapes output directory' }
        Copy-Item -LiteralPath $source -Destination $alias.destination -Force
        $copied++
    }
    if (-not (Test-Path -LiteralPath (Join-Path $outputPath 'bin/clang-cl.exe'))) { throw 'LLVM archive did not provide clang-cl' }
    [pscustomobject]@{outputDirectory=$outputPath;filesExtracted=$copied;archiveSha256=$ExpectedSha256}
} finally {
    $reader.Dispose()
    if (-not $process.HasExited) { $process.Kill($true); $process.WaitForExit() }
    $process.Dispose()
}
