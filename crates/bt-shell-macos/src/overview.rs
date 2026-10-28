//! The big view (Window ▸ Show All Tabs, ⇧⌘\): a cover over the window's whole content, the
//! side column included, that shows its tabs as pictures of their panes' real screens. The rule
//! — which tabs, in what grid, where the arrows go, what ↵ does — is
//! `bt_shell_common::overview`'s; this file draws it and hands it the events.
//!
//! **A cover, not a window**, and the palette's way ([`crate::palette`]): the root view holds it
//! above everything ([`RootView::seat_overview`]), so it goes with the window into full screen
//! and a Space and opens no second path for a question to sit over (`sheets`). It takes the
//! keyboard in an invisible text field: Esc, the arrows, ⏎ and ⇥ come from the field's command
//! hook, letters would be the field's (the search row's, once it is there); ⌘1…⌘9 are key
//! equivalents and reach the view before the menu does. A press on the title row falls through
//! the cover to the row's claim (`hitTest:` answers nothing there), so the window is still
//! dragged by it (`.claude/rules/sekmeler.md`). The palette and this view are not open at once.
//!
//! **The pictures are the card's** (`.claude/rules/render.md` → pane'in görüntüsü): each pane is
//! asked for a still at the size it is drawn ([`TerminalWindow::tab_shots`]), pulled — no frame,
//! no counter — when the view opens and by one delayed wake every [`tabs::MAP_REFRESH`] after
//! it; the wake is not set again once the window is not seen, and the panes let go of their
//! textures when the view closes ([`TerminalWindow::release_tab_shots`]). Only the tabs in the
//! viewport are asked.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};

use bt_core::Theme;
use bt_shell_common::overview::{self as rules, Direction, Layout, Machine, Overview, Row};
use bt_shell_common::split::Rect;
use bt_shell_common::tabs;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel,
};
use objc2_app_kit::{
    NSAttributedStringNSStringDrawing, NSAutoresizingMaskOptions, NSBezierPath,
    NSCompositingOperation, NSControlTextEditingDelegate, NSEvent, NSEventModifierFlags,
    NSFocusRingType, NSFont, NSFontWeightMedium, NSFontWeightSemibold, NSGraphicsContext, NSImage,
    NSTextField, NSTextFieldDelegate, NSTrackingArea, NSTrackingAreaOptions, NSView,
    NSWindowOrderingMode,
};
use objc2_foundation::{
    NSAttributedString, NSNotification, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
};
use objc2_quartz_core::CALayer;

use crate::app;
use crate::arrange;
use crate::palette::{
    Attributes, Look, attributes, draw_text, draw_text_right, styled, text_width,
};
use crate::server_stats;
use crate::tab_bar::{Ease, PALETTE_ICON_SIDE, Tint, palette_icon, play, rounded, selected_row};
use crate::window::{RootView, TerminalWindow};

/// The air above the first row, under the title row, and below the last one.
const TOP_AIR: f64 = 16.0;
const BOTTOM_AIR: f64 = 8.0;
/// The search row at the bottom: a plate the dock's way, this tall, this far above the window's
/// bottom edge and in from the column and the right edge; what the grid leaves for it.
const PLATE_HEIGHT: f64 = 44.0;
const PLATE_GAP: f64 = 16.0;
const PLATE_SIDE: f64 = 24.0;
const PLATE_RADIUS: f64 = 12.0;
const PLATE_PAD: f64 = 18.0;
const PLATE_AREA: f64 = PLATE_HEIGHT + 2.0 * PLATE_GAP;
const FIELD_SIZE: f64 = 14.0;
/// The most of the plate's width the keys and the count at its right take.
const KEYS_SHARE: f64 = 0.45;
const KEYS_MAX: f64 = 340.0;
/// The corner of a picture and of the ring round the selected one.
const RADIUS: f64 = 8.0;
const RING: f64 = 2.5;
const RING_AIR: f64 = 4.0;
/// How far a wheel notch scrolls when the device gives lines, not points.
const LINE_SCROLL: f64 = 12.0;
/// The air kept round the selected cell when an arrow scrolls it into view.
const REVEAL_AIR: f64 = 16.0;
/// The opening: the whole view settles from this much larger, over this long.
const OPEN_SCALE: f64 = 1.06;
const OPEN_SECS: f64 = 0.16;
/// A pane without a still yet is this much of the ground, and a pane that gives way to another is
/// covered with this much of it.
const DIM_COVER: f64 = 0.62;

const NAME_SIZE: f64 = 13.0;
const SMALL_SIZE: f64 = 12.0;
const GAP_AFTER_ICON: f64 = 7.0;
const GAP_AFTER_NAME: f64 = 10.0;

/// The left column's width, its rows' inset from the window's edge and from each other, and a
/// machine row's height (two lines: the name, how many tabs).
const SIDE: f64 = 244.0;
const SIDE_INSET: f64 = 14.0;
const MACHINE_ROW: f64 = 46.0;
const MACHINE_GAP: f64 = 2.0;
/// Inside a machine row: the text's left, and the load block's lines under a server's row.
const ROW_TEXT: f64 = 12.0;
const LOAD_LINE: f64 = 17.0;
const LOAD_PAD: f64 = 8.0;
const SPARK_HEIGHT: f64 = 22.0;
const SPARK_BAR: f64 = 4.0;
const SPARK_GAP: f64 = 2.0;
/// The key hint under the column.
const HINT: &str = "⇥ next machine   ⇧⇥ previous";

