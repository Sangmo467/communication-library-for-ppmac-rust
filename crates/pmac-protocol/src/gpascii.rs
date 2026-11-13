use crate::ProtocolError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpasciiFrame {
    pub body: Vec<u8>,
    pub remote_error: bool,
}

#[derive(Debug)]
pub struct GpasciiDecoder {
    buffer: Vec<u8>,
    echo: Vec<u8>,
    echo_checked: bool,
    maximum: usize,
    terminal_count: usize,
}

impl GpasciiDecoder {
    pub fn new(command: &[u8], maximum: usize) -> Self {
        Self {
            buffer: Vec::new(),
            echo: command.to_vec(),
            echo_checked: false,
            maximum,
            terminal_count: 0,
        }
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<GpasciiFrame>, ProtocolError> {
        self.terminal_count += bytes
            .iter()
            .filter(|&&byte| byte == 0x06 || byte == 0x15)
            .count();
        if self.terminal_count > 1 {
            return Err(ProtocolError::UnexpectedData(
                "more than one response terminator for an in-flight command".into(),
            ));
        }
        if self.buffer.len().saturating_add(bytes.len())
            > self
                .maximum
                .saturating_add(self.echo.len())
                .saturating_add(64)
        {
            return Err(ProtocolError::ResponseTooLarge(self.maximum));
        }
        self.buffer.extend_from_slice(bytes);
        if !self.echo_checked && !self.echo.is_empty() {
            self.strip_echo()?;
        }
        let Some(index) = self
            .buffer
            .iter()
            .position(|&byte| byte == 0x06 || byte == 0x15)
        else {
            // An incomplete echo is bounded by the command length and is not response data.
            // A trailing CR/LF may also be framing rather than part of the body.
            let pending_echo = if self.echo_checked {
                0
            } else {
                self.echo.len().saturating_add(1)
            };
            let framing = usize::from(self.buffer.ends_with(b"\r\n")) * 2
                + usize::from(
                    !self.buffer.ends_with(b"\r\n")
                        && (self.buffer.ends_with(b"\r") || self.buffer.ends_with(b"\n")),
                );
            let leading = self
                .buffer
                .iter()
                .take_while(|&&b| matches!(b, b'\r' | b'\n' | b' ' | b'\t'))
                .count();
            if self.buffer.len().saturating_sub(leading)
                > self
                    .maximum
                    .saturating_add(pending_echo)
                    .saturating_add(framing)
            {
                return Err(ProtocolError::ResponseTooLarge(self.maximum));
            }
            return Ok(Vec::new());
        };
        let remote_error = self.buffer[index] == 0x15;
        let mut body = self.buffer.drain(..index).collect::<Vec<_>>();
        self.buffer.drain(
            ..self
                .buffer
                .iter()
                .position(|&byte| byte == 0x06 || byte == 0x15)
                .map_or(0, |index| index + 1),
        );
        trim_trailing_newline(&mut body);
        trim_leading_newlines(&mut body);
        if body.len() > self.maximum {
            return Err(ProtocolError::ResponseTooLarge(self.maximum));
        }
        if !self.buffer.is_empty() && !self.buffer.iter().all(|byte| matches!(byte, b'\r' | b'\n'))
        {
            return Err(ProtocolError::UnexpectedData(
                "data follows the completed response frame".into(),
            ));
        }
        self.buffer.clear();
        Ok(vec![GpasciiFrame { body, remote_error }])
    }

    pub fn finish(self) -> Result<(), ProtocolError> {
        if self.buffer.iter().all(|byte| matches!(byte, b'\r' | b'\n')) {
            Ok(())
        } else {
            Err(ProtocolError::IncompleteFrame)
        }
    }

    fn strip_echo(&mut self) -> Result<(), ProtocolError> {
        trim_leading_newlines(&mut self.buffer);
        if self.buffer.len() < self.echo.len() {
            if !self.echo.starts_with(&self.buffer) {
                self.echo_checked = true;
            }
            return Ok(());
        }
        if self.buffer.starts_with(&self.echo) {
            // A named value ("Sys.ServoCount=42") is response data, not echo.
            // Wait for the delimiter when a packet ends exactly at the command.
            if self.buffer.len() == self.echo.len() {
                return Ok(());
            }
            if !matches!(self.buffer[self.echo.len()], b'\r' | b'\n') {
                self.echo_checked = true;
                return Ok(());
            }
            self.buffer.drain(..self.echo.len());
            if self.buffer.starts_with(b"\r") {
                self.buffer.drain(..1);
            }
            if self.buffer.starts_with(b"\n") {
                self.buffer.drain(..1);
            }
        }
        self.echo_checked = true;
        Ok(())
    }
}

fn trim_leading_newlines(bytes: &mut Vec<u8>) {
    let count = bytes
        .iter()
        .take_while(|&&byte| matches!(byte, b'\r' | b'\n' | b' ' | b'\t'))
        .count();
    bytes.drain(..count);
}

fn trim_trailing_newline(bytes: &mut Vec<u8>) {
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
    }
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_values_are_not_mistaken_for_command_echo() {
        let wire = b"Sys.ServoCount=42\r\n\x06";
        for split in 0..=wire.len() {
            let mut decoder = GpasciiDecoder::new(b"Sys.ServoCount", 1024);
            let mut frames = decoder.push(&wire[..split]).unwrap();
            frames.extend(decoder.push(&wire[split..]).unwrap());
            assert_eq!(frames[0].body, b"Sys.ServoCount=42", "split {split}");
        }
    }
    #[test]
    fn accepts_every_split_point_and_line_ending() {
        let wire = b"#1p\r\n12.5\r\n\x06\r\n";
        for split in 0..=wire.len() {
            let mut decoder = GpasciiDecoder::new(b"#1p", 1024);
            let mut frames = decoder.push(&wire[..split]).unwrap();
            frames.extend(decoder.push(&wire[split..]).unwrap());
            assert_eq!(
                frames,
                vec![GpasciiFrame {
                    body: b"12.5".to_vec(),
                    remote_error: false
                }],
                "split {split}"
            );
        }
    }

