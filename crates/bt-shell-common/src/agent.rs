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
//!   panicking. The text and its reader are `kablo-bateri`'s, which the `bateri` command in kablo
//!   writes with; the version's frozen text is in its tests.
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

// The names a pane is written by (`p9`, `p9@prod`) and the machine words are the app's and the
// command's alike: one definition, in the crate both read the message with.
pub use kablo_bateri::{Anchor, LOCAL_MACHINE, LOCAL_SERVER, Terminal, is_machine};
