//! Terminal window: an `NSWindow` and its `NSWindowDelegate`, and what
//! belongs to the **window** rather than to one of its tabs — chrome, the
//! title and tab dot it writes, the close question and its scope, and the
//! tab and split actions (`closeTab:`, `closeWindow:`, `selectTab:`,
//! `splitRight:`, `splitDown:`, `selectPreviousSplit:`/`selectNextSplit:`,
//! `selectSplit:`, `resizeSplit:`, `equalizeSplits:`, `toggleSplitZoom:`).
//! The responder chain reaches the window's delegate, never a tab, so the
//! actions are here and hand the tab's work to its tab
//! (`tab::TerminalTab`: the splits container, the panes, the focused pane,
//! the title's read). Today a window carries one tab.
//!
//! The window's inputs come from `AppDelegate::open_window`; the panes'
//! events reach their tab (`tab::TabHost`) and through it the window or the
//! application. The application-wide parts (settings, watching, subtitle
//! slots, measurement ledger, timed-run recipe, window list) are in `app`;
//! the save-time paths coming from there reach **every tab**. The window's
//! geometry, occlusion and focus notifications are distributed to **all**
//! panes too. There is no drawing call here either; this file's job is
//! wiring.
//!
//! Renderer per pane (`pane`'s header).

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::path::PathBuf;
use std::time::Instant;

use block2::RcBlock;
use bt_core::{
    ConfirmClose, ContentEdge, InitialInput, Settings, ShutdownHandle, TabId, Teardown, Theme,
    contrast_ratio,
};
use bt_gpu::GpuError;
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAppearance, NSAppearanceCustomization,
    NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication, NSBackingStoreType, NSBox,
    NSBoxType, NSColor, NSControlStateValueOff, NSControlStateValueOn, NSFloatingWindowLevel,
    NSMenuItem, NSModalResponse, NSModalResponseCancel, NSTitlePosition, NSTitlebarSeparatorStyle,
    NSWindow, NSWindowDelegate, NSWindowOcclusionState, NSWindowOrderingMode, NSWindowStyleMask,
};
use objc2_foundation::{
    NSKeyValueObservingOptions, NSNotification, NSObject, NSObjectNSKeyValueObserverRegistration,
    NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, ns_string,
};

use crate::Run;
use crate::app::{self, AppDelegate};
use crate::jobs::Foreground;
use crate::pane::{PaneLaunch, TerminalPane};
use crate::restore::SavedTab;
use crate::sheets::{self, Asker};
use crate::split::{Axis, Direction};
use crate::tab::{self, TerminalTab};
use crate::tabs::tab_index;

/// Whether the theme's background is dark — the window chrome's appearance
/// (Aqua / DarkAqua) comes from this ([`TerminalWindow::apply_chrome`]).
///
/// The question is "which text reads better on this background: white or
/// black" and the answer is from WCAG's contrast ratio
/// ([`bt_core::contrast_ratio`], the measure the theme's own choices are made
/// with): if the background gives a higher contrast with white, it is dark.
/// The threshold is not invented, it arises from the equality of the two
/// ratios; the system's dark appearance means exactly "light text".
///
/// Not in `bt-core`'s `Theme` but here: lightness is not a theme role, it is
/// a translation into AppKit's appearance vocabulary.
pub(crate) fn is_dark_background(theme: &Theme) -> bool {
    contrast_ratio(theme.background, 0xffffff) > contrast_ratio(theme.background, 0x000000)
}

/// A window closing that has begun ([`TerminalWindow::begin_close`]).
pub(crate) enum Closing {
    /// This call started it; the handle knows the result.
    Started(ShutdownHandle),
    /// The closing had begun before (like ⌘Q while the window is closing):
    /// nothing to wait for, the first call knew the real result.
    AlreadyDone,
}

impl Closing {
    /// Waits until `deadline` at the latest ([`ShutdownHandle::wait_until`]).
    pub(crate) fn wait_until(self, deadline: Instant) -> Teardown {
        match self {
            Self::Started(handle) => handle.wait_until(deadline),
            Self::AlreadyDone => Teardown::AlreadyDone,
        }
    }
}

/// What the confirmed question will close ([`TerminalWindow::ask`]): ids,
/// looked up again at answer time.
#[derive(Clone, Debug)]
enum CloseTarget {
    /// Tabs (window ids), with all their panes.
    Tabs(Vec<u64>),
    /// A single pane of a tab (tab and pane ids); the tab stays open.
    Pane { tab: u64, pane: u64 },
}

/// Title of the Shell ▸ Close Tab item: with several panes ⌘W
/// closes the focused pane and the item is "Close", with one pane it closes
/// the tab and is "Close Tab".
pub(crate) fn close_title(panes: usize) -> &'static str {
    if panes > 1 { "Close" } else { "Close Tab" }
}

/// What is being closed — chooses the question's title and confirm button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CloseScope {
    /// A pane whose tab has other panes (⌘W).
    Pane,
    /// A tab whose group has other tabs (⌘W).
    Tab,
    /// Part of the group, several tabs ("Close Other Tabs").
    Tabs(usize),
    /// The whole window: ⌘W in a single-tab window, or ⇧⌘W.
    Window,
    /// The application (⌘Q, Dock ▸ Quit, logout).
    Quit,
}

/// The number of tabs a gesture asks for and the group's size → the
/// question's scope.
///
/// The whole group is the window (red button, ⌘W in a single-tab window), a
/// single tab is the tab (⌘W), everything in between is counted tabs
/// ("Close Other Tabs").
pub(crate) fn close_scope(requested: usize, group: usize) -> CloseScope {
    if requested >= group {
        CloseScope::Window
    } else if requested == 1 {
        CloseScope::Tab
    } else {
        CloseScope::Tabs(requested)
    }
}

/// Whether to ask on close — the **single** decision of the three closing
/// paths.
///
/// The timed run is the **first** question and its answer is no under every
/// setting: a timed run reads no settings, so `confirm` there is the default
/// `running`, and a headless question with no guard set up would hang `make
/// smoke`. The process table (`running`) is read only if the answer depends
/// on it, i.e. only under `running`.
pub(crate) fn should_ask(
    timed: bool,
    confirm: ConfirmClose,
    running: impl FnOnce() -> bool,
) -> bool {
    if timed {
        return false;
    }
    match confirm {
        ConfirmClose::Never => false,
        ConfirmClose::Always => true,
        ConfirmClose::Running => running(),
    }
}

/// The foreground of every closing pane if it will ask, `None` if it will
/// not — [`should_ask`] over panes (the question gathers the
/// running job from the **panes**, not the tabs).
///
/// Under `running` the table is read once for the decision and the text uses
/// the same read; under `always` the decision does not look at the table but
/// the text still wants to name the running job, so it is read once the
/// question is settled.
pub(crate) fn foregrounds_to_ask(
    timed: bool,
    confirm: ConfirmClose,
    panes: &[Retained<TerminalPane>],
) -> Option<Vec<Foreground>> {
    let read = || {
        panes
            .iter()
            .map(|pane| pane.foreground())
            .collect::<Vec<_>>()
    };
    let mut seen = None;
    let ask = should_ask(timed, confirm, || {
        let foregrounds = read();
        let running = foregrounds
            .iter()
            .any(|foreground| matches!(foreground, Foreground::Running(_)));
        seen = Some(foregrounds);
        running
    });
    ask.then(|| seen.unwrap_or_else(read))
}

/// The unit the question counts: tab or pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unit {
    Tab,
    Pane,
}

impl Unit {
    fn plural(self) -> &'static str {
        match self {
            Unit::Tab => "tabs",
            Unit::Pane => "panes",
        }
    }
}

/// The unit from the count of closing panes and tabs: if a tab has several
/// panes what is counted is panes, otherwise tabs — for single-pane tabs the
/// text is byte for byte the same as the text before splits. Pure, tested.
pub(crate) fn unit_for(panes: usize, tabs: usize) -> Unit {
    if panes > tabs { Unit::Pane } else { Unit::Tab }
}

/// The question's text: title, explanation and confirm button.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Prompt {
    pub(crate) title: String,
    pub(crate) message: String,
    pub(crate) confirm: &'static str,
}

/// The question's text from the closing panes' foregrounds. Pure;
/// the single text source of the three paths that build the question
/// ([`alert`]).
///
/// The explanation counts the running jobs **by name**: with a single pane
/// the names ("“claude” is still running."), with several the `unit` count
/// (tab or pane, [`unit_for`]) and deduplicated names. A job whose name could
/// not be read is stated nameless ("A process"), and if there is no running
/// job at all (`always`) what will be closed.
pub(crate) fn prompt(scope: CloseScope, unit: Unit, tabs: &[Foreground]) -> Prompt {
    let (title, confirm, verb) = match scope {
        CloseScope::Pane => ("Close this pane?".to_owned(), "Close", "Closing"),
        CloseScope::Tab => ("Close this tab?".to_owned(), "Close", "Closing"),
        CloseScope::Tabs(n) => (format!("Close {n} tabs?"), "Close", "Closing"),
        CloseScope::Window => ("Close this window?".to_owned(), "Close", "Closing"),
        CloseScope::Quit => ("Quit bateri?".to_owned(), "Quit", "Quitting"),
    };
    let running: Vec<&[String]> = tabs
        .iter()
        .filter_map(|tab| match tab {
            Foreground::Running(names) => Some(names.as_slice()),
            Foreground::Idle => None,
        })
        .collect();
    let message = match running.as_slice() {
        [] => idle_message(scope, unit, tabs.len()),
        [names] => {
            let (subject, pronoun) = match names {
                [] => ("A process is".to_owned(), "it"),
                [name] => (format!("{} is", quoted(name)), "it"),
                _ => (format!("{} are", listed(names)), "them"),
            };
            format!("{subject} still running. {verb} ends {pronoun}.")
        }
        many => {
            let mut names: Vec<&String> = Vec::new();
            for name in many.iter().flat_map(|names| names.iter()) {
                if !names.contains(&name) {
                    names.push(name);
                }
            }
            let count = many.len();
            let unit = unit.plural();
            if names.is_empty() {
                format!("Processes are running in {count} {unit}. {verb} ends them.")
            } else {
                let names: Vec<String> = names.into_iter().map(|name| quoted(name)).collect();
                format!(
                    "Processes are running in {count} {unit}: {}. {verb} ends them.",
                    names.join(", ")
                )
            }
        }
    };
    Prompt {
        title,
        message,
        confirm,
    }
}

