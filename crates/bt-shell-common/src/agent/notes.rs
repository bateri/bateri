//! What the script on a server left for the Mac while it was away, and how the Mac reads it.
//!
//! `bateri split` on a server with no Mac connected cannot ask where the pane goes. The terminal
//! starts anyway and the same `place` message the Mac would have been asked waits in a file,
//! `~/.local/share/bateri/placing/tN` (the script's own state, under `$HOME`: the Mac's `sh -c` and
//! a cron job have no login shell's environment). When a connection to the server comes up and the
//! server has said what it keeps, the Mac reads the files in one `sh` run on the account's main
//! connection ([`READ`]) and **removes them as it reads**: a note is read once, and a terminal that
//! ended or that somebody else took by then is simply not placed (the terminal stays in the list of
//! what runs in the background, which is where a terminal with no pane is found anyway).
//!
//! A note is a request like any other and is read like one ([`Request::parse`], strictly): it can
//! ask for a pane for the server it is on, and nothing else — no program, no directory, no byte for
//! a shell. The only file names that are notes are `t` and a number; `tN.tmp.PID` is a note being
//! written.
//!
//! `~/.local/share/bateri/terminals/tN` says who opened a terminal and whether it stays open when
//! its program ends ([`Record`]); the same script writes it, and `ls` ([`RECORDS`] reads them all,
//! leaving them) and `close` (who may close a terminal without asking) read it.

use super::message::{MAX_BYTES, Place, Request};
use super::{Anchor, Terminal};

/// The script that reads and removes the notes: run with `sh -c` on the server, it prints each
/// note as a header line, `bateri-note tN LENGTH`, then exactly LENGTH bytes and a newline. The
/// length frames the note (what is in it is anybody's text, a login shell may say words of its own
/// before the first header), a note over [`MAX_BYTES`] is removed unread, and a link is no note.
pub const READ: &str = r#"dir="$HOME/.local/share/bateri/placing"
[ -d "$dir" ] || exit 0
for file in "$dir"/t*; do
  [ -f "$file" ] && [ ! -L "$file" ] || continue
  name=${file##*/}
  case ${name#t} in
    '' | *[!0-9]*) continue ;;
  esac
  size=$(wc -c < "$file" 2>/dev/null) || continue
  size=$(( ${size:-0} + 0 ))
  if [ "$size" -gt 8192 ]; then
    rm -f "$file"
    continue
  fi
  printf 'bateri-note %s %s\n' "$name" "$size"
  cat "$file" && rm -f "$file"
  printf '\n'
done
exit 0
"#;

/// The script that reads who opened what: run with `sh -c` on the server, it prints every
/// `terminals/tN` record the way [`READ`] prints a note — `bateri-record tN LENGTH`, the bytes, a
/// newline — and removes nothing. A record over 1 KiB is none (the script writes two short lines).
pub const RECORDS: &str = r#"dir="$HOME/.local/share/bateri/terminals"
[ -d "$dir" ] || exit 0
for file in "$dir"/t*; do
  [ -f "$file" ] && [ ! -L "$file" ] || continue
  name=${file##*/}
  case ${name#t} in
    '' | *[!0-9]*) continue ;;
  esac
  size=$(wc -c < "$file" 2>/dev/null) || continue
  size=$(( ${size:-0} + 0 ))
  [ "$size" -le 1024 ] || continue
  printf 'bateri-record %s %s\n' "$name" "$size"
  cat "$file"
  printf '\n'
done
exit 0
"#;

/// A note: the pane the Mac was asked for, as the script wrote it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub terminal: Terminal,
    pub place: Place,
}

/// The notes in what [`READ`] printed. A line that is not a header (a login shell's words), a note
/// that is not a `place` for the terminal its file is named after, one that names no anchor (a
/// note is read when nobody names its sender) and one that is not text are left out; so is
/// whatever comes after a note that is shorter than its header says.
pub fn parse(output: &[u8]) -> Vec<Note> {
    frames(output, "bateri-note")
        .into_iter()
        .filter_map(|(terminal, body)| note_of(terminal, body))
        .collect()
}

/// The records in what [`RECORDS`] printed, framed like the notes; a frame that is no record is
/// left out.
pub fn parse_records(output: &[u8]) -> Vec<(Terminal, Record)> {
    frames(output, "bateri-record")
        .into_iter()
        .filter_map(|(terminal, body)| {
            Record::parse(std::str::from_utf8(body).ok()?).map(|record| (terminal, record))
        })
        .collect()
}

/// The frames a script printed under header word `word`: the terminal its file is named for and
/// the bytes of it.
fn frames<'a>(output: &'a [u8], word: &str) -> Vec<(Terminal, &'a [u8])> {
    let mut found = Vec::new();
    let mut rest = output;
    while !rest.is_empty() {
        let (line, after) = match rest.iter().position(|&byte| byte == b'\n') {
            Some(at) => (&rest[..at], &rest[at + 1..]),
            None => (rest, &rest[rest.len()..]),
        };
        rest = after;
        let Some((terminal, size)) = header(line, word) else {
            continue;
        };
        if rest.len() < size {
            break;
        }
        let (body, tail) = rest.split_at(size);
        rest = tail.strip_prefix(b"\n").unwrap_or(tail);
        found.push((terminal, body));
    }
    found
}

