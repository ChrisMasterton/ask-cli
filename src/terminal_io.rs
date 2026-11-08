//! Terminal selection, concurrent stream draining, bounded retention, and
//! terminal-control decoding. Capture errors are resolved after the child waits.

use std::io::{self, IsTerminal, Read, Write};
use std::process::{Child, ChildStderr, ChildStdout, Command, Stdio};
use std::thread::{self, JoinHandle};

pub(crate) const MAX_CAPTURE_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy)]
pub(crate) enum InputMode {
    Inherited,
    Closed,
}

#[derive(Clone, Copy)]
pub(crate) enum IoMode {
    Inherited,
    Terminal,
    Captured(InputMode),
}

impl IoMode {
    pub(crate) fn for_shell() -> Self {
        let stdin_is_terminal = io::stdin().is_terminal();
        if stdin_is_terminal && io::stdout().is_terminal() && io::stderr().is_terminal() {
            Self::Terminal
        } else {
            Self::Captured(if stdin_is_terminal {
                InputMode::Inherited
            } else {
                InputMode::Closed
            })
        }
    }

    pub(crate) fn configure(self, command: &mut Command) {
        match self {
            Self::Inherited => {
                command
                    .stdin(Stdio::inherit())
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::inherit());
            }
            Self::Terminal => {
                command
                    .stdin(Stdio::inherit())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::inherit());
            }
            Self::Captured(input) => {
                command
                    .stdin(match input {
                        InputMode::Inherited => Stdio::inherit(),
                        InputMode::Closed => Stdio::null(),
                    })
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
            }
        }
    }
}

pub(crate) enum CapturedOutput {
    Inherited,
    Terminal(io::Result<String>),
    Streams {
        stdout: io::Result<String>,
        stderr: thread::Result<io::Result<String>>,
    },
}

impl CapturedOutput {
    pub(crate) fn into_result(self) -> io::Result<String> {
        match self {
            Self::Inherited => Ok(String::new()),
            Self::Terminal(result) => result,
            Self::Streams { stdout, stderr } => {
                let mut output = stdout?;
                let stderr = stderr.map_err(|_| io::Error::other("stderr capture failed"))??;
                if !stderr.is_empty() {
                    if !output.is_empty() {
                        output.push('\n');
                    }
                    output.push_str(&stderr);
                }
                Ok(output)
            }
        }
    }
}

enum CaptureState {
    Terminal(ChildStdout),
    Streams {
        stdout: ChildStdout,
        stderr: ChildStderr,
    },
    Joining {
        stdout: io::Result<String>,
        stderr: JoinHandle<io::Result<String>>,
    },
    Complete(CapturedOutput),
}

pub(crate) fn capture_child_output(child: &mut Child, mode: IoMode) -> CapturedOutput {
    let mut state = match mode {
        IoMode::Inherited => CaptureState::Complete(CapturedOutput::Inherited),
        IoMode::Terminal => CaptureState::Terminal(child.stdout.take().expect("piped stdout")),
        IoMode::Captured(_) => CaptureState::Streams {
            stdout: child.stdout.take().expect("piped stdout"),
            stderr: child.stderr.take().expect("piped stderr"),
        },
    };
    loop {
        state = match state {
            CaptureState::Terminal(reader) => CaptureState::Complete(CapturedOutput::Terminal(
                capture_output(reader, io::stdout()).map(|output| clean_terminal_output(&output)),
            )),
            CaptureState::Streams { stdout, stderr } => {
                let stderr = thread::spawn(move || capture_output(stderr, io::stderr()));
                let stdout = capture_output(stdout, io::stdout());
                CaptureState::Joining { stdout, stderr }
            }
            CaptureState::Joining { stdout, stderr } => {
                CaptureState::Complete(CapturedOutput::Streams {
                    stdout,
                    stderr: stderr.join(),
                })
            }
            CaptureState::Complete(output) => return output,
        };
    }
}

enum StreamState {
    Reading,
    Forwarding(usize),
    Flushing(usize),
    Retaining(usize),
    Complete,
    Failed(io::Error),
}

enum Retention {
    Whole,
    Tail,
}

pub(crate) fn capture_output(mut reader: impl Read, mut writer: impl Write) -> io::Result<String> {
    let mut captured = Vec::new();
    let mut retention = Retention::Whole;
    let mut buffer = [0u8; 8192];
    let mut state = StreamState::Reading;
    loop {
        state = match state {
            StreamState::Reading => match reader.read(&mut buffer) {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => StreamState::Reading,
                Err(error) => StreamState::Failed(error),
                Ok(0) => StreamState::Complete,
                Ok(size) => StreamState::Forwarding(size),
            },
            StreamState::Forwarding(size) => match writer.write_all(&buffer[..size]) {
                Ok(()) => StreamState::Flushing(size),
                Err(error) => StreamState::Failed(error),
            },
            StreamState::Flushing(size) => match writer.flush() {
                Ok(()) => StreamState::Retaining(size),
                Err(error) => StreamState::Failed(error),
            },
            StreamState::Retaining(size) => {
                captured.extend_from_slice(&buffer[..size]);
                if captured.len() > MAX_CAPTURE_BYTES {
                    captured.drain(..captured.len() - MAX_CAPTURE_BYTES);
                    retention = Retention::Tail;
                }
                StreamState::Reading
            }
            StreamState::Complete => {
                let output = String::from_utf8_lossy(&captured);
                return Ok(match retention {
                    Retention::Whole => output.into_owned(),
                    Retention::Tail => format!("... (earlier output truncated)\n{output}"),
                });
            }
            StreamState::Failed(error) => return Err(error),
        };
    }
}

