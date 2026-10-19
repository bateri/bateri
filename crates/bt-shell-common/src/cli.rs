//! What the user-facing subcommands (`bateri config`, `bateri help`) share about the terminal
//! they print to: whether to color, and the few styles they color with.
//!
//! **Color only for a person.** Escape codes go out only when standard output is a terminal,
//! `NO_COLOR` is unset or empty (<https://no-color.org>) and `TERM` isn't `dumb`; piped into
//! `grep`, written to a file or read by a script, the text is the same text without them. The
//! decision is taken once, by the caller, and handed in: the printing code takes a [`Style`]
//! and a test hands it [`Style::plain`].
//!
//! The palette is the terminal's own (SGR bold, dim and the basic colors), never a fixed RGB:
//! the text sits on whatever theme the user runs, bateri's or another terminal's over ssh.

use std::io::IsTerminal as _;

/// How a subcommand's text is styled: with escape codes or without.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Style {
    color: bool,
}

impl Style {
    /// No escape codes — piped output, `NO_COLOR`, tests.
    pub fn plain() -> Self {
        Self { color: false }
    }

    /// Escape codes, whatever the output is.
    pub fn colored() -> Self {
        Self { color: true }
    }

    /// The decision for standard output: [`Self::decide`] over this process's
    /// environment and whether its standard output is a terminal.
    pub fn for_stdout() -> Self {
        Self::decide(
            std::io::stdout().is_terminal(),
            std::env::var_os("NO_COLOR").as_deref(),
            std::env::var_os("TERM").as_deref(),
        )
    }

    /// Color when the output is a terminal, `NO_COLOR` is absent or empty (the convention
    /// says a *non-empty* value turns color off) and the terminal isn't `dumb`.
    pub fn decide(
        tty: bool,
        no_color: Option<&std::ffi::OsStr>,
        term: Option<&std::ffi::OsStr>,
    ) -> Self {
        let refused = no_color.is_some_and(|value| !value.is_empty());
        let dumb = term.is_some_and(|term| term == "dumb");
        Self {
            color: tty && !refused && !dumb,
        }
    }

    /// Whether escape codes go out.
    pub fn is_colored(self) -> bool {
        self.color
    }

    fn wrap(self, code: &str, text: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_owned()
        }
    }

    /// A heading or a key: bold.
    pub fn strong(self, text: &str) -> String {
        self.wrap("1", text)
    }

    /// Secondary text — a meaning, a default: dim.
    pub fn quiet(self, text: &str) -> String {
        self.wrap("2", text)
    }

    /// A value the user chose, a command to type: the terminal's cyan.
    pub fn accent(self, text: &str) -> String {
        self.wrap("36", text)
    }

    /// A shortcut or a changed value: the terminal's yellow.
    pub fn mark(self, text: &str) -> String {
        self.wrap("33", text)
    }

    /// A problem: the terminal's red.
    pub fn alert(self, text: &str) -> String {
        self.wrap("31", text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn color_only_for_a_terminal_that_wants_it() {
        let term = Some(OsStr::new("xterm-256color"));
        assert!(Style::decide(true, None, term).is_colored());
        // Piped: never.
        assert!(!Style::decide(false, None, term).is_colored());
        // `NO_COLOR` with a value turns it off; an empty one doesn't (no-color.org).
        assert!(!Style::decide(true, Some(OsStr::new("1")), term).is_colored());
        assert!(Style::decide(true, Some(OsStr::new("")), term).is_colored());
        assert!(!Style::decide(true, None, Some(OsStr::new("dumb"))).is_colored());
    }

    #[test]
    fn plain_text_carries_no_escape() {
        let plain = Style::plain();
        for text in [
            plain.strong("a"),
            plain.quiet("a"),
            plain.accent("a"),
            plain.mark("a"),
            plain.alert("a"),
        ] {
            assert_eq!(text, "a");
        }
        assert_eq!(Style::colored().strong("a"), "\x1b[1ma\x1b[0m");
    }
}
