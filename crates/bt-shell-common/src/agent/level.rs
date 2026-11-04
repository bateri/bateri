//! What the `bateri` command may do on a machine: [`CommandLevel`] (`off`, `own`, `screen`,
//! `full`) and the one place that turns it into an answer.
//!
//! The level is a property of the **machine that gave the command**, not of who typed it — an
//! agent and a person cannot be told apart — and it is one row of this table:
//!
//! | level    | sees                                   | changes and closes                      | reaches another machine |
//! |----------|----------------------------------------|-----------------------------------------|-------------------------|
//! | `off`    | the command is not there               | nothing                                 | no                      |
//! | `own`    | what holds a pane of its own machine   | what is made of its own machine's panes | no                      |
//! | `screen` | everything bateri has open             | everything                              | no                      |
//! | `full`   | everything                             | everything                              | yes                     |
//!
//! A pane is its machine's; a tab, a group and a window are made of panes, so they are a machine's
//! when every pane in them is. `own` sees a layer that holds at least one pane of its machine and
//! changes it only when it holds no other's: a tab with a pane of another machine is seen but not
//! changed (a refusal, [`Verdict::Refused`]), and a layer with none of its own is not there at all
//! ([`Verdict::Unknown`], the same answer as for a name nobody has: the level protects by what it
//! says, not by how short an id is). "Reaches another machine" is starting a terminal on it and
//! typing into or reading one of its panes.
//!
//! Everything here is a pure function so that the table is tested row by row; the Mac asks it and
//! carries the answer out, and nothing asks the user.

pub use bt_core::CommandLevel;

/// What a command wants to do to a layer or a machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    /// List it.
    See,
    /// Focus, rename, move, resize, close it.
    Change,
    /// Start a terminal on another machine, or type into or read one of its panes.
    Reach,
}

/// The panes a layer holds, by whose they are: the machine that gave the command (`own`) or any
/// other (`other`). A pane is `Panes { own: 1, other: 0 }`; a tab is the sum of its panes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Panes {
    pub own: usize,
    pub other: usize,
}

impl Panes {
    pub const fn own(count: usize) -> Panes {
        Panes {
            own: count,
            other: 0,
        }
    }

    pub const fn other(count: usize) -> Panes {
        Panes {
            own: 0,
            other: count,
        }
    }

    /// This and `more` together: a layer is the sum of what is in it.
    pub const fn and(self, more: Panes) -> Panes {
        Panes {
            own: self.own + more.own,
            other: self.other + more.other,
        }
    }
}

/// Why a command was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// The level is `off`.
    Off,
    /// The layer holds a pane of another machine, and the level is `own`.
    Mixed,
    /// The command reaches another machine, and the level is not `full`.
    Elsewhere,
}

/// The answer to "may it?".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Allowed,
    /// It may not, and it is told why (the command's 77).
    Refused(Why),
    /// It is not there for this level: answered as a name nobody has.
    Unknown,
}

/// Whether `act` on a layer that holds `panes` is allowed at `level`.
pub const fn judge(level: CommandLevel, act: Act, panes: Panes) -> Verdict {
    match (level, act) {
        (CommandLevel::Off, _) => Verdict::Refused(Why::Off),
        (CommandLevel::Own, Act::See) => {
            if panes.own > 0 {
                Verdict::Allowed
            } else {
                Verdict::Unknown
            }
        }
        (CommandLevel::Own, Act::Change) => {
            if panes.own == 0 {
                Verdict::Unknown
            } else if panes.other > 0 {
                Verdict::Refused(Why::Mixed)
            } else {
                Verdict::Allowed
            }
        }
        (CommandLevel::Own | CommandLevel::Screen, Act::Reach) => Verdict::Refused(Why::Elsewhere),
        (CommandLevel::Screen | CommandLevel::Full, _) => Verdict::Allowed,
    }
}

/// How much a machine's `ls` shows of a layer: the id and label (`true`) or only
/// "another machine" (`false`).
pub const fn shown(level: CommandLevel, panes: Panes) -> bool {
    matches!(judge(level, Act::See, panes), Verdict::Allowed)
}

