//! Where a web address ⌘-clicked in a terminal opens, and a file that a running program takes
//! (`[apps] open_links_with`, `[apps] open_with`): beside the terminal in the program the person
//! chose, or in the system's browser.
//!
//! The decision is [`bt_shell_common::web_links::route`], from what this computer already knows;
//! this module reads that state (the panes of the tab and what their programs said they take, what
//! is installed here and what a server said it has, whether the engine is on the disk, what the
//! releases looked like when they were read) and does what the route says. **A click never waits
//! for the network.** Where nothing was decided about links, the first click asks, with a card over
//! the pane, before the address opens ([`crate::apps_card`]), if the releases of browserpen and
//! the engine were read at launch and are there to be had; the address opens by the answer. The
//! releases are read in the turn of the check of the programs ([`AppDelegate::read_links_offer`]),
//! and again at its retry rhythm when the network did not answer.

use std::sync::atomic::AtomicBool;
use std::time::Duration;

use bt_core::pencere::{Take, TakeKind};
use bt_shell_common::apps::card::{Coming, Stage};
use bt_shell_common::web_links::{self, Facts, Look, Machine, Pane, Reading, Route};
use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn, NSModalResponse};
use objc2_foundation::{NSString, ns_string};

use super::*;
use crate::pane::PaneMachine;
use crate::sheets::{self, Asker};
use crate::{hyperlink, launch};

/// How long the first read of the releases waits after one the network did not answer; it
/// doubles after each, up to [`READ_RETRY_MAX`] (the check of the programs does the same).
pub(crate) const READ_RETRY: Duration = Duration::from_secs(5 * 60);
const READ_RETRY_MAX: Duration = Duration::from_secs(60 * 60);

/// What the download of the browser engine, which saying yes to the question starts, is doing: what
/// the Apps page shows beside its row, and the first click's card under its title.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum EngineJob {
    /// Nothing is being downloaded.
    Idle,
    /// The release was asked for.
    Looking,
    /// Bytes are coming; the percent when the size is known.
    Downloading(Option<u8>),
    /// It could not be downloaded: the words for the person, until the next try.
    Failed(String),
}

/// Whether the program `name` has its link where the installer of this Mac puts it, which is what
/// starting it runs: a program that landed in the folder of the followed ones has none until
/// kablo has put it in place.
fn program_linked(name: &str) -> bool {
    child::home().is_some_and(|home| {
        std::fs::symlink_metadata(bt_shell_common::open_with::bin_here(&home).join(name)).is_ok()
    })
}

/// [`AppDelegate::links_install_ended`] from the thread of the check of the programs: kablo has
/// put what landed in place, or `trouble` says why it could not.
pub(crate) fn links_install_ended(trouble: Option<String>) {
    DispatchQueue::main().exec_async(move || {
        // audit: a block running on the main queue is on the main thread by definition.
        let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
        if let Some(app) = delegate(mtm) {
            app.ivars().links_install_trouble.replace(trouble);
            app.links_progress();
        }
    });
}

impl AppDelegate {
    /// A web address ⌘-clicked in `from`, a pane of `tab`.
    pub(crate) fn open_web_address(&self, tab: &TerminalTab, from: &TerminalPane, url: &str) {
        match self.web_route(tab, from) {
            Route::System => hyperlink::open_url(url),
            Route::SystemThenFetch => {
                hyperlink::open_url(url);
                self.fetch_for_links();
            }
            Route::Ask => {
                if !self.ask_here(from, url) {
                    hyperlink::open_url(url);
                }
            }
            Route::Take(id) => {
                let take = Take::Url(url.to_owned());
                if !self.hand_to(tab, id, &take) {
                    // The view ended between the look and the hand-over: a program of its own.
                    self.start_address(tab, from, url);
                }
            }
            Route::Start => self.start_address(tab, from, url),
            Route::Card => self.offer_link_card(from, url),
        }
    }

