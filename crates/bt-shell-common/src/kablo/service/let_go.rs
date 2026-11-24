//! The lines of the log about a pane that is let go of for taking nothing of what its terminal
//! writes ([`super::Core::stalled`]), and about its coming back.
//!
//! A pane that took nothing for [`super::HOLD_STALL`] while the account was held back by it has
//! its terminal's stream stopped, and nothing else in the application shows it: the pane's
//! program goes on, the others' output flows again, and the one that did not read looks the same
//! as before. The log says which pane it was, what it showed, how much it was owed when it was
//! let go of, and how long it was let go of when it read again — one line each way, so that
//! "the page did not come back" and the like can be matched with them by their times.

use std::time::Duration;

use super::super::ledger::ViewMark;
use super::waiting::seconds;

/// How a pane that was let go of is streamed to again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Back {
    /// It has taken all it was owed: it reads.
    Read,
    /// Its terminal is to end and it is to be told, whether it reads or not.
    ToEnd,
}

/// What a pane shows, in the words of a line: a plain terminal, or the view a program opened
/// over it, with the name the program gave itself when the ledger has it.
pub(super) fn shows(view: Option<&ViewMark>) -> String {
    match view {
        None => "a plain terminal".to_owned(),
        Some(ViewMark {
            program: Some(name),
            ..
        }) => format!("the view of the program {name:?}"),
        Some(ViewMark { program: None, .. }) => "the view of a program".to_owned(),
    }
}

/// The line for a pane that took nothing for `stalled` and owed `owed` bytes when it was let go
/// of.
pub(super) fn let_go_line(pane: &str, shows: &str, owed: usize, stalled: Duration) -> String {
    format!(
        "the pane {pane}, showing {shows}, took nothing of what its terminal wrote for {} and the account was held back by it ({owed} bytes owed): the panes beside it go on, and it is given the rest when it reads",
        seconds(stalled),
    )
}

/// The line for a pane that was let go of for `away` and is streamed to again.
pub(super) fn taken_back_line(pane: &str, shows: &str, away: Duration, how: Back) -> String {
    let why = match how {
        Back::Read => "it has taken all it was owed",
        Back::ToEnd => "its terminal is to end and it is told so whether it reads or not",
    };
    format!(
        "the pane {pane}, showing {shows}, was let go of for {}: {why}, and its terminal streams to it again",
        seconds(away),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mark(program: Option<&str>) -> ViewMark {
        ViewMark {
            program: program.map(str::to_owned),
            waits: None,
        }
    }

    #[test]
    fn a_pane_shows_a_plain_terminal_or_the_view_of_a_program_by_its_name() {
        assert_eq!(shows(None), "a plain terminal");
        assert_eq!(
            shows(Some(&mark(Some("claude")))),
            "the view of the program \"claude\""
        );
        assert_eq!(shows(Some(&mark(None))), "the view of a program");
    }

    #[test]
    fn the_line_of_a_pane_let_go_of_says_which_pane_what_it_shows_and_what_it_owed() {
        let line = let_go_line(
            "5A1E2D3C-4B5A-6978-8796-A5B4C3D2E1A1",
            "a plain terminal",
            3_145_728,
            Duration::from_millis(300),
        );
        assert!(
            line.contains("5A1E2D3C-4B5A-6978-8796-A5B4C3D2E1A1"),
            "{line}"
        );
        assert!(line.contains("showing a plain terminal"), "{line}");
        assert!(line.contains("(3145728 bytes owed)"), "{line}");
        assert!(line.contains("for 0.3 s"), "{line}");
    }

    #[test]
    fn the_line_of_a_pane_that_is_back_says_how_long_it_was_let_go_of() {
        let line = taken_back_line(
            "5A1E2D3C-4B5A-6978-8796-A5B4C3D2E1A1",
            "the view of the program \"pen\"",
            Duration::from_millis(12_340),
            Back::Read,
        );
        assert!(line.contains("let go of for 12.3 s"), "{line}");
        assert!(line.contains("the view of the program \"pen\""), "{line}");
        let line = taken_back_line("p", "a plain terminal", Duration::ZERO, Back::ToEnd);
        assert!(line.contains("is to end"), "{line}");
    }
}
