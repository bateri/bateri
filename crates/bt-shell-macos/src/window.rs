//! Terminal window: an `NSWindow`, its splits container
//! (`split_view::SplitView`, the `contentView`) and its panes
//! (`pane::TerminalPane` — session, link, renderer, surface, `BateriView`,
//! search panel, upload queue), and everything that belongs to the **tab**:
//! chrome, title, tab dot, the close question, tab and split actions
//! (`closeTab:`, `closeWindow:`, `selectTab:`, `splitRight:`, `splitDown:`,
//! `selectPreviousSplit:`/`selectNextSplit:`, `selectSplit:`, `resizeSplit:`,
//! `equalizeSplits:`, `toggleSplitZoom:`); the window's `NSWindowDelegate` is
//! here too.
//!
//! **The focused pane** is the pane of the window's first responder
//! ([`TerminalWindow::focused_pane`]): the title, `⇄`, upload
//! percentage, tab dot and the inheritance of a new tab/split come from it.
//! ⌘W closes it, and in the last pane the tab. The other panes are
//! under the dim veil ([`TerminalWindow::refresh_dim`]). Split,
//! navigation, resizing, equalizing and pane closing drop the zoom (⇧⌘↩)
//! (resizing and equalizing because the user asked for a layout
//! change — silently changing a hidden layout would be an invisible effect).
//!
//! This is the pane's owner: the pane's events come through
//! [`WindowHost`] (`PaneHost`) and reach the window or the application, its
//! inputs from the `PaneLaunch` that `AppDelegate::open_window` builds. The
//! application-wide parts (settings, watching, subtitle slots, measurement
//! ledger, timed-run recipe, window list) are in `app`; the save-time paths
//! coming from there reach **every pane** (`TerminalWindow::panes`). The
//! window's geometry, occlusion and focus notifications are distributed to
//! **all** panes too. There is no drawing call here either; this file's job
//! is wiring.
//!
//! Renderer per pane (`pane`'s header).

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::path::PathBuf;
use std::time::Instant;

use block2::RcBlock;
use bt_core::{
    ConfirmClose, HostMark, InitialInput, Settings, ShutdownHandle, TabId, Teardown, Theme,
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
    NSView, NSWindow, NSWindowDelegate, NSWindowOcclusionState, NSWindowOrderingMode,
    NSWindowStyleMask,
};
use objc2_foundation::{
    NSKeyValueObservingOptions, NSNotification, NSObject, NSObjectNSKeyValueObserverRegistration,
    NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, ns_string,
};

use crate::Run;
use crate::app::{self, AppDelegate};
use crate::jobs::Foreground;
use crate::notices::Source;
use crate::pane::{PaneHost, PaneLaunch, TerminalPane};
use crate::restore::{SavedTab, Shape};
use crate::split::{Axis, Direction, Removal};
use crate::split_view::SplitView;
use crate::upload;
use crate::uploader;

/// Whether the theme's background is dark — the window chrome's appearance
/// (Aqua / DarkAqua) comes from this ([`TerminalWindow::apply_chrome`]).
///
/// The question is "which text reads better on this background: white or
/// black" and the answer is from WCAG's contrast ratio: if the background's
/// relative luminance (Rec. 709 coefficients, from the **linear** components)
/// gives a higher contrast with white, the background is dark. The threshold
/// is not invented, it arises from the equality of the two ratios; the
/// system's dark appearance means exactly "light text".
///
/// Not in `bt-core`'s `Theme` but here: lightness is not a theme role, it is
/// a translation into AppKit's appearance vocabulary.
pub(crate) fn is_dark_background(theme: &Theme) -> bool {
    let [r, g, b, _] = theme.background_linear().to_array();
    let luminance = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    // WCAG: ratio = (light + 0.05) / (dark + 0.05); white's luminance is 1.
    let against_white = 1.05 / (luminance + 0.05);
    let against_black = (luminance + 0.05) / 0.05;
    against_white > against_black
}

/// The owner handle the window gives the pane ([`PaneHost`]).
///
/// **It finds the window by id**, does not hold it by reference: the window
/// holds the pane strongly (`contentView` and ivar), a back reference would
/// be a cycle. The id is known before the window is born
/// (`AppDelegate::open_window`'s counter draws first), so the handle can go
/// into the birth package — there is no slot set up afterwards. If the window
/// left the list the event is dropped.
pub(crate) struct WindowHost {
    window: u64,
}

impl WindowHost {
    pub(crate) fn new(window: u64) -> Self {
        Self { window }
    }

