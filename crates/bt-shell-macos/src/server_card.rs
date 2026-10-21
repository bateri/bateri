//! **The server's card**: a click on `⇄ rasp` in the dock of a pane that shows a terminal the
//! server keeps opens it. Above, the server's load (small: the CPU and the memory as a bar each
//! and the busiest processes); below, the terminals the server keeps that no pane of this Mac
//! shows — *Running in the background · N*, with *Open All in New Tab* in the heading — and the
//! ones another Mac has open, *On other Macs · N*. A second click, a click outside or Esc closes
//! it.
//!
//! - **What it says is in `bt-shell-common::server_card`**: the words, the numbers, the rows
//!   (at most six, "and 3 more"), the time at a row's right. Here only the views and the
//!   input.
//! - **A row is a click**: the pointer on it (or the arrows) selects it and writes at its right
//!   what a click does, *Open in New Tab*; ⏎ or a click opens it. A right click offers *End
//!   Terminal*, which asks in the words closing a pane does, through the sheet gate
//!   (`sheets`), and only then reaches the server. Nothing that is open in a tab is listed, and
//!   nothing here moves a pane that is.
//! - **A row of *On other Macs*** says the Mac's name at its right and, selected, *Move Here*; a
//!   line under the section says what that does (the pane there goes dim). It has no *End
//!   Terminal*: a terminal somebody is using is not ended from here.
//! - **Keys** are the upload list's pattern: `transient`, anchored to the control that was
//!   clicked (`Session::footer_span` → `BateriView::context_span_rect`, the drawing's own
//!   layout), and a local key monitor in the pane's window swallows Esc, ↑, ↓ and ⏎ — the
//!   terminal window stays key, so they would otherwise reach the remote shell. The pane is
//!   the delegate of both popovers and tells them apart by the notification's object; each has
//!   its own close-time slot, so a second press on the control does not reopen it.
//! - **Refreshed on events, never on a clock of its own**: a sample (the load), the service
//!   hearing the list change ([`TerminalPane::refresh_server_card`]) and the card opening — which
//!   asks the servers again. The views are rebuilt only when what they say differs.
//! - **It goes with the door**: when the control it hangs from is no longer drawn (an upload row
//!   took its place, the remote session ended, a hide) it moves to the server's name, and closes
//!   if that is gone too ([`TerminalPane::stats_gauge_changed`]).
//!
//! The actions leave the pane through its owner ([`crate::pane::PaneHost`]): the pane's modules
//! do not know the application.

use std::cell::Cell;
use std::time::{SystemTime, UNIX_EPOCH};

use block2::RcBlock;
use bt_core::{FooterControl, StatsLevel, Theme};
use bt_shell_common::server_card::{self, Listing, Load, Local, Meter};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{
    AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel,
};
use objc2_app_kit::{
    NSAccessibility, NSAlert, NSAlertFirstButtonReturn, NSApplication, NSBox, NSBoxType, NSColor,
    NSEvent, NSEventModifierFlags, NSFont, NSLineBreakMode, NSMenu, NSMenuItem, NSModalResponse,
    NSPopover, NSPopoverBehavior, NSTextAlignment, NSTextField, NSTitlePosition, NSTrackingArea,
    NSTrackingAreaOptions, NSView, NSViewController,
};
use objc2_foundation::{
    NSNotification, NSObjectProtocol, NSPoint, NSRect, NSRectEdge, NSSize, NSString, ns_string,
};

use crate::pane::{PaneLookup, TerminalPane};
use crate::server_stats;
use crate::sheets::{self, Asker};
use crate::uploader::{ESCAPE, add_key_monitor, label, remove_monitor};

/// The card's width, inner padding, the gap between sections, a text line's height, a row's
/// height and the bar's height — **design constants** (pt), the upload list's measures.
const WIDTH: f64 = 340.0;
const PAD: f64 = 12.0;
const GAP: f64 = 8.0;
const LINE: f64 = 16.0;
const ROW: f64 = 26.0;
const BAR: f64 = 4.0;
/// The most a row's right-hand text (`idle since yesterday 09:40`) and its title may take of
/// the row; the directory, which shortens from the left, has the rest.
const STATUS_MOST: f64 = 164.0;
const TITLE_MOST: f64 = 100.0;

/// Key codes (hardware, independent of the layout).
const RETURN: u16 = 36;
const KEYPAD_ENTER: u16 = 76;
const DOWN: u16 = 125;
const UP: u16 = 126;

/// What a navigable part of the card is: the heading's *Open All in New Tab*, the terminal on
/// the list's `n`th row, or the terminal on the `n`th row of *On other Macs*.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Item {
    OpenAll,
    Terminal(usize),
    Move(usize),
}

/// A part of the card as it is named: the terminal's own name (and the list it is on), not its
/// row, which a new list may give another terminal.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Picked {
    OpenAll,
    Terminal(String),
    Move(String),
}

