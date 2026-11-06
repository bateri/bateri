//! The text a program in a pane and the app say to each other (`bateri 2`): the requests the
//! `bateri` command makes, the replies the window gives, and the strict reader and the writer of
//! both. It is written once, in `kablo-bateri`, in kablo's repository where the command runs
//! (`kablo bateri`), and what is here is that crate's, with what only this side adds: what a
//! pane that a request asked for runs ([`runs`]), which is the app's decision and needs its types.
//! The frozen text of every version, and the tests that hold it, are in `kablo-bateri`.

use crate::child::PaneRuns;

pub use kablo_bateri::message::*;

/// What the pane a request asks for runs: no `fresh` is a program the command started in the
/// terminal, `fresh` is a shell for the bateri app to start, the one named or the user's.
pub fn runs(place: &Place) -> PaneRuns {
    match (&place.fresh, &place.shell) {
        (None, _) => PaneRuns::Program {
            keep_open: place.keep_open,
        },
        (Some(_), Some(shell)) => PaneRuns::Shell(std::path::PathBuf::from(shell)),
        (Some(_), None) => PaneRuns::UserShell,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Anchor, Terminal};

    fn place() -> Place {
        Place {
            terminal: Terminal(9),
            anchor: Some(Anchor::of(Terminal(4))),
            side: None,
            ratio: None,
            keep_open: false,
            focus: false,
            float: None,
            home: Home::default(),
            fresh: Some("/tmp".to_owned()),
            shell: Some("/opt/bin/fish".to_owned()),
            server: None,
        }
    }

    #[test]
    fn what_the_pane_runs_follows_from_the_request_with_no_second_word_for_it() {
        assert_eq!(runs(&place()), PaneRuns::Shell("/opt/bin/fish".into()));
        let own = Place {
            shell: None,
            ..place()
        };
        assert_eq!(runs(&own), PaneRuns::UserShell);
        let program = Place {
            fresh: None,
            shell: None,
            keep_open: true,
            ..place()
        };
        assert_eq!(runs(&program), PaneRuns::Program { keep_open: true });
    }
}
