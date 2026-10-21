//! What becomes of the terminals bateri keeps on servers when something happens
//! to bateri: the one table, so that the quit, the update, the crash and the
//! launch after them all read the same cell.
//!
//! **One rule for the user: a terminal on a server is a local terminal that does
//! not die with the connection.** What the user chose for the programs here
//! (`[terminal] keep_running`) is what the terminals there get — nothing is
//! learned about the server — and what a lost connection or a closed lid does is
//! nothing at all, in every row: that is the point of keeping them.
//!
//! |  `keep_running` | connection lost | update | crash | ⌘Q | ⌥⌘Q (Quit and End Programs) |
//! |---|---|---|---|---|---|
//! | `"update"` | keeps | keeps | ends at the next launch | ends | ends |
//! | `"crash"` | keeps | keeps | keeps | ends | ends |
//! | `"quit"` | keeps | keeps | keeps | keeps, no question | ends |
//!
//! "Ends" after the usual question: the quit's question about running jobs
//! (`confirm_close`) counts a terminal's program in front like a local one, and
//! counts it only when this table says the terminal ends — a quit that leaves the
//! terminals alone does not ask about them. A crash cannot say anything while it
//! happens, so the terminals a crash leaves behind under `"update"` are ended by
//! the next launch ([`crate::kablo::ledger::Ledger::end_all`]) unless that launch
//! is the update's own (the programs came over: they are not left behind).
//! ⌥⌘Q exists only under `"quit"` (the menu hides it otherwise); under the other
//! two values it would be ⌘Q, and the table says so.

use bt_core::KeepRunning;

/// What happens to bateri.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// The connection to the server went, the lid closed, the network changed.
    ConnectionLost,
    /// bateri quit to come back newer.
    Update,
    /// bateri died.
    Crash,
    /// ⌘Q.
    Quit,
    /// ⌥⌘Q, Quit and End Programs.
    QuitAndEnd,
}

/// What happens to a terminal a server keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fate {
    /// It runs on; the panes show it again when bateri is back.
    Keeps,
    /// It is ended — after the question about running jobs, when `confirm_close`
    /// asks one — before bateri is gone.
    Ends,
    /// It runs on until bateri is back, which ends it before showing anything.
    EndsAtTheNextLaunch,
}

/// The table's cell.
pub fn fate(keep: KeepRunning, event: Event) -> Fate {
    match (event, keep) {
        (Event::ConnectionLost | Event::Update, _) => Fate::Keeps,
        (Event::Crash, KeepRunning::Update) => Fate::EndsAtTheNextLaunch,
        (Event::Crash, KeepRunning::Crash | KeepRunning::Quit) => Fate::Keeps,
        (Event::Quit, KeepRunning::Quit) => Fate::Keeps,
        (Event::Quit, KeepRunning::Update | KeepRunning::Crash) => Fate::Ends,
        (Event::QuitAndEnd, _) => Fate::Ends,
    }
}

/// Whether a launch ends the terminals the bateri before it left in the ledger: after a
/// crash under `"update"`, and only when the programs did not come over (`carried`: the
/// update's own relaunch, or a holder that kept them through the crash).
pub fn ends_leftovers(keep: KeepRunning, carried: bool) -> bool {
    !carried && fate(keep, Event::Crash) == Fate::EndsAtTheNextLaunch
}

#[cfg(test)]
mod tests;