/// `always`'s text without a running job: states what will be closed.
fn idle_message(scope: CloseScope, unit: Unit, tabs: usize) -> String {
    match (scope, tabs) {
        (CloseScope::Pane, _) => "Closing this pane ends its shell session.".to_owned(),
        (CloseScope::Tab, _) => "Closing this tab ends its shell session.".to_owned(),
        (CloseScope::Tabs(n), _) => format!("Closing these {n} tabs ends their shell sessions."),
        (CloseScope::Window, 0 | 1) => "Closing this window ends its shell session.".to_owned(),
        (CloseScope::Window, n) => {
            format!(
                "Closing this window ends the shell sessions in its {n} {}.",
                unit.plural()
            )
        }
        (CloseScope::Quit, 0 | 1) => "Quitting ends the open shell session.".to_owned(),
        (CloseScope::Quit, n) => format!("Quitting ends {n} open shell sessions."),
    }
}

/// A name in macOS's typographic quotes.
fn quoted(name: &str) -> String {
    format!("\u{201c}{name}\u{201d}")
}

/// "“a”", "“a” and “b”", "“a”, “b” and “c”".
fn listed(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => quoted(one),
        [rest @ .., last] => {
            let rest: Vec<String> = rest.iter().map(|name| quoted(name)).collect();
            format!("{} and {}", rest.join(", "), quoted(last))
        }
    }
}

/// The reminder a quit leaves when it keeps the programs running
/// (`keep_running = "quit"`): a system notification after bateri is gone,
/// so a program left behind is not forgotten. Pure.
///
/// `None` when no pane runs a program — an idle shell costs nothing and
/// comes back at the next launch anyway — and while the Mac logs out or
/// restarts (`powering_off`): the programs end with it. The names are
/// deduplicated across the panes, at most three of them, then "and N more";
/// a program whose name could not be read is counted, not named.
pub(crate) fn kept_notice(tabs: &[Foreground], powering_off: bool) -> Option<Notice> {
    /// How many names the notification spells out; the rest are counted.
    const SHOWN: usize = 3;
    if powering_off {
        return None;
    }
    let mut names: Vec<String> = Vec::new();
    let mut nameless = 0;
    for tab in tabs {
        match tab {
            Foreground::Running(running) if running.is_empty() => nameless += 1,
            Foreground::Running(running) => {
                for name in running {
                    if !names.contains(name) {
                        names.push(name.clone());
                    }
                }
            }
            Foreground::Idle => {}
        }
    }
    let count = names.len() + nameless;
    let more = names.len().saturating_sub(SHOWN) + nameless;
    names.truncate(SHOWN);
    let subject = match (names.is_empty(), more) {
        (true, 0) => return None,
        (true, 1) => "A program".to_owned(),
        (true, _) => "Programs".to_owned(),
        (false, 0) => listed(&names),
        (false, _) => {
            let names: Vec<String> = names.iter().map(|name| quoted(name)).collect();
            format!("{} and {more} more", names.join(", "))
        }
    };
    let (verb, pronoun) = if count == 1 {
        ("keeps", "it")
    } else {
        ("keep", "them")
    };
    Some(Notice {
        title: "Programs keep running",
        body: format!(
            "{subject} {verb} running in the background. Open bateri to return to \
             {pronoun}; \u{2325}\u{2318}Q ends {pronoun}."
        ),
    })
}

/// A notification's text ([`kept_notice`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Notice {
    pub(crate) title: &'static str,
    pub(crate) body: String,
}

/// `NSAlert` from a [`Prompt`]: confirm is the first button (Return),
/// "Cancel" the second (Esc).
///
/// Esc is bound **by hand**: the documentation says it binds Esc to a button
/// titled "Cancel" itself, but in a real window Esc did not close the sheet
/// (measured); Return worked on the first button.
pub(crate) fn alert(mtm: MainThreadMarker, prompt: &Prompt) -> Retained<NSAlert> {
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(&prompt.title));
    alert.setInformativeText(&NSString::from_str(&prompt.message));
    alert.addButtonWithTitle(&NSString::from_str(prompt.confirm));
    let cancel = alert.addButtonWithTitle(ns_string!("Cancel"));
    cancel.setKeyEquivalent(ns_string!("\u{1b}"));
    alert
}

/// The direction of a Select/Resize Split ▸ item: the sender's `tag`.
fn direction_of(sender: Option<&AnyObject>) -> Option<Direction> {
    let item = sender?.downcast_ref::<NSMenuItem>()?;
    Direction::from_tag(item.tag())
}

/// Collects the `windowShouldClose:` requests at the end of the turn: a
/// single decision per group with the flag set ([`TerminalWindow::should_close_now`]).
fn close_requested_tabs(app: &AppDelegate) {
    while let Some(anchor) = app
        .windows()
        .into_iter()
        .find(|window| window.ivars().close_requested.get())
    {
        anchor.close_requested_group(app);
        // A flag that fell outside a group (in the list but whose group
        // cannot be resolved) must not lock the loop.
        anchor.ivars().close_requested.set(false);
    }
}

/// The window's state — chrome, tab dot, the close question and its tab.
/// The splits container, the panes and the focused pane are the tab's
/// ([`TerminalTab`]); the session's core is in the panes ([`TerminalPane`]).
pub(crate) struct WindowIvars {
    /// Our own counter ([`AppDelegate`] hands it out): the key by which the
    /// close question and removal from the list find the window. The tab's
    /// and the pane's ids are separate ([`TerminalTab::id`],
    /// [`TerminalPane::id`]) and from the same counter.
    id: u64,
    /// The timed run's recipe, a copy of `AppDelegate`'s (`Copy`): the close
    /// question is never asked in a timed run and must be answerable without
    /// reaching the application delegate ([`TerminalWindow::should_close_now`]).
    run: Option<Run>,
    window: Retained<NSWindow>,
    /// The window's tab: its container is the `contentView`. The only strong
    /// reference to the tab object — the list finds it through the window
    /// ([`TerminalWindow::tabs`]).
    tab: Retained<TerminalTab>,
    /// The background the chrome was last painted with (the gate of
    /// [`TerminalWindow::apply_chrome`]); `None`: not painted yet.
    chrome: Cell<Option<u32>>,
    /// The last colour set for the tab's dot, sRGB (the gate of
    /// [`TerminalWindow::refresh_tab_mark`]); `None`: no dot.
    tab_mark: Cell<Option<u32>>,
    /// The open close question in this window: keeps the `NSAlert`
    /// alive for the sheet's duration and is the "no second question while the
    /// sheet is open" gate ([`TerminalWindow::asking`]). The completion block
    /// empties it on every answer.
    alert: RefCell<Option<Retained<NSAlert>>>,
    /// In this turn `windowShouldClose:` asked for this tab — the gesture's
    /// scope is gathered from these flags at the end of the turn
    /// ([`TerminalWindow::close_requested_tabs`]).
    close_requested: Cell<bool>,
}

/// The new shell's birth information — the two decisions the birth package
/// (`PaneLaunch::launch`) takes from the caller (`AppDelegate::open_window`).
pub(crate) struct Launch {
    /// Start directory (the active tab's directory, else home).
    pub(crate) working_directory: Option<PathBuf>,
    /// The shell's first input and whether it runs: ⌘T in a
    /// remote tab runs it, a restored remote pane leaves it ready;
    /// `None` → an ordinary local shell.
    pub(crate) initial_input: Option<InitialInput>,
    /// The pane's persistent identity; `None` → a new one. A restored
    /// pane keeps its saved one, so `bateri://tab/<id>` and
    /// `TERM_SESSION_ID` survive the quit.
    pub(crate) tab_id: Option<TabId>,
    /// A previous session's scrollback, replayed before the shell starts
    /// (`SessionOptions::replay`); `None` → an empty grid.
    pub(crate) replay: Option<Vec<u8>>,
    /// The update's handover: a running program to carry on
    /// instead of a new shell; `None` → a shell is born.
    pub(crate) adopt: Option<Adopted>,
}

/// A pane the previous bateri froze and its holder gave, checked to be
/// adoptable (`AppDelegate`'s arrival): the master, the exit watch of its
/// child, the pane's state and the bytes to read before the master's.
#[derive(Debug)]
pub(crate) struct Adopted {
    pub(crate) master: std::os::fd::OwnedFd,
    pub(crate) exit: std::os::fd::OwnedFd,
    pub(crate) pid: u32,
    pub(crate) state: crate::handover::PaneState,
    /// The frozen tail, then what the holder drained (`HeldPane::buffer`).
    pub(crate) prefix: Vec<u8>,
    /// The socket of the holder it came from: the pane registers with this
    /// bateri's own holder unconfirmed until that one is acknowledged.
    pub(crate) taken_from: Option<std::path::PathBuf>,
    /// Which kind of holder it came from (`bt_core::AdoptMode`).
    pub(crate) mode: bt_core::AdoptMode,
    /// Its screen did not come back whole: the program is nudged to redraw
    /// it (`Session::nudge_size`).
    pub(crate) nudge: bool,
    /// The note if the session cannot be adopted after all and the pane
    /// falls back to a new shell.
    pub(crate) note: Note,
}

/// The dim line a pane says when it does not come back whole — no pane
/// comes back as a half screen, or as a new shell, without saying so.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Note {
    /// The program did not cross an update.
    Update,
    /// The program was not carried through a crash (its holder did not have
    /// it, or could not give it).
    Crash,
    /// A deliberate handover to a bound holder (a quit, or an update that
    /// found one) did not carry the program — which of the two it was, the
    /// holder cannot tell.
    NotCarried,
    /// The program ended while bateri was closed.
    Ended,
    /// The program runs on, its screen did not come back: it redraws it.
    Screenless,
    /// The program runs on, but the output of while bateri was closed lost
    /// its oldest part.
    Cut,
}

