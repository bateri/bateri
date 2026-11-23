//! Where a web address ⌘-clicked in a terminal opens (`[apps] open_links_with`): in the program the
//! person chose, beside the terminal and on the computer the terminal is on, or in the system's
//! browser — and, for the first click that decides nothing, nowhere yet: a card over the pane asks
//! first, and the address opens by what it is answered.
//!
//! The decision is pure ([`route`]) and reads only what this computer already knows: what it
//! follows and has installed, what a server last said it has, whether the browser engine is on
//! disk, what the last look at the releases found ([`Lookup`]), and which panes of the tab run
//! which program. **A click never waits for the network**: a program that is not there is the
//! system's browser, and the first click asks only when the releases were read before it (at
//! launch, with the check of the programs). The platform shell reads that state, holds the panes
//! and does what the route says.
//!
//! A file Command-clicked in a pane of a tab with a program that takes files open takes it as a new
//! tab of that program ([`take_pane`]); the same rule picks the pane for an address.

use bt_core::SettingsEdit;
use bt_core::pencere::TakeKind;
use bt_core::pencere::apps::{self, Opens};

use crate::apps::opens::Opener;
use crate::apps::{State, Status};

/// The program an address opens in that needs the browser engine: its pages are drawn by the
/// engine this computer has (the helper), whichever machine the program runs on.
pub const BROWSER_PROGRAM: &str = "browserpen";

/// Where the terminal clicked in is, as far as what was clicked is concerned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Machine<'a> {
    /// This computer: the programs it follows, and which of them open what.
    Here { openers: &'a [Opener] },
    /// A server: the programs it last said it has installed (`None` while it has said nothing —
    /// not yet, or a level of `off` that never says), and whether this computer follows the
    /// program, which is what lets it install one there.
    Server {
        installed: Option<&'a [String]>,
        followed: bool,
    },
}

/// A pane of the tab that runs a view, on the machine of the terminal clicked in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pane<K> {
    /// What the shell calls the pane by.
    pub key: K,
    /// What the program calls itself (its `open`'s `program`).
    pub program: Option<String>,
    /// What it said it takes while it runs.
    pub takes: Vec<TakeKind>,
}

/// What the route is decided from.
#[derive(Clone, Debug)]
pub struct Facts<'a, K> {
    /// `[apps] open_links_with`: `None` is not decided, `""` is the system's browser.
    pub setting: Option<&'a str>,
    /// Where the terminal is.
    pub machine: Machine<'a>,
    /// The browser engine is on this computer's disk.
    pub engine: bool,
    /// What is not here of the program and the engine can be had (the last look at the releases
    /// found both, [`Lookup::offered`]): what makes the first click worth a question.
    pub offered: bool,
    /// What the person said yes to is coming (the engine is downloading, or the programs are
    /// being checked): a click does not start it again.
    pub fetching: bool,
    /// The card of the first click's question is up, over a pane: one question at a time.
    pub asking: bool,
    /// The panes of the tab on that machine that run a view, the one the person was last in first.
    pub panes: &'a [Pane<K>],
}

/// What the click does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route<K> {
    /// The system's browser.
    System,
    /// Nothing opens yet: the card over the pane asks whether to open links beside the terminal
    /// with browserpen (and downloads what is not here when it is said yes), and the address opens
    /// by the answer.
    Ask,
    /// The system's browser, and what was said yes to comes again: `"browserpen"` is written but
    /// the engine or the program is not here and nothing is bringing it (the last download did
    /// not finish, or somebody deleted it). The address is not kept waiting for it.
    SystemThenFetch,
    /// A new tab of the program already running in this pane, which comes forward.
    Take(K),
    /// A floating pane that starts the program with the address.
    Start,
    /// A server that does not have the program: the card that offers to install it there and
    /// open the address, or to open it in the system's browser.
    Card,
}

/// Decides where the web address of a click opens.
pub fn route<K: Clone>(facts: &Facts<'_, K>) -> Route<K> {
    let Some(setting) = facts.setting else {
        return if facts.offered && !facts.asking {
            Route::Ask
        } else {
            Route::System
        };
    };
    if setting.is_empty() || !apps::is_name(setting) {
        return Route::System;
    }
    // Where what was said yes to is missing and nothing is bringing it, the click brings it again
    // and the address goes to the system's browser meanwhile.
    let missing = || {
        if facts.fetching {
            Route::System
        } else {
            Route::SystemThenFetch
        }
    };
    // The engine is this computer's even when the program runs on a server: a pane here draws it.
    if setting == BROWSER_PROGRAM && !facts.engine {
        return missing();
    }
    match facts.machine {
        Machine::Here { openers } => {
            let named = openers.iter().find(|opener| opener.name == setting);
            match named {
                Some(opener) if opener.opens.contains(&Opens::Url) => {}
                // Not on this Mac: it is brought again. One that is, and does not open addresses
                // (a release from before it did), is not made to by looking again at every click.
                None if setting == BROWSER_PROGRAM => return missing(),
                _ => return Route::System,
            }
        }
        Machine::Server {
            installed: None, ..
        } => return Route::System,
        Machine::Server {
            installed: Some(installed),
            followed,
        } => {
            if !installed.iter().any(|name| name == setting) {
                return if followed { Route::Card } else { Route::System };
            }
        }
    }
    take_pane(facts.panes, setting, TakeKind::Url).map_or(Route::Start, Route::Take)
}

