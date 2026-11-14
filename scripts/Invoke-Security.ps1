[CmdletBinding()]
param([string]$OutputDirectory = 'build/reports/security', [switch]$SkipAudit, [switch]$SkipDeny)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Push-Location $root
try {
    New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
    $checks = [System.Collections.Generic.List[object]]::new()
    foreach ($check in @(
        @{ name = 'audit'; skip = [bool]$SkipAudit; arguments = @('audit','--db','build/security-audit-db','--json') },
        @{ name = 'deny'; skip = [bool]$SkipDeny; arguments = @('deny','--config','scripts/deny.toml','check') }
    )) {
        if ($check.skip) { $checks.Add(@{ name = $check.name; status = 'explicitly-skipped' }); continue }
        $version = (& cargo $check.name --version 2>&1) -join "`n"
        if ($LASTEXITCODE -ne 0) { $checks.Add(@{ name = $check.name; status = 'missing'; exitCode = -1 }); continue }
        $output = & cargo @($check.arguments) 2>&1
        $exitCode = $LASTEXITCODE
        $output | Set-Content -LiteralPath (Join-Path $OutputDirectory "$($check.name).log") -Encoding utf8NoBOM
        if ($check.name -eq 'audit') {
            foreach ($line in $output) {
                if (([string]$line).TrimStart().StartsWith('{')) {
                    try {
                        $audit = ([string]$line) | ConvertFrom-Json
                        if ($audit.database -and $audit.vulnerabilities) {
                            $audit | ConvertTo-Json -Depth 30 | Set-Content -LiteralPath (Join-Path $OutputDirectory 'audit.json') -Encoding utf8NoBOM
                        }
                    } catch { }
                }
            }
        }
        $checks.Add(@{ name = $check.name; version = $version; status = $(if ($exitCode -eq 0) { 'passed' } else { 'failed' }); exitCode = $exitCode })
    }
    $report = @{ createdUtc = [DateTime]::UtcNow.ToString('o'); cargoLockSha256 = (Get-FileHash -LiteralPath 'Cargo.lock' -Algorithm SHA256).Hash.ToLowerInvariant(); checks = $checks.ToArray() }
    $report | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $OutputDirectory 'security.json') -Encoding utf8NoBOM
    if (@($checks | Where-Object { $_.status -in @('failed','missing') }).Count -ne 0) { throw 'Dependency security gate failed; inspect saved logs' }
    if (@($checks | Where-Object status -eq 'explicitly-skipped').Count -ne 0) { Write-Warning 'Security checks were explicitly skipped; report is not a full security sign-off' }
} finally { Pop-Location }
