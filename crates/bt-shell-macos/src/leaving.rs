//! The kinds of quit and the small decisions around `keep_running`.
//!
//! What a quit does to the programs is kablo's table
//! ([`bt_shell_common::kablo::quit::fate`]): a pane's terminal lives in kablo and
//! outlives bateri or ends with it by the setting and the kind of quit. This
//! module only names the kind of quit ([`QuitKind`]) and holds the two numbers
//! and questions that are not the table's.

use std::time::Duration;

use bt_core::KeepRunning;

/// The layout's delay after an edge (a window, a tab, a split, the focus, a
/// directory, a close). Edges come in bursts — a split moves the focus, a
/// restore opens many tabs, `cd` retitles — and one second folds a burst into
/// one layout while a crash in that second loses at most the window
/// arrangement of that second.
pub(crate) const LAYOUT_DELAY: Duration = Duration::from_secs(1);

/// Which quit this is — the one input of what the programs do besides the
/// setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QuitKind {
    /// ⌘Q, the red buttons' last window, Dock ▸ Quit, logging out.
    Quit,
    /// bateri ▸ Quit and End Programs (⌥⌘Q, shown under `"quit"`): the
    /// programs end whatever the setting says, after today's question.
    EndPrograms,
    /// Sparkle's relaunch: it never ends the programs — it wins over an
    /// ⌥⌘Q that happened to be pending.
    Relaunch,
}

impl QuitKind {
    /// What this quit is to the terminals the panes show
    /// ([`bt_shell_common::kablo::quit`]): they end with the programs, by the same table.
    pub(crate) fn event(self) -> bt_shell_common::kablo::quit::Event {
        use bt_shell_common::kablo::quit::Event;
        match self {
            QuitKind::Quit => Event::Quit,
            QuitKind::EndPrograms => Event::QuitAndEnd,
            QuitKind::Relaunch => Event::Update,
        }
    }

    /// The kind from Sparkle's relaunch flag and the ⌥⌘Q flag.
    pub(crate) fn of(relaunch: bool, end_programs: bool) -> QuitKind {
        match (relaunch, end_programs) {
            (true, _) => QuitKind::Relaunch,
            (false, true) => QuitKind::EndPrograms,
            (false, false) => QuitKind::Quit,
        }
    }
}

/// Whether the permission to notify is asked for now: when `keep_running`
/// becomes `"quit"` — `before` is the value it had, `None` at launch. ⌘Q's
/// reminder needs the permission and its own moment (quitting) must not ask,
/// so the question comes right after the user chose the value, or at the
/// launch that finds it, while bateri is in front. The system asks once and
/// answers from its record afterwards.
pub(crate) fn asks_notification_permission(before: Option<KeepRunning>, now: KeepRunning) -> bool {
    now == KeepRunning::Quit && before != Some(KeepRunning::Quit)
}

/// What a pane has that a quit ends, as [`told_of_the_end`] asks it.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PaneKeeps {
    /// It shows a terminal a server keeps.
    pub(crate) shown: bool,
    /// Its shell runs in a terminal of this Mac that kablo keeps.
    pub(crate) local: bool,
    /// It chose an ordinary shell over a terminal of this Mac it found again, which runs on
    /// unseen ([`crate::hold_service::left_running`]).
    pub(crate) left_running: bool,
}