fn header(line: &[u8], word: &str) -> Option<(Terminal, usize)> {
    let line = std::str::from_utf8(line).ok()?;
    let mut words = line.split(' ');
    if words.next()? != word {
        return None;
    }
    let terminal = Terminal::parse(words.next()?)?;
    let size: usize = words.next()?.parse().ok()?;
    (words.next().is_none() && size <= MAX_BYTES).then_some((terminal, size))
}

fn note_of(terminal: Terminal, body: &[u8]) -> Option<Note> {
    let Ok(Request::Place(place)) = Request::parse(std::str::from_utf8(body).ok()?) else {
        return None;
    };
    (place.terminal == terminal && place.anchor.is_some()).then_some(Note { terminal, place })
}

/// What `~/.local/share/bateri/terminals/tN` says: plain `key value` lines, the keys the script
/// writes after `kablo new` has made the terminal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    /// The terminal that opened this one, as kablo's `here` named it; none when it was opened from
    /// no terminal (a cron job).
    pub opener: Option<Anchor>,
    /// The pane stays when the program ends.
    pub keep_open: bool,
}

impl Record {
    /// The record `text` is. `None` for a text that holds neither key; a key it does not know is
    /// left alone (a later script may write more), and an `opener` that is no name is none.
    pub fn parse(text: &str) -> Option<Record> {
        let mut found = false;
        let mut record = Record {
            opener: None,
            keep_open: false,
        };
        for line in text.lines() {
            let (key, value) = line.split_once(' ').unwrap_or((line, ""));
            match key {
                "opener" => {
                    found = true;
                    record.opener = (value != "-").then(|| Anchor::parse(value)).flatten();
                }
                "keep-open" => {
                    found = true;
                    record.keep_open = value == "yes";
                }
                _ => {}
            }
        }
        found.then_some(record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::place::Side;
    use crate::settings::TempRoot;
    use std::process::Command;

    fn place_text(terminal: &str, anchor: &str, extra: &str) -> String {
        format!("bateri 1\nplace\nterminal {terminal}\nanchor {anchor}\n{extra}")
    }

    /// Runs [`READ`] with `sh` (and `dash`, the Linux image's) over a home that holds `files`.
    fn read(name: &str, files: &[(&str, &[u8])]) -> (Vec<u8>, TempRoot) {
        let root = TempRoot::new(&format!("agent-notes-{name}"));
        let dir = root.0.join(".local/share/bateri/placing");
        std::fs::create_dir_all(&dir).expect("a directory");
        for (file, bytes) in files {
            std::fs::write(dir.join(file), bytes).expect("a note");
        }
        let mut printed = Vec::new();
        for shell in ["/bin/sh", "dash"] {
            let Ok(output) = Command::new(shell)
                .arg("-c")
                .arg(READ)
                .env("HOME", &root.0)
                .output()
            else {
                continue;
            };
            assert!(output.status.success(), "{shell}");
            if printed.is_empty() {
                printed = output.stdout;
            } else {
                assert!(
                    output.stdout.is_empty(),
                    "{shell} finds nothing the second time"
                );
            }
        }
        (printed, root)
    }

    #[test]
    fn the_script_prints_each_note_with_its_length_and_removes_it() {
        let first = place_text("t7", "t4", "side below\nratio 30\n");
        let second = place_text("t8", "t4", "keep-open yes\n");
        let (printed, root) = read(
            "framed",
            &[
                ("t7", first.as_bytes()),
                ("t8", second.as_bytes()),
                // A note being written, a name that is none, a directory-like name: not notes.
                ("t9.tmp.123", b"half"),
                ("t10x", b"x"),
                ("t", b"x"),
                ("other", b"x"),
            ],
        );
        let text = String::from_utf8(printed.clone()).expect("text");
        assert!(
            text.starts_with(&format!("bateri-note t7 {}\n{first}\n", first.len())),
            "{text:?}"
        );
        let placing = root.0.join(".local/share/bateri/placing");
        assert!(!placing.join("t7").exists() && !placing.join("t8").exists());
        assert!(placing.join("t9.tmp.123").exists() && placing.join("other").exists());
        let notes = parse(&printed);
        assert_eq!(notes.len(), 2, "{notes:?}");
        assert_eq!(notes[0].terminal, Terminal(7));
        assert_eq!(notes[0].place.side, Some(Side::Below));
        assert_eq!(notes[0].place.ratio, Some(30));
        assert_eq!(notes[1].terminal, Terminal(8));
        assert!(notes[1].place.keep_open);
        assert_eq!(notes[1].place.anchor.as_ref().map(Anchor::name), Some("t4"));
    }

    #[test]
    fn a_note_of_a_size_no_request_has_is_removed_unread() {
        let big = vec![b'x'; 9000];
        let ok = place_text("t2", "t1", "");
        let (printed, root) = read("big", &[("t1", &big), ("t2", ok.as_bytes())]);
        let notes = parse(&printed);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].terminal, Terminal(2));
        assert!(
            !root.0.join(".local/share/bateri/placing/t1").exists(),
            "removed, not printed"
        );
    }

