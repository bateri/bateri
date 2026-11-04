//! What an agent on a server asks of bateri with `bateri split`, `tab`, `ls`, `close` and `notify`,
//! and what is decided about it — the half that needs no AppKit, so `make linux` runs its tests.
//!
//! **Who does what.** The terminal itself is the server's: the script on the server starts it
//! with `kablo new`, numbers it (`p9`) and answers `wait`, `capture` and `send` without this Mac.
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
//! - [`serve`] is what the Mac does with a letter: the checks, the ask of the windows (`Shell`),
//!   the answer, and what waits for its anchor ([`wait`]); [`notes`] is what a server keeps for a
//!   Mac that was away and the record of who opened a terminal; [`summary`] is the one line that
//!   tells the user what was opened.
//! - [`listing`] is `ls`: the windows and tabs as they stand with each tab's real split tree,
//!   then a table of the server's terminals, as text and as JSON.
//!
//! A terminal that runs on another machine than the one that asked (`bateri split -S prod`, for a
//! machine whose level is `full`) is started by the asking program itself over `ssh`, with the words
//! the Mac gives it (the `ssh` verb), and its pane is a `place` that names the `server`; a pane of
//! another machine is written `p9@prod` ([`Anchor::address`]) in the verbs that work on panes.
//!
//! Who asked is never in a message: the Mac knows it from the ticket kablo hands it for the
//! conversation, which names the terminal the request came from.

pub mod arrange;
pub mod level;
pub mod listing;
pub mod message;
pub mod notes;
pub mod place;
pub mod relay;
pub mod serve;
pub mod summary;
pub mod wait;

#[cfg(test)]
mod script;

use std::fmt;

/// A terminal's number on its server: the `p9` an agent is told when it opens one and names when it
/// acts on one. Numbers are per server, count up and are never given twice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Terminal(pub u64);

impl Terminal {
    /// The terminal a name stands for: `p` and the number written plainly (`p9`, never `p09` or
    /// `P9`). A terminal that has another kind of name (one a person made with kablo itself, or
    /// one numbered `t9` before the unit was the pane) is no terminal for an agent to name; it
    /// goes on running under that name.
    pub fn parse(name: &str) -> Option<Terminal> {
        let digits = name.strip_prefix('p')?;
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
        write!(out, "p{}", self.0)
    }
}

/// The terminal a pane is placed beside: kablo's name for it. An agent *writes* a terminal as its
/// number (`p4`, [`Terminal`]); the terminal it runs in is the caller's whatever it is called, and
/// a request that defaults to it names it so — a terminal the Mac opened before the numbers
/// existed is `b-` and 32 digits, one a person made with `kablo new` is what they typed. Reading
/// takes every name kablo allows ([`kablo_proto::SessionId`]'s alphabet and length, so a name is
/// never a path), which includes every `pN`.
///
/// A terminal of another machine is written with the machine after an `@` — `p9@prod`, this Mac
/// `p9@local`, as `bateri ls` says them ([`Anchor::address`]). The `@` is no part of kablo's
/// alphabet, so a name and an address never read as one another.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Anchor {
    name: kablo_proto::SessionId,
    machine: Option<String>,
}

/// The machine an address may name: what `bateri ls` prints after the `@` — a host name as the
/// user typed it to `ssh` without its `user@`, or `local`. Letters, digits and `. _ - :`, starting
/// with a letter or a digit (so it is never an option), at most 253 bytes.
pub fn is_machine(name: &str) -> bool {
    name.len() <= 253
        && name
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_alphanumeric())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b':'))
}

/// The machine of this Mac in an address.
pub const LOCAL_MACHINE: &str = "local";

/// This Mac as `-S` and a `server` line name it: the word `@local`, which is no host `ssh` could be
/// given (a host starts with a letter or a digit), so it cannot be a server of that name.
pub const LOCAL_SERVER: &str = "@local";

impl Anchor {
    /// The terminal a name stands for, `None` for a name kablo does not allow (an address is
    /// not a name: [`Anchor::address`]).
    pub fn parse(name: &str) -> Option<Anchor> {
        kablo_proto::SessionId::new(name).ok().map(|name| Anchor {
            name,
            machine: None,
        })
    }

    /// A terminal as a name or as an address — `p9` or `p9@prod` — `None` for what is neither.
    pub fn address(text: &str) -> Option<Anchor> {
        let Some((name, machine)) = text.split_once('@') else {
            return Anchor::parse(text);
        };
        if !is_machine(machine) {
            return None;
        }
        Anchor::parse(name).map(|found| Anchor {
            machine: Some(machine.to_owned()),
            ..found
        })
    }

    /// An agent's terminal as an anchor.
    pub fn of(terminal: Terminal) -> Anchor {
        Anchor {
            name: kablo_proto::SessionId::new(&terminal.to_string())
                .expect("`p` and a number is a name kablo allows"),
            machine: None,
        }
    }

    /// The name, as kablo has it.
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    /// The machine the address names, `None` for a terminal of the machine that asks.
    pub fn machine(&self) -> Option<&str> {
        self.machine.as_deref()
    }

    /// The agent's number, when the name is one.
    pub fn terminal(&self) -> Option<Terminal> {
        Terminal::parse(self.name())
    }
}

impl fmt::Display for Anchor {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str(self.name())?;
        match &self.machine {
            Some(machine) => write!(out, "@{machine}"),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_terminal_is_named_by_its_plain_number() {
        assert_eq!(Terminal::parse("p9"), Some(Terminal(9)));
        assert_eq!(Terminal::parse("p0"), Some(Terminal(0)));
        assert_eq!(Terminal(14).to_string(), "p14");
        for name in [
            "",
            "p",
            "9",
            "P9",
            "p09",
            "p+9",
            "p-9",
            "p9 ",
            " p9",
            "pp9",
            "p9a",
            "b-0123456789",
            "p99999999999999999999",
            // The name the numbers had before the unit was the pane: a terminal that has it
            // runs on, and is no pane for an agent to name.
            "t9",
        ] {
            assert_eq!(Terminal::parse(name), None, "{name:?}");
        }
        assert_eq!(Terminal::number_of("p3"), Some(3));
        assert_eq!(Terminal::number_of("t3"), None);
        assert_eq!(Terminal::number_of("b-3"), None);
    }

    #[test]
    fn an_anchor_is_any_name_kablo_allows_and_an_agents_number_is_one() {
        let made = format!("b-{}", "0123456789abcdef".repeat(2));
        for name in [
            "p4",
            "p0",
            "p07",
            "t4",
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
            " p4",
            "p4 ",
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
            Anchor::parse("p9").and_then(|a| a.terminal()),
            Some(Terminal(9))
        );
        assert_eq!(
            Anchor::of(Terminal(9)),
            Anchor::parse("p9").expect("a name")
        );
        assert_eq!(Anchor::parse("p09").and_then(|a| a.terminal()), None);
        assert_eq!(Anchor::parse("t9").and_then(|a| a.terminal()), None);
        assert_eq!(Anchor::parse(&made).and_then(|a| a.terminal()), None);
    }
}
