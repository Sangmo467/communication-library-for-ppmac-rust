# GetResponse Pattern Guide

[中文](pattern-migration.md) | [English](pattern-migration.en.md)

The GetResponse pattern parameter extracts result bytes from a completed request. Extraction does not resend the command.

## Compatible extraction modes

The following three exact, case-sensitive patterns preserve the old helper's actual behavior:

| Pattern | Behavior |
|---|---|
| Empty string | Returns the complete response bytes, allowing an empty response; no additional capture group |
| `number` | Extracts a numeric fragment starting at the first candidate digit, sign, or decimal point, preserving permissive exponent handling and the first-candidate failure rule |
| `variable` | Skips C-locale whitespace after the first equals sign, then takes bytes until the next whitespace or response end; no match if there is no equals sign or the value is empty |

For example, `number` extracts `1` from `P1=3.14`, extracts `1e+` from `value=1e+`, and finds no match in `prefix .broken 42`. These rules apply only to text extraction. Typed APIs such as motor position still perform strict numeric parsing and reject `1e+`, NaN, and Inf.

Whitespace for `variable` includes vertical tab and form feed. To use a reserved word as an ordinary regex literal, write `(?:number)` or `(?:variable)`.

## Byte regex

Other patterns execute as actual byte regexes using Rust `regex::bytes`. For example:

```text
^P[0-9]+=(.+)$
```

Capture group 1 extracts the value after the equals sign. No match returns a no-match result; an invalid expression returns `PMAC_PARSE_ERROR`. Lookaround and backreferences are not supported by this engine.

The C ABI uses `pmac_request_copy_match`, C++ uses `Request::match`, .NET uses `Request.Match`, and Python uses `Request.match`. The synchronous GetResponse `pattern` parameter in each binding is a convenience wrapper for the same request.
