//! The terminal's identity: the environment that tells the shell which
//! terminal it runs in (`TERM_PROGRAM`, `TERM_PROGRAM_VERSION`) and the pane's
//! externally openable name (`TERM_SESSION_ID`, `BATERI_TAB_URL=bateri://tab/<id>`).
//!
//! **Why here:** all four are in the `TERM` family and are written in the same
//! "cannot be overridden" layer as it (`Session::spawn`), so their owner is
//! `TERM`'s owner. For the same reason the `bateri://` scheme lives in a single
//! crate: the scheme's other path, the prompt's internal anchor
//! `bateri://block/<n>`, is in `block_id` in `session.rs`. The path here
//! (`tab/`) is the external name — the application takes it as a URL and
//! brings only that pane, and the tab holding it, to the front. The party that
//! **generates** the UUID is `bt-shell` (`NSUUID`; this crate is platformless
//! and carries no randomness source), the owner of the format is here.
//!
//! **An outward contract.** Other programs read these names and keep reading
//! them across bateri's versions — a status strip of AI sessions finds the
//! pane its agent runs in from `BATERI_TAB_URL` (or `LC_BATERI_TAB_URL` across
//! ssh) and asks `bateri focus` about it. So the path stays `tab/` and means a
//! **pane**, the UUID stays uppercase and the same value for the pane's whole
//! life, restored or handed over included, and the variables keep their
//! names. A tab given an identity of its own would get an address of its own,
//! never this one. `bt-shell-common`'s outward contract test holds the text.

/// The value of `TERM_PROGRAM`.
pub const TERM_PROGRAM: &str = "bateri";

/// The value of `TERM_PROGRAM_VERSION`: the workspace version. All crates use
/// `version.workspace = true`; a test in `bt-shell` expects equality.
pub const TERM_PROGRAM_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The value of `LC_TERMINAL`: the identity that crosses ssh. The
/// `LC_` prefix is the carrier — the stock `SendEnv LANG LC_*` /
/// `AcceptEnv LANG LC_*` pair passes it without any configuration — and the
/// name is iTerm2's precedent, so tools that already read it find us. Never
/// another terminal's value. With it go `LC_TERMINAL_VERSION`
/// ([`TERM_PROGRAM_VERSION`]) and `LC_BATERI_TAB_URL` ([`PaneUuid::url`]).
pub const LC_TERMINAL: &str = TERM_PROGRAM;

/// The URL's scheme + host prefix; written in a single place ([`PaneUuid::url`]).
/// The host is `tab` for a pane: the name is outward and older than the panes.
const TAB_URL_PREFIX: &str = "bateri://tab/";

/// The variables the shell is given a pane's identity in, all in the `TERM`
/// layer of `Session::spawn` (an inherited value never wins): the UUID itself,
/// its URL, and the URL again in the `LC_` family that crosses ssh.
pub fn pane_env(id: &PaneUuid) -> [(&'static str, String); 3] {
    [
        ("TERM_SESSION_ID", id.as_str().to_owned()),
        ("BATERI_TAB_URL", id.url()),
        ("LC_BATERI_TAB_URL", id.url()),
    ]
}

/// A pane's persistent identity: the canonical UUID text (8-4-4-4-12 hex),
/// normalized to uppercase. Not the in-process pane number, which means
/// nothing in another process: this one is given outward, written to the
/// saved layout and carried across an update's handover.
///
/// Typed, because the same text goes two ways — into the shell's environment
/// and to matching an incoming URL — and both sides must see the same form: a
/// lowercase URL and an uppercase identity match only once normalized.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PaneUuid(String);

impl PaneUuid {
    /// Accepts the canonical UUID text (letter case is free); every other
    /// form is `None`.
    pub fn parse(text: &str) -> Option<PaneUuid> {
        let bytes = text.as_bytes();
        if bytes.len() != 36 {
            return None;
        }
        let canonical = bytes.iter().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => *b == b'-',
            _ => b.is_ascii_hexdigit(),
        });
        canonical.then(|| PaneUuid(text.to_ascii_uppercase()))
    }

    /// The identity's text (the value of `TERM_SESSION_ID`).
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// `bateri://tab/<id>` — the only place that writes the URL.
    pub fn url(&self) -> String {
        format!("{TAB_URL_PREFIX}{}", self.0)
    }

    /// The only place that decodes `bateri://tab/<id>`. Scheme and host are
    /// case-insensitive, the UUID goes through [`PaneUuid::parse`]; a query, a
    /// fragment, an extra path component, a trailing `/` and
    /// `bateri://block/…` → `None`.
    pub fn from_url(url: &str) -> Option<PaneUuid> {
        let prefix = url.get(..TAB_URL_PREFIX.len())?;
        if !prefix.eq_ignore_ascii_case(TAB_URL_PREFIX) {
            return None;
        }
        // `parse` checks the length and character set exactly: `?`, `#` and `/`
        // are not hex, so a query/fragment/extra path is rejected here.
        PaneUuid::parse(&url[TAB_URL_PREFIX.len()..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0F1E2D3C-4B5A-6978-8796-A5B4C3D2E1F0";

    #[test]
    fn url_round_trips() {
        let id = PaneUuid::parse(ID).expect("canonical UUID must be accepted");
        assert_eq!(id.url(), format!("bateri://tab/{ID}"));
        assert_eq!(PaneUuid::from_url(&id.url()), Some(id));
    }

    #[test]
    fn case_is_normalized() {
        let lower = ID.to_ascii_lowercase();
        let id = PaneUuid::parse(&lower).expect("lowercase UUID must be accepted");
        assert_eq!(id.as_str(), ID);
        assert_eq!(
            PaneUuid::from_url(&format!("BATERI://TAB/{lower}")),
            Some(id.clone())
        );
        assert_eq!(
            PaneUuid::from_url(&format!("bateri://tab/{lower}")),
            Some(id)
        );
    }

    #[test]
    fn foreign_forms_are_rejected() {
        let rejected = [
            "bateri://block/3".to_owned(),
            "bateri://tab/".to_owned(),
            "bateri://tab".to_owned(),
            format!("bateri://tab/{ID}/"),
            format!("bateri://tab/{ID}?x"),
            format!("bateri://tab/{ID}#x"),
            format!("bateri://tab/{ID}/extra"),
            "bateri://tab/zzzzzzzz-zzzz-zzzz-zzzz-zzzzzzzzzzzz".to_owned(),
            format!("https://tab/{ID}"),
            format!("bateri://tab/{}", ID.replace('-', "")),
            // A multi-byte character must not split the prefix boundary (no panic).
            "bateri://tağ/".to_owned(),
            "ğ".to_owned(),
            String::new(),
        ];
        for url in rejected {
            assert_eq!(PaneUuid::from_url(&url), None, "{url} must be rejected");
        }
        assert_eq!(PaneUuid::parse(&format!("{ID}0")), None);
        assert_eq!(PaneUuid::parse(&ID.replace('-', "_")), None);
    }
}
