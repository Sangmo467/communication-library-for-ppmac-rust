[CmdletBinding()]
param(
    [ValidateRange(1, 1209600)][int]$DurationSeconds = 30,
    [ValidateSet(1,2,8,32)][int]$Threads = 1,
    [ValidateRange(1,255)][int]$PoolSize = 1,
    [ValidateRange(0.01,100000)][double]$Rate = 100,
    [ValidateRange(1,3600000)][int]$TimeoutMs = 5000,
    [string]$PythonExecutable = 'python',
    [string]$OutputPath = '',
    [switch]$SelfTest
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$previousPythonPath = $env:PYTHONPATH
Push-Location $root
try {
    $env:PYTHONPATH = Join-Path $root 'bindings/python'
    $arguments = @('-B', (Join-Path $PSScriptRoot 'soak.py'))
    if ($SelfTest) {
        & $PythonExecutable @arguments --self-test
        if ($LASTEXITCODE -ne 0) { throw 'Soak driver self-test failed' }
        return
    }
    if (-not $OutputPath) { $OutputPath = "build/reports/soak/$([DateTime]::UtcNow.ToString('yyyyMMddTHHmmssZ'))-$Threads-threads.jsonl" }
    if (-not [System.IO.Path]::IsPathRooted($OutputPath)) { $OutputPath = Join-Path $root $OutputPath }
    if (Test-Path -LiteralPath $OutputPath) { throw 'Refusing to append to an existing soak report; choose a new path' }
    New-Item -ItemType Directory -Path (Split-Path $OutputPath -Parent) -Force | Out-Null
    $arguments += @('--duration-seconds', $DurationSeconds, '--threads', $Threads, '--pool-size', $PoolSize,
                    '--rate', $Rate.ToString([Globalization.CultureInfo]::InvariantCulture), '--timeout-ms', $TimeoutMs)
    & $PythonExecutable @arguments | ForEach-Object {
        Add-Content -LiteralPath $OutputPath -Value $_ -Encoding utf8NoBOM
        Write-Output $_
    }
    $exitCode = $LASTEXITCODE
    if ($exitCode -ne 0) { throw "Soak driver exited with $exitCode; the report is not a pass: $OutputPath" }
    Write-Output "Report saved: $OutputPath"
} finally {
    $env:PYTHONPATH = $previousPythonPath
    Pop-Location
}
