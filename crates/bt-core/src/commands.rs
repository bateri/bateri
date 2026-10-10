//! Commands one by one, for a host that keeps a timeline of them: what the newest command is doing
//! as the shell's ledger knows it ([`CommandState`]) and what changed between two looks
//! ([`news`]) — a command started, a command finished.
//!
//! bateri's own tabs read counts ([`crate::Activity`]: how many ended, how many failed) and need
//! no more. A host that shows each command — its line, its exit code, how long it ran — looks at
//! the newest one on every activity edge and asks [`news`] what happened since its last look.
//!
//! **A look can miss nothing but a command it never saw.** The edges arrive coalesced: between two
//! looks the newest command may have started and finished, or a new prompt may have replaced it.
//! [`news`] reports the steps the look did not see — a start it missed comes before the finish —
//! and the command the last look saw is read again by its identity, so its finish is not lost when
//! a newer one has taken its place. Only a command that came and went entirely between two looks
//! (typeahead of several commands at once) is not reported.
//!
//! Pure: the ledger is read by [`crate::Session::last_command`] and
//! [`crate::Session::command`]; this module only compares.

use std::time::Duration;

use crate::shell::{BlockKey, Outcome, ShellLog};

/// A command's identity: its block in the shell's ledger — the local shell's, or our remote
/// shell's. Equal only for the same command of one session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandId(pub(crate) BlockKey);

/// Where a command is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandPhase {
    /// Its prompt is up: nothing has run yet.
    Prompt,
    /// It runs.
    Running,
    /// It ended.
    Finished,
}

/// The newest command as the ledger knows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandState {
    pub id: CommandId,
    /// It runs in our remote shell, on the far end of ssh.
    pub remote: bool,
    pub phase: CommandPhase,
    /// The exit code, once it ended; `None` while it runs or when the shell printed it in a form
    /// that could not be read.
    pub exit: Option<i32>,
    /// How long it ran, once it ended; `None` when its start was never seen.
    pub elapsed: Option<Duration>,
    /// When it started, seconds since the Unix epoch; `None` when unknown.
    pub started: Option<u32>,
}

impl CommandState {
    /// It ran at all: its start was seen. A prompt answered with nothing (an empty line) ends
    /// without having run, and is not a command.
    fn ran(self) -> bool {
        self.started.is_some() || self.elapsed.is_some()
    }
}

/// Block `key` as a command, as `log` knows it: running from its `C` to its `D`, a prompt before.
pub(crate) fn command_state(log: &ShellLog, key: BlockKey) -> Option<CommandState> {
    let (outcome, running) = log.outcome(key)?;
    let remote = matches!(key, BlockKey::Remote { .. });
    let stamped = |started: u32| (started != 0).then_some(started);
    Some(match outcome {
        Outcome::Finished {
            exit,
            elapsed_ms,
            started,
        } if !running => CommandState {
            id: CommandId(key),
            remote,
            phase: CommandPhase::Finished,
            exit,
            // Zero is the ledger's "never saw its start" too, unless the start was stamped.
            elapsed: (elapsed_ms > 0 || started != 0)
                .then(|| Duration::from_millis(elapsed_ms.into())),
            started: stamped(started),
        },
        Outcome::Pending { started } | Outcome::Finished { started, .. } => CommandState {
            id: CommandId(key),
            remote,
            phase: if running {
                CommandPhase::Running
            } else {
                CommandPhase::Prompt
            },
            exit: None,
            elapsed: None,
            started: stamped(started),
        },
    })
}

/// What happened to a command between two looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandNews {
    /// It started; its state as of this look.
    Started(CommandState),
    /// It ended; its exit code and duration are in its state.
    Finished(CommandState),
}