/// The pane to hand a `kind` of thing to: the first of `panes` (the one the person was last in
/// first) whose program calls itself `name` and said it takes that kind.
pub fn take_pane<K: Clone>(panes: &[Pane<K>], name: &str, kind: TakeKind) -> Option<K> {
    panes
        .iter()
        .find(|pane| pane.program.as_deref() == Some(name) && pane.takes.contains(&kind))
        .map(|pane| pane.key.clone())
}

/// What one look at a release came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    /// Not looked at yet in this run.
    Unread,
    /// The release is there to be had.
    Found,
    /// There is none (nothing published, or none for this Mac): looking again finds the same, so
    /// it is not looked at again in this run.
    Nothing,
    /// The network did not answer: looked at again when the check of the programs runs again
    /// (its retry rhythm: five minutes, and longer after each).
    Unreadable,
}

/// What the releases of browserpen and of the browser engine looked like when they were last read
/// (at launch, in the turn of the check of the programs, and not at a click: a click never waits
/// for the network).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lookup {
    pub program: Look,
    pub engine: Look,
    /// The engine's download, in bytes, from its release notice.
    pub engine_bytes: Option<u64>,
}

/// What one reading of the releases found of the things it looked at: `None` for one it did not.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reading {
    pub program: Option<Look>,
    pub engine: Option<(Look, Option<u64>)>,
}

impl Lookup {
    /// Nothing was read.
    pub const UNREAD: Self = Self {
        program: Look::Unread,
        engine: Look::Unread,
        engine_bytes: None,
    };

    /// Whether the click is worth a question: what is not on the disk can be had. A thing that is
    /// here is not looked at, so a Mac that has both asks without any look at the network.
    pub fn offered(&self, program_here: bool, engine_here: bool) -> bool {
        (program_here || self.program == Look::Found) && (engine_here || self.engine == Look::Found)
    }

    /// What is to be read now: the things that are not here and not yet read, or that could not
    /// be read last time. `None` when there is nothing to read.
    pub fn wants(&self, program_here: bool, engine_here: bool) -> Option<(bool, bool)> {
        let again =
            |here: bool, look: Look| !here && matches!(look, Look::Unread | Look::Unreadable);
        let wanted = (
            again(program_here, self.program),
            again(engine_here, self.engine),
        );
        (wanted.0 || wanted.1).then_some(wanted)
    }

    /// What a reading found, laid over what was known.
    pub fn read(&mut self, reading: Reading) {
        if let Some(look) = reading.program {
            self.program = look;
        }
        if let Some((look, bytes)) = reading.engine {
            self.engine = look;
            self.engine_bytes = bytes.or(self.engine_bytes).filter(|_| look == Look::Found);
        }
    }
}

/// Whether the route of an address the first click's card has just got a program for needs the pane's
/// corner: a server that has no browserpen gets its own card (*Install and Open*), and the corner has
/// one place, so the first card goes at once. Every other route opens beside the terminal or in the
/// system's browser, and the first card stays to say it is ready.
pub fn frees_the_corner<K>(route: &Route<K>) -> bool {
    matches!(route, Route::Card)
}

/// The words of the one-time question, from what is on this computer: what it would download.
/// `engine_bytes` is the engine's size, known from the release notice; `None` when it is not
/// known, and the size is left out of the words when the engine is here.
pub fn question(program_here: bool, engine_here: bool, engine_bytes: Option<u64>) -> Question {
    const TITLE: &str = "Open links beside your terminal with browserpen?";
    let what = match (program_here, engine_here) {
        (true, true) => {
            return Question {
                title: TITLE.to_owned(),
                detail: None,
                what: None,
            };
        }
        (false, true) => "browserpen",
        (true, false) => "the browser engine",
        (false, false) => "browserpen and the browser engine",
    };
    let detail = match engine_bytes.filter(|_| !engine_here) {
        Some(bytes) => format!("It downloads {what} ({}).", megabytes(bytes)),
        None => format!("It downloads {what}."),
    };
    Question {
        title: TITLE.to_owned(),
        detail: Some(detail),
        what: Some(what.to_owned()),
    }
}

