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

use crate::web_links::Question;

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

/// The row for the program `name` on the server `machine` said it has, for a person who just
/// asked for it by clicking a link ([`Card::for_link`]): the program is followed, its release
/// landed and lists a file for the server's kind of computer, and its source can be reached from
/// the server. What the server was told it need not have (`not-here`) does not keep it from this:
/// the person is asking now. `None` when any of that is missing, and the link opens in the
/// system's browser.
pub fn row_for(root: &Path, machine: &MachineState, name: &str) -> Option<Row> {
    let address = store::followed(root, name)?;
    (public(&address) && has_file_for(root, name, &machine.target)).then(|| Row {
        words: followed::install_words(root, name, &address),
        name: name.to_owned(),
    })
}

/// What decides whether a server is asked now, besides what it lacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ground {
    /// The account is this computer's own: its programs are brought without asking.
    pub local: bool,
    /// The server gets shell integration (`remote.integration`, or the host's own entry): a server
    /// that does not has no terminal here that a card could hang over. A Production mark is not
    /// part of it: it colors a server and decides nothing else.
    pub integration: bool,
    /// The person has answered a card of this server (installed, or said Not Here): once is once.
    pub answered: bool,
    /// A card of this server is showing.
    pub open: bool,
}

impl Ground {
    /// Whether a card may be raised for the server.
    pub fn may_ask(self) -> bool {
        !self.local && self.integration && !self.answered && !self.open
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
    /// What is under its name while it is on its way: how far it has come (the first click's card;
    /// a server's rows say nothing until they fail).
    pub note: String,
}

/// The card of one server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    host: String,
    stage: Stage,
    lines: Vec<Line>,
    /// The web address a person clicked that the program is being brought to open: set for the
    /// card [`Card::for_link`] makes, and opened when the program is there ([`Card::link`]).
    link: Option<String>,
    /// Set for the card of this Mac's one question ([`Card::for_here`]): what it says and where it
    /// is in its life are its own; its rows are what its yes brings, shown once it is answered
    /// (and not when the answer could not be saved, [`Card::not_saved`]).
    here: Option<Here>,
    /// Set for a notice ([`Card::for_waiting_view`], [`Card::for_given_up_windows`]): a card that
    /// says one thing and has one answer, and no rows.
    notice: Option<Notice>,
}

/// What a notice says: its title, a line under it (or none) and its one answer.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Notice {
    title: String,
    note: String,
    yes: &'static str,
}

/// Why a program's view waits behind a card instead of being open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// bateri quit unexpectedly while it was open.
    Quit,
    /// A page of it failed in bateri's own code.
    Stopped,
}

/// The one answer of the card of a view that waits.
pub const REOPEN: &str = "Reopen";

/// The one answer of the card of a launch that gave the windows up.
pub const OPEN_THEM: &str = "Open Them";

/// What the card of a view that waits tells of the pane.
const END_HINT: &str = "Press \u{2303}C in this pane to end it.";

/// What the card of a launch that gave the windows up says.
const GAVE_UP: &str =
    "bateri didn\u{2019}t open your windows after quitting unexpectedly. They are kept.";

impl Why {
    /// The card's title: what the program is called, when it is known.
    fn title(self, program: Option<&str>) -> String {
        match (self, program) {
            (Why::Quit, Some(name)) => format!("{name} was open when bateri quit unexpectedly."),
            (Why::Quit, None) => {
                "A program\u{2019}s view was open here when bateri quit unexpectedly.".to_owned()
            }
            (Why::Stopped, Some(name)) => format!("{name}\u{2019}s view stopped."),
            (Why::Stopped, None) => "A program\u{2019}s view stopped.".to_owned(),
        }
    }
}

/// What the card of the first click's question knows besides the address and its rows (what is not
/// on this Mac yet, a row each): the question it asks, the size of the engine's download, and why
/// it could not be started at all.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Here {
    question: Question,
    /// The engine's download in bytes, when its release notice said.
    engine_bytes: Option<u64>,
    /// Why the yes could not be carried out at all (the choice could not be written): nothing was
    /// started, so no row says it; the card shows it under its title, in place of its rows.
    why: String,
}

/// How one thing the first click's yes brings stands, as the shell found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Coming {
    /// On its way; for the engine, the percent when it is known.
    Busy(Option<u8>),
    /// It is here.
    Done,
    /// It could not be had: a plain sentence.
    Failed(String),
}

