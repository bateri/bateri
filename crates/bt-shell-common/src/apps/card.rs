//! The card a server asks with, once: the programs the person follows that the server does not
//! have. What it says and what each key does is here; the views are the shell's.
//!
//! The server is asked about itself, not the other way round: the `package state` its watch sent
//! says which programs it has and which it was told it need not have (`not-here`)
//! ([`super::package::MachineState`]). The programs the person follows that are in neither, and
//! that their author has built for the server's kind of computer, are the rows. Only a program
//! that landed is a row, and a program lands with its key approved, so the card never asks the
//! person about a key. *Install* has the server's own kablo bring them
//! (`bateri package install`, run on the server's account, as the person would type it) and
//! writes the rows left unchecked into the server's `not-here`; *Not Here* writes all the names
//! there, so that it does not ask again.

use std::fs;
use std::path::Path;

use bt_core::pencere::apps;

use super::followed;
use super::package::{MachineState, public};
use super::store;

/// The most rows a card lists: what a server's `not-here` takes in one line of words.
const ROWS_MAX: usize = 40;

/// A program the server could be brought.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub name: String,
    /// The words of the `bateri` command that brings it (`package install SOURCE [--trust KEY]`).
    pub words: Vec<String>,
}

/// The followed programs `machine` lacks and would be sent a file for, in the order of their
/// names. A program whose source only this computer can reach (a `file:` source, a name on the
/// local network) is not a row: the server would be refused it, so asking would only fail.
pub fn missing(root: &Path, machine: &MachineState) -> Vec<Row> {
    store::followed_names(root)
        .into_iter()
        .filter(|(name, _)| !machine.installed.contains(name) && !machine.not_here.contains(name))
        .filter(|(_, address)| public(address))
        .filter(|(name, _)| has_file_for(root, name, &machine.target))
        .take(ROWS_MAX)
        .map(|(name, address)| Row {
            words: followed::install_words(root, &name, &address),
            name,
        })
        .collect()
}

/// Whether the release of `name` that landed lists a file for `target`. A folder that does not
/// hold the release it is named for, or that does not verify, has none: nothing is offered from
/// what cannot be vouched for.
fn has_file_for(root: &Path, name: &str, target: &str) -> bool {
    let Some(version) = store::newest(root, name) else {
        return false;
    };
    let folder = store::release_dir(root, name, version);
    let (Ok(bytes), Ok(signature)) = (
        fs::read(folder.join("app.json")),
        fs::read(folder.join("app.json.sig")),
    ) else {
        return false;
    };
    apps::verify(&bytes, &signature)
        .is_ok_and(|manifest| manifest.name == name && manifest.files.contains_key(target))
}

/// What decides whether a server is asked now, besides what it lacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ground {
    /// The account is this computer's own: its programs are brought without asking.
    pub local: bool,
    /// The server gets shell integration (`remote.integration`, or the host's own entry): a server
    /// that does not has no terminal here that a card could hang over.
    pub integration: bool,
    /// The server is marked Production: nothing asks about it, whatever the integration says.
    pub production: bool,
    /// The person has answered a card of this server (installed, or said Not Here): once is once.
    pub answered: bool,
    /// A card of this server is showing.
    pub open: bool,
}

impl Ground {
    /// Whether a card may be raised for the server.
    pub fn may_ask(self) -> bool {
        !self.local && self.integration && !self.production && !self.answered && !self.open
    }
}

/// A pane the tabs have, as the card sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Candidate {
    /// It shows a terminal of the server.
    pub on_server: bool,
    /// Its shell has come to a prompt: a card does not ask for a decision in a shell's opening
    /// moment.
    pub prompted: bool,
}

/// The pane the card hangs over, among `panes` in the order of the tabs and of the splits in each:
/// the first that shows the server and has come to a prompt.
pub fn pane_for(panes: &[Candidate]) -> Option<usize> {
    panes
        .iter()
        .position(|pane| pane.on_server && pane.prompted)
}

/// Where the card is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Asking: the boxes are the person's to check.
    Ask,
    /// Bringing the checked programs, one after the other.
    Installing,
    /// Every one that was to come has tried and some did not.
    Failed,
    /// Every one that was to come is there.
    Done,
}

/// What a row shows beside its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// Nothing yet (while asking, the row's box is shown instead).
    Idle,
    Busy,
    Done,
    Failed,
}

/// A row and what the person and the server have done with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub row: Row,
    /// Whether its box is checked.
    pub on: bool,
    pub mark: Mark,
    /// Why it failed, in a line.
    pub why: String,
}

/// The card of one server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    host: String,
    stage: Stage,
    lines: Vec<Line>,
}