impl Note {
    pub(crate) fn text(self) -> &'static str {
        match self {
            Note::Update => {
                "bateri: the program running here did not survive the update; this is a new shell"
            }
            Note::Crash => {
                "bateri: the program running here did not survive the crash; this is a new shell"
            }
            Note::NotCarried => {
                "bateri: the program running here could not be carried over; this is a new shell"
            }
            Note::Ended => {
                "bateri: the program running here ended while bateri was closed; this is a new shell"
            }
            Note::Screenless => {
                "bateri: the screen could not be restored; the program kept running"
            }
            Note::Cut => "bateri: output from while bateri was closed was cut short",
        }
    }

    /// The note as a line of its own: dim, and the pen reset after it.
    pub(crate) fn line(self) -> Vec<u8> {
        let mut line = b"\x1b[0m\x1b[2m".to_vec();
        line.extend_from_slice(self.text().as_bytes());
        line.extend_from_slice(b"\x1b[0m\r\n");
        line
    }
}

/// `history` (if any) with `note` under it, on a line of its own: the
/// replay of a pane that fell back to a new shell.
pub(crate) fn fallen_back(history: Option<Vec<u8>>, note: Note) -> Vec<u8> {
    let mut replay = history.unwrap_or_default();
    if !replay.is_empty() && !replay.ends_with(b"\n") {
        replay.extend_from_slice(b"\r\n");
    }
    replay.extend_from_slice(&note.line());
    replay
}

define_class!(
    // SAFETY: NSObject has no subclassing requirement; TerminalWindow implements no Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriTerminalWindow"]
    #[ivars = WindowIvars]
    pub(crate) struct TerminalWindow;

    unsafe impl NSObjectProtocol for TerminalWindow {}

    unsafe impl NSWindowDelegate for TerminalWindow {
        // The frame is part of the layout the bound holder keeps; a drag or
        // a live resize is a burst the delayed trigger folds into one.
        #[unsafe(method(windowDidMove:))]
        fn window_did_move(&self, _n: &NSNotification) {
            self.layout_changed();
        }

        #[unsafe(method(windowDidResize:))]
        fn window_did_resize(&self, _n: &NSNotification) {
            self.layout_changed();
        }

        // On a move between screens the size (points) does not change but the
        // scale does; nobody but us writes that in a layer-hosting view.
        //
        // The split boundaries sit on the device pixel, so on a scale change
        // first the re-layout, then **every** pane's geometry: the notification
        // of a pane whose frame did not change does not arrive.
        #[unsafe(method(windowDidChangeBackingProperties:))]
        fn window_did_change_backing(&self, _n: &NSNotification) {
            for tab in self.tabs() {
                tab.refresh_geometry();
            }
        }

        // Visibility path: the compositor can discard the layer contents of an
        // occluded or miniaturized window. Since the grid did not change no
        // damage flag is set and the link keeps sleeping — the returning
        // window stays blank.
        //
        // A single hook suffices: both miniaturizing and occlusion drop
        // `occlusionState`, so `windowDidDeminiaturize:` would be a subset of
        // it. **The non-selected tab goes through this path too** (measured): when a tab goes to the back `visible=false` arrives, when
        // it comes to the front `true`, so a background tab draws zero frames
        // and there is no tab-specific hook. Stacking special cases on top of
        // the general signal would mean the list never closes (full screen,
        // Space, `unhide`, screen wake...).
        #[unsafe(method(windowDidChangeOcclusionState:))]
        fn window_did_change_occlusion(&self, _n: &NSNotification) {
            // The notification comes in both directions; asking for a frame
            // while GOING occluded would mean drawing a frame nobody will see.
            let visible = self
                .ivars()
                .window
                .occlusionState()
                .contains(NSWindowOcclusionState::Visible);
            // A hidden pane behind the zoom is counted as occluded
            // (`SplitView::apply_visibility`).
            for tab in self.tabs() {
                tab.apply_visibility(visible);
            }
        }

        // **Focus path.** In an unfocused window the caret's inside empties and
        // the blink stops; both are `bt-gpu`'s decision, `bt-core` never sees
        // focus.
        //
        // The "a single hook suffices" reasoning above **does not carry over
        // here**: there occlusion and miniaturizing are two states of the same
        // general signal (`occlusionState`), here there are two separate facts
        // and AppKit gives them with separate notifications — there is no
        // general signal to merge them.
        //
        // The window's key bit goes to **all** panes; an
        // unfocused pane's hollow caret comes from the second bit, from its
        // own `BateriView`'s first-responder hooks.
        #[unsafe(method(windowDidBecomeKey:))]
        fn window_did_become_key(&self, _n: &NSNotification) {
            for pane in self.panes() {
                pane.apply_focus(true);
                pane.rehover_upload();
                // Coming back to the window is an interaction: the remote load
                // indicator samples again at once.
                pane.note_interaction();
            }
            // The key window and the selected tab are part of the layout.
            self.layout_changed();
        }

        #[unsafe(method(windowDidResignKey:))]
        fn window_did_resign_key(&self, _n: &NSNotification) {
            // The ⌘-hovered link clears too: ⌘'s release may go to
            // another application. The key window also resigns key when the
            // application deactivates, so this one hook covers both.
            for pane in self.panes() {
                pane.apply_focus(false);
                pane.unhover_upload();
                pane.view().clear_link();
                // The scroll bar's strip is tracked in the key window only:
                // no exit would come, and the bar would stay wide.
                pane.view().release_scrollbar_hover();
            }
        }

        /// The red button and the tab bar's menu (Close Tab, Close Other
        /// Tabs): whether to ask before closing. `false` stops
        /// the closing; if a question was asked the closing is in its answer
        /// ([`TerminalWindow::ask`]). The main menu's ⌘W does not go through
        /// here (`closeTab:`).
        ///
        /// The shell's exit does **not** go through here: `close` does not ask
        /// the delegate.
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _sender: &NSWindow) -> bool {
            self.should_close_now()
        }

        /// The window (or tab) is closing: the red button, ⌘W, ⇧⌘W and the
        /// shell's exit (`ShellWake::child_exit` → `close`) arrive here.
        ///
        /// Closing is **not waited on**: it is started and the
        /// handle drops, the `"PTY teardown"` thread finishes its work in the
        /// background — closing a single tab must not stall the main thread for
        /// up to half a second. The order is in [`TerminalWindow::begin_close`].
        ///
        /// In a timed run this path does not run: the shell's exit goes to
        /// `terminate:` and the report must find the window in the list.
        ///
        /// **Removal from the list is deferred by one turn.** The `Retained`
        /// the list holds is this object's only strong reference (the window's
        /// delegate property is weak) and if it dropped here the object would
        /// be freed inside its own method, in the middle of AppKit's `-close` —
        /// and the `NSWindow`'s `Retained` with it. The deferred job carries the
        /// id (the alternate-screen notifier's pattern); the object still drops
        /// on the main thread.
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _n: &NSNotification) {
            // Closing the panes also removes the frame observers and drops the
            // panes from `AppDelegate::pane`'s lookup
            // (`TerminalPane::begin_close`).
            drop(self.begin_close());
            // Let go of the delegate now: AppKit must not send the closing
            // window notifications from here on (focus, occlusion), not even
            // until the object drops.
            self.ivars().window.setDelegate(None);
            // SAFETY: the registration was made in `observe_focus` for this
            // observer and this path; the window drops with this object and must
            // not drop while its observer is registered.
            unsafe {
                self.ivars()
                    .window
                    .removeObserver_forKeyPath(self, ns_string!("firstResponder"));
            }
            let id = self.ivars().id;
            DispatchQueue::main().exec_async(move || {
                // audit: a block running on the main queue is on the main thread by definition.
                let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
                if let Some(app) = app::delegate(mtm) {
                    app.forget_window(id);
                }
            });
        }
    }

    // **Actions that belong to the tab** are here (closing, tab selection,
    // split) and the split ones hand their work to the selected tab — the
    // tab object is not on the responder chain; the pane-level ones (point
    // size, find, clear, scroll, upload cancel) are in the pane, those that
    // spread application-wide (`settingsDidChange:`, theme, `openSettings:`)
    // in `AppDelegate`. The responder chain of a targetless action is view →
    // pane → container → window → **window delegate** → `NSApp` → app
    // delegate; so if this object implemented a spreading selector the key
    // window would swallow it and the other windows would never hear it.
    impl TerminalWindow {

        /// The window's first responder changed (`observe_focus`): if the
        /// keyboard moved to another pane the focus is that one's.
        /// `BateriView`'s own hook (`PaneHost::focused`) did not see a click on
        /// the search field — the keyboard goes to the other pane's field, and
        /// the veil and title would stay on the old pane. This is the single source: **every** path of a focus change
        /// (click, field, menu) goes through here.
        #[unsafe(method(observeValueForKeyPath:ofObject:change:context:))]
        fn observe_value(
            &self,
            _key_path: Option<&NSString>,
            _object: Option<&AnyObject>,
            _change: Option<&AnyObject>,
            _context: *mut c_void,
        ) {
            let tab = self.selected_tab();
            if let Some(pane) = tab.responder_pane() {
                tab.pane_focused(pane.id());
            }
        }

        /// ⌘W's title and the split's enabled state; **an unknown item is
        /// `true`**. With several panes ⌘W is "Close" (the focused pane), with
        /// one pane "Close Tab". A split is grey if one of the
        /// halves would drop below the smallest pane limit.
        #[unsafe(method(validateMenuItem:))]
        fn validate_menu_item(&self, item: &NSMenuItem) -> bool {
            let action = item.action();
            let tab = self.selected_tab();
            // No `return`: `define_class!` converts the `bool` at the end of the body.
            if action == Some(sel!(closeTab:)) {
                item.setTitle(&NSString::from_str(close_title(tab.panes().len())));
                true
            } else if action == Some(sel!(splitRight:)) {
                tab.can_split(Axis::Horizontal)
            } else if action == Some(sel!(splitDown:)) {
                tab.can_split(Axis::Vertical)
            } else if action == Some(sel!(toggleSplitZoom:)) {
                let zoomed = tab.zoomed().is_some();
                item.setState(if zoomed {
                    NSControlStateValueOn
                } else {
                    NSControlStateValueOff
                });
                tab.panes().len() > 1
            } else if [
                sel!(selectPreviousSplit:),
                sel!(selectNextSplit:),
                sel!(selectSplit:),
                sel!(resizeSplit:),
                sel!(equalizeSplits:),
            ]
            .into_iter()
            .any(|split| action == Some(split))
            {
                // With a single pane there is nothing to navigate or resize.
                tab.panes().len() > 1
            } else {
                true
            }
        }

        /// Window ▸ Select Previous Split (⌘[): the previous pane in tree
        /// order, cyclic.
        #[unsafe(method(selectPreviousSplit:))]
        fn select_previous_split(&self, _sender: Option<&AnyObject>) {
            self.selected_tab().select_split(false);
        }

        /// Window ▸ Select Next Split (⌘]).
        #[unsafe(method(selectNextSplit:))]
        fn select_next_split(&self, _sender: Option<&AnyObject>) {
            self.selected_tab().select_split(true);
        }

        /// Window ▸ Select Split ▸ (⌥⌘ + arrow): the item's `tag` is the
        /// direction ([`Direction::from_tag`]).
        #[unsafe(method(selectSplit:))]
        fn select_split_action(&self, sender: Option<&AnyObject>) {
            if let Some(direction) = direction_of(sender) {
                self.selected_tab().select_split_toward(direction);
            }
        }

        /// Window ▸ Resize Split ▸ (⌃⌘ + arrow).
        #[unsafe(method(resizeSplit:))]
        fn resize_split_action(&self, sender: Option<&AnyObject>) {
            if let Some(direction) = direction_of(sender) {
                self.selected_tab().resize_split(direction);
            }
        }

        /// Window ▸ Equalize Splits (⌃⌘=).
        #[unsafe(method(equalizeSplits:))]
        fn equalize_splits_action(&self, _sender: Option<&AnyObject>) {
            self.selected_tab().equalize_splits();
        }

        /// Window ▸ Zoom Split (⇧⌘↩).
        #[unsafe(method(toggleSplitZoom:))]
        fn toggle_split_zoom_action(&self, _sender: Option<&AnyObject>) {
            self.selected_tab().toggle_split_zoom();
        }

        /// Shell ▸ Split Right (⌘D): splits the focused pane in two, the new
        /// one on the right.
        #[unsafe(method(splitRight:))]
        fn split_right(&self, _sender: Option<&AnyObject>) {
            self.selected_tab().split(Axis::Horizontal);
        }

        /// Shell ▸ Split Down (⇧⌘D): splits the focused pane in two, the new
        /// one below.
        #[unsafe(method(splitDown:))]
        fn split_down(&self, _sender: Option<&AnyObject>) {
            self.selected_tab().split(Axis::Vertical);
        }

        /// Shell ▸ Close Tab (⌘W): with several panes the **focused pane**,
        /// with one pane **only this tab**; asking if needed.
        ///
        /// Not `performClose:`, because AppKit's interpretation of it is
        /// stateful (measured): when the red
        /// button's group close was stopped by a `windowShouldClose:` `false`,
        /// the next `performClose:` also sent `windowShouldClose:` to every
        /// tab of the group and ⌘W ended up asking about the window. Our own
        /// action knows the scope itself; the question and the closing take the
        /// same path as ⇧⌘W's.
        #[unsafe(method(closeTab:))]
        fn close_tab(&self, _sender: Option<&AnyObject>) {
            self.close_tab_asking();
        }

        /// Shell ▸ Close Window (⇧⌘W): the window **with all its tabs and
        /// panes**.
        ///
        /// A **single** question for the whole group and on confirm
        /// every tab closes with `close` — not `performClose:`, because that
        /// goes through each tab's `windowShouldClose:` and would produce a
        /// second question per tab. The closing itself is still every tab's
        /// `windowWillClose:`.
        #[unsafe(method(closeWindow:))]
        fn close_window(&self, _sender: Option<&AnyObject>) {
            self.close_group_asking();
        }

        /// Window ▸ Select Tab ▸ Tab n (⌘1…⌘8) and Last Tab (⌘9): the item's
        /// `tag` ([`crate::menu`]) lands on an index in the tab group
        /// ([`tab_index`]). A nonexistent tab is a no-op.
        #[unsafe(method(selectTab:))]
        fn select_tab(&self, sender: Option<&AnyObject>) {
            let Some(item) = sender.and_then(|sender| sender.downcast_ref::<NSMenuItem>()) else {
                return;
            };
            let Ok(tag) = u8::try_from(item.tag()) else {
                return;
            };
            let windows = self.tab_windows();
            if let Some(window) = tab_index(tag, windows.len()).map(|index| &windows[index]) {
                window.makeKeyAndOrderFront(None);
            }
        }
    }
);

