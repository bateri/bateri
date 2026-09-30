//! The terminal's identity: the environment that tells the shell which
//! terminal it runs in (`TERM_PROGRAM`, `TERM_PROGRAM_VERSION`) and the tab's
//! externally openable name (`TERM_SESSION_ID`, `BATERI_TAB_URL=bateri://tab/<id>`;
//! 038).
//!
//! **Why here:** all four are in the `TERM` family and are written in the same
//! "cannot be overridden" layer as it (`Session::spawn`), so their owner is
//! `TERM`'s owner. For the same reason the `bateri://` scheme lives in a single
//! crate: the scheme's other path, the prompt's internal anchor
//! `bateri://block/<n>`, is in `block_id` in `session.rs`. The path here
//! (`tab/`) is the external name — the application takes it as a URL and
//! brings only that tab to the front. The party that **generates** the UUID is
//! `bt-shell` (`NSUUID`; this crate is platformless and carries no randomness
//! source), the owner of the format is here.

/// The value of `TERM_PROGRAM`.
pub const TERM_PROGRAM: &str = "bateri";

/// The value of `TERM_PROGRAM_VERSION`: the workspace version. All crates use
/// `version.workspace = true`; a test in `bt-shell` expects equality
/// (`.tasks/038-terminal-kimligi/discussion.md` → Karar 3).
pub const TERM_PROGRAM_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The URL's scheme + host prefix; written in a single place ([`TabId::url`]).
const TAB_URL_PREFIX: &str = "bateri://tab/";

/// A tab's persistent identity: the canonical UUID text (8-4-4-4-12 hex),
/// normalized to uppercase.
///
/// Typed, because the same text goes two ways — into the shell's environment
/// and to matching an incoming URL — and both sides must see the same form: a
/// lowercase URL and an uppercase identity match only once normalized.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TabId(String);

impl TabId {
    /// Accepts the canonical UUID text (letter case is free); every other
    /// form is `None`.
    pub fn parse(text: &str) -> Option<TabId> {
        let bytes = text.as_bytes();
        if bytes.len() != 36 {
            return None;
        }
        let canonical = bytes.iter().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => *b == b'-',
            _ => b.is_ascii_hexdigit(),
        });
        canonical.then(|| TabId(text.to_ascii_uppercase()))
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
    /// case-insensitive, the UUID goes through [`TabId::parse`]; a query, a
    /// fragment, an extra path component, a trailing `/` and
    /// `bateri://block/…` → `None`.
    pub fn from_url(url: &str) -> Option<TabId> {
        let prefix = url.get(..TAB_URL_PREFIX.len())?;
        if !prefix.eq_ignore_ascii_case(TAB_URL_PREFIX) {
            return None;
        }
        // `parse` checks the length and character set exactly: `?`, `#` and `/`
        // are not hex, so a query/fragment/extra path is rejected here.
        TabId::parse(&url[TAB_URL_PREFIX.len()..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0F1E2D3C-4B5A-6978-8796-A5B4C3D2E1F0";

    #[test]
    fn url_round_trips() {
        let id = TabId::parse(ID).expect("canonical UUID must be accepted");
        assert_eq!(id.url(), format!("bateri://tab/{ID}"));
        assert_eq!(TabId::from_url(&id.url()), Some(id));
    }

    #[test]
    fn case_is_normalized() {
        let lower = ID.to_ascii_lowercase();
        let id = TabId::parse(&lower).expect("lowercase UUID must be accepted");
        assert_eq!(id.as_str(), ID);
        assert_eq!(
            TabId::from_url(&format!("BATERI://TAB/{lower}")),
            Some(id.clone())
        );
        assert_eq!(TabId::from_url(&format!("bateri://tab/{lower}")), Some(id));
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
            assert_eq!(TabId::from_url(&url), None, "{url} must be rejected");
        }
        assert_eq!(TabId::parse(&format!("{ID}0")), None);
        assert_eq!(TabId::parse(&ID.replace('-', "_")), None);
    }
}