/// What happened between two looks. `before` is the newest command the last look saw;
/// `before_now` is that same command as the ledger knows it now — read again by its identity when
/// a newer one has taken its place, `None` otherwise; `now` is the newest command now.
pub fn news(
    before: Option<CommandState>,
    before_now: Option<CommandState>,
    now: Option<CommandState>,
) -> Vec<CommandNews> {
    let mut out = Vec::new();
    let Some(now) = now else {
        return out;
    };
    match before {
        Some(before) if before.id == now.id => moved(Some(before.phase), now, &mut out),
        Some(before) => {
            if let Some(old) = before_now.filter(|old| old.id == before.id) {
                moved(Some(before.phase), old, &mut out);
            }
            moved(None, now, &mut out);
        }
        None => moved(None, now, &mut out),
    }
    out
}

/// The news of one command that was at `from` (`None`: never seen) and is at `to` now.
fn moved(from: Option<CommandPhase>, to: CommandState, out: &mut Vec<CommandNews>) {
    let seen_start = matches!(from, Some(CommandPhase::Running | CommandPhase::Finished));
    match to.phase {
        CommandPhase::Prompt => {}
        CommandPhase::Running => {
            if !seen_start {
                out.push(CommandNews::Started(to));
            }
        }
        CommandPhase::Finished => {
            if from == Some(CommandPhase::Finished) || (!to.ran() && !seen_start) {
                return;
            }
            if !seen_start {
                out.push(CommandNews::Started(to));
            }
            out.push(CommandNews::Finished(to));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(id: u32, phase: CommandPhase) -> CommandState {
        CommandState {
            id: CommandId(BlockKey::Local(id)),
            remote: false,
            phase,
            exit: (phase == CommandPhase::Finished).then_some(0),
            elapsed: (phase == CommandPhase::Finished).then_some(Duration::from_millis(1200)),
            started: (phase != CommandPhase::Prompt).then_some(1_700_000_000),
        }
    }

    use CommandPhase::{Finished, Prompt, Running};

    #[test]
    fn each_step_is_told_once() {
        let (prompt, running, done) = (state(1, Prompt), state(1, Running), state(1, Finished));
        assert_eq!(news(None, None, Some(prompt)), []);
        assert_eq!(
            news(Some(prompt), None, Some(running)),
            [CommandNews::Started(running)]
        );
        assert_eq!(news(Some(running), None, Some(running)), []);
        assert_eq!(
            news(Some(running), None, Some(done)),
            [CommandNews::Finished(done)]
        );
        assert_eq!(news(Some(done), None, Some(done)), []);
    }

    #[test]
    fn a_start_the_look_missed_comes_before_the_finish() {
        let done = state(1, Finished);
        assert_eq!(
            news(Some(state(1, Prompt)), None, Some(done)),
            [CommandNews::Started(done), CommandNews::Finished(done)]
        );
        assert_eq!(
            news(None, None, Some(done)),
            [CommandNews::Started(done), CommandNews::Finished(done)]
        );
    }

    #[test]
    fn a_command_replaced_by_a_newer_one_still_tells_its_end() {
        let (old, newer) = (state(1, Finished), state(2, Running));
        assert_eq!(
            news(Some(state(1, Running)), Some(old), Some(newer)),
            [CommandNews::Finished(old), CommandNews::Started(newer)]
        );
        // The old one gone from the ledger: only the new one's news.
        assert_eq!(
            news(Some(state(1, Running)), None, Some(newer)),
            [CommandNews::Started(newer)]
        );
        // A prompt nobody ran anything at, replaced by the next prompt: nothing.
        assert_eq!(
            news(
                Some(state(1, Prompt)),
                Some(state(1, Prompt)),
                Some(state(2, Prompt))
            ),
            []
        );
    }

    #[test]
    fn an_empty_line_is_not_a_command() {
        let empty = CommandState {
            exit: Some(0),
            elapsed: None,
            started: None,
            ..state(1, Finished)
        };
        assert_eq!(news(Some(state(1, Prompt)), None, Some(empty)), []);
        assert_eq!(news(None, None, Some(empty)), []);
    }

    #[test]
    fn nothing_now_is_no_news() {
        assert_eq!(news(Some(state(1, Running)), None, None), []);
    }
}
