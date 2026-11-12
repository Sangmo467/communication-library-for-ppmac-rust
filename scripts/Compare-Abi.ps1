[CmdletBinding()]
param([Parameter(Mandatory)][string]$CLayoutPath, [Parameter(Mandatory)][string]$RustLayoutPath)
$ErrorActionPreference = 'Stop'
function Read-Layout([string]$Path) {
    $result = [System.Collections.Generic.Dictionary[string,long]]::new([StringComparer]::Ordinal)
    foreach ($line in Get-Content -LiteralPath $Path -Encoding UTF8) {
        if ($line -match '^(?:\d+:\s*)?(?<key>(?:pmac_\w+\.\w+|runtime\.pointer_width))=(?<value>\d+)\s*$') {
            if ($result.ContainsKey($Matches.key)) { throw "Duplicate ABI layout key: $($Matches.key)" }
            $result.Add($Matches.key, [long]$Matches.value)
        }
    }
    if ($result.Count -lt 10) { throw "ABI report is missing field layouts: $Path" }
    return ,$result
}
$c = Read-Layout $CLayoutPath
$rust = Read-Layout $RustLayoutPath
$differences = [System.Collections.Generic.List[string]]::new()
foreach ($key in $c.Keys) {
    if (-not $rust.ContainsKey($key)) { $differences.Add("Rust report lacks $key") }
    elseif ($c[$key] -ne $rust[$key]) { $differences.Add("$key differs: C=$($c[$key]), Rust=$($rust[$key])") }
}
foreach ($key in $rust.Keys) { if (-not $c.ContainsKey($key)) { $differences.Add("C report lacks $key") } }
if ($differences.Count) { throw ($differences -join "`n") }
Write-Output "ABI size/align/offset parity passed: $($c.Count) values, $($c['runtime.pointer_width'])-bit"
