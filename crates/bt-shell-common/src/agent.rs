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

/// The terminal a pane is placed beside: kablo's name for it. An agent *writes* a terminal as its
/// number (`t4`, [`Terminal`]); the terminal it runs in is the caller's whatever it is called, and
/// a request that defaults to it names it so — a terminal the Mac opened before the numbers
/// existed is `b-` and 32 digits, one a person made with `kablo new` is what they typed. Reading
/// takes every name kablo allows ([`kablo_proto::SessionId`]'s alphabet and length, so a name is
/// never a path), which includes every `tN`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Anchor(kablo_proto::SessionId);

impl Anchor {
    /// The terminal a name stands for, `None` for a name kablo does not allow.
    pub fn parse(name: &str) -> Option<Anchor> {
        kablo_proto::SessionId::new(name).ok().map(Anchor)
    }

    /// An agent's terminal as an anchor.
    pub fn of(terminal: Terminal) -> Anchor {
        Anchor(
            kablo_proto::SessionId::new(&terminal.to_string())
                .expect("`t` and a number is a name kablo allows"),
        )
    }

    /// The name, as kablo has it.
    pub fn name(&self) -> &str {
        self.0.as_str()
    }

    /// The agent's number, when the name is one.
    pub fn terminal(&self) -> Option<Terminal> {
        Terminal::parse(self.name())
    }
}

impl fmt::Display for Anchor {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str(self.name())
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

    #[test]
    fn an_anchor_is_any_name_kablo_allows_and_an_agents_number_is_one() {
        let made = format!("b-{}", "0123456789abcdef".repeat(2));
        for name in [
            "t4",
            "t0",
            "t07",
            "mine",
            "b-0a1b",
            &made,
            "a.b_c-d",
            &"x".repeat(64),
        ] {
            let anchor = Anchor::parse(name).unwrap_or_else(|| panic!("{name:?}"));
            assert_eq!(anchor.name(), name);
            assert_eq!(anchor.to_string(), name);
        }
        for name in [
            "",
            " t4",
            "t4 ",
            "a b",
            "a/b",
            "../x",
            "a\nb",
            "é",
            &"x".repeat(65),
        ] {
            assert_eq!(Anchor::parse(name), None, "{name:?}");
        }
        // The agent's own spelling keeps its number; every other name has none.
        assert_eq!(
            Anchor::parse("t9").and_then(|a| a.terminal()),
            Some(Terminal(9))
        );
        assert_eq!(
            Anchor::of(Terminal(9)),
            Anchor::parse("t9").expect("a name")
        );
        assert_eq!(Anchor::parse("t09").and_then(|a| a.terminal()), None);
        assert_eq!(Anchor::parse(&made).and_then(|a| a.terminal()), None);
    }
}