/// What the first click's card and the settings' sheet say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Question {
    /// The question.
    pub title: String,
    /// What saying yes downloads; `None` when everything is here already.
    pub detail: Option<String>,
    /// What it downloads, as the words go in a sentence (`browserpen and the browser engine`);
    /// `None` when everything is here.
    pub what: Option<String>,
}

impl Question {
    /// The whole note in one line, as the documents' words have it.
    #[must_use]
    pub fn line(&self) -> String {
        match &self.detail {
            Some(detail) => format!("{} {detail}", self.title),
            None => self.title.clone(),
        }
    }
}

/// What the browser engine on this computer is, as the Apps page's Links row says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Engine {
    /// Not on this computer, and nothing is bringing it.
    Missing,
    /// The release notice is being read.
    Looking,
    /// Coming: the percent when the size is known.
    Downloading(Option<u8>),
    /// It could not be downloaded: what went wrong, as the download said it.
    Failed(String),
    /// On the disk: the version of its release (a development copy that runs an app where it is
    /// has none to say) and its size in bytes, when it was measured.
    Here {
        version: Option<String>,
        bytes: Option<u64>,
    },
}

/// The source browserpen is followed from.
pub const BROWSER_SOURCE: &str = "bateri/browserpen";

/// What following browserpen writes to `[apps] sources`: the list with it, or nothing when it is
/// there (the entries are compared by the address they stand for, so `bateri/browserpen` and its
/// full address are one source). `Err` with a line for the person when the list could not be read
/// whole: a list that is not whole is not rewritten, it would lose what the person wrote.
pub fn follow_edit(sources: &[String], whole: bool) -> Result<Option<SettingsEdit>, String> {
    let address = bt_core::app_source_address(BROWSER_SOURCE);
    let followed = sources.iter().any(|have| {
        have == BROWSER_SOURCE
            || (address.is_some() && bt_core::app_source_address(have) == address)
    });
    if followed {
        return Ok(None);
    }
    if !whole {
        return Err(
            "An entry of [apps] sources could not be read, so bateri does not rewrite the list. \
             Fix it, or add bateri/browserpen to it, and choose browserpen again."
                .to_owned(),
        );
    }
    let mut list = sources.to_vec();
    list.push(BROWSER_SOURCE.to_owned());
    Ok(Some(SettingsEdit::AppsSources(list)))
}

/// What the first click's yes writes: browserpen followed (when it is not), and chosen for links.
pub fn yes_edits(sources: &[String], whole: bool) -> Result<Vec<SettingsEdit>, String> {
    let mut edits: Vec<SettingsEdit> = follow_edit(sources, whole)?.into_iter().collect();
    edits.push(SettingsEdit::AppsOpenLinksWith(BROWSER_PROGRAM.to_owned()));
    Ok(edits)
}

/// What the other answer to the first click's question writes: the person's own browser, for good.
pub fn no_edit() -> SettingsEdit {
    SettingsEdit::AppsOpenLinksWith(String::new())
}

/// What stops browserpen from coming, as the last check of the programs found it: a line for the
/// person, or `None` while it is coming or here. A check that is still running says nothing yet,
/// and neither does one that has not looked at the source (a list that was just written).
pub fn program_trouble(statuses: &[Status], running: bool) -> Option<String> {
    if running {
        return None;
    }
    let address = bt_core::app_source_address(BROWSER_SOURCE)?;
    let status = statuses.iter().find(|status| {
        status.source == BROWSER_SOURCE
            || bt_core::app_source_address(&status.source).as_ref() == Some(&address)
    })?;
    match &status.state {
        State::Ready { .. } => None,
        State::Awaiting { .. } => Some(
            "browserpen is signed with a key you have not approved yet. Approve it in Settings\u{a0}▸\u{a0}Apps."
                .to_owned(),
        ),
        State::WaitsForUpdate { .. } => {
            Some("browserpen needs a newer bateri than this one.".to_owned())
        }
        State::NotHere { .. } => Some("browserpen has no build for this Mac.".to_owned()),
        State::Error(failure) => Some(plain_reason(&failure.to_string())),
    }
}