    /// The route of a web address ⌘-clicked in `from`.
    fn web_route(&self, tab: &TerminalTab, from: &TerminalPane) -> Route<u64> {
        let machine = from.machine();
        let setting = self.settings().apps_open_links_with.clone();
        let panes = self.hosts(tab, &machine);
        let openers = self.openers();
        let linked;
        let installed;
        let facts_machine = match &machine {
            PaneMachine::Elsewhere => return Route::System,
            PaneMachine::Here => {
                // A program is on this Mac when kablo has put its link in place, which is what
                // starts it; one that only landed is not (yet), unless a pane of the tab runs it
                // already: that one takes the address.
                linked = openers
                    .iter()
                    .filter(|opener| {
                        program_linked(&opener.name)
                            || web_links::take_pane(&panes, &opener.name, TakeKind::Url).is_some()
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                Machine::Here { openers: &linked }
            }
            PaneMachine::Server(account) => {
                let mut said = crate::apps_answerer()
                    .and_then(|answerer| answerer.machine(account))
                    .map(|state| state.installed);
                // What a card of a click just brought is there before the server says so.
                let brought = crate::apps_card::brought_to(account);
                if !brought.is_empty() {
                    said.get_or_insert_with(Vec::new).extend(brought);
                }
                installed = said;
                Machine::Server {
                    installed: installed.as_deref(),
                    followed: setting.as_deref().is_some_and(|name| {
                        crate::apps_root().is_some_and(|root| {
                            bt_shell_common::apps::store::followed(&root, name).is_some()
                        })
                    }),
                }
            }
        };
        let engine = crate::browser_install::engine_ready();
        web_links::route(&Facts {
            setting: setting.as_deref(),
            machine: facts_machine,
            engine,
            offered: self
                .ivars()
                .links_lookup
                .get()
                .offered(self.program_landed(), engine),
            fetching: self.links_fetching(),
            asking: crate::apps_card::here_open(),
            panes: &panes,
        })
    }

    /// The panes of `tab` on `machine` whose program runs a view and said what it takes, the one
    /// the focus was last in first.
    fn hosts(&self, tab: &TerminalTab, machine: &PaneMachine) -> Vec<Pane<u64>> {
        tab.recent_panes()
            .iter()
            .filter(|pane| pane.machine() == *machine)
            .filter_map(|pane| {
                let hosted = pane.session()?.hosted()?;
                Some(Pane {
                    key: pane.id(),
                    program: hosted.program,
                    takes: hosted.takes,
                })
            })
            .collect()
    }

    /// Hands `take` to the program of the pane `id` of `tab` and brings the pane forward. `false`
    /// when nothing was sent (the pane is gone, or its program no longer takes it).
    fn hand_to(&self, tab: &TerminalTab, id: u64, take: &Take) -> bool {
        let Some(pane) = tab.panes().into_iter().find(|pane| pane.id() == id) else {
            return false;
        };
        let sent = pane
            .session()
            .is_some_and(|session| session.hand_over(take));
        if sent {
            tab.focus_pane(&pane);
        }
        sent
    }

    /// A file ⌘-clicked in `from` that the program `name` opens: a new tab of that program when
    /// one is open in a pane of the tab on the same machine and said it takes files. `false` when
    /// there is none, and the program is started.
    pub(super) fn hand_file(
        &self,
        tab: &TerminalTab,
        from: &TerminalPane,
        name: &str,
        path: &str,
        line: Option<u32>,
        server: bool,
    ) -> bool {
        let machine = from.machine();
        // The path is on the machine it was found on (`server`): a program on another one, in the
        // same tab, is not handed it.
        if machine == PaneMachine::Elsewhere || matches!(machine, PaneMachine::Server(_)) != server
        {
            return false;
        }
        let panes = self.hosts(tab, &machine);
        let Some(id) = web_links::take_pane(&panes, name, TakeKind::File) else {
            return false;
        };
        let take = Take::File {
            path: path.to_owned(),
            line: line.filter(|line| *line > 0),
            column: None,
        };
        self.hand_to(tab, id, &take)
    }

    /// A floating pane that starts the program the person chose with the address.
    fn start_address(&self, tab: &TerminalTab, from: &TerminalPane, url: &str) {
        let Some(name) = self.settings().apps_open_links_with.clone() else {
            return hyperlink::open_url(url);
        };
        let server = matches!(from.machine(), PaneMachine::Server(_));
        let started = self.open_program(
            tab,
            from,
            launch::OpenProgram {
                name,
                target: launch::OpenTarget::Address(url.to_owned()),
                server,
            },
        );
        // An address is not lost: where the program could not be started (no way to reach the
        // server, a setting that names no program) the system's browser opens it, as it did before.
        if !started {
            hyperlink::open_url(url);
        }
    }

    // ─── the first click's question ───────────────────────────────────────────

    /// Whether browserpen has landed on this Mac (its folder is among the followed programs'): not
    /// that it can be started, which wants its link ([`Self::program_ready`]).
    fn program_landed(&self) -> bool {
        self.openers()
            .iter()
            .any(|opener| opener.name == web_links::BROWSER_PROGRAM && opener.opens_addresses())
    }

    /// Whether browserpen can be started on this Mac now.
    fn program_ready(&self) -> bool {
        self.program_landed() && program_linked(web_links::BROWSER_PROGRAM)
    }

    /// What was said yes to is on its way: the engine is downloading, or the programs are being
    /// checked, or a click brought it back a moment ago (what a click does not start a second
    /// time: where it cannot be had, every click would otherwise check the programs and read the
    /// engine's notice again).
    fn links_fetching(&self) -> bool {
        matches!(
            *self.ivars().engine_job.borrow(),
            EngineJob::Looking | EngineJob::Downloading(_)
        ) || self
            .ivars()
            .apps
            .get()
            .is_some_and(bt_shell_common::apps::Tracker::running)
            || self
                .ivars()
                .links_fetched
                .get()
                .is_some_and(|at| at.elapsed() < READ_RETRY)
    }

    /// The first click on a web address, before anything is decided: the card over `from` that
    /// asks whether to open links beside the terminal with browserpen, with what it would download.
    /// The address opens by the answer. `false` when the card cannot be put there.
    fn ask_here(&self, from: &TerminalPane, url: &str) -> bool {
        crate::apps_card::offer_here(
            self.mtm(),
            from,
            url,
            self.program_ready(),
            crate::browser_install::engine_ready(),
            self.ivars().links_lookup.get().engine_bytes,
        )
    }

    /// Reads the releases of what is not on this Mac yet (browserpen's `app.json`, the engine's
    /// notice), on a thread of its own, when nothing is decided about links: so that the first
    /// click knows without waiting whether there is anything to ask about. Called where the check
    /// of the programs ended, which is at launch, when the list changes, and once a day; a read the
    /// network did not answer comes again after the wait the checks use for that.
    pub(super) fn read_links_offer(&self) {
        if self.settings().apps_open_links_with.is_some() || self.ivars().links_reading.get() {
            return;
        }
        let Some((program, engine)) = self.ivars().links_lookup.get().wants(
            self.program_landed(),
            crate::browser_install::engine_ready(),
        ) else {
            return;
        };
        self.ivars().links_reading.set(true);
        let spawned = std::thread::Builder::new()
            .name("links-releases".into())
            .spawn(move || {
                let reading = Reading {
                    program: program
                        .then(|| crate::browser_install::read_program(web_links::BROWSER_SOURCE)),
                    engine: engine.then(crate::browser_install::read_engine),
                };
                DispatchQueue::main().exec_async(move || {
                    // audit: a block running on the main queue is on the main thread by definition.
                    let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
                    if let Some(app) = delegate(mtm) {
                        app.links_read(reading);
                    }
                });
            });
        if spawned.is_err() {
            self.ivars().links_reading.set(false);
        }
    }

    /// The releases were read.
    fn links_read(&self, reading: Reading) {
        let mut lookup = self.ivars().links_lookup.get();
        lookup.read(reading);
        self.ivars().links_lookup.set(lookup);
        self.ivars().links_reading.set(false);
        let unanswered = [reading.program, reading.engine.map(|(look, _)| look)]
            .contains(&Some(Look::Unreadable));
        if !unanswered {
            self.ivars().links_retry.set(READ_RETRY);
            return;
        }
        // One chain of retries: a read that came meanwhile (the list changed, the person asked) and
        // found no answer either waits for the turn that is already set.
        if self.ivars().links_retry_pending.replace(true) {
            return;
        }
        let wait = self.ivars().links_retry.get();
        self.ivars().links_retry.set((wait * 2).min(READ_RETRY_MAX));
        let set = dispatch2::DispatchTime::try_from(wait).is_ok_and(|when| {
            DispatchQueue::main()
                .after(when, || {
                    // audit: a block running on the main queue is on the main thread by definition.
                    let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
                    if let Some(app) = delegate(mtm) {
                        app.ivars().links_retry_pending.set(false);
                        app.read_links_offer();
                    }
                })
                .is_ok()
        });
        if !set {
            self.ivars().links_retry_pending.set(false);
        }
    }

    /// The first click's card was answered yes: browserpen is followed and chosen, and what is not
    /// here comes. What the card says follows what comes of it ([`Self::links_progress`]); where it
    /// could not even be written, the card says so.
    pub(crate) fn said_yes_to_links(&self) {
        self.ivars().links_install_trouble.replace(None);
        // What failed before is not what this yes will find: the card does not read it, and the
        // programs are looked at again at once where their check had ended without the program in
        // place (landed, or landed and not yet linked by kablo: its install comes with the check; a
        // check that runs says nothing yet, so a failure it left is not read either).
        if matches!(*self.ivars().engine_job.borrow(), EngineJob::Failed(_)) {
            self.ivars().engine_job.replace(EngineJob::Idle);
        }
        match self.use_browserpen_for_links() {
            Ok(()) => {
                // After the list is written, so that the check is of the list that follows
                // browserpen: writing it already asks for one, which a check that runs makes
                // this one unnecessary.
                if !self.program_ready()
                    && let Some(tracker) = self.ivars().apps.get()
                    && !tracker.running()
                {
                    tracker.refresh();
                }
                crate::apps_card::here_watch();
                self.links_progress();
            }
            Err(why) => crate::apps_card::here_failed(self.mtm(), &why),
        }
    }

    /// Where the first click's card waits for what its yes brought: each thing is here, on its way
    /// (the engine says how far) or could not be had (why, in a plain sentence), and the card's rows
    /// follow it. The card is ready when both are here, which opens the address.
    pub(crate) fn links_progress(&self) {
        if !matches!(
            crate::apps_card::here_stage(),
            Some(Stage::Installing | Stage::Failed)
        ) {
            return;
        }
        let program = if self.program_ready() {
            Coming::Done
        } else {
            let trouble = self
                .apps_tracked()
                .and_then(|tracked| web_links::program_trouble(&tracked.statuses, tracked.running))
                .or_else(|| {
                    self.program_landed()
                        .then(|| self.ivars().links_install_trouble.borrow().clone())
                        .flatten()
                });
            let trouble = trouble.or_else(|| {
                // Nothing follows programs on this Mac (the tracker never started): nothing
                // brings the program, and nothing will say so.
                self.ivars()
                    .apps
                    .get()
                    .is_none()
                    .then(|| "bateri can\u{2019}t install programs right now.".to_owned())
            });
            trouble.map_or(Coming::Busy(None), Coming::Failed)
        };
        let engine = if crate::browser_install::engine_ready() {
            Coming::Done
        } else {
            match self.engine_job() {
                EngineJob::Failed(why) => Coming::Failed(web_links::plain_reason(&why)),
                EngineJob::Downloading(percent) => Coming::Busy(percent),
                EngineJob::Idle | EngineJob::Looking => Coming::Busy(None),
            }
        };
        crate::apps_card::here_sync(self.mtm(), &program, &engine);
    }

    /// A click found browserpen or the engine gone though the person said yes: it comes again,
    /// quietly (the address has opened in the system's browser).
    fn fetch_for_links(&self) {
        self.ivars()
            .links_fetched
            .set(Some(std::time::Instant::now()));
        let file = self.file_settings();
        if let Ok(Some(edit)) = web_links::follow_edit(&file.apps_sources, file.apps_sources_whole)
        {
            let _ = self.save_edits(&[edit]);
        }
        self.download_engine();
        if !self.program_ready()
            && let Some(tracker) = self.ivars().apps.get()
            && !tracker.running()
        {
            tracker.refresh();
        }
    }

    // ─── the question in Settings ─────────────────────────────────────────────

    /// Settings ▸ Apps ▸ Links ▸ browserpen, where it is not all here: the question as a sheet of
    /// the settings window, with *Use browserpen* and *Cancel* (what the person did is the choice;
    /// nothing is written for a cancel). `false` when this copy has no release to look at (a copy
    /// that runs an engine where it is): there is nothing to offer.
    pub(crate) fn ask_to_use_browserpen(&self) -> bool {
        let Some(installer) = crate::browser_install::installer() else {
            return false;
        };
        let (program_here, _) = self.links_facts();
        let _ = std::thread::Builder::new()
            .name("links-notice".into())
            .spawn(move || {
                let check = installer.check();
                // A question about bringing browserpen is asked only where it can be brought.
                let released = (!program_here).then(|| {
                    crate::browser_install::program_released(&format!(
                        "bateri/{}",
                        web_links::BROWSER_PROGRAM
                    ))
                });
                DispatchQueue::main().exec_async(move || {
                    // audit: a block running on the main queue is on the main thread by definition.
                    let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
                    if let Some(app) = delegate(mtm) {
                        app.links_noticed(check, released);
                    }
                });
            });
        true
    }

    /// The seat of a sheet of the settings window, when no other sheet is up there.
    fn settings_seat(&self) -> Option<sheets::Seat> {
        self.settings_window()
            .and_then(|window| window.ns_window())
            .and_then(|window| sheets::seat(Asker::Window(&window)))
            .filter(|seat| !seat.is_taken())
    }

    /// Says something to the person in the settings window, with one button; to the log where no
    /// sheet can be put there.
    fn inform(&self, title: &str, detail: &str) {
        let Some(seat) = self.settings_seat() else {
            eprintln!("bateri: {title}: {detail}");
            return;
        };
        let alert = NSAlert::new(self.mtm());
        alert.setMessageText(&NSString::from_str(title));
        alert.setInformativeText(&NSString::from_str(detail));
        alert.addButtonWithTitle(ns_string!("OK"));
        seat.begin(&alert, &RcBlock::new(|_: NSModalResponse| {}));
    }

    /// The release notice was read: the question, with what it would download.
    fn links_noticed(
        &self,
        check: crate::browser_install::Check,
        released: Option<Result<(), String>>,
    ) {
        use crate::browser_install::Check;
        if let Some(Err(why)) = &released {
            self.inform(
                "browserpen can’t be reached",
                &format!("Its release could not be read ({why}). Try again when you are online."),
            );
            return;
        }
        let engine = crate::browser_install::engine_installed();
        let bytes = match &check {
            Check::Offer(offer) => Some(offer.asset.size),
            // The size is not known: the question says what it brings.
            Check::Current | Check::Refused(_) | Check::Unavailable(_) | Check::Absent(_) => None,
        };
        let (program_here, _) = self.links_facts();
        let question = web_links::question(program_here, engine.is_some(), bytes);
        let Some(seat) = self.settings_seat() else {
            return;
        };
        let alert = note_alert(self.mtm(), &question);
        let answered = RcBlock::new(move |response: NSModalResponse| {
            // audit: the sheet's completion block runs on AppKit's main thread.
            let mtm = MainThreadMarker::new().expect("the sheet block is on the main thread");
            let Some(app) = delegate(mtm) else {
                return;
            };
            if response == NSAlertFirstButtonReturn {
                app.use_browserpen_from_settings();
            }
            app.refresh_settings_window();
        });
        seat.begin(&alert, &answered);
    }

    /// What the Links row is built from: whether browserpen is installed here, and the engine as
    /// it is now (on its way, failed, on the disk with its version and size, or not here).
    pub(crate) fn links_facts(&self) -> (bool, web_links::Engine) {
        let program_here = self.program_landed();
        let engine = match self.engine_job() {
            EngineJob::Looking => web_links::Engine::Looking,
            EngineJob::Downloading(percent) => web_links::Engine::Downloading(percent),
            EngineJob::Failed(why) => web_links::Engine::Failed(why),
            EngineJob::Idle => match crate::browser_install::engine_installed() {
                Some(here) => web_links::Engine::Here {
                    version: here.version.map(|version| version.to_string()),
                    bytes: folder_bytes(&here.folder),
                },
                None => web_links::Engine::Missing,
            },
        };
        (program_here, engine)
    }

    /// Yes: browserpen is followed, it opens the links from now on, and the engine comes if it is
    /// not here. What a click does in the meantime is the system's browser (the program and the
    /// engine are not both on the disk yet). `Err` with a line for the person when it could not be
    /// written (a list of sources that is not whole is not rewritten; a settings file that cannot
    /// be written).
    pub(crate) fn use_browserpen_for_links(&self) -> Result<(), String> {
        let edits = {
            let settings = self.file_settings();
            web_links::yes_edits(&settings.apps_sources, settings.apps_sources_whole)?
        };
        self.save_edits(&edits)?;
        self.download_engine();
        Ok(())
    }

    /// [`Self::use_browserpen_for_links`] as the settings window asks it: what could not be done
    /// is said in a sheet there.
    pub(crate) fn use_browserpen_from_settings(&self) {
        if let Err(why) = self.use_browserpen_for_links() {
            self.inform("browserpen was not turned on", &why);
        }
    }

    /// Brings the browser engine when it is not on this Mac: the release notice, then the archive,
    /// on a thread of its own; what the Apps page shows follows it ([`EngineJob`]).
    pub(crate) fn download_engine(&self) {
        let busy = matches!(
            *self.ivars().engine_job.borrow(),
            EngineJob::Looking | EngineJob::Downloading(_)
        );
        if crate::browser_install::engine_ready() || busy {
            return;
        }
        let Some(installer) = crate::browser_install::installer() else {
            return;
        };
        self.set_engine_job(EngineJob::Looking);
        let _ = std::thread::Builder::new()
            .name("browser-download".into())
            .spawn(move || {
                let outcome = match installer.check() {
                    crate::browser_install::Check::Offer(offer) => {
                        let last = std::sync::atomic::AtomicU64::new(u64::MAX);
                        let progress = |got: u64, total: Option<u64>| {
                            let Some(total) = total.filter(|total| *total > 0) else {
                                return;
                            };
                            let percent = got.saturating_mul(100) / total;
                            // Told when the figure changes, not for every piece that arrives.
                            if last.swap(percent, std::sync::atomic::Ordering::Relaxed) != percent {
                                engine_job_on_main(EngineJob::Downloading(
                                    u8::try_from(percent.min(100)).ok(),
                                ));
                            }
                        };
                        installer
                            .install(&offer, &progress, &AtomicBool::new(false))
                            .map(drop)
                            .map_err(|e| e.to_string())
                    }
                    crate::browser_install::Check::Current => Ok(()),
                    crate::browser_install::Check::Refused(_) => {
                        Err("the engine on offer is not one this bateri uses".to_owned())
                    }
                    crate::browser_install::Check::Unavailable(why)
                    | crate::browser_install::Check::Absent(why) => Err(why),
                };
                engine_job_on_main(match outcome {
                    Ok(()) => EngineJob::Idle,
                    Err(why) => EngineJob::Failed(why),
                });
            });
    }

    /// What the engine's download does now: kept, the Apps page told, and the first click's card,
    /// which waits for it.
    fn set_engine_job(&self, job: EngineJob) {
        let progress = matches!(job, EngineJob::Downloading(_));
        self.ivars().engine_job.replace(job);
        // A figure that moves redraws the Links row's line, not the whole page.
        if progress && let Some(window) = self.settings_window() {
            window.refresh_links(self.file_settings().apps_open_links_with.as_deref());
        } else {
            self.apps_changed();
        }
        self.links_progress();
    }

    /// The download of the engine, as the Apps page shows it.
    pub(crate) fn engine_job(&self) -> EngineJob {
        self.ivars().engine_job.borrow().clone()
    }

    /// A web address ⌘-clicked in a pane of a server that has no browserpen: the card that
    /// offers to install it there and open the address, or to open it in the system's browser.
    /// Where the card cannot be offered the address opens in the system's browser.
    fn offer_link_card(&self, from: &TerminalPane, url: &str) {
        let Some(name) = self.settings().apps_open_links_with.clone() else {
            return hyperlink::open_url(url);
        };
        if !crate::apps_card::offer_link(self.mtm(), from, &name, url) {
            hyperlink::open_url(url);
        }
    }

    /// The tab and the pane of the pane `id`, when it is open.
    fn pane_in_tab(&self, id: u64) -> Option<(Retained<TerminalTab>, Retained<TerminalPane>)> {
        self.tabs().into_iter().find_map(|tab| {
            let pane = tab.panes().into_iter().find(|pane| pane.id() == id)?;
            Some((tab, pane))
        })
    }

    /// The program the card of a click brought to a server is there: the address opens in it, in
    /// a floating pane over the pane the click was in (the server's state has not said it has the
    /// program yet, so the route is not asked). The system's browser when that pane has gone.
    pub(crate) fn open_address_in_installed(&self, pane: u64, url: &str) {
        match self.pane_in_tab(pane) {
            Some((tab, from)) => self.start_address(&tab, &from, url),
            None => hyperlink::open_url(url),
        }
    }

    /// The first click's card has what its yes brought: the address goes the way any click on it goes
    /// now ([`Self::open_web_address`]): a tab of a browserpen open in the pane's tab, a floating pane
    /// that starts one, or, from a server's pane where that server has no browserpen, the card that
    /// offers to install it there, which wants the corner the first card is in, so that goes first.
    /// The system's browser when the pane has gone.
    pub(crate) fn open_address_here(&self, pane: u64, url: &str) {
        let Some((tab, from)) = self.pane_in_tab(pane) else {
            return hyperlink::open_url(url);
        };
        if web_links::frees_the_corner(&self.web_route(&tab, &from)) {
            crate::apps_card::remove_here(self.mtm());
        }
        self.open_web_address(&tab, &from, url);
    }
}

/// [`AppDelegate::set_engine_job`] from the download's thread.
fn engine_job_on_main(job: EngineJob) {
    DispatchQueue::main().exec_async(move || {
        // audit: a block running on the main queue is on the main thread by definition.
        let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
        if let Some(app) = delegate(mtm) {
            app.set_engine_job(job);
        }
    });
}

/// The question as the sheet of the settings window it is: the answers are to use browserpen and
/// to leave it (nothing is written for that).
fn note_alert(mtm: MainThreadMarker, question: &web_links::Question) -> Retained<NSAlert> {
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(&question.title));
    if let Some(detail) = &question.detail {
        alert.setInformativeText(&NSString::from_str(detail));
    }
    alert.addButtonWithTitle(ns_string!("Use browserpen"));
    let later = alert.addButtonWithTitle(ns_string!("Cancel"));
    later.setKeyEquivalent(ns_string!("\u{1b}"));
    alert
}

/// The question's sheet in each of its forms laid out and, with `shots`, written as pictures
/// (`links-note-{form}.png`): what it asks is the question's own words, so what the check looks
/// at is that every answer has room. Only for `tests/settings_window.rs`.
pub(crate) fn note_layout_problems(
    mtm: MainThreadMarker,
    shots: Option<&std::path::Path>,
) -> Vec<String> {
    let mut problems = Vec::new();
    for (name, program_here, engine_here, bytes) in [
        ("settings", false, false, Some(181_000_000)),
        ("settings-no-size", false, false, None),
    ] {
        let question = web_links::question(program_here, engine_here, bytes);
        let alert = note_alert(mtm, &question);
        alert.layout();
        let window = alert.window();
        let Some(content) = window.contentView() else {
            problems.push(format!("the note ({name}) has no content"));
            continue;
        };
        let bounds = content.bounds();
        if bounds.size.width < 200.0 || bounds.size.height < 80.0 {
            problems.push(format!(
                "the note ({name}) is {:.0} x {:.0}",
                bounds.size.width, bounds.size.height
            ));
        }
        for button in alert.buttons().iter() {
            let frame = button.frame();
            if frame.origin.x < 0.0 || frame.origin.x + frame.size.width > bounds.size.width + 0.5 {
                problems.push(format!(
                    "the note ({name}): the button {:?} leaves the sheet",
                    button.title()
                ));
            }
        }
        if let Some(dir) = shots
            && let Some(rep) = content.bitmapImageRepForCachingDisplayInRect(bounds)
        {
            content.cacheDisplayInRect_toBitmapImageRep(bounds, &rep);
            // SAFETY: the properties are an empty dictionary, which the type takes for no property.
            let png = unsafe {
                rep.representationUsingType_properties(
                    objc2_app_kit::NSBitmapImageFileType::PNG,
                    &objc2_foundation::NSDictionary::new(),
                )
            };
            if let Some(png) = png {
                let _ = std::fs::write(dir.join(format!("links-note-{name}.png")), png.to_vec());
            }
        }
    }
    problems
}

thread_local! {
    /// The folders whose size was measured, and what it came to (a folder that cannot be read: 0).
    static SIZES: RefCell<Vec<(PathBuf, u64)>> = const { RefCell::new(Vec::new()) };
}

/// Set while a folder is being measured: one at a time.
static MEASURING: AtomicBool = AtomicBool::new(false);

/// The size of what is under `folder`, in bytes, as it was measured: the Apps page asks on every
/// refresh, and the engine is a few thousand files, so the first ask starts a measure on a thread
/// of its own (`None` until it is done, when the Links row is drawn again) and the size is kept.
fn folder_bytes(folder: &std::path::Path) -> Option<u64> {
    let known = SIZES.with(|sizes| {
        sizes
            .borrow()
            .iter()
            .find(|(seen, _)| seen == folder)
            .map(|(_, bytes)| *bytes)
    });
    if let Some(bytes) = known {
        return (bytes > 0).then_some(bytes);
    }
    if !MEASURING.swap(true, std::sync::atomic::Ordering::AcqRel) {
        let folder = folder.to_owned();
        let started = std::thread::Builder::new()
            .name("engine-size".into())
            .spawn(move || {
                let bytes = folder_walk(&folder, 0);
                DispatchQueue::main().exec_async(move || {
                    SIZES.with(|sizes| sizes.borrow_mut().push((folder, bytes)));
                    MEASURING.store(false, std::sync::atomic::Ordering::Release);
                    // audit: a block running on the main queue is on the main thread by definition.
                    let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
                    if let Some(app) = delegate(mtm)
                        && let Some(window) = app.settings_window()
                    {
                        window.refresh_links(app.file_settings().apps_open_links_with.as_deref());
                    }
                });
            });
        if started.is_err() {
            MEASURING.store(false, std::sync::atomic::Ordering::Release);
        }
    }
    None
}

/// What the files under `folder` add up to, going down at most 16 folders.
fn folder_walk(folder: &std::path::Path, depth: usize) -> u64 {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() && depth < 16 => folder_walk(&entry.path(), depth + 1),
            Ok(kind) if kind.is_file() => entry.metadata().map_or(0, |meta| meta.len()),
            _ => 0,
        })
        .sum()
}