/// A new window's first size, before the caller places it — also the first
/// frame of its tab's container and panes.
pub(crate) fn initial_rect() -> NSRect {
    NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(900.0, 600.0))
}

impl TerminalWindow {
    /// Builds the window, its tab and the tab's single pane (view, surface,
    /// renderer); the session and link are **not there yet**
    /// ([`TerminalWindow::start`]).
    ///
    /// The reason for two steps is the work in between: the settings must be
    /// read **after** the window is born (so a notice can be written to the
    /// subtitle) and **before** the geometry — the font setting determines the
    /// cell size, i.e. the first grid and the first `TIOCSWINSZ` the shell
    /// sees. A single constructor would either break that order or move the
    /// settings read inside the window.
    ///
    /// The renderer is born with the pane and its error returns to the
    /// caller: if the GPU device or the pipelines cannot be built the window
    /// has nothing to draw. `launch` is the first pane's birth package (its id
    /// from the same counter as the window's and the tab's, its owner tab
    /// `tab`'s [`tab::TabHost`]); the tab bears it as the container's single
    /// pane, splits come afterwards ([`TerminalTab::add_pane`]).
    pub(crate) fn new(
        mtm: MainThreadMarker,
        id: u64,
        tab: u64,
        launch: PaneLaunch,
    ) -> Result<Retained<Self>, GpuError> {
        let run = launch.run;
        let pane = TerminalPane::new(mtm, initial_rect(), launch)?;
        Ok(Self::with_pane(mtm, id, tab, run, &pane))
    }

    /// The window around a new tab `tab` and its first pane —
    /// [`TerminalWindow::new`]'s body and the start of
    /// [`TerminalWindow::restore`]: one constructor, so a restored window is
    /// the same window a new one is.
    fn with_pane(
        mtm: MainThreadMarker,
        id: u64,
        tab: u64,
        run: Option<Run>,
        pane: &TerminalPane,
    ) -> Retained<Self> {
        let rect = initial_rect();
        let tab = TerminalTab::new(mtm, tab, id, pane);
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable
            | NSWindowStyleMask::Resizable;
        // SAFETY: with defer=false the window is created immediately. The
        // constructor is unsafe because of `releasedWhenClosed`: without a
        // window controller AppKit releases the window on close and the
        // Retained in `WindowIvars.window` would dangle; we turn it off right below.
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                rect,
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        // SAFETY: only changes the ownership semantics; we are the Retained's owner.
        unsafe { window.setReleasedWhenClosed(false) };
        // Session restore is bateri's own file: were AppKit to
        // restore its own copy of the window one day, every window would come
        // back twice.
        window.setRestorable(false);
        // The content view is the tab's splits container; the window sets its
        // frame, the container lays the panes out (`SplitView::layout_panes`;
        // with a single pane the whole boundary).
        window.setContentView(Some(tab.container()));
        window.setTitle(ns_string!("bateri"));
        // **Native tabs**: AppKit gathers windows carrying the
        // same identifier into a single window as tabs. `tabbingMode` is
        // deliberately left at the default — respecting the system's "Prefer
        // tabs" setting. This is the only place the identifier is written, so
        // all windows share one identifier.
        window.setTabbingIdentifier(ns_string!("bateri.terminal"));
        // Mouse-moved events without a button are **off** by default; an
        // application asking for mouse reporting (1003) could never see the
        // pointer without them. They reach the first responder only, which is
        // all the report, the links and the upload buttons need (their hand
        // cursor comes from `NSView`'s own cursor rect,
        // `BateriView::hand_cursor_rects`). The one `NSTrackingArea` is the
        // scroll bar's strip (`BateriView::track_scrollbar_strip`): it must
        // see an unfocused pane and the pointer leaving, which this does
        // not. Turning these on and off by mode would want broadcasting the
        // mode to `bt-shell-macos`.
        window.setAcceptsMouseMovedEvents(true);
        // The keyboard's path to the PTY starts here. The view (even as the
        // pane's child) is NOT an automatic first responder; without this line
        // the window becomes key, keys never reach the view and the terminal
        // silently does not type. `acceptsFirstResponder` is also required, both together.
        let accepted = window.makeFirstResponder(Some(pane.view()));
        debug_assert!(accepted, "BateriView must be first responder");
        let this = Self::alloc(mtm).set_ivars(WindowIvars {
            id,
            run,
            window: window.clone(),
            tab,
            chrome: Cell::new(None),
            tab_mark: Cell::new(None),
            alert: RefCell::new(None),
            close_requested: Cell::new(false),
        });
        // SAFETY: NSObject's init takes no arguments and the ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        // The ivars are filled before the delegate is attached: a window
        // notification falling in between must not find the geometry empty and
        // draw with a stale size. The delegate property is weak; the owner is
        // `AppDelegate`'s window list.
        window.setDelegate(Some(ProtocolObject::from_ref(&*this)));
        // The content's size changes independently of the window too (the tab
        // bar); so the geometry comes from the view's own notification, its
        // observer the pane (`TerminalPane::observe_frame`). The constructor's
        // last step: the earlier steps' layout must not make the geometry be
        // built before the window is ready.
        pane.observe_frame();
        this.observe_focus();
        this
    }

