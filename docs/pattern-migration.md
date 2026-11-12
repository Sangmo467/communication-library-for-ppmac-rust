# GetResponse 模式迁移说明

更新于2026-11-12。旧库1.0.40的 `PMAC_GetResponse` 实际调用 `src/pmac_comm_core.c:3619` 的 `simple_pattern_match_len`。该文件虽然包含 `<regex.h>`，此调用链并未调用 POSIX、PCRE 或 C++ 正则引擎。因此不能把新引擎不支持环视/反向引用误记为删除旧库已实现的功能。

## 保留的旧提取模式

| 精确模式字符串 | 旧行为与新行为 |
|---|---|
| 空字符串 | 返回完整响应字节，包含空响应；没有额外捕获组 |
| `number` | 从首个数字、正负号或小数点候选开始提取数字片段；保留旧 helper 的宽松指数及首候选失败行为 |
| `variable` | 从首个等号后跳过 C locale 空白，取到下一个空白或响应末尾；没有等号或值为空时无匹配 |

三种模式区分大小写，额外捕获组数均为0。为保持实际旧提取结果，`number` 对 `P1=3.14` 得到 `1`，对 `value=1e+` 得到 `1e+`，对 `prefix .broken 42` 无匹配。这些是兼容提取规则，并不放宽 typed 数字 API；`parse_f64` 仍拒绝 `1e+` 等不完整数值。`variable` 的空白包含垂直制表符与换页符。

## 其他模式的新语义

旧 helper 对其他任意模式直接返回原始响应，没有解释表达式。新 API 则使用 `regex::bytes` 真正执行字节正则并提供捕获组：例如 `^P[0-9]+=(.+)$` 可以匹配整行并提取值；不匹配返回无匹配，非法表达式返回错误。环视与反向引用不属于该引擎支持的语法，不再被静默忽略。

模式恰为保留字时仍按上述规则提取；匹配字面量 `number` 可以写 `(?:number)`。此项是明确的新 API 语义，没有承诺把旧版本任意模式的原样返回行为继续隐藏为成功匹配。

## 冻结与回归证据

- 旧提交：`0ec71531d9ce1b1d42851e54e441b90d8b05d557`；旧 C 文件 SHA-256：`9054ED5647642D9AD04F8EC762C24F5F615935003D786CFE8E717B4A761D869E`。完整工作树与公开符号来源见[逐符号矩阵](legacy-symbol-matrix.md)。
- [32例 corpus](../crates/pmac-protocol/tests/legacy-pattern-corpus.tsv) 来自编译未改动的旧 `simple_pattern_match_len` 及三个字符/长度 helper，然后在 C locale 执行人工构造的边界输入。字段均以十六进制保存；没有复制设备变量、生产表达式或用户业务文件。
- [Rust 回归](../crates/pmac-protocol/src/parse.rs) 对照旧 helper 的返回值/结果，并显式登记其他模式的新正则匹配、无匹配和语法错误。另一个测试验证兼容提取不影响 typed 严格数值解析。
- [生成脚本](../scripts/Update-LegacyPatternCorpus.ps1) 先验证冻结源哈希，只读旧源码，所有生成物写入本工作区。编译器需要支持 C99。重建只用于审阅冻结样本，不是日常构建对旧仓库的依赖。

```powershell
./scripts/Update-LegacyPatternCorpus.ps1 -LegacySource E:/PMAC/comm/src/pmac_comm_core.c -Compiler gcc
cargo test -p pmac-protocol --locked
```

本轮使用 GCC 6.4.0 实际生成32例，Rust protocol 的40项测试、格式检查及Clippy通过。没有为此增加第二个正则引擎。