    /// We are on the main thread: all of `PaneHost`'s calls come from the
    /// pane, on the main thread.
    fn mtm() -> MainThreadMarker {
        // audit: `PaneHost` is called only on the main thread (the trait's doc).
        MainThreadMarker::new().expect("PaneHost is called on the main thread")
    }

    fn window(&self) -> Option<Retained<TerminalWindow>> {
        app::delegate(Self::mtm())?.window(self.window)
    }
}

impl PaneHost for WindowHost {
    fn title_changed(&self, _pane: u64) {
        // The title is from the focused pane; a background pane's news does
        // the same read and rewrites the unchanged title — cheap and branchless.
        if let Some(window) = self.window() {
            window.refresh_title();
            // The news also says a directory moved: a layout edge.
            window.layout_changed();
        }
    }

    fn shell_exited(&self, pane: u64) {
        // Only that pane; the last pane closes the tab.
        if let Some(window) = self.window() {
            window.close_pane(pane);
        }
    }

    fn focused(&self, pane: u64) {
        if let Some(window) = self.window() {
            window.pane_focused(pane);
        }
    }

    fn uploads_changed(&self, _pane: u64) {
        if let Some(app) = app::delegate(Self::mtm()) {
            app.refresh_dock_tile();
        }
    }

    fn notify(&self, _pane: u64, title: &str, body: &str) {
        uploader::notify(Self::mtm(), title, body);
    }