impl Card {
    /// A card for `host` asking about `rows`, all of them checked. `None` when there are none:
    /// a server that lacks nothing is not asked.
    pub fn new(host: &str, rows: Vec<Row>) -> Option<Self> {
        if rows.is_empty() {
            return None;
        }
        Some(Self {
            host: host.to_owned(),
            stage: Stage::Ask,
            lines: rows
                .into_iter()
                .map(|row| Line {
                    row,
                    on: true,
                    mark: Mark::Idle,
                    why: String::new(),
                })
                .collect(),
        })
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn stage(&self) -> Stage {
        self.stage
    }

    pub fn lines(&self) -> &[Line] {
        &self.lines
    }

    /// The card's title.
    pub fn title(&self) -> String {
        match self.stage {
            Stage::Ask => format!("Programs you follow aren\u{2019}t on {}", self.host),
            Stage::Installing | Stage::Failed => format!("Installing on {}", self.host),
            Stage::Done => format!("Ready on {}", self.host),
        }
    }

    /// Checks or unchecks the box of line `i`; only while asking.
    pub fn toggle(&mut self, i: usize) {
        if self.stage != Stage::Ask {
            return;
        }
        if let Some(line) = self.lines.get_mut(i) {
            line.on = !line.on;
        }
    }

    /// Whether *Install* does something: asking, with a box checked.
    pub fn can_install(&self) -> bool {
        self.stage == Stage::Ask && self.lines.iter().any(|line| line.on)
    }

    /// *Install*: the unchecked rows leave the card, and the first of the rest is being
    /// brought. Returns its index; `None` (and nothing changes) when it cannot be pressed.
    pub fn begin(&mut self) -> Option<usize> {
        if !self.can_install() {
            return None;
        }
        self.lines.retain(|line| line.on);
        self.stage = Stage::Installing;
        self.advance()
    }

    /// Line `i` has come or failed (`why` is a line for the person); the next one to bring is
    /// started and returned, and when there is none the card is `Done` or `Failed`.
    pub fn settle(&mut self, i: usize, outcome: Result<(), String>) -> Option<usize> {
        if self.stage != Stage::Installing {
            return None;
        }
        if let Some(line) = self.lines.get_mut(i) {
            match outcome {
                Ok(()) => line.mark = Mark::Done,
                Err(why) => {
                    line.mark = Mark::Failed;
                    line.why = why;
                }
            }
        }
        let next = self.advance();
        if next.is_none() {
            self.stage = if self.lines.iter().any(|line| line.mark == Mark::Failed) {
                Stage::Failed
            } else {
                Stage::Done
            };
        }
        next
    }

    /// *Try Again*: the rows that failed are brought again. Returns the first; `None` when none
    /// failed or the card is not showing a failure.
    pub fn retry(&mut self) -> Option<usize> {
        if self.stage != Stage::Failed {
            return None;
        }
        for line in &mut self.lines {
            if line.mark == Mark::Failed {
                line.mark = Mark::Idle;
                line.why.clear();
            }
        }
        self.stage = Stage::Installing;
        self.advance()
    }

    /// The first line that waits is the one being brought now.
    fn advance(&mut self) -> Option<usize> {
        let i = self.lines.iter().position(|line| line.mark == Mark::Idle)?;
        if let Some(line) = self.lines.get_mut(i) {
            line.mark = Mark::Busy;
        }
        Some(i)
    }

    /// The words that bring line `i`.
    pub fn words(&self, i: usize) -> Option<&[String]> {
        self.lines.get(i).map(|line| line.row.words.as_slice())
    }

    /// *Install*: the words that tell the server the rows whose boxes the person unchecked are not
    /// wanted there (`package not-here NAME…`), so that it does not ask about them again; `None`
    /// when every box is checked. Read before [`Card::begin`], which takes those rows off the
    /// card.
    pub fn declined_words(&self) -> Option<Vec<String>> {
        let names: Vec<&str> = self
            .lines
            .iter()
            .filter(|line| !line.on)
            .map(|line| line.row.name.as_str())
            .collect();
        if names.is_empty() {
            return None;
        }
        let mut words = vec!["package".to_owned(), "not-here".to_owned()];
        words.extend(names.into_iter().map(str::to_owned));
        Some(words)
    }

    /// *Not Here*: the words that tell the server none of the rows the card lists is wanted
    /// (`package not-here NAME…`).
    pub fn not_here_words(&self) -> Vec<String> {
        let mut words = vec!["package".to_owned(), "not-here".to_owned()];
        words.extend(self.lines.iter().map(|line| line.row.name.clone()));
        words
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(names: &[&str]) -> Vec<Row> {
        names
            .iter()
            .map(|name| Row {
                name: (*name).to_owned(),
                words: vec![
                    "package".to_owned(),
                    "install".to_owned(),
                    format!("https://example.org/{name}/app.json"),
                ],
            })
            .collect()
    }

    fn card(names: &[&str]) -> Card {
        Card::new("dev-1", rows(names)).expect("a card")
    }

    #[test]
    fn a_server_that_lacks_nothing_is_not_asked() {
        assert!(Card::new("dev-1", Vec::new()).is_none());
    }

    #[test]
    fn it_asks_about_every_row_checked_and_says_whose_server_it_is() {
        let card = card(&["textpen", "notes"]);
        assert_eq!(card.title(), "Programs you follow aren\u{2019}t on dev-1");
        assert!(card.lines().iter().all(|line| line.on));
        assert!(card.can_install());
    }

    #[test]
    fn install_needs_a_box_checked_and_leaves_out_the_ones_that_are_not() {
        let mut card = card(&["textpen", "notes"]);
        card.toggle(0);
        card.toggle(1);
        assert!(!card.can_install());
        assert_eq!(card.begin(), None);
        assert_eq!(card.stage(), Stage::Ask);
        card.toggle(1);
        assert_eq!(card.begin(), Some(0));
        let names: Vec<&str> = card
            .lines()
            .iter()
            .map(|line| line.row.name.as_str())
            .collect();
        assert_eq!(names, ["notes"]);
        assert_eq!(card.stage(), Stage::Installing);
        assert_eq!(card.title(), "Installing on dev-1");
        // The boxes are not the person's any more.
        card.toggle(0);
        assert!(card.lines()[0].on);
    }

    #[test]
    fn the_rows_are_brought_one_after_the_other_and_the_card_is_ready_after_the_last() {
        let mut card = card(&["textpen", "notes"]);
        assert_eq!(card.begin(), Some(0));
        assert_eq!(card.lines()[1].mark, Mark::Idle);
        assert_eq!(card.settle(0, Ok(())), Some(1));
        assert_eq!(card.lines()[0].mark, Mark::Done);
        assert_eq!(card.lines()[1].mark, Mark::Busy);
        assert_eq!(card.settle(1, Ok(())), None);
        assert_eq!(card.stage(), Stage::Done);
        assert_eq!(card.title(), "Ready on dev-1");
    }

    #[test]
    fn a_row_that_fails_does_not_stop_the_others_and_is_tried_again_alone() {
        let mut card = card(&["textpen", "notes", "sketch"]);
        assert_eq!(card.begin(), Some(0));
        assert_eq!(
            card.settle(0, Err("No space left on this machine.".to_owned())),
            Some(1)
        );
        assert_eq!(card.settle(1, Ok(())), Some(2));
        assert_eq!(card.settle(2, Ok(())), None);
        assert_eq!(card.stage(), Stage::Failed);
        assert_eq!(card.title(), "Installing on dev-1");
        assert_eq!(card.lines()[0].why, "No space left on this machine.");

        assert_eq!(card.retry(), Some(0));
        assert_eq!(card.stage(), Stage::Installing);
        assert_eq!(card.lines()[0].why, "");
        assert_eq!(card.lines()[1].mark, Mark::Done);
        assert_eq!(card.settle(0, Ok(())), None);
        assert_eq!(card.stage(), Stage::Done);
        assert_eq!(card.retry(), None);
    }

    #[test]
    fn not_here_names_every_row_the_card_lists() {
        let card = card(&["textpen", "notes"]);
        assert_eq!(
            card.not_here_words(),
            ["package", "not-here", "textpen", "notes"]
        );
    }

    #[test]
    fn a_settle_after_the_card_stopped_asking_changes_nothing() {
        let mut card = card(&["textpen"]);
        assert_eq!(card.settle(0, Ok(())), None);
        assert_eq!(card.stage(), Stage::Ask);
        assert_eq!(card.lines()[0].mark, Mark::Idle);
    }

    #[test]
    fn a_card_is_raised_for_a_server_with_integration_that_nobody_was_asked_about() {
        let ground = Ground {
            local: false,
            integration: true,
            production: false,
            answered: false,
            open: false,
        };
        assert!(ground.may_ask());
        for (what, ground) in [
            (
                "this computer",
                Ground {
                    local: true,
                    ..ground
                },
            ),
            (
                "no integration",
                Ground {
                    integration: false,
                    ..ground
                },
            ),
            (
                "production",
                Ground {
                    production: true,
                    ..ground
                },
            ),
            (
                "answered",
                Ground {
                    answered: true,
                    ..ground
                },
            ),
            (
                "open",
                Ground {
                    open: true,
                    ..ground
                },
            ),
        ] {
            assert!(!ground.may_ask(), "{what}");
        }
    }

    #[test]
    fn the_card_hangs_over_the_first_pane_of_the_server_that_has_come_to_a_prompt() {
        let pane = |on_server, prompted| Candidate {
            on_server,
            prompted,
        };
        assert_eq!(pane_for(&[]), None);
        // Another server's pane, and a pane of the server whose shell is still opening.
        assert_eq!(pane_for(&[pane(false, true), pane(true, false)]), None);
        assert_eq!(
            pane_for(&[
                pane(false, true),
                pane(true, false),
                pane(true, true),
                pane(true, true)
            ]),
            Some(2)
        );
    }

    #[test]
    fn the_rows_left_unchecked_are_what_install_tells_the_server_it_need_not_have() {
        let mut card = Card::new("dev-1", rows(&["textpen", "notes", "pickpen"])).expect("a card");
        // Every box checked: there is nothing to say.
        assert_eq!(card.declined_words(), None);
        card.toggle(1);
        card.toggle(2);
        assert_eq!(
            card.declined_words(),
            Some(vec![
                "package".to_owned(),
                "not-here".to_owned(),
                "notes".to_owned(),
                "pickpen".to_owned()
            ])
        );
        // The checked one is what is brought; the others have left the card.
        assert_eq!(card.begin(), Some(0));
        assert_eq!(card.declined_words(), None);
    }
}
