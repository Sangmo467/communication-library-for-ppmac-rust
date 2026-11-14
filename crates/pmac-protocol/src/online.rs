use crate::{CommandEffect, CommandScope, MaintenanceKind, ProtocolError};
use regex::{Regex, RegexBuilder};
use std::sync::OnceLock;

type Classification = (CommandEffect, CommandScope, MaintenanceKind);
struct Rule {
    pattern: Regex,
    scope: &'static str,
    effect: &'static str,
}
fn rules() -> &'static [Rule] {
    static RULES: OnceLock<Vec<Rule>> = OnceLock::new();
    RULES.get_or_init(|| {
        include_str!("online-command-rules.tsv")
            .lines()
            .skip(1)
            .filter(|line| !line.is_empty())
            .map(|line| {
                let mut columns = line.split('\t');
                let pattern = columns.next().expect("pattern");
                Rule {
                    pattern: RegexBuilder::new(pattern)
                        .case_insensitive(true)
                        .build()
                        .expect("manual command regex"),
                    scope: columns.next().expect("scope"),
                    effect: columns.next().expect("effect"),
                }
            })
            .collect()
    })
}

fn selected_target(segment: &str) -> Result<Option<(u8, &str, &str, CommandScope)>, ProtocolError> {
    let Some(prefix) = segment
        .as_bytes()
        .first()
        .copied()
        .filter(|b| matches!(b, b'#' | b'&'))
    else {
        return Ok(None);
    };
    if segment.len() == 1 {
        return Ok(None);
    }
    let count = segment[1..]
        .bytes()
        .take_while(|b| b.is_ascii_digit() || matches!(b, b',' | b'.' | b'*'))
        .count();
    if count == 0 {
        return Err(ProtocolError::AmbiguousTarget(segment.into()));
    }
    let selector = &segment[1..1 + count];
    let maximum = if prefix == b'#' { 255 } else { 127 };
    if selector != "*" {
        for item in selector.split(',') {
            let pieces: Vec<_> = item.split("..").collect();
            if pieces.len() > 2 {
                return Err(ProtocolError::AmbiguousTarget(segment.into()));
            }
            let mut previous = None;
            for part in pieces {
                let value = part
                    .parse::<u32>()
                    .map_err(|_| ProtocolError::AmbiguousTarget(segment.into()))?;
                if value > maximum || previous.is_some_and(|start| start > value) {
                    return Err(ProtocolError::AmbiguousTarget(segment.into()));
                }
                previous = Some(value);
            }
        }
    }
    let scope = match selector.parse::<u8>() {
        Ok(n) if prefix == b'#' => CommandScope::Motor(n),
        Ok(n) => CommandScope::Coord(n),
        Err(_) => CommandScope::Global,
    };
    Ok(Some((prefix, selector, segment[1 + count..].trim(), scope)))
}

