[CmdletBinding()]
param([string]$OutputPath = 'build/abi_layout.c', [string]$RustOutputPath = '')
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$header = Get-Content -LiteralPath (Join-Path $root 'include/pmac.h') -Raw -Encoding UTF8
$header = [regex]::Replace($header, '/\*.*?\*/', '', [System.Text.RegularExpressions.RegexOptions]::Singleline)
$structs = [regex]::Matches($header, 'typedef\s+struct\s*\{(?<body>[^}]*)\}\s*(?<name>pmac_\w+)\s*;')
if ($structs.Count -eq 0) { throw 'No public C ABI structures found' }
$lines = [System.Collections.Generic.List[string]]::new()
@(
    '/* Generated from include/pmac.h by Update-AbiProbe.ps1. */',
    '#include "pmac.h"',
    '#include <stddef.h>',
    '#include <stdio.h>',
    '#define TYPE(T) printf(#T ".size=%u\n", (unsigned)sizeof(T)); printf(#T ".align=%u\n", (unsigned)_Alignof(T))',
    '#define FIELD(T, F) printf(#T "." #F "=%u\n", (unsigned)offsetof(T, F))',
    'int main(void) {',
    '    pmac_runtime_info info;',
    '    if (pmac_get_runtime_info(&info) != PMAC_OK) return 1;',
    '    if (info.abi_major != PMAC_ABI_MAJOR || info.abi_minor < PMAC_ABI_MINOR || info.pointer_width != sizeof(void *) * 8u) return 2;',
    '    printf("runtime.pointer_width=%u\n", info.pointer_width);'
) | ForEach-Object { $lines.Add($_) }
foreach ($struct in $structs) {
    $name = $struct.Groups['name'].Value
    $lines.Add("    TYPE($name);")
    foreach ($declaration in $struct.Groups['body'].Value.Split(';')) {
        if ([string]::IsNullOrWhiteSpace($declaration)) { continue }
        foreach ($fieldPart in $declaration.Split(',')) {
            $field = [regex]::Match($fieldPart, '(?<name>[A-Za-z_]\w*)\s*(?:\[[^\]]+\])?\s*$')
            if (-not $field.Success) { throw "Cannot parse public ABI declaration: $fieldPart" }
            $lines.Add("    FIELD($name, $($field.Groups['name'].Value));")
        }
    }
}
$lines.Add('    return 0;')
$lines.Add('}')
if (-not [System.IO.Path]::IsPathRooted($OutputPath)) { $OutputPath = Join-Path $root $OutputPath }
New-Item -ItemType Directory -Path (Split-Path $OutputPath -Parent) -Force | Out-Null
$lines | Set-Content -LiteralPath $OutputPath -Encoding utf8NoBOM
if ($RustOutputPath) {
    if (-not [System.IO.Path]::IsPathRooted($RustOutputPath)) { $RustOutputPath = Join-Path $root $RustOutputPath }
    $rust = [System.Collections.Generic.List[string]]::new()
    @(
        '// Generated from include/pmac.h by scripts/Update-AbiProbe.ps1.',
        '#![allow(dead_code)]',
        '#[path = "../src/types.rs"]',
        'mod types;',
        'use std::mem::{align_of, offset_of, size_of};',
        'use types::*;',
        'macro_rules! layout {',
        '    ($ty:ty, $name:literal; $($field:ident),* $(,)?) => {',
        '        println!(concat!($name, ".size={}"), size_of::<$ty>());',
        '        println!(concat!($name, ".align={}"), align_of::<$ty>());',
        '        $(println!(concat!($name, ".", stringify!($field), "={}"), offset_of!($ty, $field));)*',
        '    };',
        '}',
        'fn main() {',
        '    println!("runtime.pointer_width={}", usize::BITS);'
    ) | ForEach-Object { $rust.Add($_) }
    foreach ($struct in $structs) {
        $name = $struct.Groups['name'].Value
        $rustName = -join @($name.Split('_') | ForEach-Object { $_.Substring(0,1).ToUpperInvariant() + $_.Substring(1) })
        $fields = @($struct.Groups['body'].Value.Split(';') | Where-Object { -not [string]::IsNullOrWhiteSpace($_) } | ForEach-Object {
            $_.Split(',') | ForEach-Object { [regex]::Match($_, '(?<name>[A-Za-z_]\w*)\s*(?:\[[^\]]+\])?\s*$').Groups['name'].Value }
        })
        $rust.Add("    layout!($rustName, `"$name`"; $($fields -join ', '));")
    }
    $rust.Add('}')
    New-Item -ItemType Directory -Path (Split-Path $RustOutputPath -Parent) -Force | Out-Null
    $rust | Set-Content -LiteralPath $RustOutputPath -Encoding utf8NoBOM
    $formatted = $rust | & rustfmt --edition 2021 --config skip_children=true --emit stdout
    if ($LASTEXITCODE -ne 0) { throw 'Failed to format Rust ABI probe' }
    $formatted | Set-Content -LiteralPath $RustOutputPath -Encoding utf8NoBOM
}
