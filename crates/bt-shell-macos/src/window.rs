//! Terminal window: an `NSWindow` and its `NSWindowDelegate`, and what
//! belongs to the **window** rather than to one of its tabs — chrome, the
//! title row and its tab bar (`tab_bar::TabBar`), the order and selection of
//! its tabs, the title it writes, the close question and its scope, and the
//! tab and split actions (`closeTab:`, `closeWindow:`, `selectTab:`,
//! `showNextTab:`/`showPreviousTab:`, `splitRight:`, `splitDown:`,
//! `selectPreviousSplit:`/`selectNextSplit:`, `selectSplit:`,
//! `resizeSplit:`, `equalizeSplits:`, `toggleSplitZoom:`). The responder
//! chain reaches the window's delegate, never a tab, so the actions are
//! here and hand the tab's work to its tab (`tab::TerminalTab`: the splits
//! container, the panes, the focused pane, the title's read).
//!
//! **One window, several tabs.** macOS's own tabs are off
//! (`tabbingMode = Disallowed`): its tab bar cannot be hidden or drawn in
//! the theme's colours through public API, so the window carries its tabs
//! itself. The content reaches under the title row (`FullSizeContentView`,
//! the title hidden) and an empty compact toolbar raises that row and
//! centres the traffic lights in it; the root view ([`RootView`]) puts the
//! bar in the row — its height read from AppKit, never a constant here —
//! and every tab's splits container below it. Only the selected tab's
//! container is shown; the others stay in the hierarchy, hidden, sized with
//! the window, so their panes keep their grids and draw nothing.
//!
//! **Every change to the tab list goes through one applier** (select, add,
//! close, name, reorder, leave for another window, join from one —
//! [`TerminalWindow::select_tab`], [`TerminalWindow::add_tab`],
//! [`TerminalWindow::close_tab_now`], [`TerminalWindow::rename_tab`],
//! [`TerminalWindow::move_tab`], [`TerminalWindow::release_tab`],
//! [`TerminalWindow::adopt_tab`]; and the same for a **pane** carried
//! between panes and tabs — [`TerminalWindow::swap_panes`],
//! [`TerminalWindow::move_pane`], [`TerminalWindow::new_tab_of`],
//! [`TerminalWindow::release_pane`] with [`TerminalWindow::adopt_pane`],
//! [`TerminalWindow::fold_tab`]) and ends
//! in a layout edge (`AppDelegate::layout_changed`): the selection, the
//! order, the names and which window a tab is in are part of the layout the
//! bound holder keeps and the crash restore reads, and nothing else would
//! carry them — selecting a tab no longer makes another window key. The applier's order is fixed: a container is shown or hidden
//! **first**, and only then are its panes told — visibility and focus read
//! the hierarchy as it stands (`isHiddenOrHasHiddenAncestor`). While the
//! window holds a sheet the selection does not move (a beep): the sheet
//! belongs to the tab on screen or to the whole window, and another tab
//! must not come up under it.
//!
//! **What a move comes to is not decided here.** A pane or a tab joining
//! another tab or becoming a tab or a window of its own, a tab let go on a
//! strip, Merge All Windows — in this window or another — is planned by
//! `moves` from the
//! windows' pictures ([`TerminalWindow::picture`]) — which steps, in which
//! order, what is selected and focused after, what Undo Move keeps — and the
//! application carries the plan out through the appliers above
//! (`AppDelegate::make_move`).
//!
//! **Undo Move is the appliers' too.** A move of panes writes down what it is
//! about to change and leaves that picture ([`Record`]) after its last layout
//! edge; every other applier of the tab list that changes its shape — adding,
//! closing, naming, reordering, a tab leaving or joining — drops it
//! ([`TerminalWindow::forget_undo`]), because the picture restores the whole
//! strip. Selecting a tab does not: the picture carries the selection and
//! puts it back, so "move a pane to web, open web, Undo Move" brings the pane
//! back. Taking it back is planned by `moves` too (`Move::Undo`) and carried
//! out through the steps the moves are made of and three of its own —
//! [`TerminalWindow::dissolve_tab`], [`TerminalWindow::reshape_tab`],
//! [`TerminalWindow::put_strip`] — ending in one layout edge.
//!
//! **A tab moves between windows as itself** (Move Tab to New Window, Merge
//! All Windows, and a tab dragged out of its strip or onto another window's —
//! the drag only picks the place, the move is the same one): it is taken out
//! of one window's order and hierarchy and put
//! into another's, never closed — its panes, shells, questions, indicators
//! and name go on. It leaves the screen the way a closing tab does, hidden
//! before its panes are told, and comes up the way a selected tab does; its
//! question's owner is ordered out with it and attached to the new window
//! when the tab is shown there ([`sheets::hide_owner`], [`sheets::show_owner`]).
//! The panes read their scale from the window they are in, so the move ends
//! with their geometry ([`TerminalTab::refresh_geometry`]): a move between two
//! screens of one size sends no notice. Nothing moves while either window holds
//! a question of its own ([`TerminalWindow::selection_free`]).
//!
//! The window's inputs come from `AppDelegate::open_window`; the panes'
//! events reach their tab (`tab::TabHost`) and through it the window or the
//! application. The application-wide parts (settings, watching, notice
//! slots, measurement ledger, timed-run recipe, window list) are in `app`;
//! the save-time paths coming from there reach **every tab**. The window's
//! geometry and occlusion reach **every** tab (a hidden one answers
//! "hidden" by itself); its key focus only the selected one. There is no
//! terminal drawing call here either; this file's job is wiring.
//!
//! Renderer per pane (`pane`'s header).

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::time::Duration;

use block2::RcBlock;
use bt_core::{ConfirmClose, ContentEdge, PointerShape, Settings, TabPosition, Theme};
use bt_gpu::GpuError;
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSAccessibilityAnnouncementKey,
    NSAccessibilityAnnouncementRequestedNotification, NSAccessibilityNotificationUserInfoKey,
    NSAccessibilityPostNotificationWithUserInfo, NSAlert, NSAlertFirstButtonReturn, NSAppearance,
    NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication,
    NSBackingStoreType, NSColor, NSControlStateValueOff, NSControlStateValueOn, NSCursor, NSEvent,
    NSFloatingWindowLevel, NSFont, NSFontAttributeName, NSMenuItem, NSModalResponse,
    NSModalResponseCancel, NSStringDrawing, NSTitlebarSeparatorStyle, NSToolbar, NSView, NSWindow,
    NSWindowDelegate, NSWindowOcclusionState, NSWindowOrderingMode, NSWindowStyleMask,
    NSWindowTabbingMode, NSWindowTitleVisibility, NSWindowToolbarStyle,
};
use objc2_core_foundation::CFRetained;
use objc2_core_graphics::CGImage;
use objc2_foundation::{
    NSAttributedStringKey, NSDictionary, NSKeyValueObservingOptions, NSNotification, NSObject,
    NSObjectNSKeyValueObserverRegistration, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
    ns_string,
};

use crate::Run;
use crate::app;
use crate::arrange::Tool;
use crate::card::{Ground, Shade, fade_to};
use crate::embed;
use crate::groups::{
    Click, GroupId, GroupSpot, Groups, Item, Part, Picked, Reach, Spot, Target, View,
};
use crate::jobs::Foreground;
use crate::launch::Closing;
use crate::moves::{self, Joins, Move};
use crate::palette::{self, PaletteScrim};
use crate::pane::TerminalPane;
use crate::pointer;
use crate::preview::beep;
use crate::restore::SavedTab;
use crate::sheets::{self, Asker};
use crate::split::{Axis, Direction, Tree};
use crate::split_view::{Hairline, SplitView};
use crate::tab::{self, TerminalTab};
use crate::tab_bar::{Heading, Label, RowClaim, SwitchRow, Switcher, TabBar, counted};
use crate::tabs::{self, Card, Column, Finish, Tabs};
use crate::undo::{Record, Scene, Shape};

/// Whether the theme's background is dark — the window chrome's appearance
/// (Aqua / DarkAqua) comes from this ([`TerminalWindow::apply_chrome`]).
///
/// The question is "which text reads better on this background: white or
/// black" and the answer is from WCAG's contrast ratio
/// (`bt_core::contrast_ratio`, the measure the theme's own choices are made
/// with): if the background gives a higher contrast with white, it is dark.
/// The threshold is not invented, it arises from the equality of the two
/// ratios; the system's dark appearance means exactly "light text".
///
/// The answer is `Theme::is_dark`'s: the split tab's ground (`bt-core`) asks
/// the same question, and one threshold lives in one place.
pub(crate) fn is_dark_background(theme: &Theme) -> bool {
    theme.is_dark()
}

/// What the confirmed question will close ([`TerminalWindow::ask`]): ids,
/// looked up again at answer time.
#[derive(Clone, Debug)]
enum CloseTarget {
    /// The whole window, every tab.
    Window,
    /// Some of the window's tabs (tab ids), with all their panes.
    Tabs(Vec<u64>),
    /// A single pane of a tab (tab and pane ids); the tab stays open.
    Pane { tab: u64, pane: u64 },
}

/// Where a tab that comes from another window goes ([`TerminalWindow::adopt_tab`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Placement {
    /// Last, and the window keeps showing what it showed: Merge All Windows.
    End,
    /// At this place in the strip, and the tab is the one on screen: a tab let go on the strip.
    At(usize),
}

/// Where a new tab sits among the groups ([`TerminalWindow::add_tab_seated`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Seat {
    /// Beside the tab on screen and in its group, a closed group opening for it: ⌘T, New Tab,
    /// the foot's `+`.
    Beside,
    /// Among the ungrouped tabs, after the last of them: Shell ▸ New Ungrouped Tab, and ⌃⌘0 when
    /// there is none.
    Loose,
    /// At the end of a group, which opens: the `+` of its header.
    Group(GroupId),
    /// In a group made for it after the others, with this name: Shell ▸ New Group.
    New(String),
}

/// Title of the Shell ▸ Close Tab item: with several panes ⌘W
/// closes the focused pane and the item is "Close", with one pane it closes
/// the tab and is "Close Tab".
pub(crate) fn close_title(panes: usize) -> &'static str {
    if panes > 1 { "Close" } else { "Close Tab" }
}

/// Title of the Shell ▸ Close Tab item while tabs are picked together in the side column: ⌘W
/// closes all of them ([`TerminalWindow::close_picked`]) and always asks first, so the title
/// ends in the "…" of a command that asks — as the picked rows' own menu says it.
pub(crate) fn close_picked_title(count: usize) -> String {
    counted("Close", count, "\u{2026}")
}

/// What is being closed — chooses the question's title and confirm button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CloseScope {
    /// A pane whose tab has other panes (⌘W).
    Pane,
    /// A tab whose window has other tabs (⌘W, the tab's `×`, a middle click).
    Tab,
    /// Part of the window, several tabs ("Close Other Tabs").
    Tabs(usize),
    /// The whole window: ⌘W in a single-tab window, ⇧⌘W, the red button.
    Window,
    /// The application (⌘Q, Dock ▸ Quit, logout).
    Quit,
}

