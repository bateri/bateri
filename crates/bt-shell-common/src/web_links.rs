//! Where a web address ⌘-clicked in a terminal opens (`[apps] open_links_with`): in the program the
//! person chose, beside the terminal and on the computer the terminal is on, or in the system's
//! browser — and, for the first click that decides nothing, in the system's browser with a question
//! afterwards.
//!
//! The decision is pure ([`route`]) and reads only what this computer already knows: what it
//! follows and has installed, what a server last said it has, whether the browser engine is on
//! disk, and which panes of the tab run which program. **A click never waits for the network**: a
//! program that is not there is the system's browser, said or not, and the first-click question
//! comes after the address has opened. The platform shell reads that state, holds the panes and
//! does what the route says.
//!
//! A file Command-clicked in a pane of a tab with a program that takes files open takes it as a new
//! tab of that program ([`take_pane`]); the same rule picks the pane for an address.

use bt_core::pencere::TakeKind;
use bt_core::pencere::apps::{self, Opens};

use crate::apps::opens::Opener;

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
    /// This computer can fetch the program and the engine when it is told to (a copy that has a
    /// release to ask): what makes the first click worth a question.
    pub offered: bool,
    /// The panes of the tab on that machine that run a view, the one the person was last in first.
    pub panes: &'a [Pane<K>],
}

/// What the click does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route<K> {
    /// The system's browser.
    System,
    /// The system's browser, and then the one-time question.
    SystemThenAsk,
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
        return if facts.offered {
            Route::SystemThenAsk
        } else {
            Route::System
        };
    };
    if setting.is_empty() || !apps::is_name(setting) {
        return Route::System;
    }
    // The engine is this computer's even when the program runs on a server: a pane here draws it.
    if setting == BROWSER_PROGRAM && !facts.engine {
        return Route::System;
    }
    match facts.machine {
        Machine::Here { openers } => {
            if !openers
                .iter()
                .any(|opener| opener.name == setting && opener.opens.contains(&Opens::Url))
            {
                return Route::System;
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
            };
        }
        (false, true) => "browserpen",
        (_, false) => "browserpen and the browser engine",
    };
    let detail = match engine_bytes.filter(|_| !engine_here) {
        Some(bytes) => format!("It downloads {what} ({}).", megabytes(bytes)),
        None => format!("It downloads {what}."),
    };
    Question {
        title: TITLE.to_owned(),
        detail: Some(detail),
    }
}

/// What the one-time note says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Question {
    /// The question.
    pub title: String,
    /// What saying yes downloads; `None` when everything is here already.
    pub detail: Option<String>,
}

impl Question {
    /// The whole note in one line, as the plan's and the documents' words have it.
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
    /// It did not come, in a line for the person.
    Failed(String),
    /// On the disk: the version of its release (a development copy that runs an app where it is
    /// has none to say) and its size in bytes, when it was measured.
    Here {
        version: Option<String>,
        bytes: Option<u64>,
    },
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
        Engine::Failed(why) => {
            format!("The browser engine did not come ({why}). Links open in your browser.")
        }
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
            panes,
        }
    }

    #[test]
    fn a_program_that_is_not_installed_or_has_no_engine_is_the_system_browser() {
        let none: [Opener; 0] = [];
        let here = Machine::Here { openers: &none };
        assert_eq!(
            route(&facts(Some("browserpen"), here, true, &[])),
            Route::System
        );
        let installed = [opener("browserpen", &[Opens::Url])];
        let here = Machine::Here {
            openers: &installed,
        };
        // Installed, no engine: nothing to draw the pages with.
        assert_eq!(
            route(&facts(Some("browserpen"), here, false, &[])),
            Route::System
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
        // No engine here: the card would install what could not show a page.
        let lacks = Machine::Server {
            installed: Some(&said),
            followed: true,
        };
        assert_eq!(
            route(&facts(Some("browserpen"), lacks, false, &[])),
            Route::System
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
    fn nothing_decided_opens_in_the_system_and_asks_only_where_the_program_can_be_had() {
        let none: [Opener; 0] = [];
        let here = Machine::Here { openers: &none };
        assert_eq!(route(&facts(None, here, false, &[])), Route::SystemThenAsk);
        let mut asked = facts(None, here, false, &[]);
        asked.offered = false;
        assert_eq!(route(&asked), Route::System);
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
        assert!(
            row(
                Some("browserpen"),
                true,
                Engine::Failed("offline".to_owned())
            )
            .note
            .contains("offline")
        );
        let app = Engine::Here {
            version: None,
            bytes: None,
        };
        assert_eq!(row(Some("browserpen"), true, app).note, "Browser engine");
    }
}
