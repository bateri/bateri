//! What an agent on a server asks of bateri with `bateri split`, `tab`, `ls`, `close` and `notify`,
//! and what is decided about it — the half that needs no AppKit, so `make linux` runs its tests.
//!
//! **Who does what.** The terminal itself is the server's: the script on the server starts it
//! with `kablo new`, numbers it (`t9`) and answers `wait`, `capture` and `send` without this Mac.
//! The Mac only decides where a pane that shows it goes, and shows it — it runs nothing and
//! writes no byte to any shell, so the request is a *place*, never a *program*. The request
//! travels on kablo's app channel as lines of text ([`message`]); when the Mac is not there it
//! waits on the server as the same lines and is read when the Mac connects.
//!
//! - [`message`] is the wire: one request per conversation, one reply, strictly read and never
//!   panicking. Version 1's text is frozen in the tests; a later version is added beside it.
//! - [`place`] is the rule: given a tab's tree, the pane it is asked to go beside and what the
//!   agent asked (a side, a share), where the new pane lands — the drag's own plan
//!   ([`crate::split::Tree::plan_beside_sharing`]), the palette's choice of a side when none was
//!   asked ([`crate::palette::beside`]), and a new tab when the tab has no room left.
//! - [`listing`] is `ls`: the windows and tabs as they stand with each tab's real split tree,
//!   then a table of the server's terminals, as text and as JSON.
//!
//! Who asked is never in a message: the Mac knows it from the ticket kablo hands it for the
//! conversation, which names the terminal the request came from.

pub mod listing;
pub mod message;
pub mod place;

#[cfg(test)]
mod script;

use std::fmt;

/// A terminal's number on its server: the `t9` an agent is told when it opens one and names when it
/// acts on one. Numbers are per server, count up and are never given twice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Terminal(pub u64);

impl Terminal {
    /// The terminal a name stands for: `t` and the number written plainly (`t9`, never `t09` or
    /// `T9`). A terminal that has another kind of name (one a person made with kablo itself) is
    /// no terminal for an agent to name.
    pub fn parse(name: &str) -> Option<Terminal> {
        let digits = name.strip_prefix('t')?;
        let number: u64 = digits.parse().ok()?;
        // `u64::from_str` takes a `+` and leading zeros; the one spelling is the plain one.
        (number.to_string() == digits).then_some(Terminal(number))
    }

    /// The number a terminal's name carries, `None` for a name that is not an agent's number.
    pub fn number_of(name: &str) -> Option<u64> {
        Terminal::parse(name).map(|terminal| terminal.0)
    }
}

impl fmt::Display for Terminal {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(out, "t{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_terminal_is_named_by_its_plain_number() {
        assert_eq!(Terminal::parse("t9"), Some(Terminal(9)));
        assert_eq!(Terminal::parse("t0"), Some(Terminal(0)));
        assert_eq!(Terminal(14).to_string(), "t14");
        for name in [
            "",
            "t",
            "9",
            "T9",
            "t09",
            "t+9",
            "t-9",
            "t9 ",
            " t9",
            "tt9",
            "t9a",
            "b-0123456789",
            "t99999999999999999999",
        ] {
            assert_eq!(Terminal::parse(name), None, "{name:?}");
        }
        assert_eq!(Terminal::number_of("t3"), Some(3));
        assert_eq!(Terminal::number_of("b-3"), None);
    }
}