    /// Session restore's **single** setup path for a window and its tab
    /// `tab`: every pane is born ([`tab::restored_panes`]), laid out in the
    /// saved `shape` with its ratios at once ([`TerminalTab::adopt`]), the
    /// window is placed by the caller (`place`: the list, the theme, the
    /// frame or the tab group — the application's business) and only
    /// **then** do the shells start ([`TerminalTab::start_restored`]), so
    /// each sees its final size in its first `TIOCSWINSZ` and the replayed
    /// history wraps once. The live handover hands file descriptors here
    /// instead of shells.
    ///
    /// `launches` is indexed by `saved`'s shape's leaves. If no shell starts
    /// the window closes and the error returns.
    pub(crate) fn restore(
        mtm: MainThreadMarker,
        id: u64,
        tab: u64,
        saved: &SavedTab,
        launches: Vec<PaneLaunch>,
        place: impl FnOnce(&Retained<Self>),
    ) -> Result<Retained<Self>, String> {
        let run = launches.first().and_then(|launch| launch.run);
        let (tree, panes) = tab::restored_panes(mtm, &saved.shape, launches)?;
        let ids: Vec<u64> = panes.iter().map(|pane| pane.id()).collect();
        let (first, extra) = panes
            .split_first()
            .ok_or_else(|| "a saved tab without panes".to_owned())?;
        let this = Self::with_pane(mtm, id, tab, run, first);
        let tab = this.selected_tab();
        tab.adopt(tree, extra);
        place(&this);
        if let Err(e) = tab.start_restored(mtm, &ids, saved.focused, saved.zoomed) {
            this.close();
            return Err(e);
        }
        this.refresh_title();
        tab.refresh_dim();
        Ok(this)
    }

    /// The `NSWindow` — session restore reads its frame and tab group.
    pub(crate) fn ns_window(&self) -> &NSWindow {
        &self.ivars().window
    }

    /// The tab group's windows in tab-bar order and its selected one; only
    /// this window if there is no group (the [`Self::tab_windows`] precedent).
    pub(crate) fn tab_group_windows(
        &self,
    ) -> (Vec<Retained<NSWindow>>, Option<Retained<NSWindow>>) {
        let window = &self.ivars().window;
        match window.tabGroup() {
            Some(group) => (group.windows().to_vec(), group.selectedWindow()),
            None => (vec![window.clone()], Some(window.clone())),
        }
    }

    /// Brings a restored window to the front at its saved `frame` (already
    /// clamped onto a visible screen by the caller).
    pub(crate) fn show_at(&self, frame: NSRect) {
        let window = &self.ivars().window;
        window.setFrame_display(frame, false);
        window.makeKeyAndOrderFront(None);
    }

    /// Makes the window its group's selected tab and key — the restored
    /// selection and key window.
    pub(crate) fn select(&self) {
        self.ivars().window.makeKeyAndOrderFront(None);
    }

    pub(crate) fn id(&self) -> u64 {
        self.ivars().id
    }

    /// The window's tabs, in order — today the one.
    pub(crate) fn tabs(&self) -> Vec<Retained<TerminalTab>> {
        vec![self.ivars().tab.clone()]
    }

    /// The tab on screen: the title, the dot, the menu's split actions and
    /// the inheritance of a new tab or window are its — today the window's
    /// one tab.
    pub(crate) fn selected_tab(&self) -> Retained<TerminalTab> {
        self.ivars().tab.clone()
    }

    /// Every tab's panes, tab by tab in tree order — the window-wide
    /// distributions (scale, key, the close question) reach them all.
    pub(crate) fn panes(&self) -> Vec<Retained<TerminalPane>> {
        self.tabs().iter().flat_map(|tab| tab.panes()).collect()
    }

    /// Watches the window's first responder (KVO; compatible since macOS
    /// 10.14). At the end of the constructor, after the first first responder
    /// is set; the registration is removed in `windowWillClose:`.
    fn observe_focus(&self) {
        // SAFETY: the observer is this class and it implements
        // `observeValueForKeyPath:…`; the context is null, this is the only
        // path watched. The registration is removed when the window closes.
        unsafe {
            self.ivars().window.addObserver_forKeyPath_options_context(
                self,
                ns_string!("firstResponder"),
                NSKeyValueObservingOptions::empty(),
                std::ptr::null_mut(),
            );
        }
    }

    /// A layout edge for the bound holder (`AppDelegate::layout_changed`).
    pub(crate) fn layout_changed(&self) {
        if let Some(app) = app::delegate(self.mtm()) {
            app.layout_changed();
        }
    }

    /// `bateri://tab/<id>`'s only effect:
    /// reopens it if miniaturized, makes it the selected tab and key, brings
    /// the application to the front and gives the keyboard to the id's pane
    /// (in `tab`). Sends no byte to the shell.
    ///
    /// `makeKeyAndOrderFront` makes the window in a tab group the selected tab
    /// (the precedent of `selectTab:`); on a miniaturized window it would only
    /// change the order and leave it in the Dock, so `deminiaturize` comes first.
    pub(crate) fn bring_to_front(&self, tab: &TerminalTab, pane: &TerminalPane) {
        let window = &self.ivars().window;
        if window.isMiniaturized() {
            window.deminiaturize(None);
        }
        window.makeKeyAndOrderFront(None);
        tab.focus_pane(pane);
        NSApplication::sharedApplication(self.mtm()).activate();
    }

    /// Whether this object's `NSWindow` is that one — the active window is
    /// looked up in the list this way from `NSApp.keyWindow` (`AppDelegate::key_window`).
    pub(crate) fn owns(&self, window: &NSWindow) -> bool {
        std::ptr::eq(&*self.ivars().window, window)
    }

    /// Closes the window (via the `windowWillClose:` path), **without
    /// asking**: the shell's exit and a confirmed close question.
    ///
    /// If there is an open question in this window it is dropped first, with
    /// a `Cancel` answer: the block waiting for the answer counts every answer
    /// other than "close" as a cancel. If the question was for other tabs
    /// ("Close Other Tabs" and the shell of the selected tab carrying the
    /// sheet exited) the gesture is dropped and those tabs stay open — a
    /// **known limit**, the wrong direction is the safe one: nothing closes
    /// without asking, the gesture can be repeated.
    pub(crate) fn close(&self) {
        let alert = self.ivars().alert.take();
        if let (Some(alert), Some(seat)) =
            (alert, sheets::seat(Asker::Window(&self.ivars().window)))
        {
            seat.end(&alert.window(), NSModalResponseCancel);
        }
        self.ivars().window.close();
    }

    /// Whether a close question is open in this window.
    pub(crate) fn asking(&self) -> bool {
        self.ivars().alert.borrow().is_some()
    }

    /// The tab group's terminal windows, in order; only this one if there is no group.
    fn tab_group(&self, app: &AppDelegate) -> Vec<Retained<TerminalWindow>> {
        self.tab_windows()
            .iter()
            .filter_map(|window| app.window_owning(window))
            .collect()
    }

    /// `windowShouldClose:`'s body (⌘W, Close Tab, the red button, "Close
    /// Other Tabs"): whether to close now.
    ///
    /// **The decision is not made in this call, but one turn later and for
    /// the whole gesture** ([`close_requested`]). Measured: in a multi-tab
    /// window the red button sends a
    /// `windowShouldClose:` to **every** tab of the group, "Close Other Tabs"
    /// to every other tab, both in the same event turn. A decision looking at
    /// a single tab would open a question tab by tab on the red button or ask
    /// the first tab "Close this tab?" and leave the rest; "one gesture, at
    /// most one question" holds only by seeing the gesture's scope.
    ///
    /// A timed run and `never` never ask: the answer is settled now, i.e.
    /// AppKit's own closing (`true`) — the only reason for deferring is the
    /// question. If a question is already open in the group a second request is not born.
    fn should_close_now(&self) -> bool {
        if self.ivars().run.is_some() {
            return true;
        }
        let Some(app) = app::delegate(self.mtm()) else {
            return true;
        };
        if app.settings().confirm_close == ConfirmClose::Never {
            return true;
        }
        let Some(group) = self.group_unless_asking(&app) else {
            return false;
        };
        // The gesture's first request sets up a single job at the end of the
        // turn; the later ones only set their flag and enter the same job's
        // scope. The job looks for the **flags**, not the requesting window:
        // if the first requester closed in the meantime (its shell exited in
        // the same turn) the other tabs' flags would stay set permanently and
        // the red button would never set up a job again.
        let first = !group.iter().any(|tab| tab.ivars().close_requested.get());
        self.ivars().close_requested.set(true);
        if first {
            DispatchQueue::main().exec_async(|| {
                // audit: a block running on the main queue is on the main thread by definition.
                let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
                if let Some(app) = app::delegate(mtm) {
                    close_requested_tabs(&app);
                }
            });
        }
        false
    }

    /// The group's tabs — if there is no open question in the group. The
    /// single copy of the "one gesture, at most one question" gate.
    fn group_unless_asking(&self, app: &AppDelegate) -> Option<Vec<Retained<TerminalWindow>>> {
        let group = self.tab_group(app);
        (!group.iter().any(|tab| tab.asking())).then_some(group)
    }

    /// A single decision for the group of this turn's requests; the flags are reset.
    fn close_requested_group(&self, app: &AppDelegate) {
        let group = self.tab_group(app);
        let mut targets = Vec::new();
        for tab in &group {
            if tab.ivars().close_requested.replace(false) {
                targets.push(tab.clone());
            }
        }
        if targets.is_empty() || group.iter().any(|tab| tab.asking()) {
            return;
        }
        let scope = close_scope(targets.len(), group.len());
        self.confirm_close(app, &group, &targets, scope);
    }

