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
use bt_core::{ConfirmClose, ContentEdge, Settings, Theme};
use bt_gpu::GpuError;
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibilityAnnouncementKey, NSAccessibilityAnnouncementRequestedNotification,
    NSAccessibilityNotificationUserInfoKey, NSAccessibilityPostNotificationWithUserInfo, NSAlert,
    NSAlertFirstButtonReturn, NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua,
    NSAppearanceNameDarkAqua, NSApplication, NSBackingStoreType, NSColor, NSControlStateValueOff,
    NSControlStateValueOn, NSFloatingWindowLevel, NSFont, NSFontAttributeName, NSMenuItem,
    NSModalResponse, NSModalResponseCancel, NSStringDrawing, NSTitlebarSeparatorStyle, NSToolbar,
    NSView, NSWindow, NSWindowDelegate, NSWindowOcclusionState, NSWindowStyleMask,
    NSWindowTabbingMode, NSWindowTitleVisibility, NSWindowToolbarStyle,
};
use objc2_foundation::{
    NSAttributedStringKey, NSDictionary, NSKeyValueObservingOptions, NSNotification, NSObject,
    NSObjectNSKeyValueObserverRegistration, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
    ns_string,
};

use crate::Run;
use crate::app;
use crate::arrange::Tool;
use crate::card::{Ground, Shade};
use crate::embed;
use crate::jobs::Foreground;
use crate::launch::Closing;
use crate::moves::{self, Joins, Move};
use crate::pane::TerminalPane;
use crate::preview::beep;
use crate::restore::SavedTab;
use crate::sheets::{self, Asker};
use crate::split::{Axis, Direction, Tree};
use crate::split_view::SplitView;
use crate::tab::{self, TerminalTab};
use crate::tab_bar::{Label, TabBar};
use crate::tabs::{self, Card, Tabs};
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
            // The bar's clock runs only while the window is seen: a window
            // coming back steps its rings to now and sets it again, one
            // going away lets it lapse.
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

        /// The lights left the row: the strip starts at the edge.
        #[unsafe(method(windowDidEnterFullScreen:))]
        fn window_did_enter_full_screen(&self, _n: &NSNotification) {
            self.ivars().root.lay_out();
        }

        #[unsafe(method(windowDidExitFullScreen:))]
        fn window_did_exit_full_screen(&self, _n: &NSNotification) {
            self.ivars().toolbar.setVisible(true);
            self.ivars().root.lay_out();
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
                item.setTitle(&NSString::from_str(close_title(tab.panes().len())));
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
        /// as the tab's `×`.
        #[unsafe(method(closeTab:))]
        fn close_tab(&self, _sender: Option<&AnyObject>) {
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
            let target = self.ivars().order.borrow().by_shortcut(tag);
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
    /// The title row's last measured height; `0` until the window reports one.
    row: Cell<f64>,
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
    fn new(mtm: MainThreadMarker, frame: NSRect, bar: &TabBar) -> Retained<Self> {
        let ground = Ground::new(mtm, NSRect::new(NSPoint::ZERO, frame.size));
        let shade = Shade::new(mtm, NSRect::new(NSPoint::ZERO, frame.size));
        let this = Self::alloc(mtm).set_ivars(RootIvars {
            bar: bar.retain(),
            ground: ground.clone(),
            shade: shade.clone(),
            row: Cell::new(0.0),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        // The panes' Metal layers sit under it: a layer-backed tree all the
        // way up, so their compositing mode does not change.
        this.setWantsLayer(true);
        this.addSubview(&ground);
        this.addSubview(&shade);
        this.addSubview(bar);
        this
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

    /// Measures the title row and puts the bar in it, every container under
    /// it.
    pub(crate) fn lay_out(&self) {
        self.place(self.title_row());
    }

    /// Puts the bar in a `row` tall title row and every container under it;
    /// a question up in a tab follows its container (`sheets::fit_owner`).
    fn place(&self, row: f64) {
        let bounds = self.bounds();
        let row = row.min(bounds.size.height);
        // The whole view, the title row too: this view places its subviews
        // itself (`resizeSubviewsWithOldSize:`), so no autoresizing reaches it.
        self.ivars().ground.setFrame(bounds);
        self.ivars().shade.setFrame(bounds);
        let bar = &self.ivars().bar;
        bar.setFrame(NSRect::new(
            NSPoint::ZERO,
            NSSize::new(bounds.size.width, row),
        ));
        let below = NSRect::new(
            NSPoint::new(0.0, row),
            NSSize::new(bounds.size.width, bounds.size.height - row),
        );
        for view in self.subviews() {
            if let Ok(container) = view.downcast::<SplitView>() {
                container.setFrame(below);
                sheets::fit_owner(&container);
            }
        }
        bar.lay_out();
    }

    /// A tab's container joins under the bar, at the content's frame.
    fn add_container(&self, container: &SplitView) {
        self.addSubview(container);
        self.lay_out();
    }
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
        let this = Self::with_tab(mtm, id, run, tab, None);
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
    /// before its tab joins: a moved tab's panes then meet their final size
    /// once, and their programs one resize, not two.
    pub(crate) fn with_tab(
        mtm: MainThreadMarker,
        id: u64,
        run: Option<Run>,
        tab: Retained<TerminalTab>,
        frame: Option<NSRect>,
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
        let root = RootView::new(mtm, rect, &bar);
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

    /// Whether tab `id` is the selected one — the tab the user left on
    /// screen, whether or not the window is in front.
    pub(crate) fn is_selected(&self, id: u64) -> bool {
        self.ivars().order.borrow().selected() == Some(id)
    }

    /// Every tab's running time (`TerminalTab::running_for`) — the bar's
    /// clock steps its rings with these, without reading titles again.
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

    /// The selected tab's position in the strip.
    pub(crate) fn selected_index(&self) -> usize {
        self.ivars().order.borrow().selected_index().unwrap_or(0)
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
        self.ivars().order.borrow_mut().select(id);
        self.switch(old.as_deref());
        self.layout_changed();
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
        self.ivars().order.borrow_mut().insert(tab.id());
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
            self.ivars().order.borrow_mut().close(id);
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
                self.ivars().order.borrow_mut().append(tab.id());
                tab.refresh_geometry();
                tab.apply_visibility(self.window_visible());
                self.refresh_bar();
            }
            Placement::At(index) => {
                let old = self.ivars().order.borrow().selected();
                let old = old.and_then(|old| self.tab(old));
                // The gap the tab was carried over is the place it takes now.
                self.bar().close_gap_quietly();
                self.ivars().order.borrow_mut().insert_at(tab.id(), index);
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
        if self.tab(id).is_none() || !self.ivars().order.borrow_mut().move_to(id, index) {
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
        if !self
            .ivars()
            .order
            .borrow_mut()
            .pane_to_tab(source, true, into)
        {
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
        self.ivars().order.borrow_mut().place_new(id, gap);
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
    /// tabs in `order`, its selection with it — what was on screen leaves it
    /// first, the selected tab comes up ([`Self::switch`]).
    pub(crate) fn put_strip(&self, order: Tabs<u64>) {
        let old = self
            .ivars()
            .order
            .borrow()
            .selected()
            .and_then(|id| self.tab(id));
        self.ivars().order.replace(order);
        self.switch(old.as_deref());
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
        let Some(app) = app::delegate(self.mtm()) else {
            return;
        };
        if others.is_empty() || !self.select_tab(keep) {
            return;
        }
        let confirm = app.settings().confirm_close;
        let panes: Vec<Retained<TerminalPane>> = others
            .iter()
            .filter_map(|&id| self.tab(id))
            .flat_map(|tab| tab.panes())
            .collect();
        let Some(foregrounds) = foregrounds_to_ask(self.ivars().run.is_some(), confirm, &panes)
        else {
            for id in others {
                self.close_tab_now(id);
            }
            return;
        };
        let scope = close_scope(others.len(), self.tab_count());
        let unit = unit_for(panes.len(), others.len());
        self.ask(
            &prompt(scope, unit, &foregrounds),
            CloseTarget::Tabs(others),
        );
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
        self.ivars().order.borrow_mut().close(tab.id());
        if was_selected {
            self.switch(Some(tab));
        }
    }

    /// Keeps a closed tab's object until the next main-queue turn
    /// ([`Self::close_tab_now`]).
    pub(crate) fn retire(&self, tab: Retained<TerminalTab>) {
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
        let tabs = self.tabs();
        let single = tabs.len() == 1;
        let labels = tabs
            .iter()
            .map(|tab| {
                let title = if single {
                    tab.title()
                } else {
                    tab.session_title()
                };
                Label {
                    tab: tab.id(),
                    title: title.unwrap_or_else(|| "bateri".to_owned()),
                    indicator: tab.indicator(),
                    running: tab.running_for(),
                    upload: tab.upload(),
                    mark: tab.host_mark(),
                }
            })
            .collect();
        let notice = self.ivars().notice.borrow().clone();
        self.bar().show(labels, self.selected_index(), notice);
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

    /// `[appearance] split_style` changed: every tab's container is laid out
    /// again, as cards or divided by a line, and the ground follows the one
    /// on screen.
    pub(crate) fn apply_split_style(&self) {
        for tab in self.tabs() {
            tab.container().layout_panes();
            tab.refresh_look();
        }
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
            "pub(crate) fn add_tab(",
            "pub(crate) fn close_tab_now(",
            "pub(crate) fn rename_tab(",
            "pub(crate) fn release_tab(",
            "pub(crate) fn adopt_tab(",
            "pub(crate) fn move_tab(",
        ] {
            assert!(
                body(applier).contains("self.forget_undo()"),
                "{applier} drops the record"
            );
        }
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
