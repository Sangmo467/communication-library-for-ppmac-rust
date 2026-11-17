# GetResponse 模式说明

[中文](pattern-migration.md) | [English](pattern-migration.en.md)

GetResponse 的模式参数用于从已完成请求中提取字节结果。提取不会重新发送命令。

## 兼容提取模式

以下三个精确模式保留旧 helper 的实际行为，区分大小写：

| 模式 | 行为 |
|---|---|
| 空字符串 | 返回完整响应字节，允许空响应；没有额外捕获组 |
| `number` | 从首个数字、正负号或小数点候选开始提取数字片段，保留宽松指数和首候选失败规则 |
| `variable` | 从首个等号后跳过 C locale 空白，取到下一个空白或响应末尾；无等号或空值时无匹配 |

例如，`number` 对 `P1=3.14` 得到 `1`，对 `value=1e+` 得到 `1e+`，对 `prefix .broken 42` 无匹配。这些规则只用于文本提取；电机位置等 typed API 仍执行严格数值解析，不接受 `1e+`、NaN 或 Inf。

`variable` 的空白包含垂直制表符和换页符。若要把保留字当作普通正则字面量，可写 `(?:number)` 或 `(?:variable)`。

## 字节正则

其他模式使用 Rust `regex::bytes` 执行真正的字节正则。例如：

```text
^P[0-9]+=(.+)$
```

配合捕获组 1 可提取等号后的值。无匹配返回无匹配结果，非法表达式返回 `PMAC_PARSE_ERROR`。环视和反向引用不属于该引擎支持的语法。

C ABI 使用 `pmac_request_copy_match`，C++ 使用 `Request::match`，.NET 使用 `Request.Match`，Python 使用 `Request.match`。各绑定同步 GetResponse 的 `pattern` 参数是同一请求的便捷封装。
