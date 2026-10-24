//! The newest command as the shell's ledger knows it ([`CommandState`]): its identity, where it is
//! (its prompt, running, ended), its exit code and how long it ran. Read by
//! [`crate::Session::last_command`]; this module only turns the ledger's block into the state.

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