/// The number of tabs a gesture asks for and the window's tab count → the
/// question's scope.
///
/// Every tab is the window (red button, ⇧⌘W, ⌘W in a single-tab window), a
/// single tab is the tab (⌘W, `×`), everything in between is counted tabs
/// ("Close Other Tabs").
pub(crate) fn close_scope(requested: usize, tabs: usize) -> CloseScope {
    if requested >= tabs {
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
///
/// **The one exception** is `selected`, ⌘W over tabs the user picked together in the side
/// column: it asks under `running` too, with nothing running in them (`never` still does not).
/// The hand that reaches for ⌘W to close one tab reaches for it to close five, and a row of
/// tabs gone for good at an empty prompt is not a thing to undo.
pub(crate) fn should_ask(
    timed: bool,
    confirm: ConfirmClose,
    selected: bool,
    running: impl FnOnce() -> bool,
) -> bool {
    if timed {
        return false;
    }
    match confirm {
        ConfirmClose::Never => false,
        ConfirmClose::Always => true,
        ConfirmClose::Running => selected || running(),
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
    foregrounds_to_ask_for(timed, confirm, false, panes)
}

/// [`foregrounds_to_ask`] for a close that may be of tabs picked together (`selected`,
/// [`should_ask`]'s exception).
pub(crate) fn foregrounds_to_ask_for(
    timed: bool,
    confirm: ConfirmClose,
    selected: bool,
    panes: &[Retained<TerminalPane>],
) -> Option<Vec<Foreground>> {
    let read = || {
        panes
            .iter()
            .map(|pane| pane.foreground())
            .collect::<Vec<_>>()
    };
    let mut seen = None;
    let ask = should_ask(timed, confirm, selected, || {
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

/// The panes of `pool` named by `ids`, in that order; each leaves the pool.
fn take_panes(
    pool: &mut Vec<Retained<TerminalPane>>,
    ids: impl IntoIterator<Item = u64>,
) -> Vec<Retained<TerminalPane>> {
    ids.into_iter()
        .filter_map(|id| {
            let index = pool.iter().position(|pane| pane.id() == id)?;
            Some(pool.remove(index))
        })
        .collect()
}

/// What a pane is called to VoiceOver: its session's title.
pub(crate) fn pane_name(pane: &TerminalPane) -> String {
    pane.session()
        .map_or_else(|| "Split".to_owned(), |session| session.title())
}

/// The direction of a Select/Resize Split ▸ item: the sender's `tag`.
fn direction_of(sender: Option<&AnyObject>) -> Option<Direction> {
    let item = sender?.downcast_ref::<NSMenuItem>()?;
    Direction::from_tag(item.tag())
}

/// The window's state — chrome, the title row and its bar, the tabs and
/// their order, the close question. The splits container, the panes and the
/// focused pane are each tab's ([`TerminalTab`]); the session's core is in
/// the panes ([`TerminalPane`]).
pub(crate) struct WindowIvars {
    /// Our own counter (`AppDelegate` hands it out): the key by which the
    /// close question and removal from the list find the window. The tab's
    /// and the pane's ids are separate ([`TerminalTab::id`],
    /// [`TerminalPane::id`]) and from the same counter.
    id: u64,
    /// The timed run's recipe, a copy of `AppDelegate`'s (`Copy`): the close
    /// question is never asked in a timed run and must be answerable without
    /// reaching the application delegate ([`TerminalWindow::should_close_now`]).
    run: Option<Run>,
    window: Retained<NSWindow>,
    /// The `contentView`: the bar on top, the tabs' containers below.
    root: Retained<RootView>,
    /// The empty compact toolbar that gives the title row its height; hidden
    /// in full screen, where AppKit moves it into a window of its own over
    /// the bar ([`TerminalWindow`]'s full-screen hooks).
    toolbar: Retained<NSToolbar>,
    /// The tabs' order and the selected one — the pure model
    /// (`tabs::Tabs`); [`WindowIvars::tabs`] holds the objects. Never empty
    /// while the window lives: closing the last tab closes the window.
    order: RefCell<Tabs<u64>>,
    /// The tabs' groups — the pure model (`groups::Groups`) beside [`WindowIvars::order`]: which
    /// tabs are gathered under which name. The strip changes only together with them
    /// ([`TerminalWindow::with_strip`]).
    groups: RefCell<Groups<u64>>,
    /// The window's tabs — the only strong references to the tab objects;
    /// the list finds them through the window ([`TerminalWindow::tabs`]).
    /// Their order is [`WindowIvars::order`]'s, not this vector's.
    tabs: RefCell<Vec<Retained<TerminalTab>>>,
    /// Tabs closed this turn, dropped on the next: the close is reached
    /// from inside the tab's own methods ([`TerminalWindow::close_tab_now`]).
    retired: RefCell<Vec<Retained<TerminalTab>>>,
    /// The settings diagnostic the bar shows (`AppDelegate::post_notices`);
    /// empty without one.
    notice: RefCell<String>,
    /// The background the chrome was last painted with (the gate of
    /// [`TerminalWindow::apply_chrome`]); `None`: not painted yet.
    chrome: Cell<Option<u32>>,
    /// The open close question in this window: keeps the `NSAlert`
    /// alive for the sheet's duration and is the "no second question while the
    /// sheet is open" gate ([`TerminalWindow::asking`]). The completion block
    /// empties it on every answer.
    alert: RefCell<Option<Retained<NSAlert>>>,
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
            // A question up in the tab on screen sits on a window of its own
            // that does not follow by itself (`sheets::fit_owner`).
            if let Some(tab) = self.try_selected_tab() {
                sheets::fit_owner(tab.container());
            }
            self.layout_changed();
        }

        #[unsafe(method(windowDidResize:))]
        fn window_did_resize(&self, _n: &NSNotification) {
            self.layout_changed();
        }

        // `[appearance] resize_in_steps`: an edge the user drags moves a
        // whole cell at a time. Only the user's drag (`inLiveResize`): a frame
        // set by macOS — tiling, Fill, zoom, a restored layout — is taken as
        // given, or a tiled window would leave a gap beside its neighbour.
        #[unsafe(method(windowWillResize:toSize:))]
        fn window_will_resize(&self, sender: &NSWindow, size: NSSize) -> NSSize {
            let stepping = sender.inLiveResize()
                && app::delegate(self.mtm()).is_some_and(|app| app.settings().resize_in_steps);
            if !stepping {
                return size;
            }
            self.stepped_size(size).unwrap_or(size)
        }

        // On a move between screens the size (points) does not change but the
        // scale does; nobody but us writes that in a layer-hosting view.
        //
        // The split boundaries sit on the device pixel, so on a scale change
        // first the re-layout, then **every** pane's geometry — of every tab,
        // a hidden one's too: the notification of a pane whose frame did not
        // change does not arrive.
        #[unsafe(method(windowDidChangeBackingProperties:))]
        fn window_did_change_backing(&self, _n: &NSNotification) {
            self.ivars().root.lay_out();
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
        // it. **Every tab hears it**: a background tab's panes answer
        // "hidden" by themselves (their container is hidden,
        // `SplitView::apply_visibility`), so they stay at zero frames when the
        // window comes back, and selecting a tab is the applier's own call to
        // the same path. Stacking special cases on top of the general signal
        // would mean the list never closes (full screen, Space, `unhide`,
        // screen wake...).
        #[unsafe(method(windowDidChangeOcclusionState:))]
        fn window_did_change_occlusion(&self, _n: &NSNotification) {
            // The notification comes in both directions; asking for a frame
            // while GOING occluded would mean drawing a frame nobody will see.
            let visible = self.window_visible();
            // A hidden pane behind the zoom is counted as occluded
            // (`SplitView::apply_visibility`).
            for tab in self.tabs() {
                tab.apply_visibility(visible);
            }
            // The bar's clock and its rings' turns run only while the window
            // is seen: a window coming back sets its rings turning from now
            // and the clock again, one going away takes the turns off and
            // lets the clock lapse.
            self.refresh_bar();
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
        // The window's key bit goes to the **selected** tab's panes — a
        // background tab's are not the user's (`TerminalPane::is_active`);
        // an unfocused pane's hollow caret comes from the second bit, from
        // its own `BateriView`'s first-responder hooks.
        //
        // A question up in the selected tab sits on its own window and does
        // not block this one, so the key it would have kept is handed to it
        // (`TerminalTab::key_to_sheet`): typing never reaches the pane under
        // the question, and the window's resigning gives the panes their
        // focus off at once.
        #[unsafe(method(windowDidBecomeKey:))]
        fn window_did_become_key(&self, _n: &NSNotification) {
            // The key window is part of the layout.
            self.layout_changed();
            // None while the window has given up its last tab and is about to
            // close: handing the keyboard back from the tab's question
            // (`sheets::hide_owner`) makes it key on the way.
            let Some(tab) = self.try_selected_tab() else {
                return;
            };
            self.note_visit(&tab);
            if tab.key_to_sheet() {
                return;
            }
            for pane in tab.panes() {
                pane.window_became_key();
            }
        }

        #[unsafe(method(windowDidResignKey:))]
        fn window_did_resign_key(&self, _n: &NSNotification) {
            // ⌘'s release, or the pointer's leaving, may go to the
            // application ⌘-Tab brings forward: the tabs' key hints and the
            // summary card go now, not at an event we never see.
            self.bar().resigned();
            // The palette is the window's while the window is key: a question opened over it,
            // another application in front, and it goes.
            self.close_palette();
            // A lifted tab (⌥⌘ held) is set down with them.
            if let Some(app) = app::delegate(self.mtm()) {
                app.key_window_resigned();
            }
            // The ⌘-hovered link clears too: ⌘'s release may go to
            // another application. The key window also resigns key when the
            // application deactivates, so this one hook covers both.
            for pane in self.panes() {
                pane.window_resigned_key();
            }
        }

        /// The window's own question ended: a question the selected tab
        /// parked behind it opens now ([`TerminalTab::open_parked`]). A tab's
        /// questions sit on its owner, whose ending is heard there.
        #[unsafe(method(windowDidEndSheet:))]
        fn window_did_end_sheet(&self, _n: &NSNotification) {
            if let Some(tab) = self.try_selected_tab() {
                tab.open_parked();
            }
        }

        /// Full screen moves the toolbar into a window of its own that
        /// covers the bar and takes its clicks (measured); hidden there, the
        /// bar is on top and keeps the row's height it had.
        #[unsafe(method(windowWillEnterFullScreen:))]
        fn window_will_enter_full_screen(&self, _n: &NSNotification) {
            self.ivars().toolbar.setVisible(false);
        }

        /// The lights left the row: the strip starts at the edge. The window's
        /// corners went with them, and the panes' text no longer keeps clear
        /// of a curve ([`TerminalWindow::refresh_corners`]).
        #[unsafe(method(windowDidEnterFullScreen:))]
        fn window_did_enter_full_screen(&self, _n: &NSNotification) {
            self.ivars().root.lay_out();
            self.refresh_corners();
        }

        #[unsafe(method(windowDidExitFullScreen:))]
        fn window_did_exit_full_screen(&self, _n: &NSNotification) {
            self.ivars().toolbar.setVisible(true);
            self.ivars().root.lay_out();
            self.refresh_corners();
        }

        /// The red button: whether to ask before closing the whole window.
        /// `false` stops the closing; if a question was asked the closing is
        /// in its answer ([`TerminalWindow::ask`]). The main menu's ⌘W and
        /// ⇧⌘W do not go through here (`closeTab:`, `closeWindow:`).
        ///
        /// The shell's exit does **not** go through here: `close` does not ask
        /// the delegate.
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _sender: &NSWindow) -> bool {
            self.should_close_now()
        }

        /// The window is closing: the red button, ⇧⌘W, ⌘W in its last tab
        /// and the last tab's shell exit arrive here.
        ///
        /// Closing is **not waited on**: it is started and the
        /// handle drops, the `"PTY teardown"` thread finishes its work in the
        /// background — closing a window must not stall the main thread for
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
    //
    // **The tab actions are named apart from `NSWindow`'s**
    // (`showNextTab:`, not `selectNextTab:`): the window itself answers its
    // own tab selectors before the chain reaches its delegate, so a shared
    // name would never arrive here.
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
            // A window that has just given up its last tab (Merge All Windows)
            // has none: the first responder going with the tab is not news.
            let Some(tab) = self.try_selected_tab() else {
                return;
            };
            if let Some(pane) = tab.responder_pane() {
                tab.pane_focused(pane.id());
            }
        }

        /// ⌘W's title and the split's enabled state; **an unknown item is
        /// `true`**. With several panes ⌘W is "Close" (the focused pane), with
        /// one pane "Close Tab". A split is grey if one of the
        /// halves would drop below the smallest pane limit. Previous and Next
        /// Tab are grey with a single tab.
        #[unsafe(method(validateMenuItem:))]
        fn validate_menu_item(&self, item: &NSMenuItem) -> bool {
            let action = item.action();
            let tab = self.selected_tab();
            // No `return`: `define_class!` converts the `bool` at the end of the body.
            if action == Some(sel!(closeTab:)) {
                let picked = self.bar().picks().len();
                item.setTitle(&NSString::from_str(&if picked > 0 {
                    close_picked_title(picked)
                } else {
                    close_title(tab.panes().len()).to_owned()
                }));
                true
            } else if [
                sel!(showNextTab:),
                sel!(showPreviousTab:),
                sel!(showTabList:),
                sel!(detachTab:),
                sel!(closeOtherTabs:),
            ]
            .into_iter()
            .any(|tabs| action == Some(tabs))
            {
                // With a single tab there is nothing to go to, list, move out
                // or close besides. (Naming stays: a lone tab's name is the
                // window's title, and one named before the others closed must
                // be possible to change.)
                self.ivars().order.borrow().len() > 1
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
                sel!(swapSplit:),
                sel!(movePaneToNewTab:),
            ]
            .into_iter()
            .any(|split| action == Some(split))
            {
                // With a single pane there is nothing to navigate, resize,
                // swap, and a tab's only pane made a tab is the tab.
                tab.panes().len() > 1
            } else if action == Some(sel!(movePaneToTab:)) {
                // A tab the menu lists, other than the one on screen: the
                // placeholder of a window with one tab names none.
                tabs::tab_of_menu_tag(item.tag()).is_some_and(|target| {
                    self.tab(target).is_some() && !self.is_selected(target)
                })
            } else if [
                sel!(movePaneToPreviousTab:),
                sel!(movePaneToNextTab:),
            ]
            .into_iter()
            .any(|to| action == Some(to))
            {
                self.ivars().order.borrow().len() > 1
            } else if action == Some(sel!(movePaneToNewWindow:)) {
                // A tab's only pane is the tab (Move Tab to New Window); a
                // lone pane of a lone tab has nothing to leave.
                tab.panes().len() > 1 || self.ivars().order.borrow().len() > 1
            } else {
                // A group item says for itself; an unknown item is `true`.
                self.validate_group_item(item).unwrap_or(true)
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

        /// Window ▸ Swap Split ▸ (⇧⌥⌘ + arrow): the focused pane trades places
        /// with its neighbour in that direction; a no-op at the edge.
        #[unsafe(method(swapSplit:))]
        fn swap_split_action(&self, sender: Option<&AnyObject>) {
            let Some(direction) = direction_of(sender) else {
                return;
            };
            let tab = self.selected_tab();
            if let Some((focused, neighbour)) = tab.swap_toward(direction) {
                self.swap_panes(tab.id(), focused, neighbour);
            }
        }

        /// Window ▸ Move Split to New Tab: the focused pane becomes a tab of
        /// its own right of this one, not selected.
        #[unsafe(method(movePaneToNewTab:))]
        fn move_pane_to_new_tab_action(&self, _sender: Option<&AnyObject>) {
            let tab = self.selected_tab();
            let gap = self.index_of(tab.id()).map_or(0, |index| index + 1);
            self.make_move(Move::PaneToNewTab {
                pane: tab.focused_pane().id(),
                window: self.id(),
                gap,
                target: self.beside_target(tab.id()),
            });
        }

        /// Window ▸ Move Split to Tab ▸ (the item's `tag` names the tab,
        /// [`tabs::menu_tag`]): the focused pane joins that tab beside its
        /// focused pane, on the right.
        #[unsafe(method(movePaneToTab:))]
        fn move_pane_to_tab_action(&self, sender: Option<&AnyObject>) {
            if let Some(target) = tagged_tab(sender) {
                let pane = self.selected_tab().focused_pane().id();
                self.make_move(Move::PaneToTab {
                    pane,
                    into: target,
                    place: Joins::Beside(Direction::Right),
                });
            }
        }

        /// Window ▸ Move Split to Previous Tab (⇧⌥⌘[).
        #[unsafe(method(movePaneToPreviousTab:))]
        fn move_pane_to_previous_tab_action(&self, _sender: Option<&AnyObject>) {
            self.move_pane_to_adjacent(false);
        }

        /// Window ▸ Move Split to Next Tab (⇧⌥⌘]).
        #[unsafe(method(movePaneToNextTab:))]
        fn move_pane_to_next_tab_action(&self, _sender: Option<&AnyObject>) {
            self.move_pane_to_adjacent(true);
        }

        /// Window ▸ Move Split to New Window: the focused pane becomes a
        /// window of its own (a tab's only pane takes its tab, as Move Tab to
        /// New Window). A turn later, as that action.
        #[unsafe(method(movePaneToNewWindow:))]
        fn move_pane_to_new_window_action(&self, _sender: Option<&AnyObject>) {
            let pane = self.selected_tab().focused_pane().id();
            self.later(move |window| {
                window.make_move(Move::PaneToNewWindow { pane, at: None });
            });
        }

        /// The chip menu's Merge into Current Tab ▸ Split Right: the chip's
        /// tab joins the selected one, its panes as a block on the right of
        /// the selected tab's focused pane. A turn later (the chip).
        #[unsafe(method(mergeTabRight:))]
        fn merge_tab_right_action(&self, sender: Option<&AnyObject>) {
            self.merge_chip_tab(sender, Direction::Right);
        }

        /// The chip menu's Merge into Current Tab ▸ Split Down.
        #[unsafe(method(mergeTabDown:))]
        fn merge_tab_down_action(&self, sender: Option<&AnyObject>) {
            self.merge_chip_tab(sender, Direction::Down);
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
        /// with one pane **only this tab** (the window in its last tab);
        /// asking if needed. The question and the closing take the same path
        /// as the tab's `×`. With tabs picked together in the side column it is **all of
        /// them**, and it always asks ([`should_ask`]).
        #[unsafe(method(closeTab:))]
        fn close_tab(&self, _sender: Option<&AnyObject>) {
            if self.close_picked() {
                return;
            }
            let tab = self.selected_tab();
            if tab.panes().len() > 1 {
                self.close_pane_asking(&tab);
            } else {
                self.close_tab_asking(tab.id());
            }
        }

        /// Shell ▸ Close Window (⇧⌘W): the window **with all its tabs and
        /// panes**, under a single question.
        #[unsafe(method(closeWindow:))]
        fn close_window(&self, _sender: Option<&AnyObject>) {
            self.close_window_asking();
        }

        /// Window ▸ Select Tab ▸ Tab n (⌘1…⌘8) and Last Tab (⌘9): the item's
        /// `tag` ([`crate::menu`]) lands on a tab (`tabs::tab_index`). A
        /// nonexistent tab is a no-op.
        #[unsafe(method(selectTab:))]
        fn select_tab_action(&self, sender: Option<&AnyObject>) {
            let Some(item) = sender.and_then(|sender| sender.downcast_ref::<NSMenuItem>()) else {
                return;
            };
            let Ok(tag) = u8::try_from(item.tag()) else {
                return;
            };
            // ⌘ counts the tabs as listed: down the column, a closed group's hidden ones not at
            // all; along the title row, the group on screen.
            let target = self
                .ivars()
                .groups
                .borrow()
                .shortcut(&self.ivars().order.borrow(), self.listing(), tag);
            if let Some(tab) = target {
                self.select_tab(tab);
            }
        }

        /// Window ▸ Show All Tabs (⇧⌘\): every tab in a list under the
        /// list button, or where it would be while the tabs fit.
        #[unsafe(method(showTabList:))]
        fn show_tab_list(&self, _sender: Option<&AnyObject>) {
            self.bar().show_list();
        }

        /// A row of the Show All Tabs list: the item's `tag` names the tab.
        #[unsafe(method(pickTab:))]
        fn pick_tab(&self, sender: Option<&AnyObject>) {
            if let Some(id) = tagged_tab(sender) {
                self.select_tab(id);
            }
        }

        /// Window ▸ Rename Tab… (the selected tab) and the chip menu's (the
        /// chip's, whichever it is): the name field opens over the chip, the
        /// tab brought forward first.
        #[unsafe(method(renameTab:))]
        fn rename_tab_action(&self, sender: Option<&AnyObject>) {
            if let Some(id) = self.menu_tab(sender)
                && self.select_tab(id)
            {
                self.bar().begin_rename(id);
            }
        }

        /// The chip menu's Close Tab: **the tab**, however many panes it
        /// has — ⌘W closes the focused pane first. A turn later.
        #[unsafe(method(closeChipTab:))]
        fn close_chip_tab(&self, sender: Option<&AnyObject>) {
            if let Some(id) = self.menu_tab(sender) {
                self.later(move |window| window.close_tab_asking(id));
            }
        }

        /// The chip menu's Close Other Tabs: every tab but the chip's, under
        /// one question. A turn later.
        #[unsafe(method(closeOtherTabs:))]
        fn close_other_tabs_action(&self, sender: Option<&AnyObject>) {
            if let Some(id) = self.menu_tab(sender) {
                self.later(move |window| window.close_other_tabs(id));
            }
        }

        /// The picked tabs' menu: Close N Tabs… A turn later, as every action that takes tabs
        /// out from under the menu's chip.
        #[unsafe(method(closePicked:))]
        fn close_picked_action(&self, _sender: Option<&AnyObject>) {
            self.later(|window| {
                window.close_picked();
            });
        }

        /// The picked tabs' menu: Move N Tabs to New Window, ungrouped, in the order they had.
        #[unsafe(method(detachPicked:))]
        fn detach_picked_action(&self, _sender: Option<&AnyObject>) {
            self.later(|window| {
                let tabs = window.bar().picks();
                if !tabs.is_empty() {
                    window.make_move(Move::TabsToNewWindow {
                        window: window.id(),
                        tabs,
                        at: None,
                    });
                }
            });
        }

        /// The picked tabs' menu: Move N Tabs to Group ▸ a group (the item's `tag`).
        #[unsafe(method(movePickedToGroup:))]
        fn move_picked_to_group_action(&self, sender: Option<&AnyObject>) {
            if let Some(group) = self.menu_group(sender) {
                self.later(move |window| {
                    window.regroup_picked(Target::Group {
                        group,
                        before: None,
                    });
                });
            }
        }

        /// The picked tabs' menu: Move N Tabs to New Group (and Move to Group ▸ New Group…).
        #[unsafe(method(movePickedToNewGroup:))]
        fn move_picked_to_new_group_action(&self, _sender: Option<&AnyObject>) {
            self.later(|window| {
                let tabs = window.bar().picks();
                if window.move_tabs_to_new_group(&tabs) {
                    window.bar().drop_picks();
                }
            });
        }

        /// The picked tabs' menu: Remove N Tabs from Group.
        #[unsafe(method(removePickedFromGroup:))]
        fn remove_picked_from_group_action(&self, _sender: Option<&AnyObject>) {
            self.later(|window| {
                window.regroup_picked(Target::Loose { before: None });
            });
        }

        /// Window ▸ Move Tab to New Window (the selected tab) and the chip
        /// menu's (the chip's): a window of its own with the same size. A turn
        /// later, the tab leaves the chip that was clicked.
        #[unsafe(method(detachTab:))]
        fn detach_tab(&self, sender: Option<&AnyObject>) {
            if let Some(id) = self.menu_tab(sender) {
                self.later(move |window| {
                    window.make_move(Move::TabToNewWindow { tab: id, at: None });
                });
            }
        }

        /// Window ▸ Show Next Tab (⇧⌘], ⌃⇥): the selected tab's right
        /// neighbour, wrapping around.
        #[unsafe(method(showNextTab:))]
        fn show_next_tab(&self, _sender: Option<&AnyObject>) {
            let target = self.ivars().order.borrow().adjacent(true);
            if let Some(tab) = target {
                self.select_tab(tab);
            }
        }

        /// Window ▸ Show Previous Tab (⇧⌘[, ⌃⇧⇥).
        #[unsafe(method(showPreviousTab:))]
        fn show_previous_tab(&self, _sender: Option<&AnyObject>) {
            let target = self.ivars().order.borrow().adjacent(false);
            if let Some(tab) = target {
                self.select_tab(tab);
            }
        }

        /// Window ▸ Rename Group… (the group on screen) and a header menu's (the header's): the
        /// name field opens over the header.
        #[unsafe(method(renameGroup:))]
        fn rename_group_action(&self, sender: Option<&AnyObject>) {
            if let Some(group) = self.menu_group(sender) {
                self.bar().begin_group_rename(group, false);
            }
        }

        /// Move to Group ▸ a group: the item's `tag` names the group, the tab is its menu's
        /// ([`Self::submenu_tab`]). A turn later: the move changes the list the menu came from.
        #[unsafe(method(moveTabToGroup:))]
        fn move_tab_to_group_action(&self, sender: Option<&AnyObject>) {
            if let (Some(group), Some(tab)) = (self.menu_group(sender), self.submenu_tab(sender)) {
                self.later(move |window| {
                    window.regroup_tab(
                        tab,
                        Target::Group {
                            group,
                            before: None,
                        },
                    );
                });
            }
        }

        /// Move to New Group and Move Tab to Group ▸ New Group…: the tab is in a group of its own
        /// and the name field is open on its header.
        #[unsafe(method(moveTabToNewGroup:))]
        fn move_tab_to_new_group_action(&self, sender: Option<&AnyObject>) {
            if let Some(tab) = self.menu_tab(sender) {
                self.later(move |window| window.move_to_new_group(tab));
            }
        }

        /// Remove from Group: the tab goes among the ungrouped ones.
        #[unsafe(method(removeTabFromGroup:))]
        fn remove_tab_from_group_action(&self, sender: Option<&AnyObject>) {
            if let Some(tab) = self.menu_tab(sender) {
                self.later(move |window| {
                    window.regroup_tab(tab, Target::Loose { before: None });
                });
            }
        }

        /// A header menu's Ungroup: the group's tabs stay, ungrouped.
        #[unsafe(method(ungroupGroup:))]
        fn ungroup_group_action(&self, sender: Option<&AnyObject>) {
            if let Some(group) = self.menu_group(sender) {
                self.later(move |window| window.ungroup(group));
            }
        }

        /// Window ▸ Move Group to New Window (the group on screen) and a header menu's (the
        /// header's): the group becomes a window of its own, a turn later like the tab's.
        #[unsafe(method(moveGroupToNewWindow:))]
        fn move_group_to_new_window_action(&self, sender: Option<&AnyObject>) {
            if let Some(group) = self.menu_group(sender) {
                self.later(move |window| {
                    window.move_group_to_new_window(group);
                });
            }
        }

        /// A header menu's Close Group: every tab of the group, under one question.
        #[unsafe(method(closeGroup:))]
        fn close_group_action(&self, sender: Option<&AnyObject>) {
            if let Some(group) = self.menu_group(sender) {
                self.later(move |window| window.close_group(group));
            }
        }

        /// A header menu's New Tab in Group: a tab at the group's end, the group open.
        #[unsafe(method(newTabInGroup:))]
        fn new_tab_in_group_action(&self, sender: Option<&AnyObject>) {
            if let Some(group) = self.menu_group(sender)
                && let Some(app) = app::delegate(self.mtm())
            {
                app.new_tab_seated(self.id(), Seat::Group(group));
            }
        }

        /// Window ▸ Select Group ▸ (⌃⌘1…⌃⌘9, ⌃⌘0 the ungrouped tabs): the item's `tag`
        /// ([`Groups::reach`]); with no ungrouped tab ⌃⌘0 opens one.
        #[unsafe(method(selectGroup:))]
        fn select_group_action(&self, sender: Option<&AnyObject>) {
            let Some(item) = sender.and_then(|sender| sender.downcast_ref::<NSMenuItem>()) else {
                return;
            };
            let Ok(tag) = u8::try_from(item.tag()) else {
                return;
            };
            self.reach_group(tag);
        }
    }
);

/// How wide a window's title may be in the menu font, points. The system lists
/// a window by its title — the Dock icon's menu, the Window menu — and draws
/// the list as wide as its longest title; a program's title can be a whole
/// sentence. A design number: room for a folder's path or a short sentence,
/// so the list stays one width whatever runs.
const LISTED_TITLE_PT: f64 = 320.0;

/// `title` as the window carries it: at most [`LISTED_TITLE_PT`] wide in the
/// menu font, cut with "…" past that ([`tabs::fit_title`]). Nothing in
/// bateri reads the window's title back — the bar draws each tab's own —
/// so only the system's lists, Mission Control and VoiceOver's window name see
/// the cut.
pub(crate) fn listed_title(title: &str) -> String {
    let font = NSFont::menuFontOfSize(0.0);
    let values: [&AnyObject; 1] = [font.as_ref()];
    // SAFETY: AppKit's font attribute key (an extern static) with the
    // `NSFont` it documents.
    let attributes = unsafe {
        NSDictionary::<NSAttributedStringKey, AnyObject>::from_slices(
            &[NSFontAttributeName],
            &values,
        )
    };
    tabs::fit_title(title, LISTED_TITLE_PT, |text| {
        // SAFETY: the dictionary is the font attribute with an `NSFont`.
        unsafe { NSString::from_str(text).sizeWithAttributes(Some(&attributes)) }.width
    })
}

/// The tab a menu item's `tag` names ([`tabs::menu_tag`]); `None` for a
/// bar item (zero), which acts on the selected tab.
fn tagged_tab(sender: Option<&AnyObject>) -> Option<u64> {
    let item = sender?.downcast_ref::<NSMenuItem>()?;
    tabs::tab_of_menu_tag(item.tag())
}

/// The window's `contentView`: the tab bar along the top, the title row's
/// height tall, and every tab's splits container below it — all the same
/// frame, only the selected one shown ([`TerminalWindow`]'s header).
///
/// The row's height is AppKit's: the inset the toolbar leaves at the top
/// of the content layout rect. In full screen the hidden toolbar leaves no
/// inset, and the height the row had in a window stays — one copy, read from
/// the window, never written as a constant.
///
/// **It is read in the layout pass, never in `resizeSubviewsWithOldSize:`.**
/// While the window's frame changes AppKit resizes the content view first
/// and settles its own chrome after: at that hook the content layout rect,
/// the window frame's top inset and the title bar's buttons all still
/// describe the previous size, so the inset came out as the row plus the
/// resize's delta — measured: a window grown by 200 pt got a 240 pt row —
/// and it stayed, since nothing laid the root out again. The layout pass
/// that follows every frame change (`layout`) sees the settled rect. Pinning
/// the bar to `contentLayoutGuide` with constraints instead would not keep
/// the row: in full screen the guide's top reaches the view's top and the
/// row would collapse, so the height would still need a remembered copy.
pub(crate) struct RootIvars {
    bar: Retained<TabBar>,
    /// The ground under a split tab's cards (`card::Ground`): the bottom of
    /// the stack, under the bar and every container.
    ground: Retained<Ground>,
    /// The cards' shadows and cuts (`card::Shade`): over the ground, under
    /// the containers.
    shade: Retained<Shade>,
    /// With the tabs in the side column, the one line between the column and the content, in
    /// the dividers' tone: over the ground and the shade, under the bar. It shows while the tab
    /// on screen is not cards on the ground and gives way to the ground over the ground's own
    /// 220 ms ([`RootView::sync_ground`]).
    rule: Retained<Hairline>,
    /// Whether [`RootIvars::rule`] is meant to show.
    ruled: Cell<bool>,
    /// With the tabs in the side column, the title row's band over the content: a claim on it,
    /// so a press there is the pane's and the window server does not move the window
    /// ([`RowClaim`]) — a press on text there selects; one on an empty spot of a pane moves the
    /// window by the pane's own hand (`tabs::side_grab`). Hidden while the bar is in the row.
    /// Above every container.
    claim: Retained<RowClaim>,
    /// The side column's right edge, which resizes it ([`SideEdge`]): the top of the stack, over
    /// the claim and every container. Hidden while the bar is in the row.
    edge: Retained<SideEdge>,
    /// The side column's width as the user left it by dragging its edge; the column is this
    /// fitted to the window ([`tabs::side_width`]), so one the window narrowed comes back as it
    /// widens. The default until dragged, and kept while the tabs are in the title row.
    side_width: Cell<f64>,
    /// The title row's last measured height; `0` until the window reports one.
    row: Cell<f64>,
    /// The command palette while it is open: over everything the window holds, the side
    /// column's edge included ([`crate::palette`]).
    palette: RefCell<Option<Retained<PaletteScrim>>>,
}

/// Whether `[appearance] tab_position` puts the tabs in the side column. One answer for every
/// window; without an application (a pane embedded in another one) the bar in the title row.
pub(crate) fn tabs_at_side(mtm: MainThreadMarker) -> bool {
    app::delegate(mtm).is_some_and(|app| app.settings().tab_position == TabPosition::Side)
}

define_class!(
    // SAFETY: NSView is designed for subclassing; RootView implements no
    // `Drop` and is born with `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriWindowRoot"]
    #[ivars = RootIvars]
    pub(crate) struct RootView;

    unsafe impl NSObjectProtocol for RootView {}

    impl RootView {
        /// Top-down coordinates, like the bar and the containers.
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        /// The window's size changed: the bar and every container follow at
        /// the row's last height — what AppKit reports here is the previous
        /// size's (the header) — and the layout pass measures it again.
        #[unsafe(method(resizeSubviewsWithOldSize:))]
        fn resize_subviews(&self, _old: NSSize) {
            self.place(self.ivars().row.get());
            self.setNeedsLayout(true);
            // The palette keeps its place while it is open, but not the window's old size.
            if let Some(palette) = self.palette() {
                palette.refit(self.content());
            }
        }

        /// AppKit's layout pass: the window's chrome is settled, so the row
        /// is measured and everything placed by it.
        #[unsafe(method(layout))]
        fn layout_pass(&self) {
            // SAFETY: `NSView`'s argumentless method returning nothing.
            let _: () = unsafe { msg_send![super(self), layout] };
            self.lay_out();
        }
    }
);

impl RootView {
    fn new(mtm: MainThreadMarker, frame: NSRect, bar: &TabBar, side_width: f64) -> Retained<Self> {
        let ground = Ground::new(mtm, NSRect::new(NSPoint::ZERO, frame.size));
        let shade = Shade::new(mtm, NSRect::new(NSPoint::ZERO, frame.size));
        // Born unseen, not hidden: it fades in and out like the ground.
        let rule = Hairline::new(mtm);
        rule.setHidden(false);
        rule.setAlphaValue(0.0);
        let claim = RowClaim::new(mtm);
        claim.setHidden(true);
        let edge = SideEdge::new(mtm);
        let this = Self::alloc(mtm).set_ivars(RootIvars {
            bar: bar.retain(),
            ground: ground.clone(),
            shade: shade.clone(),
            rule: rule.clone(),
            ruled: Cell::new(false),
            claim: claim.clone(),
            edge: edge.clone(),
            side_width: Cell::new(side_width),
            row: Cell::new(0.0),
            palette: RefCell::new(None),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        // The panes' Metal layers sit under it: a layer-backed tree all the
        // way up, so their compositing mode does not change.
        this.setWantsLayer(true);
        this.addSubview(&ground);
        this.addSubview(&shade);
        this.addSubview(&rule);
        this.addSubview(bar);
        this.addSubview(&claim);
        this.addSubview(&edge);
        this
    }

    /// The side column's line takes the theme's dividers' tone (`Theme::separator_srgb`, the
    /// split's lines and the cards' frames). `NSColor` takes sRGB.
    pub(crate) fn paint_rule(&self, theme: &Theme) {
        let [r, g, b] = theme.separator_srgb().map(|byte| f64::from(byte) / 255.0);
        self.ivars()
            .rule
            .setFillColor(&NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, 1.0));
    }

    /// The ground follows the tab on screen: shown under a split one, gone
    /// under one pane or a zoomed pane. Asked after every switch and every
    /// container layout; a no-op when nothing changed.
    pub(crate) fn sync_ground(&self) {
        let shown = self.subviews().iter().find_map(|view| {
            view.downcast::<SplitView>()
                .ok()
                .filter(|container| !container.isHidden() && container.carded())
        });
        let shade = &self.ivars().shade;
        let cards = shown.as_ref().map_or_else(Vec::new, |container| {
            container
                .panes()
                .iter()
                .filter(|pane| !pane.isHidden())
                .map(|pane| {
                    let at = shade.convertRect_fromView(pane.frame(), Some(container));
                    (at, pane.shows_focus())
                })
                .collect()
        });
        shade.set_cards(cards);
        let still = crate::app::delegate(self.mtm()).is_some_and(|app| app.reduce_motion());
        let on_screen = self.window().is_some_and(|window| window.isVisible());
        self.ivars()
            .ground
            .show(shown.is_some(), still || !on_screen);
        // The side column's line is the ground's counterpart: it stands while the content is
        // not cards on the ground (one pane, a zoomed pane, a lined split) and fades out as the
        // ground comes in, so the column is never both ruled off and on the ground.
        let ruled = shown.is_none() && tabs_at_side(self.mtm());
        if self.ivars().ruled.replace(ruled) != ruled {
            fade_to(&self.ivars().rule, ruled, still || !on_screen);
            // A line the window first showed transparent — a window restored on a split tab —
            // was never drawn, and the box draws its fill again only when the fill changes:
            // given again as it comes in, or it would come in as a clear box.
            if ruled {
                let rule = &self.ivars().rule;
                let fill = rule.fillColor();
                rule.setFillColor(&NSColor::clearColor());
                rule.setFillColor(&fill);
            }
        }
    }

    /// The cards slide for `secs`: the shade waits it out.
    pub(crate) fn hold_shade(&self, secs: f64) {
        self.ivars().shade.hold(secs);
    }

    /// The tab on screen is lifted for arranging, or set down.
    pub(crate) fn shade_lifted(&self, lifted: bool) {
        self.ivars().shade.set_lifted(lifted);
    }

    /// The title row's height: the content layout rect's top inset — what
    /// the compact toolbar leaves above the content. Kept when the window
    /// reports none (full screen with the toolbar hidden). Only outside a
    /// frame change, where the rect is settled ([`RootIvars`]).
    fn title_row(&self) -> f64 {
        let Some(window) = self.window() else {
            return self.ivars().row.get();
        };
        let layout = window.contentLayoutRect();
        let inset = self.frame().size.height - (layout.origin.y + layout.size.height);
        if inset > 0.0 {
            self.ivars().row.set(inset);
        }
        self.ivars().row.get()
    }

    /// What the tabs keep of a content `width` wide: the width of the side column, or the
    /// height of the title row the bar stands in. The content is the rest.
    pub(crate) fn tabs_inset(&self, width: f64) -> NSSize {
        if tabs_at_side(self.mtm()) {
            NSSize::new(self.column_for(width), 0.0)
        } else {
            NSSize::new(0.0, self.title_row())
        }
    }

    /// The side column's width in a content `width` wide: the width the user left it at, fitted
    /// to the window ([`tabs::side_width`]).
    fn column_for(&self, width: f64) -> f64 {
        tabs::side_width(self.ivars().side_width.get(), width)
    }

    /// The side column's width as it stands now.
    pub(crate) fn column_width(&self) -> f64 {
        self.column_for(self.bounds().size.width)
    }

    /// The width the user left the side column at ([`RootIvars::side_width`]).
    pub(crate) fn side_width(&self) -> f64 {
        self.ivars().side_width.get()
    }

    /// The side column is to be `width` wide: kept, and everything placed again when it changed.
    pub(crate) fn set_side_width(&self, width: f64) {
        let width = width.clamp(tabs::SIDE_MIN, tabs::SIDE_MAX);
        if self.ivars().side_width.replace(width) != width {
            self.lay_out();
        }
    }

    /// Measures the title row and puts the bar where the tabs are, every container beside or
    /// under it.
    pub(crate) fn lay_out(&self) {
        self.place(self.title_row());
    }

    /// Puts the tabs where `[appearance] tab_position` says and every container in the rest;
    /// a question up in a tab follows its container (`sheets::fit_owner`).
    ///
    /// In the title row (`row` tall) the bar is the row's width and the containers are under it.
    /// In the side column it is the window's height and the column's width at the left
    /// ([`RootView::column_for`]), the containers beside it from the window's top — there is no
    /// title row over the content — the row's band over them is claimed, so a press there is the
    /// pane's (which moves the window from an empty spot, `tabs::side_grab`), and the column's
    /// last [`EDGE_GRIP`] is its resizing edge.
    fn place(&self, row: f64) {
        let bounds = self.bounds();
        let row = row.min(bounds.size.height);
        // The whole view, the title row too: this view places its subviews
        // itself (`resizeSubviewsWithOldSize:`), so no autoresizing reaches it.
        self.ivars().ground.setFrame(bounds);
        self.ivars().shade.setFrame(bounds);
        let bar = &self.ivars().bar;
        let claim = &self.ivars().claim;
        let side = tabs_at_side(self.mtm());
        let column = side.then(|| self.column_for(bounds.size.width));
        let below = content_frame(bounds.size, row, column);
        let bar_frame = if let Some(column) = column {
            self.ivars().edge.setFrame(NSRect::new(
                NSPoint::new((column - EDGE_GRIP).max(0.0), 0.0),
                NSSize::new(EDGE_GRIP.min(column), bounds.size.height),
            ));
            claim.setFrame(NSRect::new(
                NSPoint::new(column, 0.0),
                NSSize::new(bounds.size.width - column, row),
            ));
            NSRect::new(NSPoint::ZERO, NSSize::new(column, bounds.size.height))
        } else {
            NSRect::new(NSPoint::ZERO, NSSize::new(bounds.size.width, row))
        };
        claim.setHidden(!side);
        self.ivars().edge.setHidden(!side);
        bar.set_side(side.then_some(row));
        bar.setFrame(bar_frame);
        if side {
            // One device pixel, along the column's last: the content starts right after it.
            let scale = self
                .window()
                .map_or(2.0, |window| window.backingScaleFactor());
            let line = 1.0 / scale;
            self.ivars().rule.setFrame(NSRect::new(
                NSPoint::new((bar_frame.size.width - line).max(0.0), 0.0),
                NSSize::new(line, bounds.size.height),
            ));
        }
        for view in self.subviews() {
            if let Ok(container) = view.downcast::<SplitView>() {
                container.setFrame(below);
                sheets::fit_owner(&container);
            }
        }
        bar.lay_out();
        // The line is the side column's: it comes and goes with the place of the tabs, and the
        // containers laid out above may have been at the frame they already had.
        self.sync_ground();
    }

    /// The part of the window the tabs' containers fill, in this view's (flipped) space: all
    /// of it below the title row, or what is right of the side column. Where the command
    /// palette stands is measured from it ([`crate::palette`]).
    pub(crate) fn content(&self) -> NSRect {
        let size = self.bounds().size;
        let row = self.ivars().row.get().min(size.height);
        let column = tabs_at_side(self.mtm()).then(|| self.column_for(size.width));
        content_frame(size, row, column)
    }

    /// The command palette, while it is open.
    pub(crate) fn palette(&self) -> Option<Retained<PaletteScrim>> {
        self.ivars().palette.borrow().clone()
    }

    /// Seats the palette that was put up, or clears the seat when it is taken down.
    pub(crate) fn seat_palette(&self, palette: Option<Retained<PaletteScrim>>) {
        self.ivars().palette.replace(palette);
    }

    /// A tab's container joins under the bar, at the content's frame; under the title row's
    /// claim too, which stays the top of the stack.
    fn add_container(&self, container: &SplitView) {
        let claim: &NSView = &self.ivars().claim;
        self.addSubview_positioned_relativeTo(container, NSWindowOrderingMode::Below, Some(claim));
        self.lay_out();
    }
}

/// The rectangle the tabs' containers fill in a window `size` big, with the title row `row`
/// tall: all of the window under the row, or — with the tabs in the side column, `column` wide —
/// what is right of it, from the window's top.
pub(crate) fn content_frame(size: NSSize, row: f64, column: Option<f64>) -> NSRect {
    match column {
        Some(column) => NSRect::new(
            NSPoint::new(column, 0.0),
            NSSize::new(size.width - column, size.height),
        ),
        None => NSRect::new(
            NSPoint::new(0.0, row),
            NSSize::new(size.width, size.height - row),
        ),
    }
}

/// How far the side column's resizing edge reaches in from the column's right side: the column's
/// own empty padding right of the rows ([`tabs::EDGE`]), so the edge takes no press from a row,
/// and none from a pane.
const EDGE_GRIP: f64 = tabs::EDGE;

/// Where a press on the column's edge started: the pointer's x in the window and the column's
/// width at that moment.
#[derive(Clone, Copy)]
struct Grab {
    x: f64,
    width: f64,
}

pub(crate) struct EdgeIvars {
    /// The press being dragged; `None` between drags.
    grab: Cell<Option<Grab>>,
}

define_class!(
    /// The side column's right edge: dragging it makes the column wider or narrower, a double
    /// click brings it back to the default ([`tabs::SIDE_WIDTH`]) — the way a Finder or Mail
    /// sidebar's edge does. Nothing is drawn; the resize cursor is the cue.
    ///
    /// It is the root view's top subview, over the column's empty right padding, so the bar's
    /// own `mouseDown:` (which moves the window from the column's empty part) never sees the
    /// press. In the title row's band the bar's claim lies under it (`RowClaim`), so the window
    /// server does not move the window from there either.
    ///
    /// The terminal reflows as the column moves, as it does while the window is resized; under
    /// `[appearance] resize_in_steps` the column moves a whole cell at a time
    /// ([`TerminalWindow::drag_side_to`]). The layout is saved once, when the press ends.
    // SAFETY: NSView is designed for subclassing; SideEdge implements no
    // `Drop` and is born with `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriSideEdge"]
    #[ivars = EdgeIvars]
    pub(crate) struct SideEdge;

    unsafe impl NSObjectProtocol for SideEdge {}

    impl SideEdge {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        /// Like the bar beside it: the first click on an inactive window takes the edge.
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        /// The edge's press resizes the column; it never moves the window.
        #[unsafe(method(mouseDownCanMoveWindow))]
        fn can_move_window(&self) -> bool {
            false
        }

        #[unsafe(method(resetCursorRects))]
        fn reset_cursor_rects(&self) {
            self.addCursorRect_cursor(self.bounds(), &edge_cursor());
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let Some(window) = self.terminal_window() else {
                return;
            };
            if event.clickCount() >= 2 {
                self.ivars().grab.set(None);
                window.drag_side_to(tabs::SIDE_WIDTH);
                window.layout_changed();
                return;
            }
            edge_cursor().set();
            self.ivars().grab.set(Some(Grab {
                x: event.locationInWindow().x,
                width: window.column_width(),
            }));
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            let (Some(grab), Some(window)) = (self.ivars().grab.get(), self.terminal_window())
            else {
                return;
            };
            // The pointer may outrun the edge it holds: the cursor stays the edge's.
            edge_cursor().set();
            window.drag_side_to(grab.width + event.locationInWindow().x - grab.x);
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, _event: &NSEvent) {
            if self.ivars().grab.take().is_some()
                && let Some(window) = self.terminal_window()
            {
                window.layout_changed();
            }
        }
    }
);

impl SideEdge {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(EdgeIvars {
            grab: Cell::new(None),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.setHidden(true);
        this.setAccessibilityElement(false);
        this
    }

    /// The window the edge belongs to, found by its `NSWindow`.
    fn terminal_window(&self) -> Option<Retained<TerminalWindow>> {
        let window = self.window()?;
        app::delegate(self.mtm())?.window_owning(&window)
    }
}

/// The edge's cursor: the column resize cursor, the older left-right one before macOS 15
/// (`pointer::cursor`, a program's `col-resize`).
fn edge_cursor() -> Retained<NSCursor> {
    pointer::cursor(PointerShape::ColResize)
}

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
    /// read **after** the window is born (so a notice can reach the bar) and
    /// **before** the geometry — the font setting determines the
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
        launch: embed::Config,
    ) -> Result<Retained<Self>, GpuError> {
        let run = launch.run;
        let pane = embed::open(mtm, initial_rect(), launch)?;
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
        let tab = TerminalTab::new(mtm, tab, id, pane);
        let this = Self::with_tab(mtm, id, run, tab, None, tabs::SIDE_WIDTH);
        // The content's size changes independently of the window too (the
        // title row); so the geometry comes from the view's own notification,
        // its observer the pane (`TerminalPane::observe_frame`). The
        // constructor's last step: the earlier steps' layout must not make the
        // geometry be built before the window is ready.
        pane.observe_frame();
        this
    }

    /// The window around tab `tab`, which becomes its only tab and takes
    /// the keyboard to its focused pane. The tab is new
    /// ([`TerminalWindow::with_pane`]) or one that left another window
    /// ([`TerminalWindow::release_tab`]): its panes' observers, if it is that
    /// one, are already set. `frame` is the size (and place) of the window
    /// before its tab joins, and `side_width` its side column's
    /// ([`TerminalWindow::set_side_width`]): a moved tab's panes then meet
    /// their final size once, and their programs one resize, not two.
    pub(crate) fn with_tab(
        mtm: MainThreadMarker,
        id: u64,
        run: Option<Run>,
        tab: Retained<TerminalTab>,
        frame: Option<NSRect>,
        side_width: f64,
    ) -> Retained<Self> {
        let rect = initial_rect();
        tab.moved_to(id);
        let pane = tab.focused_pane();
        // The content reaches under the title row: the tab bar is drawn in it.
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable
            | NSWindowStyleMask::Resizable
            | NSWindowStyleMask::FullSizeContentView;
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
        // **macOS's own tabs are off**: the window carries its tabs itself
        // (the module header). Without this AppKit would still gather a new
        // window into a tab of its own bar under the system's "Prefer tabs"
        // setting — which ⌘N honours by hand instead (`AppDelegate`).
        window.setTabbingMode(NSWindowTabbingMode::Disallowed);
        // The title row: the title itself hidden (the bar draws it), and an
        // empty compact toolbar that raises the row and centres the traffic
        // lights in it. The identifier is the window's own: toolbars sharing
        // one are kept in step by AppKit, and hiding a full-screen window's
        // would hide every window's.
        window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
        let toolbar = NSToolbar::initWithIdentifier(
            NSToolbar::alloc(mtm),
            &NSString::from_str(&format!("bateri.title-row.{id}")),
        );
        window.setToolbar(Some(&toolbar));
        window.setToolbarStyle(NSWindowToolbarStyle::UnifiedCompact);
        let bar = TabBar::new(mtm, id);
        let root = RootView::new(mtm, rect, &bar, side_width);
        // The content view is the root: the bar on top, the tabs' containers
        // below; each container lays its panes out (`SplitView::layout_panes`;
        // with a single pane the whole boundary).
        window.setContentView(Some(&root));
        if let Some(frame) = frame {
            window.setFrame_display(frame, false);
        }
        root.add_container(tab.container());
        window.setTitle(ns_string!("bateri"));
        // Mouse-moved events without a button are **off** by default; an
        // application asking for mouse reporting (1003) could never see the
        // pointer without them. They reach the first responder only, which is
        // all the report, the links and the upload buttons need (their hand
        // cursor comes from `NSView`'s own cursor rect,
        // `BateriView::hand_cursor_rects`). The `NSTrackingArea`s are the
        // scroll bar's strip (`BateriView::track_scrollbar_strip`) and the
        // tab bar's hover: they must see an unfocused view and the pointer
        // leaving, which this does not. Turning these on and off by mode would
        // want broadcasting the mode to `bt-shell-macos`.
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
            root,
            toolbar,
            order: RefCell::new(Tabs::new(tab.id())),
            groups: RefCell::new(Groups::new()),
            tabs: RefCell::new(vec![tab]),
            retired: RefCell::new(Vec::new()),
            notice: RefCell::new(String::new()),
            chrome: Cell::new(None),
            alert: RefCell::new(None),
        });
        // SAFETY: NSObject's init takes no arguments and the ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        // The ivars are filled before the delegate is attached: a window
        // notification falling in between must not find the geometry empty and
        // draw with a stale size. The delegate property is weak; the owner is
        // `AppDelegate`'s window list.
        window.setDelegate(Some(ProtocolObject::from_ref(&*this)));
        this.observe_focus();
        this.refresh_bar();
        this
    }

    /// Session restore's **single** setup path for a window and its first
    /// tab `tab`: every pane is born ([`tab::restored_panes`]), laid out in
    /// the saved `shape` with its ratios at once ([`TerminalTab::adopt`]), the
    /// window is placed by the caller (`place`: the list, the theme, the
    /// frame — the application's business) and only **then** do the shells
    /// start ([`TerminalTab::start_restored`]), so each sees its final size
    /// in its first `TIOCSWINSZ` and the replayed history wraps once. The
    /// live handover hands file descriptors here instead of shells. The
    /// window's later tabs come back through [`TerminalWindow::restore_tab`].
    ///
    /// `launches` is indexed by `saved`'s shape's leaves. If no shell starts
    /// the window closes and the error returns.
    pub(crate) fn restore(
        mtm: MainThreadMarker,
        id: u64,
        tab: u64,
        saved: &SavedTab,
        launches: Vec<embed::Config>,
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
        tab.set_name(saved.name.clone());
        tab.adopt(tree, extra);
        place(&this);
        if let Err(e) = tab.start_restored(mtm, &ids, saved.focused, saved.zoomed) {
            this.close();
            return Err(e);
        }
        this.refresh_title();
        tab.refresh_look();
        Ok(this)
    }

    /// Session restore's later tabs: tab `tab` joins this already placed
    /// window ([`TerminalWindow::add_tab`]) with its panes born and laid
    /// out the way [`TerminalWindow::restore`] does it, then its shells
    /// start — the container is attached and sized, so each shell sees its
    /// final size first. The theme and the top edge are the window's. A tab
    /// whose shells do not start leaves again and the error returns.
    pub(crate) fn restore_tab(
        &self,
        mtm: MainThreadMarker,
        tab: u64,
        saved: &SavedTab,
        launches: Vec<embed::Config>,
        look: (Theme, ContentEdge),
    ) -> Result<Retained<TerminalTab>, String> {
        let (tree, panes) = tab::restored_panes(mtm, &saved.shape, launches)?;
        let ids: Vec<u64> = panes.iter().map(|pane| pane.id()).collect();
        let (first, extra) = panes
            .split_first()
            .ok_or_else(|| "a saved tab without panes".to_owned())?;
        let tab = TerminalTab::new(mtm, tab, self.id(), first);
        tab.set_name(saved.name.clone());
        self.add_tab(&tab, look);
        first.observe_frame();
        tab.adopt(tree, extra);
        if let Err(e) = tab.start_restored(mtm, &ids, saved.focused, saved.zoomed) {
            self.close_tab_now(tab.id());
            return Err(e);
        }
        self.refresh_title();
        tab.refresh_look();
        Ok(tab)
    }

    /// The `NSWindow` — session restore reads its frame.
    pub(crate) fn ns_window(&self) -> &NSWindow {
        &self.ivars().window
    }

    /// The window's tab bar — its delayed jobs find it again through the
    /// window by id (`tab_bar`'s clock, card and hints).
    pub(crate) fn bar(&self) -> &TabBar {
        &self.ivars().root.ivars().bar
    }

    /// Brings a restored window to the front at its saved `frame` (already
    /// clamped onto a visible screen by the caller).
    pub(crate) fn show_at(&self, frame: NSRect) {
        let window = &self.ivars().window;
        window.setFrame_display(frame, false);
        window.makeKeyAndOrderFront(None);
        self.ivars().root.lay_out();
    }

    /// Makes the window key and brings it to the front — the restored key
    /// window.
    pub(crate) fn select(&self) {
        self.ivars().window.makeKeyAndOrderFront(None);
    }

    pub(crate) fn id(&self) -> u64 {
        self.ivars().id
    }

    /// The window's tabs, in strip order.
    pub(crate) fn tabs(&self) -> Vec<Retained<TerminalTab>> {
        let tabs = self.ivars().tabs.borrow();
        self.ivars()
            .order
            .borrow()
            .ids()
            .iter()
            .filter_map(|id| tabs.iter().find(|tab| tab.id() == *id).cloned())
            .collect()
    }

    /// The window's tab with id `id`.
    pub(crate) fn tab(&self, id: u64) -> Option<Retained<TerminalTab>> {
        self.ivars()
            .tabs
            .borrow()
            .iter()
            .find(|tab| tab.id() == id)
            .cloned()
    }

    /// How many tabs the window carries.
    pub(crate) fn tab_count(&self) -> usize {
        self.ivars().order.borrow().len()
    }

    /// The arrangement moment ([`crate::arrange`]): lifts the selected tab,
    /// or — setting down — every tab, since the selection may have moved
    /// while the keys were held.
    pub(crate) fn arrange(&self, on: bool, animate: bool) {
        if on {
            self.selected_tab().arrange(true, animate);
        } else {
            for tab in self.tabs() {
                tab.arrange(false, animate);
            }
        }
    }

    /// A capsule's tool, pressed on `pane`: the pane takes the focus, then
    /// the work is the menu's — a split from it, the move to a tab of its own
    /// beside this one, the close that asks if it must. Through the same
    /// callers as ⌘D, Move Split to New Tab and ⌘W, so nothing here is a
    /// second way to change the tab list.
    pub(crate) fn arrange_act(&self, pane: u64, tool: Tool) {
        let Some(tab) = self.tab_holding(pane) else {
            return;
        };
        let Some(target) = tab
            .panes()
            .into_iter()
            .find(|candidate| candidate.id() == pane)
        else {
            return;
        };
        tab.focus_pane(&target);
        match tool {
            Tool::SplitRight | Tool::SplitDown => {
                if let Some(axis) = tool.axis() {
                    tab.split(axis);
                }
            }
            Tool::NewTab => {
                let gap = self.index_of(tab.id()).map_or(0, |index| index + 1);
                self.make_move(Move::PaneToNewTab {
                    pane,
                    window: self.id(),
                    gap,
                    target: self.beside_target(tab.id()),
                });
            }
            Tool::Close => {
                if tab.panes().len() > 1 {
                    self.close_pane_asking(&tab);
                } else {
                    self.close_tab_asking(tab.id());
                }
            }
        }
    }

    /// Tab `id`'s place in the strip; `None` if it is not this window's.
    pub(crate) fn index_of(&self, id: u64) -> Option<usize> {
        self.ivars().order.borrow().index_of(id)
    }

    /// The tab on screen: the title, the menu's split actions and the
    /// inheritance of a new tab or window are its.
    pub(crate) fn selected_tab(&self) -> Retained<TerminalTab> {
        self.try_selected_tab()
            // audit: the order is never empty while the window lives — closing
            // the last tab closes the window (`close_tab_now`), and so does
            // giving it up (`release_tab`, whose caller closes the window in
            // the same call) — and every id in it has its object (the applier
            // adds and removes both).
            .expect("a window has a selected tab")
    }

    /// The selected tab, `None` only in the moment a window has given up its
    /// last tab and is about to close ([`Self::release_tab`]). What AppKit
    /// calls back with while the tab leaves reads this one.
    pub(crate) fn try_selected_tab(&self) -> Option<Retained<TerminalTab>> {
        let selected = self.ivars().order.borrow().selected();
        selected.and_then(|id| self.tab(id))
    }

    /// The tab a tab action of a menu acts on: the one its item names — `None`
    /// if it closed while the menu was open, and then the action is nothing,
    /// not an action on another tab — or, for an item that names none (the
    /// menu bar's), the selected one.
    fn menu_tab(&self, sender: Option<&AnyObject>) -> Option<u64> {
        match tagged_tab(sender) {
            Some(id) => self.tab(id).map(|_| id),
            None => self.try_selected_tab().map(|tab| tab.id()),
        }
    }

    /// The tab an item of a submenu acts on: the one its parent item names (the chip menu's
    /// Move to Group ▸ is tagged with its tab), else the selected one (the menu bar's).
    fn submenu_tab(&self, sender: Option<&AnyObject>) -> Option<u64> {
        let item = sender?.downcast_ref::<NSMenuItem>()?;
        // SAFETY: reading the item's parent, on the main thread.
        let parent = unsafe { item.parentItem() };
        match parent.and_then(|parent| tabs::tab_of_menu_tag(parent.tag())) {
            Some(id) => self.tab(id).map(|_| id),
            None => self.try_selected_tab().map(|tab| tab.id()),
        }
    }

    /// What the group items of the menus say of themselves: whether they are offered, their
    /// checkmark and title. `None` for an item that is not a group's.
    fn validate_group_item(&self, item: &NSMenuItem) -> Option<bool> {
        let action = item.action()?;
        if action == sel!(moveTabToNewGroup:) {
            return Some(true);
        }
        if action == sel!(renameGroup:) || action == sel!(moveGroupToNewWindow:) {
            // The group of the tab on screen, or the header's own: none for an ungrouped tab.
            return Some(self.menu_group_of(item));
        }
        if action == sel!(removeTabFromGroup:) {
            let tab = self.menu_tab_of(item)?;
            return Some(self.ivars().groups.borrow().group_of(tab).is_some());
        }
        if action == sel!(moveTabToGroup:) {
            // The group its own tab is in is checked and grey: it is where the tab is.
            let tab = self.submenu_tab_of(item)?;
            let own = self
                .ivars()
                .groups
                .borrow()
                .group_of(tab)
                .map(|group| isize::try_from(group.get()).unwrap_or(isize::MAX));
            let here = own == Some(item.tag());
            item.setState(if here {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });
            return Some(!here);
        }
        if action == sel!(movePickedToGroup:) {
            // Where every picked tab already is, the group is checked and grey.
            let picks = self.bar().picks();
            let groups = self.ivars().groups.borrow();
            let here = !picks.is_empty()
                && picks.iter().all(|&tab| {
                    groups
                        .group_of(tab)
                        .is_some_and(|group| isize::try_from(group.get()) == Ok(item.tag()))
                });
            item.setState(if here {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });
            return Some(!here);
        }
        if action == sel!(selectGroup:) {
            let tag = u8::try_from(item.tag()).ok()?;
            let groups = self.ivars().groups.borrow();
            let named = match tag {
                0 => Some("Ungrouped".to_owned()),
                tag => tabs::tab_index(tag, groups.len())
                    .map(|index| groups.groups()[index].name.clone()),
            };
            item.setTitle(&NSString::from_str(&match (&named, tag) {
                (Some(name), _) => name.clone(),
                (None, 9) => "Last Group".to_owned(),
                (None, tag) => format!("Group {tag}"),
            }));
            return Some(named.is_some());
        }
        None
    }

    /// Whether the group a bar item names exists ([`Self::menu_group`] for an item in hand).
    fn menu_group_of(&self, item: &NSMenuItem) -> bool {
        let object: &AnyObject = item;
        self.menu_group(Some(object)).is_some()
    }

    /// [`Self::menu_tab`] for an item in hand.
    fn menu_tab_of(&self, item: &NSMenuItem) -> Option<u64> {
        let object: &AnyObject = item;
        self.menu_tab(Some(object))
    }

    /// [`Self::submenu_tab`] for an item in hand.
    fn submenu_tab_of(&self, item: &NSMenuItem) -> Option<u64> {
        let object: &AnyObject = item;
        self.submenu_tab(Some(object))
    }

    /// Whether tab `id` is the selected one — the tab the user left on
    /// screen, whether or not the window is in front.
    pub(crate) fn is_selected(&self, id: u64) -> bool {
        self.ivars().order.borrow().selected() == Some(id)
    }

    /// Every tab's running time (`TerminalTab::running_for`) — the bar's
    /// clock sets its rings and the card's seconds from these, without
    /// reading titles again.
    pub(crate) fn running_times(&self) -> Vec<(u64, Option<Duration>)> {
        self.tabs()
            .iter()
            .map(|tab| (tab.id(), tab.running_for()))
            .collect()
    }

    /// Tab `id`'s summary card (`TerminalTab::card`); `None` if it is not
    /// this window's.
    pub(crate) fn tab_card(&self, id: u64) -> Option<Card> {
        self.tab(id).map(|tab| tab.card())
    }

    /// A picture of each pane of tab `id` the card's map asks for ([`TerminalTab::card_shots`]);
    /// none if the tab is not this window's.
    pub(crate) fn tab_shots(
        &self,
        id: u64,
        tiles: &[(u64, (f64, f64))],
        scale: f64,
    ) -> Vec<(u64, Option<CFRetained<CGImage>>)> {
        self.tab(id)
            .map(|tab| tab.card_shots(tiles, scale))
            .unwrap_or_default()
    }

    /// The card is done with tab `id`'s pictures ([`TerminalTab::release_card_shots`]).
    pub(crate) fn release_tab_shots(&self, id: u64) {
        if let Some(tab) = self.tab(id) {
            tab.release_card_shots();
        }
    }

    /// Every tab's panes, tab by tab in tree order — the window-wide
    /// distributions (scale, resigning key, the close question) reach them all.
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

    /// Whether the window is visible (`occlusionState`) — the panes' links
    /// and the bar's clock ask the same.
    pub(crate) fn window_visible(&self) -> bool {
        self.ivars()
            .window
            .occlusionState()
            .contains(NSWindowOcclusionState::Visible)
    }

    /// Whether the selection may move: not while the window holds a
    /// question of its own (the close question, the application's report),
    /// which blocks the whole window ([`sheets::window_asks`]). A tab's own
    /// question does not hold it: it sits on the tab's owner and leaves the
    /// screen with its tab.
    pub(crate) fn selection_free(&self) -> bool {
        !sheets::window_asks(&self.ivars().window)
    }

    /// **The applier: selection** — the only one that leaves Undo Move's record
    /// standing (see the module header). Tab `id` comes on screen; `true` if it is
    /// (or already was) the selected one. A beep and `false` while the
    /// window holds a question of its own ([`Self::selection_free`]); a
    /// tab's question leaves the screen with its tab. ⌘1…⌘9, the next and
    /// previous tab, a press on a chip, `bateri://tab` and the close
    /// question's "the tab the eye is on" all come here.
    pub(crate) fn select_tab(&self, id: u64) -> bool {
        let current = self.ivars().order.borrow().selected();
        if current == Some(id) {
            return true;
        }
        if self.tab(id).is_none() {
            return false;
        }
        if !self.selection_free() {
            beep();
            return false;
        }
        let old = current.and_then(|current| self.tab(current));
        self.with_strip(|groups, order| groups.select(order, id));
        self.switch(old.as_deref());
        self.layout_changed();
        true
    }

    /// A press on a pane of the summary card's map: tab `tab` is selected, by the applier's rules
    /// (a question of the window's own refuses it, with a beep), and pane `pane` of it gets the
    /// keyboard — so the tab's chip turns to that pane's program. `false` if the tab was not
    /// selected.
    pub(crate) fn show_pane(&self, tab: u64, pane: u64) -> bool {
        if !self.select_tab(tab) {
            return false;
        }
        if let Some(tab) = self.tab(tab)
            && let Some(pane) = tab
                .panes()
                .into_iter()
                .find(|candidate| candidate.id() == pane)
        {
            tab.focus_pane(&pane);
        }
        true
    }

    /// **The applier: a new tab.** `tab` (its first pane born, not started)
    /// joins right of the selected one and is selected; its container goes
    /// under the bar at the content's size, so its shell's first
    /// `TIOCSWINSZ` is the final one. `look` is the window's theme and top
    /// edge — the tab's panes were born with the same, the container's
    /// separator and line take them here. The caller checked
    /// [`Self::selection_free`] before building the tab.
    pub(crate) fn add_tab(&self, tab: &TerminalTab, look: (Theme, ContentEdge)) {
        self.add_tab_seated(tab, look, &Seat::Beside);
    }

    /// [`Self::add_tab`] with the place among the groups named: right of the selected tab
    /// ([`Seat::Beside`], the rule of every plain new tab) or in the group or among the ungrouped
    /// tabs the user asked for. A group made for the tab, or one it joins, is on screen with it.
    pub(crate) fn add_tab_seated(
        &self,
        tab: &TerminalTab,
        look: (Theme, ContentEdge),
        seat: &Seat,
    ) {
        let (theme, edge) = look;
        self.forget_undo();
        let old = self.ivars().order.borrow().selected();
        let old = old.and_then(|old| self.tab(old));
        tab.set_theme(theme);
        tab.set_content_edge(edge);
        // Hidden until the switch shows it, so its panes never answer
        // "visible" from in between.
        tab.container().setHidden(true);
        self.ivars().root.add_container(tab.container());
        // The window's list is the tab's owner.
        self.ivars().tabs.borrow_mut().push(tab.retain());
        self.with_strip(|groups, order| match seat {
            Seat::Beside => groups.open(order, tab.id()),
            Seat::Loose => groups.open_loose(order, tab.id()),
            Seat::Group(group) => groups.open_in(order, tab.id(), *group),
            Seat::New(name) => {
                groups.open_in_new_group(order, tab.id(), name);
            }
        });
        self.switch(old.as_deref());
        self.layout_changed();
    }

    /// **The applier: a tab closes**, without asking (its last pane's shell
    /// exited, a confirmed question). The window's last tab closes the
    /// window. A selected tab hands the screen to its right neighbour (the
    /// left one at the end — `tabs::Tabs::close`) **before** its panes
    /// start closing, so the window is never left without a responder; its
    /// object drops a turn later — this is reached from inside its own
    /// methods.
    pub(crate) fn close_tab_now(&self, id: u64) {
        let Some(tab) = self.tab(id) else {
            return;
        };
        self.forget_undo();
        if self.tab_count() <= 1 {
            self.close();
            return;
        }
        self.leave_order(&tab);
        drop(tab.begin_close());
        tab.container().removeFromSuperview();
        self.ivars()
            .tabs
            .borrow_mut()
            .retain(|kept| kept.id() != id);
        self.retire(tab);
        self.refresh_title();
        if let Some(app) = app::delegate(self.mtm()) {
            // The closed panes' queues are gone from the Dock icon's total.
            app.refresh_dock_tile();
        }
        self.layout_changed();
    }

    /// **The applier: a name.** Tab `id` carries the name `draft` asks for
    /// ([`tabs::custom_name`]: trimmed, and none when empty or the title
    /// itself — the tab's own title shows again), the window and the bar write
    /// their titles from it, and the layout it is part of is told.
    pub(crate) fn rename_tab(&self, id: u64, draft: &str) {
        let Some(tab) = self.tab(id) else {
            return;
        };
        let automatic = tab.automatic_title().unwrap_or_default();
        let name = tabs::custom_name(draft, &automatic);
        if tab.name() == name {
            return;
        }
        self.forget_undo();
        tab.set_name(name);
        self.refresh_title();
        self.layout_changed();
    }

    /// The title tab `id` would be named from: its name, else the title its
    /// focused pane's session reports — never an upload's prefix, which is
    /// not part of what a name replaces.
    pub(crate) fn tab_title(&self, id: u64) -> Option<String> {
        self.tab(id)?.session_title()
    }

    /// The window's root view: the bar, the tabs' containers and, while it is open, the
    /// command palette.
    pub(crate) fn root(&self) -> &RootView {
        &self.ivars().root
    }

    /// View ▸ Command Palette… (⇧⌘P): opens the palette over the window's content, or takes
    /// it down.
    pub(crate) fn toggle_palette(&self) {
        palette::toggle(self);
    }

    /// Window ▸ Go to Tab… (⌘P): the palette on the tabs, or its highlight one tab on
    /// ([`palette::go_to_tab`]).
    pub(crate) fn go_to_tab(&self) {
        palette::go_to_tab(self);
    }

    /// Shell ▸ Go to Folder… (⌘O): the palette on the folders, or its highlight one folder on
    /// ([`palette::go_to_folder`]).
    pub(crate) fn go_to_folder(&self) {
        palette::go_to_folder(self);
    }

    /// Window ▸ Jump to Command…: the palette on the command history, ⏎ going to the output, or
    /// its highlight one command on ([`palette::jump_to_command`]).
    pub(crate) fn jump_to_command(&self) {
        palette::jump_to_command(self);
    }

    /// Whether the palette may open now: not while a question is up on the window or on the
    /// tab on screen — it would sit over a sheet it cannot answer — and not on a window with no
    /// tab to give the keyboard back to.
    pub(crate) fn palette_may_open(&self) -> bool {
        if sheets::window_asks(&self.ivars().window) {
            return false;
        }
        self.try_selected_tab()
            .is_some_and(|tab| sheets::question_bottom(tab.container()).is_none())
    }

    /// Takes the palette down (a choice, esc, a press outside, the window no longer key) and gives
    /// the keyboard back to the tab on screen. A no-op while it is not open.
    pub(crate) fn close_palette(&self) {
        if self.ivars().root.palette().is_none() {
            return;
        }
        palette::close(&self.ivars().root);
        self.focus_selected_tab();
    }

    /// Opens the find panel of the focused pane with `text` in it ("Find … in This Pane").
    pub(crate) fn find_in_focused_pane(&self, text: &str) {
        if let Some(tab) = self.try_selected_tab() {
            tab.focused_pane().find_text(text);
        }
    }

    /// The keyboard goes back to the selected tab's focused pane — from the
    /// name field, which gave it up.
    pub(crate) fn focus_selected_tab(&self) {
        if let Some(tab) = self.try_selected_tab() {
            let _ = self
                .ivars()
                .window
                .makeFirstResponder(Some(tab.focused_pane().view()));
        }
    }

    /// **The applier: a tab leaves** for another window — Move Tab to New
    /// Window, Merge All Windows. It is neither closed nor told to close: its
    /// panes, shells and questions go on, so it comes out of the order and the
    /// hierarchy and is handed to the window that takes it
    /// ([`Self::adopt_tab`], or a window built around it).
    ///
    /// A selected tab hands the screen on first, as in a close
    /// ([`Self::leave_order`]), so what it showed — its panes' frames, focus,
    /// popovers and a question up on its owner — goes off screen the usual way.
    /// The window's **last** tab leaves the screen by itself, the model is left
    /// empty and the caller closes the window in the same call: there is no
    /// neighbour to hand the screen to and nothing may be selected in between
    /// ([`Self::try_selected_tab`]). `None` if the tab is not here.
    pub(crate) fn release_tab(&self, id: u64) -> Option<Retained<TerminalTab>> {
        let tab = self.tab(id)?;
        self.forget_undo();
        if self.tab_count() > 1 {
            self.leave_order(&tab);
        } else {
            self.with_strip(|groups, order| groups.close(order, &[id]));
            tab.container().setHidden(true);
            tab.apply_visibility(self.window_visible());
            tab.leave_screen();
            tab.look();
        }
        tab.container().removeFromSuperview();
        self.ivars()
            .tabs
            .borrow_mut()
            .retain(|kept| kept.id() != id);
        self.refresh_bar();
        self.layout_changed();
        Some(tab)
    }

    /// **The applier: a tab joins** from another window ([`Placement`]). It comes in hidden, its
    /// panes hidden with it, and learns this window's screen
    /// ([`TerminalTab::refresh_geometry`]: the scale is read from the window the container is
    /// in, and a move between screens of the same size sends no notice). Its questions,
    /// indicators and name came with it. A layout edge.
    ///
    /// At the end and unselected (Merge All Windows) the window shows what it showed. At a place
    /// (a tab let go on the strip) the tab is selected and comes up the way a selected tab does
    /// ([`Self::switch`]) — what the window showed before leaves the screen first.
    pub(crate) fn adopt_tab(&self, tab: &Retained<TerminalTab>, placement: Placement) {
        self.forget_undo();
        tab.moved_to(self.id());
        tab.container().setHidden(true);
        self.ivars().root.add_container(tab.container());
        self.ivars().tabs.borrow_mut().push(tab.clone());
        match placement {
            Placement::End => {
                self.with_strip(|groups, order| groups.append(order, tab.id()));
                tab.refresh_geometry();
                tab.apply_visibility(self.window_visible());
                self.refresh_bar();
            }
            Placement::At(index) => {
                let old = self.ivars().order.borrow().selected();
                let old = old.and_then(|old| self.tab(old));
                // The gap the tab was carried over is the place it takes now.
                self.bar().close_gap_quietly();
                self.with_strip(|groups, order| groups.insert_at(order, tab.id(), index));
                tab.refresh_geometry();
                self.switch(old.as_deref());
            }
        }
        self.layout_changed();
    }

    /// **The applier: a place.** Tab `id` takes `index` in the strip (a tab let go on its own
    /// window's strip); what is on screen does not change. A layout edge — once for the
    /// drag, not for every place it passed on the way.
    pub(crate) fn move_tab(&self, id: u64, index: usize) {
        if self.tab(id).is_none()
            || !self.with_strip(|groups, order| groups.move_to(order, id, index))
        {
            return;
        }
        self.forget_undo();
        self.refresh_bar();
        self.layout_changed();
    }

    /// The tab that holds pane `pane`.
    pub(crate) fn tab_holding(&self, pane: u64) -> Option<Retained<TerminalTab>> {
        self.tabs()
            .into_iter()
            .find(|tab| tab.container().pane(pane).is_some())
    }

    /// The tabs a split of the selected tab can be moved to, in strip order,
    /// each with the title the menu lists it by: every tab but the selected one.
    pub(crate) fn move_targets(&self) -> Vec<(u64, String)> {
        let selected = self.ivars().order.borrow().selected();
        self.tabs()
            .iter()
            .filter(|tab| Some(tab.id()) != selected)
            .map(|tab| {
                let title = tab.session_title().unwrap_or_else(|| "bateri".to_owned());
                (tab.id(), listed_title(&title))
            })
            .collect()
    }

    /// Tells VoiceOver what happened to a split — a move shows only as
    /// panes changing places, which nothing else says.
    fn announce(&self, text: &str) {
        let message = NSString::from_str(text);
        let objects: [&AnyObject; 1] = [message.as_ref()];
        // SAFETY: AppKit's announcement key (an extern static) with the
        // `NSString` it documents.
        let info = unsafe {
            NSDictionary::<NSAccessibilityNotificationUserInfoKey, AnyObject>::from_slices(
                &[NSAccessibilityAnnouncementKey],
                &objects,
            )
        };
        let element: &AnyObject = self.ns_window();
        // SAFETY: the window is an accessibility element and the user info is
        // the announcement the notification documents.
        unsafe {
            NSAccessibilityPostNotificationWithUserInfo(
                element,
                NSAccessibilityAnnouncementRequestedNotification,
                Some(&info),
            );
        }
    }

    /// **The applier: a swap.** Panes `a` and `b` of tab `tab` trade places
    /// (⇧⌥⌘ + arrow); nothing leaves the tab. A beep and `false` if a pane
    /// would be left smaller than its smallest. A layout edge.
    pub(crate) fn swap_panes(&self, tab: u64, a: u64, b: u64) -> bool {
        let Some(tab) = self.tab(tab) else {
            return false;
        };
        let (Some(first), Some(second)) = (tab.container().pane(a), tab.container().pane(b)) else {
            return false;
        };
        let before = self.undo_scene(&[tab.id()]);
        if !tab.swap(a, b) {
            beep();
            return false;
        }
        self.announce(&format!(
            "{} swapped with {}",
            pane_name(&first),
            pane_name(&second)
        ));
        self.layout_changed();
        self.remember(vec![before], Vec::new());
        true
    }

    /// **The applier: a move inside a tab.** Pane `pane` of tab `tab` is let go
    /// beside another pane or at the window's edge, and the panes take the
    /// places of `tree` ([`Tree::verdict`]'s landing) — all of them laid out
    /// once, so a program is resized once, and sliding there. The pane never
    /// leaves its tab, so its question, its focus and its programs stay where
    /// they are. A beep and `false` if the tree is not this tab's panes or a
    /// pane would be left below its smallest. A layout edge.
    pub(crate) fn move_pane(&self, tab: u64, pane: u64, tree: Tree) -> bool {
        let Some(tab) = self.tab(tab) else {
            return false;
        };
        let Some(moved) = tab.container().pane(pane) else {
            return false;
        };
        let before = self.undo_scene(&[tab.id()]);
        if !tab.rearrange(tree) {
            beep();
            return false;
        }
        self.announce(&format!("{} moved", pane_name(&moved)));
        self.layout_changed();
        self.remember(vec![before], Vec::new());
        true
    }

    /// **The applier: a pane leaves** its tab for another place — moved, not
    /// closed: its shell, programs and questions go on ([`Self::adopt_pane`]
    /// takes it in). Not a tab's last pane: that pane is the tab
    /// ([`Self::fold_tab`], [`Self::release_tab`]); `None` then, or if it is
    /// not here. A layout edge.
    pub(crate) fn release_pane(&self, tab: u64, pane: u64) -> Option<Retained<TerminalPane>> {
        let released = self.tab(tab)?.release_pane(pane)?;
        self.refresh_bar();
        self.layout_changed();
        Some(released)
    }

    /// **The applier: panes arrive** in tab `tab` of this window, laid out as
    /// `tree` says (a plan, [`TerminalTab::plan_beside`]). The tab is not
    /// selected by it and the keyboard stays where it is; a question one of
    /// them had up opens again once the tab is on screen. `false` if the
    /// tree does not hold exactly the tab's panes and these. A layout edge.
    pub(crate) fn adopt_pane(
        &self,
        tab: &TerminalTab,
        panes: &[Retained<TerminalPane>],
        tree: Tree,
    ) -> bool {
        if !tab.receive(panes, tree) {
            return false;
        }
        for pane in panes {
            sheets::reopen_later(pane);
        }
        let name = tab.session_title().unwrap_or_else(|| "bateri".to_owned());
        let what = match panes {
            [one] => pane_name(one),
            many => format!("{} splits", many.len()),
        };
        self.announce(&format!("{what} moved to {name}"));
        self.refresh_bar();
        self.layout_changed();
        true
    }

    /// **A pane or a tab moves** to another tab or becomes one — Move Split to
    /// Tab, to Previous / Next / New Tab, a chip's Merge into Current Tab, a pane
    /// let go on a chip, between chips or in a tab's panes, a tab let go with ⌥⌘
    /// on one — in this window or another: what it comes to is
    /// [`moves::plan`]'s, carried out by the application
    /// ([`AppDelegate::make_move`]). `true` if it was made.
    pub(crate) fn make_move(&self, wanted: Move) -> bool {
        app::delegate(self.mtm()).is_some_and(|app| app.make_move(wanted))
    }

    /// **The applier: a tab folds into another** of this window ([`Step::Fold`]):
    /// it leaves the strip — `tabs::Tabs::pane_to_tab`: if it was the selected
    /// one the screen goes to `into`, not to a neighbour — and is thrown away
    /// without closing anything; its panes are handed back to join `into`.
    /// `None` if either tab is not here.
    pub(crate) fn fold_tab(&self, source: u64, into: u64) -> Option<Vec<Retained<TerminalPane>>> {
        let from = self.tab(source)?;
        let was_selected = self.is_selected(source);
        if !self.with_strip(|groups, order| groups.pane_to_tab(order, source, true, into)) {
            return None;
        }
        if was_selected {
            self.switch(Some(&from));
        }
        from.container().removeFromSuperview();
        self.ivars()
            .tabs
            .borrow_mut()
            .retain(|kept| kept.id() != source);
        let panes = from.drain();
        self.retire(from);
        Some(panes)
    }

    /// **The applier: a pane becomes a tab** of its own ([`moves::Step::NewTab`]):
    /// `pane`, taken from a tab of this window, is the first pane of tab `id`,
    /// in the strip before tab number `gap` (`tab_count()` is the end), **not**
    /// selected — the user stays where they were ([`tabs::Tabs::place_new`]).
    /// A layout edge.
    pub(crate) fn new_tab_of(&self, pane: &TerminalPane, id: u64, gap: usize) {
        let tab = self.born_tab(id, pane);
        self.with_strip(|groups, order| groups.place_new(order, id, gap));
        self.settle_born(&tab, pane);
        self.announce(&format!("{} moved to a new tab", pane_name(pane)));
        self.refresh_title();
        self.layout_changed();
    }

    /// A tab `id` born around `first`, a pane that came from another tab: it
    /// has the pane's theme and the application's top edge, and sits in the
    /// window's hierarchy and list **hidden** — so its pane never answers
    /// "visible" from in between ([`Self::add_tab`]) — but not yet in the
    /// strip, which is the caller's ([`Self::settle_born`] after it).
    fn born_tab(&self, id: u64, first: &TerminalPane) -> Retained<TerminalTab> {
        let edge = app::delegate(self.mtm()).map(|app| app.settings().content_edge);
        let tab = TerminalTab::around(self.mtm(), id, self.id(), first, edge);
        tab.container().setHidden(true);
        self.ivars().root.add_container(tab.container());
        self.ivars().tabs.borrow_mut().push(tab.clone());
        tab
    }

    /// A [`Self::born_tab`] that is in the strip learns where it is: its
    /// pane hidden with it, the screen it is on.
    fn settle_born(&self, tab: &TerminalTab, first: &TerminalPane) {
        tab.apply_visibility(self.window_visible());
        first.leave_screen();
        tab.refresh_geometry();
    }

    /// What a move writes down before it changes anything: the strip as it
    /// stands and the tabs `tabs` as they stand — name, split tree with its
    /// ratios, the pane that has the keyboard ([`Scene`]).
    pub(crate) fn undo_scene(&self, tabs: &[u64]) -> Scene {
        self.picture().scene(tabs)
    }

    /// The window as a move sees it ([`moves::Window`]): the strip, every tab's
    /// shape and whether a question of its own holds the window.
    pub(crate) fn picture(&self) -> moves::Window {
        moves::Window {
            id: self.id(),
            order: self.ivars().order.borrow().clone(),
            groups: self.ivars().groups.borrow().clone(),
            tabs: self
                .tabs()
                .iter()
                .map(|tab| Shape {
                    tab: tab.id(),
                    name: tab.name(),
                    tree: tab.container().tree(),
                    focus: tab.focused_pane().id(),
                })
                .collect(),
            asking: !self.selection_free(),
            // A window whose last tab leaves closes: bateri has no window without a tab.
            stays_empty: false,
            // The title row's bar shows one group at a time; the column lists them all.
            one_cluster: !self.bar().is_side(),
        }
    }

    /// A move is done: the record Undo Move takes back. Written by the move
    /// that ends here, **after** its last layout edge — the outermost move,
    /// not the steps it is made of ([`Self::release_pane`] with
    /// [`Self::adopt_pane`] undo nothing alone).
    fn remember(&self, scenes: Vec<Scene>, born: Vec<u64>) {
        if let Some(app) = app::delegate(self.mtm()) {
            app.remember_undo(Record { scenes, born });
        }
    }

    /// The tab list changed in a way the record does not know: it is no
    /// longer a picture of what is here ([`AppDelegate::forget_undo`]).
    /// Every applier of the tab list but the moves calls it.
    fn forget_undo(&self) {
        if let Some(app) = app::delegate(self.mtm()) {
            app.forget_undo();
        }
    }

    /// **The applier: a tab a move made comes apart** ([`moves::Step::Dissolve`],
    /// Undo Move's first half): tab `id` leaves the hierarchy and the list and
    /// is thrown away, its panes handed back — moved, not closed: their
    /// shells, programs and questions go on. Its place in the strip stays
    /// until the strip is put back ([`Self::put_strip`]). `None` if it is not
    /// here.
    pub(crate) fn dissolve_tab(&self, id: u64) -> Option<Vec<Retained<TerminalPane>>> {
        let tab = self.tab(id)?;
        tab.container().removeFromSuperview();
        self.ivars()
            .tabs
            .borrow_mut()
            .retain(|kept| kept.id() != id);
        let panes = tab.drain();
        self.retire(tab);
        Some(panes)
    }

    /// **The applier: a tab as it was** ([`moves::Step::Reshape`], Undo Move's
    /// second half): tab `id` holds exactly the panes of `tree`, in its saved
    /// layout and ratios, and its name `name` — the panes it lacks taken from
    /// `pool`; a tab the move closed is born again under its own identity,
    /// hidden and not yet in the strip ([`Self::put_strip`] puts it there). A
    /// question a pane brought opens again once its tab is on screen.
    pub(crate) fn reshape_tab(
        &self,
        id: u64,
        tree: Tree,
        name: Option<String>,
        pool: &mut Vec<Retained<TerminalPane>>,
    ) {
        let tab = if let Some(tab) = self.tab(id) {
            let have: Vec<u64> = tab.panes().iter().map(|pane| pane.id()).collect();
            let incoming = take_panes(
                pool,
                tree.leaves().into_iter().filter(|id| !have.contains(id)),
            );
            let placed = tab.receive(&incoming, tree);
            debug_assert!(placed, "the picture holds, so its tree is these panes");
            for pane in &incoming {
                sheets::reopen_later(pane);
            }
            tab
        } else {
            let mut panes = take_panes(pool, tree.leaves()).into_iter();
            let Some(first) = panes.next() else {
                return;
            };
            let rest: Vec<Retained<TerminalPane>> = panes.collect();
            let tab = self.born_tab(id, &first);
            self.settle_born(&tab, &first);
            let placed = tab.receive(&rest, tree);
            debug_assert!(placed, "the picture holds, so its tree is these panes");
            sheets::reopen_later(&first);
            for pane in &rest {
                sheets::reopen_later(pane);
            }
            tab
        };
        tab.set_name(name);
    }

    /// **The applier: the strip as it was** ([`moves::Step::PutStrip`]): the
    /// tabs in `order`, its selection and its `groups` with it — what was on
    /// screen leaves it first, the selected tab comes up ([`Self::switch`]).
    pub(crate) fn put_strip(&self, order: Tabs<u64>, groups: Groups<u64>) {
        let old = self
            .ivars()
            .order
            .borrow()
            .selected()
            .and_then(|id| self.tab(id));
        debug_assert!(
            groups.check(&order).is_ok(),
            "a picture's groups agree with its strip"
        );
        self.ivars().order.replace(order);
        self.ivars().groups.replace(groups);
        self.switch(old.as_deref());
    }

    /// **The applier: tabs regroup** ([`moves::Step::Regroup`]): `tabs` go where
    /// `target` says ([`Groups::place`]) — into a group, into a group made for
    /// them, out of any group. What is on screen does not change. A layout
    /// edge. It does not drop Undo Move's record: the record is the plan's, and
    /// is written after the step.
    pub(crate) fn regroup(&self, tabs: &[u64], target: Target<u64>) {
        if self
            .with_strip(|groups, order| groups.place(order, tabs, target))
            .is_none()
        {
            return;
        }
        self.refresh_bar();
        self.layout_changed();
    }

    /// **The applier: a group's place** ([`moves::Step::MoveGroup`]): `group`
    /// takes place `to` among the groups, its tabs with it. As
    /// [`Self::regroup`] it leaves the record to the plan.
    pub(crate) fn move_group(&self, group: GroupId, to: usize) {
        if !self.with_strip(|groups, order| groups.move_group(order, group, to)) {
            return;
        }
        self.refresh_bar();
        self.layout_changed();
    }

    /// **The applier: a group's name.** Group `group` is called what `draft` says, trimmed; an
    /// empty name keeps the old one. The strip is not touched, so this does not go through
    /// [`Self::with_strip`]; the layout it is part of is told, and a record of a move that
    /// captured the old name is dropped.
    pub(crate) fn rename_group(&self, group: GroupId, draft: &str) {
        if !self.ivars().groups.borrow_mut().rename(group, draft) {
            return;
        }
        self.forget_undo();
        self.refresh_bar();
        self.layout_changed();
    }

    /// **The applier: the name a group is born asking for.** A group a move made opens its name
    /// field on its header ([`TabBar::begin_group_rename`]); the name typed there is part of the
    /// move, not a change after it. Like [`Self::rename_group`] it names the group, but the move's
    /// record stands: it is told the name ([`AppDelegate::name_in_undo`]) so that Undo Move still
    /// takes the move back — the group and its name go — and does not put an old name back where
    /// the group was known to an earlier picture.
    pub(crate) fn name_new_group(&self, group: GroupId, draft: &str) {
        let named = {
            let mut groups = self.ivars().groups.borrow_mut();
            groups
                .rename(group, draft)
                .then(|| groups.group(group).map(|group| group.name.clone()))
                .flatten()
        };
        let Some(name) = named else {
            return;
        };
        if let Some(app) = app::delegate(self.mtm()) {
            app.name_in_undo(self.id(), group, &name);
        }
        self.refresh_bar();
        self.layout_changed();
    }

    /// **The applier: a group opens or closes** in the column. Which tab is on screen does not
    /// change: a closed group keeps listing it under its header. As [`Self::rename_group`], the
    /// strip is untouched.
    pub(crate) fn set_group_collapsed(&self, group: GroupId, collapsed: bool) {
        if !self
            .ivars()
            .groups
            .borrow_mut()
            .set_collapsed(group, collapsed)
        {
            return;
        }
        self.forget_undo();
        self.refresh_bar();
        self.layout_changed();
    }

    /// The group named `name` for the tab on screen: the folder it stands in, made unique
    /// ([`Groups::suggest_name`]).
    pub(crate) fn new_group_name(&self, tab: Option<u64>) -> String {
        let title = tab
            .and_then(|id| self.tab(id))
            .or_else(|| self.try_selected_tab())
            .and_then(|tab| tab.automatic_title())
            .unwrap_or_default();
        self.ivars().groups.borrow().suggest_name(&title)
    }

    /// What the bar lists: every group down the column, the group on screen along the title row.
    pub(crate) fn listing(&self) -> View {
        if self.bar().is_side() {
            View::Side
        } else {
            View::Top
        }
    }

    /// Where the `listed`th place of the bar's list is in the strip the appliers count
    /// ([`Groups::listed_place`]): the column's lists the tabs under open headers, the title row's
    /// the tabs of the group on screen.
    pub(crate) fn strip_place(&self, listed: usize) -> usize {
        self.ivars().groups.borrow().listed_place(
            &self.ivars().order.borrow(),
            self.listing(),
            listed,
        )
    }

    /// Where a tab opened right of tab `tab` goes: its own group, after it
    /// ([`Groups::beside_target`]); `None` in a window with no group.
    pub(crate) fn beside_target(&self, tab: u64) -> Option<Target<u64>> {
        self.ivars()
            .groups
            .borrow()
            .beside_target(&self.ivars().order.borrow(), tab)
    }

    /// Where a tab (or a pane become one) let go at gap `gap` of what the bar lists goes, in a
    /// window that has groups ([`Groups::gap_target`]): the gap above a group's header is the end
    /// of the group above it.
    pub(crate) fn gap_target(&self, gap: usize) -> Option<Target<u64>> {
        self.ivars()
            .groups
            .borrow()
            .gap_target(&self.ivars().order.borrow(), self.listing(), gap)
    }

    /// The window has groups: its side column lists headers, and a row dragged there is carried
    /// to a place of the column ([`Groups::side_spots`]), not slid along a list.
    pub(crate) fn has_groups(&self) -> bool {
        !self.ivars().groups.borrow().is_empty()
    }

    /// How many groups the window has.
    pub(crate) fn group_count(&self) -> usize {
        self.ivars().groups.borrow().len()
    }

    /// Where group `group` stands among the window's groups.
    pub(crate) fn group_position(&self, group: GroupId) -> Option<usize> {
        self.ivars().groups.borrow().position(group)
    }

    /// The places the side column `column` — as it lays out the window's list — takes carried
    /// tabs at ([`Groups::side_spots`]).
    pub(crate) fn side_spots(&self, column: &Column) -> Vec<Spot<u64>> {
        self.ivars()
            .groups
            .borrow()
            .side_spots(&self.ivars().order.borrow(), column)
    }

    /// The column's list with tab `tab` slid to slot `slot` of its spots ([`Groups::side_preview`]).
    pub(crate) fn side_preview(
        &self,
        tab: u64,
        slot: usize,
    ) -> Option<crate::groups::SidePreview<u64>> {
        self.ivars()
            .groups
            .borrow()
            .side_preview(&self.ivars().order.borrow(), tab, slot)
    }

    /// The places the side column `column` takes a carried group at ([`Groups::group_spots`]).
    pub(crate) fn group_spots(&self, column: &Column) -> Vec<GroupSpot> {
        self.ivars()
            .groups
            .borrow()
            .group_spots(&self.ivars().order.borrow(), column)
    }

    /// Where tabs let go at gap `gap` of what the bar lists go: [`Self::gap_target`], and in a
    /// window with no group, before the tab of that place among the plain list.
    pub(crate) fn drop_target(&self, gap: usize) -> Target<u64> {
        self.gap_target(gap).unwrap_or_else(|| {
            let before = self
                .ivars()
                .groups
                .borrow()
                .numbered(&self.ivars().order.borrow(), self.listing())
                .get(gap)
                .copied();
            Target::Loose { before }
        })
    }

    /// Where tab `tab` of this window, let go on its own title row at `place`, goes
    /// ([`Groups::own_target`]).
    pub(crate) fn own_drop_target(&self, tab: u64, place: usize) -> Target<u64> {
        self.ivars()
            .groups
            .borrow()
            .own_target(&self.ivars().order.borrow(), tab, place)
    }

    /// **The applier: a place in the bar's list.** Tab `id` takes the `to`th place of what the bar
    /// lists (a tab let go on its own bar). The column's list is the strip's, place by place
    /// ([`Self::move_tab`]); the title row lists one group and the tab stays in it, which a place
    /// of the strip cannot say at the group's end ([`Groups::move_within`]).
    pub(crate) fn move_tab_listed(&self, id: u64, to: usize) {
        let in_group = !self.bar().is_side() && !self.ivars().groups.borrow().is_empty();
        if !in_group {
            self.move_tab(id, self.strip_place(to));
            return;
        }
        if self.tab(id).is_none()
            || !self.with_strip(|groups, order| groups.move_within(order, id, to))
        {
            return;
        }
        self.forget_undo();
        self.refresh_bar();
        self.layout_changed();
    }

    /// The groups the menus list, in order: the group's tag (its identity), its name, and whether
    /// `tab` is in it ([`Groups::group_of`]).
    pub(crate) fn group_targets(&self, tab: Option<u64>) -> Vec<(isize, String, bool)> {
        let groups = self.ivars().groups.borrow();
        let in_group = tab.and_then(|tab| groups.group_of(tab));
        groups
            .groups()
            .iter()
            .map(|group| {
                (
                    isize::try_from(group.id.get()).unwrap_or(isize::MAX),
                    group.name.clone(),
                    in_group == Some(group.id),
                )
            })
            .collect()
    }

    /// The group a menu item names by its `tag` ([`Self::group_targets`]); a bar item (zero) names
    /// the group of the tab on screen.
    fn menu_group(&self, sender: Option<&AnyObject>) -> Option<GroupId> {
        let tag = sender?.downcast_ref::<NSMenuItem>()?.tag();
        let groups = self.ivars().groups.borrow();
        match tag {
            0 => groups.shown(&self.ivars().order.borrow()),
            tag => groups
                .groups()
                .iter()
                .find(|group| isize::try_from(group.id.get()) == Ok(tag))
                .map(|group| group.id),
        }
    }

    /// Move to Group, Move to New Group, Remove from Group: tab `tab` goes where `target` says,
    /// as the move [`moves::Move::RegroupTabs`] — the record Undo Move keeps is the plan's.
    /// `true` if it moved.
    pub(crate) fn regroup_tab(&self, tab: u64, target: Target<u64>) -> bool {
        self.make_move(Move::RegroupTabs {
            window: self.id(),
            tabs: vec![tab],
            target,
        })
    }

    /// Move Tab to New Group: the tab is the only one in a group of its own, named for its folder,
    /// and the name field opens on the header so the user can say better.
    pub(crate) fn move_to_new_group(&self, tab: u64) {
        self.move_tabs_to_new_group(&[tab]);
    }

    /// The tabs `tabs` are the tabs of a group of their own, in the order they had, named for the
    /// first one's folder; the name field opens on the header. `true` if they moved.
    pub(crate) fn move_tabs_to_new_group(&self, tabs: &[u64]) -> bool {
        let Some(&first) = tabs.first() else {
            return false;
        };
        let name = self.new_group_name(Some(first));
        let moved = self.make_move(Move::RegroupTabs {
            window: self.id(),
            tabs: tabs.to_vec(),
            target: Target::New(name),
        });
        if !moved {
            return false;
        }
        let made = self.ivars().groups.borrow().group_of(first);
        if let Some(made) = made {
            self.bar().begin_group_rename(made, true);
        }
        true
    }

    /// The tabs picked together in the side column go where `target` says, as one move
    /// ([`moves::Move::RegroupTabs`], one Undo Move); the picks let go once they have moved.
    fn regroup_picked(&self, target: Target<u64>) {
        let tabs = self.bar().picks();
        if tabs.is_empty() {
            return;
        }
        if self.make_move(Move::RegroupTabs {
            window: self.id(),
            tabs,
            target,
        }) {
            self.bar().drop_picks();
        }
    }

    /// What a ⇧ or ⌘ press on the row of `tab` comes to, given the tabs picked together now
    /// ([`Groups::pick`]).
    pub(crate) fn pick(&self, picks: &[u64], click: Click, tab: u64) -> Picked<u64> {
        let ivars = self.ivars();
        ivars
            .groups
            .borrow()
            .pick(&ivars.order.borrow(), picks, click, tab)
    }

    /// ⌘W with tabs picked together in the side column: they close, all of them, under one
    /// question that comes whatever runs in them ([`should_ask`]). `false` when nothing is
    /// picked, and ⌘W is the usual one.
    pub(crate) fn close_picked(&self) -> bool {
        let tabs = self.bar().picks();
        if tabs.is_empty() {
            return false;
        }
        self.close_tabs_with(tabs, true);
        true
    }

    /// The name field opens over the header of the group the tab on screen is in: a group that
    /// was just made opens it the way a group made by Move to New Group does.
    pub(crate) fn rename_shown_group(&self) {
        let group = self
            .ivars()
            .groups
            .borrow()
            .shown(&self.ivars().order.borrow());
        if let Some(group) = group {
            self.bar().begin_group_rename(group, true);
        }
    }

    /// ⌃⌘`tag`: the group `tag` names (⌃⌘0 the ungrouped tabs) comes on screen, on the tab the user
    /// was last on in it ([`Groups::reach`]); a closed group opens first. What it reached comes
    /// back so the caller can open a tab for ⌃⌘0 where there is none.
    pub(crate) fn select_group(&self, tag: u8) -> Reach<u64> {
        let reach = self
            .ivars()
            .groups
            .borrow()
            .reach(&self.ivars().order.borrow(), tag);
        self.arrive(reach)
    }

    /// A row of the list under the group button: the group `of` (`None`, the ungrouped tabs)
    /// comes on screen as [`Self::select_group`] brings it, and with no ungrouped tab the first
    /// row opens one.
    pub(crate) fn reach_cluster(&self, of: Option<GroupId>) {
        let reach = self
            .ivars()
            .groups
            .borrow()
            .reach_in(&self.ivars().order.borrow(), of);
        if self.arrive(reach) == Reach::NewLoose
            && let Some(app) = app::delegate(self.mtm())
        {
            app.new_tab_seated(self.id(), Seat::Loose);
        }
    }

    /// Goes where `reach` says: the tab, a closed group opening first.
    fn arrive(&self, reach: Reach<u64>) -> Reach<u64> {
        if let Reach::Tab { tab, expand } = reach {
            if let Some(group) = expand {
                self.set_group_collapsed(group, false);
            }
            self.select_tab(tab);
        }
        reach
    }

    /// ⌃⌘`tag` as the keys do it and the list under the group button does: [`Self::select_group`],
    /// and with no ungrouped tab for ⌃⌘0 a new one opens.
    pub(crate) fn reach_group(&self, tag: u8) {
        if self.select_group(tag) == Reach::NewLoose
            && let Some(app) = app::delegate(self.mtm())
        {
            app.new_tab_seated(self.id(), Seat::Loose);
        }
    }

    /// Close Group: every tab of group `group` closes, under one question when the setting asks.
    pub(crate) fn close_group(&self, group: GroupId) {
        let tabs = self
            .ivars()
            .groups
            .borrow()
            .cluster(&self.ivars().order.borrow(), Some(group));
        self.close_tabs_asking(tabs);
    }

    /// Ungroup: the group's tabs stay, among the ungrouped ones, in their order — a regrouping
    /// like any other, undone with Undo Move.
    pub(crate) fn ungroup(&self, group: GroupId) {
        let tabs = self
            .ivars()
            .groups
            .borrow()
            .cluster(&self.ivars().order.borrow(), Some(group));
        if tabs.is_empty() {
            return;
        }
        self.make_move(Move::RegroupTabs {
            window: self.id(),
            tabs,
            target: Target::Loose { before: None },
        });
    }

    /// The window's groups as they stand — what session restore saves
    /// ([`crate::restore::saved_groups`]).
    pub(crate) fn groups(&self) -> Groups<u64> {
        self.ivars().groups.borrow().clone()
    }

    /// Session restore: the groups the saved layout names, over the tabs that
    /// were built ([`crate::restore::SavedWindow::parts`]). Parts that are not
    /// a grouping of the strip leave the tabs ungrouped, as a window without
    /// groups — the layout is dropped as a whole when it is read, so this is
    /// only a guard.
    pub(crate) fn restore_groups(&self, parts: Vec<Part<u64>>) {
        if parts.is_empty() {
            return;
        }
        let groups = Groups::from_parts(&self.ivars().order.borrow(), parts);
        match groups {
            Some(groups) => {
                self.ivars().groups.replace(groups);
                self.refresh_bar();
            }
            None => eprintln!("bateri: saved tab groups did not fit the restored tabs"),
        }
    }

    /// **The strip's one door.** Runs `change` on the groups and the strip they
    /// sit beside, and — in a debug build — asks the groups' own rule whether the
    /// two still agree. Every applier that changes the tabs' order goes through
    /// here, or through [`Self::put_strip`] which puts a picture back whole; a
    /// change of the strip by itself would leave a group's tabs apart.
    fn with_strip<R>(&self, change: impl FnOnce(&mut Groups<u64>, &mut Tabs<u64>) -> R) -> R {
        let mut order = self.ivars().order.borrow_mut();
        let mut groups = self.ivars().groups.borrow_mut();
        let result = change(&mut groups, &mut order);
        debug_assert!(
            groups.check(&order).is_ok(),
            "the groups and the strip agree: {:?}",
            groups.check(&order)
        );
        result
    }

    /// Tab `id`'s panes, if they no longer fit their smallest on this
    /// window's screen, are evened out, as a restored tab's are
    /// ([`moves::Step::Fit`], [`SplitView::fits`]).
    pub(crate) fn fit_tab(&self, id: u64) {
        if let Some(tab) = self.tab(id)
            && !tab.container().fits()
        {
            tab.container().equalize();
        }
    }

    /// Undo Move has put this window back ([`moves::Step::Undone`]): its bar
    /// and title are written again, and VoiceOver says so.
    pub(crate) fn undone(&self) {
        self.refresh_bar();
        self.refresh_title();
        self.announce("Move undone");
    }

    /// A window built around a tab that came from another one is on screen:
    /// the tab learns the screen it is on and comes up the way a selected tab
    /// does ([`Self::switch`]) — its frames, focus and a question left up.
    pub(crate) fn show_arrived(&self) {
        let tab = self.selected_tab();
        tab.refresh_geometry();
        self.switch(None);
        self.layout_changed();
    }

    /// The timed run's recipe this window was born with: a window built for a
    /// moved tab has the same ([`Self::with_tab`]).
    pub(crate) fn run(&self) -> Option<Run> {
        self.ivars().run
    }

    /// Close Other Tabs: every tab but `keep` closes, under one question when
    /// the setting asks ([`foregrounds_to_ask`]); `keep` is selected first —
    /// the question is about the tabs the eye is not on, with the one that
    /// stays in front. The question is the window's, from the gate
    /// ([`Self::ask`]).
    pub(crate) fn close_other_tabs(&self, keep: u64) {
        if self.asking() || self.tab(keep).is_none() {
            return;
        }
        let others: Vec<u64> = self
            .tabs()
            .iter()
            .map(|tab| tab.id())
            .filter(|&id| id != keep)
            .collect();
        if others.is_empty() || !self.select_tab(keep) {
            return;
        }
        self.close_tabs_asking(others);
    }

    /// Closes the tabs `ids`, under one question when the setting asks
    /// ([`foregrounds_to_ask`]) — Close Other Tabs, Close Group. All of the window's tabs are the
    /// window's close ([`close_scope`]). The question is the window's, from the gate
    /// ([`Self::ask`]).
    pub(crate) fn close_tabs_asking(&self, ids: Vec<u64>) {
        self.close_tabs_with(ids, false);
    }

    /// [`Self::close_tabs_asking`], for `selected` tabs the user picked together too: that asks
    /// under `confirm_close = "running"` with nothing running ([`should_ask`]).
    fn close_tabs_with(&self, ids: Vec<u64>, selected: bool) {
        if self.asking() || ids.is_empty() {
            return;
        }
        let Some(app) = app::delegate(self.mtm()) else {
            return;
        };
        let confirm = app.settings().confirm_close;
        let panes: Vec<Retained<TerminalPane>> = ids
            .iter()
            .filter_map(|&id| self.tab(id))
            .flat_map(|tab| tab.panes())
            .collect();
        let Some(foregrounds) =
            foregrounds_to_ask_for(self.ivars().run.is_some(), confirm, selected, &panes)
        else {
            for id in ids {
                self.close_tab_now(id);
            }
            return;
        };
        let scope = close_scope(ids.len(), self.tab_count());
        let unit = unit_for(panes.len(), ids.len());
        self.ask(&prompt(scope, unit, &foregrounds), CloseTarget::Tabs(ids));
    }

    /// Move Split to Previous / Next Tab: the focused pane goes to the
    /// strip's neighbour of the selected tab, wrapping around.
    fn move_pane_to_adjacent(&self, forward: bool) {
        let target = self.ivars().order.borrow().adjacent(forward);
        if let Some(target) = target {
            let pane = self.selected_tab().focused_pane().id();
            self.make_move(Move::PaneToTab {
                pane,
                into: target,
                place: Joins::Beside(Direction::Right),
            });
        }
    }

    /// The chip menu's merge: the chip's tab (named by the item's `tag`)
    /// joins the selected tab on `side`, a turn later — the chip whose event
    /// is still being handled goes with the tab.
    fn merge_chip_tab(&self, sender: Option<&AnyObject>, side: Direction) {
        let Some(source) = self.menu_tab(sender) else {
            return;
        };
        let Some(into) = self.try_selected_tab().map(|tab| tab.id()) else {
            return;
        };
        self.later(move |window| {
            window.make_move(Move::TabToTab {
                tab: source,
                into,
                place: Joins::Beside(side),
            });
        });
    }

    /// Runs `job` on this window one main-queue turn later — what a menu
    /// action does that takes tabs, and with them the chip whose event is
    /// still being handled, out from under it. Found again by id.
    fn later(&self, job: impl FnOnce(&TerminalWindow) + Send + 'static) {
        let id = self.id();
        DispatchQueue::main().exec_async(move || {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(window) = app::delegate(mtm).and_then(|app| app.window(id)) {
                job(&window);
            }
        });
    }

    /// Takes `tab` out of the order. A selected tab hands the screen to its
    /// right neighbour (the left one at the end) at once ([`Self::switch`]),
    /// so the window is never left without a responder.
    fn leave_order(&self, tab: &TerminalTab) {
        let was_selected = self.ivars().order.borrow().selected() == Some(tab.id());
        self.with_strip(|groups, order| groups.close(order, &[tab.id()]));
        if was_selected {
            self.switch(Some(tab));
        }
    }

    /// Keeps a closed tab's object until the next main-queue turn
    /// ([`Self::close_tab_now`]).
    pub(crate) fn retire(&self, tab: Retained<TerminalTab>) {
        // The tab object is at its end (closed, folded into another, its panes taken out): it
        // keeps no place among the tabs looked at. A tab that moves to another window is not
        // retired — it keeps its identity and with it its place ([`Self::release_tab`]).
        if let Some(app) = app::delegate(self.mtm()) {
            app.forget_visit(tab.id());
        }
        self.ivars().retired.borrow_mut().push(tab);
        let id = self.id();
        DispatchQueue::main().exec_async(move || {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(window) = app::delegate(mtm).and_then(|app| app.window(id)) {
                let retired = window.ivars().retired.take();
                drop(retired);
            }
        });
    }

    /// The applier's one switch: `old` (the tab that was on screen, if it is
    /// not the selected one now) leaves the screen and the selected tab
    /// comes on it. **The order is fixed**: the containers' `setHidden`
    /// first, then the panes — visibility and focus read the hierarchy
    /// (`SplitView::apply_visibility`, `TerminalPane::is_active`).
    fn switch(&self, old: Option<&TerminalTab>) {
        let new = self.selected_tab();
        let old = old.filter(|old| old.id() != new.id());
        if let Some(old) = old {
            old.container().setHidden(true);
        }
        new.container().setHidden(false);
        self.ivars().root.sync_ground();
        let visible = self.window_visible();
        if let Some(old) = old {
            old.apply_visibility(visible);
            old.leave_screen();
            // What ended while it was on screen was seen, even if its
            // news is still in the main queue behind this switch.
            old.look();
        }
        new.apply_visibility(visible);
        let key = self.ivars().window.isKeyWindow();
        for pane in new.panes() {
            pane.apply_focus(key);
            if key {
                pane.rehover_footer();
            }
        }
        // The tab on screen in the window the user is in has been looked at: ⌘P's order. A
        // window that is not key records it when it becomes key (`windowDidBecomeKey:`), so a
        // tab that came up in the background is not a visit.
        if key {
            self.note_visit(&new);
        }
        // The keyboard to the tab's focused pane — the one it had when it
        // left the screen; the old tab's responder is hidden now.
        let _ = self
            .ivars()
            .window
            .makeFirstResponder(Some(new.focused_pane().view()));
        // What ended while it was away is seen now: its tick or dot goes
        // before the bar is drawn.
        new.look();
        self.refresh_title();
        new.refresh_look();
        // A question one of its panes asked in the background opens now.
        new.shown();
    }

    /// `bateri://tab/<id>`'s only effect:
    /// reopens the window if miniaturized, selects `tab`, makes the window
    /// key, brings the application to the front and gives the keyboard to
    /// the id's pane (in `tab`). Sends no byte to the shell. While the
    /// window holds a sheet the tab is not selected (the applier's beep); the
    /// window still comes forward.
    ///
    /// `makeKeyAndOrderFront` on a miniaturized window would only change the
    /// order and leave it in the Dock, so `deminiaturize` comes first.
    pub(crate) fn bring_to_front(&self, tab: &TerminalTab, pane: &TerminalPane) {
        let window = &self.ivars().window;
        if window.isMiniaturized() {
            window.deminiaturize(None);
        }
        let selected = self.select_tab(tab.id());
        window.makeKeyAndOrderFront(None);
        if selected {
            tab.focus_pane(pane);
        }
        NSApplication::sharedApplication(self.mtm()).activate();
    }

    /// Whether this object's `NSWindow` is that one, or one of its tabs'
    /// sheet owners (`sheets`), which stands for it — the active window is
    /// looked up in the list this way from `NSApp.keyWindow`
    /// (`AppDelegate::key_window`: a key sheet's parent).
    pub(crate) fn owns(&self, window: &NSWindow) -> bool {
        std::ptr::eq(&*self.ivars().window, window)
            || self
                .tabs()
                .iter()
                .any(|tab| sheets::is_owner(tab.container(), window))
    }

    /// Closes the window (via the `windowWillClose:` path), **without
    /// asking**: the last tab's shell exit and a confirmed close question.
    ///
    /// If there is an open question in this window it is dropped first, with
    /// a `Cancel` answer: the block waiting for the answer counts every answer
    /// other than "close" as a cancel.
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

    /// `windowShouldClose:`'s body — the red button: whether to close now.
    /// The scope is the whole window, every tab under one question; `true`
    /// lets AppKit close it at once (a timed run, nothing to ask), `false`
    /// stops it — the question is open or was already, and its answer closes.
    fn should_close_now(&self) -> bool {
        if self.ivars().run.is_some() {
            return true;
        }
        if self.asking() {
            return false;
        }
        let Some(app) = app::delegate(self.mtm()) else {
            return true;
        };
        let confirm = app.settings().confirm_close;
        let panes = self.panes();
        let Some(foregrounds) = foregrounds_to_ask(false, confirm, &panes) else {
            return true;
        };
        let unit = unit_for(panes.len(), self.tab_count());
        self.ask(
            &prompt(CloseScope::Window, unit, &foregrounds),
            CloseTarget::Window,
        );
        false
    }

    /// ⇧⌘W: a single question for the whole window, or closes at once if it
    /// will not ask.
    fn close_window_asking(&self) {
        if self.asking() {
            return;
        }
        let Some(app) = app::delegate(self.mtm()) else {
            return;
        };
        let confirm = app.settings().confirm_close;
        let panes = self.panes();
        let Some(foregrounds) = foregrounds_to_ask(self.ivars().run.is_some(), confirm, &panes)
        else {
            self.close();
            return;
        };
        let unit = unit_for(panes.len(), self.tab_count());
        self.ask(
            &prompt(CloseScope::Window, unit, &foregrounds),
            CloseTarget::Window,
        );
    }

    /// Tab `id` closes, asking if needed: ⌘W in a single-pane tab, the tab's
    /// `×`, a middle click. In the window's last tab the question is the
    /// window's. Closes at once if it will not ask.
    ///
    /// **A background tab is selected first** when it asks: "Close this
    /// tab?" must ask about the tab the eye is on. Without a question it
    /// just closes where it stands.
    pub(crate) fn close_tab_asking(&self, id: u64) {
        if self.asking() {
            return;
        }
        let (Some(tab), Some(app)) = (self.tab(id), app::delegate(self.mtm())) else {
            return;
        };
        let confirm = app.settings().confirm_close;
        let panes = tab.panes();
        let Some(foregrounds) = foregrounds_to_ask(self.ivars().run.is_some(), confirm, &panes)
        else {
            self.close_tab_now(id);
            return;
        };
        if !self.select_tab(id) {
            return;
        }
        let scope = close_scope(1, self.tab_count());
        let target = match scope {
            CloseScope::Window => CloseTarget::Window,
            _ => CloseTarget::Tabs(vec![id]),
        };
        let unit = unit_for(panes.len(), 1);
        self.ask(&prompt(scope, unit, &foregrounds), target);
    }

    /// ⌘W in a multi-pane tab: only the focused pane, asking only about the
    /// running job if there is one.
    fn close_pane_asking(&self, tab: &TerminalTab) {
        if self.asking() {
            return;
        }
        let Some(app) = app::delegate(self.mtm()) else {
            return;
        };
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

    /// Opens the question on this window as a sheet; on confirm closes the
    /// window, the tabs in `targets` or the pane.
    ///
    /// **The block captures only ids** (the alternate-screen notifier's
    /// pattern): it looks the window and tabs up at answer time and skips
    /// what it cannot find. Only `NSAlertFirstButtonReturn` closes — if the
    /// window closes while the sheet is open [`TerminalWindow::close`] drops
    /// the sheet with `Cancel`, and since `forget_window` is deferred by one
    /// turn the window can still be found in the list in the meantime.
    ///
    /// The closing is **deferred by one main-queue turn** (`windowWillClose:`'s
    /// pattern): the answer comes inside AppKit's sheet teardown and closing
    /// the window there would pull the rug from under the teardown.
    ///
    /// The question is the **window's** (its seat in [`crate::sheets`]): it
    /// asks about the window, its tabs or one of its panes. A question up in
    /// the tab on screen is in its way: a beep, nothing opens — two
    /// questions are never shown at once.
    fn ask(&self, prompt: &Prompt, targets: CloseTarget) {
        let Some(seat) = sheets::seat(Asker::Window(&self.ivars().window)) else {
            return;
        };
        if seat.is_taken() {
            beep();
            return;
        }
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
                    CloseTarget::Window => {
                        if let Some(window) = app.window(host) {
                            window.close();
                        }
                    }
                    CloseTarget::Tabs(ids) => {
                        if let Some(window) = app.window(host) {
                            for &id in ids {
                                window.close_tab_now(id);
                            }
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
        self.ivars().root.lay_out();
        self.fit_to_cells();
    }

    /// The window's frame size nearest to `size` at which the pane filling the
    /// selected tab leaves no leftover — its text keeps exactly the dock's
    /// margins and the top's strip its least height ([`TerminalPane::fitted_size`]).
    /// `None` where no single pane decides it: a split tab, full screen (the
    /// screen decides), no tab.
    fn stepped_size(&self, size: NSSize) -> Option<NSSize> {
        let window = &self.ivars().window;
        if window.styleMask().contains(NSWindowStyleMask::FullScreen) {
            return None;
        }
        let pane = self.try_selected_tab()?.filling_pane()?;
        // The tab's area is the content beside the side column or below the title row, and the
        // filling pane is all of it.
        let content = window
            .contentRectForFrameRect(NSRect::new(NSPoint::ZERO, size))
            .size;
        let inset = self.ivars().root.tabs_inset(content.width);
        let fitted = pane.fitted_size(NSSize::new(
            content.width - inset.width,
            content.height - inset.height,
        ))?;
        let content = NSSize::new(fitted.width + inset.width, fitted.height + inset.height);
        Some(
            window
                .frameRectForContentRect(NSRect::new(NSPoint::ZERO, content))
                .size,
        )
    }

    /// Fits the window to its lone pane's cells now, when
    /// `[appearance] resize_in_steps` is on: the step a drag would have
    /// taken, applied once — the top-left corner stays, the window grows or
    /// shrinks by less than a cell. A new window and the setting turned on
    /// ask; a restored or macOS-placed frame is left as it was.
    pub(crate) fn fit_to_cells(&self) {
        let on = app::delegate(self.mtm()).is_some_and(|app| app.settings().resize_in_steps);
        if !on {
            return;
        }
        let window = &self.ivars().window;
        let frame = window.frame();
        let Some(size) = self.stepped_size(frame.size) else {
            return;
        };
        if size == frame.size {
            return;
        }
        // AppKit's origin is the bottom-left corner: the top edge stays put.
        let top = frame.origin.y + frame.size.height;
        window.setFrame_display(
            NSRect::new(NSPoint::new(frame.origin.x, top - size.height), size),
            true,
        );
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

    /// Writes the titles: the window's from the selected tab
    /// ([`TerminalTab::title`]: the **focused** pane's session, an
    /// upload's prefix included — the Window menu and Mission Control show
    /// it) and every tab's label in the bar. The pane's
    /// `PaneHost::title_changed` event ([`tab::TabHost`]), the focus change
    /// ([`TerminalTab::pane_focused`]) and every change of the tab list call
    /// it. The frame path computes no title. If there is no session yet the
    /// window's title stays what it was (the constructor's `bateri`).
    pub(crate) fn refresh_title(&self) {
        if let Some(title) = self.try_selected_tab().and_then(|tab| tab.title()) {
            let listed = listed_title(&title);
            self.ivars().window.setTitle(&NSString::from_str(&listed));
        }
        self.refresh_bar();
    }

    /// Gives the bar every tab's label and indicator, the selection and the
    /// diagnostic. A single tab's label is the window's title (an upload's
    /// prefix included); among several each tab shows its session title, its
    /// indicator ([`TerminalTab::indicator`]), its running time, transfer
    /// and marked host. Every change of the tab list and of a title calls
    /// it, a command's edge in a pane (`TerminalTab::activity_changed`), the
    /// window's occlusion and Reduce Motion — the bar's clock is set again
    /// from what it is given — and the sheet gate when a tab's question
    /// starts or stops waiting (`sheets`).
    pub(crate) fn refresh_bar(&self) {
        let single = self.lone_tab();
        let side = self.bar().is_side();
        let label = |tab: &TerminalTab| Self::label_of(tab, single);
        let order = self.ivars().order.borrow();
        let groups = self.ivars().groups.borrow();
        // The column lists headers and hides a closed group's other tabs; the title row shows the
        // group on screen — or every tab, in the order of the strip, while no group exists.
        let items: Vec<Item<u64>> = if side {
            groups.side_items(&order)
        } else {
            groups
                .numbered(&order, View::Top)
                .into_iter()
                .map(Item::Tab)
                .collect()
        };
        let mut labels = Vec::with_capacity(items.len());
        let mut headings = Vec::new();
        for item in items {
            match item {
                Item::Tab(id) => labels.extend(self.tab(id).map(|tab| label(&tab))),
                Item::Header(id) => {
                    let Some(group) = groups.group(id) else {
                        continue;
                    };
                    let members = groups.cluster(&order, Some(id));
                    headings.push(Heading {
                        group: id,
                        name: group.name.clone(),
                        collapsed: group.collapsed,
                        tabs: members.len(),
                        indicator: self.cluster_indicator(&members),
                        hint: groups
                            .position(id)
                            .and_then(|at| tabs::shortcut_digit(at, groups.len())),
                        first_row: labels.len(),
                    });
                }
            }
        }
        let selected = order
            .selected()
            .and_then(|shown| labels.iter().position(|label| label.tab == shown))
            .unwrap_or(0);
        // The title row of a window with groups leads with the button that switches between them.
        let switcher = (!side && !groups.is_empty()).then(|| {
            let on_screen = groups.shown(&order);
            let rows = std::iter::once((None, "Ungrouped".to_owned(), Some(0)))
                .chain(groups.groups().iter().enumerate().map(|(at, group)| {
                    (
                        Some(group.id),
                        group.name.clone(),
                        tabs::shortcut_digit(at, groups.len()),
                    )
                }))
                .map(|(group, name, hint)| SwitchRow {
                    group,
                    name,
                    indicator: self.cluster_indicator(&groups.cluster(&order, group)),
                    hint,
                })
                .collect();
            let elsewhere: Vec<u64> = order
                .ids()
                .iter()
                .copied()
                .filter(|&tab| groups.group_of(tab) != on_screen)
                .collect();
            Switcher {
                shown: on_screen,
                name: on_screen
                    .and_then(|id| groups.group(id))
                    .map(|group| group.name.clone()),
                elsewhere: self.cluster_indicator(&elsewhere),
                rows,
            }
        });
        drop((order, groups));
        let notice = self.ivars().notice.borrow().clone();
        self.bar()
            .show(labels, selected, headings, switcher, notice);
    }

    /// What a cluster of tabs — a group's, the ungrouped ones, everything outside the group on
    /// screen — says of itself: the most urgent thing that asks for the user in any of them
    /// ([`tabs::group_indicator`] of what their panes report together). The signals of every tab
    /// are read as they are, so a failure is not hidden under a tab's own running command.
    fn cluster_indicator(&self, members: &[u64]) -> Option<tabs::Indicator> {
        let signals = tabs::Signals::union(
            members
                .iter()
                .filter_map(|&member| self.tab(member))
                .map(|member| member.signals()),
        );
        tabs::group_indicator(signals)
    }

    /// Whether the window has one tab and no group: the bar is only its title. With a group the
    /// tab is a chip beside the group button, as in any other window with groups.
    fn lone_tab(&self) -> bool {
        self.tab_count() == 1 && self.ivars().groups.borrow().is_empty()
    }

    /// One tab as the bar shows it: a single tab's label is the window's title, an upload's
    /// prefix included; among several each tab shows its session title.
    fn label_of(tab: &TerminalTab, single: bool) -> Label {
        let title = if single {
            tab.title()
        } else {
            tab.session_title()
        };
        let indicator = tab.indicator();
        Label {
            tab: tab.id(),
            title: title.unwrap_or_else(|| "bateri".to_owned()),
            indicator,
            emblem: tab.emblem(indicator),
            running: tab.running_for(),
            upload: tab.upload(),
            mark: tab.host_mark(),
        }
    }

    /// Every tab's label, in the order of the strip — closed groups' hidden tabs too, which the
    /// column does not list: what Show All Tabs lists.
    pub(crate) fn all_labels(&self) -> Vec<Label> {
        let single = self.lone_tab();
        self.tabs()
            .iter()
            .map(|tab| Self::label_of(tab, single))
            .collect()
    }

    /// `tab` is on screen in this window while it is key: ⌘P's order puts it first
    /// ([`app::AppDelegate::note_visit`]).
    fn note_visit(&self, tab: &TerminalTab) {
        if let Some(app) = app::delegate(self.mtm()) {
            app.note_visit(tab.id());
        }
    }

    /// What the command palette is told of this window's tabs and groups
    /// ([`app::AppDelegate::palette_places`]): in strip order, with the ⌘ and ⌃⌘ digits only when
    /// this is the key window (they reach this window's tabs and groups alone), and "Window
    /// `number`" for the tabs of any other. The ungrouped tabs are a row of their own only in a
    /// window that has groups.
    pub(crate) fn palette_facts(
        &self,
        key: bool,
        number: usize,
    ) -> (
        Vec<bt_shell_common::palette::TabFacts>,
        Vec<bt_shell_common::palette::GroupFacts>,
    ) {
        use bt_shell_common::palette as rules;
        let other_window = (!key).then_some(number);
        let order = self.ivars().order.borrow();
        let groups = self.ivars().groups.borrow();
        let keys = if key {
            rules::tab_keys(&groups, &order, self.listing())
        } else {
            Vec::new()
        };
        let selected = order.selected();
        let tabs = order
            .ids()
            .iter()
            .filter_map(|&id| self.tab(id))
            .map(|tab| {
                let indicator = tab.indicator();
                let group = groups
                    .group_of(tab.id())
                    .and_then(|id| groups.group(id).map(|group| (id.get(), group.name.clone())));
                rules::TabFacts {
                    tab: tab.id(),
                    window: self.id(),
                    title: tab.session_title().unwrap_or_else(|| "bateri".to_owned()),
                    report: indicator.map(|indicator| rules::Report {
                        indicator,
                        running: tab
                            .running_for()
                            .filter(|_| indicator == tabs::Indicator::Running),
                    }),
                    place: tab.place(),
                    group,
                    other_window,
                    key: keys
                        .iter()
                        .find(|(listed, _)| *listed == tab.id())
                        .map(|(_, hint)| hint.clone()),
                    here: key && selected == Some(tab.id()),
                }
            })
            .collect();
        let mut places = Vec::new();
        if !groups.is_empty() {
            let shown = groups.shown(&order);
            let mut push = |of: Option<GroupId>, name: String, position: Option<usize>| {
                let members = groups.cluster(&order, of);
                places.push(rules::GroupFacts {
                    window: self.id(),
                    group: of.map(GroupId::get),
                    name,
                    key: key
                        .then(|| rules::group_key(position, groups.len()))
                        .flatten(),
                    indicator: self.cluster_indicator(&members),
                    tabs: members.len(),
                    back_to: groups
                        .resident(of)
                        .and_then(|tab| self.tab(tab))
                        .and_then(|tab| tab.session_title()),
                    shown: shown == of,
                    other_window,
                });
            };
            push(None, "Ungrouped".to_owned(), None);
            for (at, group) in groups.groups().iter().enumerate() {
                push(Some(group.id), group.name.clone(), Some(at));
            }
        }
        (tabs, places)
    }

    /// The group of this window the palette's row names by its number ([`GroupId::get`]).
    pub(crate) fn group_numbered(&self, number: u64) -> Option<GroupId> {
        self.ivars()
            .groups
            .borrow()
            .groups()
            .iter()
            .map(|group| group.id)
            .find(|id| id.get() == number)
    }

    /// The window comes forward and is key, deminiaturized (what `bateri://` does to a window
    /// before it selects a tab).
    pub(crate) fn raise(&self) {
        let window = &self.ivars().window;
        if window.isMiniaturized() {
            window.deminiaturize(None);
        }
        window.makeKeyAndOrderFront(None);
        NSApplication::sharedApplication(self.mtm()).activate();
    }

    /// Move Group to New Window: group `group` becomes a window of its own with its name
    /// ([`Move::GroupToNewWindow`]); a window that is only that group stays.
    pub(crate) fn move_group_to_new_window(&self, group: GroupId) -> bool {
        self.make_move(Move::GroupToNewWindow {
            window: self.id(),
            group,
            at: None,
        })
    }

    /// A transfer of tab `tab` ended as `finish` says (`TerminalTab::upload_ended`): the bar plays
    /// the end of the tab's fill, once, in the layout this starts.
    pub(crate) fn transfer_ended(&self, tab: u64, finish: Finish) {
        self.bar().transfer_ended(tab, finish);
        self.refresh_bar();
    }

    /// The settings diagnostic (`AppDelegate::post_notices`, the only
    /// writer): beside a single tab's title, a `⚠` among several.
    pub(crate) fn set_notice(&self, notice: &str) {
        if *self.ivars().notice.borrow() == notice {
            return;
        }
        self.ivars().notice.replace(notice.to_owned());
        self.bar().set_notice(notice.to_owned());
    }

    /// Opens the first pane's session ([`TerminalTab::start`], from the
    /// birth package) and reads the title from the session once: a title
    /// notification that arrived before the session entered the slot may have
    /// found an empty slot and dropped; this read closes that (writes the same
    /// `bateri` if unchanged). The error returns to the caller: in the first
    /// window the process exits, in ⌘N only that window closes.
    pub(crate) fn start(&self, mtm: MainThreadMarker) -> std::io::Result<()> {
        self.selected_tab().start(mtm)?;
        self.refresh_title();
        Ok(())
    }

    /// `[remote] hosts` changed — the pattern list goes to every tab's panes
    /// ([`TerminalTab::set_host_marks`]).
    pub(crate) fn set_host_marks(&self, settings: &Settings) {
        for tab in self.tabs() {
            tab.set_host_marks(settings);
        }
        // A host's mark is a chip's top line: the bar reads them again.
        self.refresh_bar();
    }

    /// Gives the theme to the tabs ([`TerminalTab::set_theme`]: the panes'
    /// sessions and search panels, the separator), paints the chrome with
    /// it ([`TerminalWindow::apply_chrome`]) and the bar.
    ///
    /// All in a single call, because there are two paths that change the
    /// theme (`AppDelegate::reload_settings`, `AppDelegate::apply_appearance`)
    /// and if one forgot the chrome the grid would be in the new theme and the
    /// title row in the old — the symptom is exactly the seam the user would see.
    pub(crate) fn set_theme(&self, theme: Theme) {
        for tab in self.tabs() {
            tab.set_theme(theme);
        }
        self.apply_chrome(&theme);
        self.bar().set_theme(&theme);
        // An open palette wears the new theme too (a theme shown while the palette is up).
        if let Some(palette) = self.ivars().root.palette() {
            palette.set_theme(&theme);
        }
    }

    /// Gives what the content does at the panes' top edge (`[appearance]
    /// content_edge`) to every tab ([`TerminalTab::set_content_edge`]: its
    /// panes and its container's line, in one call).
    pub(crate) fn set_content_edge(&self, edge: ContentEdge) {
        for tab in self.tabs() {
            tab.set_content_edge(edge);
        }
    }

    /// Gives `[appearance] dim_unfocused_splits` to every tab: each reads the
    /// setting again and veils or unveils its unfocused panes
    /// ([`TerminalTab::refresh_look`]).
    pub(crate) fn refresh_split_look(&self) {
        for tab in self.tabs() {
            tab.refresh_look();
        }
    }

    /// The window's corners changed (full screen came or went): every tab's
    /// container is laid out again, which tells each pane the corners it now
    /// stands in ([`SplitView::layout_panes`]). The container's own frame can
    /// stay what it was across the change, so nothing else would ask.
    fn refresh_corners(&self) {
        for tab in self.tabs() {
            tab.container().layout_panes();
        }
    }

    /// `[appearance] split_style` changed: every tab's container is laid out
    /// again, as cards or divided by a line, and the ground follows the one
    /// on screen.
    pub(crate) fn apply_split_style(&self) {
        for tab in self.tabs() {
            tab.container().layout_panes();
            tab.refresh_look();
        }
    }

    /// The title row's height: the strip the traffic lights stand in, whether the bar is in it
    /// or the side column stands under it.
    pub(crate) fn title_row(&self) -> f64 {
        self.ivars().root.title_row()
    }

    /// The width the user left the side column at — what a window opened from this one starts
    /// with and what the saved layout keeps ([`RootView::side_width`]).
    pub(crate) fn side_width(&self) -> f64 {
        self.ivars().root.side_width()
    }

    /// The side column's width as it stands in this window ([`tabs::side_width`]).
    pub(crate) fn column_width(&self) -> f64 {
        self.ivars().root.column_width()
    }

    /// Gives the side column `width` (a window opened from another, a restored one), before the
    /// window is shown or its tab's programs see their size.
    pub(crate) fn set_side_width(&self, width: f64) {
        self.ivars().root.set_side_width(width);
    }

    /// The side column's edge was dragged to `wanted` (or double-clicked back to the default):
    /// the column takes it, fitted to the window. Under `[appearance] resize_in_steps` the lone
    /// pane beside it keeps whole cells, as the window's own edges do
    /// ([`TerminalWindow::stepped_size`]): the column takes the remainder, so it moves a cell at
    /// a time. Not in full screen, where the window does not step either.
    pub(crate) fn drag_side_to(&self, wanted: f64) {
        let root = &self.ivars().root;
        let size = root.bounds().size;
        let column = tabs::side_width(wanted, size.width);
        let stepping = app::delegate(self.mtm()).is_some_and(|app| app.settings().resize_in_steps)
            && !self
                .ivars()
                .window
                .styleMask()
                .contains(NSWindowStyleMask::FullScreen);
        let fitted = stepping
            .then(|| self.try_selected_tab()?.filling_pane())
            .flatten()
            .and_then(|pane| pane.fitted_size(NSSize::new(size.width - column, size.height)));
        let column = fitted.map_or(column, |fitted| {
            tabs::side_width(size.width - fitted.width, size.width)
        });
        root.set_side_width(column);
    }

    /// `[appearance] tab_position` changed: the tabs move between the title row and the side
    /// column in place, and the content takes the rest — its container's frame changes once.
    /// The content's width and height change with it, so under `resize_in_steps` the window
    /// takes the step of the new area at once ([`TerminalWindow::fit_to_cells`], a no-op when
    /// the setting is off). What the bar lists is read again: the column lists every group under
    /// its header, the title row only the group on screen, and a bar that kept the other place's
    /// list showed the title row's one cluster in the column until something else refreshed it.
    pub(crate) fn apply_tab_position(&self) {
        self.ivars().root.lay_out();
        self.refresh_bar();
        self.fit_to_cells();
    }

    /// Paints the window chrome with the theme: the
    /// title row transparent and separatorless, the window's background the
    /// theme's `background`, its appearance (traffic lights) from the
    /// background's lightness ([`is_dark_background`]).
    ///
    /// What shows under the transparent title row is the window's background,
    /// so the bar and the content are **a single surface**: the
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
    /// system's grey bar for a frame on every ⌘N.
    pub(crate) fn apply_chrome(&self, theme: &Theme) {
        // The ground derives from four roles, the rest of the chrome below
        // only from the background.
        self.ivars().root.ivars().ground.paint(theme);
        self.ivars().root.ivars().shade.paint(theme);
        self.ivars().root.paint_rule(theme);
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
        // A question up in a tab sits on a window of its own: it follows.
        for tab in self.tabs() {
            sheets::follow_appearance(tab.container(), window);
        }
    }

    /// The closing sequence's steps that fall to the window — **starts, does
    /// not wait**. It has two callers: the window's closing
    /// (`windowWillClose:`, the handles drop) and the application's closing
    /// (`AppDelegate::shutdown`, all handles waited on until a single
    /// deadline). The order is the pane's ([`TerminalPane::begin_close`]: the
    /// upload queue, rhythm, `Waker`, `SIGHUP`) and is for **every** pane of
    /// every tab ([`TerminalTab::begin_close`]); the return is one result per
    /// pane beside its id, tab by tab in tree order. Idempotent; the result
    /// of a pane whose session never came to be is `None`.
    pub(crate) fn begin_close(&self) -> Vec<(u64, Option<Closing>)> {
        self.tabs()
            .iter()
            .flat_map(|tab| tab.begin_close())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_content_is_what_the_tabs_leave() {
        let window = NSSize::new(1000.0, 738.0);
        // The tabs in the title row: everything under it.
        assert_eq!(
            content_frame(window, 38.0, None),
            NSRect::new(NSPoint::new(0.0, 38.0), NSSize::new(1000.0, 700.0))
        );
        // The tabs in the side column: what is right of it, from the window's top.
        assert_eq!(
            content_frame(window, 38.0, Some(240.0)),
            NSRect::new(NSPoint::new(240.0, 0.0), NSSize::new(760.0, 738.0))
        );
    }

    /// The guard of "selecting does not drop Undo Move": nothing but the source
    /// can tell, since the applier is AppKit's. The record is a picture that
    /// carries the selection and puts it back; a `forget_undo` in the
    /// selection applier would make "move a pane to web, open web, Undo Move"
    /// a grey item. The appliers that change the strip's shape still drop it.
    #[test]
    fn selecting_a_tab_leaves_the_undo_record_standing() {
        let source = include_str!("window.rs");
        // The text of the method that begins with `signature`, up to the next
        // method of the impl.
        let body = |signature: &str| {
            let start = source.find(signature).expect("the applier is in this file");
            let rest = &source[start + signature.len()..];
            let end = rest.find("\n    pub(crate) fn ").unwrap_or(rest.len());
            &rest[..end]
        };
        assert!(
            !body("pub(crate) fn select_tab(").contains("self.forget_undo()"),
            "selecting a tab must not drop the undo record"
        );
        for applier in [
            "pub(crate) fn add_tab_seated(",
            "pub(crate) fn close_tab_now(",
            "pub(crate) fn rename_tab(",
            "pub(crate) fn rename_group(",
            "pub(crate) fn set_group_collapsed(",
            "pub(crate) fn release_tab(",
            "pub(crate) fn adopt_tab(",
            "pub(crate) fn move_tab(",
        ] {
            assert!(
                body(applier).contains("self.forget_undo()"),
                "{applier} drops the record"
            );
        }
        // The name a group is born asking for belongs to the move that made it: the record
        // stands and is told the name, where a later rename (above) drops it.
        let naming = body("pub(crate) fn name_new_group(");
        assert!(
            !naming.contains("self.forget_undo()") && naming.contains("name_in_undo("),
            "naming a new group keeps the record and tells it the name"
        );
    }

    /// What the bar lists depends on its place (the column's groups and headers, the title row's
    /// one cluster): a change of place reads the list again, or the column shows the title row's
    /// cluster until a later refresh. An AppKit path with no window to test it in, so a source
    /// scan.
    #[test]
    fn a_change_of_place_reads_the_bars_list_again() {
        let source = include_str!("window.rs");
        let code = &source[..source.find("#[cfg(test)]").expect("the tests follow")];
        let start = code
            .find("pub(crate) fn apply_tab_position(")
            .expect("the setting's applier");
        let body = &code[start..start + code[start..].find("\n    }").expect("its end")];
        assert!(body.contains("self.refresh_bar()"), "{body}");
    }

    /// The guard of "the strip changes with its groups": the groups sit beside the strip and a
    /// change of the strip by itself would leave a group's tabs apart, with nothing to say so
    /// but a window that draws wrong. Every applier of the tab list goes through
    /// `with_strip` (or puts a whole picture back, `put_strip`), and each one asks the group
    /// operation that keeps the rule its plain counterpart had — a new tab beside the one on
    /// screen and in its group, a closed tab's neighbour inside its group first.
    #[test]
    fn the_strip_changes_only_with_its_groups() {
        let source = include_str!("window.rs");
        let code = &source[..source.find("#[cfg(test)]").expect("the tests follow")];
        assert_eq!(
            code.matches("order.borrow_mut()").count(),
            1,
            "the strip is borrowed for change in `with_strip` and nowhere else"
        );
        assert_eq!(
            code.matches(".order.replace(").count(),
            1,
            "the strip is replaced in `put_strip` and nowhere else"
        );
        // A method's text, up to the next method of the impl, public or not.
        let body = |signature: &str| {
            let start = code.find(signature).expect("the applier is in this file");
            let rest = &code[start + signature.len()..];
            let end = ["\n    pub(crate) fn ", "\n    fn "]
                .iter()
                .filter_map(|next| rest.find(next))
                .min()
                .unwrap_or(rest.len());
            &rest[..end]
        };
        for (applier, operation) in [
            ("pub(crate) fn select_tab(", "groups.select("),
            ("pub(crate) fn add_tab_seated(", "groups.open("),
            ("pub(crate) fn add_tab_seated(", "groups.open_loose("),
            ("pub(crate) fn add_tab_seated(", "groups.open_in("),
            ("pub(crate) fn add_tab_seated(", "groups.open_in_new_group("),
            ("pub(crate) fn release_tab(", "groups.close("),
            ("pub(crate) fn adopt_tab(", "groups.append("),
            ("pub(crate) fn adopt_tab(", "groups.insert_at("),
            ("pub(crate) fn move_tab(", "groups.move_to("),
            ("pub(crate) fn move_tab_listed(", "groups.move_within("),
            ("pub(crate) fn fold_tab(", "groups.pane_to_tab("),
            ("pub(crate) fn new_tab_of(", "groups.place_new("),
            ("fn leave_order(", "groups.close("),
            ("pub(crate) fn regroup(", "groups.place("),
            ("pub(crate) fn move_group(", "groups.move_group("),
            ("pub(crate) fn picture(", "groups:"),
            ("pub(crate) fn put_strip(", "groups.replace(groups)"),
        ] {
            assert!(
                body(applier).contains(operation),
                "{applier} does not call {operation}"
            );
        }
        // A closed tab goes through the same door: `close_tab_now` leaves the order by
        // `leave_order`, and that is checked above.
        assert!(body("pub(crate) fn close_tab_now(").contains("self.leave_order("));
        // The plan writes the record of a regrouping, after its step: an applier that dropped
        // the record would drop it before the plan could write it.
        for applier in [
            "pub(crate) fn regroup(",
            "pub(crate) fn move_group(",
            "pub(crate) fn put_strip(",
        ] {
            assert!(
                !body(applier).contains("self.forget_undo()"),
                "{applier} leaves the record to the plan"
            );
        }
    }

    /// The guard of ⌘P's order: the tab looked at is written where a tab comes on screen — the
    /// applier's one switch (a selection, a new tab, a neighbour taking over from a closed one)
    /// and a window becoming key (a window that was not key when its tab came up) — and forgotten
    /// where a tab object ends (`retire`), never where a tab only moves to another window
    /// (`release_tab`, whose tab keeps its identity and so its place). A source scan: the paths
    /// are AppKit's, with no window to try them in.
    #[test]
    fn the_tab_looked_at_is_written_on_the_screen_and_forgotten_at_its_end() {
        let source = include_str!("window.rs");
        let code = &source[..source.find("#[cfg(test)]").expect("the tests follow")];
        let body = |signature: &str| {
            let start = code.find(signature).expect("the method is in this file");
            let rest = &code[start + signature.len()..];
            let end = ["\n    pub(crate) fn ", "\n    fn ", "\n        fn "]
                .iter()
                .filter_map(|next| rest.find(next))
                .min()
                .unwrap_or(rest.len());
            &rest[..end]
        };
        assert!(
            body("    fn switch(&self, old: Option<&TerminalTab>)").contains("self.note_visit(")
        );
        assert!(body("fn window_did_become_key(&self").contains("self.note_visit("));
        assert!(body("pub(crate) fn retire(").contains("forget_visit("));
        for mover in ["pub(crate) fn release_tab(", "pub(crate) fn adopt_tab("] {
            assert!(
                !body(mover).contains("forget_visit("),
                "{mover} keeps the tab's place: it only changes window"
            );
        }
        // The only writers of the order are these.
        assert_eq!(code.matches("app.note_visit(").count(), 1);
        let app_source = include_str!("app.rs");
        let app_code = &app_source[..app_source.find("#[cfg(test)]").expect("tests follow")];
        let closing = &app_code[app_code
            .find("pub(crate) fn forget_window(")
            .expect("the window's end")..];
        assert!(
            closing[..closing.find("\n    }\n").expect("its end")].contains("forget_visit("),
            "a window's closing ends its tabs"
        );
    }

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
        assert!(!is_dark_background(&Theme::LINEN), "linen is light");
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
                let ask = should_ask(true, confirm, true, || {
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
            assert!(!should_ask(false, ConfirmClose::Never, false, read));
            assert!(should_ask(false, ConfirmClose::Always, false, read));
            assert_eq!(reads.get(), 0, "read while the decision is table-free");
        }
    }

    #[test]
    fn running_asks_only_while_a_job_runs() {
        let reads = Cell::new(0);
        let ask = |busy| {
            should_ask(false, ConfirmClose::Running, false, || {
                reads.set(reads.get() + 1);
                busy
            })
        };
        assert!(ask(true));
        assert!(!ask(false));
        assert_eq!(reads.get(), 2);
    }

    #[test]
    fn tabs_picked_together_ask_under_running_but_never_under_never() {
        // ⌘W over a selection is the one asking that ignores what runs: an empty prompt asks
        // under `running`, and the table is not read for the decision. `never` stays never, and a
        // timed run (no settings, a headless question would hang) still does not ask.
        let reads = Cell::new(0);
        let read = || {
            reads.set(reads.get() + 1);
            false
        };
        assert!(should_ask(false, ConfirmClose::Running, true, read));
        assert!(should_ask(false, ConfirmClose::Always, true, read));
        assert!(!should_ask(false, ConfirmClose::Never, true, read));
        assert!(!should_ask(true, ConfirmClose::Running, true, read));
        assert_eq!(
            reads.get(),
            0,
            "the table was read for a decision it does not make"
        );
        // One tab alone asks only while something runs, as it always did.
        assert!(!should_ask(false, ConfirmClose::Running, false, read));
        assert_eq!(reads.get(), 1);
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
        // ⌘W and a tab's `×` ask for a single tab, the red button and ⇧⌘W
        // for every tab, "Close Other Tabs" for the unselected ones.
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
}
