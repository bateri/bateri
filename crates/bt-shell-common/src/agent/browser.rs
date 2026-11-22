//! `bateri browser`: what an agent asks of the browsers a pane's page shows, and the part of the
//! answer that needs no window. The shell (`bt-shell-macos`) finds the pane
//! and sends the helper its jobs; what is decided here is whose the browser is (the level), what a
//! word of the request means, and how an answer is put into a reply or into a file.
//!
//! **Who may.** A browser is an element of a pane's page, and it belongs to the machine of the
//! terminal the pane shows. `ls`, `text`, `snapshot`, `shot` and `console` see it, the other verbs
//! (`clear` among them: it empties what the browser keeps, and signs the person out of a site) change it:
//! the same two acts as the rest of the commands ([`level::Act`]), judged over a layer of that one
//! pane ([`judge`]). On a pane of the asker's own machine that is the table as it stands: `own`,
//! `screen` and `full` may, `off` may not. A pane of **another machine** is read or driven only by
//! `full`, as `capture` and `send` are for it ([`level::Act::Reach`]: a page may hold what a person
//! signed in to): under `own` it is answered as a name nobody has, under `screen` it is refused
//! with the line that says `full` is what allows it. Nothing here reaches another machine: the
//! helper runs on the computer the bateri app runs on.
//!
//! **What comes back.** A text that fits a reply is the reply; one that does not, and every
//! picture, is a file the app writes on the machine of the one who asked ([`file_on`]), since a
//! program on a server cannot open a path of this computer.

use bt_core::CommandLevel;
pub use kablo_bateri::browser::{Action, Browser, Clearing, FileKind, Filed, TEXT_MAX};
use kablo_bateri::message::{Failure, Kind};
use kablo_bateri::paths::{BROWSER, DIR};

use super::level::{self, Act, Panes, Verdict};
use crate::kablo::install::Channel;

fn usage(text: impl Into<String>) -> Failure {
    Failure::new(Kind::Usage, text)
}

/// What a verb does to a browser: the five that read see it, the others change it.
pub fn act_of(action: &Action) -> Act {
    match action {
        Action::Ls
        | Action::Text { .. }
        | Action::Snapshot { .. }
        | Action::Shot { .. }
        | Action::Console { .. } => Act::See,
        Action::Click { .. }
        | Action::Type { .. }
        | Action::Open { .. }
        | Action::Back
        | Action::Reload
        | Action::Clear { .. } => Act::Change,
    }
}

/// Whether the machine that asked (its `level`) may do `action` to a browser of a pane of its own
/// machine (`own`: the level's table) or of another's (`full` alone, [`Act::Reach`]). `host` is the server as the user typed it (`None` for this
/// computer) and `pane` the address as the request wrote it, for the words of a refusal.
pub fn judge(
    level: CommandLevel,
    action: &Action,
    own: bool,
    host: Option<&str>,
    pane: &str,
) -> Result<(), Failure> {
    let act = act_of(action);
    let verdict = if own {
        level::judge(level, act, Panes::own(1))
    } else {
        // The browser of another machine's pane is a thing to reach: what the table says of seeing
        // or changing it (`off` refuses, `own` does not show it) holds first, and then only `full`.
        match level::judge(level, act, Panes::other(1)) {
            Verdict::Allowed => level::judge(level, Act::Reach, Panes::default()),
            held => held,
        }
    };
    match verdict {
        Verdict::Allowed => Ok(()),
        Verdict::Refused(why) => Err(Failure::new(
            Kind::Refused,
            level::refusal(level, why, host),
        )),
        Verdict::Unknown => Err(Failure::new(
            Kind::Unknown,
            format!("there is no pane {pane}"),
        )),
    }
}

/// What a `click`, or a `type`'s `into`, points at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Aim {
    /// A node a snapshot printed (`[12]`).
    Node(i64),
    /// A point of the page, in its pixels from the top-left.
    Point(f64, f64),
}