    /// ⌘W: with several panes a question for the focused pane; with one pane
    /// for this tab. Closes at once if it will not ask.
    fn close_tab_asking(&self) {
        let Some(app) = app::delegate(self.mtm()) else {
            return;
        };
        let Some(group) = self.group_unless_asking(&app) else {
            return;
        };
        if self.selected_tab().panes().len() > 1 {
            self.close_pane_asking(&app);
            return;
        }
        let Some(this) = app.window(self.id()) else {
            return;
        };
        let scope = close_scope(1, group.len());
        self.confirm_close(&app, &group, &[this], scope);
    }

    /// ⌘W in a multi-pane tab: only the focused pane, asking only about the
    /// running job if there is one.
    fn close_pane_asking(&self, app: &AppDelegate) {
        let tab = self.selected_tab();
        let pane = tab.focused_pane();
        let confirm = app.settings().confirm_close;
        let Some(foregrounds) = foregrounds_to_ask(
            self.ivars().run.is_some(),
            confirm,
            std::slice::from_ref(&pane),
        ) else {
            tab.close_pane(pane.id());
            return;
        };
        self.ask(
            &prompt(CloseScope::Pane, Unit::Pane, &foregrounds),
            CloseTarget::Pane {
                tab: tab.id(),
                pane: pane.id(),
            },
        );
    }

    /// ⇧⌘W: a single question for the whole group, or closes at once if it will not ask.
    fn close_group_asking(&self) {
        let Some(app) = app::delegate(self.mtm()) else {
            return;
        };
        if let Some(group) = self.group_unless_asking(&app) {
            self.confirm_close(&app, &group, &group, CloseScope::Window);
        }
    }

    /// `targets` will close; if it will ask the question opens as a sheet on
    /// the group's **selected** tab, otherwise they all close at once.
    ///
    /// The sheet is on the selected tab, because a sheet attached to a
    /// background tab is invisible — and in "Close Other Tabs" none of the
    /// tabs to be closed is selected. **If the single target is a background
    /// tab** (a background tab's × in the tab bar) that tab is selected first
    /// and the question is on it: "Close this tab?" must ask about the tab the
    /// eye is on, not another.
    fn confirm_close(
        &self,
        app: &AppDelegate,
        group: &[Retained<TerminalWindow>],
        targets: &[Retained<TerminalWindow>],
        scope: CloseScope,
    ) {
        let Some(first) = targets.first() else {
            return;
        };
        let panes: Vec<Retained<TerminalPane>> =
            targets.iter().flat_map(|tab| tab.panes()).collect();
        let unit = unit_for(panes.len(), targets.len());
        let confirm = app.settings().confirm_close;
        let Some(foregrounds) = foregrounds_to_ask(self.ivars().run.is_some(), confirm, &panes)
        else {
            targets.iter().for_each(|tab| tab.close());
            return;
        };
        let selected = self
            .ivars()
            .window
            .tabGroup()
            .and_then(|tab_group| tab_group.selectedWindow())
            .and_then(|window| app.window_owning(&window));
        let host = match (targets, selected.as_ref()) {
            ([only], Some(selected)) if only.id() != selected.id() => {
                only.ivars().window.makeKeyAndOrderFront(None);
                only
            }
            _ => selected
                .as_ref()
                .or_else(|| group.iter().find(|tab| tab.id() == self.id()))
                .unwrap_or(first),
        };
        let ids = targets.iter().map(|tab| tab.id()).collect();
        host.ask(&prompt(scope, unit, &foregrounds), CloseTarget::Tabs(ids));
    }

    /// Opens the question on this window as a sheet; on confirm closes the
    /// tabs in `targets` or the pane.
    ///
    /// **The block captures only ids** (the alternate-screen notifier's
    /// pattern): it looks the windows up in the list at answer time and skips
    /// those it cannot find. Only `NSAlertFirstButtonReturn` closes — if the
    /// shell exits while the sheet is open [`TerminalWindow::close`] drops the
    /// sheet with `Cancel`, and since `forget_window` is deferred by one turn
    /// the window can still be found in the list in the meantime.
    ///
    /// The closing is **deferred by one main-queue turn** (`windowWillClose:`'s
    /// pattern): the answer comes inside AppKit's sheet teardown and closing
    /// the window there would pull the rug from under the teardown.
    ///
    /// The question is the **window's** (its seat in [`crate::sheets`]): it
    /// asks about the window, its tabs or one of its panes.
    fn ask(&self, prompt: &Prompt, targets: CloseTarget) {
        let Some(seat) = sheets::seat(Asker::Window(&self.ivars().window)) else {
            return;
        };
        let alert = alert(self.mtm(), prompt);
        let host = self.id();
        let answered = RcBlock::new(move |response: NSModalResponse| {
            // audit: the sheet's completion block runs on AppKit's main thread.
            let mtm = MainThreadMarker::new().expect("the sheet block is on the main thread");
            if let Some(window) = app::delegate(mtm).and_then(|app| app.window(host)) {
                drop(window.ivars().alert.take());
            }
            if response != NSAlertFirstButtonReturn {
                return;
            }
            let targets = targets.clone();
            DispatchQueue::main().exec_async(move || {
                // audit: a block running on the main queue is on the main thread by definition.
                let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
                let Some(app) = app::delegate(mtm) else {
                    return;
                };
                match &targets {
                    CloseTarget::Tabs(ids) => {
                        for window in ids.iter().filter_map(|&id| app.window(id)) {
                            window.close();
                        }
                    }
                    // If the pane closed in the meantime (its shell exited) a no-op.
                    CloseTarget::Pane { tab, pane } => {
                        if let Some(tab) = app.tab(*tab) {
                            tab.close_pane(*pane);
                        }
                    }
                }
            });
        });
        self.ivars().alert.replace(Some(alert.clone()));
        seat.begin(&alert, &answered);
    }

    /// Adds the window to `from`'s tab group, to the **right** of the selected
    /// tab, and brings it to the front.
    pub(crate) fn show_as_tab_of(&self, from: &TerminalWindow) {
        from.ivars()
            .window
            .addTabbedWindow_ordered(&self.ivars().window, NSWindowOrderingMode::Above);
        self.ivars().window.makeKeyAndOrderFront(None);
    }

    /// Brings it to the front as a separate window; cascaded from `from` if
    /// there is one (top-left corner one step right-down), otherwise centred
    /// on the screen.
    pub(crate) fn show_after(&self, from: Option<&TerminalWindow>) {
        let window = &self.ivars().window;
        match from {
            // A call with `NSZeroPoint` does not move the window, it gives the
            // next window's corner — AppKit's cascading idiom.
            Some(from) => {
                let next = from.ivars().window.cascadeTopLeftFromPoint(NSPoint::ZERO);
                window.cascadeTopLeftFromPoint(next);
            }
            None => window.center(),
        }
        window.makeKeyAndOrderFront(None);
    }

    /// Timed run only (`make smoke`): keeps the window above every other
    /// app's windows, so it is never occluded.
    ///
    /// wgpu hands out no drawable for an occluded window (its Metal backend's
    /// fix for occluded-surface hangs) and the pane's gate stops drawing on the
    /// occlusion notification, so a smoke run started behind another app drew
    /// `kare=0`. Activating the app would not be enough — `activate()` is
    /// cooperative since macOS 14 and the frontmost app may keep the focus —
    /// and forcing it would steal the user's keyboard. A floating level
    /// changes only the stacking order, for the three seconds of the run;
    /// normal use never calls this.
    pub(crate) fn float_for_timed_run(&self) {
        let window = &self.ivars().window;
        window.setLevel(NSFloatingWindowLevel);
        window.orderFrontRegardless();
    }

    /// The windows in the window's tab group, in order; only itself if there
    /// is no group.
    ///
    /// `tabGroup`, not `tabbedWindows`: the former gives `nil` while the bar
    /// is not visible, so with a single tab ⇧⌘W would close nothing.
    fn tab_windows(&self) -> Vec<Retained<NSWindow>> {
        let window = &self.ivars().window;
        match window.tabGroup() {
            Some(group) => group.windows().to_vec(),
            None => vec![window.clone()],
        }
    }

    /// Reads the selected tab's title ([`TerminalTab::title`]: the
    /// **focused** pane's session) and writes it to the window — the pane's
    /// `PaneHost::title_changed` event ([`tab::TabHost`]) and the focus
    /// change ([`TerminalTab::pane_focused`]).
    /// The frame path computes no title; writing is only on **change**. If there is no session yet the title stays the constructor's
    /// `bateri`.
    ///
    /// The tab's dot is refreshed from here too ([`Self::refresh_tab_mark`]):
    /// the remote state's two edges (the return of `set_remote`, the
    /// `title_changed` that `D`/`A`'s deletion brings) are the same as the
    /// title's. The upload queue's connection edge is the
    /// pane's, **before** the event (`TerminalPane::remote_or_title_changed`).
    pub(crate) fn refresh_title(&self) {
        self.apply_title();
        self.refresh_tab_mark();
    }

    /// Writes the window's (and tab's) title from the selected tab.
    fn apply_title(&self) {
        if let Some(title) = self.selected_tab().title() {
            self.ivars().window.setTitle(&NSString::from_str(&title));
        }
    }