/// What went wrong, as one plain sentence for the person: what a download or a fetch said, with the
/// names of the steps it went through taken off ("the release notice: …"), the server's answer and
/// a network that is not there put the way a person says them, and anything else as it was said,
/// as a sentence.
pub fn plain_reason(raw: &str) -> String {
    let lower = raw.to_lowercase();
    // The server's answer, as NSURLSession says it ("the server answered 503") and as curl does
    // ("The requested URL returned error: 503").
    for marker in ["answered ", "returned error: "] {
        if let Some(at) = lower.find(marker) {
            let status: String = lower[at + marker.len()..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            if !status.is_empty() {
                return format!("The server answered {status}.");
            }
        }
    }
    const NO_NETWORK: [&str; 10] = [
        "internet connection",
        "network connection",
        "could not connect",
        "couldn't connect",
        "failed to connect",
        "could not resolve",
        "hostname could not be found",
        "not connected",
        "offline",
        "network is unreachable",
    ];
    if NO_NETWORK.iter().any(|words| lower.contains(words)) {
        return "No connection to the internet.".to_owned();
    }
    if lower.contains("timed out") {
        return "The server did not answer in time.".to_owned();
    }
    // The words after the last step's name: "the download is not the release: …".
    let said = raw.trim();
    let said = said.strip_prefix("curl: ").unwrap_or(said);
    // curl's exit code in brackets goes ("(35) SSL connect error"), as the step names before a
    // colon do.
    let said = said.rsplit(": ").next().unwrap_or(said).trim();
    let said = match said
        .strip_prefix('(')
        .and_then(|rest| rest.split_once(") "))
    {
        Some((code, rest)) if code.chars().all(|c| c.is_ascii_digit()) => rest,
        _ => said,
    };
    let mut sentence = said.trim_end_matches('.').to_owned();
    if let Some(first) = sentence.chars().next() {
        let upper: String = first.to_uppercase().collect();
        sentence.replace_range(..first.len_utf8(), &upper);
    }
    if sentence.is_empty() {
        return "Something went wrong.".to_owned();
    }
    sentence.push('.');
    sentence
}

/// What the Apps page's Links row is built from.
#[derive(Clone, Debug)]
pub struct RowFacts<'a> {
    /// `[apps] open_links_with` as the file says it.
    pub setting: Option<&'a str>,
    /// The program is installed on this computer (its release landed and its link is there).
    pub program_here: bool,
    /// The browser engine.
    pub engine: Engine,
}

/// Which of the row's two choices is selected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chosen {
    Browserpen,
    YourBrowser,
    /// Another program, which the file names: neither choice is shown as made.
    Other,
}

/// What the Links row shows: the choice, and the line under it, which says what the next click
/// does (no warning colour: nothing here is wrong).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowState {
    pub chosen: Chosen,
    pub note: String,
    /// Choosing browserpen would write the setting at once (it is installed and its engine is
    /// here); otherwise it asks first, as the first click does.
    pub ready: bool,
}

/// The Links row in its states: not installed (the browser is the person's own and browserpen is
/// offered), installed (browserpen, and the engine under it), and answered no (the browser, and
/// browserpen is asked about again when it is chosen).
pub fn links_row(facts: &RowFacts<'_>) -> RowState {
    let ready = facts.program_here && matches!(facts.engine, Engine::Here { .. });
    // A program the file names that is neither of the two choices opens links all the same
    // (`route` sends them to it): the row says so and shows neither choice as made.
    if let Some(name) = facts
        .setting
        .filter(|name| !name.is_empty() && *name != BROWSER_PROGRAM)
    {
        return RowState {
            chosen: Chosen::Other,
            note: format!(
                "Links open in {name}, as settings.toml says. Choose one here to change it."
            ),
            ready,
        };
    }
    if facts.setting != Some(BROWSER_PROGRAM) {
        let note = if facts.setting == Some("") {
            "Links open in your browser. Choose browserpen to open them beside your terminal."
        } else {
            "browserpen can open links beside your terminal."
        };
        return RowState {
            chosen: Chosen::YourBrowser,
            note: note.to_owned(),
            ready,
        };
    }
    let note = match &facts.engine {
        Engine::Here { version, bytes } => {
            if facts.program_here {
                let mut parts = vec!["Browser engine".to_owned()];
                parts.extend(version.clone());
                parts.extend(bytes.map(megabytes));
                parts.join(" \u{00b7} ")
            } else {
                "Getting browserpen. Links open in your browser until it is here.".to_owned()
            }
        }
        Engine::Looking => "Looking for the browser engine\u{2026}".to_owned(),
        Engine::Downloading(Some(percent)) => {
            format!("Downloading the browser engine\u{2026} {percent}%")
        }
        Engine::Downloading(None) => "Downloading the browser engine\u{2026}".to_owned(),
        Engine::Failed(why) => format!(
            "The browser engine could not be downloaded. {} Links open in your browser.",
            plain_reason(why)
        ),
        Engine::Missing => {
            "Links open in your browser until the browser engine is downloaded.".to_owned()
        }
    };
    RowState {
        chosen: Chosen::Browserpen,
        note,
        ready,
    }
}

