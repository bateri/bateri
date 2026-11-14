use std::os::fd::RawFd;
use std::thread;

use super::*;

/// A pseudo-terminal: `(master, slave)`.
fn terminal() -> (RawFd, RawFd) {
    let (mut master, mut slave) = (0, 0);
    // SAFETY: both descriptors are written by the call, the rest may be null.
    let made = unsafe {
        libc::openpty(
            &raw mut master,
            &raw mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(made, 0, "openpty");
    (master, slave)
}

fn type_on(master: RawFd, bytes: &[u8]) {
    // SAFETY: `bytes` is a readable slice and `master` an open descriptor.
    let written = unsafe { libc::write(master, bytes.as_ptr().cast(), bytes.len()) };
    assert_eq!(written, bytes.len() as isize);
}

fn close(fd: RawFd) {
    // SAFETY: the test owns the descriptor.
    unsafe { libc::close(fd) };
}

/// What the terminal's other end has read, until `marker` is in it.
fn read_until(master: RawFd, marker: &str) -> String {
    let mut seen = Vec::new();
    while !String::from_utf8_lossy(&seen).contains(marker) {
        let mut chunk = [0u8; 256];
        // SAFETY: `chunk` is writable for its length and `master` is open.
        let read = unsafe { libc::read(master, chunk.as_mut_ptr().cast(), chunk.len()) };
        assert!(read > 0, "the terminal ended before {marker:?}: {seen:?}");
        seen.extend_from_slice(&chunk[..read.unsigned_abs()]);
    }
    String::from_utf8_lossy(&seen).into_owned()
}

/// The question on a terminal's own output: a writer to its slave side.
fn on_terminal(slave: RawFd) -> std::fs::File {
    // SAFETY: `dup` gives a descriptor of its own, which the `File` owns.
    let copy = unsafe { libc::dup(slave) };
    assert!(copy >= 0, "dup");
    // SAFETY: `copy` is open and nothing else owns it.
    unsafe { std::os::fd::FromRawFd::from_raw_fd(copy) }
}

/// Asks on a fresh terminal with `before` typed on it ahead of the question and `after` typed
/// once the question is on screen: what the process ended with and what the person saw.
fn asked(before: &[u8], after: &[u8]) -> (u8, String) {
    let (master, slave) = terminal();
    type_on(master, before);
    let handle = thread::spawn(move || {
        let mut out = on_terminal(slave);
        ask_on(slave, "deploy@prod", &mut out)
    });
    // The answer is typed when the question has been shown, which is after what was typed
    // ahead has been thrown away: nothing is waited for by the clock.
    let mut seen = read_until(master, "[a]lways ");
    type_on(master, after);
    let status = handle.join().expect("the thread ends");
    // What is on the terminal after the answer: the word and the line's end, if there is one.
    if status != INTERRUPTED {
        seen.push_str(&read_until(master, "\r\n"));
    }
    close(master);
    close(slave);
    (status, seen)
}

#[test]
fn every_key_answers_with_its_status() {
    for (key, status) in [
        (b'\r', YES),
        (b'\n', YES),
        (b'y', YES),
        (b'Y', YES),
        (b'n', NO),
        (b'N', NO),
        (b'a', ALWAYS),
        (b'A', ALWAYS),
        (0x03, INTERRUPTED),
        (0x04, INTERRUPTED),
    ] {
        assert_eq!(status_of_key(key), Some(status), "{key:#x}");
    }
    for key in [b' ', b'x', b'1', 0x1b, b'q'] {
        assert_eq!(status_of_key(key), None, "{key:#x}");
    }
}

#[test]
fn the_status_is_what_the_service_reads_back() {
    assert_eq!(Reply::of_status(YES).ok(), Some(Reply::Yes));
    assert_eq!(Reply::of_status(NO).ok(), Some(Reply::No));
    assert_eq!(Reply::of_status(ALWAYS).ok(), Some(Reply::Always));
    // Control-C and a signal end the connection and show the person what they saw.
    for status in [INTERRUPTED, 129, 143, 255] {
        assert_eq!(
            Reply::of_status(status).err(),
            Some(Refusal::ended("")),
            "{status}"
        );
    }
    // Anything else is a program that did not run as it should: the plain `ssh`.
    for status in [3, 64, 78, 127] {
        assert_eq!(
            Reply::of_status(status).err(),
            Some(Refusal::plain()),
            "{status}"
        );
    }
}

#[test]
fn the_question_names_the_machine_and_what_is_put_there() {
    assert_eq!(
        question("deploy@prod"),
        "Keep this terminal running on prod if the connection drops? bateri puts one folder \
         there (~/.local/share/bateri). [Y]es / [n]o / [a]lways "
    );
    assert!(question("prod").contains("on prod if"));
}

#[test]
fn a_key_on_the_terminal_answers_and_is_not_echoed() {
    for (typed, status, word) in [
        (&b"\r"[..], YES, "yes"),
        (b"y", YES, "yes"),
        (b"n", NO, "no"),
        (b"a", ALWAYS, "always"),
        (b"\x03", INTERRUPTED, ""),
    ] {
        let (code, seen) = asked(b"", typed);
        assert_eq!(code, status, "{typed:?}");
        assert!(
            seen.starts_with("Keep this terminal running on prod"),
            "{seen:?}"
        );
        assert!(
            seen.ends_with("[a]lways ") || seen.ends_with(&format!("{word}\r\n")),
            "the key is not echoed: {seen:?}"
        );
        assert!(
            !seen.contains(&format!("always {}", typed[0] as char)),
            "{seen:?}"
        );
    }
}

#[test]
fn a_key_that_answers_nothing_is_passed_over() {
    let (code, _) = asked(b"", b"xq 1\x1bn");
    assert_eq!(code, NO, "the first key that answers is the answer");
}

#[test]
fn what_was_typed_before_the_question_does_not_answer_it() {
    // An Enter hit while the connection started is not a yes.
    let (code, _) = asked(b"\r\r\r", b"n");
    assert_eq!(code, NO);
}

#[test]
fn no_terminal_and_no_input_are_an_interrupt() {
    // Not a terminal: nobody to answer.
    let mut shown = Vec::new();
    let null = std::fs::File::open("/dev/null").expect("/dev/null");
    assert_eq!(ask_on(null.as_raw_fd(), "prod", &mut shown), INTERRUPTED);
    assert!(shown.is_empty(), "nothing is asked of no one");
    // The terminal's other end gone: the read ends.
    let (master, slave) = terminal();
    let handle = thread::spawn(move || {
        let mut out = on_terminal(slave);
        ask_on(slave, "prod", &mut out)
    });
    read_until(master, "[a]lways ");
    close(master);
    assert_eq!(handle.join().expect("the thread ends"), INTERRUPTED);
    close(slave);
}

#[test]
fn the_terminal_is_put_back_as_it_was() {
    let (master, slave) = terminal();
    let mode = |fd: RawFd| {
        // SAFETY: a zeroed `termios` is plain integers, `tcgetattr` fills it in.
        let mut termios: libc::termios = unsafe { std::mem::zeroed() };
        // SAFETY: `termios` is valid and `fd` is open.
        assert_eq!(unsafe { libc::tcgetattr(fd, &raw mut termios) }, 0);
        // PENDIN is the kernel's own mark that input was pending when the mode changed.
        termios.c_lflag & !libc::PENDIN
    };
    let before = mode(slave);
    let handle = thread::spawn(move || {
        let mut out = on_terminal(slave);
        ask_on(slave, "prod", &mut out)
    });
    read_until(master, "[a]lways ");
    type_on(master, b"n");
    assert_eq!(handle.join().expect("the thread ends"), NO);
    assert_eq!(mode(slave), before);
    close(master);
    close(slave);
}