    /// The tab's dot: on a marked remote host a small filled
    /// circle in the mark's colour next to the tab title
    /// (`NSWindowTab.accessoryView`); none on an unmarked remote or locally —
    /// an unmarked remote tab already carries `⇄` in its title and a dot on
    /// every ssh tab would dilute prod's red.
    ///
    /// The colour is from the session's theme, from the same mapping as the
    /// dock's (`Theme::mark_rgb`), sRGB — `NSColor` encodes it itself. Its
    /// triggers are the remote state's edges ([`Self::refresh_title`]), the
    /// settings ([`Self::set_host_marks`]) and the theme ([`Self::set_theme`]);
    /// a no-op on the same colour, so a new view does not go to AppKit on
    /// every title news.
    /// The drawing is an `NSBox` (the precedent of the search panel): asking for
    /// a colour through the layer would want `CGColor`, i.e. the
    /// `objc2-core-graphics` edge. The dot exists only while the tab bar is
    /// visible; in a single-tab window the indicator is the dock's top line.
    fn refresh_tab_mark(&self) {
        let color = self.selected_tab().mark_rgb();
        if self.ivars().tab_mark.replace(color) == color {
            return;
        }
        let tab = self.ivars().window.tab();
        let Some(color) = color else {
            tab.setAccessoryView(None);
            return;
        };
        const DIAMETER: f64 = 8.0;
        let mtm = self.mtm();
        let dot = NSBox::new(mtm);
        dot.setBoxType(NSBoxType::Custom);
        dot.setTitlePosition(NSTitlePosition::NoTitle);
        dot.setBorderWidth(0.0);
        dot.setCornerRadius(DIAMETER / 2.0);
        let byte = |shift: u32| f64::from((color >> shift) & 0xff) / 255.0;
        dot.setFillColor(&NSColor::colorWithSRGBRed_green_blue_alpha(
            byte(16),
            byte(8),
            byte(0),
            1.0,
        ));
        // Auto Layout sizes the tab accessory: constrain the size.
        dot.setTranslatesAutoresizingMaskIntoConstraints(false);
        dot.widthAnchor()
            .constraintEqualToConstant(DIAMETER)
            .setActive(true);
        dot.heightAnchor()
            .constraintEqualToConstant(DIAMETER)
            .setActive(true);
        tab.setAccessoryView(Some(&dot));
    }

    /// Writing the subtitle; the text is built by `AppDelegate::post_notices`.
    pub(crate) fn set_subtitle(&self, subtitle: &NSString) {
        self.ivars().window.setSubtitle(subtitle);
    }

    /// Opens the first pane's session ([`TerminalTab::start`], from the
    /// birth package) and reads the title from the session once: a title
    /// notification that arrived before the session entered the slot may have
    /// found an empty slot and dropped; this read closes that (writes the same
    /// `bateri` if unchanged). The error returns to the caller: in the first
    /// window the process exits, in ⌘T/⌘N only that window closes.
    pub(crate) fn start(&self, mtm: MainThreadMarker) -> std::io::Result<()> {
        self.selected_tab().start(mtm)?;
        self.refresh_title();
        Ok(())
    }

    /// `[remote] hosts` changed — the pattern list goes to every tab's panes
    /// ([`TerminalTab::set_host_marks`]), the tab's dot from the new resolution.
    pub(crate) fn set_host_marks(&self, settings: &Settings) {
        for tab in self.tabs() {
            tab.set_host_marks(settings);
        }
        self.refresh_tab_mark();
    }

    /// Gives the theme to the tabs ([`TerminalTab::set_theme`]: the panes'
    /// sessions and search panels, the separator) and paints the chrome with
    /// it ([`TerminalWindow::apply_chrome`]).
    ///
    /// Both in a single call, because there are two paths that change the
    /// theme (`AppDelegate::reload_settings`, `AppDelegate::apply_appearance`)
    /// and if one forgot the chrome the grid would be in the new theme and the
    /// title bar in the old — the symptom is exactly the seam the user would see.
    pub(crate) fn set_theme(&self, theme: Theme) {
        for tab in self.tabs() {
            tab.set_theme(theme);
        }
        self.apply_chrome(&theme);
        // The tab's dot is from the mark's role; the role is another colour in the new theme.
        self.refresh_tab_mark();
    }

    /// Gives what the content does at the panes' top edge (`[appearance]
    /// content_edge`) to every tab ([`TerminalTab::set_content_edge`]: its
    /// panes and its container's line, in one call).
    pub(crate) fn set_content_edge(&self, edge: ContentEdge) {
        for tab in self.tabs() {
            tab.set_content_edge(edge);
        }
    }

    /// Paints the window chrome with the theme: the
    /// title bar transparent and separatorless, the window's background the
    /// theme's `background`, its appearance (traffic lights, title text, tab
    /// bar) from the background's lightness ([`is_dark_background`]).
    ///
    /// What shows under the transparent title bar is the window's background,
    /// so in a single tab the title and content are **a single surface**: the
    /// clear colour is from the same theme (`Theme::background_linear`). The
    /// colour is set here in **sRGB**, not linear — the linear value is
    /// `bt-gpu`'s, because the hardware encodes it to sRGB; giving `NSColor`
    /// a linear value would lighten the background.
    ///
    /// Setting an appearance on the window **detaches** it from the system's
    /// appearance: the view no longer sees the system's light/dark change and
    /// the appearance change is watched from the application itself
    /// (`AppDelegate::observe_appearance`).
    ///
    /// Called for the first time not in the constructor but right before the
    /// window is shown (`AppDelegate::open_window` → [`TerminalWindow::set_theme`]):
    /// the theme comes from there and painting afterwards would show the
    /// system's grey bar for a frame on every ⌘T.
    pub(crate) fn apply_chrome(&self, theme: &Theme) {
        // The chrome derives only from the background; giving AppKit the colour
        // and appearance again on the same background would redraw all title
        // bars on every settings save (the twin of `Session::set_theme` being a
        // no-op on the same theme).
        if self.ivars().chrome.replace(Some(theme.background)) == Some(theme.background) {
            return;
        }
        let window = &self.ivars().window;
        window.setTitlebarAppearsTransparent(true);
        window.setTitlebarSeparatorStyle(NSTitlebarSeparatorStyle::None);
        let [r, g, b] = theme.background_srgb().map(|byte| f64::from(byte) / 255.0);
        window.setBackgroundColor(Some(&NSColor::colorWithSRGBRed_green_blue_alpha(
            r, g, b, 1.0,
        )));
        // SAFETY: two constant `NSString`s AppKit exposes; they live for the
        // whole process and are only read (like `NSRunLoopCommonModes`).
        let name = unsafe {
            if is_dark_background(theme) {
                NSAppearanceNameDarkAqua
            } else {
                NSAppearanceNameAqua
            }
        };
        window.setAppearance(NSAppearance::appearanceNamed(name).as_deref());
    }

