use crate::ProtocolError;
use regex::bytes::Regex;

pub fn response_text(response: &[u8]) -> Result<&str, ProtocolError> {
    std::str::from_utf8(response)
        .map(str::trim)
        .map_err(|_| ProtocolError::InvalidNumber("response is not UTF-8".into()))
}

fn value_text(response: &[u8]) -> Result<&str, ProtocolError> {
    let text = response_text(response)?;
    let value = if let Some((name, value)) = text.split_once('=') {
        if value.contains('=')
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_[].#&".contains(&byte))
        {
            return Err(ProtocolError::InvalidNumber(text.into()));
        }
        value.trim()
    } else {
        text
    };
    if value.is_empty() {
        Err(ProtocolError::InvalidNumber("empty response".into()))
    } else {
        Ok(value)
    }
}

pub fn parse_u64(response: &[u8]) -> Result<u64, ProtocolError> {
    let text = value_text(response)?;
    let (digits, radix) = text.strip_prefix('$').map_or((text, 10), |v| (v, 16));
    if digits.is_empty() || digits.starts_with(['+', '-']) {
        return Err(ProtocolError::InvalidNumber(text.into()));
    }
    u64::from_str_radix(digits, radix).map_err(|_| ProtocolError::InvalidNumber(text.into()))
}

pub fn parse_u32(response: &[u8]) -> Result<u32, ProtocolError> {
    let text = value_text(response)?;
    if text.starts_with(['+', '-']) {
        return Err(ProtocolError::InvalidNumber(text.into()));
    }
    text.parse::<u32>()
        .map_err(|_| ProtocolError::InvalidNumber(text.into()))
}

pub fn parse_f64(response: &[u8]) -> Result<f64, ProtocolError> {
    let text = value_text(response)?;
    let value = text
        .parse::<f64>()
        .map_err(|_| ProtocolError::InvalidNumber(text.into()))?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(ProtocolError::InvalidFloat)
    }
}