impl Picked {
    /// What `item` of the card as `model` drew it names.
    fn of(model: &Model, item: Item) -> Picked {
        let name = |listing: &Listing, index: usize| {
            listing
                .entries
                .get(index)
                .map(|entry| entry.terminal.clone())
                .unwrap_or_default()
        };
        match item {
            Item::OpenAll => Picked::OpenAll,
            Item::Terminal(index) => Picked::Terminal(name(&model.listing, index)),
            Item::Move(index) => Picked::Move(name(&model.others, index)),
        }
    }

    /// The part of the card as `model` draws it that this names; `None` when it is not there.
    fn item_in(&self, model: &Model) -> Option<Item> {
        let find = |listing: &Listing, terminal: &str| {
            listing
                .entries
                .iter()
                .position(|entry| entry.terminal == terminal)
        };
        match self {
            Picked::OpenAll => Some(Item::OpenAll),
            Picked::Terminal(terminal) => find(&model.listing, terminal).map(Item::Terminal),
            Picked::Move(terminal) => find(&model.others, terminal).map(Item::Move),
        }
    }
}

/// What the card shows: when this differs from what is drawn, the views are rebuilt.
#[derive(Clone, Debug, PartialEq)]
struct Model {
    /// The server as the user typed it.
    host: String,
    /// `None` when `[remote] stats` is `off`: the card is the list alone.
    load: Option<Load>,
    listing: Listing,
    /// The terminals another Mac has open.
    others: Listing,
}

/// One row's views, the ones that change with the selection.
struct RowViews {
    view: Retained<CardRow>,
    status: Retained<NSTextField>,
    /// What the status shows while the row is not selected: the time the server's list gave it,
    /// or the name of the Mac that has the terminal open.
    resting: String,
    /// What a click does, written in the status while the row is selected.
    verb: &'static str,
}

/// The open card and what it holds for refreshing.
pub(crate) struct ServerCard {
    popover: Retained<NSPopover>,
    canvas: Retained<CardCanvas>,
    /// The Esc, arrows and ⏎ monitor (removed on close).
    monitor: Option<Retained<AnyObject>>,
    /// The control the card hangs from.
    anchor: FooterControl,
    /// What the views say now; `None` before the first build.
    drawn: Option<Model>,
    /// The navigable parts, in the order the keys walk.
    items: Vec<Item>,
    /// The terminals' rows, by the list's index.
    rows: Vec<RowViews>,
    /// The rows of *On other Macs*, by their list's index.
    moved: Vec<RowViews>,
    /// The line under that section: what moving a terminal does, written while a row of it is
    /// selected (the space is kept the whole time, so the card does not grow under the pointer).
    note: Option<Retained<NSTextField>>,
    /// What the line says for each of those rows.
    notes: Vec<String>,
    /// The heading's *Open All in New Tab*.
    open_all: Option<Retained<CardRow>>,
    /// The selected part: the pointer's or the keys'.
    selected: Option<usize>,
}

// ─── The views ────────────────────────────────────────────────────────────

define_class!(
    // SAFETY: NSView is designed for subclassing; CardCanvas implements no `Drop`, has no ivar and
    // is born with NSView's constructor (`new`).
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriServerCardCanvas"]
    struct CardCanvas;

    unsafe impl NSObjectProtocol for CardCanvas {}

    impl CardCanvas {
        /// Top-down: the layout is written in reading order.
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }
);

/// What a row knows to tell the pane: whose card it is and which part it is.
struct RowIvars {
    pane: u64,
    lookup: PaneLookup,
    item: Item,
    /// Drawn with the selection's fill.
    selected: Cell<bool>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; CardRow implements no `Drop` and is born with
    // `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriServerCardRow"]
    #[ivars = RowIvars]
    struct CardRow;

    unsafe impl NSObjectProtocol for CardRow {}

    impl CardRow {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            if self.ivars().selected.get() {
                NSColor::controlAccentColor().colorWithAlphaComponent(0.22).setFill();
                let path = objc2_app_kit::NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                    self.bounds(),
                    5.0,
                    5.0,
                );
                path.fill();
            }
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(resetCursorRects))]
        fn reset_cursor_rects(&self) {
            self.addCursorRect_cursor(self.bounds(), &objc2_app_kit::NSCursor::pointingHandCursor());
        }

        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            self.tell(|pane, item| pane.card_select(Some(item)));
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            self.tell(|pane, item| pane.card_unselect(item));
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            self.tell(|pane, item| pane.card_activate(item));
        }

        #[unsafe(method(rightMouseDown:))]
        fn right_mouse_down(&self, event: &NSEvent) {
            let Item::Terminal(_) = self.ivars().item else {
                return;
            };
            self.tell(|pane, item| pane.card_select(Some(item)));
            let menu = NSMenu::new(self.mtm());
            menu.setAutoenablesItems(false);
            // SAFETY: `initWithTitle:action:keyEquivalent:` takes two `NSString`s and a selector
            // the target implements (`endTerminal:` below); the row keeps the menu's tracking
            // alive and is the item's target for as long as the menu is up.
            let entry = unsafe {
                let entry = NSMenuItem::initWithTitle_action_keyEquivalent(
                    NSMenuItem::alloc(self.mtm()),
                    &NSString::from_str(server_card::END),
                    Some(sel!(endTerminal:)),
                    ns_string!(""),
                );
                entry.setTarget(Some(self));
                entry
            };
            menu.addItem(&entry);
            let at = self.convertPoint_fromView(event.locationInWindow(), None);
            menu.popUpMenuPositioningItem_atLocation_inView(None, at, Some(self));
        }

        #[unsafe(method(endTerminal:))]
        fn end_terminal(&self, _sender: Option<&AnyObject>) {
            self.tell(|pane, item| pane.card_end(item));
        }

        #[unsafe(method(accessibilityPerformPress))]
        fn accessibility_press(&self) -> bool {
            self.tell(|pane, item| pane.card_activate(item));
            true
        }
    }
);

