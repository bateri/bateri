//! What a terminal pane is born with and what its close leaves: the start directory and first
//! input, its persistent identity, a replayed history or a program handed over by a previous
//! bateri ([`Launch`], [`Adopted`]), the line a pane says when it does not come back whole
//! ([`Note`]), and the handle its close returns ([`Closing`]).
//!
//! The pane's own vocabulary, apart from the windows and tabs that hold panes: whoever opens a
//! pane — bateri's tabs, or an application that embeds one — speaks it.

use std::path::PathBuf;
use std::time::Instant;

use bt_core::{InitialInput, PaneUuid, ShutdownHandle, Teardown};

/// A pane's closing that has begun (`TerminalPane::begin_close`), and so a
/// tab's and a window's, made of their panes'.
pub(crate) enum Closing {
    /// This call started it; the handle knows the result.
    Started(ShutdownHandle),
    /// The closing had begun before (like ⌘Q while the window is closing):
    /// nothing to wait for, the first call knew the real result.
    AlreadyDone,
}

impl Closing {
    /// Waits until `deadline` at the latest ([`ShutdownHandle::wait_until`]).
    pub(crate) fn wait_until(self, deadline: Instant) -> Teardown {
        match self {
            Self::Started(handle) => handle.wait_until(deadline),
            Self::AlreadyDone => Teardown::AlreadyDone,
        }
    }
}

/// The new shell's birth information — the two decisions the birth package
/// (`PaneLaunch::launch`) takes from the caller (`AppDelegate::open_window`).
pub(crate) struct Launch {
    /// Start directory (the active tab's directory, else home).
    pub(crate) working_directory: Option<PathBuf>,
    /// The shell's first input and whether it runs: ⌘T in a
    /// remote tab runs it, a restored remote pane leaves it ready;
    /// `None` → an ordinary local shell.
    pub(crate) initial_input: Option<InitialInput>,
    /// The pane's persistent identity; `None` → a new one. A restored
    /// pane keeps its saved one, so `bateri://tab/<id>` and
    /// `TERM_SESSION_ID` survive the quit.
    pub(crate) uuid: Option<PaneUuid>,
    /// A previous session's scrollback, replayed before the shell starts
    /// (`SessionOptions::replay`); `None` → an empty grid.
    pub(crate) replay: Option<Vec<u8>>,
    /// The update's handover: a running program to carry on
    /// instead of a new shell; `None` → a shell is born.
    pub(crate) adopt: Option<Adopted>,
}

/// A pane the previous bateri froze and its holder gave, checked to be
/// adoptable (`AppDelegate`'s arrival): the master, the exit watch of its
/// child, the pane's state and the bytes to read before the master's.
#[derive(Debug)]
pub(crate) struct Adopted {
    pub(crate) master: std::os::fd::OwnedFd,
    pub(crate) exit: std::os::fd::OwnedFd,
    pub(crate) pid: u32,
    pub(crate) state: crate::handover::PaneState,
    /// The frozen tail, then what the holder drained (`HeldPane::buffer`).
    pub(crate) prefix: Vec<u8>,
    /// The socket of the holder it came from: the pane registers with this
    /// bateri's own holder unconfirmed until that one is acknowledged.
    pub(crate) taken_from: Option<std::path::PathBuf>,
    /// Which kind of holder it came from (`bt_core::AdoptMode`).
    pub(crate) mode: bt_core::AdoptMode,
    /// Its screen did not come back whole: the program is nudged to redraw
    /// it (`Session::nudge_size`).
    pub(crate) nudge: bool,
    /// The note if the session cannot be adopted after all and the pane
    /// falls back to a new shell.
    pub(crate) note: Note,
}

/// The dim line a pane says when it does not come back whole — no pane
/// comes back as a half screen, or as a new shell, without saying so.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Note {
    /// The program did not cross an update.
    Update,
    /// The program was not carried through a crash (its holder did not have
    /// it, or could not give it).
    Crash,
    /// A deliberate handover to a bound holder (a quit, or an update that
    /// found one) did not carry the program — which of the two it was, the
    /// holder cannot tell.
    NotCarried,
    /// The program ended while bateri was closed.
    Ended,
    /// The program runs on, its screen did not come back: it redraws it.
    Screenless,
    /// The program runs on, but the output of while bateri was closed lost
    /// its oldest part.
    Cut,
}

impl Note {
    pub(crate) fn text(self) -> &'static str {
        match self {
            Note::Update => {
                "bateri: the program running here did not survive the update; this is a new shell"
            }
            Note::Crash => {
                "bateri: the program running here did not survive the crash; this is a new shell"
            }
            Note::NotCarried => {
                "bateri: the program running here could not be carried over; this is a new shell"
            }
            Note::Ended => {
                "bateri: the program running here ended while bateri was closed; this is a new shell"
            }
            Note::Screenless => {
                "bateri: the screen could not be restored; the program kept running"
            }
            Note::Cut => "bateri: output from while bateri was closed was cut short",
        }
    }

    /// The note as a line of its own: dim, and the pen reset after it.
    pub(crate) fn line(self) -> Vec<u8> {
        let mut line = b"\x1b[0m\x1b[2m".to_vec();
        line.extend_from_slice(self.text().as_bytes());
        line.extend_from_slice(b"\x1b[0m\r\n");
        line
    }
}

/// `history` (if any) with `note` under it, on a line of its own: the
/// replay of a pane that fell back to a new shell.
pub(crate) fn fallen_back(history: Option<Vec<u8>>, note: Note) -> Vec<u8> {
    let mut replay = history.unwrap_or_default();
    if !replay.is_empty() && !replay.ends_with(b"\n") {
        replay.extend_from_slice(b"\r\n");
    }
    replay.extend_from_slice(&note.line());
    replay
}

#[cfg(test)]
mod tests {
    use super::{Note, fallen_back};

    /// The fallback's note sits on a line of its own under the history,
    /// dim, and resets what the history left on.
    #[test]
    fn the_fallback_note_has_a_line_of_its_own() {
        for kind in [Note::Update, Note::Crash, Note::NotCarried, Note::Ended] {
            let note = format!("\x1b[0m\x1b[2m{}\x1b[0m\r\n", kind.text());
            assert_eq!(kind.line(), note.as_bytes());
            assert_eq!(fallen_back(None, kind), note.as_bytes());
            assert_eq!(fallen_back(Some(Vec::new()), kind), note.as_bytes());
            assert_eq!(
                fallen_back(Some(b"$ ls\r\n".to_vec()), kind),
                [&b"$ ls\r\n"[..], note.as_bytes()].concat()
            );
            assert_eq!(
                fallen_back(Some(b"\x1b[1m$ half".to_vec()), kind),
                [&b"\x1b[1m$ half\r\n"[..], note.as_bytes()].concat()
            );
        }
        // Every event says its own thing, and the update's is today's text.
        assert_eq!(
            Note::Update.text(),
            "bateri: the program running here did not survive the update; this is a new shell"
        );
        let texts = [
            Note::Update,
            Note::Crash,
            Note::NotCarried,
            Note::Ended,
            Note::Screenless,
            Note::Cut,
        ]
        .map(Note::text);
        for (index, text) in texts.iter().enumerate() {
            assert!(!texts[..index].contains(text), "{text}");
        }
    }
}