/// The one line a refusal says, for the host the command ran on: what is refused, the level it
/// ran at and the setting that changes it. `host` is the server as the user typed it, or `None`
/// for this Mac.
pub fn refusal(level: CommandLevel, why: Why, host: Option<&str>) -> String {
    let (place, setting) = match host {
        Some(host) => (
            format!("on {host}"),
            format!("`commands` in the [remote] hosts entry for {host}"),
        ),
        None => (
            "on this Mac".to_owned(),
            "`level` in the [commands] section".to_owned(),
        ),
    };
    let wider = match why {
        Why::Off => "own",
        Why::Mixed => "screen",
        Why::Elsewhere => "full",
    };
    let what = match why {
        Why::Off => "bateri commands are off".to_owned(),
        Why::Mixed => {
            "this holds a pane of another machine, which \"own\" does not change".to_owned()
        }
        Why::Elsewhere => "this reaches another machine, which only \"full\" does".to_owned(),
    };
    format!(
        "{what} {place} (level \"{}\"); set {setting} to \"{wider}\" in settings.toml to allow it",
        level.name()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEVELS: [CommandLevel; 4] = [
        CommandLevel::Off,
        CommandLevel::Own,
        CommandLevel::Screen,
        CommandLevel::Full,
    ];

    /// A layer of its own machine's panes, one that is mixed, one of another machine's only.
    const OWN: Panes = Panes::own(2);
    const MIXED: Panes = Panes { own: 1, other: 1 };
    const OTHER: Panes = Panes::other(3);

    #[test]
    fn the_table_holds_row_by_row() {
        use Verdict::{Allowed, Refused, Unknown};
        // (level, act, own layer, mixed layer, another machine's layer)
        let table = [
            (
                CommandLevel::Off,
                Act::See,
                Refused(Why::Off),
                Refused(Why::Off),
                Refused(Why::Off),
            ),
            (
                CommandLevel::Off,
                Act::Change,
                Refused(Why::Off),
                Refused(Why::Off),
                Refused(Why::Off),
            ),
            (
                CommandLevel::Off,
                Act::Reach,
                Refused(Why::Off),
                Refused(Why::Off),
                Refused(Why::Off),
            ),
            (CommandLevel::Own, Act::See, Allowed, Allowed, Unknown),
            (
                CommandLevel::Own,
                Act::Change,
                Allowed,
                Refused(Why::Mixed),
                Unknown,
            ),
            (
                CommandLevel::Own,
                Act::Reach,
                Refused(Why::Elsewhere),
                Refused(Why::Elsewhere),
                Refused(Why::Elsewhere),
            ),
            (CommandLevel::Screen, Act::See, Allowed, Allowed, Allowed),
            (CommandLevel::Screen, Act::Change, Allowed, Allowed, Allowed),
            (
                CommandLevel::Screen,
                Act::Reach,
                Refused(Why::Elsewhere),
                Refused(Why::Elsewhere),
                Refused(Why::Elsewhere),
            ),
            (CommandLevel::Full, Act::See, Allowed, Allowed, Allowed),
            (CommandLevel::Full, Act::Change, Allowed, Allowed, Allowed),
            (CommandLevel::Full, Act::Reach, Allowed, Allowed, Allowed),
        ];
        for (level, act, own, mixed, other) in table {
            assert_eq!(judge(level, act, OWN), own, "{level:?} {act:?} own");
            assert_eq!(judge(level, act, MIXED), mixed, "{level:?} {act:?} mixed");
            assert_eq!(judge(level, act, OTHER), other, "{level:?} {act:?} other");
        }
        // Every level and act is in the table.
        assert_eq!(table.len(), LEVELS.len() * 3);
    }

    #[test]
    fn a_layer_is_the_sum_of_its_panes_and_an_empty_one_is_nobodys() {
        let tab = Panes::own(1).and(Panes::other(2)).and(Panes::own(1));
        assert_eq!(tab, Panes { own: 2, other: 2 });
        assert_eq!(
            judge(CommandLevel::Own, Act::Change, tab),
            Verdict::Refused(Why::Mixed)
        );
        // Nothing in it: it is not own's to see or to change.
        assert_eq!(
            judge(CommandLevel::Own, Act::See, Panes::default()),
            Verdict::Unknown
        );
        assert_eq!(
            judge(CommandLevel::Own, Act::Change, Panes::default()),
            Verdict::Unknown
        );
    }

    #[test]
    fn what_ls_shows_follows_what_a_level_sees() {
        assert!(!shown(CommandLevel::Own, OTHER));
        assert!(shown(CommandLevel::Own, MIXED));
        assert!(shown(CommandLevel::Own, OWN));
        for level in [CommandLevel::Screen, CommandLevel::Full] {
            assert!(shown(level, OTHER), "{level:?}");
        }
        assert!(!shown(CommandLevel::Off, OWN));
    }

    #[test]
    fn a_refusal_says_the_level_and_the_setting_that_changes_it() {
        let text = refusal(CommandLevel::Own, Why::Mixed, Some("rasp"));
        assert!(text.contains("\"own\""), "{text}");
        assert!(text.contains("rasp"), "{text}");
        assert!(text.contains("\"screen\""), "{text}");
        assert!(text.contains("[remote] hosts"), "{text}");
        let text = refusal(CommandLevel::Screen, Why::Elsewhere, None);
        assert!(text.contains("[commands]"), "{text}");
        assert!(text.contains("\"full\""), "{text}");
        let text = refusal(CommandLevel::Off, Why::Off, Some("prod"));
        assert!(text.contains("off"), "{text}");
        assert!(!text.contains('\n'));
    }
}