pub fn parse_bool(response: &[u8]) -> Result<bool, ProtocolError> {
    match value_text(response)? {
        "0" => Ok(false),
        "1" => Ok(true),
        other => Err(ProtocolError::InvalidNumber(other.into())),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegexMatch {
    pub matched: Vec<u8>,
    pub groups: Vec<Option<Vec<u8>>>,
}

// These exact tokens are the extraction modes in the frozen old C helper.
// They intentionally preserve byte extraction (including its permissive exponent),
// while the typed numeric parsers above remain strict.
fn legacy_number(response: &[u8]) -> Option<&[u8]> {
    let start = response
        .iter()
        .position(|byte| byte.is_ascii_digit() || matches!(byte, b'+' | b'-' | b'.'))?;
    let mut end = start;
    if matches!(response[end], b'+' | b'-') {
        end += 1;
    }
    let mut has_digits = false;
    while end < response.len() && response[end].is_ascii_digit() {
        has_digits = true;
        end += 1;
    }
    if response.get(end) == Some(&b'.') {
        end += 1;
        while end < response.len() && response[end].is_ascii_digit() {
            has_digits = true;
            end += 1;
        }
    }
    if matches!(response.get(end), Some(b'e' | b'E')) {
        end += 1;
        if matches!(response.get(end), Some(b'+' | b'-')) {
            end += 1;
        }
        while end < response.len() && response[end].is_ascii_digit() {
            end += 1;
        }
    }
    has_digits.then(|| &response[start..end])
}

fn legacy_space(byte: u8) -> bool {
    // C isspace in the C locale includes vertical tab, unlike Rust's byte helper.
    matches!(byte, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

fn legacy_variable(response: &[u8]) -> Option<&[u8]> {
    let mut start = response.iter().position(|byte| *byte == b'=')? + 1;
    while start < response.len() && legacy_space(response[start]) {
        start += 1;
    }
    let mut end = start;
    while end < response.len() && !legacy_space(response[end]) {
        end += 1;
    }
    (end > start).then(|| &response[start..end])
}
/// Extract only the requested capture, avoiding copies of every overlapping group.
pub fn regex_capture<'a>(
    response: &'a [u8],
    pattern: &str,
    group: usize,
) -> Result<Option<&'a [u8]>, ProtocolError> {
    let legacy = match pattern {
        "" => Some(Some(response)),
        "number" => Some(legacy_number(response)),
        "variable" => Some(legacy_variable(response)),
        _ => None,
    };
    if let Some(value) = legacy {
        return Ok(if group == 0 { value } else { None });
    }
    let regex = Regex::new(pattern).map_err(|error| ProtocolError::Regex(error.to_string()))?;
    Ok(regex
        .captures(response)
        .and_then(|captures| captures.get(group))
        .map(|value| value.as_bytes()))
}
pub fn regex_match(response: &[u8], pattern: &str) -> Result<Option<RegexMatch>, ProtocolError> {
    let extracted = match pattern {
        "" => Some(Some(response)),
        "number" => Some(legacy_number(response)),
        "variable" => Some(legacy_variable(response)),
        _ => None,
    };
    if let Some(matched) = extracted {
        return Ok(matched.map(|matched| RegexMatch {
            matched: matched.to_vec(),
            groups: Vec::new(),
        }));
    }
    let regex = Regex::new(pattern).map_err(|error| ProtocolError::Regex(error.to_string()))?;
    Ok(regex.captures(response).map(|captures| RegexMatch {
        matched: captures
            .get(0)
            .map_or_else(Vec::new, |value| value.as_bytes().to_vec()),
        groups: (1..captures.len())
            .map(|index| captures.get(index).map(|value| value.as_bytes().to_vec()))
            .collect(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unhex(value: &str) -> Vec<u8> {
        assert_eq!(value.len() % 2, 0);
        value
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }

    #[test]
    fn frozen_c_pattern_corpus_preserves_extraction_and_documents_new_regex_semantics() {
        let corpus = include_str!("../tests/legacy-pattern-corpus.tsv");
        let mut count = 0;
        for line in corpus
            .lines()
            .filter(|line| !line.starts_with('#') && !line.is_empty())
        {
            let fields: Vec<_> = line.split('\t').collect();
            assert_eq!(fields.len(), 7, "{}", line);
            let pattern_bytes = unhex(fields[1]);
            let pattern = std::str::from_utf8(&pattern_bytes).unwrap();
            let response = unhex(fields[2]);
            let result = regex_match(&response, pattern);
            match fields[5] {
                "match" => {
                    let matched = result.unwrap().unwrap();
                    assert_eq!(matched.matched, unhex(fields[6]), "{}", fields[0]);
                    if matches!(pattern, "" | "number" | "variable") {
                        assert_eq!(fields[3], "0", "{}", fields[0]);
                        assert_eq!(matched.matched, unhex(fields[4]), "{}", fields[0]);
                        assert!(matched.groups.is_empty(), "{}", fields[0]);
                    }
                }
                "no_match" => assert_eq!(result.unwrap(), None, "{}", fields[0]),
                "error" => assert!(
                    matches!(result, Err(ProtocolError::Regex(_))),
                    "{}",
                    fields[0]
                ),
                other => panic!("unknown expected pattern result: {other}"),
            }
            count += 1;
        }
        assert_eq!(count, 32);
    }

    #[test]
    fn legacy_extraction_does_not_relax_typed_numeric_parsing() {
        let matched = regex_match(b"value=1e+", "number").unwrap().unwrap();
        assert_eq!(matched.matched, b"1e+");
        assert!(parse_f64(&matched.matched).is_err());
        assert!(parse_f64(b"value=1e+").is_err());
        let literal = regex_match(b"number=3", "(?:number)").unwrap().unwrap();
        assert_eq!(literal.matched, b"number");
    }
    #[test]
    fn strict_numbers_do_not_hide_zero_or_errors() {
        assert_eq!(parse_u64(b"$FFFFFFFFFFFFFFFF\r\n"), Ok(u64::MAX));
        assert_eq!(
            parse_u64(b"18446744073709551616"),
            Err(ProtocolError::InvalidNumber("18446744073709551616".into()))
        );
        assert_eq!(parse_u32(b"Sys.ServoCount=0"), Ok(0));
        assert!(parse_u32(b"garbage=1=0").is_err());
        assert!(parse_u32(b"error line\nSys.ServoCount=0").is_err());
        assert_eq!(parse_f64(b"-0.0"), Ok(-0.0));
        assert!(parse_f64(b"NaN").is_err());
        assert!(parse_f64(b"1.0junk").is_err());
        assert_eq!(parse_bool(b"0"), Ok(false));
        assert!(parse_bool(b"false").is_err());
    }

    #[test]
    fn regex_preserves_bytes_and_groups() {
        let found = regex_match(b"P1=12\r\n", r"P1=(\d+)").unwrap().unwrap();
        assert_eq!(found.matched, b"P1=12");
        assert_eq!(found.groups, vec![Some(b"12".to_vec())]);
    }
}

#[cfg(test)]
mod capture_tests {
    use super::*;
    #[test]
    fn selected_capture_matches_legacy_and_nested_groups() {
        let input = b"value=123";
        for pattern in ["", "number", "variable", "((123))", "(missing)?(123)"] {
            let all = regex_match(input, pattern).unwrap();
            for group in 0..5 {
                let expected = all.as_ref().and_then(|m| {
                    if group == 0 {
                        Some(m.matched.as_slice())
                    } else {
                        m.groups.get(group - 1).and_then(|v| v.as_deref())
                    }
                });
                assert_eq!(regex_capture(input, pattern, group).unwrap(), expected);
            }
        }
    }
}