enum TerminalTextState {
    Text,
    Escape,
    Csi,
    Osc,
    OscEscape,
}

pub(crate) fn clean_terminal_output(value: &str) -> String {
    let mut cleaned = String::new();
    let mut chars = value.chars().peekable();
    let mut state = TerminalTextState::Text;
    while let Some(ch) = chars.next() {
        state = match state {
            TerminalTextState::Text if ch == '\u{1b}' => TerminalTextState::Escape,
            TerminalTextState::Text => {
                match ch {
                    '\r' if chars.peek() != Some(&'\n') => cleaned.push('\n'),
                    _ if !ch.is_control() || ch == '\n' || ch == '\t' => cleaned.push(ch),
                    _ => {}
                }
                TerminalTextState::Text
            }
            TerminalTextState::Escape => match ch {
                '[' => TerminalTextState::Csi,
                ']' => TerminalTextState::Osc,
                _ => TerminalTextState::Text,
            },
            TerminalTextState::Csi => {
                if ('@'..='~').contains(&ch) {
                    TerminalTextState::Text
                } else {
                    TerminalTextState::Csi
                }
            }
            TerminalTextState::Osc => match ch {
                '\u{7}' => TerminalTextState::Text,
                '\u{1b}' => TerminalTextState::OscEscape,
                _ => TerminalTextState::Osc,
            },
            TerminalTextState::OscEscape => {
                if ch == '\\' {
                    TerminalTextState::Text
                } else {
                    TerminalTextState::Osc
                }
            }
        };
    }
    cleaned
}

/// Preserve final diagnostics rather than the beginning of a long build log.
pub(crate) fn output_tail(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let mut start = value.len() - max_bytes;
    while !value.is_char_boundary(start) {
        start += 1;
    }
    format!("... (earlier output truncated)\n{}", &value[start..])
}

#[cfg(test)]
mod tests {
    use super::*;

    struct InterruptedOnce<R> {
        inner: R,
        interrupted: bool,
    }

    impl<R: Read> Read for InterruptedOnce<R> {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if !self.interrupted {
                self.interrupted = true;
                Err(io::ErrorKind::Interrupted.into())
            } else {
                self.inner.read(buffer)
            }
        }
    }

    struct ReadFailure;

    impl Read for ReadFailure {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("synthetic read failure"))
        }
    }

    struct FailingWriter {
        fail_on_flush: bool,
        visible: Vec<u8>,
    }

    impl Write for FailingWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.fail_on_flush {
                self.visible.extend_from_slice(bytes);
                Ok(bytes.len())
            } else {
                Err(io::ErrorKind::BrokenPipe.into())
            }
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
    }

    #[test]
    fn interrupted_reads_retry_without_repeating_visible_output() {
        let bytes = "café\n".as_bytes();
        let reader = InterruptedOnce {
            inner: bytes,
            interrupted: false,
        };
        let mut visible = Vec::new();
        assert_eq!(capture_output(reader, &mut visible).unwrap(), "café\n");
        assert_eq!(visible, bytes);
    }

    #[test]
    fn stream_errors_preserve_already_visible_bytes_and_never_return_success() {
        let mut visible = Vec::new();
        let error =
            capture_output(b"visible".as_slice().chain(ReadFailure), &mut visible).unwrap_err();
        assert_eq!(error.to_string(), "synthetic read failure");
        assert_eq!(visible, b"visible");

        for fail_on_flush in [false, true] {
            let mut writer = FailingWriter {
                fail_on_flush,
                visible: Vec::new(),
            };
            let error = capture_output(b"chunk".as_slice(), &mut writer).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
            assert_eq!(
                writer.visible,
                if fail_on_flush {
                    b"chunk".as_slice()
                } else {
                    b""
                }
            );
        }
    }

    #[test]
    fn separate_streams_keep_stdout_then_stderr_and_stdout_error_precedence() {
        for (stdout, stderr, expected) in [
            ("out", "err", "out\nerr"),
            ("", "err", "err"),
            ("out", "", "out"),
            ("", "", ""),
        ] {
            let output = CapturedOutput::Streams {
                stdout: Ok(stdout.into()),
                stderr: Ok(Ok(stderr.into())),
            }
            .into_result()
            .unwrap();
            assert_eq!(output, expected);
        }
        let error = CapturedOutput::Streams {
            stdout: Err(io::Error::other("stdout failed")),
            stderr: Err(Box::new("stderr panicked")),
        }
        .into_result()
        .unwrap_err();
        assert_eq!(error.to_string(), "stdout failed");
        let error = CapturedOutput::Streams {
            stdout: Ok("out".into()),
            stderr: Err(Box::new("stderr panicked")),
        }
        .into_result()
        .unwrap_err();
        assert_eq!(error.to_string(), "stderr capture failed");
    }

    #[test]
    fn terminal_decoding_keeps_escape_and_incomplete_sequence_behavior() {
        for (input, expected) in [
            ("\x1b]title\x1b\\visible", "visible"),
            ("a\x1b[31", "a"),
            ("a\x1b]unfinished", "a"),
            ("a\x1bXb", "ab"),
            ("a\r\nb\rc\n\t", "a\nb\nc\n\t"),
            ("\x1b]title\x1bXignored\x07visible", "visible"),
            ("\x1b]title\x1b\x07ignored\x07visible", "visible"),
            ("\x1b]title\x1b\x1b\\ignored\x07visible", "visible"),
        ] {
            assert_eq!(clean_terminal_output(input), expected, "{input:?}");
        }
    }
}