    fn post_notices(&self, _pane: u64, source: Source, messages: Vec<String>) {
        if let Some(app) = app::delegate(Self::mtm()) {
            app.post_notices(source, messages);
        }
    }
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

/// The Select Tab ▸ item's `tag` + tab count → index of the tab to select.
///
/// ⌘1…⌘8 is the nth tab, `None` if absent (no-op); ⌘9 is the **last** tab —
/// the shared rule of Safari, Terminal.app and browsers: with more than nine
/// tabs too the last one is reached with a single key. Pure, tested.
pub(crate) fn tab_index(tag: u8, count: usize) -> Option<usize> {
    match tag {
        1..=8 => Some(usize::from(tag) - 1).filter(|&index| index < count),
        9 => count.checked_sub(1),
        _ => None,
    }
}

/// What the confirmed question will close ([`TerminalWindow::ask`]): ids,
/// looked up again at answer time.
#[derive(Clone, Debug)]
enum CloseTarget {
    /// Tabs (window ids), with all their panes.
    Tabs(Vec<u64>),
    /// A single pane (pane id); the tab stays open.
    Pane(u64),
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

/// The pane of `view` or one of its ancestors — from the first responder to
/// the focused pane (`BateriView`, the search field's field editor).
fn pane_containing(view: Retained<NSView>) -> Option<Retained<TerminalPane>> {
    let mut current = Some(view);
    while let Some(view) = current {
        match view.downcast::<TerminalPane>() {
            Ok(pane) => return Some(pane),
            // SAFETY: reading the parent view; we are on the main thread (`MainThreadOnly`).
            Err(view) => current = unsafe { view.superview() },
        }
    }
    None
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

/// The window's state — what belongs to the **tab**: chrome, tab dot, the
/// close question and focus. The session's core (session, link, renderer,
/// surface, view, dock reserve, point size, identity, search, upload) is in
/// the panes ([`TerminalPane`]), the panes and the split tree
/// in the container ([`SplitView`]).
pub(crate) struct WindowIvars {
    /// Our own counter ([`AppDelegate`] hands it out): the key by which the
    /// close question and removal from the list find the window. The pane's
    /// id is separate ([`TerminalPane::id`]) and from the same counter.
    id: u64,
    /// The timed run's recipe, a copy of `AppDelegate`'s (`Copy`): the close
    /// question is never asked in a timed run and must be answerable without
    /// reaching the application delegate ([`TerminalWindow::should_close_now`]).
    run: Option<Run>,
    window: Retained<NSWindow>,
    /// The splits container and the `contentView`: `NSWindow` already holds it
    /// strongly, this copy is for typed access ([`TerminalWindow::panes`]).
    container: Retained<SplitView>,
    /// The id of the last focused pane — the answer of focus when the first
    /// responder is not inside a pane (the window itself)
    /// ([`TerminalWindow::focused_pane`]). Written by the pane's
    /// `PaneHost::focused` event when `BateriView` becomes first responder.
    focused: Cell<u64>,
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
            self.ivars().container.layout_panes();
            for pane in self.panes() {
                pane.refresh_geometry();
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
            self.ivars().container.apply_visibility(visible);
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
    // split); the pane-level ones (point size, find, clear, scroll, upload
    // cancel) are in the pane, those that spread application-wide
    // (`settingsDidChange:`, theme, `openSettings:`) in `AppDelegate`.
    // The responder chain of a targetless action is view → pane → container →
    // window → **window delegate** → `NSApp` → app delegate; so if this object
    // implemented a spreading selector the key window would swallow it and
    // the other windows would never hear it.
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
            if let Some(pane) = self.responder_pane() {
                self.pane_focused(pane.id());
            }
        }

        /// ⌘W's title and the split's enabled state; **an unknown item is
        /// `true`**. With several panes ⌘W is "Close" (the focused pane), with
        /// one pane "Close Tab". A split is grey if one of the
        /// halves would drop below the smallest pane limit.
        #[unsafe(method(validateMenuItem:))]
        fn validate_menu_item(&self, item: &NSMenuItem) -> bool {
            let action = item.action();
            // No `return`: `define_class!` converts the `bool` at the end of the body.
            if action == Some(sel!(closeTab:)) {
                item.setTitle(&NSString::from_str(close_title(self.panes().len())));
                true
            } else if action == Some(sel!(splitRight:)) {
                self.can_split(Axis::Horizontal)
            } else if action == Some(sel!(splitDown:)) {
                self.can_split(Axis::Vertical)
            } else if action == Some(sel!(toggleSplitZoom:)) {
                let zoomed = self.ivars().container.zoomed().is_some();
                item.setState(if zoomed {
                    NSControlStateValueOn
                } else {
                    NSControlStateValueOff
                });
                self.panes().len() > 1
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
                self.panes().len() > 1
            } else {
                true
            }
        }

        /// Window ▸ Select Previous Split (⌘[): the previous pane in tree
        /// order, cyclic.
        #[unsafe(method(selectPreviousSplit:))]
        fn select_previous_split(&self, _sender: Option<&AnyObject>) {
            self.select_split(false);
        }

        /// Window ▸ Select Next Split (⌘]).
        #[unsafe(method(selectNextSplit:))]
        fn select_next_split(&self, _sender: Option<&AnyObject>) {
            self.select_split(true);
        }

        /// Window ▸ Select Split ▸ (⌥⌘ + arrow): the item's `tag` is the
        /// direction ([`Direction::from_tag`]).
        #[unsafe(method(selectSplit:))]
        fn select_split_action(&self, sender: Option<&AnyObject>) {
            if let Some(direction) = direction_of(sender) {
                self.select_split_toward(direction);
            }
        }

        /// Window ▸ Resize Split ▸ (⌃⌘ + arrow).
        #[unsafe(method(resizeSplit:))]
        fn resize_split_action(&self, sender: Option<&AnyObject>) {
            if let Some(direction) = direction_of(sender) {
                self.resize_split(direction);
            }
        }

        /// Window ▸ Equalize Splits (⌃⌘=).
        #[unsafe(method(equalizeSplits:))]
        fn equalize_splits_action(&self, _sender: Option<&AnyObject>) {
            self.equalize_splits();
        }

        /// Window ▸ Zoom Split (⇧⌘↩).
        #[unsafe(method(toggleSplitZoom:))]
        fn toggle_split_zoom_action(&self, _sender: Option<&AnyObject>) {
            self.toggle_split_zoom();
        }

        /// Shell ▸ Split Right (⌘D): splits the focused pane in two, the new
        /// one on the right.
        #[unsafe(method(splitRight:))]
        fn split_right(&self, _sender: Option<&AnyObject>) {
            self.split(Axis::Horizontal);
        }

        /// Shell ▸ Split Down (⇧⌘D): splits the focused pane in two, the new
        /// one below.
        #[unsafe(method(splitDown:))]
        fn split_down(&self, _sender: Option<&AnyObject>) {
            self.split(Axis::Vertical);
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

/// Saved scrollback, `(tab id, VT bytes)` per pane (`restore::save`'s input).
pub(crate) type Histories = Vec<(TabId, Vec<u8>)>;

/// A new window's first size, before the caller places it.
fn initial_rect() -> NSRect {
    NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(900.0, 600.0))
}

impl TerminalWindow {
    /// Builds the window and its single pane (view, surface, renderer); the
    /// session and link are **not there yet** ([`TerminalWindow::start`]).
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
    /// from the same counter as the window's id, its owner this window's
    /// [`WindowHost`]); the window bears it as the container's single pane,
    /// splits come afterwards ([`TerminalWindow::add_pane`]).
    pub(crate) fn new(
        mtm: MainThreadMarker,
        id: u64,
        launch: PaneLaunch,
    ) -> Result<Retained<Self>, GpuError> {
        let run = launch.run;
        let pane = TerminalPane::new(mtm, initial_rect(), launch)?;
        Ok(Self::with_pane(mtm, id, run, &pane))
    }

    /// The window around its first pane — [`TerminalWindow::new`]'s body and
    /// the start of [`TerminalWindow::restore`]: one constructor, so a
    /// restored window is the same window a new one is.
    fn with_pane(
        mtm: MainThreadMarker,
        id: u64,
        run: Option<Run>,
        pane: &TerminalPane,
    ) -> Retained<Self> {
        let rect = initial_rect();
        let container = SplitView::new(mtm, rect, pane);
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
        // The content view is the splits container; the window sets its frame,
        // the container lays the panes out (`SplitView::layout_panes`; with a
        // single pane the whole boundary).
        window.setContentView(Some(&container));
        window.setTitle(ns_string!("bateri"));
        // **Native tabs**: AppKit gathers windows carrying the
        // same identifier into a single window as tabs. `tabbingMode` is
        // deliberately left at the default — respecting the system's "Prefer
        // tabs" setting. This is the only place the identifier is written, so
        // all windows share one identifier.
        window.setTabbingIdentifier(ns_string!("bateri.terminal"));
        // Mouse-moved events without a button are **off** by default; an
        // application asking for mouse reporting (1003) could never see the
        // pointer without them. No `NSTrackingArea` is needed:
        // `mouseEntered:`/`mouseExited:` are not wanted, the upload buttons'
        // hand cursor comes from `NSView`'s own cursor rect
        // (`BateriView::hand_cursor_rects`), and the view is already first
        // responder — the window-level `mouseMoved:` reaches it. Turning them
        // on and off by mode would want broadcasting the mode to
        // `bt-shell-macos`.
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
            container: container.clone(),
            focused: Cell::new(pane.id()),
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

    /// Session restore's **single** setup path for a tab: every
    /// pane is born, laid out in the saved `shape` with its ratios at once
    /// ([`SplitView::adopt`]), the window is placed by the caller (`place`:
    /// the list, the theme, the frame or the tab group — the application's
    /// business) and only **then** do the shells start, so each sees its
    /// final size in its first `TIOCSWINSZ` and the replayed history wraps
    /// once. The live handover hands file descriptors here instead of
    /// shells.
    ///
    /// `launches` is indexed by `shape`'s leaves. A tree that does not fit
    /// the panes' smallest size on this screen is equalized; the zoom comes
    /// before the focus, so a focus on another pane drops the zoom (the
    /// keyboard is never given to a hidden pane, [`Self::focus_pane`]). A pane
    /// whose shell cannot start leaves the tree (the precedent of
    /// [`Self::add_pane`]); if none starts the window closes and the error
    /// returns.
    pub(crate) fn restore(
        mtm: MainThreadMarker,
        id: u64,
        shape: &Shape,
        launches: Vec<PaneLaunch>,
        focused: usize,
        zoomed: Option<usize>,
        place: impl FnOnce(&Retained<Self>),
    ) -> Result<Retained<Self>, String> {
        let ids: Vec<u64> = launches.iter().map(|launch| launch.id).collect();
        let tree = shape
            .to_tree(&ids)
            .filter(|tree| {
                let mut leaves = tree.leaves();
                leaves.sort_unstable();
                let mut wanted = ids.clone();
                wanted.sort_unstable();
                leaves == wanted
            })
            .ok_or_else(|| "the saved split tree does not match its panes".to_owned())?;
        let run = launches.first().and_then(|launch| launch.run);
        let rect = initial_rect();
        let panes = launches
            .into_iter()
            .map(|launch| TerminalPane::new(mtm, rect, launch).map_err(|e| e.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        let (first, extra) = panes
            .split_first()
            .ok_or_else(|| "a saved tab without panes".to_owned())?;
        let this = Self::with_pane(mtm, id, run, first);
        let container = this.ivars().container.clone();
        // Cannot fail: the leaves were matched against the panes above.
        let adopted = container.adopt(tree, extra);
        debug_assert!(adopted, "the checked tree must be adopted");
        for pane in extra {
            pane.observe_frame();
        }
        place(&this);
        if !container.fits() {
            container.equalize();
        }
        // The zoom after the plain layout: the hidden panes keep real frames.
        let zoomed = zoomed.and_then(|index| ids.get(index).copied());
        if panes.len() > 1 {
            container.set_zoomed(zoomed);
        }
        for pane in this.panes() {
            if let Err(e) = pane.start(mtm) {
                eprintln!("bateri: could not start a restored pane's shell: {e}");
                if let Removal::Last = container.remove_leaf(pane.id()) {
                    this.close();
                    return Err(format!("could not start the shell: {e}"));
                }
                drop(pane.begin_close());
                drop(container.detach(pane.id()));
            }
        }
        if container
            .zoomed()
            .is_some_and(|zoomed| container.pane(zoomed).is_none())
        {
            container.set_zoomed(None);
        }
        let focus = ids
            .get(focused)
            .and_then(|id| container.pane(*id))
            .or_else(|| this.panes().into_iter().next());
        if let Some(focus) = focus {
            this.focus_pane(&focus);
        }
        // The links were born after the zoom hid its panes; they learn it now.
        let visible = this
            .ivars()
            .window
            .occlusionState()
            .contains(NSWindowOcclusionState::Visible);
        container.apply_visibility(visible);
        this.refresh_title();
        this.refresh_dim();
        Ok(this)
    }

    /// What session restore saves of this tab and, with
    /// `with_history`, its panes' scrollback (`(tab id, bytes)`). `None` if a
    /// pane has nothing live to save ([`TerminalPane::saved`]): a tab whose
    /// tree would not match its panes is not saved at all.
    pub(crate) fn saved_tab(&self, with_history: bool) -> Option<(SavedTab, Histories)> {
        let panes = self.panes();
        let mut saved = Vec::with_capacity(panes.len());
        let mut histories = Vec::new();
        for pane in &panes {
            let (entry, history) = pane.saved(with_history)?;
            if let Some(history) = history {
                histories.push((entry.tab_id.clone(), history));
            }
            saved.push(entry);
        }
        let ids: Vec<u64> = panes.iter().map(|pane| pane.id()).collect();
        let shape = Shape::from_tree(&self.ivars().container.tree(), &ids)?;
        let position = |id: u64| ids.iter().position(|candidate| *candidate == id);
        let tab = SavedTab {
            shape,
            panes: saved,
            focused: position(self.focused_pane().id()).unwrap_or(0),
            zoomed: self.ivars().container.zoomed().and_then(position),
        };
        Some((tab, histories))
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

    /// The tab's panes, in tree order (left to right, top to bottom). Never
    /// empty: when the last pane closes the tab closes. A single pane in a
    /// timed run.
    pub(crate) fn panes(&self) -> Vec<Retained<TerminalPane>> {
        self.ivars().container.panes()
    }

    /// The focused pane: the pane of the window's first
    /// responder — `BateriView` or the search field's field editor, both
    /// descendants of the pane. If the first responder is not inside a pane
    /// (the window itself) the last focused pane ([`WindowIvars::focused`]),
    /// and if there is none the first pane.
    pub(crate) fn focused_pane(&self) -> Retained<TerminalPane> {
        let panes = self.panes();
        let focused = self.ivars().focused.get();
        self.responder_pane()
            .or_else(|| panes.iter().find(|pane| pane.id() == focused).cloned())
            .or_else(|| panes.first().cloned())
            // audit: the container never empties (`SplitIvars::panes`): closing
            // the last pane closes the tab and the window constructor is born
            // with a pane.
            .expect("the tab has at least one pane")
    }

    /// The first responder's pane — if it is one of this tab's panes
    /// (`BateriView` or the search field's field editor); otherwise `None`.
    fn responder_pane(&self) -> Option<Retained<TerminalPane>> {
        let pane = self
            .ivars()
            .window
            .firstResponder()
            .and_then(|responder| responder.downcast::<NSView>().ok())
            .and_then(pane_containing)?;
        self.panes()
            .into_iter()
            .find(|candidate| candidate.id() == pane.id())
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

    /// The pane's `BateriView` became first responder (`PaneHost::focused`):
    /// the focus moved to it, the title and tab dot are its.
    ///
    /// The title is read **one main-queue turn later**: the event comes from
    /// inside `becomeFirstResponder`, the window's `firstResponder` may still
    /// be the old view at that moment and [`Self::focused_pane`] asks it
    /// before the ivar — the title would be written from the old pane. The
    /// job captures the id (the pattern of `windowWillClose:`).
    pub(crate) fn pane_focused(&self, id: u64) {
        if self.ivars().focused.replace(id) == id {
            return;
        }
        self.layout_changed();
        let window = self.id();
        DispatchQueue::main().exec_async(move || {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(window) = app::delegate(mtm).and_then(|app| app.window(window)) {
                window.refresh_title();
                window.refresh_dim();
            }
        });
    }

    /// Gives the keyboard to `pane` (first responder) and moves the focus to
    /// it. If another pane is zoomed the zoom is dropped first: the keyboard
    /// is never given to a hidden pane (navigation, the closing's neighbour,
    /// `bateri://tab/`).
    fn focus_pane(&self, pane: &TerminalPane) {
        if self
            .ivars()
            .container
            .zoomed()
            .is_some_and(|zoomed| zoomed != pane.id())
        {
            self.set_zoom(None);
        }
        let _ = self.ivars().window.makeFirstResponder(Some(pane.view()));
        self.pane_focused(pane.id());
        self.refresh_dim();
    }

    /// The dim veil of unfocused panes: with several panes
    /// in the window, those other than the focused one. No veil with a single
    /// pane. AppKit's work, it asks for no frame.
    pub(crate) fn refresh_dim(&self) {
        let panes = self.panes();
        let focused = self.focused_pane().id();
        let many = panes.len() > 1;
        for pane in &panes {
            pane.set_dimmed(many && pane.id() != focused);
        }
    }

    /// Sets or drops the zoom; the links of hidden panes sleep, those of
    /// returning ones ask for a frame (if the window is visible).
    fn set_zoom(&self, zoomed: Option<u64>) {
        let container = &self.ivars().container;
        if container.zoomed() == zoomed {
            return;
        }
        container.set_zoomed(zoomed);
        let visible = self
            .ivars()
            .window
            .occlusionState()
            .contains(NSWindowOcclusionState::Visible);
        container.apply_visibility(visible);
        self.refresh_dim();
    }

    /// ⌘] / ⌘[: the next or previous pane in tree order.
    pub(crate) fn select_split(&self, forward: bool) {
        let from = self.focused_pane();
        if let Some(next) = self
            .ivars()
            .container
            .cycle(from.id(), forward)
            .and_then(|id| self.ivars().container.pane(id))
        {
            self.focus_pane(&next);
        }
    }

    /// ⌥⌘ + arrow: the pane in that direction; a no-op at the edge.
    /// The neighbour is from the layout without zoom — the panes' real places
    /// even while zoomed.
    pub(crate) fn select_split_toward(&self, direction: Direction) {
        let from = self.focused_pane();
        if let Some(next) = self
            .ivars()
            .container
            .neighbour(from.id(), direction)
            .and_then(|id| self.ivars().container.pane(id))
        {
            self.focus_pane(&next);
        }
    }

    /// ⌃⌘ + arrow: moves the focused pane's divider on that axis by one cell.
    /// The step is the focused pane's **one cell** — every press
    /// changes the grid by one column or row; a design decision, the number
    /// from the font. Stops at the smallest pane limit.
    pub(crate) fn resize_split(&self, direction: Direction) {
        self.set_zoom(None);
        let pane = self.focused_pane();
        let Some(cell) = pane.cell_size() else {
            return;
        };
        let step = match direction {
            Direction::Left | Direction::Right => cell.width,
            Direction::Up | Direction::Down => cell.height,
        };
        self.ivars().container.resize(pane.id(), direction, step);
        self.layout_changed();
    }

    /// ⌃⌘=: the panes on the same axis are equal.
    pub(crate) fn equalize_splits(&self) {
        self.set_zoom(None);
        self.ivars().container.equalize();
        self.layout_changed();
    }

    /// ⇧⌘↩: zooms the focused pane or undoes the zoom. A no-op
    /// with a single pane.
    pub(crate) fn toggle_split_zoom(&self) {
        if self.panes().len() < 2 {
            return;
        }
        let zoomed = match self.ivars().container.zoomed() {
            Some(_) => None,
            None => Some(self.focused_pane().id()),
        };
        self.set_zoom(zoomed);
        self.layout_changed();
    }

    /// A layout edge for the bound holder (`AppDelegate::layout_changed`).
    pub(crate) fn layout_changed(&self) {
        if let Some(app) = app::delegate(self.mtm()) {
            app.layout_changed();
        }
    }

    /// Whether the focused pane can be split on `axis`: both
    /// halves' grids must pass the smallest pane limit
    /// ([`TerminalPane::grid_fits`]). Since the new pane inherits the focused
    /// one's point-size delta the measure is from the focused one's cell.
    fn can_split(&self, axis: Axis) -> bool {
        let pane = self.focused_pane();
        self.ivars()
            .container
            .halves(pane.id(), axis)
            .is_some_and(|(first, second)| pane.grid_fits(first) && pane.grid_fits(second))
    }

    /// ⌘D / ⇧⌘D: a new split from the focused pane — the application builds
    /// the birth package (`AppDelegate::open_split`: directory, point-size
    /// delta, theme and remote line from the focused one). A
    /// no-op if it would drop below the limit.
    fn split(&self, axis: Axis) {
        // The zoom is dropped first: the split's limit is asked from
        // the layout without zoom (`can_split`) and the new pane must be visible.
        self.set_zoom(None);
        if !self.can_split(axis) {
            return;
        }
        let Some(app) = app::delegate(self.mtm()) else {
            return;
        };
        let from = self.focused_pane();
        if let Some(this) = app.window(self.id()) {
            app.open_split(&this, &from, axis);
        }
    }

    /// Puts the new pane in `target`'s second half on `axis`, opens its
    /// session and gives it the keyboard. The order is required: the pane's
    /// `start` reads the scale from the window, so it is first attached to
    /// the container; the frame observer after layout. If the session cannot
    /// be born the pane is taken apart again — no sessionless leaf remains —
    /// and the error goes to the caller.
    pub(crate) fn add_pane(
        &self,
        mtm: MainThreadMarker,
        launch: PaneLaunch,
        target: u64,
        axis: Axis,
    ) -> Result<(), String> {
        let container = &self.ivars().container;
        let (_, half) = container
            .halves(target, axis)
            .ok_or_else(|| "no pane to split".to_owned())?;
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), half);
        let pane = TerminalPane::new(mtm, frame, launch).map_err(|e| e.to_string())?;
        if !container.insert(target, axis, &pane) {
            return Err("no pane to split".to_owned());
        }
        pane.observe_frame();
        if let Err(e) = pane.start(mtm) {
            drop(pane.begin_close());
            let _ = container.remove_leaf(pane.id());
            drop(container.detach(pane.id()));
            return Err(format!("could not start the shell: {e}"));
        }
        self.focus_pane(&pane);
        Ok(())
    }

    /// Closes only the pane `id`, **without asking** (the shell's exit, a
    /// confirmed question). If it is the last pane, the tab's closing
    /// ([`TerminalWindow::close`]).
    ///
    /// If the focused pane is closing the focus goes to the neighbour in the
    /// tree ([`Removal::Removed`]) and **before the teardown**: taking apart
    /// the view that carries the first responder would leave the window
    /// without a responder. The closing is the pane's own sequence
    /// ([`TerminalPane::begin_close`], not waited on); the Dock icon's total
    /// again, because the closed pane's queue is gone.
    pub(crate) fn close_pane(&self, id: u64) {
        let container = &self.ivars().container;
        let Some(pane) = container.pane(id) else {
            return;
        };
        let was_focused = self.focused_pane().id() == id;
        match container.remove_leaf(id) {
            Removal::Missing => {}
            Removal::Last => self.close(),
            Removal::Removed { focus } => {
                self.set_zoom(None);
                if was_focused && let Some(next) = container.pane(focus) {
                    self.focus_pane(&next);
                }
                drop(pane.begin_close());
                drop(container.detach(id));
                self.refresh_title();
                self.refresh_dim();
                if let Some(app) = app::delegate(self.mtm()) {
                    app.refresh_dock_tile();
                    app.layout_changed();
                }
            }
        }
    }

    /// `bateri://tab/<id>`'s only effect:
    /// reopens it if miniaturized, makes it the selected tab and key, brings
    /// the application to the front and gives the keyboard to the id's pane.
    /// Sends no byte to the shell.
    ///
    /// `makeKeyAndOrderFront` makes the window in a tab group the selected tab
    /// (the precedent of `selectTab:`); on a miniaturized window it would only
    /// change the order and leave it in the Dock, so `deminiaturize` comes first.
    pub(crate) fn bring_to_front(&self, pane: &TerminalPane) {
        let window = &self.ivars().window;
        if window.isMiniaturized() {
            window.deminiaturize(None);
        }
        window.makeKeyAndOrderFront(None);
        self.focus_pane(pane);
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
        if let Some(alert) = alert {
            self.ivars()
                .window
                .endSheet_returnCode(&alert.window(), NSModalResponseCancel);
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
        if self.panes().len() > 1 {
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
        let pane = self.focused_pane();
        let confirm = app.settings().confirm_close;
        let Some(foregrounds) = foregrounds_to_ask(
            self.ivars().run.is_some(),
            confirm,
            std::slice::from_ref(&pane),
        ) else {
            self.close_pane(pane.id());
            return;
        };
        self.ask(
            &prompt(CloseScope::Pane, Unit::Pane, &foregrounds),
            CloseTarget::Pane(pane.id()),
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
    fn ask(&self, prompt: &Prompt, targets: CloseTarget) {
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
                    // The pane's tab is the window carrying the question; if
                    // the pane closed in the meantime (its shell exited) a no-op.
                    CloseTarget::Pane(pane) => {
                        if let Some(window) = app.window(host) {
                            window.close_pane(*pane);
                        }
                    }
                }
            });
        });
        self.ivars().alert.replace(Some(alert.clone()));
        alert.beginSheetModalForWindow_completionHandler(&self.ivars().window, Some(&answered));
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

    /// Reads the title from the **focused** pane's session and writes it to
    /// the window — the pane's `PaneHost::title_changed` event ([`WindowHost`])
    /// and the focus change ([`TerminalWindow::pane_focused`]).
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

    /// Writes the window's (and tab's) title from the session; while an upload
    /// flows `↑ N% · ` in front (`upload::titled_as`; `↓` while
    /// only downloads flow; the arrow and percentage from the
    /// pane's queue).
    fn apply_title(&self) {
        let pane = self.focused_pane();
        if let Some(session) = pane.session() {
            let prefix = pane.upload_title_prefix();
            self.ivars()
                .window
                .setTitle(&NSString::from_str(&upload::titled_as(
                    prefix,
                    &session.title(),
                )));
        }
    }

    /// The focused pane's remote host and resolved mark; `None` locally
    /// (`Session::remote_mark`).
    pub(crate) fn remote_mark(&self) -> Option<(String, HostMark)> {
        self.focused_pane().session()?.remote_mark()
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
        let pane = self.focused_pane();
        let color = pane.session().and_then(|session| {
            let (_, mark) = session.remote_mark()?;
            (mark != HostMark::None).then(|| session.theme().mark_rgb(mark))
        });
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

    /// Opens the first pane's session ([`TerminalPane::start`], from the
    /// birth package) and reads the title from the session once: a title
    /// notification that arrived before the session entered the slot may have
    /// found an empty slot and dropped; this read closes that (writes the same
    /// `bateri` if unchanged). The error returns to the caller: in the first
    /// window the process exits, in ⌘T/⌘N only that window closes.
    pub(crate) fn start(&self, mtm: MainThreadMarker) -> std::io::Result<()> {
        self.focused_pane().start(mtm)?;
        self.refresh_title();
        Ok(())
    }

    /// `[remote] hosts` changed — the pattern list goes to every pane's
    /// session ([`TerminalPane::set_host_marks`]), the tab's dot from the new resolution.
    pub(crate) fn set_host_marks(&self, settings: &Settings) {
        for pane in self.panes() {
            pane.set_host_marks(settings);
        }
        self.refresh_tab_mark();
    }

    /// Gives the theme to the panes ([`TerminalPane::set_theme`]: session and
    /// search panel), the separator ([`SplitView::set_theme`]) and paints the
    /// chrome with it ([`TerminalWindow::apply_chrome`]).
    ///
    /// Both in a single call, because there are two paths that change the
    /// theme (`AppDelegate::reload_settings`, `AppDelegate::apply_appearance`)
    /// and if one forgot the chrome the grid would be in the new theme and the
    /// title bar in the old — the symptom is exactly the seam the user would see.
    pub(crate) fn set_theme(&self, theme: Theme) {
        for pane in self.panes() {
            pane.set_theme(theme);
        }
        self.ivars().container.set_theme(&theme);
        self.apply_chrome(&theme);
        // The tab's dot is from the mark's role; the role is another colour in the new theme.
        self.refresh_tab_mark();
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
    /// upload queue, rhythm, `Waker`, `SIGHUP`) and is for **every** pane; the
    /// return is one result per pane in tree order. Idempotent; the place of a
    /// pane whose session never came to be is `None`.
    pub(crate) fn begin_close(&self) -> Vec<Option<Closing>> {
        self.panes().iter().map(|pane| pane.begin_close()).collect()
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
        tab_index, unit_for,
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
    fn numbered_tabs_select_the_nth_or_nothing() {
        assert_eq!(tab_index(1, 3), Some(0));
        assert_eq!(tab_index(3, 3), Some(2));
        assert_eq!(tab_index(4, 3), None, "a nonexistent tab is a no-op");
        assert_eq!(tab_index(8, 8), Some(7));
        assert_eq!(
            tab_index(8, 20),
            Some(7),
            "⌘8 is the eighth even with more than nine tabs"
        );
    }

    #[test]
    fn nine_selects_the_last_tab() {
        assert_eq!(tab_index(9, 1), Some(0), "with a single tab ⌘9 is that tab");
        assert_eq!(tab_index(9, 3), Some(2));
        assert_eq!(tab_index(9, 20), Some(19));
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

    #[test]
    fn no_tabs_or_unknown_tags_select_nothing() {
        assert_eq!(tab_index(1, 0), None);
        assert_eq!(tab_index(9, 0), None);
        assert_eq!(tab_index(0, 3), None);
        assert_eq!(tab_index(10, 12), None);
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