pub(crate) struct OverviewIvars {
    /// The window the view is in: where closing, going to a tab and the wake find it again.
    window: u64,
    state: RefCell<Overview>,
    field: Retained<NSTextField>,
    look: Cell<Look>,
    /// The selected machine row's fill and line (the side column's selected row).
    row_look: Cell<(Tint, Tint)>,
    /// Each tab's icon, drawn in the theme's colours.
    icons: RefCell<HashMap<u64, Retained<NSImage>>>,
    /// The last still of each pane, `(tab, pane)`.
    shots: RefCell<HashMap<(u64, u64), Retained<NSImage>>>,
    /// The tabs some pane of which holds textures for the view.
    shot_tabs: RefCell<HashSet<u64>>,
    /// The selected server's last sample as the column says it, and its CPU levels.
    load: RefCell<Option<(rules::Load, Vec<u8>)>>,
    /// The account whose load the window's view is asking for ([`server_stats::set_view`]).
    viewed: RefCell<Option<String>>,
    scroll: Cell<f64>,
    /// The window's content, height over width: the shape of a tab's picture.
    aspect: Cell<f64>,
    /// The title row's height: the grid starts under it.
    top: Cell<f64>,
    /// Set when the view is on its way out: late news from the field is ignored.
    closing: Cell<bool>,
    /// The live wake's number; a wake that finds another has been replaced.
    generation: Cell<u64>,
    /// The wake lapsed because the window was not seen; it is set again when it is.
    parked: Cell<bool>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; the view implements no `Drop` and is born with
    // `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriOverview"]
    #[ivars = OverviewIvars]
    pub(crate) struct OverviewView;

    unsafe impl NSObjectProtocol for OverviewView {}
    unsafe impl NSControlTextEditingDelegate for OverviewView {}
    unsafe impl NSTextFieldDelegate for OverviewView {}

    impl OverviewView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(mouseDownCanMoveWindow))]
        fn can_move_window(&self) -> bool {
            false
        }

        /// The title row is the window's to drag (the row's claim answers there); the rest is
        /// the view's, until it is on its way out.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
            let iv = self.ivars();
            // SAFETY: the superview is read on the main thread, for the length of this call.
            let parent = unsafe { self.superview() };
            let local = self.convertPoint_fromView(point, parent.as_deref());
            if iv.closing.get() || local.y < iv.top.get() {
                None
            } else {
                // SAFETY: `NSView`'s `hitTest:` takes a point in the superview's space.
                unsafe { msg_send![super(self), hitTest: point] }
            }
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            self.paint();
        }

        #[unsafe(method(updateTrackingAreas))]
        fn update_tracking_areas(&self) {
            // SAFETY: `NSView`'s argumentless method returning nothing.
            let _: () = unsafe { msg_send![super(self), updateTrackingAreas] };
            self.track();
        }

        /// The pointer moved: the cell under it is selected, and a split tab's pane under it
        /// lit. Only a move does it, never a scroll that slid a cell under a still pointer.
        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, event: &NSEvent) {
            self.point_at(event);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            if self.ivars().closing.get() {
                return;
            }
            let selected = self.ivars().state.borrow().selected();
            if let Some(selected) = selected {
                self.ivars().state.borrow_mut().select(selected);
            }
            self.ivars().state.borrow_mut().glance(None);
            self.setNeedsDisplay(true);
        }

        /// A press on a cell is ↵ on it; a press anywhere else is nothing.
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let at = self.convertPoint_fromView(event.locationInWindow(), None);
            if at.x < SIDE {
                self.press_column(at.y);
            } else if self.point_at(event) {
                self.activate();
            }
        }

        #[unsafe(method(rightMouseDown:))]
        fn right_mouse_down(&self, _event: &NSEvent) {}

        #[unsafe(method(otherMouseDown:))]
        fn other_mouse_down(&self, _event: &NSEvent) {}

        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, event: &NSEvent) {
            let delta = if event.hasPreciseScrollingDeltas() {
                event.scrollingDeltaY()
            } else {
                event.scrollingDeltaY() * LINE_SCROLL
            };
            self.scroll_to(self.ivars().scroll.get() - delta);
        }

        /// ⌘1…⌘9: the tab the strip's digit reaches, before the menu takes the key (it would
        /// select the tab and leave this view up).
        #[unsafe(method(performKeyEquivalent:))]
        fn perform_key_equivalent(&self, event: &NSEvent) -> bool {
            self.equivalent(event)
        }

        /// The letters changed: the grid keeps the tabs the search finds.
        #[unsafe(method(controlTextDidChange:))]
        fn control_text_changed(&self, _notification: &NSNotification) {
            self.queried();
        }

        /// The keyboard went somewhere else (a window that is not key, another field): the view
        /// is done.
        #[unsafe(method(controlTextDidEndEditing:))]
        fn control_end_editing(&self, _notification: &NSNotification) {
            if !self.ivars().closing.get() {
                self.dismiss();
            }
        }

        /// The field's command hook: the keys the view owns.
        #[unsafe(method(control:textView:doCommandBySelector:))]
        fn control_do_command(
            &self,
            _control: &AnyObject,
            _text_view: &AnyObject,
            command: Sel,
        ) -> bool {
            self.command(command)
        }
    }
);

/// How the view looks for `theme`'s colours.
fn colours(theme: &Theme) -> Look {
    Look::of(theme)
}