    /// The closing sequence's steps that fall to the window — **starts, does
    /// not wait**. It has two callers: the window's closing
    /// (`windowWillClose:`, the handles drop) and the application's closing
    /// (`AppDelegate::shutdown`, all handles waited on until a single
    /// deadline). The order is the pane's ([`TerminalPane::begin_close`]: the
    /// upload queue, rhythm, `Waker`, `SIGHUP`) and is for **every** pane of
    /// every tab ([`TerminalTab::begin_close`]); the return is one result per
    /// pane, tab by tab in tree order. Idempotent; the place of a pane whose
    /// session never came to be is `None`.
    pub(crate) fn begin_close(&self) -> Vec<Option<Closing>> {
        self.tabs()
            .iter()
            .flat_map(|tab| tab.begin_close())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn term_program_version_is_the_workspace_version() {
        // The guard: `bt-core`'s constant and the application's version
        // come from the same field (`version.workspace = true`); if one
        // diverges it turns red here.
        assert_eq!(bt_core::TERM_PROGRAM_VERSION, env!("CARGO_PKG_VERSION"));
    }

    use std::cell::Cell;

    use super::{
        CloseScope, Unit, close_scope, is_dark_background, kept_notice, prompt, should_ask,
        unit_for,
    };
    use crate::jobs::Foreground;
    use bt_core::{ConfirmClose, Theme};

    const ALL: [ConfirmClose; 3] = [
        ConfirmClose::Never,
        ConfirmClose::Running,
        ConfirmClose::Always,
    ];

    fn running(names: &[&str]) -> Foreground {
        Foreground::Running(names.iter().map(|&name| name.to_owned()).collect())
    }

    /// The quit's reminder names what keeps running: one program by name,
    /// several deduplicated across the panes, three at most and then a count;
    /// a nameless one is counted.
    #[test]
    fn the_kept_notice_names_the_running_programs() {
        let body = |tabs: &[Foreground]| kept_notice(tabs, false).map(|notice| notice.body);
        let tail = |pronoun: &str| {
            format!(
                "running in the background. Open bateri to return to {pronoun}; \u{2325}\u{2318}Q ends {pronoun}."
            )
        };
        assert_eq!(
            body(&[running(&["vim"]), Foreground::Idle]),
            Some(format!("\u{201c}vim\u{201d} keeps {}", tail("it")))
        );
        assert_eq!(
            body(&[running(&["vim"]), running(&["npm", "vim"])]),
            Some(format!(
                "\u{201c}vim\u{201d} and \u{201c}npm\u{201d} keep {}",
                tail("them")
            ))
        );
        assert_eq!(
            body(&[running(&["vim", "npm", "ssh", "htop", "less"])]),
            Some(format!(
                "\u{201c}vim\u{201d}, \u{201c}npm\u{201d}, \u{201c}ssh\u{201d} and 2 more keep {}",
                tail("them")
            ))
        );
        assert_eq!(
            body(&[running(&["vim"]), running(&[])]),
            Some(format!(
                "\u{201c}vim\u{201d} and 1 more keep {}",
                tail("them")
            ))
        );
        assert_eq!(
            body(&[running(&[])]),
            Some(format!("A program keeps {}", tail("it")))
        );
        assert_eq!(
            body(&[running(&[]), running(&[])]),
            Some(format!("Programs keep {}", tail("them")))
        );
        let notice = kept_notice(&[running(&["vim"])], false).expect("a program runs");
        assert_eq!(notice.title, "Programs keep running");
    }

    /// No program, no reminder: idle shells come back anyway; and none while
    /// the Mac logs out or restarts — the programs end with it.
    #[test]
    fn no_kept_notice_for_idle_shells_or_a_logout() {
        assert_eq!(kept_notice(&[], false), None);
        assert_eq!(
            kept_notice(&[Foreground::Idle, Foreground::Idle], false),
            None
        );
        assert_eq!(kept_notice(&[running(&["vim"])], true), None);
        assert_eq!(kept_notice(&[running(&[])], true), None);
    }

    fn with_background(background: u32) -> Theme {
        Theme {
            background,
            ..Theme::BATERI
        }
    }

    #[test]
    fn black_is_dark_and_white_is_light() {
        assert!(is_dark_background(&with_background(0x000000)));
        assert!(!is_dark_background(&with_background(0xffffff)));
    }

    #[test]
    fn embedded_themes_get_their_own_appearance() {
        assert!(is_dark_background(&Theme::BATERI), "bateri is dark");
        assert!(
            !is_dark_background(&Theme::BATERI_LIGHT),
            "bateri-light is light"
        );
    }

    #[test]
    fn mid_grey_splits_at_equal_contrast() {
        // The luminance at which contrast with white and black is equal is
        // √(1.05·0.05) − 0.05, falling between #757575 and #767676 in sRGB:
        // mid grey is light (black text reads better), a few shades darker is dark.
        assert!(!is_dark_background(&with_background(0x808080)));
        assert!(is_dark_background(&with_background(0x606060)));
    }

    #[test]
    fn a_timed_run_never_asks_and_never_reads_the_table() {
        // A timed run reads no settings and a headless question would hang
        // `make smoke`: the answer is no under every setting **and** the table
        // is never read.
        for confirm in ALL {
            for busy in [false, true] {
                let reads = Cell::new(0);
                let ask = should_ask(true, confirm, || {
                    reads.set(reads.get() + 1);
                    busy
                });
                assert!(!ask, "{confirm:?}");
                assert_eq!(reads.get(), 0, "{confirm:?}: the table was read");
            }
        }
    }

    #[test]
    fn never_and_always_decide_without_the_table() {
        for busy in [false, true] {
            let reads = Cell::new(0);
            let read = || {
                reads.set(reads.get() + 1);
                busy
            };
            assert!(!should_ask(false, ConfirmClose::Never, read));
            assert!(should_ask(false, ConfirmClose::Always, read));
            assert_eq!(reads.get(), 0, "read while the decision is table-free");
        }
    }

    #[test]
    fn running_asks_only_while_a_job_runs() {
        let reads = Cell::new(0);
        let ask = |busy| {
            should_ask(false, ConfirmClose::Running, || {
                reads.set(reads.get() + 1);
                busy
            })
        };
        assert!(ask(true));
        assert!(!ask(false));
        assert_eq!(reads.get(), 2);
    }

    #[test]
    fn titles_and_buttons_follow_the_scope() {
        let vim = [running(&["vim"])];
        let tab = prompt(CloseScope::Tab, Unit::Tab, &vim);
        assert_eq!(
            (tab.title.as_str(), tab.confirm),
            ("Close this tab?", "Close")
        );
        let tabs = prompt(CloseScope::Tabs(2), Unit::Tab, &vim);
        assert_eq!(
            (tabs.title.as_str(), tabs.confirm),
            ("Close 2 tabs?", "Close")
        );
        let window = prompt(CloseScope::Window, Unit::Tab, &vim);
        assert_eq!(
            (window.title.as_str(), window.confirm),
            ("Close this window?", "Close")
        );
        let quit = prompt(CloseScope::Quit, Unit::Tab, &vim);
        assert_eq!(
            (quit.title.as_str(), quit.confirm),
            ("Quit bateri?", "Quit")
        );
    }

    #[test]
    fn the_gesture_scope_comes_from_how_many_tabs_it_asked_for() {
        // The measured gestures: ⌘W a single tab,
        // the red button the whole group, "Close Other Tabs" the unselected ones.
        assert_eq!(close_scope(1, 1), CloseScope::Window, "single-tab window");
        assert_eq!(close_scope(1, 3), CloseScope::Tab, "⌘W");
        assert_eq!(close_scope(3, 3), CloseScope::Window, "red button");
        assert_eq!(close_scope(2, 3), CloseScope::Tabs(2), "Close Other Tabs");
    }

    #[test]
    fn one_tab_names_what_runs_in_it() {
        let message =
            |names: &[&str]| prompt(CloseScope::Tab, Unit::Tab, &[running(names)]).message;
        assert_eq!(
            message(&["claude"]),
            "“claude” is still running. Closing ends it."
        );
        assert_eq!(
            message(&["make", "cc"]),
            "“make” and “cc” are still running. Closing ends them."
        );
        assert_eq!(
            message(&["a", "b", "c"]),
            "“a”, “b” and “c” are still running. Closing ends them."
        );
        // The table could not be read but a job was counted as running: nameless.
        assert_eq!(message(&[]), "A process is still running. Closing ends it.");
    }

    #[test]
    fn only_the_running_tab_counts_in_a_group() {
        // In a three-tab window only one tab has a job: a name, not a count.
        let tabs = [Foreground::Idle, running(&["vim"]), Foreground::Idle];
        assert_eq!(
            prompt(CloseScope::Window, Unit::Tab, &tabs).message,
            "“vim” is still running. Closing ends it."
        );
    }

    #[test]
    fn many_tabs_are_counted_and_names_are_not_repeated() {
        let tabs = [
            running(&["claude"]),
            Foreground::Idle,
            running(&["vim"]),
            running(&["claude"]),
        ];
        assert_eq!(
            prompt(CloseScope::Window, Unit::Tab, &tabs).message,
            "Processes are running in 3 tabs: “claude”, “vim”. Closing ends them."
        );
        assert_eq!(
            prompt(CloseScope::Quit, Unit::Tab, &tabs).message,
            "Processes are running in 3 tabs: “claude”, “vim”. Quitting ends them."
        );
        // A count only if none of them has a readable name.
        assert_eq!(
            prompt(CloseScope::Quit, Unit::Tab, &[running(&[]), running(&[])]).message,
            "Processes are running in 2 tabs. Quitting ends them."
        );
    }

    #[test]
    fn always_says_what_closes_when_nothing_runs() {
        let message =
            |scope, tabs: usize| prompt(scope, Unit::Tab, &vec![Foreground::Idle; tabs]).message;
        assert_eq!(
            message(CloseScope::Tab, 1),
            "Closing this tab ends its shell session."
        );
        assert_eq!(
            message(CloseScope::Tabs(2), 2),
            "Closing these 2 tabs ends their shell sessions."
        );
        assert_eq!(
            message(CloseScope::Window, 1),
            "Closing this window ends its shell session."
        );
        assert_eq!(
            message(CloseScope::Window, 3),
            "Closing this window ends the shell sessions in its 3 tabs."
        );
        assert_eq!(
            message(CloseScope::Quit, 1),
            "Quitting ends the open shell session."
        );
        assert_eq!(
            message(CloseScope::Quit, 4),
            "Quitting ends 4 open shell sessions."
        );
    }

    #[test]
    fn one_pane_per_tab_keeps_the_tab_wording() {
        // With single-pane tabs the unit is the tab and the text is the same as
        // before splits; the pane once the pane count exceeds the tabs.
        assert_eq!(unit_for(1, 1), Unit::Tab);
        assert_eq!(unit_for(3, 3), Unit::Tab);
        assert_eq!(unit_for(2, 1), Unit::Pane);
        assert_eq!(unit_for(4, 3), Unit::Pane);
    }

    #[test]
    fn many_panes_are_counted_as_panes() {
        let panes = [running(&["vim"]), running(&["claude"]), Foreground::Idle];
        assert_eq!(
            prompt(CloseScope::Window, Unit::Pane, &panes).message,
            "Processes are running in 2 panes: “vim”, “claude”. Closing ends them."
        );
        assert_eq!(
            prompt(CloseScope::Window, Unit::Pane, &vec![Foreground::Idle; 3]).message,
            "Closing this window ends the shell sessions in its 3 panes."
        );
        // A single running job by name, independent of the unit.
        assert_eq!(
            prompt(
                CloseScope::Window,
                Unit::Pane,
                &[Foreground::Idle, running(&["vim"])]
            )
            .message,
            "“vim” is still running. Closing ends it."
        );
    }

    #[test]
    fn one_pane_asks_about_the_pane() {
        let pane = prompt(CloseScope::Pane, Unit::Pane, &[running(&["htop"])]);
        assert_eq!(
            (pane.title.as_str(), pane.confirm, pane.message.as_str()),
            (
                "Close this pane?",
                "Close",
                "“htop” is still running. Closing ends it."
            )
        );
        assert_eq!(
            prompt(CloseScope::Pane, Unit::Pane, &[Foreground::Idle]).message,
            "Closing this pane ends its shell session."
        );
    }

    /// The fallback's note sits on a line of its own under the history,
    /// dim, and resets what the history left on.
    #[test]
    fn the_fallback_note_has_a_line_of_its_own() {
        use super::{Note, fallen_back};
        for kind in [Note::Update, Note::Crash, Note::NotCarried, Note::Ended] {
            let note = format!("\x1b[0m\x1b[2m{}\x1b[0m\r\n", kind.text());
            assert_eq!(kind.line(), note.as_bytes());
            assert_eq!(fallen_back(None, kind), note.as_bytes());
            assert_eq!(fallen_back(Some(Vec::new()), kind), note.as_bytes());
            assert_eq!(
                fallen_back(Some(b"$ ls\r\n".to_vec()), kind),
                [&b"$ ls\r\n"[..], note.as_bytes()].concat()
            );
            assert_eq!(
                fallen_back(Some(b"\x1b[1m$ half".to_vec()), kind),
                [&b"\x1b[1m$ half\r\n"[..], note.as_bytes()].concat()
            );
        }
        // Every event says its own thing, and the update's is today's text.
        assert_eq!(
            Note::Update.text(),
            "bateri: the program running here did not survive the update; this is a new shell"
        );
        let texts = [
            Note::Update,
            Note::Crash,
            Note::NotCarried,
            Note::Ended,
            Note::Screenless,
            Note::Cut,
        ]
        .map(Note::text);
        for (index, text) in texts.iter().enumerate() {
            assert!(!texts[..index].contains(text), "{text}");
        }
    }
}