/// `n MB` for a size in bytes, rounded up to a whole megabyte.
pub fn megabytes(bytes: u64) -> String {
    format!("{} MB", bytes.div_ceil(1_000_000).max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opener(name: &str, opens: &[Opens]) -> Opener {
        Opener {
            name: name.to_owned(),
            opens: opens.to_vec(),
        }
    }

    fn pane(key: u32, program: &str, takes: &[TakeKind]) -> Pane<u32> {
        Pane {
            key,
            program: Some(program.to_owned()),
            takes: takes.to_vec(),
        }
    }

    fn facts<'a>(
        setting: Option<&'a str>,
        machine: Machine<'a>,
        engine: bool,
        panes: &'a [Pane<u32>],
    ) -> Facts<'a, u32> {
        Facts {
            setting,
            machine,
            engine,
            offered: true,
            fetching: false,
            asking: false,
            panes,
        }
    }

    #[test]
    fn a_program_that_is_not_installed_or_has_no_engine_is_the_system_browser() {
        let none: [Opener; 0] = [];
        let here = Machine::Here { openers: &none };
        // Not on this Mac: the address opens in the system's browser, and the click brings it.
        assert_eq!(
            route(&facts(Some("browserpen"), here, true, &[])),
            Route::SystemThenFetch
        );
        let mut coming = facts(Some("browserpen"), here, true, &[]);
        coming.fetching = true;
        assert_eq!(route(&coming), Route::System);
        let installed = [opener("browserpen", &[Opens::Url])];
        let here = Machine::Here {
            openers: &installed,
        };
        // Installed, no engine: nothing to draw the pages with, and the click brings it
        // (`a_yes_that_did_not_finish_...`).
        assert_eq!(
            route(&facts(Some("browserpen"), here, false, &[])),
            Route::SystemThenFetch
        );
        // A program that opens files only is not one for an address.
        let files = [opener("browserpen", &[Opens::File])];
        let here = Machine::Here { openers: &files };
        assert_eq!(
            route(&facts(Some("browserpen"), here, true, &[])),
            Route::System
        );
    }

    #[test]
    fn the_system_browser_and_a_name_that_is_none_are_the_system_browser() {
        let installed = [opener("browserpen", &[Opens::Url])];
        let here = Machine::Here {
            openers: &installed,
        };
        assert_eq!(route(&facts(Some(""), here, true, &[])), Route::System);
        assert_eq!(
            route(&facts(Some("Not A Name"), here, true, &[])),
            Route::System
        );
    }

    #[test]
    fn an_open_pane_that_takes_addresses_gets_it_and_else_a_program_starts() {
        let installed = [opener("browserpen", &[Opens::Url])];
        let here = Machine::Here {
            openers: &installed,
        };
        // No pane: a floating one starts.
        assert_eq!(
            route(&facts(Some("browserpen"), here, true, &[])),
            Route::Start
        );
        // The first capable pane, in the order given (the one last in first).
        let panes = [
            pane(7, "textpen", &[TakeKind::File]),
            pane(8, "browserpen", &[TakeKind::Url]),
            pane(9, "browserpen", &[TakeKind::Url]),
        ];
        assert_eq!(
            route(&facts(Some("browserpen"), here, true, &panes)),
            Route::Take(8)
        );
        // A browserpen that did not say it takes addresses (an older one), or another
        // program that does, is not one to hand it to.
        let panes = [
            pane(1, "browserpen", &[]),
            pane(2, "notes", &[TakeKind::Url]),
        ];
        assert_eq!(
            route(&facts(Some("browserpen"), here, true, &panes)),
            Route::Start
        );
    }

    #[test]
    fn a_server_without_the_program_gets_the_card_and_one_that_has_not_said_gets_the_system() {
        let panes = [pane(3, "browserpen", &[TakeKind::Url])];
        let said = ["textpen".to_owned()];
        let lacks = Machine::Server {
            installed: Some(&said),
            followed: true,
        };
        assert_eq!(
            route(&facts(Some("browserpen"), lacks, true, &panes)),
            Route::Card
        );
        // This computer does not follow it: it cannot install it there.
        let lacks = Machine::Server {
            installed: Some(&said),
            followed: false,
        };
        assert_eq!(
            route(&facts(Some("browserpen"), lacks, true, &[])),
            Route::System
        );
        // No engine here: the card would install what could not show a page, so the engine is
        // brought first and the address opens in the system's browser meanwhile.
        let lacks = Machine::Server {
            installed: Some(&said),
            followed: true,
        };
        assert_eq!(
            route(&facts(Some("browserpen"), lacks, false, &[])),
            Route::SystemThenFetch
        );
        let silent = Machine::Server {
            installed: None,
            followed: true,
        };
        assert_eq!(
            route(&facts(Some("browserpen"), silent, true, &[])),
            Route::System
        );
        let has = ["textpen".to_owned(), "browserpen".to_owned()];
        let has = Machine::Server {
            installed: Some(&has),
            followed: true,
        };
        assert_eq!(
            route(&facts(Some("browserpen"), has, true, &panes)),
            Route::Take(3)
        );
        assert_eq!(
            route(&facts(Some("browserpen"), has, true, &[])),
            Route::Start
        );
    }

    #[test]
    fn nothing_decided_asks_first_and_only_where_what_is_missing_can_be_had() {
        let none: [Opener; 0] = [];
        let here = Machine::Here { openers: &none };
        // The card asks; nothing opens before it is answered.
        assert_eq!(route(&facts(None, here, false, &[])), Route::Ask);
        // Releases that were not found, or not read yet: the system's browser, no question.
        let mut unread = facts(None, here, false, &[]);
        unread.offered = false;
        assert_eq!(route(&unread), Route::System);
        // The card is up already: the second click opens in the system's browser.
        let mut up = facts(None, here, false, &[]);
        up.asking = true;
        assert_eq!(route(&up), Route::System);
    }

    #[test]
    fn a_yes_that_did_not_finish_is_brought_again_by_the_next_click_and_not_twice() {
        let none: [Opener; 0] = [];
        let here = Machine::Here { openers: &none };
        // The engine is gone, or never came: the click brings it and the address goes to the system.
        assert_eq!(
            route(&facts(Some("browserpen"), here, false, &[])),
            Route::SystemThenFetch
        );
        // The program is gone (the engine is here).
        assert_eq!(
            route(&facts(Some("browserpen"), here, true, &[])),
            Route::SystemThenFetch
        );
        // A browserpen that is here but opens no addresses is not brought again.
        let old = [opener("browserpen", &[Opens::File])];
        assert_eq!(
            route(&facts(
                Some("browserpen"),
                Machine::Here { openers: &old },
                true,
                &[]
            )),
            Route::System
        );
        // On a server's pane the engine is still this computer's.
        let said = ["textpen".to_owned()];
        let server = Machine::Server {
            installed: Some(&said),
            followed: true,
        };
        assert_eq!(
            route(&facts(Some("browserpen"), server, false, &[])),
            Route::SystemThenFetch
        );
        // Under way: the click starts nothing.
        let mut coming = facts(Some("browserpen"), here, false, &[]);
        coming.fetching = true;
        assert_eq!(route(&coming), Route::System);
        // Another program of the file's naming is never fetched, and the system's browser is not
        // a thing to bring.
        let mut other = facts(Some("web"), here, true, &[]);
        assert_eq!(route(&other), Route::System);
        other.setting = Some("");
        assert_eq!(route(&other), Route::System);
    }

    #[test]
    fn the_first_click_asks_by_what_the_last_look_found_and_what_is_on_the_disk() {
        let mut lookup = Lookup::UNREAD;
        // Nothing read, nothing here: nothing to ask about, and both are to be read.
        assert!(!lookup.offered(false, false));
        assert_eq!(lookup.wants(false, false), Some((true, true)));
        // Both here: asks without any look at the network, and reads nothing.
        assert!(lookup.offered(true, true));
        assert_eq!(lookup.wants(true, true), None);
        // The engine's release read, the program's not: the program is read.
        lookup.read(Reading {
            program: None,
            engine: Some((Look::Found, Some(181_000_000))),
        });
        assert!(!lookup.offered(false, false));
        assert!(lookup.offered(true, false));
        assert_eq!(lookup.wants(false, false), Some((true, false)));
        // No release of the program: not read again in this run.
        lookup.read(Reading {
            program: Some(Look::Nothing),
            engine: None,
        });
        assert!(!lookup.offered(false, false));
        assert_eq!(lookup.wants(false, false), None);
        // A network that did not answer is read again; a program that is here is not read at all.
        lookup.read(Reading {
            program: Some(Look::Unreadable),
            engine: None,
        });
        assert_eq!(lookup.wants(false, false), Some((true, false)));
        assert_eq!(lookup.wants(true, false), None);
        // Found: both asked about, with the size the notice gave.
        lookup.read(Reading {
            program: Some(Look::Found),
            engine: None,
        });
        assert!(lookup.offered(false, false));
        assert_eq!(lookup.engine_bytes, Some(181_000_000));
        assert_eq!(lookup.wants(false, false), None);
        // The engine's release is withdrawn later: the size goes with it.
        lookup.read(Reading {
            program: None,
            engine: Some((Look::Nothing, None)),
        });
        assert_eq!(lookup.engine_bytes, None);
    }

    #[test]
    fn yes_follows_browserpen_once_and_chooses_it_and_no_chooses_the_persons_browser() {
        let textpen = vec!["bateri/textpen".to_owned()];
        let edits = yes_edits(&textpen, true).expect("a whole list is written");
        assert_eq!(
            edits,
            [
                SettingsEdit::AppsSources(vec![
                    "bateri/textpen".to_owned(),
                    "bateri/browserpen".to_owned()
                ]),
                SettingsEdit::AppsOpenLinksWith("browserpen".to_owned()),
            ]
        );
        // Followed already, by the short name or by the address it stands for: only the choice.
        let both = vec!["bateri/browserpen".to_owned()];
        assert_eq!(
            yes_edits(&both, true),
            Ok(vec![SettingsEdit::AppsOpenLinksWith(
                "browserpen".to_owned()
            )])
        );
        let address = bt_core::app_source_address("bateri/browserpen").expect("an address");
        assert_eq!(
            yes_edits(&[address], true),
            Ok(vec![SettingsEdit::AppsOpenLinksWith(
                "browserpen".to_owned()
            )])
        );
        // A list that could not be read whole is not rewritten: the yes says why and writes nothing.
        let why = yes_edits(&textpen, false).expect_err("a list that is not whole");
        assert!(why.contains("[apps] sources"), "{why}");
        assert_eq!(yes_edits(&both, false).map(|edits| edits.len()), Ok(1));
        assert_eq!(no_edit(), SettingsEdit::AppsOpenLinksWith(String::new()));
    }

    #[test]
    fn a_reason_is_one_plain_sentence() {
        assert_eq!(
            plain_reason("the release notice: the server answered 503"),
            "The server answered 503."
        );
        assert_eq!(
            plain_reason("the download failed: The server answered 404"),
            "The server answered 404."
        );
        for offline in [
            "the release notice: The Internet connection appears to be offline.",
            "The network connection was lost.",
            "Could not resolve host: github.com",
            "A server with the specified hostname could not be found.",
        ] {
            assert_eq!(
                plain_reason(offline),
                "No connection to the internet.",
                "{offline}"
            );
        }
        assert_eq!(
            plain_reason("The request timed out."),
            "The server did not answer in time."
        );
        // What curl says (the first line it printed, which is what a failed fetch of a program
        // keeps).
        assert_eq!(
            plain_reason("curl: (22) The requested URL returned error: 503"),
            "The server answered 503."
        );
        assert_eq!(
            plain_reason("curl: (6) Could not resolve host: github.com"),
            "No connection to the internet."
        );
        assert_eq!(
            plain_reason(
                "curl: (7) Failed to connect to github.com port 443 after 12 ms: Couldn't connect to server"
            ),
            "No connection to the internet."
        );
        assert_eq!(
            plain_reason("curl: (28) Operation timed out after 30001 milliseconds"),
            "The server did not answer in time."
        );
        assert_eq!(
            plain_reason("curl: (35) SSL connect error"),
            "SSL connect error."
        );
        // Anything else is said as a sentence; nothing the person reads says it "did not come".
        assert_eq!(
            plain_reason("the download is not the release"),
            "The download is not the release."
        );
        assert_eq!(plain_reason(""), "Something went wrong.");
        assert!(!plain_reason("No space left on this machine.").contains("come"));
    }

    #[test]
    fn what_stops_browserpen_coming_is_told_only_when_the_check_has_ended() {
        use crate::apps::Failure;
        let status = |source: &str, state| Status {
            source: source.to_owned(),
            name: None,
            state,
            problem: None,
        };
        let failed = [status(
            "bateri/browserpen",
            State::Error(Failure::Fetch("offline".to_owned())),
        )];
        assert_eq!(program_trouble(&failed, true), None);
        assert_eq!(
            program_trouble(&failed, false).as_deref(),
            Some("No connection to the internet.")
        );
        // Another program's trouble is not browserpen's, and a check that did not look says nothing.
        let other = [status("bateri/textpen", State::Error(Failure::Digest))];
        assert_eq!(program_trouble(&other, false), None);
        assert_eq!(program_trouble(&[], false), None);
        // The source by the address it stands for.
        let address = bt_core::app_source_address("bateri/browserpen").expect("an address");
        let waiting = [status(
            &address,
            State::Awaiting {
                version: bt_core::pencere::apps::Version::parse("0.1.0").expect("a version"),
                key: "ed25519:AAAA".to_owned(),
                changed: false,
            },
        )];
        assert!(
            program_trouble(&waiting, false)
                .expect("a key to approve")
                .contains("key")
        );
    }

    /// The first click was in a server's pane: when what it asked for is on this Mac, the address is
    /// routed as any click now is, so a server without browserpen is offered it at once and one that
    /// has it opens the address there.
    #[test]
    fn an_address_of_a_server_pane_goes_the_way_any_click_goes_once_the_first_card_is_done() {
        let said = ["textpen".to_owned()];
        let lacks = Machine::Server {
            installed: Some(&said),
            followed: true,
        };
        let card = route(&facts(Some("browserpen"), lacks, true, &[]));
        assert_eq!(card, Route::Card);
        assert!(frees_the_corner(&card), "its own card wants the corner");
        let has = ["textpen".to_owned(), "browserpen".to_owned()];
        let has = Machine::Server {
            installed: Some(&has),
            followed: true,
        };
        let panes = [pane(4, "browserpen", &[TakeKind::Url])];
        let take = route(&facts(Some("browserpen"), has, true, &panes));
        assert_eq!(take, Route::Take(4));
        assert!(!frees_the_corner(&take));
        let start = route(&facts(Some("browserpen"), has, true, &[]));
        assert_eq!(start, Route::Start);
        assert!(!frees_the_corner(&start));
        // The server has not said what it has: the system's browser, the first card stays.
        let silent = Machine::Server {
            installed: None,
            followed: true,
        };
        let system = route(&facts(Some("browserpen"), silent, true, &[]));
        assert_eq!(system, Route::System);
        assert!(!frees_the_corner(&system));
    }

    #[test]
    fn a_file_goes_to_the_open_pane_of_the_program_that_takes_files() {
        let panes = [
            pane(1, "browserpen", &[TakeKind::Url]),
            pane(2, "textpen", &[TakeKind::File]),
            pane(3, "textpen", &[TakeKind::File]),
        ];
        assert_eq!(take_pane(&panes, "textpen", TakeKind::File), Some(2));
        assert_eq!(take_pane(&panes, "textpen", TakeKind::Folder), None);
        assert_eq!(take_pane(&panes, "notes", TakeKind::File), None);
    }

    #[test]
    fn the_question_says_what_would_be_downloaded() {
        assert_eq!(
            question(false, false, Some(180_000_001)).line(),
            "Open links beside your terminal with browserpen? It downloads browserpen and the browser engine (181 MB)."
        );
        assert_eq!(
            question(false, false, None).line(),
            "Open links beside your terminal with browserpen? It downloads browserpen and the browser engine."
        );
        assert_eq!(
            question(false, true, Some(1)).line(),
            "Open links beside your terminal with browserpen? It downloads browserpen."
        );
        // The program is here and the engine is not: only the engine is downloaded.
        assert_eq!(
            question(true, false, Some(181_000_000)).line(),
            "Open links beside your terminal with browserpen? It downloads the browser engine (181 MB)."
        );
        let all_here = question(true, true, None);
        assert_eq!(
            all_here.line(),
            "Open links beside your terminal with browserpen?"
        );
        assert_eq!(all_here.detail, None);
    }

    #[test]
    fn the_links_row_says_what_the_next_click_does_in_each_of_its_states() {
        let here = Engine::Here {
            version: Some("0.1.0".to_owned()),
            bytes: Some(181_000_000),
        };
        let row = |setting, program_here, engine| {
            links_row(&RowFacts {
                setting,
                program_here,
                engine,
            })
        };
        // Not installed, nothing decided: the person's own browser, and the offer.
        let offered = row(None, false, Engine::Missing);
        assert_eq!(offered.chosen, Chosen::YourBrowser);
        assert_eq!(
            offered.note,
            "browserpen can open links beside your terminal."
        );
        assert!(!offered.ready);
        // Installed and chosen: browserpen, and the engine under it.
        let installed = row(Some("browserpen"), true, here.clone());
        assert_eq!(installed.chosen, Chosen::Browserpen);
        assert_eq!(
            installed.note,
            "Browser engine \u{00b7} 0.1.0 \u{00b7} 181 MB"
        );
        assert!(installed.ready);
        // Answered no: the browser, and choosing browserpen asks again unless it is all here.
        let no = row(Some(""), false, Engine::Missing);
        assert_eq!(no.chosen, Chosen::YourBrowser);
        assert!(no.note.starts_with("Links open in your browser."));
        assert!(!no.ready);
        assert!(row(Some(""), true, here.clone()).ready);
        // The engine on its way is said as it is, and so is a failure; a development copy's
        // engine has no version to say.
        assert_eq!(
            row(Some("browserpen"), true, Engine::Downloading(Some(42))).note,
            "Downloading the browser engine\u{2026} 42%"
        );
        assert_eq!(
            row(
                Some("browserpen"),
                true,
                Engine::Failed("the download failed: the server answered 503".to_owned())
            )
            .note,
            "The browser engine could not be downloaded. The server answered 503. Links open in your browser."
        );
        // A program the file names is the one that opens links, and neither choice is shown made.
        let other = row(Some("web"), false, Engine::Missing);
        assert_eq!(other.chosen, Chosen::Other);
        assert!(other.note.contains("web"), "{}", other.note);
        let app = Engine::Here {
            version: None,
            bytes: None,
        };
        assert_eq!(row(Some("browserpen"), true, app).note, "Browser engine");
    }
}