impl OverviewView {
    fn new(mtm: MainThreadMarker, window: u64, state: Overview, theme: &Theme) -> Retained<Self> {
        let field = NSTextField::new(mtm);
        field.setBordered(false);
        field.setBezeled(false);
        field.setDrawsBackground(false);
        field.setFocusRingType(NSFocusRingType::None);
        field.setFont(Some(&NSFont::systemFontOfSize(FIELD_SIZE)));
        field.setAutoresizingMask(NSAutoresizingMaskOptions::empty());
        field.setTextColor(Some(&colours(theme).text.color()));
        let this = Self::alloc(mtm).set_ivars(OverviewIvars {
            window,
            state: RefCell::new(state),
            field: field.clone(),
            look: Cell::new(colours(theme)),
            row_look: Cell::new(selected_row(theme)),
            load: RefCell::new(None),
            viewed: RefCell::new(None),
            icons: RefCell::new(HashMap::new()),
            shots: RefCell::new(HashMap::new()),
            shot_tabs: RefCell::new(HashSet::new()),
            scroll: Cell::new(0.0),
            aspect: Cell::new(0.6),
            top: Cell::new(0.0),
            closing: Cell::new(false),
            generation: Cell::new(0),
            parked: Cell::new(false),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.addSubview(&field);
        // SAFETY: the view implements the field's delegate protocols (the three methods it uses
        // are defined above); the field holds its delegate weakly and the view owns the field.
        unsafe { field.setDelegate(Some(ProtocolObject::from_ref(&*this))) };
        this
    }

    fn track(&self) {
        for area in self.trackingAreas().iter() {
            self.removeTrackingArea(&area);
        }
        let options = NSTrackingAreaOptions::MouseMoved
            | NSTrackingAreaOptions::MouseEnteredAndExited
            | NSTrackingAreaOptions::ActiveInKeyWindow
            | NSTrackingAreaOptions::InVisibleRect;
        // SAFETY: `owner` is the view, which owns the area and outlives it; no user info.
        let area = unsafe {
            NSTrackingArea::initWithRect_options_owner_userInfo(
                NSTrackingArea::alloc(),
                NSRect::ZERO,
                options,
                Some(self),
                None,
            )
        };
        self.addTrackingArea(&area);
    }

    /// The window this view is in.
    fn terminal_window(&self) -> Option<Retained<TerminalWindow>> {
        app::delegate(self.mtm()).and_then(|app| app.window(self.ivars().window))
    }

    fn layout(&self) -> Layout {
        let iv = self.ivars();
        iv.state
            .borrow()
            .layout((self.bounds().size.width - SIDE).max(1.0), iv.aspect.get())
    }

    /// The viewport the grid scrolls in: from under the title row to the view's bottom.
    fn viewport(&self) -> f64 {
        (self.bounds().size.height - self.ivars().top.get() - TOP_AIR - PLATE_AREA).max(1.0)
    }

    /// Where the grid's origin is in the view: the grid's y is the view's plus this.
    fn origin_y(&self) -> f64 {
        let iv = self.ivars();
        iv.top.get() + TOP_AIR - iv.scroll.get()
    }

    /// The most the grid scrolls: its height and the air under it, less the viewport.
    fn max_scroll(&self, layout: &Layout) -> f64 {
        (layout.height + BOTTOM_AIR - self.viewport()).max(0.0)
    }

    fn scroll_to(&self, scroll: f64) {
        let layout = self.layout();
        let scroll = scroll.clamp(0.0, self.max_scroll(&layout));
        if (scroll - self.ivars().scroll.get()).abs() < f64::EPSILON {
            return;
        }
        self.ivars().scroll.set(scroll);
        self.shoot();
        self.setNeedsDisplay(true);
    }

    // ─── The pointer ─────────────────────────────────────────────────────

    /// The event's point in the grid's own coordinates.
    fn grid_point(&self, event: &NSEvent) -> (f64, f64) {
        let at = self.convertPoint_fromView(event.locationInWindow(), None);
        (at.x - SIDE, at.y - self.origin_y())
    }

    /// Selects the cell under the pointer, and names its pane under it; `true` when the pointer is
    /// over a cell.
    fn point_at(&self, event: &NSEvent) -> bool {
        let iv = self.ivars();
        if iv.closing.get() {
            return false;
        }
        let (x, y) = self.grid_point(event);
        let at = self.convertPoint_fromView(event.locationInWindow(), None);
        self.touch();
        if at.x < SIDE {
            self.glance_at(at.y);
            return false;
        }
        let layout = self.layout();
        let mut state = iv.state.borrow_mut();
        let glanced = state.glance(None);
        let Some(index) = layout.tab_at(x, y) else {
            drop(state);
            if glanced {
                self.setNeedsDisplay(true);
            }
            return false;
        };
        let tab = state.tab(index);
        let id = tab.id;
        let pane = layout
            .in_picture(index, x, y)
            .filter(|_| tab.panes.len() > 1)
            .and_then(|(fx, fy)| tab.pane_at(fx, fy));
        let before = (state.selected(), state.pointed());
        state.point(id, pane);
        if glanced || before != (state.selected(), state.pointed()) {
            drop(state);
            self.setNeedsDisplay(true);
        }
        true
    }

    // ─── The keyboard ────────────────────────────────────────────────────

    /// The field's command `command`: `true` when the view took it.
    fn command(&self, command: Sel) -> bool {
        if self.ivars().closing.get() {
            return false;
        }
        let direction = if command == sel!(moveLeft:) {
            Some(Direction::Left)
        } else if command == sel!(moveRight:) {
            Some(Direction::Right)
        } else if command == sel!(moveUp:) {
            Some(Direction::Up)
        } else if command == sel!(moveDown:) {
            Some(Direction::Down)
        } else {
            None
        };
        if let Some(direction) = direction {
            self.step(direction);
            return true;
        }
        if command == sel!(insertNewline:)
            || command == sel!(insertNewlineIgnoringFieldEditor:)
            || command == sel!(insertLineBreak:)
        {
            self.activate();
            true
        } else if command == sel!(cancelOperation:) {
            self.escape();
            true
        } else if command == sel!(insertTab:) {
            self.cycle(true);
            true
        } else if command == sel!(insertBacktab:) {
            self.cycle(false);
            true
        } else {
            false
        }
    }

    /// Esc: the search first, then the view.
    fn escape(&self) {
        let close = self.ivars().state.borrow_mut().escape();
        if close {
            self.dismiss();
        } else {
            self.ivars().field.setStringValue(&NSString::from_str(""));
            self.queried();
        }
    }

    /// The search row's text is the model's query now: the grid starts from the top, with the
    /// pictures of the tabs it keeps.
    fn queried(&self) {
        let iv = self.ivars();
        let text = iv.field.stringValue().to_string();
        iv.state.borrow_mut().set_query(&text);
        iv.scroll.set(0.0);
        self.shoot();
        self.setNeedsDisplay(true);
    }

    /// ⇥ (`forward`) and ⇧⇥: the next machine of the left column.
    fn cycle(&self, forward: bool) {
        self.ivars().state.borrow_mut().cycle(forward);
        self.machine_changed();
    }

    /// A click on a machine's row selects it.
    fn choose_machine(&self, machine: Machine) {
        if self.ivars().state.borrow_mut().set_machine(machine) {
            self.machine_changed();
        }
    }

    /// The grid is of another machine: it starts from the top, with its own pictures, and the
    /// server's load is asked for if it is one that has any.
    fn machine_changed(&self) {
        let iv = self.ivars();
        iv.scroll.set(0.0);
        let layout = self.layout();
        self.reveal(&layout);
        self.place_field();
        self.sync_view();
        self.touch();
        self.shoot();
        self.setNeedsDisplay(true);
    }

    /// The view is being used: the server it shows is sampled for another stretch.
    fn touch(&self) {
        if let Some(account) = self.ivars().viewed.borrow().as_deref() {
            server_stats::interaction(account);
        }
    }

    /// The server's load is asked for while the view shows it — the details, whether or not a
    /// pane of it is on screen — and only then: the account the view asked for before lets go.
    fn sync_view(&self) {
        let iv = self.ivars();
        let wanted = iv.state.borrow().account();
        let mut viewed = iv.viewed.borrow_mut();
        if *viewed == wanted {
            return;
        }
        if let Some(old) = viewed.take() {
            server_stats::set_view(&old, iv.window, false);
        }
        if let Some(account) = &wanted {
            server_stats::set_view(account, iv.window, true);
        }
        *viewed = wanted;
        drop(viewed);
        iv.load.replace(None);
        self.read_load();
    }

    /// The selected server's last sample, as the column says it.
    fn read_load(&self) {
        let iv = self.ivars();
        let load = iv.viewed.borrow().as_deref().and_then(|account| {
            let detail = server_stats::detail(account)?;
            Some((rules::load(&detail), server_stats::history(account)))
        });
        iv.load.replace(load);
    }

    // ─── The left column ─────────────────────────────────────────────────

    /// The block under the selected server's row: its height, 0 without a sample.
    fn load_height(&self) -> f64 {
        let load = self.ivars().load.borrow();
        let Some((load, levels)) = load.as_ref() else {
            return 0.0;
        };
        let lines = load.rows.len() + usize::from(load.os.is_some());
        2.0 * LOAD_PAD
            + lines as f64 * LOAD_LINE
            + if levels.is_empty() {
                0.0
            } else {
                SPARK_HEIGHT + 4.0
            }
    }

    /// The rows of the column and where they stand in the view; the selected server's row has
    /// its load block under it, which moves the rows below.
    fn column(&self) -> Vec<(rules::MachineRow, NSRect)> {
        let iv = self.ivars();
        let state = iv.state.borrow();
        let mut top = iv.top.get() + TOP_AIR;
        let block = self.load_height();
        state
            .machines()
            .into_iter()
            .map(|row| {
                let rect = NSRect::new(
                    NSPoint::new(SIDE_INSET, top),
                    NSSize::new(SIDE - 2.0 * SIDE_INSET, MACHINE_ROW),
                );
                top += MACHINE_ROW + MACHINE_GAP;
                if row.machine == *state.machine() && row.account.is_some() {
                    top += block;
                }
                (row, rect)
            })
            .collect()
    }

    /// The pointer is at `y` in the column: that machine's panes light, without selecting it.
    fn glance_at(&self, y: f64) {
        let machine = self
            .column()
            .into_iter()
            .find(|(_, rect)| y >= rect.origin.y && y < rect.origin.y + rect.size.height)
            .map(|(row, _)| row.machine);
        if self.ivars().state.borrow_mut().glance(machine) {
            self.setNeedsDisplay(true);
        }
    }

    /// A press in the column at `y`: the machine under it is selected.
    fn press_column(&self, y: f64) {
        let machine = self
            .column()
            .into_iter()
            .find(|(_, rect)| y >= rect.origin.y && y < rect.origin.y + rect.size.height)
            .map(|(row, _)| row.machine);
        if let Some(machine) = machine {
            self.choose_machine(machine);
        }
    }

    fn step(&self, direction: Direction) {
        let layout = self.layout();
        let moved = self.ivars().state.borrow_mut().step(&layout, direction);
        if !moved {
            return;
        }
        self.reveal(&layout);
        self.setNeedsDisplay(true);
    }

    /// Scrolls the selected cell into view.
    fn reveal(&self, layout: &Layout) {
        let iv = self.ivars();
        let Some(index) = iv.state.borrow().selected_index() else {
            return;
        };
        let scroll = layout.reveal(index, iv.scroll.get(), self.viewport(), REVEAL_AIR);
        let scroll = scroll.clamp(0.0, self.max_scroll(layout));
        if (scroll - iv.scroll.get()).abs() >= f64::EPSILON {
            iv.scroll.set(scroll);
            self.shoot();
        }
    }

    /// ⌘1…⌘9 ([`Self::perform_key_equivalent`]).
    fn equivalent(&self, event: &NSEvent) -> bool {
        if self.ivars().closing.get() {
            return false;
        }
        let Some(mods) = crate::menu::keys::family_mods(self.mtm(), "select_tab") else {
            return false;
        };
        let wanted = crate::menu::keys::flags_of(mods.into());
        let held = event.modifierFlags()
            & (NSEventModifierFlags::Command
                | NSEventModifierFlags::Option
                | NSEventModifierFlags::Control
                | NSEventModifierFlags::Shift);
        if held != wanted {
            return false;
        }
        let digit = event
            .charactersIgnoringModifiers()
            .and_then(|characters| characters.to_string().chars().next())
            .and_then(|character| character.to_digit(10))
            .and_then(|digit| u8::try_from(digit).ok())
            .filter(|digit| (1..=9).contains(digit));
        let (Some(tag), Some(window)) = (digit, self.terminal_window()) else {
            return false;
        };
        window.overview_digit(tag)
    }

    // ─── What it does ────────────────────────────────────────────────────

    /// ↵: the selected tab is shown, the pane under the pointer focused, and the view is gone.
    fn activate(&self) {
        let act = self.ivars().state.borrow().act();
        if let (Some(act), Some(window)) = (act, self.terminal_window()) {
            window.overview_go(act);
        }
    }

    /// The view asks to be taken down (Esc, the keyboard gone elsewhere).
    fn dismiss(&self) {
        if let Some(window) = self.terminal_window() {
            window.close_overview();
        }
    }

    // ─── Pictures ────────────────────────────────────────────────────────

    /// The tabs and what they report, read afresh, and a still of every pane in the viewport.
    fn reload(&self) {
        let iv = self.ivars();
        let Some(window) = self.terminal_window() else {
            return;
        };
        let theme = window.bar().theme();
        iv.look.set(colours(&theme));
        iv.row_look.set(selected_row(&theme));
        let (tabs, labels) = window.overview_facts();
        iv.icons.replace(
            labels
                .iter()
                .map(|label| {
                    (
                        label.tab,
                        palette_icon(self.mtm(), label.emblem, label.indicator, &theme),
                    )
                })
                .collect(),
        );
        iv.state.borrow_mut().refresh(tabs);
        self.sync_view();
        self.read_load();
        let layout = self.layout();
        iv.scroll
            .set(iv.scroll.get().clamp(0.0, self.max_scroll(&layout)));
        self.shoot();
        self.setNeedsDisplay(true);
    }

    /// A still of each pane of the tabs in the viewport, at the size it is drawn; a pane that
    /// gave none keeps the one it had. The tabs that left the viewport let go of theirs.
    fn shoot(&self) {
        let iv = self.ivars();
        let Some(window) = self.terminal_window() else {
            return;
        };
        if !window.window_visible() {
            return;
        }
        let scale = window.backing_scale();
        let layout = self.layout();
        let state = iv.state.borrow();
        let shown: HashSet<u64> = layout
            .visible(iv.scroll.get(), self.viewport())
            .into_iter()
            .map(|index| state.tab(index).id)
            .collect();
        for gone in iv.shot_tabs.borrow().difference(&shown) {
            window.release_tab_shots(*gone);
        }
        iv.shots
            .borrow_mut()
            .retain(|(tab, _), _| shown.contains(tab));
        for index in layout.visible(iv.scroll.get(), self.viewport()) {
            let tab = state.tab(index);
            let tiles: Vec<(u64, (f64, f64))> = tab
                .panes
                .iter()
                .filter_map(|pane| {
                    let at = layout.pane_rect(index, pane)?;
                    Some((pane.id, (at.width, at.height)))
                })
                .collect();
            for (pane, image) in window.tab_shots(tab.id, &tiles, scale) {
                if let Some(image) = image {
                    let size = tiles
                        .iter()
                        .find(|(id, _)| *id == pane)
                        .map_or((1.0, 1.0), |(_, size)| *size);
                    let picture = NSImage::initWithCGImage_size(
                        NSImage::alloc(),
                        &image,
                        NSSize::new(size.0, size.1),
                    );
                    iv.shots.borrow_mut().insert((tab.id, pane), picture);
                }
            }
        }
        iv.shot_tabs.replace(shown);
    }

    /// The panes let go of what they kept for the view's pictures.
    fn release(&self) {
        let iv = self.ivars();
        if let Some(window) = self.terminal_window() {
            for tab in iv.shot_tabs.borrow().iter() {
                window.release_tab_shots(*tab);
            }
        }
        iv.shot_tabs.borrow_mut().clear();
        iv.shots.borrow_mut().clear();
    }

    // ─── The wake ────────────────────────────────────────────────────────

    /// Sets the one delayed wake that takes the pictures afresh ([`tabs::MAP_REFRESH`]).
    fn schedule(&self) {
        let iv = self.ivars();
        let generation = iv.generation.get().wrapping_add(1);
        iv.generation.set(generation);
        let window = iv.window;
        arrange::after(tabs::MAP_REFRESH, move |app| {
            if let Some(window) = app.window(window) {
                window.overview_tick(generation);
            }
        });
    }

    /// The wake came: if it is the live one and the window is seen, everything is read again and
    /// the next wake set; a window nobody sees lets it lapse ([`Self::resume`]).
    pub(crate) fn tick(&self, generation: u64) {
        let iv = self.ivars();
        if iv.generation.get() != generation || iv.closing.get() {
            return;
        }
        let seen = self
            .terminal_window()
            .is_some_and(|window| window.window_visible());
        if !seen {
            iv.parked.set(true);
            return;
        }
        self.reload();
        self.schedule();
    }

    /// The window is seen again: a lapsed wake is set once more, with the pictures taken now.
    pub(crate) fn resume(&self) {
        let iv = self.ivars();
        if !iv.parked.replace(false) || iv.closing.get() {
            return;
        }
        self.reload();
        self.schedule();
    }

    /// The window's size changed: the view takes the root's, and the grid with it.
    pub(crate) fn refit(&self, frame: NSRect, aspect: f64, top: f64) {
        let iv = self.ivars();
        self.setFrame(frame);
        iv.aspect.set(aspect);
        iv.top.set(top);
        let layout = self.layout();
        iv.scroll
            .set(iv.scroll.get().clamp(0.0, self.max_scroll(&layout)));
        self.place_field();
        self.shoot();
        self.setNeedsDisplay(true);
    }

    /// The theme changed while the view is up.
    pub(crate) fn set_theme(&self, theme: &Theme) {
        self.ivars().look.set(colours(theme));
        self.ivars().row_look.set(selected_row(theme));
        self.ivars()
            .field
            .setTextColor(Some(&colours(theme).text.color()));
        self.place_field();
        self.reload();
    }

    // ─── Drawing ─────────────────────────────────────────────────────────

    fn paint(&self) {
        let iv = self.ivars();
        let look = iv.look.get();
        let bounds = self.bounds();
        look.ground.color().setFill();
        NSBezierPath::fillRect(bounds);
        let layout = self.layout();
        let state = iv.state.borrow();
        let origin = self.origin_y();
        // What scrolls goes under the title row, not over the window's buttons.
        NSGraphicsContext::saveGraphicsState_class();
        NSBezierPath::clipRect(NSRect::new(
            NSPoint::new(0.0, iv.top.get()),
            NSSize::new(
                bounds.size.width,
                (bounds.size.height - iv.top.get() - PLATE_AREA).max(0.0),
            ),
        ));
        let dim_font = NSFont::systemFontOfSize_weight(SMALL_SIZE, unsafe { NSFontWeightSemibold });
        let dim = attributes(&dim_font, look.dim);
        for (at, row) in layout.rows.iter().enumerate() {
            if let Row::Heading { text, .. } = row
                && let Some(rect) = layout.heading(at)
            {
                draw_text(
                    text,
                    rect.x + SIDE + 2.0,
                    rect.y + origin + rect.height * 0.35,
                    rect.height * 0.65,
                    &dim,
                );
            }
        }
        let selected = state.selected();
        for index in layout.visible(iv.scroll.get(), self.viewport()) {
            let tab = state.tab(index);
            self.paint_cell(&state, &layout, index, origin, selected == Some(tab.id));
        }
        NSGraphicsContext::restoreGraphicsState_class();
        // The column is over the grid's left, which has the room for it.
        self.paint_column(&state);
        self.paint_plate(&state);
    }

    /// The search row's plate, in the dock's tones, with the count and the keys at its right.
    fn plate(&self) -> NSRect {
        let bounds = self.bounds();
        NSRect::new(
            NSPoint::new(
                SIDE + PLATE_SIDE,
                bounds.size.height - PLATE_GAP - PLATE_HEIGHT,
            ),
            NSSize::new(
                (bounds.size.width - SIDE - 2.0 * PLATE_SIDE).max(1.0),
                PLATE_HEIGHT,
            ),
        )
    }

    /// How much of the plate's width the count and the keys take.
    fn keys_room(&self, plate: NSRect) -> f64 {
        (plate.size.width * KEYS_SHARE).min(KEYS_MAX)
    }

    /// The field stands in the plate left of the keys, and says what the row finds.
    fn place_field(&self) {
        let iv = self.ivars();
        let plate = self.plate();
        let height = 22.0;
        iv.field.setFrame(NSRect::new(
            NSPoint::new(
                plate.origin.x + PLATE_PAD,
                plate.origin.y + (PLATE_HEIGHT - height) / 2.0,
            ),
            NSSize::new(
                (plate.size.width - 2.0 * PLATE_PAD - self.keys_room(plate)).max(40.0),
                height,
            ),
        ));
        let look = iv.look.get();
        let text = iv.state.borrow().placeholder();
        let quiet = attributes(&NSFont::systemFontOfSize(FIELD_SIZE), look.dim);
        // SAFETY: the attributes are `NSColor`/`NSFont` under AppKit's two keys.
        let placeholder =
            unsafe { NSAttributedString::new_with_attributes(&NSString::from_str(&text), &quiet) };
        iv.field.setPlaceholderAttributedString(Some(&placeholder));
    }

    fn paint_plate(&self, state: &Overview) {
        let look = self.ivars().look.get();
        let plate = self.plate();
        rounded(plate, PLATE_RADIUS, look.band, Some(look.line));
        let small = attributes(&NSFont::systemFontOfSize(SMALL_SIZE), look.dim);
        let key = attributes(
            &NSFont::systemFontOfSize_weight(SMALL_SIZE, unsafe { NSFontWeightSemibold }),
            look.text,
        );
        let mut right = plate.origin.x + plate.size.width - PLATE_PAD;
        let least = plate.origin.x + plate.size.width - self.keys_room(plate);
        for (glyph, verb) in state.hints().into_iter().rev() {
            let word = format!(" {verb}");
            let width = text_width(glyph, &key) + text_width(&word, &small);
            if right - width < least {
                break;
            }
            let left = draw_text_right(&word, right, plate.origin.y, PLATE_HEIGHT, &small);
            draw_text_right(glyph, left, plate.origin.y, PLATE_HEIGHT, &key);
            right = left - text_width(glyph, &key) - 14.0;
        }
        if let Some(found) = state.found() {
            draw_text_right(&found, right, plate.origin.y, PLATE_HEIGHT, &small);
        }
    }

    /// The left column: the machines, the selected one wearing the side column's selected row,
    /// a server's load under it, the keys that move along it at the bottom.
    fn paint_column(&self, state: &Overview) {
        let iv = self.ivars();
        let look = iv.look.get();
        let (fill, line) = iv.row_look.get();
        let column = self.column();
        let name = |selected: bool| {
            attributes(
                &NSFont::systemFontOfSize_weight(NAME_SIZE, unsafe { NSFontWeightMedium }),
                if selected { look.text } else { look.dim },
            )
        };
        let small = attributes(&NSFont::systemFontOfSize(SMALL_SIZE), look.dim);
        for (row, rect) in &column {
            let selected = row.machine == *state.machine();
            if selected {
                rounded(*rect, tabs::TAB_RADIUS, fill, Some(line));
            }
            draw_text(
                &row.title,
                rect.origin.x + ROW_TEXT,
                rect.origin.y + 6.0,
                20.0,
                &name(selected),
            );
            draw_text(
                &row.count,
                rect.origin.x + ROW_TEXT,
                rect.origin.y + 25.0,
                16.0,
                &small,
            );
            if selected && row.account.is_some() {
                self.paint_load(rect.origin.y + rect.size.height + LOAD_PAD, &small);
            }
        }
        let bottom = self.bounds().size.height;
        draw_text(
            HINT,
            SIDE_INSET + ROW_TEXT,
            bottom - 44.0,
            18.0,
            &attributes(&NSFont::systemFontOfSize(SMALL_SIZE - 0.5), look.dim),
        );
    }

    /// A server's load under its row: the CPU's last levels as bars, then label and value for
    /// the CPU, memory, disk and uptime, and the operating system last.
    fn paint_load(&self, top: f64, small: &Attributes) {
        let iv = self.ivars();
        let look = iv.look.get();
        let load = iv.load.borrow();
        let Some((load, levels)) = load.as_ref() else {
            return;
        };
        let (left, right) = (SIDE_INSET + ROW_TEXT, SIDE - SIDE_INSET - ROW_TEXT);
        let mut y = top;
        if !levels.is_empty() {
            for (at, level) in levels.iter().enumerate() {
                let height = (f64::from(*level) + 1.0) / 8.0 * SPARK_HEIGHT;
                let bar = NSRect::new(
                    NSPoint::new(
                        left + at as f64 * (SPARK_BAR + SPARK_GAP),
                        y + SPARK_HEIGHT - height,
                    ),
                    NSSize::new(SPARK_BAR, height),
                );
                look.accent.color().setFill();
                NSBezierPath::fillRect(bar);
            }
            y += SPARK_HEIGHT + 4.0;
        }
        let text = attributes(&NSFont::systemFontOfSize(SMALL_SIZE), look.text);
        for (label, value) in &load.rows {
            draw_text(label, left, y, LOAD_LINE, small);
            draw_text_right(value, right, y, LOAD_LINE, &text);
            y += LOAD_LINE;
        }
        if let Some(os) = &load.os {
            let os = fit(os, right - left, small);
            draw_text(&os, left, y, LOAD_LINE, small);
        }
    }

    fn paint_cell(
        &self,
        state: &Overview,
        layout: &Layout,
        index: usize,
        origin: f64,
        selected: bool,
    ) {
        let iv = self.ivars();
        let look = iv.look.get();
        let tab = state.tab(index);
        let (Some(picture), Some(name_row)) = (layout.picture(index), layout.name_row(index))
        else {
            return;
        };
        let at = |rect: Rect| {
            NSRect::new(
                NSPoint::new(rect.x + SIDE, rect.y + origin),
                NSSize::new(rect.width, rect.height),
            )
        };
        // The pictures: each pane where the tab's tree puts it, a thin gap between neighbours.
        NSGraphicsContext::saveGraphicsState_class();
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(at(picture), RADIUS, RADIUS)
            .addClip();
        look.band.color().setFill();
        NSBezierPath::fillRect(at(picture));
        let shots = iv.shots.borrow();
        for pane in &tab.panes {
            let Some(rect) = layout.pane_rect(index, pane) else {
                continue;
            };
            let rect = at(rect);
            if let Some(shot) = shots.get(&(tab.id, pane.id)) {
                draw_image(shot, rect);
            } else {
                look.ground.color().setFill();
                NSBezierPath::fillRect(rect);
            }
            if state.dim(tab.id, pane) {
                Tint::of(look.ground.rgb, DIM_COVER).color().setFill();
                NSBezierPath::fillRect(rect);
            }
        }
        NSGraphicsContext::restoreGraphicsState_class();
        if selected {
            let ring = NSRect::new(
                NSPoint::new(picture.x + SIDE - RING_AIR, picture.y + origin - RING_AIR),
                NSSize::new(
                    picture.width + 2.0 * RING_AIR,
                    picture.height + 2.0 * RING_AIR,
                ),
            );
            let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                ring,
                RADIUS + RING_AIR,
                RADIUS + RING_AIR,
            );
            path.setLineWidth(RING);
            look.accent.color().setStroke();
            path.stroke();
        }
        // The name row: the tab's icon, its name, what ↵ does when it is selected, its key.
        let row = at(name_row);
        let side = PALETTE_ICON_SIDE;
        let mut left = row.origin.x + 2.0;
        if let Some(icon) = iv.icons.borrow().get(&tab.id) {
            icon.drawInRect(NSRect::new(
                NSPoint::new(left, row.origin.y + (row.size.height - side) / 2.0),
                NSSize::new(side, side),
            ));
            left += side + GAP_AFTER_ICON;
        }
        let mut right = row.origin.x + row.size.width - 2.0;
        let small = attributes(&NSFont::systemFontOfSize(SMALL_SIZE), look.dim);
        if let Some(key) = &tab.key {
            right =
                draw_text_right(key, right, row.origin.y, row.size.height, &small) - GAP_AFTER_NAME;
        }
        let strong = attributes(
            &NSFont::systemFontOfSize_weight(NAME_SIZE, unsafe { NSFontWeightMedium }),
            if selected { look.text } else { look.dim },
        );
        let verb = state
            .verb()
            .filter(|_| selected)
            .map(|verb| format!("↵ {verb}"));
        let verb_width = verb.as_ref().map_or(0.0, |verb| text_width(verb, &small));
        let room = (right - left - verb_width - GAP_AFTER_NAME).max(0.0);
        let marked = attributes(
            &NSFont::systemFontOfSize_weight(NAME_SIZE, unsafe { NSFontWeightSemibold }),
            look.accent,
        );
        let hit = state.hit(tab.id);
        let name_marks = hit.map_or(&[][..], |hit| hit.name_marks.as_slice());
        let mut end = draw_marked(
            &tab.name,
            name_marks,
            left,
            room,
            row.origin.y,
            row.size.height,
            &strong,
            &marked,
        );
        // The program the search found in a pane, beside the name: `api  cargo test`.
        if let Some((program, marks)) = hit.and_then(|hit| hit.program.as_ref()) {
            let spare = room - (end - left) - GAP_AFTER_NAME;
            if spare > 24.0 {
                end = draw_marked(
                    program,
                    marks,
                    end + GAP_AFTER_NAME,
                    spare,
                    row.origin.y,
                    row.size.height,
                    &small,
                    &marked,
                );
            }
        }
        if let Some(verb) = verb {
            draw_text(
                &verb,
                end + GAP_AFTER_NAME,
                row.origin.y,
                row.size.height,
                &small,
            );
        }
    }

    // ─── Opening ─────────────────────────────────────────────────────────

    /// The view settles from slightly larger and fades in, once (Core Animation; none with
    /// Reduce Motion).
    fn appear(&self) {
        let reduce = app::delegate(self.mtm()).is_none_or(|app| app.reduce_motion());
        if reduce {
            return;
        }
        self.setWantsLayer(true);
        if let Some(layer) = self.layer() {
            settle(&layer);
        }
    }
}

/// `layer` plays from a little larger and clear to rest.
fn settle(layer: &CALayer) {
    play(
        layer,
        "transform.scale",
        OPEN_SCALE,
        1.0,
        OPEN_SECS,
        Ease::Out,
        0.0,
    );
    play(layer, "opacity", 0.0, 1.0, OPEN_SECS, Ease::Out, 0.0);
}

/// `image` in `rect`, the right way up in a flipped drawing too.
fn draw_image(image: &NSImage, rect: NSRect) {
    // SAFETY: no hints.
    unsafe {
        image.drawInRect_fromRect_operation_fraction_respectFlipped_hints(
            rect,
            NSRect::ZERO,
            NSCompositingOperation::SourceOver,
            1.0,
            true,
            None,
        );
    }
}

/// `text` with the letters at `marks` in `marked`, cut with an ellipsis to fit `room` points from
/// `x`, centred in the slot `[top, top + height)`; returns where it ended (`x` if not even a
/// letter fits).
#[allow(clippy::too_many_arguments)]
fn draw_marked(
    text: &str,
    marks: &[usize],
    x: f64,
    room: f64,
    top: f64,
    height: f64,
    plain: &Attributes,
    marked: &Attributes,
) -> f64 {
    let chars: Vec<char> = text.chars().collect();
    let mut keep = chars.len();
    let mut string = styled(&chars, marks, keep, plain, marked);
    let mut width = string.size().width;
    while width > room && keep > 1 {
        // A guess that lands close, then single steps.
        let guess = ((keep as f64) * room / width) as usize;
        keep = guess.clamp(1, keep - 1);
        string = styled(&chars, marks, keep, plain, marked);
        width = string.size().width;
    }
    if width > room {
        return x;
    }
    string.drawAtPoint(NSPoint::new(x, top + (height - string.size().height) / 2.0));
    x + width
}

/// `text` cut to fit `width` points, ended with an ellipsis when it was cut.
fn fit(text: &str, width: f64, attributes: &Attributes) -> String {
    if text_width(text, attributes) <= width {
        return text.to_owned();
    }
    let mut kept: Vec<char> = text.chars().collect();
    while !kept.is_empty() {
        kept.pop();
        let candidate: String = kept.iter().collect::<String>() + "…";
        if text_width(&candidate, attributes) <= width {
            return candidate;
        }
    }
    String::new()
}

// ─── The window's side of it ──────────────────────────────────────────────

/// Window ▸ Show All Tabs (⇧⌘\): the view opens over the window, or is taken down.
pub(crate) fn toggle(window: &TerminalWindow) {
    if window.root().overview().is_some() {
        window.close_overview();
    } else {
        open(window);
    }
}

/// Whether the view may open now: the palette's rule, a question up on the window or on the tab on
/// screen would be under it.
pub(crate) fn may_open(window: &TerminalWindow) -> bool {
    window.palette_may_open()
}

/// Puts the view up over the window's content, the selected tab the window shows, and gives
/// the keyboard to its field.
pub(crate) fn open(window: &TerminalWindow) {
    let mtm = window.mtm();
    let root = window.root();
    if root.overview().is_some() || !may_open(window) {
        return;
    }
    window.close_palette();
    let theme = window.bar().theme();
    let (tabs, _) = window.overview_facts();
    let here = window.selected_tab_id();
    let view = OverviewView::new(mtm, window.id(), Overview::new(tabs, here), &theme);
    let content = root.content();
    view.refit(
        root.bounds(),
        content.size.height / content.size.width.max(1.0),
        root.title_row(),
    );
    // Over everything the window holds, the side column's edge included.
    root.addSubview_positioned_relativeTo(&view, NSWindowOrderingMode::Above, None);
    root.seat_overview(Some(view.clone()));
    view.reload();
    let layout = view.layout();
    view.reveal(&layout);
    view.shoot();
    if let Some(ns_window) = view.window() {
        ns_window.makeFirstResponder(Some(&view.ivars().field));
    }
    view.appear();
    view.schedule();
}

/// Takes the view down: the window has none from here on, the panes let go of the pictures
/// they kept for it and the wake finds nothing to do.
pub(crate) fn close(root: &RootView) {
    let Some(view) = root.overview() else {
        return;
    };
    let iv = view.ivars();
    iv.closing.set(true);
    iv.generation.set(iv.generation.get().wrapping_add(1));
    root.seat_overview(None);
    if let Some(account) = iv.viewed.take() {
        server_stats::set_view(&account, iv.window, false);
    }
    view.release();
    view.removeFromSuperview();
}
