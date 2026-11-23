//! Where a web address ⌘-clicked in a terminal opens, and a file that a running program takes
//! (`[apps] open_links_with`, `[apps] open_with`): beside the terminal in the program the person
//! chose, or in the system's browser.
//!
//! The decision is [`bt_shell_common::web_links::route`], from what this computer already knows;
//! this module reads that state (the panes of the tab and what their programs said they take, what
//! is installed here and what a server said it has, whether the engine is on the disk) and does what
//! the route says. **A click never waits for the network**: a program that is not there is the
//! system's browser, and the one-time note comes after the address has opened.

use std::sync::atomic::AtomicBool;

use bt_core::pencere::{Take, TakeKind};
use bt_shell_common::web_links::{self, Facts, Machine, Pane, Route};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAlertSecondButtonReturn, NSModalResponse,
};
use objc2_foundation::{NSString, ns_string};

use super::*;
use crate::pane::PaneMachine;
use crate::sheets::{self, Asker};
use crate::{hyperlink, launch};

/// Where the question of the one-time note is put.
enum Place {
    /// Over a pane: the click that asked.
    Pane(u64),
    /// Over the settings window: the person chose browserpen there.
    Settings,
}

/// What the download of the browser engine, which saying yes to the note starts, is doing: what
/// the Apps page shows beside its row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum EngineJob {
    /// Nothing is being downloaded.
    Idle,
    /// The release was asked for.
    Looking,
    /// Bytes are coming; the percent when the size is known.
    Downloading(Option<u8>),
    /// It did not come: the words for the person, until the next try.
    Failed(String),
}