/// The most a point's coordinate may be: a page is not larger than the display it is on.
const POINT_MAX: f64 = 1.0e6;

/// What `target` says: the reference a snapshot printed, `[12]` (the number alone is taken too), or
/// a point, `X,Y`. What is neither is a mistake of the request, with the words that say what is
/// read.
pub fn aim(target: &str) -> Result<Aim, Failure> {
    let words = || {
        usage(format!(
            "`{target}` names no element: write a node as `[12]`, as a snapshot prints it, or a point as `X,Y`"
        ))
    };
    let text = target.trim();
    let bare = text
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(text);
    if !bare.is_empty() && bare.bytes().all(|b| b.is_ascii_digit()) {
        return bare.parse::<i64>().map(Aim::Node).map_err(|_| words());
    }
    let (x, y) = text.split_once(',').ok_or_else(words)?;
    let number = |part: &str| {
        part.trim()
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite() && (0.0..=POINT_MAX).contains(n))
    };
    match (number(x), number(y)) {
        (Some(x), Some(y)) => Ok(Aim::Point(x, y)),
        _ => Err(words()),
    }
}

/// How a browser is named in an `ls` and in `--element`: the number of its id, in hex.
pub fn element_name(number: u64) -> String {
    format!("{number:x}")
}

/// The number of the id an `--element` word names.
pub fn element_number(word: &str) -> Result<u64, Failure> {
    let text = word.trim();
    if text.is_empty() || text.len() > 16 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(usage(format!(
            "`{word}` is no browser's id: `ls` prints them"
        )));
    }
    u64::from_str_radix(text, 16)
        .map_err(|_| usage(format!("`{word}` is no browser's id: `ls` prints them")))
}

/// A browser of the pane, as `ls` tells it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed {
    /// The number of its id ([`element_name`]).
    pub id: u64,
    /// What the browser is (`ready`, `starting`, `crashed`, `no_route`…).
    pub status: String,
    /// The address it is at: empty before the first load.
    pub url: String,
    pub title: String,
    /// The person has the keyboard in it.
    pub keyboard: bool,
}

/// The most of an address, and of a title, one line of an `ls` keeps (in characters).
const URL_SHOWN: usize = 400;
const TITLE_SHOWN: usize = 120;

/// `text` on one line, at most `most` characters: a control character is a space, and what is cut
/// ends in `…`.
fn one_line(text: &str, most: usize) -> String {
    let mut line: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(most + 1)
        .collect();
    if line.chars().count() > most {
        line = line.chars().take(most.saturating_sub(1)).collect();
        line.push('…');
    }
    line
}

/// What an `ls` says: one line for each browser, tab-separated: `*` for the one with the keyboard
/// (else nothing), the id, the status, the address and the title. A page with none says nothing.
/// What does not fit a reply is left out and counted in the last line, so that the answer is
/// always a reply and an agent that asks for more has the ids it needs for it.
pub fn listing(rows: &[Listed]) -> String {
    let mut out = String::new();
    for (shown, row) in rows.iter().enumerate() {
        let line = format!(
            "{}\t{}\t{}\t{}\t{}\n",
            if row.keyboard { "*" } else { "" },
            element_name(row.id),
            one_line(&row.status, 32),
            one_line(&row.url, URL_SHOWN),
            one_line(&row.title, TITLE_SHOWN),
        );
        let rest = format!("… and {} more\n", rows.len() - shown);
        if out.len() + line.len() + rest.len() > TEXT_MAX {
            out.push_str(&rest);
            break;
        }
        out.push_str(&line);
    }
    out
}

/// What a text or a snapshot comes to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Said {
    /// It fits a reply.
    Reply(String),
    /// It does not: these bytes are a file.
    File(Vec<u8>),
}

/// The line a file ends with when the helper cut the page's text at its size.
const CUT: &str = "\n[the page's text was cut here: it is longer than the browser keeps]\n";

