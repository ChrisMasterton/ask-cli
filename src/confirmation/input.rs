//! Acquire one fresh confirmation line. Opening the controlling terminal can
//! fall back to stdin; a terminal read failure cannot. Input policy is applied
//! by the parent confirmation machine after this acquisition completes.

use std::fs;
use std::io::{self, Read};
use std::os::unix::io::AsRawFd;

enum InputState<T> {
    OpeningTerminal,
    FlushingTerminal(T),
    ReadingTerminal {
        reader: io::BufReader<T>,
        input: String,
    },
    ReadingStdin,
    TerminalEof(String),
    Complete(String),
    Failed(io::Error),
}

pub(super) fn read_confirmation_line() -> io::Result<String> {
    read_with(
        || fs::OpenOptions::new().read(true).open("/dev/tty"),
        |tty| {
            // Discard stale rustyline input. The existing flush failure policy
            // is best effort, so its return code does not change the source.
            unsafe {
                libc::tcflush(tty.as_raw_fd(), libc::TCIFLUSH);
            }
        },
        |input| io::stdin().read_line(input),
    )
}

fn read_with<T: Read>(
    mut open_terminal: impl FnMut() -> io::Result<T>,
    mut flush_terminal: impl FnMut(&T),
    mut read_stdin: impl FnMut(&mut String) -> io::Result<usize>,
) -> io::Result<String> {
    let mut state = InputState::OpeningTerminal;
    loop {
        state = match state {
            InputState::OpeningTerminal => match open_terminal() {
                Ok(terminal) => InputState::FlushingTerminal(terminal),
                Err(_) => InputState::ReadingStdin,
            },
            InputState::FlushingTerminal(terminal) => {
                flush_terminal(&terminal);
                InputState::ReadingTerminal {
                    reader: io::BufReader::new(terminal),
                    input: String::new(),
                }
            }
            InputState::ReadingTerminal {
                mut reader,
                mut input,
            } => {
                let mut byte = [0u8; 1];
                match reader.read(&mut byte) {
                    Ok(0) => InputState::TerminalEof(input),
                    // rustyline can leave ICRNL unset. Consume either delimiter,
                    // retaining the terminal reader's existing byte-to-char rule.
                    Ok(_) if byte[0] == b'\n' || byte[0] == b'\r' => InputState::Complete(input),
                    Ok(_) => {
                        input.push(byte[0] as char);
                        InputState::ReadingTerminal { reader, input }
                    }
                    Err(error) => InputState::Failed(error),
                }
            }
            InputState::ReadingStdin => {
                let mut input = String::new();
                match read_stdin(&mut input) {
                    Ok(0) => InputState::Failed(no_confirmation()),
                    Ok(_) => InputState::Complete(input),
                    Err(error) => InputState::Failed(error),
                }
            }
            InputState::TerminalEof(input) => {
                if input.is_empty() {
                    InputState::Failed(no_confirmation())
                } else {
                    InputState::Complete(input)
                }
            }
            InputState::Complete(input) => return Ok(input),
            InputState::Failed(error) => return Err(error),
        };
    }
}

fn no_confirmation() -> io::Error {
    io::Error::new(
        io::ErrorKind::UnexpectedEof,
        "No confirmation received before input closed",
    )
}

#[cfg(test)]
mod tests;