    #[test]
    fn no_directory_and_no_home_print_nothing_and_succeed() {
        let root = TempRoot::new("agent-notes-none");
        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(READ)
            .env("HOME", &root.0)
            .output()
            .expect("sh");
        assert!(output.status.success() && output.stdout.is_empty());
    }

    #[test]
    fn what_a_login_shell_says_and_a_bad_note_are_left_out_and_the_good_one_after_them_is_read() {
        let good = place_text("t5", "t4", "");
        let wrong_name = place_text("t6", "t4", "");
        let no_anchor = "bateri 1\nplace\nterminal t7\n";
        let ls = "bateri 1\nls\n";
        let mut out = b"Welcome to rasp\nbateri-note oops\n".to_vec();
        for (name, body) in [
            ("t5", good.as_str()),
            ("t9", wrong_name.as_str()),
            ("t7", no_anchor),
            ("t8", ls),
        ] {
            out.extend(format!("bateri-note {name} {}\n{body}\n", body.len()).bytes());
        }
        out.extend(b"bateri-note t5 99999999\nrest");
        out.extend(b"bateri-note t3 5\nabc");
        let notes = parse(&out);
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert_eq!(notes[0].terminal, Terminal(5));
        // Not text.
        assert!(parse(b"bateri-note t5 2\n\xff\xfe\n").is_empty());
        assert!(parse(b"").is_empty());
    }

