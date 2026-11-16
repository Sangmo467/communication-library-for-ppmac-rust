use crate::{parse_u64, GpasciiDecoder, ProtocolError};

pub const RESET_READINESS_PROBE: &str = "Sys.ServoCount";
const INPUT_READY: &[u8] = b"STDIN Open for ASCII Input";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResetEvent {
    ProbeRequired,
    Complete(Vec<u8>),
    Rejected(Vec<u8>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    AwaitReset,
    AwaitProbeDispatch,
    AwaitProbe,
    Complete,
}

/// A reset is never completed by silence or a prompt substring. Reopened input
/// without an ACK requires a newly sent, read-only readiness query and its ACK.
#[derive(Debug)]
pub struct ResetDecoder {
    command: Vec<u8>,
    phase: Phase,
    reset_output: Vec<u8>,
    probe_output: Vec<u8>,
    line: Vec<u8>,
    maximum: usize,
}

impl ResetDecoder {
    pub fn new(command: &[u8], maximum: usize) -> Self {
        Self {
            command: command.to_vec(),
            phase: Phase::AwaitReset,
            reset_output: Vec::new(),
            probe_output: Vec::new(),
            line: Vec::new(),
            maximum,
        }
    }

    /// Called only after the transport has queued the unique readiness probe.
    /// Bytes already delivered with the reset banner cannot acknowledge it.
    pub fn probe_sent(&mut self) -> Result<(), ProtocolError> {
        if self.phase != Phase::AwaitProbeDispatch {
            return Err(ProtocolError::UnexpectedData(
                "reset readiness probe sent out of order".into(),
            ));
        }
        self.phase = Phase::AwaitProbe;
        self.probe_output.clear();
        self.line.clear();
        Ok(())
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<ResetEvent>, ProtocolError> {
        let mut events = Vec::new();
        for &byte in bytes {
            match self.phase {
                Phase::AwaitReset => {
                    self.append_reset(byte)?;
                    if matches!(byte, 0x06 | 0x15) {
                        let frames = GpasciiDecoder::new(&self.command, self.maximum)
                            .push(&self.reset_output)?;
                        let frame = frames
                            .into_iter()
                            .next()
                            .ok_or(ProtocolError::IncompleteFrame)?;
                        self.phase = Phase::Complete;
                        events.push(if frame.remote_error {
                            ResetEvent::Rejected(frame.body)
                        } else {
                            ResetEvent::Complete(frame.body)
                        });
                    } else if self.ready_line(byte) {
                        self.phase = Phase::AwaitProbeDispatch;
                        events.push(ResetEvent::ProbeRequired);
                    }
                }
                Phase::AwaitProbeDispatch => {
                    // Only bytes received after probe_sent() may confirm readiness.
                    if !matches!(byte, 0x06 | 0x15) {
                        self.append_reset(byte)?;
                    }
                }
                Phase::AwaitProbe => {
                    if self.probe_output.len() >= 256 {
                        return Err(ProtocolError::UnexpectedData(
                            "reset readiness probe output exceeds its bounded numeric frame".into(),
                        ));
                    }
                    self.probe_output.push(byte);
                    if matches!(byte, 0x06 | 0x15) {
                        let frame = GpasciiDecoder::new(RESET_READINESS_PROBE.as_bytes(), 128)
                            .push(&self.probe_output)?
                            .into_iter()
                            .next()
                            .ok_or(ProtocolError::IncompleteFrame)?;
                        self.probe_output.clear();
                        self.line.clear();
                        // A late empty reset ACK is not the numeric probe confirmation.
                        if !frame.remote_error && frame.body.is_empty() {
                            continue;
                        }
                        if frame.remote_error || !valid_counter(&frame.body) {
                            return Err(ProtocolError::UnexpectedData(
                                "reset readiness probe did not return a valid Sys.ServoCount frame"
                                    .into(),
                            ));
                        }
                        self.phase = Phase::Complete;
                        events.push(ResetEvent::Complete(self.response_body()));
                    } else if self.ready_line(byte) {
                        // Repeated startup banners do not authorize another probe/reset.
                        self.probe_output.clear();
                    }
                }
                Phase::Complete => {
                    if !matches!(byte, b'\r' | b'\n') {
                        return Err(ProtocolError::UnexpectedData(
                            "data follows reset completion".into(),
                        ));
                    }
                }
            }
        }
        Ok(events)
    }

    fn append_reset(&mut self, byte: u8) -> Result<(), ProtocolError> {
        if self.reset_output.len()
            >= self
                .maximum
                .saturating_add(self.command.len())
                .saturating_add(64)
        {
            return Err(ProtocolError::ResponseTooLarge(self.maximum));
        }
        self.reset_output.push(byte);
        Ok(())
    }

    fn ready_line(&mut self, byte: u8) -> bool {
        if matches!(byte, b'\r' | b'\n') {
            let ready = trim_ascii(&self.line) == INPUT_READY;
            self.line.clear();
            ready
        } else {
            self.line.push(byte);
            false
        }
    }

    fn response_body(&self) -> Vec<u8> {
        let mut body = trim_ascii(&self.reset_output);
        if body.starts_with(&self.command)
            && body
                .get(self.command.len())
                .is_some_and(|byte| matches!(byte, b'\r' | b'\n'))
        {
            body = trim_ascii(&body[self.command.len()..]);
        }
        let mut body = body.to_vec();
        crate::gpascii::filter_input_ready(&mut body);
        trim_ascii(&body).to_vec()
    }
}

fn valid_counter(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let value = if let Some((name, value)) = text.trim().split_once('=') {
        if !name.eq_ignore_ascii_case(RESET_READINESS_PROBE) {
            return false;
        }
        value.as_bytes()
    } else {
        bytes
    };
    parse_u64(value).is_ok()
}

fn trim_ascii(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[1..];
    }
    while bytes.last().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reopened_input_and_numeric_probe_accept_every_packet_split() {
        let banner = b"$$$***\r\nSTDIN Open for ASCII Input\r\n";
        let probe = b"Sys.ServoCount\r\nSys.ServoCount=70173\r\n\x06\r\n";
        for split in 0..=banner.len() {
            for probe_split in 0..=probe.len() {
                let mut decoder = ResetDecoder::new(b"$$$***", 1024);
                let mut events = decoder.push(&banner[..split]).unwrap();
                events.extend(decoder.push(&banner[split..]).unwrap());
                assert_eq!(events, vec![ResetEvent::ProbeRequired]);
                decoder.probe_sent().unwrap();
                let mut events = decoder.push(&probe[..probe_split]).unwrap();
                events.extend(decoder.push(&probe[probe_split..]).unwrap());
                assert_eq!(events, vec![ResetEvent::Complete(Vec::new())]);
            }
        }
    }

    #[test]
    fn repeated_banners_and_late_empty_acks_do_not_confirm_or_repeat_the_probe() {
        let mut decoder = ResetDecoder::new(b"$$$", 1024);
        assert_eq!(
            decoder.push(b"STDIN Open for ASCII Input\n").unwrap(),
            vec![ResetEvent::ProbeRequired]
        );
        assert!(
            decoder.push(b"Sys.ServoCount=1\x06").unwrap().is_empty(),
            "pre-probe bytes cannot confirm a probe that has not been sent"
        );
        decoder.probe_sent().unwrap();
        assert!(decoder.probe_sent().is_err());
        assert!(decoder
            .push(b"STDIN Open for ASCII Input\n\x06\r\n")
            .unwrap()
            .is_empty());
        assert!(matches!(
            decoder.push(b"42\r\n\x06").unwrap().as_slice(),
            [ResetEvent::Complete(_)]
        ));
    }

    #[test]
    fn unrelated_banner_substrings_and_partial_lines_do_not_authorize_a_probe() {
        let mut decoder = ResetDecoder::new(b"$$$", 1024);
        assert!(decoder
            .push(b"diagnostic: STDIN Open for ASCII Input\nSTDIN Open for ASCII Inpu")
            .unwrap()
            .is_empty());
        assert!(decoder.probe_sent().is_err());
        assert!(decoder.push(b"t").unwrap().is_empty());
        assert_eq!(
            decoder.push(b"\r\n").unwrap(),
            vec![ResetEvent::ProbeRequired]
        );
    }

    #[test]
    fn reset_ack_and_remote_rejection_keep_their_protocol_meaning() {
        let mut decoder = ResetDecoder::new(b"$$$", 1024);
        assert_eq!(
            decoder.push(b"$$$\r\n\x06\r\n").unwrap(),
            vec![ResetEvent::Complete(Vec::new())]
        );
        let mut rejected = ResetDecoder::new(b"$$$", 1024);
        assert_eq!(
            rejected.push(b"ERR003\x15").unwrap(),
            vec![ResetEvent::Rejected(b"ERR003".to_vec())]
        );
    }

    #[test]
    fn invalid_named_or_non_numeric_probe_frames_are_never_readiness() {
        for frame in [
            b"Motor[1].Pos=42\x06".as_slice(),
            b"garbage\x06",
            b"-1\x06",
            b"18446744073709551616\x06",
            b"ERR003\x15",
        ] {
            let mut decoder = ResetDecoder::new(b"$$$", 1024);
            decoder.push(b"STDIN Open for ASCII Input\n").unwrap();
            decoder.probe_sent().unwrap();
            assert!(decoder.push(frame).is_err(), "accepted {frame:?}");
        }
    }

    #[test]
    fn startup_noise_is_bounded_and_trailing_completed_frames_are_rejected() {
        let mut decoder = ResetDecoder::new(b"$$$", 8);
        assert!(decoder.push(&[b'x'; 100]).is_err());
        let mut decoder = ResetDecoder::new(b"$$$", 1024);
        assert!(decoder.push(b"\x06old\x06").is_err());
    }
}