/// The names of the rows of the first click's card: what its yes brings.
pub const PROGRAM_ROW: &str = "browserpen";
pub const ENGINE_ROW: &str = "Browser engine";

/// What a yes to the first click's card asks the shell to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Yes {
    /// Everything is on this Mac: write the choice, and open the address in the program.
    Use,
    /// Write the choice, follow browserpen and download what is not here; the address opens in it
    /// when it is.
    Download,
}

/// What the other answer of the first click's card asks the shell to do: open `url` in the system's
/// browser, and write `""` (browserpen is not wanted) only when it is the question that is
/// answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Browser {
    pub url: String,
    pub writes: bool,
}

/// The title of the first click's card when its yes could not be written (the reason is under it).
pub const NOT_SAVED: &str = "Your choice couldn\u{2019}t be saved";

/// What is under the engine's name when every byte has come and it is being checked and put in
/// place (a few seconds in which the percent would stay at 100).
pub const ENGINE_FINISHING: &str = "Installing\u{2026}";

/// The line under the first click's question that says where the answer can be changed.
pub const CHANGE_LATER: &str = "You can change this in Settings\u{a0}▸\u{a0}Apps.";

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
                    note: String::new(),
                })
                .collect(),
            link: None,
            here: None,
            notice: None,
        })
    }

    /// The card of a click on the web address `url` in a pane of `host`, which lacks the program
    /// `row` names: *Install and Open* brings the program and then the address opens in it; the
    /// other answer opens the address in the system's browser.
    pub fn for_link(host: &str, row: Row, url: &str) -> Self {
        Self {
            host: host.to_owned(),
            stage: Stage::Ask,
            lines: vec![Line {
                row,
                on: true,
                mark: Mark::Idle,
                why: String::new(),
                note: String::new(),
            }],
            link: Some(url.to_owned()),
            here: None,
            notice: None,
        }
    }

    /// The card of the first click on the web address `url`, before anything is decided: it asks
    /// whether to open links beside the terminal with browserpen over the pane clicked in, and the
    /// address opens by the answer. `program_here` and `engine_here` say what is on this Mac
    /// already; what is not is what its yes brings, a row each, and `engine_bytes` is the engine's
    /// download as its release notice gave it.
    pub fn for_here(
        url: &str,
        program_here: bool,
        engine_here: bool,
        engine_bytes: Option<u64>,
    ) -> Self {
        let row = |name: &str| Line {
            row: Row {
                name: name.to_owned(),
                words: Vec::new(),
            },
            on: true,
            mark: Mark::Idle,
            why: String::new(),
            note: String::new(),
        };
        let mut lines = Vec::new();
        if !program_here {
            lines.push(row(PROGRAM_ROW));
        }
        if !engine_here {
            lines.push(row(ENGINE_ROW));
        }
        Self {
            host: "this Mac".to_owned(),
            stage: Stage::Ask,
            lines,
            link: Some(url.to_owned()),
            here: Some(Here {
                question: crate::web_links::question(program_here, engine_here, engine_bytes),
                engine_bytes,
                why: String::new(),
            }),
            notice: None,
        }
    }

    /// The card of a program's view that waits (`program`: what it calls itself, when the ledger
    /// or the view said): bateri quit unexpectedly with it open, or a page of it failed; it is
    /// not opened by itself, and its one answer opens it.
    pub fn for_waiting_view(program: Option<&str>, why: Why) -> Self {
        Self::notice(why.title(program), END_HINT, REOPEN)
    }

    /// The card of a launch that did not open the windows after bateri quit unexpectedly: they
    /// are kept, and its one answer opens them.
    pub fn for_given_up_windows() -> Self {
        Self::notice(GAVE_UP.to_owned(), "", OPEN_THEM)
    }

    fn notice(title: String, note: &str, yes: &'static str) -> Self {
        Self {
            host: String::new(),
            stage: Stage::Ask,
            lines: Vec::new(),
            link: None,
            here: None,
            notice: Some(Notice {
                title,
                note: note.to_owned(),
                yes,
            }),
        }
    }

    /// The one answer of a notice ([`Card::for_waiting_view`]); `None` for every other card.
    pub fn notice_answer(&self) -> Option<&'static str> {
        self.notice.as_ref().map(|notice| notice.yes)
    }

    /// Whether this is the card of this Mac's one question ([`Card::for_here`]).
    pub fn is_here(&self) -> bool {
        self.here.is_some()
    }

    /// The lines under the title of the card of this Mac's question: while it asks, what it would
    /// download and where the answer can be changed; when its yes could not be carried out at all,
    /// why. None otherwise (its rows say how far each thing has come).
    pub fn notes(&self) -> Vec<String> {
        if let Some(notice) = &self.notice {
            return Some(notice.note.clone())
                .filter(|note| !note.is_empty())
                .into_iter()
                .collect();
        }
        let Some(here) = &self.here else {
            return Vec::new();
        };
        match self.stage {
            Stage::Ask => here
                .question
                .detail
                .iter()
                .cloned()
                .chain([CHANGE_LATER.to_owned()])
                .collect(),
            Stage::Failed if !here.why.is_empty() => vec![here.why.clone()],
            _ => Vec::new(),
        }
    }

    /// The title of the yes button of the first click's card: what it does says what it brings.
    pub fn yes_title(&self) -> &'static str {
        match &self.here {
            Some(here) if here.question.what.is_none() => "Use browserpen",
            _ => "Download and Open",
        }
    }

    /// The first click's card says yes: what is not here comes, a row of the card each, on its way.
    /// `None` (and nothing changes) when it is not asking.
    pub fn say_yes(&mut self) -> Option<Yes> {
        let here = self.here.as_ref()?;
        if self.stage != Stage::Ask {
            return None;
        }
        let yes = if here.question.what.is_none() {
            Yes::Use
        } else {
            Yes::Download
        };
        self.stage = Stage::Installing;
        for line in &mut self.lines {
            line.mark = Mark::Busy;
        }
        Some(yes)
    }

    /// The other answer: the address opens in the system's browser and the card goes. Only the
    /// answer to the question writes anything (`""`); to open the address in the browser now while
    /// the download goes on, or after it failed, is not a change of mind. `None` when it is done
    /// with already.
    pub fn say_no(&self) -> Option<Browser> {
        self.here.as_ref()?;
        let writes = match self.stage {
            Stage::Ask => true,
            Stage::Installing | Stage::Failed => false,
            Stage::Done => return None,
        };
        Some(Browser {
            url: self.link.clone()?,
            writes,
        })
    }

    /// What the shell found of the program and of the engine, laid on the rows that are there: a
    /// thing on its way spins (the engine says how far, in a line under its name), one that is here
    /// is a green dot, one that could not be had says why under its name. Stage follows the rows as
    /// a server's card does: ready when each is here, failed when none is on its way and some could
    /// not be had, on its way while one is. Returns the stage. Only a card that waits for its yes
    /// (or for a *Try Again*) takes it.
    pub fn sync_here(&mut self, program: &Coming, engine: &Coming) -> Stage {
        let Some(here) = self.here.as_ref() else {
            return self.stage;
        };
        // A yes that could not be carried out (nothing was started) is not read as what a download
        // is doing: only *Try Again* sets it going again.
        if !matches!(self.stage, Stage::Installing | Stage::Failed) || !here.why.is_empty() {
            return self.stage;
        }
        let bytes = here.engine_bytes;
        for line in &mut self.lines {
            let (coming, is_engine) = match line.row.name.as_str() {
                ENGINE_ROW => (engine, true),
                _ => (program, false),
            };
            line.why.clear();
            line.note.clear();
            match coming {
                Coming::Busy(percent) => {
                    line.mark = Mark::Busy;
                    if let (true, Some(percent)) = (is_engine, percent) {
                        line.note = match bytes {
                            // Every byte is here: what is left is the check and the unpacking.
                            _ if *percent >= 100 => ENGINE_FINISHING.to_owned(),
                            Some(bytes) => {
                                format!("{percent}% of {}", crate::web_links::megabytes(bytes))
                            }
                            None => format!("{percent}%"),
                        };
                    }
                }
                Coming::Done => line.mark = Mark::Done,
                Coming::Failed(why) => {
                    line.mark = Mark::Failed;
                    line.why.clone_from(why);
                }
            }
        }
        self.stage = if self.lines.iter().any(|line| line.mark == Mark::Busy) {
            Stage::Installing
        } else if self.lines.iter().any(|line| line.mark == Mark::Failed) {
            Stage::Failed
        } else {
            Stage::Done
        };
        self.stage
    }

    /// What its yes asked could not be carried out at all (the choice could not be written):
    /// nothing was started, so no row says anything and the card shows none ([`Card::not_saved`]);
    /// `why`, a plain sentence, stands under its title, which says the choice was not saved.
    pub fn fail(&mut self, why: &str) {
        let Some(here) = self.here.as_mut() else {
            return;
        };
        if !matches!(self.stage, Stage::Ask | Stage::Installing) {
            return;
        }
        self.stage = Stage::Failed;
        here.why = why.to_owned();
    }

    /// *Try Again*: what could not be had is asked for again, on its way. `false` when the card
    /// shows no failure.
    pub fn try_again(&mut self) -> bool {
        let Some(here) = self.here.as_mut() else {
            return false;
        };
        if self.stage != Stage::Failed {
            return false;
        }
        here.why.clear();
        for line in &mut self.lines {
            if line.mark != Mark::Done {
                line.mark = Mark::Busy;
                line.why.clear();
                line.note.clear();
            }
        }
        self.stage = Stage::Installing;
        true
    }

    /// Whether the card of the first click's question stays a moment when it is ready. Where
    /// everything was here already, its yes had nothing to bring and there is nothing to say
    /// "ready" about: the address opens and the card goes at once.
    pub fn stays_when_ready(&self) -> bool {
        !self.lines.is_empty()
    }

    /// Whether the first click's card says its yes could not be written (nothing was started, so
    /// it shows no rows: they would say things were tried that were not).
    pub fn not_saved(&self) -> bool {
        self.stage == Stage::Failed && self.here.as_ref().is_some_and(|here| !here.why.is_empty())
    }

    /// The address to open when the program is there; `None` for a card that asks about the
    /// programs the person follows.
    pub fn link(&self) -> Option<&str> {
        self.link.as_deref()
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
        if let Some(notice) = &self.notice {
            return notice.title.clone();
        }
        if let Some(here) = &self.here {
            match self.stage {
                Stage::Ask => return here.question.title.clone(),
                // Nothing was started: the choice itself could not be written, and the line under
                // the title says why.
                Stage::Failed if !here.why.is_empty() => return NOT_SAVED.to_owned(),
                _ => {}
            }
        }
        match self.stage {
            Stage::Ask if self.link.is_some() => {
                let name = self.lines.first().map_or("", |line| line.row.name.as_str());
                format!("{name} isn\u{2019}t on {}", self.host)
            }
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
        self.stage == Stage::Ask
            && (self.here.is_some()
                || self.notice.is_some()
                || self.lines.iter().any(|line| line.on))
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

    /// A click on a web address asks about the one program that opens it, by its name and the
    /// server's, brings it, and keeps the address for when it is there.
    #[test]
    fn a_link_card_asks_about_its_program_brings_it_and_keeps_the_address() {
        let [row] = <[Row; 1]>::try_from(rows(&["browserpen"])).expect("one row");
        let mut link_card = Card::for_link("rasp", row, "http://localhost:8790");
        assert_eq!(link_card.title(), "browserpen isn\u{2019}t on rasp");
        assert_eq!(link_card.link(), Some("http://localhost:8790"));
        assert_eq!(link_card.stage(), Stage::Ask);
        assert!(link_card.can_install());
        assert_eq!(link_card.begin(), Some(0));
        assert_eq!(link_card.title(), "Installing on rasp");
        assert_eq!(link_card.settle(0, Ok(())), None);
        assert_eq!(link_card.stage(), Stage::Done);
        assert_eq!(link_card.title(), "Ready on rasp");
        // The address is still the card's: what opens it is the shell's to do once.
        assert_eq!(link_card.link(), Some("http://localhost:8790"));
        // A card of the programs the person follows has none.
        assert_eq!(card(&["textpen"]).link(), None);
    }

    fn here_card(program_here: bool, engine_here: bool) -> Card {
        Card::for_here(
            "http://localhost:8790",
            program_here,
            engine_here,
            Some(181_000_000),
        )
    }

    fn names(card: &Card) -> Vec<&str> {
        card.lines()
            .iter()
            .map(|line| line.row.name.as_str())
            .collect()
    }

    /// The first click asks, in the words the note's question has, what it would download and where
    /// the answer can be changed; its buttons are the two answers.
    #[test]
    fn the_card_of_the_first_click_asks_what_it_would_download() {
        let asking = here_card(false, false);
        assert!(asking.is_here());
        assert_eq!(
            asking.title(),
            "Open links beside your terminal with browserpen?"
        );
        assert_eq!(
            asking.notes(),
            [
                "It downloads browserpen and the browser engine (181 MB).",
                "You can change this in Settings\u{a0}▸\u{a0}Apps."
            ]
        );
        assert_eq!(asking.stage(), Stage::Ask);
        assert!(asking.can_install());
        assert_eq!(asking.link(), Some("http://localhost:8790"));
        assert_eq!(asking.yes_title(), "Download and Open");
        // A row for each thing that is missing, and for nothing that is here.
        assert_eq!(names(&asking), ["browserpen", "Browser engine"]);
        assert_eq!(names(&here_card(true, false)), ["Browser engine"]);
        assert_eq!(names(&here_card(false, true)), ["browserpen"]);
        let engine = here_card(true, false);
        assert_eq!(
            engine.notes()[0],
            "It downloads the browser engine (181 MB)."
        );
        // Everything here: no size, no rows, and the yes says what it does.
        let all = here_card(true, true);
        assert!(all.lines().is_empty());
        assert_eq!(
            all.notes(),
            ["You can change this in Settings\u{a0}▸\u{a0}Apps."]
        );
        assert_eq!(all.yes_title(), "Use browserpen");
        // A card of the programs the person follows is none of this.
        assert!(!card(&["textpen"]).is_here());
        assert!(card(&["textpen"]).notes().is_empty());
    }

    /// Its rows go as a server's do: on its way, a green dot, a triangle with the reason; the card
    /// is ready when each is here, and the title is the server card's, for this Mac.
    #[test]
    fn yes_puts_each_missing_thing_on_its_way_and_the_card_follows_them_to_ready() {
        let mut card = here_card(false, false);
        assert_eq!(card.say_yes(), Some(Yes::Download));
        assert_eq!(card.stage(), Stage::Installing);
        assert_eq!(card.title(), "Installing on this Mac");
        assert!(card.notes().is_empty());
        assert!(card.lines().iter().all(|line| line.mark == Mark::Busy));
        // One yes only.
        assert_eq!(card.say_yes(), None);
        // The engine says how far, under its name, with its size; the program has nothing to say.
        assert_eq!(
            card.sync_here(&Coming::Busy(None), &Coming::Busy(Some(42))),
            Stage::Installing
        );
        assert_eq!(card.lines()[0].note, "");
        assert_eq!(card.lines()[1].note, "42% of 181 MB");
        // The program is here, the engine is not: still on its way.
        assert_eq!(
            card.sync_here(&Coming::Done, &Coming::Busy(Some(99))),
            Stage::Installing
        );
        assert_eq!(card.lines()[0].mark, Mark::Done);
        assert_eq!(card.lines()[1].note, "99% of 181 MB");
        assert_eq!(card.sync_here(&Coming::Done, &Coming::Done), Stage::Done);
        assert_eq!(card.title(), "Ready on this Mac");
        assert!(card.lines().iter().all(|line| line.mark == Mark::Done));
        assert!(card.lines().iter().all(|line| line.note.is_empty()));
        assert_eq!(card.link(), Some("http://localhost:8790"));
        // A card that is ready takes no more, and is done with answering.
        assert_eq!(
            card.sync_here(&Coming::Busy(None), &Coming::Busy(None)),
            Stage::Done
        );
        assert_eq!(card.say_no(), None);
        assert!(!card.try_again());
    }

    #[test]
    fn a_size_that_is_not_known_leaves_the_percent_alone() {
        let mut card = Card::for_here("http://localhost:8790", true, false, None);
        card.say_yes();
        card.sync_here(&Coming::Done, &Coming::Busy(Some(7)));
        assert_eq!(card.lines()[0].note, "7%");
        // Before any figure there is nothing under the name.
        card.sync_here(&Coming::Done, &Coming::Busy(None));
        assert_eq!(card.lines()[0].note, "");
    }

    #[test]
    fn when_every_byte_has_come_the_engine_says_it_is_installing_and_the_ring_goes_on() {
        for bytes in [Some(150_000_000), None] {
            let mut card = Card::for_here("http://localhost:8790", true, false, bytes);
            card.say_yes();
            card.sync_here(&Coming::Done, &Coming::Busy(Some(99)));
            assert!(card.lines()[0].note.starts_with("99%"), "{bytes:?}");
            // The download is whole but the engine is not in place: the row is still on its way.
            assert_eq!(
                card.sync_here(&Coming::Done, &Coming::Busy(Some(100))),
                Stage::Installing
            );
            assert_eq!(card.lines()[0].mark, Mark::Busy);
            assert_eq!(card.lines()[0].note, ENGINE_FINISHING);
            assert_eq!(ENGINE_FINISHING, "Installing\u{2026}");
            // Then the green dot, with nothing under the name.
            assert_eq!(card.sync_here(&Coming::Done, &Coming::Done), Stage::Done);
            assert_eq!(card.lines()[0].mark, Mark::Done);
            assert_eq!(card.lines()[0].note, "");
        }
    }

    #[test]
    fn where_everything_is_already_here_yes_has_nothing_to_download() {
        let mut card = here_card(true, true);
        assert_eq!(card.say_yes(), Some(Yes::Use));
        // No row is on its way: the first look finds it ready, with nothing to show for it, so the
        // card does not stay.
        assert!(card.lines().is_empty());
        assert!(!card.stays_when_ready());
        assert_eq!(card.sync_here(&Coming::Done, &Coming::Done), Stage::Done);
    }

    #[test]
    fn a_card_that_brought_something_stays_a_moment_when_it_is_ready() {
        for (program_here, engine_here) in [(false, false), (true, false), (false, true)] {
            let mut card = here_card(program_here, engine_here);
            card.say_yes();
            assert!(card.stays_when_ready());
            assert_eq!(card.sync_here(&Coming::Done, &Coming::Done), Stage::Done);
            assert_eq!(card.title(), "Ready on this Mac");
        }
    }

    #[test]
    fn the_other_answer_writes_only_while_the_question_is_asked() {
        let mut card = here_card(false, false);
        let no = card.say_no().expect("the question can be answered no");
        assert_eq!(no.url, "http://localhost:8790");
        // The question answered no: `""` is written, so that it is not asked again.
        assert!(no.writes);
        // Open in Your Browser Now: the address goes to the browser, the download goes on, and the
        // setting stays what the yes wrote.
        assert_eq!(card.say_yes(), Some(Yes::Download));
        let now = card
            .say_no()
            .expect("the address can go to the browser now");
        assert!(!now.writes);
        assert_eq!(card.stage(), Stage::Installing);
        // After a failure the same.
        card.sync_here(
            &Coming::Done,
            &Coming::Failed("No connection to the internet.".to_owned()),
        );
        assert_eq!(card.stage(), Stage::Failed);
        assert!(!card.say_no().expect("after a failure too").writes);
    }

    /// A row that could not be had says why under its name, in a plain sentence; the title stays
    /// the server card's, *Try Again* sets the rows that failed going again, and the others stay.
    #[test]
    fn a_failure_says_why_under_the_row_and_try_again_sets_it_going_again() {
        let mut card = here_card(false, false);
        assert_eq!(card.say_yes(), Some(Yes::Download));
        // The engine failed while the program is still on its way: the card is not failed yet.
        assert_eq!(
            card.sync_here(
                &Coming::Busy(None),
                &Coming::Failed("The server answered 503.".to_owned())
            ),
            Stage::Installing
        );
        assert_eq!(
            card.sync_here(
                &Coming::Done,
                &Coming::Failed("The server answered 503.".to_owned())
            ),
            Stage::Failed
        );
        assert_eq!(card.title(), "Installing on this Mac");
        assert_eq!(card.lines()[0].mark, Mark::Done);
        assert_eq!(card.lines()[1].mark, Mark::Failed);
        assert_eq!(card.lines()[1].why, "The server answered 503.");
        assert!(card.notes().is_empty());
        assert!(card.try_again());
        assert_eq!(card.stage(), Stage::Installing);
        assert_eq!(card.lines()[0].mark, Mark::Done);
        assert_eq!(card.lines()[1].mark, Mark::Busy);
        assert_eq!(card.lines()[1].why, "");
        // Nothing asks again of a card that did not fail, and nothing fails one that is ready.
        assert!(!card.try_again());
        card.sync_here(&Coming::Done, &Coming::Done);
        card.fail("late");
        assert_eq!(card.stage(), Stage::Done);
    }

    /// A yes that could not be written starts nothing: the card has a title of its own, says why
    /// under it and shows no row (none of them was tried), with rows to be had or without any.
    #[test]
    fn a_yes_that_could_not_be_written_says_why_under_a_title_of_its_own_and_shows_no_row() {
        let mut card = here_card(false, true);
        assert_eq!(card.say_yes(), Some(Yes::Download));
        card.fail("settings.toml is not available.");
        assert_eq!(card.stage(), Stage::Failed);
        // Nothing was tried: no row is marked as if it had been.
        assert_eq!(card.lines()[0].mark, Mark::Busy);
        assert_eq!(card.notes(), ["settings.toml is not available."]);
        // The card says what failed: the choice, not an install, and shows no row for what was
        // never tried.
        assert_eq!(card.title(), NOT_SAVED);
        assert!(card.not_saved());
        assert_eq!(NOT_SAVED, "Your choice couldn\u{2019}t be saved");
        // What the shell finds afterwards (the check of the programs ended, nothing is downloading)
        // does not turn it back into a download that is on its way.
        assert_eq!(
            card.sync_here(&Coming::Busy(None), &Coming::Busy(None)),
            Stage::Failed
        );
        assert_eq!(card.lines()[0].mark, Mark::Busy);
        assert_eq!(card.notes(), ["settings.toml is not available."]);
        assert!(card.try_again());
        assert_eq!(card.stage(), Stage::Installing);
        assert_eq!(card.lines()[0].mark, Mark::Busy);
        assert!(card.notes().is_empty());
        // Tried again, it is an install again, with its rows.
        assert_eq!(card.title(), "Installing on this Mac");
        assert!(!card.not_saved());
        let mut all = here_card(true, true);
        assert_eq!(all.say_yes(), Some(Yes::Use));
        all.fail("settings.toml is not available.");
        assert_eq!(all.notes(), ["settings.toml is not available."]);
        // With nothing to install it is the same title, and the same line under it.
        assert_eq!(all.title(), NOT_SAVED);
        // A download that failed is still an install that failed.
        let mut failed = here_card(false, true);
        failed.say_yes();
        failed.sync_here(
            &Coming::Failed("No connection to the internet.".to_owned()),
            &Coming::Done,
        );
        assert_eq!(failed.title(), "Installing on this Mac");
        assert!(!failed.not_saved());
        // Nor is the question, or a card of a server, ever "not saved".
        assert!(!here_card(false, false).not_saved());
        assert!(
            !Card::new("dev-1", rows(&["textpen"]))
                .expect("a card")
                .not_saved()
        );
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
    #[test]
    fn the_card_of_a_view_that_waits_names_its_program_and_has_one_answer() {
        let quit = Card::for_waiting_view(Some("linearpen"), Why::Quit);
        assert_eq!(
            quit.title(),
            "linearpen was open when bateri quit unexpectedly."
        );
        assert_eq!(quit.notes(), ["Press \u{2303}C in this pane to end it."]);
        assert_eq!(quit.notice_answer(), Some("Reopen"));
        assert_eq!(quit.stage(), Stage::Ask);
        assert!(quit.lines().is_empty() && quit.can_install() && !quit.is_here());
        // A page of it failed: its own words, and the same hint and answer.
        let stopped = Card::for_waiting_view(Some("linearpen"), Why::Stopped);
        assert_eq!(stopped.title(), "linearpen\u{2019}s view stopped.");
        assert_eq!(stopped.notes(), quit.notes());
        // A row an earlier bateri wrote has no name.
        assert_eq!(
            Card::for_waiting_view(None, Why::Quit).title(),
            "A program\u{2019}s view was open here when bateri quit unexpectedly."
        );
        assert_eq!(
            Card::for_waiting_view(None, Why::Stopped).title(),
            "A program\u{2019}s view stopped."
        );
    }

    #[test]
    fn the_card_of_a_launch_that_gave_the_windows_up_says_they_are_kept() {
        let card = Card::for_given_up_windows();
        assert_eq!(
            card.title(),
            "bateri didn\u{2019}t open your windows after quitting unexpectedly. They are kept."
        );
        assert!(card.notes().is_empty());
        assert_eq!(card.notice_answer(), Some("Open Them"));
        assert!(card.can_install());
        // The cards of programs are no notices.
        assert_eq!(
            Card::for_link("rasp", rows(&["a"]).remove(0), "https://x").notice_answer(),
            None
        );
    }
}