impl CardCanvas {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        // SAFETY: `NSView`'s `init`; the subclass has no ivar.
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

impl CardRow {
    fn new(
        mtm: MainThreadMarker,
        pane: &TerminalPane,
        item: Item,
        frame: NSRect,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(RowIvars {
            pane: pane.id(),
            lookup: pane.lookup(),
            item,
            selected: Cell::new(false),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        let options = NSTrackingAreaOptions::MouseEnteredAndExited
            | NSTrackingAreaOptions::ActiveAlways
            | NSTrackingAreaOptions::InVisibleRect;
        // SAFETY: `owner` is the view, which owns the area and outlives it; no user info.
        // `InVisibleRect` follows the view's bounds, so the rect is ignored and the area never
        // needs replacing.
        let area = unsafe {
            NSTrackingArea::initWithRect_options_owner_userInfo(
                NSTrackingArea::alloc(),
                NSRect::ZERO,
                options,
                Some(&this),
                None,
            )
        };
        this.addTrackingArea(&area);
        this.setAccessibilityElement(true);
        this
    }

    /// Hands the pane and this row's part to `act`; nothing when the pane is gone.
    fn tell(&self, act: impl FnOnce(&TerminalPane, Item)) {
        let iv = self.ivars();
        if let Some(pane) = (iv.lookup)(self.mtm(), iv.pane) {
            act(&pane, iv.item);
        }
    }

    fn set_selected(&self, selected: bool) {
        if self.ivars().selected.replace(selected) != selected {
            self.setNeedsDisplay(true);
        }
    }
}

// ─── The pane's half ──────────────────────────────────────────────────────

/// The server card's half of the pane.
impl TerminalPane {
    /// Opens the card from `control`'s span, or closes it if it is open. A `transient` popover
    /// closes itself on a press outside and that same press reaches here too: the close time is
    /// kept (`popoverWillClose:`) and nothing is done for that event (the upload list's rule).
    pub(crate) fn toggle_server_card(&self, control: FooterControl, (start, end): (u16, u16)) {
        let shown = self
            .server_card()
            .borrow()
            .as_ref()
            .map(|open| open.popover.clone());
        if let Some(popover) = shown {
            let was_shown = popover.isShown();
            self.close_server_card();
            if was_shown {
                return;
            }
        }
        if self.card_closed_by_current_event() {
            return;
        }
        let Some(rect) = self.view().context_span_rect(start, end) else {
            return;
        };
        let Some(window) = self.window() else {
            return;
        };
        let mtm = self.mtm();
        let popover = NSPopover::new(mtm);
        popover.setBehavior(NSPopoverBehavior::Transient);
        popover.setDelegate(Some(ProtocolObject::from_ref(self)));
        let controller = NSViewController::new(mtm);
        let canvas = CardCanvas::new(mtm);
        controller.setView(&canvas);
        popover.setContentViewController(Some(&controller));
        let (id, lookup) = (self.id(), self.lookup());
        let number = window.windowNumber();
        let monitor = add_key_monitor(move |event| {
            if event.windowNumber() != number {
                return false;
            }
            // audit: the local event monitor runs on the main thread.
            let mtm = MainThreadMarker::new().expect("the event monitor is on the main thread");
            let Some(pane) = lookup(mtm, id) else {
                return false;
            };
            if pane.server_card().borrow().is_none() {
                // Even if the card closed Esc itself, the key must not go to the shell.
                return event.keyCode() == ESCAPE && pane.card_closed_by_current_event();
            }
            pane.card_key(event)
        });
        self.server_card().replace(Some(ServerCard {
            popover: popover.clone(),
            canvas,
            monitor,
            anchor: control,
            drawn: None,
            items: Vec::new(),
            rows: Vec::new(),
            moved: Vec::new(),
            note: None,
            notes: Vec::new(),
            open_all: None,
            selected: None,
        }));
        // The list is what the servers last said; they are asked again, and the answer comes.
        self.host().refresh_background();
        self.refresh_server_card();
        let view: &NSView = self.view();
        popover.showRelativeToRect_ofView_preferredEdge(rect, view, NSRectEdge::MinY);
        self.set_stats_detail(true);
    }

    /// Whether the event that closed the card is the current event.
    fn card_closed_by_current_event(&self) -> bool {
        let now = current_event_time(self.mtm());
        now.is_some() && self.card_closed_at().get() == now
    }

    /// Whether `notification` is about the server card (the pane is the delegate of the upload
    /// list too).
    pub(crate) fn is_server_card(&self, notification: &NSNotification) -> bool {
        let Some(object) = notification.object() else {
            return false;
        };
        self.server_card().borrow().as_ref().is_some_and(|open| {
            std::ptr::eq(
                Retained::as_ptr(&object).cast::<()>(),
                Retained::as_ptr(&open.popover).cast::<()>(),
            )
        })
    }

    /// `popoverWillClose:` of the card: the time of the event that closed it.
    pub(crate) fn server_card_will_close(&self) {
        self.card_closed_at().set(current_event_time(self.mtm()));
    }

    /// Closes the card and stops asking for the details; no-op if closed.
    /// `popoverDidClose:` lands here too (AppKit closed it itself).
    pub(crate) fn close_server_card(&self) {
        let Some(open) = self.server_card().borrow_mut().take() else {
            return;
        };
        remove_monitor(open.monitor);
        if open.popover.isShown() {
            open.popover.close();
        }
        self.set_stats_detail(false);
    }

    /// What the card says now: the server's load and the terminals it keeps that no pane shows.
    /// `None` when this pane is not a door to a server.
    fn card_model(&self) -> Option<Model> {
        let shown = self.host().kept_terminal(self.uuid())?;
        self.server_target()?;
        let settings = self.stats_seat().borrow().settings();
        let load = settings
            .mode
            .form()
            .map(|_| server_card::load(&shown.host, server_stats::detail(&shown.account).as_ref()));
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        let listing = server_card::listing(&self.host().background(&shown.account), now, &local);
        let others = server_card::listing(&self.host().elsewhere(&shown.account), now, &local);
        Some(Model {
            host: shown.host,
            load,
            listing,
            others,
        })
    }

    /// Rewrites the open card from what is known now. Called on every sample, when the service
    /// hears the list change and when the card opens; the views are rebuilt only when what they
    /// say differs.
    pub(crate) fn refresh_server_card(&self) {
        if self.server_card().borrow().is_none() {
            return;
        }
        let Some(model) = self.card_model() else {
            self.close_server_card();
            return;
        };
        let theme = match self.session() {
            Some(session) => session.theme(),
            None => return,
        };
        let mtm = self.mtm();
        let mut open = self.server_card().borrow_mut();
        let Some(open) = open.as_mut() else {
            return;
        };
        if open.drawn.as_ref() == Some(&model) {
            return;
        }
        // The selection is what it names, not where it stands: a list that dropped or reordered
        // a row must not leave it on another terminal (⏎ on a row of *On other Macs* takes the
        // terminal from the Mac that is using it).
        let kept = open
            .drawn
            .as_ref()
            .zip(open.items.get(open.selected.unwrap_or(usize::MAX)))
            .map(|(drawn, item)| Picked::of(drawn, *item));
        self.build_card(mtm, open, &model, &theme);
        // It stays on the same part when that is still there, and goes when it is not.
        open.selected = kept
            .and_then(|kept| kept.item_in(&model))
            .and_then(|kept| open.items.iter().position(|item| *item == kept));
        open.apply_selection();
        open.drawn = Some(model);
    }

    /// One terminal's row, at `top` in the canvas: its title, the directory it runs in and, at
    /// the right, `resting` until the row is selected and `verb` takes its place. The right-hand
    /// text is as wide as the wider of the two; the title as it is, up to a limit; the directory
    /// has the rest.
    #[allow(clippy::too_many_arguments)]
    fn card_row(
        &self,
        mtm: MainThreadMarker,
        canvas: &CardCanvas,
        entry: &server_card::Entry,
        item: Item,
        verb: &'static str,
        resting: String,
        top: f64,
    ) -> RowViews {
        let secondary = NSColor::secondaryLabelColor();
        let primary = NSColor::labelColor();
        let inner = WIDTH - 2.0 * PAD;
        let frame = rect(PAD - 4.0, top, inner + 8.0, ROW);
        let row = CardRow::new(mtm, self, item, frame);
        let title = label(mtm, &entry.title, 12.0, &primary);
        title.setFont(Some(&NSFont::systemFontOfSize_weight(12.0, 0.2)));
        title.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        let status = label(mtm, &resting, 11.0, &secondary);
        status.setAlignment(NSTextAlignment::Right);
        status.setLineBreakMode(NSLineBreakMode::ByTruncatingHead);
        let hint = label(mtm, verb, 11.0, &secondary);
        let status_width = fitted(&status)
            .max(fitted(&hint))
            .ceil()
            .clamp(80.0, STATUS_MOST);
        let title_width = fitted(&title).ceil().clamp(30.0, TITLE_MOST);
        let row_width = inner + 8.0;
        let place_left = 4.0 + title_width + 4.0;
        let place_width = (row_width - place_left - 6.0 - status_width - 4.0).max(30.0);
        place(&row, &title, 4.0, 5.0, title_width, LINE);
        if let Some(directory) = &entry.place {
            let directory = label(mtm, directory, 11.0, &secondary);
            directory.setLineBreakMode(NSLineBreakMode::ByTruncatingHead);
            place(&row, &directory, place_left, 5.0, place_width, LINE);
        }
        place(
            &row,
            &status,
            row_width - 4.0 - status_width,
            5.0,
            status_width,
            LINE,
        );
        let spoken = [
            Some(entry.title.as_str()),
            entry.place.as_deref(),
            Some(resting.as_str()).filter(|text| !text.is_empty()),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", ");
        row.setAccessibilityLabel(Some(&NSString::from_str(&spoken)));
        canvas.addSubview(&row);
        RowViews {
            view: row,
            status,
            resting,
            verb,
        }
    }

    /// Lays the card out, top-down, into its canvas and sizes the popover to it.
    fn build_card(
        &self,
        mtm: MainThreadMarker,
        open: &mut ServerCard,
        model: &Model,
        theme: &Theme,
    ) {
        let canvas = &open.canvas;
        for view in canvas.subviews().iter() {
            view.removeFromSuperview();
        }
        open.items.clear();
        open.rows.clear();
        open.moved.clear();
        open.note = None;
        open.notes.clear();
        open.open_all = None;
        let secondary = NSColor::secondaryLabelColor();
        let inner = WIDTH - 2.0 * PAD;
        let mut top = PAD;

        if let Some(load) = &model.load {
            let title = label(mtm, &load.title, 12.0, &secondary);
            title.setFont(Some(&NSFont::boldSystemFontOfSize(12.0)));
            title.setLineBreakMode(NSLineBreakMode::ByTruncatingMiddle);
            place(canvas, &title, PAD, top, inner, LINE);
            top += LINE + GAP;
            for (name, meter) in [(&load.cpu_label[..], &load.cpu), ("Memory", &load.mem)] {
                top = meter_line(mtm, canvas, name, meter, theme, top);
            }
            if let Some(busiest) = &load.busiest {
                let line = label(mtm, busiest, 11.0, &secondary);
                line.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
                place(canvas, &line, PAD, top, inner, LINE);
                top += LINE + GAP;
            }
            let rule = NSBox::new(mtm);
            rule.setBoxType(NSBoxType::Separator);
            place(canvas, &rule, PAD, top, inner, 1.0);
            top += 1.0 + GAP;
        }

        let heading = label(
            mtm,
            &server_card::heading(model.listing.total),
            11.0,
            &secondary,
        );
        heading.setFont(Some(&NSFont::boldSystemFontOfSize(11.0)));
        place(canvas, &heading, PAD, top, inner, LINE);
        if model.listing.total > 0 {
            let frame = rect(WIDTH - PAD - 150.0, top - 3.0, 150.0, LINE + 6.0);
            let row = CardRow::new(mtm, self, Item::OpenAll, frame);
            let text = label(mtm, server_card::OPEN_ALL, 11.0, &secondary);
            text.setAlignment(NSTextAlignment::Right);
            place(&row, &text, 6.0, 3.0, 138.0, LINE);
            canvas.addSubview(&row);
            row.setAccessibilityLabel(Some(&NSString::from_str(server_card::OPEN_ALL)));
            open.items.push(Item::OpenAll);
            open.open_all = Some(row);
        }
        top += LINE + 2.0;

        if model.listing.total == 0 {
            let empty = label(mtm, server_card::EMPTY, 12.0, &secondary);
            empty.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
            place(canvas, &empty, PAD, top, inner, LINE);
            top += LINE;
        }
        for (index, entry) in model.listing.entries.iter().enumerate() {
            let resting = entry.activity.clone().unwrap_or_default();
            let views = self.card_row(
                mtm,
                canvas,
                entry,
                Item::Terminal(index),
                server_card::OPEN,
                resting,
                top,
            );
            open.items.push(Item::Terminal(index));
            open.rows.push(views);
            top += ROW;
        }
        if model.listing.more > 0 {
            let more = label(
                mtm,
                &server_card::more_text(model.listing.more),
                11.0,
                &secondary,
            );
            place(canvas, &more, PAD, top + 2.0, inner, LINE);
            top += LINE + 2.0;
        }

        if model.others.total > 0 {
            top += GAP;
            let heading = label(
                mtm,
                &server_card::others_heading(model.others.total),
                11.0,
                &secondary,
            );
            heading.setFont(Some(&NSFont::boldSystemFontOfSize(11.0)));
            place(canvas, &heading, PAD, top, inner, LINE);
            top += LINE + 2.0;
            for (index, entry) in model.others.entries.iter().enumerate() {
                // The Mac's name where the time is, until the row is selected and says its verb.
                let by = entry.by.clone().unwrap_or_default();
                let views = self.card_row(
                    mtm,
                    canvas,
                    entry,
                    Item::Move(index),
                    server_card::MOVE,
                    by.clone(),
                    top,
                );
                open.items.push(Item::Move(index));
                open.notes.push(server_card::move_note(&by));
                open.moved.push(views);
                top += ROW;
            }
            if model.others.more > 0 {
                let more = label(
                    mtm,
                    &server_card::more_text(model.others.more),
                    11.0,
                    &secondary,
                );
                place(canvas, &more, PAD, top + 2.0, inner, LINE);
                top += LINE + 2.0;
            }
            // What moving does: written under the section while a row of it is selected, in the
            // room that is kept for it the whole time.
            let note = label(mtm, "", 11.0, &secondary);
            note.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
            place(canvas, &note, PAD, top + 2.0, inner, LINE);
            open.note = Some(note);
            top += LINE + 2.0;
        }
        top += PAD - 2.0;
        let size = NSSize::new(WIDTH, top);
        canvas.setFrameSize(size);
        open.popover.setContentSize(size);
    }

    /// The pointer is on a part (`Some`), or the keys moved to it.
    pub(crate) fn card_select(&self, item: Option<Item>) {
        // `try_`: the views are rebuilt under a `borrow_mut`, and an event for a row that is
        // going is not worth a panic.
        if let Ok(mut open) = self.server_card().try_borrow_mut()
            && let Some(open) = open.as_mut()
        {
            open.selected = item.and_then(|item| open.items.iter().position(|part| *part == item));
            open.apply_selection();
        }
    }

    /// The pointer left a part: nothing is selected unless the pointer is on another by now.
    pub(crate) fn card_unselect(&self, item: Item) {
        if let Ok(mut open) = self.server_card().try_borrow_mut()
            && let Some(open) = open.as_mut()
            && open.selected.and_then(|at| open.items.get(at)) == Some(&item)
        {
            open.selected = None;
            open.apply_selection();
        }
    }

    /// A part was pressed, or ⏎ on the selected one: the card goes and the application does it.
    pub(crate) fn card_activate(&self, item: Item) {
        let Some(model) = self
            .server_card()
            .borrow()
            .as_ref()
            .and_then(|open| open.drawn.clone())
        else {
            return;
        };
        match item {
            Item::OpenAll => {
                self.close_server_card();
                self.host().open_all_background(self.id(), &model.host);
            }
            Item::Terminal(index) => {
                let Some(entry) = model.listing.entries.get(index) else {
                    return;
                };
                self.close_server_card();
                self.host()
                    .open_background(self.id(), &entry.account, &entry.terminal);
            }
            Item::Move(index) => {
                let Some(entry) = model.others.entries.get(index) else {
                    return;
                };
                self.close_server_card();
                self.host()
                    .move_here(self.id(), &entry.account, &entry.terminal);
            }
        }
    }

    /// *End Terminal* on a row: the card goes and the question comes, as closing a pane asks it;
    /// only an answer of End reaches the server.
    pub(crate) fn card_end(&self, item: Item) {
        let Item::Terminal(index) = item else {
            return;
        };
        let Some(model) = self
            .server_card()
            .borrow()
            .as_ref()
            .and_then(|open| open.drawn.clone())
        else {
            return;
        };
        let Some(entry) = model.listing.entries.get(index).cloned() else {
            return;
        };
        self.close_server_card();
        let Some(seat) = sheets::seat(Asker::Pane(self)).filter(|seat| !seat.is_taken()) else {
            // A question is up already: nothing is ended without one.
            return;
        };
        let (title, message) = server_card::end_text(&model.host, &entry);
        let alert = NSAlert::new(self.mtm());
        alert.setMessageText(&NSString::from_str(&title));
        alert.setInformativeText(&NSString::from_str(&message));
        alert.addButtonWithTitle(&NSString::from_str(server_card::END));
        let cancel = alert.addButtonWithTitle(ns_string!("Cancel"));
        // Esc by hand (the rationale of `window::alert`).
        cancel.setKeyEquivalent(ns_string!("\u{1b}"));
        let (id, lookup) = (self.id(), self.lookup());
        let answered = RcBlock::new(move |response: NSModalResponse| {
            if response != NSAlertFirstButtonReturn {
                return;
            }
            // audit: the sheet's completion block runs on AppKit's main thread.
            let mtm = MainThreadMarker::new().expect("the sheet block is on the main thread");
            if let Some(pane) = lookup(mtm, id) {
                pane.host().end_background(&entry.account, &entry.terminal);
            }
        });
        seat.begin(&alert, &answered);
    }

    /// A key while the card is open: Esc closes, ↑ and ↓ walk the parts, ⏎ does the selected
    /// one. `true` if the key is the card's and must not reach the shell. Modified keys are the
    /// terminal's.
    fn card_key(&self, event: &NSEvent) -> bool {
        let held = event.modifierFlags();
        let own = NSEventModifierFlags::Command
            | NSEventModifierFlags::Option
            | NSEventModifierFlags::Control
            | NSEventModifierFlags::Shift;
        if held.intersects(own) {
            return false;
        }
        match event.keyCode() {
            ESCAPE => {
                self.close_server_card();
                true
            }
            DOWN | UP => {
                if let Some(open) = self.server_card().borrow_mut().as_mut() {
                    let count = open.items.len();
                    if count > 0 {
                        open.selected = Some(match (open.selected, event.keyCode() == DOWN) {
                            (None, true) => 0,
                            (None, false) => count - 1,
                            (Some(at), true) => (at + 1) % count,
                            (Some(at), false) => (at + count - 1) % count,
                        });
                        open.apply_selection();
                    }
                }
                true
            }
            RETURN | KEYPAD_ENTER => {
                let item = self
                    .server_card()
                    .borrow()
                    .as_ref()
                    .and_then(|open| open.items.get(open.selected?).copied());
                if let Some(item) = item {
                    self.card_activate(item);
                }
                true
            }
            _ => false,
        }
    }

    /// The control the card hangs from may have appeared, gone or moved (a sample, a hide, the
    /// remote edge, the settings, an upload row): the card follows it, moves to the server's name
    /// if that control is the one that went, and closes if both are gone. The hand cursor's
    /// rectangles are refreshed.
    pub(crate) fn stats_gauge_changed(&self) {
        let anchored = self
            .server_card()
            .borrow()
            .as_ref()
            .map(|open| (open.popover.clone(), open.anchor));
        if let Some((popover, anchor)) = anchored {
            let span_of = |control| {
                self.view()
                    .context_budget()
                    .zip(self.session())
                    .and_then(|(budget, session)| session.footer_span(budget, control))
                    .and_then(|(start, end)| self.view().context_span_rect(start, end))
            };
            match (span_of(anchor), span_of(FooterControl::Server)) {
                (Some(rect), _) => popover.setPositioningRect(rect),
                (None, Some(rect)) => {
                    popover.setPositioningRect(rect);
                    if let Some(open) = self.server_card().borrow_mut().as_mut() {
                        open.anchor = FooterControl::Server;
                    }
                }
                (None, None) => self.close_server_card(),
            }
        }
        self.view().sync_cursor_rects();
    }
}

impl ServerCard {
    /// Draws the selection: the part's fill, and at the selected row's right what a click does;
    /// under *On other Macs*, what moving a terminal does to the pane on the other Mac.
    fn apply_selection(&mut self) {
        let selected = self.selected.and_then(|at| self.items.get(at)).copied();
        if let Some(row) = &self.open_all {
            row.set_selected(selected == Some(Item::OpenAll));
        }
        for (index, row) in self.rows.iter().enumerate() {
            row.draw(selected == Some(Item::Terminal(index)));
        }
        for (index, row) in self.moved.iter().enumerate() {
            row.draw(selected == Some(Item::Move(index)));
        }
        if let Some(note) = &self.note {
            let text = match selected {
                Some(Item::Move(index)) => self.notes.get(index).map_or("", String::as_str),
                _ => "",
            };
            set_text(note, text);
        }
    }
}

impl RowViews {
    /// The row selected, or not: its fill, and its verb or what it says at rest.
    fn draw(&self, on: bool) {
        self.view.set_selected(on);
        set_text(&self.status, if on { self.verb } else { &self.resting });
    }
}

// ─── Small helpers ────────────────────────────────────────────────────────

fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}

/// Puts `view` at (`x`, `top`) in `parent`'s top-down coordinates and adds it.
fn place(parent: &NSView, view: &NSView, x: f64, top: f64, w: f64, h: f64) {
    view.setFrame(rect(x, top, w, h));
    parent.addSubview(view);
}

/// A label + value line with a bar below it; returns the next `top`.
fn meter_line(
    mtm: MainThreadMarker,
    canvas: &NSView,
    name: &str,
    meter: &Meter,
    theme: &Theme,
    mut top: f64,
) -> f64 {
    let inner = WIDTH - 2.0 * PAD;
    let name = label(mtm, name, 12.0, &NSColor::secondaryLabelColor());
    place(canvas, &name, PAD, top, inner - 150.0, LINE);
    let value = label(mtm, &meter.value, 12.0, &NSColor::labelColor());
    value.setAlignment(NSTextAlignment::Right);
    place(canvas, &value, WIDTH - PAD - 150.0, top, 150.0, LINE);
    top += LINE + 2.0;
    let track = bar(mtm, &NSColor::quaternaryLabelColor());
    place(canvas, &track, PAD, top, inner, BAR);
    let fill = bar(mtm, &srgb(level_color(meter.level, theme)));
    place(
        canvas,
        &fill,
        PAD,
        top,
        inner * meter.fill.clamp(0.0, 1.0),
        BAR,
    );
    top + BAR + GAP
}

/// A flat, rounded bar (track or fill): an `NSBox`, because a progress indicator cannot take the
/// theme's colour.
fn bar(mtm: MainThreadMarker, color: &NSColor) -> Retained<NSBox> {
    let bar = NSBox::new(mtm);
    bar.setBoxType(NSBoxType::Custom);
    bar.setTitlePosition(NSTitlePosition::NoTitle);
    bar.setBorderWidth(0.0);
    bar.setCornerRadius(BAR / 2.0);
    bar.setFillColor(color);
    bar
}

/// The width a label's text takes in one line.
fn fitted(field: &NSTextField) -> f64 {
    field.fittingSize().width
}

/// Sets a label's text only if it changed (no relayout for the same text).
fn set_text(field: &NSTextField, text: &str) {
    if field.stringValue().to_string() != text {
        field.setStringValue(&NSString::from_str(text));
    }
}

/// The theme role of a threshold class — the context row's colours.
fn level_color(level: StatsLevel, theme: &Theme) -> u32 {
    match level {
        StatsLevel::Normal => theme.info,
        StatsLevel::Warning => theme.warning,
        StatsLevel::Critical => theme.error,
    }
}

/// A `0xRRGGBB` theme colour as an sRGB `NSColor`.
fn srgb(color: u32) -> Retained<NSColor> {
    let byte = |shift: u32| f64::from((color >> shift) & 0xff) / 255.0;
    NSColor::colorWithSRGBRed_green_blue_alpha(byte(16), byte(8), byte(0), 1.0)
}

/// The current event's time stamp (the close-time slots' unit).
fn current_event_time(mtm: MainThreadMarker) -> Option<f64> {
    NSApplication::sharedApplication(mtm)
        .currentEvent()
        .map(|event| event.timestamp())
}

/// A second of the server's clock as this Mac's calendar reads it.
fn local(seconds: u64) -> Local {
    let Ok(time) = libc::time_t::try_from(seconds) else {
        return Local::civil(1970, 1, 1, 0, 0);
    };
    // SAFETY: `tm` is plain data that `localtime_r` fills; both pointers are to live locals, and
    // the function keeps no state of its own (the reentrant one).
    let tm = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&time, &mut tm).is_null() {
            return Local::civil(1970, 1, 1, 0, 0);
        }
        tm
    };
    // audit: `tm_mon` is 0..=11, `tm_mday` 1..=31, `tm_hour` 0..=23 and `tm_min` 0..=59.
    Local::civil(
        tm.tm_year + 1900,
        (tm.tm_mon + 1) as u32,
        tm.tm_mday as u32,
        tm.tm_hour as u8,
        tm.tm_min as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(terminal: &str) -> server_card::Entry {
        server_card::Entry {
            account: "u@rasp:22".to_owned(),
            terminal: terminal.to_owned(),
            title: terminal.to_owned(),
            place: None,
            activity: None,
            running: false,
            by: None,
        }
    }

    fn model(background: &[&str], elsewhere: &[&str]) -> Model {
        let listing = |names: &[&str]| Listing {
            total: names.len(),
            entries: names.iter().map(|name| entry(name)).collect(),
            more: 0,
        };
        Model {
            host: "rasp".to_owned(),
            load: None,
            listing: listing(background),
            others: listing(elsewhere),
        }
    }

    /// The selection is the terminal it names: a list that reordered or dropped rows does not leave
    /// it on another one, and ⏎ on it moves a terminal from the Mac that is using it.
    #[test]
    fn the_selection_follows_the_terminal_it_names_through_a_new_list() {
        let drawn = model(&["a", "b"], &["x", "y"]);
        let picked = Picked::of(&drawn, Item::Move(1));
        assert_eq!(picked, Picked::Move("y".to_owned()));
        // The list reordered: the same terminal, on another row.
        assert_eq!(
            picked.item_in(&model(&["a", "b"], &["y", "x"])),
            Some(Item::Move(0))
        );
        // The terminal is gone from that list (the Mac let go of it): nothing is selected, not
        // the row that took its place.
        assert_eq!(picked.item_in(&model(&["a", "b"], &["x"])), None);
        // The same name in the other list is another part.
        assert_eq!(picked.item_in(&model(&["y"], &["x"])), None);
        assert_eq!(
            Picked::of(&drawn, Item::Terminal(1)).item_in(&model(&["b"], &[])),
            Some(Item::Terminal(0))
        );
        assert_eq!(
            Picked::of(&drawn, Item::OpenAll).item_in(&model(&[], &[])),
            Some(Item::OpenAll)
        );
    }

    #[test]
    fn this_macs_calendar_reads_a_second_as_a_date_and_a_time() {
        // Whatever the zone, a second and the one a day later are consecutive days at the same
        // time of day, and an hour later is the next hour (or the next day).
        let base = 1_700_000_000_u64;
        let (a, b) = (local(base), local(base + 86_400));
        assert_eq!(b.day - a.day, 1);
        assert_eq!((a.hour, a.minute), (b.hour, b.minute));
    }

    #[test]
    fn a_bar_takes_the_threshold_class_colour() {
        let theme = Theme::BATERI;
        assert_eq!(level_color(StatsLevel::Normal, &theme), theme.info);
        assert_eq!(level_color(StatsLevel::Warning, &theme), theme.warning);
        assert_eq!(level_color(StatsLevel::Critical, &theme), theme.error);
    }
}
