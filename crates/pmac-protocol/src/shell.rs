use crate::ProtocolError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellMarkers {
    pub begin: Vec<u8>,
    pub end: Vec<u8>,
    pub stderr_end: Vec<u8>,
}

impl ShellMarkers {
    pub fn new(nonce: &str, sequence: u64) -> Self {
        Self {
            begin: format!("__PMAC_BEGIN_{nonce}_{sequence}__").into_bytes(),
            end: format!("__PMAC_END_{nonce}_{sequence}__").into_bytes(),
            stderr_end: format!("__PMAC_STDERR_END_{nonce}_{sequence}__").into_bytes(),
        }
    }

    pub fn command(&self, command: &str) -> Result<Vec<u8>, ProtocolError> {
        if command.as_bytes().contains(&0) || command.contains(['\r', '\n']) {
            return Err(ProtocolError::InvalidCommandCharacter);
        }
        let begin = String::from_utf8_lossy(&self.begin);
        let end = String::from_utf8_lossy(&self.end);
        let stderr_end = String::from_utf8_lossy(&self.stderr_end);
        Ok(format!("printf '\\n%s\\n' '{begin}'; {{ {command}\n}}; __pmac_rc=$?; printf '\\n%s:%s\\n' '{end}' \"$__pmac_rc\"; printf '\\n%s' '{stderr_end}' >&2\n").into_bytes())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellEvent {
    Output(Vec<u8>),
    Complete { stdout: Vec<u8>, exit_code: i32 },
}

#[derive(Debug)]
pub struct ShellStderrDecoder {
    marker: Vec<u8>,
    buffer: Vec<u8>,
    stderr: Vec<u8>,
    maximum: usize,
}

impl ShellStderrDecoder {
    pub fn new(marker: Vec<u8>, maximum: usize) -> Self {
        Self {
            marker,
            buffer: Vec::new(),
            stderr: Vec::new(),
            maximum,
        }
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<Option<Vec<u8>>, ProtocolError> {
        self.buffer.extend_from_slice(bytes);
        if self.buffer.len() > self.maximum.saturating_add(self.marker.len() + 256) {
            return Err(ProtocolError::ResponseTooLarge(self.maximum));
        }
        let Some(index) = find(&self.buffer, &self.marker) else {
            let keep = self.marker.len();
            if self.buffer.len() > keep {
                let count = self.buffer.len() - keep;
                self.stderr.extend(self.buffer.drain(..count));
                if self.stderr.len() > self.maximum {
                    return Err(ProtocolError::ResponseTooLarge(self.maximum));
                }
            }
            return Ok(None);
        };
        self.stderr.extend(self.buffer.drain(..index));
        self.buffer.drain(..self.marker.len());
        if self.buffer.starts_with(b"\r\n") {
            self.buffer.drain(..2);
        } else if self.buffer.starts_with(b"\n") || self.buffer.starts_with(b"\r") {
            self.buffer.drain(..1);
        }
        trim_trailing_newline(&mut self.stderr);
        if self.stderr.len() > self.maximum {
            return Err(ProtocolError::ResponseTooLarge(self.maximum));
        }
        Ok(Some(std::mem::take(&mut self.stderr)))
    }
}

#[derive(Debug)]
pub struct ShellDecoder {
    markers: ShellMarkers,
    buffer: Vec<u8>,
    stdout: Vec<u8>,
    started: bool,
    separator_pending: bool,
    maximum: usize,
}

impl ShellDecoder {
    pub fn new(markers: ShellMarkers, maximum: usize) -> Self {
        Self {
            markers,
            buffer: Vec::new(),
            stdout: Vec::new(),
            started: false,
            separator_pending: false,
            maximum,
        }
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<ShellEvent>, ProtocolError> {
        self.buffer.extend_from_slice(bytes);
        if self.buffer.len()
            > self
                .maximum
                .saturating_add(self.markers.begin.len() * 2 + self.markers.end.len() + 256)
        {
            return Err(ProtocolError::ResponseTooLarge(self.maximum));
        }
        let mut events = Vec::new();
        if !self.started {
            let Some(index) = find(&self.buffer, &self.markers.begin) else {
                return Ok(events);
            };
            let after = index + self.markers.begin.len();
            self.buffer.drain(..after);
            self.started = true;
            self.separator_pending = true;
        }
        if self.separator_pending {
            if self.buffer.is_empty() || (self.buffer[0] == b'\r' && self.buffer.len() == 1) {
                return Ok(events);
            }
            if self.buffer.starts_with(b"\r\n") {
                self.buffer.drain(..2);
            } else if self.buffer.starts_with(b"\n") || self.buffer.starts_with(b"\r") {
                self.buffer.drain(..1);
            }
            self.separator_pending = false;
        }
        let end = [self.markers.end.as_slice(), b":"].concat();
        let Some(index) = find(&self.buffer, &end) else {
            let keep = end.len() + 8;
            if self.buffer.len() > keep {
                let count = self.buffer.len() - keep;
                self.stdout.extend(self.buffer.drain(..count));
                if self.stdout.len() > self.maximum {
                    return Err(ProtocolError::ResponseTooLarge(self.maximum));
                }
            }
            return Ok(events);
        };
        let code_start = index + end.len();
        let Some(relative_line_end) = self.buffer[code_start..]
            .iter()
            .position(|&byte| byte == b'\n')
        else {
            return Ok(events);
        };
        let line_end = relative_line_end;
        self.stdout.extend(self.buffer.drain(..index));
        trim_trailing_newline(&mut self.stdout);
        self.buffer.drain(..end.len());
        let code_bytes = self.buffer.drain(..line_end).collect::<Vec<_>>();
        if self.buffer.starts_with(b"\n") {
            self.buffer.drain(..1);
        }
        let code = String::from_utf8(code_bytes)
            .map_err(|_| ProtocolError::ShellProtocol("non-UTF-8 exit code".into()))?;
        let exit_code = code
            .trim()
            .parse::<i32>()
            .map_err(|_| ProtocolError::ShellProtocol("invalid exit code".into()))?;
        events.push(ShellEvent::Complete {
            stdout: std::mem::take(&mut self.stdout),
            exit_code,
        });
        Ok(events)
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
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
    fn completion_uses_unique_marker_not_prompt_text() {
        let markers = ShellMarkers::new("abc", 7);
        let wire = b"noise\n__PMAC_BEGIN_abc_7__\r\nroot@device# fake\n__PMAC_END_abc_7__:42\r\n";
        let mut decoder = ShellDecoder::new(markers, 1000);
        let mut events = Vec::new();
        for byte in wire {
            events.extend(decoder.push(&[*byte]).unwrap());
        }
        assert_eq!(
            events,
            vec![ShellEvent::Complete {
                stdout: b"root@device# fake".to_vec(),
                exit_code: 42
            }]
        );
    }

    #[cfg(unix)]
    #[test]
    fn generated_frames_execute_in_posix_shell_with_semicolons_comments_and_state() {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let mut process = Command::new("/bin/sh")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let commands = [
            "PMAC_FRAME_VALUE=retained",
            "printf '%s' \"$PMAC_FRAME_VALUE\";",
            ": # trailing comment",
        ];
        let markers = (0..commands.len())
            .map(|index| ShellMarkers::new("shell_test", index as u64))
            .collect::<Vec<_>>();
        {
            let mut stdin = process.stdin.take().unwrap();
            for (marker, command) in markers.iter().zip(commands) {
                stdin.write_all(&marker.command(command).unwrap()).unwrap();
            }
            stdin.write_all(b"exit\n").unwrap();
        }
        let output = process.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let expected = [b"".as_slice(), b"retained".as_slice(), b"".as_slice()];
        let mut stderr_offset = 0;
        for (marker, expected) in markers.into_iter().zip(expected) {
            let mut stderr_decoder = ShellStderrDecoder::new(marker.stderr_end.clone(), 2048);
            let mut stderr = None;
            for byte in &output.stderr[stderr_offset..] {
                stderr_offset += 1;
                if let Some(completed) = stderr_decoder.push(&[*byte]).unwrap() {
                    stderr = Some(completed);
                    break;
                }
            }
            assert_eq!(stderr.as_deref(), Some(b"".as_slice()));
            let events = ShellDecoder::new(marker, 2048)
                .push(&output.stdout)
                .unwrap();
            assert_eq!(
                events,
                vec![ShellEvent::Complete {
                    stdout: expected.to_vec(),
                    exit_code: 0
                }]
            );
        }
    }

    #[test]
    fn stderr_completion_is_incremental_and_excludes_its_marker() {
        let markers = ShellMarkers::new("abc", 8);
        let mut decoder = ShellStderrDecoder::new(markers.stderr_end, 1000);
        let mut completed = None;
        for byte in b"first\r\n__PMAC_STDERR_END_abc_8__" {
            if let Some(stderr) = decoder.push(&[*byte]).unwrap() {
                completed = Some(stderr);
            }
        }
        assert_eq!(completed.as_deref(), Some(b"first".as_slice()));
        assert!(decoder.buffer.is_empty());
    }
}