impl AppDelegate {
    /// A web address ⌘-clicked in `from`, a pane of `tab`.
    pub(crate) fn open_web_address(&self, tab: &TerminalTab, from: &TerminalPane, url: &str) {
        match self.web_route(tab, from) {
            Route::System => hyperlink::open_url(url),
            Route::SystemThenAsk => {
                hyperlink::open_url(url);
                self.ask_about_links(from);
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
        let installed;
        let facts_machine = match &machine {
            PaneMachine::Elsewhere => return Route::System,
            PaneMachine::Here => Machine::Here { openers: &openers },
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
        web_links::route(&Facts {
            setting: setting.as_deref(),
            machine: facts_machine,
            engine: crate::browser_install::engine_ready(),
            offered: crate::browser_install::installer().is_some(),
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
    ) -> bool {
        let machine = from.machine();
        if machine == PaneMachine::Elsewhere {
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
        self.open_program(
            tab,
            from,
            launch::OpenProgram {
                name,
                target: launch::OpenTarget::Address(url.to_owned()),
                server,
            },
        );
    }

    /// The one-time note: whether to open links beside the terminal with browserpen. Looked up in
    /// the background, so the click that asked has opened in the system's browser already; the
    /// note comes when the release notice has said what it would download, and not at all when
    /// the notice cannot be had (the next click asks again). One question at a time, and one
    /// that was put off ("Not Now") is not put again before the next start.
    fn ask_about_links(&self, from: &TerminalPane) {
        if self.ivars().links_asked.replace(true) {
            return;
        }
        if !self.look_up_notice(Place::Pane(from.id())) {
            self.ivars().links_asked.set(false);
        }
    }

    /// Settings ▸ Apps ▸ Links ▸ browserpen, where it is not all here: the same question, as a sheet
    /// of the settings window, with *Use browserpen* and *Cancel* (what the person did is
    /// the choice; nothing is written for a cancel).
    pub(crate) fn ask_to_use_browserpen(&self) {
        self.look_up_notice(Place::Settings);
    }

    /// Reads the release notice on a thread of its own and puts the question where `place` is
    /// when it is read. `false` when this copy has no release to look at (a copy that runs an engine
    /// where it is): there is nothing to offer.
    fn look_up_notice(&self, place: Place) -> bool {
        let Some(installer) = crate::browser_install::installer() else {
            return false;
        };
        let _ = std::thread::Builder::new()
            .name("links-notice".into())
            .spawn(move || {
                let check = installer.check();
                DispatchQueue::main().exec_async(move || {
                    // audit: a block running on the main queue is on the main thread by definition.
                    let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
                    if let Some(app) = delegate(mtm) {
                        app.links_noticed(place, check);
                    }
                });
            });
        true
    }

    /// The release notice was read: the question, with what it would download.
    fn links_noticed(&self, place: Place, check: crate::browser_install::Check) {
        use crate::browser_install::Check;
        let from_click = matches!(place, Place::Pane(_));
        let engine = crate::browser_install::engine_installed();
        let bytes = match &check {
            Check::Offer(offer) => Some(offer.asset.size),
            Check::Current => None,
            Check::Refused(_) | Check::Unavailable(_) if from_click => {
                // Nothing to promise: the next click asks again.
                self.ivars().links_asked.set(false);
                return;
            }
            // The person asked, and the size is not known: the question says what it brings.
            Check::Refused(_) | Check::Unavailable(_) => None,
        };
        let (program_here, _) = self.links_facts();
        let question = web_links::question(program_here, engine.is_some(), bytes);
        let seat = match &place {
            Place::Pane(id) => self
                .pane(*id)
                .and_then(|pane| sheets::seat(Asker::Pane(&pane))),
            Place::Settings => self
                .settings_window()
                .and_then(|window| window.ns_window())
                .and_then(|window| sheets::seat(Asker::Window(&window))),
        }
        .filter(|seat| !seat.is_taken());
        let Some(seat) = seat else {
            if from_click {
                self.ivars().links_asked.set(false);
            }
            return;
        };
        let alert = note_alert(self.mtm(), &question, from_click);
        let answered = RcBlock::new(move |response: NSModalResponse| {
            // audit: the sheet's completion block runs on AppKit's main thread.
            let mtm = MainThreadMarker::new().expect("the sheet block is on the main thread");
            let Some(app) = delegate(mtm) else {
                return;
            };
            if response == NSAlertFirstButtonReturn {
                app.use_browserpen_for_links();
            } else if from_click && response == NSAlertSecondButtonReturn {
                app.save_edit(&SettingsEdit::AppsOpenLinksWith(String::new()));
            }
            app.refresh_settings_window();
        });
        seat.begin(&alert, &answered);
    }

    /// What the Links row is built from: whether browserpen is installed here, and the engine as
    /// it is now (on its way, failed, on the disk with its version and size, or not here).
    pub(crate) fn links_facts(&self) -> (bool, web_links::Engine) {
        let program_here = self
            .openers()
            .iter()
            .any(|opener| opener.name == web_links::BROWSER_PROGRAM && opener.opens_addresses());
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
    /// engine are not both on the disk yet).
    pub(crate) fn use_browserpen_for_links(&self) {
        let mut edits = Vec::new();
        {
            let settings = self.file_settings();
            let source = format!("bateri/{}", web_links::BROWSER_PROGRAM);
            if settings.apps_sources_whole && !settings.apps_sources.contains(&source) {
                let mut sources = settings.apps_sources.clone();
                sources.push(source);
                edits.push(SettingsEdit::AppsSources(sources));
            }
        }
        edits.push(SettingsEdit::AppsOpenLinksWith(
            web_links::BROWSER_PROGRAM.to_owned(),
        ));
        let _ = self.save_edits(&edits);
        self.download_engine();
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
                    crate::browser_install::Check::Refused(why) => Err(format!(
                        "the engine on offer is not one this bateri uses ({why:?})"
                    )),
                    crate::browser_install::Check::Unavailable(why) => Err(why),
                };
                engine_job_on_main(match outcome {
                    Ok(()) => EngineJob::Idle,
                    Err(why) => EngineJob::Failed(why),
                });
            });
    }

    /// What the engine's download does now: kept, and the Apps page told.
    fn set_engine_job(&self, job: EngineJob) {
        self.ivars().engine_job.replace(job);
        self.apps_changed();
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

    /// The program the card of a click brought to a server is there: the address opens in it, in
    /// a floating pane over the pane the click was in (the server's state has not said it has the
    /// program yet, so the route is not asked). The system's browser when that pane has gone.
    pub(crate) fn open_address_in_installed(&self, pane: u64, url: &str) {
        let tab = self
            .tabs()
            .into_iter()
            .find(|tab| tab.panes().iter().any(|candidate| candidate.id() == pane));
        let from = tab.as_ref().and_then(|tab| {
            tab.panes()
                .into_iter()
                .find(|candidate| candidate.id() == pane)
        });
        match (tab, from) {
            (Some(tab), Some(from)) => self.start_address(&tab, &from, url),
            _ => hyperlink::open_url(url),
        }
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

/// The one-time note as the sheet it is: the question, and the answers (from a click, three: use
/// browserpen, use the person's browser for good, not now; from the settings window, two).
fn note_alert(
    mtm: MainThreadMarker,
    question: &web_links::Question,
    from_click: bool,
) -> Retained<NSAlert> {
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(&question.title));
    if let Some(detail) = &question.detail {
        alert.setInformativeText(&NSString::from_str(detail));
    }
    alert.addButtonWithTitle(ns_string!("Use browserpen"));
    if from_click {
        alert.addButtonWithTitle(ns_string!("Use Your Browser"));
    }
    let later = alert.addButtonWithTitle(if from_click {
        ns_string!("Not Now")
    } else {
        ns_string!("Cancel")
    });
    later.setKeyEquivalent(ns_string!("\u{1b}"));
    alert
}

/// The note in each of its forms laid out and, with `shots`, written as pictures
/// (`links-note-{form}.png`): what it asks is the question's own words, so what the check looks
/// at is that every answer has room. Only for `tests/settings_window.rs`.
pub(crate) fn note_layout_problems(
    mtm: MainThreadMarker,
    shots: Option<&std::path::Path>,
) -> Vec<String> {
    let mut problems = Vec::new();
    for (name, program_here, engine_here, bytes, from_click) in [
        ("click", false, false, Some(181_000_000), true),
        ("click-engine-here", false, true, None, true),
        ("settings", false, false, Some(181_000_000), false),
        ("settings-no-size", false, false, None, false),
    ] {
        let question = web_links::question(program_here, engine_here, bytes);
        let alert = note_alert(mtm, &question, from_click);
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

/// The size of what is under `folder`, in bytes, read once per folder: the Apps page asks on every
/// refresh, and the engine is a few thousand files.
fn folder_bytes(folder: &std::path::Path) -> Option<u64> {
    thread_local! {
        static SIZES: RefCell<Vec<(PathBuf, u64)>> = const { RefCell::new(Vec::new()) };
    }
    if let Some(known) = SIZES.with(|sizes| {
        sizes
            .borrow()
            .iter()
            .find(|(seen, _)| seen == folder)
            .map(|(_, bytes)| *bytes)
    }) {
        return Some(known);
    }
    fn walk(folder: &std::path::Path, depth: usize) -> u64 {
        let Ok(entries) = std::fs::read_dir(folder) else {
            return 0;
        };
        entries
            .flatten()
            .map(|entry| match entry.file_type() {
                Ok(kind) if kind.is_dir() && depth < 16 => walk(&entry.path(), depth + 1),
                Ok(kind) if kind.is_file() => entry.metadata().map_or(0, |meta| meta.len()),
                _ => 0,
            })
            .sum()
    }
    let bytes = walk(folder, 0);
    if bytes == 0 {
        return None;
    }
    SIZES.with(|sizes| sizes.borrow_mut().push((folder.to_owned(), bytes)));
    Some(bytes)
}