pub(crate) fn classify(segment: &str) -> Result<Option<Classification>, ProtocolError> {
    let selected = selected_target(segment)?;
    if let Some((_, selector, "", _)) = selected {
        if selector.parse::<u8>().is_ok() {
            return Ok(Some((
                CommandEffect::Control,
                CommandScope::System,
                MaintenanceKind::None,
            )));
        }
        return Err(ProtocolError::UnsupportedCommand(segment.into()));
    }
    let operation = selected.map_or(segment, |(_, _, op, _)| op);
    let lower = operation.to_ascii_lowercase();
    for (root, maximum) in [("motor[", 255), ("coord[", 127), ("plc[", 31)] {
        if let Some(rest) = lower.strip_prefix(root) {
            if let Some((index, _)) = rest.split_once(']') {
                if let Ok(value) = index.parse::<i32>() {
                    if !(0..=maximum).contains(&value) {
                        return Err(ProtocolError::AmbiguousTarget(segment.into()));
                    }
                }
            }
        }
    }
    static NUMERIC_INDEX: OnceLock<Regex> = OnceLock::new();
    let indices = NUMERIC_INDEX.get_or_init(|| Regex::new(r"\[([0-9]+)\]").expect("index regex"));
    static VARIABLE_QUERY: OnceLock<Regex> = OnceLock::new();
    let query = VARIABLE_QUERY.get_or_init(|| {
        Regex::new(r"^[dilmpqr][0-9]+(?:(?:\.\.[0-9]+)|(?:,[0-9]+){1,2})?(?::[0-9]+)?(?:->)?$")
            .expect("variable query regex")
    });
    for rule in rules() {
        if !rule.pattern.is_match(operation) {
            continue;
        }
        let valid_scope = match rule.scope {
            "Motor" => selected.is_some_and(|(prefix, _, _, _)| prefix == b'#'),
            "Coord" => selected.is_some_and(|(prefix, _, _, _)| prefix == b'&'),
            "Either" => selected.is_some(),
            "Global" => selected.is_none() || operation == "?",
            "Thread" => selected.is_none(),
            _ => unreachable!("manual scope"),
        };
        if !valid_scope {
            return Err(ProtocolError::AmbiguousTarget(segment.into()));
        }
        if (lower.starts_with("jog=") || lower.starts_with("jog:"))
            && lower[4..].parse::<f64>().is_ok_and(|n| !n.is_finite())
        {
            return Err(ProtocolError::InvalidFloat);
        }
        let scope = selected.map_or(
            if rule.effect == "Maintenance" {
                CommandScope::Global
            } else {
                CommandScope::System
            },
            |(_, _, _, scope)| scope,
        );
        let mut effect = match rule.effect {
            "Read" => CommandEffect::ReadOnly,
            "Write" => CommandEffect::Control,
            "Maintenance" => CommandEffect::Maintenance,
            _ => unreachable!("manual effect"),
        };
        // Keep the existing bounded read-pool policy for plain P ranges. Other
        // supported queries remain subject to the per-request response budget.
        if effect == CommandEffect::ReadOnly && lower.starts_with('p') {
            if let Some((first, last)) = lower[1..].split_once("..") {
                let last = last.split(':').next().unwrap_or(last);
                if let (Ok(first), Ok(last)) = (first.parse::<u32>(), last.parse::<u32>()) {
                    if !last
                        .checked_sub(first)
                        .and_then(|n| n.checked_add(1))
                        .is_some_and(|n| n <= 8192)
                    {
                        effect = CommandEffect::Control;
                    }
                }
            }
        }
        if effect == CommandEffect::ReadOnly {
            if indices
                .captures_iter(&lower)
                .any(|c| c[1].parse::<u32>().is_err())
            {
                effect = CommandEffect::Control;
            }
            if lower
                .as_bytes()
                .first()
                .is_some_and(|b| b"dilmpqr".contains(b))
                && lower.as_bytes().get(1).is_some_and(u8::is_ascii_digit)
                && (!query.is_match(&lower)
                    || lower
                        .split(|c: char| !c.is_ascii_digit())
                        .filter(|s| !s.is_empty())
                        .any(|n| n.parse::<u32>().is_err()))
            {
                effect = CommandEffect::Control;
            }
        }
        let maintenance = if effect != CommandEffect::Maintenance {
            MaintenanceKind::None
        } else if lower == "$$$***" {
            MaintenanceKind::GlobalReset
        } else if matches!(lower.as_str(), "$$$" | "reboot") {
            MaintenanceKind::Reset
        } else if matches!(lower.as_str(), "save" | "fsave") {
            MaintenanceKind::Save
        } else if lower.starts_with("ecat ") {
            MaintenanceKind::EtherCat
        } else {
            MaintenanceKind::Device
        };
        return Ok(Some((effect, scope, maintenance)));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Command;

    #[test]
    fn manual_rule_samples_are_supported_case_insensitively() {
        for line in include_str!("online-command-rules.tsv").lines().skip(1) {
            let columns: Vec<_> = line.split('\t').collect();
            let sample = columns[3];
            let expected = match columns[2] {
                "Read" => CommandEffect::ReadOnly,
                "Write" => CommandEffect::Control,
                "Maintenance" => CommandEffect::Maintenance,
                _ => unreachable!(),
            };
            for text in [sample.to_ascii_lowercase(), sample.to_ascii_uppercase()] {
                let command = Command::raw(&text)
                    .unwrap_or_else(|e| panic!("manual line {} / {text}: {e}", columns[4]));
                assert_eq!(command.effect(), expected, "{text}");
                assert_eq!(command.text(), text);
            }
        }
    }

    #[test]
    fn writes_and_dynamic_indices_never_masquerade_as_queries() {
        for text in [
            "&1Q1++",
            "&1Q(P1++)",
            "P1++",
            "Motor[1].ActPos++",
            "Motor[P1++].ActPos",
            "Sys.ServoCount++",
            "mxread0,M1,P1",
        ] {
            if let Ok(command) = Command::raw(text) {
                assert_ne!(command.effect(), CommandEffect::ReadOnly, "{text}");
            }
        }
    }
    #[test]
    fn data_structure_namespaces_accept_members_and_classify_updates() {
        for member in [
            "Sys.ServoCount",
            "EncTable[1].type",
            "Motor[1].ActPos",
            "ECAT[0].Enable",
            "Gate2[0].Chan[1].Dac[0]",
            "Gate3[0].Chan[1].OutCtrl",
            "Ldata.Motor",
            "Macro.TestValue",
            "Gate1[0].Chan[1].Dac[0].a",
            "Eip[0].TestValue",
            "Coord[1].InPos",
            "BufIO[0].TestValue",
        ] {
            for reference in [member.to_ascii_lowercase(), member.to_ascii_uppercase()] {
                let query = Command::raw(&reference).unwrap();
                assert_eq!(query.effect(), CommandEffect::ReadOnly, "{reference}");
                for operator in [
                    "=", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<=", ">>=",
                ] {
                    let text = format!("{reference} {operator} P1+5");
                    let command = Command::raw(&text).unwrap();
                    assert_eq!(command.effect(), CommandEffect::Control, "{text}");
                    assert_eq!(command.text(), text);
                }
            }
        }
        for text in [
            "Gate3[P1].Chan[Q1].TestValue",
            "Sys.Ddata[L0]",
            "Motor[n].ActPos",
        ] {
            assert_eq!(Command::raw(text).unwrap().effect(), CommandEffect::Control);
        }
        let query = Command::raw("Sys.ServoCount;Gate3[0].TestValue;BufIO[0].TestValue").unwrap();
        assert_eq!(query.effect(), CommandEffect::ReadOnly);
        assert_eq!(
            Command::raw("Sys.ServoCount;Gate3[0].TestValue=1")
                .unwrap()
                .effect(),
            CommandEffect::Control
        );
    }
    #[test]
    fn additional_structure_roots_are_not_restricted_by_a_prefix_list() {
        for member in [
            "Gather.Enable",
            "Tdata[0].TestValue",
            "UserAlgo.TestValue",
            "MyUserStructure[2].Member",
        ] {
            let query = Command::raw(member).unwrap();
            assert_eq!(query.effect(), CommandEffect::ReadOnly);
            let assignment = format!("{member}=P1+5");
            assert_eq!(
                Command::raw(&assignment).unwrap().effect(),
                CommandEffect::Control
            );
        }
    }
}