    /// Runs [`RECORDS`] with `sh` (and `dash`) over a home that holds `files` in `terminals/`; both
    /// print the same, and nothing is removed.
    fn records_of(name: &str, files: &[(&str, &[u8])]) -> (Vec<u8>, TempRoot) {
        let root = TempRoot::new(&format!("agent-records-{name}"));
        let dir = root.0.join(".local/share/bateri/terminals");
        std::fs::create_dir_all(&dir).expect("a directory");
        for (file, bytes) in files {
            std::fs::write(dir.join(file), bytes).expect("a record");
        }
        let mut printed = Vec::new();
        for shell in ["/bin/sh", "dash"] {
            let Ok(output) = Command::new(shell)
                .arg("-c")
                .arg(RECORDS)
                .env("HOME", &root.0)
                .output()
            else {
                continue;
            };
            assert!(output.status.success(), "{shell}");
            if printed.is_empty() {
                printed = output.stdout;
            } else {
                assert_eq!(output.stdout, printed, "{shell} reads the same");
            }
        }
        (printed, root)
    }

    #[test]
    fn the_records_of_who_opened_what_are_read_and_left_where_they_are() {
        let big = vec![b'x'; 2000];
        let (printed, root) = records_of(
            "all",
            &[
                ("t1", b"opener t4\nkeep-open no\n"),
                ("t2", b"opener -\nkeep-open yes\n"),
                // Being written, not a number, too big for a record, no record: not read.
                ("t3.tmp.9", b"opener t4\n"),
                ("t4x", b"opener t4\n"),
                ("t5", &big),
                ("t6", b"what is this\n"),
            ],
        );
        let records = parse_records(&printed);
        assert_eq!(records.len(), 2, "{records:?}");
        assert_eq!(records[0].0, Terminal(1));
        assert_eq!(records[0].1.opener, Anchor::parse("t4"));
        assert!(!records[0].1.keep_open);
        assert_eq!(records[1].0, Terminal(2));
        assert_eq!(records[1].1.opener, None);
        assert!(records[1].1.keep_open);
        let dir = root.0.join(".local/share/bateri/terminals");
        assert!(
            dir.join("t1").exists() && dir.join("t5").exists(),
            "nothing is removed"
        );
        // No directory: nothing printed and no failure.
        let none = TempRoot::new("agent-records-none");
        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(RECORDS)
            .env("HOME", &none.0)
            .output()
            .expect("sh");
        assert!(output.status.success() && output.stdout.is_empty());
        // What a login shell says first, and a frame cut short, are left out.
        let mut text = b"Welcome\nbateri-record oops\n".to_vec();
        text.extend(b"bateri-record t7 23\nopener t4\nkeep-open no\n\n");
        text.extend(b"bateri-record t8 99\nshort");
        assert_eq!(parse_records(&text).len(), 1);
    }

    #[test]
    fn the_record_of_who_opened_a_terminal_is_read_from_the_script_bytes() {
        let record = Record::parse("opener t4\nkeep-open yes\n").expect("a record");
        assert_eq!(record.opener, Anchor::parse("t4"));
        assert!(record.keep_open);
        let made = format!("b-{}", "0123456789abcdef".repeat(2));
        let record = Record::parse(&format!("opener {made}\nkeep-open no\n")).expect("a record");
        assert_eq!(record.opener, Anchor::parse(&made));
        assert!(!record.keep_open);
        // Opened from no terminal; a key a later script writes is left alone.
        let record = Record::parse("opener -\nkeep-open no\nlabel x\n").expect("a record");
        assert_eq!(record.opener, None);
        assert_eq!(
            Record::parse("opener not a name\n").map(|r| r.opener),
            Some(None)
        );
        assert_eq!(Record::parse(""), None);
        assert_eq!(Record::parse("label x\n"), None);
    }
}
