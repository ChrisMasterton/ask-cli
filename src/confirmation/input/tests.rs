use super::*;
use std::cell::RefCell;
use std::io::{BufRead, Cursor};
use std::rc::Rc;

struct ObservedTerminal {
    data: Cursor<Vec<u8>>,
    events: Rc<RefCell<Vec<&'static str>>>,
}

impl Read for ObservedTerminal {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.events.borrow_mut().push("read");
        self.data.read(buffer)
    }
}

impl Drop for ObservedTerminal {
    fn drop(&mut self) {
        self.events.borrow_mut().push("close");
    }
}

#[test]
fn terminal_is_opened_flushed_read_and_closed_without_reading_stdin() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut terminal = Some(ObservedTerminal {
        data: Cursor::new(b"yes\n".to_vec()),
        events: events.clone(),
    });
    let line = read_with(
        || {
            events.borrow_mut().push("open");
            Ok(terminal.take().expect("open once"))
        },
        |_| events.borrow_mut().push("flush"),
        |_| panic!("a controlling terminal takes precedence over stdin"),
    )
    .unwrap();
    assert_eq!(line, "yes");
    assert_eq!(*events.borrow(), ["open", "flush", "read", "close"]);
}

#[test]
fn terminal_delimiters_partial_eof_and_byte_decoding_keep_their_existing_rules() {
    for (bytes, expected) in [
        (&b"\n"[..], ""),
        (&b"\r"[..], ""),
        (&b"yes\rignored"[..], "yes"),
        (&b"no\nignored"[..], "no"),
        (&b"skip"[..], "skip"),
        (&b"\xff\n"[..], "\u{ff}"),
        (&b"\xc3\xa9"[..], "\u{c3}\u{a9}"),
    ] {
        let line = read_with(|| Ok(Cursor::new(bytes)), |_| {}, |_| panic!("no fallback")).unwrap();
        assert_eq!(line, expected);
    }
    let error = read_with(
        || Ok(Cursor::new(b"")),
        |_| {},
        |_| panic!("EOF is not an open failure"),
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    assert_eq!(
        error.to_string(),
        "No confirmation received before input closed"
    );
}

#[test]
fn any_terminal_open_error_falls_back_to_one_native_stdin_line() {
    for kind in [
        io::ErrorKind::NotFound,
        io::ErrorKind::PermissionDenied,
        io::ErrorKind::Interrupted,
    ] {
        let mut input = Cursor::new("YES é\r\nremaining".as_bytes());
        let line = read_with(
            || Err::<Cursor<Vec<u8>>, _>(io::Error::new(kind, "terminal unavailable")),
            |_| panic!("unopened terminal must not flush"),
            |line| input.read_line(line),
        )
        .unwrap();
        assert_eq!(line, "YES é\r\n");
        let mut remaining = String::new();
        input.read_to_string(&mut remaining).unwrap();
        assert_eq!(remaining, "remaining");
    }
}

#[test]
fn stdin_distinguishes_empty_lines_partial_eof_invalid_utf8_and_no_input() {
    for (bytes, expected) in [(&b"\n"[..], "\n"), (&b"\r"[..], "\r"), (&b"yes"[..], "yes")] {
        let mut input = Cursor::new(bytes);
        let line = read_with(
            || Err::<Cursor<Vec<u8>>, _>(io::ErrorKind::NotFound.into()),
            |_| {},
            |line| input.read_line(line),
        )
        .unwrap();
        assert_eq!(line, expected);
    }
    for (bytes, expected_kind) in [
        (&b""[..], io::ErrorKind::UnexpectedEof),
        (&b"\xff\n"[..], io::ErrorKind::InvalidData),
    ] {
        let mut input = Cursor::new(bytes);
        let error = read_with(
            || Err::<Cursor<Vec<u8>>, _>(io::ErrorKind::NotFound.into()),
            |_| {},
            |line| input.read_line(line),
        )
        .unwrap_err();
        assert_eq!(error.kind(), expected_kind);
    }
}

struct FailingTerminal(Cursor<Vec<u8>>, io::ErrorKind);

impl Read for FailingTerminal {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.0.position() == self.0.get_ref().len() as u64 {
            Err(io::Error::new(self.1, "terminal read failed"))
        } else {
            self.0.read(buffer)
        }
    }
}

#[test]
fn terminal_read_errors_reject_partial_input_and_never_fall_back_or_retry() {
    for kind in [io::ErrorKind::Interrupted, io::ErrorKind::BrokenPipe] {
        let mut terminal = Some(FailingTerminal(Cursor::new(b"yes".to_vec()), kind));
        let error = read_with(
            || Ok(terminal.take().unwrap()),
            |_| {},
            |_| panic!("read failure must not fall back to stdin"),
        )
        .unwrap_err();
        assert_eq!(error.kind(), kind);
        assert_eq!(error.to_string(), "terminal read failed");
    }
}

#[test]
fn stdin_read_errors_reject_partial_input_without_changing_the_error() {
    let error = read_with(
        || Err::<Cursor<Vec<u8>>, _>(io::ErrorKind::NotFound.into()),
        |_| {},
        |line| {
            line.push_str("yes");
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "stdin failed"))
        },
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    assert_eq!(error.to_string(), "stdin failed");
}