    #[test]
    fn fragmented_echo_does_not_consume_small_response_budget() {
        let wire = b"Sys.ServoCount\r\n1\r\n\x06";
        let mut bytewise = GpasciiDecoder::new(b"Sys.ServoCount", 1);
        let mut result = Vec::new();
        for byte in wire {
            result.extend(bytewise.push(&[*byte]).unwrap());
        }
        assert_eq!(result[0].body, b"1");
        for split in 0..=wire.len() {
            let mut decoder = GpasciiDecoder::new(b"Sys.ServoCount", 1);
            let mut frames = decoder.push(&wire[..split]).unwrap();
            frames.extend(decoder.push(&wire[split..]).unwrap());
            assert_eq!(frames[0].body, b"1", "split {split}");
        }
    }
    #[test]
    fn small_response_framing_survives_every_split() {
        for ending in [b"\n".as_slice(), b"\r", b"\r\n"] {
            for size in 0..=3 {
                let mut wire = vec![b'x'; size];
                wire.extend_from_slice(ending);
                wire.push(6);
                for split in 0..=wire.len() {
                    let mut decoder = GpasciiDecoder::new(b"cmd", size);
                    let mut frames = decoder.push(&wire[..split]).unwrap();
                    frames.extend(decoder.push(&wire[split..]).unwrap());
                    assert_eq!(frames[0].body, vec![b'x'; size]);
                }
            }
        }
    }
    #[test]
    fn supports_empty_remote_error_and_no_echo() {
        let mut decoder = GpasciiDecoder::new(b"save", 100);
        assert_eq!(decoder.push(b"\r\n\x06\r\n").unwrap()[0].body, b"");
        let mut decoder = GpasciiDecoder::new(b"#1?", 100);
        assert!(decoder.push(b"$8000000000000000\r\n\x15").unwrap()[0].remote_error);
    }

    #[test]
    fn rejects_trailing_or_oversized_data() {
        let mut decoder = GpasciiDecoder::new(b"#1?", 4);
        assert!(matches!(
            decoder.push(b"12345"),
            Err(ProtocolError::ResponseTooLarge(4))
        ));
        let mut decoder = GpasciiDecoder::new(b"#1?", 100);
        assert!(decoder.push(b"1\x06old\x06").is_err());
    }

    proptest::proptest! {
        #[test]
        fn arbitrary_packetization_does_not_change_the_frame(chunks in proptest::collection::vec(proptest::collection::vec(proptest::prelude::any::<u8>(), 0..8), 1..32)) {
            let wire = b"12.5\r\n\x06\r\n";
            let mut decoder = GpasciiDecoder::new(b"#1p", 1024);
            let mut offset = 0;
            let mut frames = Vec::new();
            for chunk in chunks {
                let end = (offset + chunk.len()).min(wire.len());
                frames.extend(decoder.push(&wire[offset..end]).unwrap());
                offset = end;
                if offset == wire.len() { break; }
            }
            if offset < wire.len() { frames.extend(decoder.push(&wire[offset..]).unwrap()); }
            proptest::prop_assert_eq!(frames, vec![GpasciiFrame { body: b"12.5".to_vec(), remote_error: false }]);
        }
    }

    #[test]
    fn eight_thousand_named_parameter_lines_survive_fragmented_ack_framing() {
        use std::fmt::Write;
        let mut expected = String::new();
        for index in 1_u32..=8192 {
            writeln!(expected, "P{index}={}", u64::from(index) * 31 + 7).unwrap();
        }
        expected.pop();
        let wire = format!("P1..8192\r\n{expected}\r\n\x06\r\n");
        for chunk_size in [1024, 4093, 8192] {
            let mut decoder = GpasciiDecoder::new(b"P1..8192", 1024 * 1024);
            let mut frames = Vec::new();
            for chunk in wire.as_bytes().chunks(chunk_size) {
                frames.extend(decoder.push(chunk).unwrap());
            }
            assert_eq!(frames.len(), 1);
            assert_eq!(frames[0].body, expected.as_bytes());
            let text = std::str::from_utf8(&frames[0].body).unwrap();
            assert_eq!(text.lines().count(), 8192);
            for (offset, line) in text.lines().enumerate() {
                let index = offset as u64 + 1;
                assert_eq!(line, format!("P{index}={}", index * 31 + 7));
            }
        }
    }
}