/// A text of the page: itself in the reply when it fits ([`TEXT_MAX`]) and the helper did not cut
/// it, else a file (a cut one says so at its end).
pub fn said(text: String, cut: bool) -> Said {
    if !cut && text.len() <= TEXT_MAX {
        return Said::Reply(text);
    }
    let mut bytes = text.into_bytes();
    if cut {
        bytes.extend_from_slice(CUT.as_bytes());
    }
    Said::File(bytes)
}

/// The bytes of a standard base64 text (the helper gives a picture so), whitespace let go.
pub fn unbase64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut bits = 0u32;
    let mut held = 0u32;
    let mut padding = 0usize;
    for byte in text.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => {
                padding += 1;
                continue;
            }
            b' ' | b'\t' | b'\r' | b'\n' => continue,
            _ => return None,
        };
        if padding > 0 {
            // A letter after the padding.
            return None;
        }
        bits = (bits << 6) | u32::from(value);
        held += 6;
        if held >= 8 {
            held -= 8;
            out.push((bits >> held) as u8);
            bits &= (1 << held) - 1;
        }
    }
    // What is left over is no whole byte: a tail of one character, or bits that are not zero.
    (padding <= 2 && bits == 0 && held < 6).then_some(out)
}

/// The exit codes of the script that writes a file.
const EXIT_UNWRITABLE: i32 = 9;
const EXIT_INCOMPLETE: i32 = 8;

/// Whether `part` is a name the command made: letters, digits, `-` and `_`, not empty, at most 64.
fn is_part(part: &str) -> bool {
    !part.is_empty()
        && part.len() <= 64
        && part
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The script that writes the standard input as `<part>.<ext>` in the folder of browser files below
/// the `HOME` of the account it runs for (not the directory it starts in, which a bridge takes from
/// the password file), as the programs of `apps` are written. Fixed text and a checked word: no byte
/// the page or the agent chose is read by a shell. The file is written whole under another name and
/// moved, so that the command never finds half of it, and is the account's alone (`umask 077`: a
/// page may hold what a person signed in to).
fn script(part: &str, kind: FileKind) -> String {
    let extension = kind.extension();
    format!(
        "umask 077; d=\"$HOME/{DIR}/{BROWSER}\"; mkdir -p \"$d\" || exit {EXIT_UNWRITABLE}; \
         f=\"$d/{part}.{extension}\"; \
         cat >\"$f.part\" && mv -f \"$f.part\" \"$f\" || {{ rm -f \"$f.part\"; exit {EXIT_INCOMPLETE}; }}"
    )
}

/// Writes `bytes` as the file `part` names, of `kind`, on the machine `channel` reaches (the one
/// that asked), and says what the reply tells the command about it.
pub fn file_on(
    channel: &dyn Channel,
    part: &str,
    kind: FileKind,
    bytes: &[u8],
) -> Result<Filed, Failure> {
    if !is_part(part) {
        return Err(usage("`part` is letters, digits, `-` and `_`, at most 64"));
    }
    let reply = channel.run(&script(part, kind), bytes).map_err(|error| {
        Failure::new(
            Kind::Failed,
            format!("the machine could not be written to: {error}"),
        )
    })?;
    match reply.status {
        Some(0) => Ok(Filed {
            kind,
            bytes: bytes.len() as u64,
        }),
        Some(EXIT_UNWRITABLE) => Err(Failure::new(
            Kind::Failed,
            "the folder for browser files cannot be made on the machine",
        )),
        Some(EXIT_INCOMPLETE) => Err(Failure::new(
            Kind::Failed,
            "the file did not reach the machine whole",
        )),
        _ => Err(Failure::new(
            Kind::Failed,
            format!(
                "writing the file failed: {}",
                reply.err.lines().next().unwrap_or("no reason given")
            ),
        )),
    }
}

#[cfg(test)]
mod tests;
