//! Notifications a program asks the terminal to show — iTerm2's `OSC 9 ; text` and urxvt's
//! `OSC 777 ; notify ; title ; body` — on their way to a host that embeds the pane. bateri's own
//! tabs show none of them, as they never have; a host hears them and decides.
//!
//! **Text from the stream is bounded and cleaned.** A sequence over its bound is dropped whole and
//! skipped to its terminator; what is kept is read as UTF-8 (a broken byte becomes U+FFFD), loses
//! every control character and the bidirectional and invisible formatting characters that could
//! make a notification read as something else, and is cut to its length at a character boundary.
//! A notification with nothing left to say is none.
//!
//! **`OSC 9` is shared.** ConEmu's commands ride the same number with a numeric first field —
//! `9 ; 4 ; …` is its progress bar ([`crate::program_status::progress`]), `9 ; 9 ; …` its working
//! directory — so a payload that starts with a number and a `;` is a command, never a notification.
//!
//! **Pending ones are few.** The ledger keeps at most [`PENDING_LIMIT`] until the host takes them
//! ([`crate::Session::take_notifications`]); a program printing them in a loop pushes the oldest out.

use std::collections::VecDeque;

/// urxvt's notification number. Not our choice: the convention's.
pub(crate) const NOTIFY_OSC: u32 = 777;

/// The upper bound of an `ESC ] 777 ;` payload, in bytes — and of `ESC ] 9 ;`'s, whose arm also
/// reads ConEmu's progress ([`crate::program_status::PROGRESS_PAYLOAD_LIMIT`] is this). A
/// design constant: a notification is a line or two, far below it.
pub(crate) const NOTIFY_PAYLOAD_LIMIT: usize = 1024;

/// A title's and a body's length, bytes, after cleaning.
const MAX_TITLE: usize = 256;
const MAX_BODY: usize = 1024;

/// How many notifications wait for the host at most.
pub(crate) const PENDING_LIMIT: usize = 8;

/// A notification a program asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notification {
    /// `OSC 777`'s title; `OSC 9` has none.
    pub title: Option<String>,
    pub body: String,
}

/// `OSC 9 ; text`: iTerm2's notification — unless the payload is one of ConEmu's commands.
pub(crate) fn osc9(payload: &[u8]) -> Option<Notification> {
    let digits = payload
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    if digits > 0 && matches!(payload.get(digits), None | Some(b';')) {
        return None;
    }
    let body = clean(payload, MAX_BODY)?;
    Some(Notification { title: None, body })
}

/// `OSC 777 ; notify ; title ; body`: urxvt's notification. Its other commands are not ours. The
/// body is everything after the title, `;` included.
pub(crate) fn osc777(payload: &[u8]) -> Option<Notification> {
    let mut fields = payload.splitn(3, |&byte| byte == b';');
    if fields.next()? != b"notify" {
        return None;
    }
    let title = fields.next().and_then(|title| clean(title, MAX_TITLE));
    let body = fields.next().and_then(|body| clean(body, MAX_BODY));
    match (title, body) {
        (None, None) => None,
        (title, Some(body)) => Some(Notification { title, body }),
        (Some(title), None) => Some(Notification {
            title: None,
            body: title,
        }),
    }
}

/// `bytes` as text a notification can show: UTF-8, no control or bidirectional/invisible
/// formatting character, trimmed, at most `limit` bytes; `None` when nothing is left.
fn clean(bytes: &[u8], limit: usize) -> Option<String> {
    let text: String = String::from_utf8_lossy(bytes)
        .chars()
        .filter(|&c| !c.is_control() && !formatting(c))
        .collect();
    let text = text.trim();
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let text = text[..end].trim_end();
    (!text.is_empty()).then(|| text.to_owned())
}

/// The characters that change how text around them reads without showing themselves: zero-width
/// ones, the bidirectional embeddings, overrides and isolates, the byte order mark.
fn formatting(c: char) -> bool {
    matches!(
        c,
        '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2069}' | '\u{FEFF}'
    )
}

/// The notifications waiting for the host, oldest first.
#[derive(Debug, Default)]
pub(crate) struct Pending(VecDeque<Notification>);

impl Pending {
    /// Keeps `note`; past [`PENDING_LIMIT`] the oldest goes.
    pub(crate) fn push(&mut self, note: Notification) {
        if self.0.len() == PENDING_LIMIT {
            self.0.pop_front();
        }
        self.0.push_back(note);
    }

    /// Every waiting notification, oldest first; none wait after.
    pub(crate) fn take(&mut self) -> Vec<Notification> {
        self.0.drain(..).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(title: Option<&str>, body: &str) -> Option<Notification> {
        Some(Notification {
            title: title.map(str::to_owned),
            body: body.to_owned(),
        })
    }

    #[test]
    fn osc_9_is_a_notification_unless_it_is_one_of_conemus_commands() {
        assert_eq!(osc9(b"build finished"), note(None, "build finished"));
        assert_eq!(osc9(b"4;1;50"), None, "the progress bar");
        assert_eq!(osc9(b"9;/tmp"), None, "the working directory");
        assert_eq!(osc9(b"4"), None);
        assert_eq!(osc9(b"42 tests passed"), note(None, "42 tests passed"));
        assert_eq!(osc9(b""), None);
    }

    #[test]
    fn osc_777_notify_has_a_title_and_a_body_with_its_semicolons() {
        assert_eq!(
            osc777(b"notify;CI;tests passed; 3 skipped"),
            note(Some("CI"), "tests passed; 3 skipped")
        );
        assert_eq!(osc777(b"notify;;done"), note(None, "done"));
        assert_eq!(osc777(b"notify;only a title"), note(None, "only a title"));
        assert_eq!(osc777(b"preexec"), None, "another command of the number");
        assert_eq!(osc777(b"notify"), None);
    }

    #[test]
    fn text_from_the_stream_is_cleaned_and_bounded() {
        assert_eq!(
            osc9("a\u{1b}[31mb\u{202E}c\u{200B}d\u{7}".as_bytes()),
            note(None, "a[31mbcd")
        );
        assert_eq!(osc9(b"\xff ok"), note(None, "\u{FFFD} ok"));
        assert_eq!(osc9(b"\x01\x02 \t"), None, "nothing left to say");
        let long = "é".repeat(MAX_BODY);
        let kept = osc9(long.as_bytes()).expect("a notification").body;
        assert!(kept.len() <= MAX_BODY && kept.chars().all(|c| c == 'é'));
    }

    #[test]
    fn a_flood_keeps_the_newest_few() {
        let mut pending = Pending::default();
        for n in 0..PENDING_LIMIT + 3 {
            pending.push(Notification {
                title: None,
                body: n.to_string(),
            });
        }
        let kept: Vec<String> = pending.take().into_iter().map(|note| note.body).collect();
        assert_eq!(kept.len(), PENDING_LIMIT);
        assert_eq!(kept.first().map(String::as_str), Some("3"));
        assert!(pending.take().is_empty());
    }
}