/// Whether a quit tells the service of `pane` before its program is signalled: a quit that ends
/// the programs (`ends`, by [`bt_shell_common::kablo::quit::fate`]: Quit and End Programs, and ⌘Q
/// under `"update"` and `"crash"`) ends every terminal the pane has — the one a pane left running
/// when it chose an ordinary shell too, whose program the quit did not ask to outlive it.
pub(crate) fn told_of_the_end(ends: bool, pane: PaneKeeps) -> bool {
    ends && (pane.shown || pane.local || pane.left_running)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pane that left the terminal it found again running for an ordinary shell is told of the
    /// end by every quit that ends the programs — Quit and End Programs whatever the setting, ⌘Q
    /// under `"update"` and `"crash"` — so that its terminal ends with the rest; by no other.
    #[test]
    fn a_quit_that_ends_the_programs_ends_the_terminal_a_pane_left_running() {
        use bt_shell_common::kablo::quit::{Fate, fate};
        let left = PaneKeeps {
            left_running: true,
            ..PaneKeeps::default()
        };
        for keep in VALUES {
            for kind in [QuitKind::Quit, QuitKind::EndPrograms, QuitKind::Relaunch] {
                let ends = fate(keep, kind.event()) == Fate::Ends;
                assert_eq!(told_of_the_end(ends, left), ends, "{keep:?} {kind:?}");
            }
        }
        assert!(told_of_the_end(
            fate(KeepRunning::Update, QuitKind::Quit.event()) == Fate::Ends,
            left
        ));
        assert!(told_of_the_end(
            fate(KeepRunning::Crash, QuitKind::Quit.event()) == Fate::Ends,
            left
        ));
        assert!(told_of_the_end(
            fate(KeepRunning::Quit, QuitKind::EndPrograms.event()) == Fate::Ends,
            left
        ));
        // A pane with nothing kept is not told; one with a terminal is, as before.
        assert!(!told_of_the_end(true, PaneKeeps::default()));
        for pane in [
            PaneKeeps {
                shown: true,
                ..PaneKeeps::default()
            },
            PaneKeeps {
                local: true,
                ..PaneKeeps::default()
            },
        ] {
            assert!(told_of_the_end(true, pane));
            assert!(!told_of_the_end(false, pane));
        }
    }

    const VALUES: [KeepRunning; 3] = [KeepRunning::Update, KeepRunning::Crash, KeepRunning::Quit];

    #[test]
    fn the_kind_of_quit_lets_a_relaunch_win() {
        assert_eq!(QuitKind::of(false, false), QuitKind::Quit);
        assert_eq!(QuitKind::of(false, true), QuitKind::EndPrograms);
        assert_eq!(QuitKind::of(true, false), QuitKind::Relaunch);
        // An update never ends the programs, even with ⌥⌘Q pending.
        assert_eq!(QuitKind::of(true, true), QuitKind::Relaunch);
    }

    #[test]
    fn each_kind_of_quit_is_the_event_kablos_table_knows() {
        use bt_shell_common::kablo::quit::{Fate, fate};
        for keep in VALUES {
            // An update never ends a terminal, whatever the setting.
            assert_eq!(
                fate(keep, QuitKind::Relaunch.event()),
                Fate::Keeps,
                "{keep:?}"
            );
            // Quit and End Programs ends it, whatever the setting.
            assert_eq!(
                fate(keep, QuitKind::EndPrograms.event()),
                Fate::Ends,
                "{keep:?}"
            );
        }
        // ⌘Q keeps the programs only in `"quit"`.
        assert_eq!(fate(KeepRunning::Quit, QuitKind::Quit.event()), Fate::Keeps);
        assert_eq!(fate(KeepRunning::Crash, QuitKind::Quit.event()), Fate::Ends);
        assert_eq!(
            fate(KeepRunning::Update, QuitKind::Quit.event()),
            Fate::Ends
        );
    }

    #[test]
    fn the_permission_is_asked_only_on_becoming_quit() {
        use KeepRunning::{Crash, Quit, Update};
        // At launch: only a value that is already `quit`.
        assert!(asks_notification_permission(None, Quit));
        assert!(!asks_notification_permission(None, Crash));
        assert!(!asks_notification_permission(None, Update));
        // On a save: only the switch to `quit`, not a save that keeps it.
        assert!(asks_notification_permission(Some(Update), Quit));
        assert!(asks_notification_permission(Some(Crash), Quit));
        assert!(!asks_notification_permission(Some(Quit), Quit));
        for before in VALUES {
            for now in [Update, Crash] {
                assert!(!asks_notification_permission(Some(before), now));
            }
        }
    }
}
