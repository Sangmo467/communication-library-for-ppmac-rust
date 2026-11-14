param(
    [Parameter(Mandatory=$true)][string]$LegacySource,
    [string]$Compiler='gcc'
)
$ErrorActionPreference='Stop'
$repoRoot=Split-Path -Parent $PSScriptRoot
$expectedHash='9054ED5647642D9AD04F8EC762C24F5F615935003D786CFE8E717B4A761D869E'
if((Get-FileHash -LiteralPath $LegacySource -Algorithm SHA256).Hash -ne $expectedHash){throw 'Legacy source differs from the reviewed frozen oracle'}
$source=Get-Content -LiteralPath $LegacySource -Raw -Encoding UTF8
function Extract-CFunction([string]$signature) {
    $start=$source.IndexOf($signature)
    if($start -lt 0){throw "Missing old function: $signature"}
    $open=$source.IndexOf('{',$start);$depth=0
    for($index=$open;$index -lt $source.Length;$index++){
        if($source[$index] -eq '{'){$depth++}elseif($source[$index] -eq '}'){$depth--;if($depth -eq 0){return $source.Substring($start,$index-$start+1)}}
    }
    throw 'Unbalanced old helper'
}
$cases=[System.Collections.Generic.List[object]]::new()
function Case([string]$id,[string]$pattern,[string]$inputValue,[string]$newKind,[string]$newValue='') {
    $cases.Add([pscustomobject]@{id=$id;pattern=$pattern;bytes=[Text.Encoding]::UTF8.GetBytes($inputValue);kind=$newKind;value=$newValue})
}
Case 'empty_raw' '' "P1=3.14`r`n" 'legacy'
Case 'empty_input' '' '' 'legacy'
Case 'number_decimal' 'number' 'value=3.14' 'legacy'
Case 'number_variable_name' 'number' 'P1=3.14' 'legacy'
Case 'number_signed_fraction' 'number' 'value=-.5e+2 tail' 'legacy'
Case 'number_leading_dot' 'number' 'prefix .25 suffix' 'legacy'
Case 'number_trailing_dot' 'number' 'value=12. rest' 'legacy'
Case 'number_loose_exponent_sign' 'number' 'value=1e+' 'legacy'
Case 'number_loose_exponent_text' 'number' 'value=2Efoo' 'legacy'
Case 'number_first_candidate_invalid' 'number' 'prefix .broken 42' 'legacy'
Case 'number_not_found' 'number' 'plain text' 'legacy'
Case 'number_sign_only' 'number' 'prefix + rest 4' 'legacy'
Case 'number_double_sign' 'number' 'value=--4' 'legacy'
Case 'number_zero' 'number' '0' 'legacy'
Case 'number_comma' 'number' 'value=1,25' 'legacy'
Case 'number_exponent_without_digits' 'number' 'value=.e9 later 3' 'legacy'
Case 'variable_value' 'variable' 'P1=3.14 rest' 'legacy'
Case 'variable_spaces' 'variable' " P =`t `r`n-.5e2`r`n" 'legacy'
Case 'variable_vertical_tab' 'variable' ("value="+[char]11+"42"+[char]11+"more") 'legacy'
Case 'variable_form_feed' 'variable' ("value="+[char]12+"42`tmore") 'legacy'
Case 'variable_no_equal' 'variable' '42' 'legacy'
Case 'variable_empty_value' 'variable' "value=`t `r`n" 'legacy'
Case 'variable_embedded_equal' 'variable' 'A=text=value rest' 'legacy'
Case 'variable_hex' 'variable' 'A=$FF' 'legacy'
Case 'unknown_pattern' 'literal' 'abc12' 'no_match'
Case 'new_regex_capture' '(\d+)' 'P1=12' 'match' '1'
Case 'lookahead_was_not_an_engine' '(?=a)a' 'ab' 'error'
Case 'backref_was_not_an_engine' '(a)\1' 'aa' 'error'
Case 'new_regex_anchored' '^P[0-9]+=(.+)$' 'P1=12' 'match' 'P1=12'
Case 'escaped_reserved_word' '(?:number)' 'number=3' 'match' 'number'
Case 'reserved_word_case_sensitive' 'Variable' 'Variable=1' 'match' 'Variable'
$cases.Add([pscustomobject]@{id='empty_binary';pattern='';bytes=[byte[]]@(255,49,65);kind='legacy';value=''})
$probe=[Text.StringBuilder]::new()
[void]$probe.AppendLine('#include <stdio.h>')
[void]$probe.AppendLine('#include <stdlib.h>')
[void]$probe.AppendLine('#include <string.h>')
[void]$probe.AppendLine('#include <ctype.h>')
[void]$probe.AppendLine('#include <locale.h>')
[void]$probe.AppendLine('typedef int ssh_shell_error_t;')
[void]$probe.AppendLine('enum { SSH_SHELL_SUCCESS=0, SSH_SHELL_ERROR_INIT=-1, SSH_SHELL_ERROR_REGEX=-13 };')
[void]$probe.AppendLine('typedef struct {char* matched_string;char** groups;int group_count;} ssh_regex_result_t;')
foreach($signature in @('static size_t pmac_span_size(','static inline int pmac_char_is_digit(','static inline int pmac_char_is_space(','static ssh_shell_error_t simple_pattern_match_len(')){
    [void]$probe.AppendLine((Extract-CFunction $signature))
}
[void]$probe.AppendLine('static void hex(const char* text) {if(text) for(size_t i=0;text[i];i++)printf("%02x",(unsigned char)text[i]);}')
[void]$probe.AppendLine('int main(void) { (void)setlocale(LC_ALL,"C");')
for($index=0;$index -lt $cases.Count;$index++){
    $case=$cases[$index];$pattern=[Text.Encoding]::UTF8.GetBytes($case.pattern)
    $inputInitializers=(@($case.bytes|ForEach-Object{"(char)$_"})+@('0'))-join ','
    $patternInitializers=(@($pattern|ForEach-Object{[string]$_})+@('0'))-join ','
    [void]$probe.AppendLine(('{{char input[]={{ {0} }};char pattern[]={{ {1} }};ssh_regex_result_t out={{0}};int rc=simple_pattern_match_len(input,{2},pattern,&out);printf("{3}\t%d\t",rc);hex(out.matched_string);printf("\t%d\n",out.group_count);free(out.matched_string);}}' -f $inputInitializers,$patternInitializers,$case.bytes.Length,$index))
}
[void]$probe.AppendLine('return 0;}')
New-Item -ItemType Directory -Path (Join-Path $repoRoot 'build') -Force|Out-Null
$probePath=Join-Path $repoRoot 'build/legacy-pattern-oracle.c';$content=$probe.ToString()
$oraclePath=Join-Path $repoRoot 'build/legacy-pattern-oracle.exe'
Set-Content -LiteralPath $probePath -Value '' -NoNewline -Encoding utf8NoBOM
for($offset=0;$offset -lt $content.Length;$offset+=5000){Add-Content -LiteralPath $probePath -Value $content.Substring($offset,[Math]::Min(5000,$content.Length-$offset)) -NoNewline -Encoding utf8NoBOM}
& $Compiler -std=c99 -Wall -Wextra -Werror $probePath -o $oraclePath
if($LASTEXITCODE -ne 0){throw 'Old pattern oracle build failed'}
$oracle=@(& $oraclePath)
if($LASTEXITCODE -ne 0 -or $oracle.Count -ne $cases.Count){throw 'Old pattern oracle failed'}
$lines=[Collections.Generic.List[string]]::new()
$lines.Add('# Frozen old helper: E:/PMAC/comm/src/pmac_comm_core.c:3619; source SHA256 9054ED5647642D9AD04F8EC762C24F5F615935003D786CFE8E717B4A761D869E')
$lines.Add('# Oracle compiled unchanged helper and three unchanged character/span helpers; C locale; synthetic inputs only.')
$lines.Add("# id`tpattern_hex`tinput_hex`told_status`told_match_hex`tnew_kind`tnew_match_hex")
for($index=0;$index -lt $cases.Count;$index++){
    $case=$cases[$index];$old=$oracle[$index].Split("`t")
    if([int]$old[0] -ne $index -or [int]$old[3] -ne 0){throw 'Unexpected legacy capture groups or case order'}
    $kind=$case.kind;$valueHex=[Convert]::ToHexString([Text.Encoding]::UTF8.GetBytes($case.value)).ToLowerInvariant()
    if($kind -eq 'legacy'){$kind=if([int]$old[1] -eq 0){'match'}else{'no_match'};$valueHex=$old[2]}
    $patternHex=[Convert]::ToHexString([Text.Encoding]::UTF8.GetBytes($case.pattern)).ToLowerInvariant()
    $inputHex=[Convert]::ToHexString($case.bytes).ToLowerInvariant()
    $lines.Add((@($case.id,$patternHex,$inputHex,$old[1],$old[2],$kind,$valueHex)-join "`t"))
}
New-Item -ItemType Directory -Path (Join-Path $repoRoot 'crates/pmac-protocol/tests') -Force|Out-Null
Set-Content -LiteralPath (Join-Path $repoRoot 'crates/pmac-protocol/tests/legacy-pattern-corpus.tsv') -Value $lines -Encoding utf8NoBOM
"$($cases.Count) unchanged-C-oracle cases captured"
