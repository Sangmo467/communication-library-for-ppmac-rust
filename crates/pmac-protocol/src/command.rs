use crate::ProtocolError;

pub const MAX_COMMAND_LEN: usize = 1023;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CommandEffect {
    ReadOnly,
    Control,
    Maintenance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandScope {
    Global,
    System,
    Motor(u8),
    Coord(u8),
    Plc(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaintenanceKind {
    None,
    Save,
    Reset,
    GlobalReset,
    EtherCat,
    BufferLoad,
    Device,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    text: String,
    effect: CommandEffect,
    scope: CommandScope,
    maintenance: MaintenanceKind,
    program_line: bool,
}

impl Command {
    pub fn raw(text: impl Into<String>) -> Result<Self, ProtocolError> {
        let text = text.into();
        validate_text(&text)?;
        let mut effect = CommandEffect::ReadOnly;
        let mut scope = CommandScope::System;
        let mut maintenance = MaintenanceKind::None;

        for segment in text.split(';') {
            let segment = segment.trim();
            if segment.is_empty() {
                return Err(ProtocolError::EmptyCommand);
            }
            let (segment_effect, segment_scope, segment_maintenance) = classify_segment(segment)?;
            effect = effect.max(segment_effect);
            scope = merge_scopes(scope, segment_scope);
            maintenance = merge_maintenance(maintenance, segment_maintenance);
            let lower = segment.to_ascii_lowercase();
            let coordinate_operation = lower.strip_prefix('&').map(|rest| {
                rest.trim_start_matches(|c: char| {
                    c.is_ascii_digit() || matches!(c, ',' | '.' | '*')
                })
                .trim_start()
            });
            if coordinate_operation.is_some_and(|op| {
                ["cpx", "cx"].iter().any(|prefix| {
                    op.strip_prefix(prefix)
                        .is_some_and(|tail| tail.starts_with(char::is_whitespace))
                })
            }) {
                // The rest of the line is opaque program/shell text, including semicolons.
                break;
            }
        }

        Ok(Self {
            text,
            effect,
            scope,
            maintenance,
            program_line: false,
        })
    }

    /// A single buffered-program line. The caller must verify an open buffer on
    /// the same channel before sending it; it must never enter a read-only lane.
    pub fn program_line(text: impl Into<String>) -> Result<Self, ProtocolError> {
        let text = text.into();
        validate_text(&text)?;
        Ok(Self {
            text,
            effect: CommandEffect::Control,
            scope: CommandScope::Global,
            maintenance: MaintenanceKind::None,
            program_line: true,
        })
    }

    pub fn is_program_line(&self) -> bool {
        self.program_line
    }

    pub fn motor(
        motor: i32,
        operation: &str,
        effect: CommandEffect,
    ) -> Result<Self, ProtocolError> {
        let motor = validate_motor(motor)?;
        Self::raw(format!("#{motor}{operation}")).map(|mut command| {
            command.effect = effect;
            command.scope = CommandScope::Motor(motor);
            command
        })
    }

    pub fn coord(
        coord: i32,
        operation: &str,
        effect: CommandEffect,
    ) -> Result<Self, ProtocolError> {
        let coord = validate_coord(coord)?;
        Self::raw(format!("&{coord}{operation}")).map(|mut command| {
            command.effect = effect;
            command.scope = CommandScope::Coord(coord);
            command
        })
    }

    pub fn motor_abs_jog(motor: i32, position: f64) -> Result<Self, ProtocolError> {
        Self::motor(
            motor,
            &format!("jog={}", format_float(position)?),
            CommandEffect::Control,
        )
    }

    pub fn motor_inc_jog(motor: i32, distance: f64) -> Result<Self, ProtocolError> {
        Self::motor(
            motor,
            &format!("jog:{}", format_float(distance)?),
            CommandEffect::Control,
        )
    }

    pub fn coord_exec_prog(coord: i32, program: i32) -> Result<Self, ProtocolError> {
        if program <= 0 {
            return Err(ProtocolError::InvalidTarget {
                kind: "program",
                target: program,
                expected: "1..=2147483647",
            });
        }
        Self::coord(
            coord,
            &format!(" begin {program} run"),
            CommandEffect::Control,
        )
    }

    pub fn plc_enable(plc: i32, enable: bool) -> Result<Self, ProtocolError> {
        let plc = validate_plc(plc)?;
        let mut command = Self::raw(format!(
            "{} plc {plc}",
            if enable { "enable" } else { "disable" }
        ))?;
        command.effect = CommandEffect::Control;
        command.scope = CommandScope::Plc(plc);
        Ok(command)
    }

    pub fn abort() -> Self {
        Self {
            text: "&*adisable".into(),
            effect: CommandEffect::Control,
            scope: CommandScope::Global,
            maintenance: MaintenanceKind::None,
            program_line: false,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn effect(&self) -> CommandEffect {
        self.effect
    }
    pub fn scope(&self) -> CommandScope {
        self.scope
    }
    pub fn maintenance(&self) -> MaintenanceKind {
        self.maintenance
    }
}

fn validate_text(text: &str) -> Result<(), ProtocolError> {
    if text.trim().is_empty() {
        return Err(ProtocolError::EmptyCommand);
    }
    if text.len() > MAX_COMMAND_LEN {
        return Err(ProtocolError::CommandTooLong {
            actual: text.len(),
            maximum: MAX_COMMAND_LEN,
        });
    }
    if text
        .bytes()
        .any(|byte| matches!(byte, 0 | b'\r' | b'\n') || byte == 0x06 || byte == 0x15)
    {
        return Err(ProtocolError::InvalidCommandCharacter);
    }
    Ok(())
}

fn classify_segment(
    segment: &str,
) -> Result<(CommandEffect, CommandScope, MaintenanceKind), ProtocolError> {
    if let Some(classification) = crate::online::classify(segment)? {
        return Ok(classification);
    }
    let lower = segment.to_ascii_lowercase();
    if lower == "&*adisable" {
        return Ok((
            CommandEffect::Control,
            CommandScope::Global,
            MaintenanceKind::None,
        ));
    }
    if lower == "$$$***" {
        return Ok((
            CommandEffect::Maintenance,
            CommandScope::Global,
            MaintenanceKind::GlobalReset,
        ));
    }
    if lower == "$$$" {
        return Ok((
            CommandEffect::Maintenance,
            CommandScope::Global,
            MaintenanceKind::Reset,
        ));
    }
    if lower == "save" {
        return Ok((
            CommandEffect::Maintenance,
            CommandScope::Global,
            MaintenanceKind::Save,
        ));
    }
    if matches!(
        lower.as_str(),
        "ecatreset"
            | "ecat reset"
            | "ecatrescan"
            | "ecat rescan"
            | "ecatslaves"
            | "ecat slaves"
            | "ecat config"
    ) {
        return Ok((
            CommandEffect::Maintenance,
            CommandScope::Global,
            MaintenanceKind::EtherCat,
        ));
    }
    if lower.starts_with('#') {
        let digits = segment[1..].bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return Err(ProtocolError::AmbiguousTarget(segment.into()));
        }
        let motor = segment[1..1 + digits]
            .parse::<i32>()
            .map_err(|_| ProtocolError::AmbiguousTarget(segment.into()))?;
        let motor = validate_motor(motor)?;
        let operation = segment[1 + digits..].trim().to_ascii_lowercase();
        if operation.is_empty() {
            return Err(ProtocolError::AmbiguousTarget(segment.into()));
        }
        let read = matches!(operation.as_str(), "?" | "p" | "d" | "v" | "f");
        if let Some(value) = operation
            .strip_prefix("jog=")
            .or_else(|| operation.strip_prefix("jog:"))
        {
            if value
                .parse::<f64>()
                .ok()
                .is_none_or(|number| !number.is_finite())
            {
                return Err(ProtocolError::InvalidFloat);
            }
        }
        let known_write = operation == "kill"
            || operation == "j/"
            || operation == "jog/"
            || operation.starts_with("jog=")
            || operation.starts_with("jog:");
        if !read && !known_write {
            return Err(ProtocolError::UnsupportedCommand(segment.into()));
        }
        return Ok((
            if read {
                CommandEffect::ReadOnly
            } else {
                CommandEffect::Control
            },
            CommandScope::Motor(motor),
            MaintenanceKind::None,
        ));
    }
    if lower.starts_with('&') {
        let digits = segment[1..].bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return Err(ProtocolError::AmbiguousTarget(segment.into()));
        }
        let coord = segment[1..1 + digits]
            .parse::<i32>()
            .map_err(|_| ProtocolError::AmbiguousTarget(segment.into()))?;
        let coord = validate_coord(coord)?;
        let operation = segment[1 + digits..].trim().to_ascii_lowercase();
        let read = matches!(operation.as_str(), "?" | "p");
        let simple_write = matches!(
            operation.as_str(),
            "enable"
                | "hold"
                | "run"
                | "abort"
                | "disable"
                | "adisable"
                | "ddisable"
                | "dkill"
                | "pmach"
        );
        let valid_program = operation
            .strip_prefix("begin ")
            .and_then(|value| value.strip_suffix(" run"))
            .is_some_and(|number| number.trim().parse::<u32>().is_ok_and(|value| value > 0));
        if !read && !simple_write && !valid_program {
            return Err(ProtocolError::UnsupportedCommand(segment.into()));
        }
        return Ok((
            if read {
                CommandEffect::ReadOnly
            } else {
                CommandEffect::Control
            },
            CommandScope::Coord(coord),
            MaintenanceKind::None,
        ));
    }
    if verified_global_read(&lower) {
        return Ok((
            CommandEffect::ReadOnly,
            CommandScope::System,
            MaintenanceKind::None,
        ));
    }
    let indexed_effect = if !structured_read(&lower, "motor")
        && !structured_read(&lower, "coord")
        && !structured_read(&lower, "plc")
    {
        CommandEffect::Control
    } else {
        CommandEffect::ReadOnly
    };
    if let Some(scope) = indexed_scope(segment, "motor[", validate_motor)? {
        return Ok((indexed_effect, scope, MaintenanceKind::None));
    }
    if let Some(scope) = indexed_scope(segment, "coord[", validate_coord)? {
        return Ok((indexed_effect, scope, MaintenanceKind::None));
    }
    if let Some(scope) = indexed_scope(segment, "plc[", validate_plc)? {
        return Ok((indexed_effect, scope, MaintenanceKind::None));
    }

    if lower.starts_with("enable plc ") || lower.starts_with("disable plc ") {
        let number = lower
            .rsplit(' ')
            .next()
            .and_then(|v| v.parse::<i32>().ok())
            .ok_or_else(|| ProtocolError::AmbiguousTarget(segment.into()))?;
        return Ok((
            CommandEffect::Control,
            CommandScope::Plc(validate_plc(number)?),
            MaintenanceKind::None,
        ));
    }
    static SYMBOLIC_VARIABLE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let variable = SYMBOLIC_VARIABLE.get_or_init(|| regex::Regex::new(r"^[a-z_][a-z0-9_]*(?:\[[^\r\n]+\])?(?:\.[a-z_][a-z0-9_]*(?:\[[^\r\n]+\])?)*(?:(?:\s*(?:=|\+=|-=|\*=|/=|->)).*|\+\+|--)?$").expect("symbolic variable regex"));
    if !variable.is_match(&lower)
        || matches!(
            lower.as_str(),
            "system"
                | "pread"
                | "dread"
                | "resume"
                | "linear"
                | "rapid"
                | "abs"
                | "inc"
                | "return"
                | "break"
                | "continue"
                | "else"
                | "endif"
                | "endwhile"
        )
    {
        return Err(ProtocolError::UnsupportedCommand(segment.into()));
    }
    Ok((
        CommandEffect::Control,
        CommandScope::System,
        MaintenanceKind::None,
    ))
}

/// These global queries are safe to distribute only when their complete syntax
/// is recognized. Assignments, operators, appended commands and implicit Q
/// variables deliberately fall through to the conservative control path.
fn verified_global_read(lower: &str) -> bool {
    if lower.strip_prefix('p').is_some_and(parameter_query) {
        return true;
    }
    if ["enctable", "ecat"]
        .iter()
        .any(|root| structured_read(lower, root))
    {
        return true;
    }
    let mut words = lower.split_ascii_whitespace();
    matches!((words.next(), words.next(), words.next(), words.next()),
        (Some("list"), Some("prog"), Some(number), None) if unsigned_index(number))
}

fn parameter_query(text: &str) -> bool {
    if unsigned_index(text) {
        return true;
    }
    let Some((start, end)) = text.split_once("..") else {
        return false;
    };
    if !unsigned_index(start) || !unsigned_index(end) {
        return false;
    }
    let (Ok(start), Ok(end)) = (start.parse::<u32>(), end.parse::<u32>()) else {
        return false;
    };
    end.checked_sub(start)
        .and_then(|span| span.checked_add(1))
        .is_some_and(|span| span <= 8192)
}
fn unsigned_index(text: &str) -> bool {
    !text.is_empty()
        && text.bytes().all(|byte| byte.is_ascii_digit())
        && text.parse::<u32>().is_ok()
}

fn consume_index(text: &str) -> Option<&str> {
    let rest = text.strip_prefix('[')?;
    let end = rest.find(']')?;
    if !unsigned_index(&rest[..end]) {
        return None;
    }
    Some(&rest[end + 1..])
}

fn structured_read(query: &str, root: &str) -> bool {
    let Some(mut rest) = query.strip_prefix(root).and_then(consume_index) else {
        return false;
    };
    loop {
        let Some(field) = rest.strip_prefix('.') else {
            return false;
        };
        let Some(first) = field.bytes().next() else {
            return false;
        };
        if !first.is_ascii_alphabetic() && first != b'_' {
            return false;
        }
        let end = field
            .bytes()
            .take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
            .count();
        rest = &field[end..];
        while rest.starts_with('[') {
            let Some(tail) = consume_index(rest) else {
                return false;
            };
            rest = tail;
        }
        if rest.is_empty() {
            return true;
        }
    }
}
fn indexed_scope(
    segment: &str,
    prefix: &str,
    validate: fn(i32) -> Result<u8, ProtocolError>,
) -> Result<Option<CommandScope>, ProtocolError> {
    let lower = segment.to_ascii_lowercase();
    if !lower.starts_with(prefix) {
        return Ok(None);
    }
    let close = lower
        .find(']')
        .ok_or_else(|| ProtocolError::AmbiguousTarget(segment.into()))?;
    let target = lower[prefix.len()..close]
        .parse::<i32>()
        .map_err(|_| ProtocolError::AmbiguousTarget(segment.into()))?;
    let target = validate(target)?;
    if close + 1 >= lower.len() || lower.as_bytes()[close + 1] != b'.' {
        return Err(ProtocolError::UnsupportedCommand(segment.into()));
    }
    Ok(Some(match prefix {
        "motor[" => CommandScope::Motor(target),
        "coord[" => CommandScope::Coord(target),
        _ => CommandScope::Plc(target),
    }))
}

fn merge_scopes(left: CommandScope, right: CommandScope) -> CommandScope {
    if left == CommandScope::System {
        right
    } else if right == CommandScope::System || left == right {
        left
    } else {
        CommandScope::Global
    }
}

fn merge_maintenance(left: MaintenanceKind, right: MaintenanceKind) -> MaintenanceKind {
    match (left, right) {
        (MaintenanceKind::None, value) | (value, MaintenanceKind::None) => value,
        (MaintenanceKind::GlobalReset, _) | (_, MaintenanceKind::GlobalReset) => {
            MaintenanceKind::GlobalReset
        } // Any reset in a compound command invalidates every lane, irrespective of order.
        (MaintenanceKind::Reset, _) | (_, MaintenanceKind::Reset) => MaintenanceKind::Reset,
        (left, _) => left,
    }
}

pub fn validate_motor(value: i32) -> Result<u8, ProtocolError> {
    u8::try_from(value).map_err(|_| ProtocolError::InvalidTarget {
        kind: "motor",
        target: value,
        expected: "0..=255",
    })
}

pub fn validate_coord(value: i32) -> Result<u8, ProtocolError> {
    if (0..=127).contains(&value) {
        Ok(value as u8)
    } else {
        Err(ProtocolError::InvalidTarget {
            kind: "coordinate",
            target: value,
            expected: "0..=127",
        })
    }
}

pub fn validate_plc(value: i32) -> Result<u8, ProtocolError> {
    if (0..=31).contains(&value) {
        Ok(value as u8)
    } else {
        Err(ProtocolError::InvalidTarget {
            kind: "PLC",
            target: value,
            expected: "0..=31",
        })
    }
}

fn format_float(value: f64) -> Result<String, ProtocolError> {
    if !value.is_finite() {
        return Err(ProtocolError::InvalidFloat);
    }
    let text = format!("{value:.17}");
    if text.len() > 32 || text.parse::<f64>().ok() != Some(value) {
        return Err(ProtocolError::InvalidFloat);
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_every_legacy_operation_with_explicit_target() {
        assert_eq!(
            Command::motor(1, "?", CommandEffect::ReadOnly)
                .unwrap()
                .text(),
            "#1?"
        );
        assert_eq!(
            Command::motor(255, "jog/", CommandEffect::Control)
                .unwrap()
                .text(),
            "#255jog/"
        );
        assert_eq!(
            Command::coord(2, " begin 3 run", CommandEffect::Control)
                .unwrap()
                .text(),
            "&2 begin 3 run"
        );
        assert_eq!(
            Command::plc_enable(31, false).unwrap().text(),
            "disable plc 31"
        );
        assert_eq!(Command::abort().text(), "&*adisable");
    }

    #[test]
    fn rejects_ambiguous_motion_and_bad_numbers() {
        for text in [
            "#jog=1",
            "&a begin 1 run",
            "jog=1",
            "#1jog=1;kill",
            "#256?",
            "&128?",
            "&128P",
            "&P",
            "#1jog=NaN",
            "Motor[-1].ClosedLoop",
            "Coord[128].AmpEna",
            "Plc[32].Active",
        ] {
            assert!(Command::raw(text).is_err(), "accepted {text}");
        }
        assert!(Command::motor_abs_jog(1, f64::NAN).is_err());
        assert!(Command::motor_inc_jog(1, f64::INFINITY).is_err());
    }

    #[test]
    fn classifies_maintenance_and_compound_scope() {
        let command = Command::raw("#1?;&2?").unwrap();
        assert_eq!(command.scope(), CommandScope::Global);
        assert_eq!(
            Command::raw("$$$***").unwrap().maintenance(),
            MaintenanceKind::GlobalReset
        );
        assert_eq!(
            Command::raw("Motor[1].AmpEna=1").unwrap().effect(),
            CommandEffect::Control
        );
        assert_eq!(
            Command::raw("Motor[1].ClosedLoop").unwrap().effect(),
            CommandEffect::ReadOnly
        );
    }

    fn assert_maintenance_variants(alias: &str, kind: MaintenanceKind) {
        let alternating = alias
            .chars()
            .enumerate()
            .map(|(index, ch)| {
                if index % 2 == 0 {
                    ch.to_ascii_uppercase()
                } else {
                    ch.to_ascii_lowercase()
                }
            })
            .collect::<String>();
        let mut cases = vec![alias.to_owned(), alias.to_ascii_uppercase(), alternating];
        cases.sort();
        cases.dedup();
        for case in cases {
            for (leading, trailing) in [("", ""), (" ", " "), ("\t  ", " \t"), ("  ", "")] {
                let padded = format!("{leading}{case}{trailing}");
                for wire in [
                    padded.clone(),
                    format!("{padded};Sys.ServoCount"),
                    format!("Sys.ServoCount;{padded}"),
                    format!("#1p;{padded};&1?"),
                ] {
                    let command = Command::raw(&wire).unwrap();
                    assert_eq!(
                        command.effect(),
                        CommandEffect::Maintenance,
                        "effect for {wire:?}"
                    );
                    assert_eq!(command.scope(), CommandScope::Global, "scope for {wire:?}");
                    assert_eq!(command.maintenance(), kind, "maintenance for {wire:?}");
                    assert_eq!(
                        command.text(),
                        wire,
                        "classification must preserve caller-owned wire text"
                    );
                }
            }
        }
    }

    macro_rules! maintenance_alias_regression {
        ($name:ident, $alias:literal, $kind:expr) => {
            #[test]
            fn $name() {
                assert_maintenance_variants($alias, $kind);
            }
        };
    }

    maintenance_alias_regression!(
        ecatreset_variants_use_device_maintenance,
        "ecatreset",
        MaintenanceKind::EtherCat
    );
    maintenance_alias_regression!(
        ecat_reset_variants_use_device_maintenance,
        "ecat reset",
        MaintenanceKind::EtherCat
    );
    maintenance_alias_regression!(
        ecatrescan_variants_use_device_maintenance,
        "ecatrescan",
        MaintenanceKind::EtherCat
    );
    maintenance_alias_regression!(
        ecat_rescan_variants_use_device_maintenance,
        "ecat rescan",
        MaintenanceKind::EtherCat
    );
    maintenance_alias_regression!(
        ecatslaves_variants_use_device_maintenance,
        "ecatslaves",
        MaintenanceKind::EtherCat
    );
    maintenance_alias_regression!(
        ecat_slaves_variants_use_device_maintenance,
        "ecat slaves",
        MaintenanceKind::EtherCat
    );
    maintenance_alias_regression!(
        save_variants_use_device_maintenance,
        "save",
        MaintenanceKind::Save
    );
    maintenance_alias_regression!(
        reset_variants_retire_all_device_lanes,
        "$$$",
        MaintenanceKind::Reset
    );
    maintenance_alias_regression!(
        global_reset_variants_retire_all_device_lanes,
        "$$$***",
        MaintenanceKind::GlobalReset
    );

    #[test]
    fn reset_dominates_other_maintenance_in_every_compound_order() {
        for (reset, expected) in [
            ("$$$", MaintenanceKind::Reset),
            ("$$$***", MaintenanceKind::GlobalReset),
        ] {
            for other in [
                "save",
                "ecatreset",
                "ecat reset",
                "ecatrescan",
                "ecat rescan",
                "ecatslaves",
                "ecat slaves",
                "#1p",
            ] {
                for wire in [
                    format!("{other};{reset}"),
                    format!("{reset};{other}"),
                    format!("Sys.ServoCount;{other}; {reset} ;&1?"),
                ] {
                    let command = Command::raw(&wire).unwrap();
                    assert_eq!(command.effect(), CommandEffect::Maintenance, "{wire:?}");
                    assert_eq!(command.scope(), CommandScope::Global, "{wire:?}");
                    assert_eq!(command.maintenance(), expected, "{wire:?}");
                }
            }
        }
        for wire in ["$$$;$$$***", "$$$***;$$$", "save;$$$;ecatreset;$$$***"] {
            assert_eq!(
                Command::raw(wire).unwrap().maintenance(),
                MaintenanceKind::GlobalReset,
                "{wire:?}"
            );
        }
    }

    #[test]
    fn maintenance_substrings_do_not_reclassify_unrelated_commands() {
        for wire in [
            "Sys.SaveCounter",
            "Sys.EcatResetCount",
            "save_backup",
            "ecatreset_extra",
            "ecat slaves_extra",
        ] {
            let command = Command::raw(wire).unwrap();
            assert_eq!(command.maintenance(), MaintenanceKind::None, "{wire:?}");
            assert_ne!(command.effect(), CommandEffect::Maintenance, "{wire:?}");
        }
    }

    #[test]
    fn float_targets_that_cannot_roundtrip_are_rejected_before_send() {
        for value in [1e-20, -1e-20, 1.5e-17, f64::MIN_POSITIVE, f64::from_bits(1)] {
            assert_eq!(
                Command::motor_abs_jog(1, value),
                Err(ProtocolError::InvalidFloat),
                "abs target {value:?}"
            );
            assert_eq!(
                Command::motor_inc_jog(1, value),
                Err(ProtocolError::InvalidFloat),
                "relative target {value:?}"
            );
        }
    }

    #[test]
    fn supported_finite_targets_preserve_their_numeric_value() {
        for value in [0.0, -0.0, 0.1, -0.125, 1e-17, 1234567.89123] {
            let command = Command::motor_abs_jog(1, value).unwrap();
            let numeric = command
                .text()
                .strip_prefix("#1jog=")
                .unwrap()
                .parse::<f64>()
                .unwrap();
            assert_eq!(numeric.to_bits(), value.to_bits(), "target {value:?}");
        }
    }

    #[test]
    fn complete_coordinate_position_queries_are_case_insensitive_and_read_only() {
        for (wire, coord) in [("&0p", 0), ("&1P", 1), (" \t&1p \t", 1), ("&127P", 127)] {
            let command = Command::raw(wire).unwrap();
            assert_eq!(command.effect(), CommandEffect::ReadOnly, "{wire:?}");
            assert_eq!(command.scope(), CommandScope::Coord(coord), "{wire:?}");
            assert_eq!(command.maintenance(), MaintenanceKind::None, "{wire:?}");
            assert_eq!(command.text(), wire);
        }
    }

    #[test]
    fn coordinate_position_compounds_preserve_scope_and_read_classification() {
        for wire in ["&1P;#1p", "Sys.ServoCount;&1p", "&1p;&1?", "&1P;&2p"] {
            let command = Command::raw(wire).unwrap();
            assert_eq!(command.effect(), CommandEffect::ReadOnly, "{wire:?}");
            assert_eq!(command.maintenance(), MaintenanceKind::None, "{wire:?}");
        }
        assert_eq!(
            Command::raw("&1P;#1p").unwrap().scope(),
            CommandScope::Global
        );
        assert_eq!(
            Command::raw("&1p;&1?").unwrap().scope(),
            CommandScope::Coord(1)
        );
        assert_eq!(
            Command::raw("&1P;&1run").unwrap().effect(),
            CommandEffect::Control
        );
    }

    #[test]
    fn verified_global_queries_are_read_only_with_full_syntax_consumption() {
        for wire in [
            "EncTable[1].type",
            " enctable[0].TYPE ",
            "ECAT[0].Enable",
            "ecat[1].Slave[2].State",
            "P82",
            "p0",
            "P4294967295",
            "list prog 100",
            "LIST PROG 0",
            " list\tprog 12 ",
        ] {
            let command = Command::raw(wire).unwrap();
            assert_eq!(command.effect(), CommandEffect::ReadOnly, "{wire:?}");
            assert_eq!(command.scope(), CommandScope::System, "{wire:?}");
            assert_eq!(command.maintenance(), MaintenanceKind::None, "{wire:?}");
        }
        assert_eq!(
            Command::raw("P82;EncTable[1].type;ECAT[0].Enable;list prog 100")
                .unwrap()
                .effect(),
            CommandEffect::ReadOnly
        );
    }

    #[test]
    fn assignments_malformed_queries_and_implicit_context_never_enter_the_read_pool() {
        for wire in [
            "EncTable[1].type=1",
            "EncTable[1].type extra",
            "EncTable[1].",
            "EncTable[1].type+1",
            "EncTable[-1].type",
            "EncTable[4294967296].type",
            "ECAT[0].Enable=1",
            "ECAT[0].Enable trailing",
            "ECAT[0].Enable()",
            "ECAT[0].Slave[].State",
            "P82=1",
            "P82 P83",
            "P-1",
            "P4294967296",
            "list prog -1",
            "list prog 1 extra",
            "list prog +1",
            "list prog 1=2",
            "Q82",
            "unknown_query",
            "P82;ECAT[0].Enable=1",
        ] {
            if let Ok(command) = Command::raw(wire) {
                assert_ne!(
                    command.effect(),
                    CommandEffect::ReadOnly,
                    "unsafe read routing for {wire:?}"
                );
            }
        }
    }

    #[test]
    fn bounded_parameter_ranges_are_complete_read_only_queries() {
        for wire in [
            "P1..8192",
            "p0..8191",
            " P82..82 ",
            "P4294967295..4294967295",
            "P1..2;ECAT[0].Enable",
        ] {
            let command = Command::raw(wire).unwrap();
            assert_eq!(command.effect(), CommandEffect::ReadOnly, "{wire:?}");
            assert_eq!(command.scope(), CommandScope::System, "{wire:?}");
            assert_eq!(command.maintenance(), MaintenanceKind::None);
        }
    }

    #[test]
    fn parameter_range_overflow_reverse_spans_and_appended_operations_are_not_read_only() {
        for wire in [
            "P1..8193",
            "P0..4294967295",
            "P82..81",
            "P1..4294967296",
            "P4294967296..4294967296",
            "P-1..2",
            "P1..-2",
            "P+1..2",
            "P1..+2",
            "P1..2=1",
            "P1..2 extra",
            "P1 ..2",
            "P1.. 2",
            "P1...2",
            "P1..2..3",
            "P..2",
            "P1..",
            "Q1..8192",
        ] {
            if let Ok(command) = Command::raw(wire) {
                assert_ne!(command.effect(), CommandEffect::ReadOnly, "{wire:?}");
            }
        }
    }
}
