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

mod motion;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::ptr::NonNull;
use std::rc::Rc;
use std::time::Duration;

use block2::RcBlock;
use bt_core::Theme;
use bt_shell_common::kablo::SessionId;
use bt_shell_common::overview::{
    self as rules, Cell as Tile, Direction, Layout, Machine, Overview, Row,
};
use bt_shell_common::server_card;
use bt_shell_common::split::Rect;
use bt_shell_common::tabs::{self, CardPane};
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel,
};
use objc2_app_kit::{
    NSAffineTransformNSAppKitAdditions, NSAlert, NSAlertFirstButtonReturn,
    NSAnimatablePropertyContainer, NSAnimationContext, NSApplication,
    NSAttributedStringNSStringDrawing, NSAutoresizingMaskOptions, NSBezierPath, NSColor,
    NSCompositingOperation, NSControlTextEditingDelegate, NSEvent, NSEventModifierFlags,
    NSFocusRingType, NSFont, NSFontWeightMedium, NSFontWeightSemibold, NSGradient,
    NSGraphicsContext, NSImage, NSMenu, NSMenuItem, NSModalResponse, NSTextField,
    NSTextFieldDelegate, NSTrackingArea, NSTrackingAreaOptions, NSView, NSWindowOrderingMode,
};
use objc2_core_graphics::CGContext;
use objc2_foundation::{
    NSAffineTransform, NSArray, NSNotification, NSObjectProtocol, NSPoint, NSRect, NSRunLoop,
    NSRunLoopCommonModes, NSSize, NSString, NSValue, ns_string,
};
use objc2_quartz_core::{
    CABasicAnimation, CACurrentMediaTime, CADisplayLink, CALayer, CAMediaTiming,
    CAMediaTimingFunction, CATransform3D, NSValueCATransform3DAdditions, kCAFillModeForwards,
    kCAMediaTimingFunctionEaseIn,
};

use crate::app;
use crate::arrange;
use crate::card::{self, IDENTITY};
use crate::palette::{
    Attributes, Look, attributes, draw_text, draw_text_right, styled, text_width,
};
use crate::screens::Screens;
use crate::server_stats;
use crate::sheets::{self, Asker};
use crate::tab_bar::{
    Ease, PALETTE_ICON_SIDE, Tint, palette_icon, play, play_look, rounded, selected_row,
};
use crate::window::{RootView, TerminalWindow};

use self::motion::{
    ARRIVE_SCALE, FILTER_SECS, Frame, From, Heading, Id, LEAVE_SCALE, Motion, Placed, SELECT_SECS,
    Was, arriving, ease, leaving, lerp, lerp_rect, scaled,
};

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
/// Where the field's own text stands in it (measured against AppKit's placeholder on the same
/// field), which the row's words while nothing is typed share.
const FIELD_TEXT_X: f64 = 2.0;
const FIELD_TEXT_Y: f64 = -2.5;
/// The most of the plate's width the keys and the count at its right take.
const KEYS_SHARE: f64 = 0.45;
const KEYS_MAX: f64 = 340.0;
/// The corner of a picture and of the ring round the selected one.
const RADIUS: f64 = 8.0;
const RING: f64 = 2.5;
const RING_AIR: f64 = 4.0;
/// How long the window's size rests before every picture is taken again at it.
const SETTLE: Duration = Duration::from_millis(150);
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
/// The line under the selected tile of another computer's terminal that says what ↵ does: it
/// stands in the gap between rows (`rules::ROW_GAP`).
const NOTE_HEIGHT: f64 = 16.0;

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
/// The air each side of a section heading's *Open All in New Tab*.
const BUTTON_PAD: f64 = 8.0;
const HINT: &str = "⇥ next machine   ⇧⇥ previous";
/// The key that does what a selected cell's verb says, and the least of a name row its name keeps
/// beside the verb (a row narrower than that shows the name alone).
const RETURN_KEY: &str = "↵";
const NAME_LEAST: f64 = 48.0;

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
    /// The sessions the servers' screens built and the stage that draws them.
    screens: RefCell<Screens>,
    /// The last picture of each terminal in the background, at the tile's size.
    away_shots: RefCell<HashMap<rules::Key, Retained<NSImage>>>,
    /// The terminals whose screen was asked for and has not come.
    asked: RefCell<HashSet<rules::Key>>,
    /// A press on a terminal's tile, until the button is up.
    press: RefCell<Option<Press>>,
    /// Where a drag of a terminal would drop: the tab and its pane under the pointer.
    drop: Cell<Option<(u64, u64)>>,
    /// The terminal the context menu is for.
    menu_for: RefCell<Option<rules::Key>>,
    /// The change on its way while one is, and the display link that draws it frame by frame
    /// ([`motion`]).
    motion: RefCell<Option<Motion>>,
    link: RefCell<Option<Retained<CADisplayLink>>>,
    /// The pictures' outline ([`edge_of`]).
    edge: Cell<Tint>,
}

/// A press on a terminal's tile: a release without a drag opens the terminal, a drag carries it
/// to a pane of a tab.
struct Press {
    key: rules::Key,
    at: NSPoint,
    dragging: bool,
}

/// How far a press moves before it is a drag.
const DRAG_START: f64 = 6.0;

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

        /// A frame of the display while something moves: the next picture of it, or its end.
        #[unsafe(method(motionTick:))]
        fn motion_tick(&self, _link: &CADisplayLink) {
            self.ticked();
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
            // A view on its way out still hears its area; the pointer is the window's again.
            if !self.ivars().closing.get() {
                self.point_at(event);
            }
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            if self.ivars().closing.get() {
                return;
            }
            let from = self.capture(false);
            let selected = self.ivars().state.borrow().selected();
            if let Some(selected) = selected {
                self.ivars().state.borrow_mut().select(selected);
            }
            self.ivars().state.borrow_mut().glance(None);
            self.start(from, SELECT_SECS);
        }

        /// A press on a tab is ↵ on it; on a terminal in the background it is ↵ once the button
        /// is up, unless it was dragged; on a section's *Open All in New Tab*, that.
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let at = self.convertPoint_fromView(event.locationInWindow(), None);
            if at.x < SIDE {
                self.press_column(at.y);
            } else if let Some(host) = self.section_button_at(event) {
                if let Some(window) = self.terminal_window() {
                    window.overview_open_all(&host);
                }
            } else if self.point_at(event) {
                let key = {
                    let state = self.ivars().state.borrow();
                    // A click on a terminal another computer has open only selects it: taking
                    // it from there is ↵ alone (as in the palette's page), never an accident of the pointer.
                    if state.selected_other() {
                        return;
                    }
                    state.selected_away().cloned()
                };
                match key {
                    Some(key) => {
                        self.ivars().press.replace(Some(Press {
                            key,
                            at,
                            dragging: false,
                        }));
                    }
                    None => self.activate(),
                }
            }
        }

        /// A terminal's tile carried over a tab: the pane it would open beside is ringed.
        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            self.dragged(event);
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            self.released(event);
        }

        /// A terminal's tile has one item: *End Terminal*.
        #[unsafe(method(rightMouseDown:))]
        fn right_mouse_down(&self, event: &NSEvent) {
            if self.ivars().closing.get() {
                return;
            }
            let at = self.convertPoint_fromView(event.locationInWindow(), None);
            if at.x < SIDE || !self.point_at(event) {
                return;
            }
            let key = {
                let state = self.ivars().state.borrow();
                // End Terminal is for the server's own: another computer's terminal is that
                // computer's to end (the server card offers it for those only).
                (!state.selected_other())
                    .then(|| state.selected_away().cloned())
                    .flatten()
            };
            let Some(key) = key else {
                return;
            };
            self.ivars().menu_for.replace(Some(key));
            let menu = NSMenu::new(self.mtm());
            menu.setAutoenablesItems(false);
            // SAFETY: `initWithTitle:action:keyEquivalent:` takes two `NSString`s and a selector
            // the target implements (`endTerminal:` below); the view is the item's target for as
            // long as the menu is up.
            let item = unsafe {
                let item = NSMenuItem::initWithTitle_action_keyEquivalent(
                    NSMenuItem::alloc(self.mtm()),
                    &NSString::from_str(rules::END_TERMINAL),
                    Some(sel!(endTerminal:)),
                    ns_string!(""),
                );
                item.setTarget(Some(self));
                item
            };
            menu.addItem(&item);
            menu.popUpMenuPositioningItem_atLocation_inView(None, at, Some(self));
        }

        #[unsafe(method(endTerminal:))]
        fn end_terminal(&self, _sender: Option<&AnyObject>) {
            self.ask_end();
        }

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

/// The hairline round a picture, so a screen as light or as dark as the ground still has an
/// edge: white in the dark, black in the light, faint — never a tint of the ground, which would
/// read as dirt on the edge.
fn edge_of(theme: &Theme) -> Tint {
    Tint::of(
        if theme.is_dark() {
            0xff_ff_ff
        } else {
            0x00_00_00
        },
        0.1,
    )
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
            screens: RefCell::new(Screens::default()),
            away_shots: RefCell::new(HashMap::new()),
            asked: RefCell::new(HashSet::new()),
            press: RefCell::new(None),
            drop: Cell::new(None),
            menu_for: RefCell::new(None),
            motion: RefCell::new(None),
            link: RefCell::new(None),
            edge: Cell::new(edge_of(theme)),
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
        // The ring glides to the cell the pointer came to.
        let from = self.capture(false);
        let layout = self.layout();
        let mut state = iv.state.borrow_mut();
        let glanced = state.glance(None);
        let Some(index) = layout.tab_at(x, y) else {
            drop(state);
            if glanced {
                self.start(from, SELECT_SECS);
            }
            return false;
        };
        if index >= state.len() {
            let before = state.selected_cell();
            state.point_cell(index);
            if glanced || before != state.selected_cell() {
                drop(state);
                self.start(from, SELECT_SECS);
            }
            return true;
        }
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
            self.start(from, SELECT_SECS);
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
            // ⌥↵ opens a terminal in the background beside the pane on screen; on a tab it is ↵.
            let option = NSApplication::sharedApplication(self.mtm())
                .currentEvent()
                .is_some_and(|event| event.modifierFlags().contains(NSEventModifierFlags::Option));
            if option {
                self.activate_beside();
            } else {
                self.activate();
            }
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
        let from = self.capture(true);
        iv.state.borrow_mut().set_query(&text);
        iv.scroll.set(0.0);
        self.shoot();
        self.start(from, FILTER_SECS);
    }

    /// ⇥ (`forward`) and ⇧⇥: the next machine of the left column.
    fn cycle(&self, forward: bool) {
        let from = self.capture(true);
        self.ivars().state.borrow_mut().cycle(forward);
        self.machine_changed(from);
    }

    /// A click on a machine's row selects it.
    fn choose_machine(&self, machine: Machine) {
        let from = self.capture(true);
        if self.ivars().state.borrow_mut().set_machine(machine) {
            self.machine_changed(from);
        }
    }

    /// The grid is of another machine: it starts from the top, with its own pictures, and the
    /// server's load is asked for if it is one that has any. What was on screen (`from`) moves
    /// to it: the tiles that stay slide, the others go and come.
    fn machine_changed(&self, from: Option<From>) {
        let iv = self.ivars();
        iv.scroll.set(0.0);
        let layout = self.layout();
        self.reveal(&layout);
        self.place_field();
        self.sync_view();
        self.touch();
        self.shoot();
        self.start(from, FILTER_SECS);
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
        let from = self.capture(false);
        if self.ivars().state.borrow_mut().glance(machine) {
            self.start(from, SELECT_SECS);
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

    /// An arrow: the ring glides to the next cell, and the grid with it when that one was out
    /// of sight.
    fn step(&self, direction: Direction) {
        let layout = self.layout();
        let from = self.capture(false);
        let moved = self.ivars().state.borrow_mut().step(&layout, direction);
        if !moved {
            return;
        }
        self.reveal(&layout);
        self.start(from, SELECT_SECS);
    }

    /// Scrolls the selected cell into view.
    fn reveal(&self, layout: &Layout) {
        let iv = self.ivars();
        let Some(index) = iv.state.borrow().selected_cell() else {
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
        // ⌘↵ opens every terminal of the server in one tab (nothing when there is no server).
        const RETURN: u16 = 36;
        const ENTER: u16 = 76;
        let held = event.modifierFlags()
            & (NSEventModifierFlags::Command
                | NSEventModifierFlags::Option
                | NSEventModifierFlags::Control
                | NSEventModifierFlags::Shift);
        if matches!(event.keyCode(), RETURN | ENTER) && held == NSEventModifierFlags::Command {
            self.activate_all();
            return true;
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

    /// ⌥↵: the selected terminal in the background opens beside the pane on screen; a tab is
    /// just shown.
    fn activate_beside(&self) {
        let act = {
            let state = self.ivars().state.borrow();
            // A terminal another computer has open is moved here by ↵ alone: it has no
            // "beside" and ⌥↵ must not fall back to the plain ↵.
            if state.selected_other() {
                None
            } else {
                state.act_beside().or_else(|| state.act())
            }
        };
        match (act, self.terminal_window()) {
            (Some(act), Some(window)) => window.overview_go(act),
            _ => crate::preview::beep(),
        }
    }

    /// ⌘↵: every terminal the selected terminal's server (or the selected server) keeps, in a
    /// new tab.
    fn activate_all(&self) {
        let host = self.ivars().state.borrow().open_all_host();
        match (host, self.terminal_window()) {
            (Some(host), Some(window)) => window.overview_open_all(&host),
            _ => crate::preview::beep(),
        }
    }

    /// The section heading's *Open All in New Tab* under the pointer: the server it is of.
    fn section_button_at(&self, event: &NSEvent) -> Option<String> {
        let iv = self.ivars();
        let (x, y) = self.grid_point(event);
        let layout = self.layout();
        let state = iv.state.borrow();
        let width = text_width(rules::OPEN_ALL, &self.heading_attributes()) + 2.0 * BUTTON_PAD;
        layout.rows.iter().enumerate().find_map(|(at, row)| {
            let Row::Section {
                section,
                other: false,
                ..
            } = row
            else {
                return None;
            };
            let rect = layout.heading(at)?;
            let inside = x >= rect.x + rect.width - width
                && x < rect.x + rect.width
                && y >= rect.y
                && y < rect.y + rect.height;
            if inside {
                Some(state.sections().get(*section)?.host.clone())
            } else {
                None
            }
        })
    }

    /// The text of the dim headings.
    fn heading_attributes(&self) -> Attributes {
        let dim_font = NSFont::systemFontOfSize_weight(SMALL_SIZE, unsafe { NSFontWeightSemibold });
        attributes(&dim_font, self.ivars().look.get().dim)
    }

    /// The tab and the pane of it under `event`, when it is over a tab's picture.
    fn drop_at(&self, event: &NSEvent) -> Option<(u64, u64)> {
        let (x, y) = self.grid_point(event);
        let layout = self.layout();
        let state = self.ivars().state.borrow();
        let index = layout.tab_at(x, y).filter(|&index| index < state.len())?;
        let tab = state.tab(index);
        let (fx, fy) = layout.in_picture(index, x, y)?;
        Some((tab.id, tab.pane_at(fx, fy)?))
    }

    /// The press moved: past the threshold it is a drag, and the pane under the pointer is the
    /// one the terminal would open beside.
    fn dragged(&self, event: &NSEvent) {
        let iv = self.ivars();
        let at = self.convertPoint_fromView(event.locationInWindow(), None);
        let dragging = {
            let mut press = iv.press.borrow_mut();
            let Some(press) = press.as_mut() else {
                return;
            };
            if !press.dragging && (at.x - press.at.x).hypot(at.y - press.at.y) >= DRAG_START {
                press.dragging = true;
            }
            press.dragging
        };
        if !dragging {
            return;
        }
        let target = self.drop_at(event);
        if iv.drop.replace(target) != target {
            self.setNeedsDisplay(true);
        }
    }

    /// The button is up: a press that was not dragged opens the terminal, a drag ends on the
    /// pane under the pointer, if there is one.
    fn released(&self, event: &NSEvent) {
        let iv = self.ivars();
        let Some(press) = iv.press.take() else {
            return;
        };
        let target = iv.drop.take();
        if press.dragging {
            self.setNeedsDisplay(true);
            if let (Some((tab, pane)), Some(window)) = (
                target.or_else(|| self.drop_at(event)),
                self.terminal_window(),
            ) {
                window.overview_drop(press.key.0, press.key.1, tab, pane);
            }
        } else if self.point_at(event) && iv.state.borrow().selected_away() == Some(&press.key) {
            // The pointer may have left the tile it went down on (within the drag threshold);
            // the release opens the terminal it went down on, never the one it ended over.
            self.activate();
        }
    }

    /// *End Terminal*: the question closing a pane asks, as a sheet on the window; only an
    /// answer of End reaches the server.
    fn ask_end(&self) {
        let iv = self.ivars();
        let Some(key) = iv.menu_for.take() else {
            return;
        };
        let found = {
            let state = iv.state.borrow();
            state.sections().iter().find_map(|section| {
                section
                    .entries
                    .iter()
                    .find(|entry| (&entry.account, &entry.terminal) == (&key.0, &key.1))
                    .map(|entry| (section.host.clone(), entry.clone()))
            })
        };
        let (Some((host, entry)), Some(window)) = (found, self.window()) else {
            return;
        };
        let Some(seat) = sheets::seat(Asker::Window(&window)).filter(|seat| !seat.is_taken())
        else {
            // A question is up already: nothing is ended without one.
            return;
        };
        let (title, message) = server_card::end_text(&host, &entry);
        let alert = NSAlert::new(self.mtm());
        alert.setMessageText(&NSString::from_str(&title));
        alert.setInformativeText(&NSString::from_str(&message));
        alert.addButtonWithTitle(&NSString::from_str(server_card::END));
        let cancel = alert.addButtonWithTitle(ns_string!("Cancel"));
        // Esc by hand (the rationale of `window::alert`).
        cancel.setKeyEquivalent(ns_string!("\u{1b}"));
        let answered = RcBlock::new(move |response: NSModalResponse| {
            if response != NSAlertFirstButtonReturn {
                return;
            }
            if let Ok(terminal) = SessionId::new(&entry.terminal) {
                crate::hold_service::end_terminal(&entry.account, &terminal);
            }
        });
        seat.begin(&alert, &answered);
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
        iv.edge.set(edge_of(&theme));
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
        {
            let mut state = iv.state.borrow_mut();
            state.refresh(tabs);
            state.set_background(background_sections());
        }
        self.sync_view();
        self.read_load();
        let layout = self.layout();
        iv.scroll
            .set(iv.scroll.get().clamp(0.0, self.max_scroll(&layout)));
        self.shoot();
        self.fetch(true);
        self.setNeedsDisplay(true);
    }

    /// The servers' screens of the terminals in the viewport are asked for: every one when
    /// `all` (the wake, a new look), else only those with no picture yet (a scroll). The
    /// pictures of the terminals that left the viewport are let go of.
    fn fetch(&self, all: bool) {
        let iv = self.ivars();
        let layout = self.layout();
        let wanted: Vec<rules::Key> = {
            let state = iv.state.borrow();
            layout
                .visible(iv.scroll.get(), self.viewport())
                .into_iter()
                .filter_map(|index| match state.cell(index)? {
                    Tile::Away { entry, .. } => {
                        Some((entry.account.clone(), entry.terminal.clone()))
                    }
                    Tile::Tab(_) => None,
                })
                .collect()
        };
        iv.away_shots
            .borrow_mut()
            .retain(|key, _| wanted.contains(key));
        iv.screens.borrow_mut().retain(|key| wanted.contains(key));
        let window = iv.window;
        for key in wanted {
            if iv.asked.borrow().contains(&key)
                || (!all && iv.away_shots.borrow().contains_key(&key))
            {
                continue;
            }
            let Ok(terminal) = SessionId::new(&key.1) else {
                continue;
            };
            iv.asked.borrow_mut().insert(key.clone());
            let account = key.0.clone();
            crate::hold_service::picture(&account, &terminal, move |screen| {
                DispatchQueue::main().exec_async(move || {
                    let Some(mtm) = objc2::MainThreadMarker::new() else {
                        return;
                    };
                    if let Some(window) = app::delegate(mtm).and_then(|app| app.window(window)) {
                        window.overview_screen(key, screen);
                    }
                });
            });
        }
    }

    /// The server's screen of `key` came (`None`: it would not give it). It is built into a
    /// session and drawn at the tile's size, unless it is the screen the tile has.
    pub(crate) fn screen_arrived(
        &self,
        key: &rules::Key,
        screen: Option<bt_shell_common::kablo::service::Screen>,
    ) {
        let iv = self.ivars();
        iv.asked.borrow_mut().remove(key);
        let (Some(screen), Some(window)) = (screen, self.terminal_window()) else {
            return;
        };
        if iv.closing.get() || !window.window_visible() {
            return;
        }
        let Some(app) = app::delegate(self.mtm()) else {
            return;
        };
        let (scale, theme) = (window.backing_scale(), window.bar().theme());
        let settings = app.settings();
        let layout = self.layout();
        let tile = (
            (layout.tile.0 * scale).round().max(1.0) as u32,
            (layout.tile.1 * scale).round().max(1.0) as u32,
        );
        let mut screens = iv.screens.borrow_mut();
        let changed = screens.arrived(key.clone(), &screen, &settings, &theme, scale);
        // The picture is drawn again when the screen changed or the tile it is of did (the
        // window was resized); the same screen at the same size is what the tile shows.
        let sized = iv.away_shots.borrow().get(key).is_some_and(|shot| {
            let size = shot.size();
            size.width == layout.tile.0 && size.height == layout.tile.1
        });
        if !changed && sized {
            return;
        }
        let Some(image) = screens.image(key, &settings, scale, tile) else {
            return;
        };
        let picture = NSImage::initWithCGImage_size(
            NSImage::alloc(),
            &image,
            NSSize::new(layout.tile.0, layout.tile.1),
        );
        iv.away_shots.borrow_mut().insert(key.clone(), picture);
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
        let tabs: Vec<usize> = layout
            .visible(iv.scroll.get(), self.viewport())
            .into_iter()
            .filter(|&index| index < state.len())
            .collect();
        let shown: HashSet<u64> = tabs.iter().map(|&index| state.tab(index).id).collect();
        for gone in iv.shot_tabs.borrow().difference(&shown) {
            window.release_tab_shots(*gone);
        }
        iv.shots
            .borrow_mut()
            .retain(|(tab, _), _| shown.contains(tab));
        for index in tabs {
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
        drop(state);
        self.fetch(false);
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
        iv.screens.borrow_mut().release();
        iv.away_shots.borrow_mut().clear();
        iv.asked.borrow_mut().clear();
    }

    // ─── The wake ────────────────────────────────────────────────────────

    /// Sets the one delayed wake that takes the pictures afresh ([`tabs::MAP_REFRESH`]).
    fn schedule(&self) {
        self.schedule_in(tabs::MAP_REFRESH);
    }

    /// Sets the one delayed wake `delay` from now, in place of the one that was set.
    fn schedule_in(&self, delay: Duration) {
        let iv = self.ivars();
        let generation = iv.generation.get().wrapping_add(1);
        iv.generation.set(generation);
        let window = iv.window;
        arrange::after(delay, move |app| {
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

    /// The window's size changed: the view takes the root's, and the grid with it; the selected
    /// cell stays in sight. A motion's places were the old size's: it ends.
    ///
    /// The pictures taken now may be of the panes' old size — a pane lays itself out after the
    /// window, and a program's view hands back the picture it had while it takes the next — so
    /// once the size has rested for [`SETTLE`] the wake takes them all again.
    pub(crate) fn refit(&self, frame: NSRect, aspect: f64, top: f64) {
        let iv = self.ivars();
        self.halt();
        self.setFrame(frame);
        iv.aspect.set(aspect);
        iv.top.set(top);
        let layout = self.layout();
        iv.scroll
            .set(iv.scroll.get().clamp(0.0, self.max_scroll(&layout)));
        self.reveal(&layout);
        self.place_field();
        self.shoot();
        self.schedule_in(SETTLE);
        self.setNeedsDisplay(true);
    }

    /// The theme changed while the view is up.
    pub(crate) fn set_theme(&self, theme: &Theme) {
        self.ivars().look.set(colours(theme));
        self.ivars().row_look.set(selected_row(theme));
        self.ivars().edge.set(edge_of(theme));
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
        let scene = self.scene(CACurrentMediaTime());
        let state = iv.state.borrow();
        let layout = self.layout();
        let origin = self.origin_y();
        // What scrolls goes under the title row, not over the window's buttons.
        let shown = NSRect::new(
            NSPoint::new(0.0, iv.top.get()),
            NSSize::new(
                bounds.size.width,
                (bounds.size.height - iv.top.get() - PLATE_AREA).max(0.0),
            ),
        );
        NSGraphicsContext::saveGraphicsState_class();
        NSBezierPath::clipRect(shown);
        for heading in &scene.headings {
            self.paint_heading(heading);
        }
        for cell in &scene.cells {
            let rect = cell.placed.rect;
            // The name row and the line under it hang below the picture.
            let bottom = rect.origin.y + rect.size.height + rules::NAME_ROW + NOTE_HEIGHT;
            if bottom > shown.origin.y && rect.origin.y < shown.origin.y + shown.size.height {
                self.paint_shown(&state, &layout, origin, cell, &scene.dims);
            }
        }
        if let Some((rect, seen)) = scene.ring {
            self.paint_ring(rect, seen);
        }
        NSGraphicsContext::restoreGraphicsState_class();
        // The column is over the grid's left, which has the room for it.
        self.paint_column(&scene);
        self.paint_plate(&state);
    }

    /// What the view shows at `now`: the model's state as it lays it out, and — while a change
    /// is on its way — everything part way from where it was on screen ([`motion`]).
    fn scene(&self, now: f64) -> Scene {
        let iv = self.ivars();
        let state = iv.state.borrow();
        let layout = self.layout();
        let origin = self.origin_y();
        let motion = iv.motion.borrow();
        let from = motion.as_ref().map(|motion| &motion.from);
        let t = motion.as_ref().map_or(1.0, |motion| motion.at(now));
        let k = ease(t);
        let selected = state.selected_cell();
        // The state's cells, each from where it was, or coming.
        let mut live = Vec::new();
        let mut target = None;
        for index in 0..state.cells() {
            let (Some(id), Some(picture)) = (cell_id(&state, index), layout.picture(index)) else {
                continue;
            };
            let to = Placed {
                rect: on_view(picture, origin),
                alpha: 1.0,
                chosen: if selected == Some(index) { 1.0 } else { 0.0 },
            };
            if selected == Some(index) {
                target = Some(to.rect);
            }
            let placed = match from.map(|from| from.cells.get(&id)) {
                None => to,
                Some(Some(was)) => Placed {
                    rect: lerp_rect(was.placed.rect, to.rect, k),
                    alpha: lerp(was.placed.alpha, 1.0, k),
                    chosen: lerp(was.placed.chosen, to.chosen, k),
                },
                Some(None) => {
                    let seen = arriving(t);
                    Placed {
                        rect: scaled(to.rect, lerp(ARRIVE_SCALE, 1.0, seen)),
                        alpha: seen,
                        ..to
                    }
                }
            };
            live.push(Shown {
                id,
                index,
                frame: None,
                placed,
            });
        }
        // The ones that went, fading and shrinking a little where they were, under the rest.
        let mut cells = Vec::new();
        if let Some(from) = from {
            let here: HashSet<&Id> = live.iter().map(|shown| &shown.id).collect();
            for (id, was) in &from.cells {
                let Some((frame, index)) = was.ghost.as_ref().filter(|_| !here.contains(id)) else {
                    continue;
                };
                let alpha = was.placed.alpha * leaving(t);
                if alpha > 0.0 {
                    cells.push(Shown {
                        id: id.clone(),
                        index: *index,
                        frame: Some(Rc::clone(frame)),
                        placed: Placed {
                            rect: scaled(was.placed.rect, lerp(1.0, LEAVE_SCALE, k)),
                            alpha,
                            chosen: was.placed.chosen,
                        },
                    });
                }
            }
        }
        cells.extend(live);
        // The ring: gliding from the picture it was round to where the selected one is going —
        // on the same curve as the picture, so a picture that moves keeps its ring on it.
        let ring = match (from.map(|from| from.ring), target) {
            (None, Some(to)) => Some((to, 1.0)),
            (Some(Some((was, seen))), Some(to)) => {
                Some((lerp_rect(was, to, k), lerp(seen, 1.0, k)))
            }
            (Some(None), Some(to)) => Some((to, k)),
            (Some(Some((was, seen))), None) => Some((was, seen * (1.0 - k))),
            (_, None) => None,
        }
        .filter(|(_, seen)| *seen > 0.0);
        // Each pane's dimness, eased from what it was.
        let mut dims = HashMap::new();
        for shown in cells.iter().filter(|shown| shown.frame.is_none()) {
            let Some(Tile::Tab(tab)) = state.cell(shown.index) else {
                continue;
            };
            for pane in &tab.panes {
                let to = if state.dim(tab.id, pane) { 1.0 } else { 0.0 };
                let was = from
                    .and_then(|from| from.dims.get(&(tab.id, pane.id)))
                    .copied()
                    .unwrap_or(to);
                dims.insert((tab.id, pane.id), lerp(was, to, k));
            }
        }
        // The headings: one that stays moves with its tiles, one that goes fades where it was.
        let mut headings = Vec::new();
        for (at, row) in layout.rows.iter().enumerate() {
            let (text, button) = match row {
                Row::Heading { text, .. } => (text, false),
                // The group of another computer's terminals is not opened with the rest.
                Row::Section { text, other, .. } => (text, !*other),
                Row::Tiles(_) => continue,
            };
            let Some(rect) = layout.heading(at) else {
                continue;
            };
            let to = on_view(rect, origin);
            let was = from.map(|from| from.headings.iter().find(|was| was.text == *text));
            let (rect, alpha) = match was {
                None => (to, 1.0),
                Some(Some(was)) => (lerp_rect(was.rect, to, k), lerp(was.alpha, 1.0, k)),
                Some(None) => (to, arriving(t)),
            };
            headings.push(Heading {
                text: text.clone(),
                button,
                rect,
                alpha,
            });
        }
        if let Some(from) = from {
            for was in &from.headings {
                let alpha = was.alpha * leaving(t);
                if alpha > 0.0 && !headings.iter().any(|heading| heading.text == was.text) {
                    headings.push(Heading {
                        alpha,
                        ..was.clone()
                    });
                }
            }
        }
        // The column: the rows slide when a server's load comes or goes under one, the selected
        // row's fill slides to its machine, the load fades in under a newly selected server.
        let machine = state.machine().clone();
        let mut rows = Vec::new();
        let mut fill_to = None;
        for (row, rect) in self.column() {
            let chosen = if row.machine == machine { 1.0 } else { 0.0 };
            let (rect, chosen) = match from.and_then(|from| from.rows.get(&row.machine)) {
                Some(&(was, was_chosen)) => (lerp_rect(was, rect, k), lerp(was_chosen, chosen, k)),
                None => (rect, chosen),
            };
            if row.machine == machine {
                fill_to = Some(rect);
            }
            rows.push((row, rect, chosen));
        }
        let fill = match (from.and_then(|from| from.fill), fill_to) {
            (Some(was), Some(to)) => Some(lerp_rect(was, to, k)),
            (_, to) => to,
        };
        let load = match from {
            Some(from) if from.machine != machine => arriving(t),
            Some(from) => lerp(from.load, 1.0, k),
            None => 1.0,
        };
        Scene {
            cells,
            ring,
            dims,
            headings,
            rows,
            fill,
            load,
        }
    }

    // ─── Motion ──────────────────────────────────────────────────────────

    /// What is on screen now, for a change about to come to move from; `None` when nothing is to
    /// move (Reduce Motion, the view on its way out). With `leaving` the change may take cells
    /// away: they keep the state and the pictures they are drawn from while they go.
    fn capture(&self, leaving: bool) -> Option<From> {
        let iv = self.ivars();
        if iv.closing.get() || reduced(self.mtm()) {
            return None;
        }
        let scene = self.scene(CACurrentMediaTime());
        let frame = leaving.then(|| {
            Rc::new(Frame {
                state: iv.state.borrow().clone(),
                layout: self.layout(),
                shots: iv.shots.borrow().clone(),
                away_shots: iv.away_shots.borrow().clone(),
            })
        });
        let cells = scene
            .cells
            .into_iter()
            .map(|shown| {
                // One already going keeps the state it goes from.
                let ghost = shown
                    .frame
                    .or_else(|| frame.clone())
                    .map(|frame| (frame, shown.index));
                (
                    shown.id,
                    Was {
                        placed: shown.placed,
                        ghost,
                    },
                )
            })
            .collect();
        Some(From {
            cells,
            dims: scene.dims,
            ring: scene.ring,
            headings: scene.headings,
            rows: scene
                .rows
                .into_iter()
                .map(|(row, rect, chosen)| (row.machine, (rect, chosen)))
                .collect(),
            fill: scene.fill,
            machine: iv.state.borrow().machine().clone(),
            load: scene.load,
        })
    }

    /// What was on screen (`from`) moves over `secs` to what the state shows now; without it, the
    /// state is simply drawn.
    fn start(&self, from: Option<From>, secs: f64) {
        if let Some(from) = from {
            let iv = self.ivars();
            iv.motion
                .replace(Some(Motion::new(from, CACurrentMediaTime(), secs)));
            if iv.link.borrow().is_none() {
                // SAFETY: the view implements `motionTick:` with the `(CADisplayLink)` signature
                // the link calls.
                let link = unsafe { self.displayLinkWithTarget_selector(self, sel!(motionTick:)) };
                // SAFETY: added to the main run loop from the main thread, in the common modes:
                // the tracking mode of a press or a live resize would silence the default one.
                unsafe {
                    link.addToRunLoop_forMode(&NSRunLoop::mainRunLoop(), NSRunLoopCommonModes)
                };
                iv.link.replace(Some(link));
            }
        }
        self.setNeedsDisplay(true);
    }

    /// The display ticked: the motion is drawn on, or — at its end — let go with its link.
    fn ticked(&self) {
        let over = self
            .ivars()
            .motion
            .borrow()
            .as_ref()
            .is_none_or(|motion| motion.at(CACurrentMediaTime()) >= 1.0);
        if over {
            self.halt();
        }
        self.setNeedsDisplay(true);
    }

    /// Nothing moves any more: the motion goes, and the link, which holds the view while it runs.
    fn halt(&self) {
        let iv = self.ivars();
        iv.motion.replace(None);
        if let Some(link) = iv.link.take() {
            link.invalidate();
        }
    }

    // ─── Drawing the parts ───────────────────────────────────────────────

    /// A group's or a section's heading where the scene has it.
    fn paint_heading(&self, heading: &Heading) {
        let look = self.ivars().look.get();
        let font = NSFont::systemFontOfSize_weight(SMALL_SIZE, unsafe { NSFontWeightSemibold });
        let rect = heading.rect;
        let (top, height) = (
            rect.origin.y + rect.size.height * 0.35,
            rect.size.height * 0.65,
        );
        draw_text(
            &heading.text,
            rect.origin.x + 2.0,
            top,
            height,
            &attributes(&font, faded(look.dim, heading.alpha)),
        );
        // A section's heading carries the button that opens all of its terminals at once.
        if heading.button {
            draw_text_right(
                rules::OPEN_ALL,
                rect.origin.x + rect.size.width - BUTTON_PAD,
                top,
                height,
                &attributes(&font, faded(look.accent, heading.alpha)),
            );
        }
    }

    /// One cell where the scene has it: drawn as its state lays it out, then moved and scaled onto
    /// its place and seen as much as the scene says. A cell that is leaving is drawn from the
    /// state it left ([`Frame`]).
    fn paint_shown(
        &self,
        state: &Overview,
        layout: &Layout,
        origin: f64,
        shown: &Shown,
        dims: &HashMap<(u64, u64), f64>,
    ) {
        let iv = self.ivars();
        let live = (iv.shots.borrow(), iv.away_shots.borrow());
        let (state, layout, origin, shots, away_shots) = match &shown.frame {
            Some(frame) => (
                &frame.state,
                &frame.layout,
                0.0,
                &frame.shots,
                &frame.away_shots,
            ),
            None => (state, layout, origin, &*live.0, &*live.1),
        };
        let (Some(picture), Some(name_row)) =
            (layout.picture(shown.index), layout.name_row(shown.index))
        else {
            return;
        };
        let base = on_view(picture, origin);
        let placed = shown.placed;
        NSGraphicsContext::saveGraphicsState_class();
        if placed.rect != base {
            let onto = NSAffineTransform::transform();
            onto.translateXBy_yBy(placed.rect.origin.x, placed.rect.origin.y);
            onto.scaleXBy_yBy(
                placed.rect.size.width / base.size.width.max(1.0),
                placed.rect.size.height / base.size.height.max(1.0),
            );
            onto.translateXBy_yBy(-base.origin.x, -base.origin.y);
            onto.concat();
        }
        // A cell part seen is drawn whole into a layer the size of the cell, which is laid on
        // at its share: the picture and the name do not show through each other.
        let layer = (placed.alpha < 1.0)
            .then(NSGraphicsContext::currentContext)
            .flatten()
            .map(|context| context.CGContext());
        if let Some(context) = &layer {
            NSBezierPath::clipRect(NSRect::new(
                NSPoint::new(base.origin.x - RING_AIR, base.origin.y - RING_AIR),
                NSSize::new(
                    base.size.width + 2.0 * RING_AIR,
                    base.size.height + name_row.height + NOTE_HEIGHT + 2.0 * RING_AIR,
                ),
            ));
            CGContext::set_alpha(Some(context), placed.alpha);
            // SAFETY: no auxiliary dictionary; the layer is ended below, inside the same saved
            // graphics state.
            unsafe { CGContext::begin_transparency_layer(Some(context), None) };
        }
        let dim = |tab: u64, pane: &CardPane| {
            dims.get(&(tab, pane.id))
                .copied()
                .unwrap_or(if state.dim(tab, pane) { 1.0 } else { 0.0 })
        };
        match state.cell(shown.index) {
            Some(Tile::Tab(tab)) => {
                self.paint_tab(
                    state,
                    layout,
                    shown.index,
                    tab,
                    origin,
                    placed.chosen,
                    shots,
                    &dim,
                );
            }
            Some(Tile::Away { entry, other, .. }) => {
                self.paint_away(
                    state,
                    layout,
                    shown.index,
                    entry,
                    other,
                    origin,
                    placed.chosen,
                    away_shots,
                );
            }
            None => {}
        }
        if let Some(context) = &layer {
            CGContext::end_transparency_layer(Some(context));
        }
        NSGraphicsContext::restoreGraphicsState_class();
    }

    /// The ring round the selected picture at `rect`, seen as much as `seen` — one ring for the
    /// view, drawn over the cells, so it can glide between them.
    fn paint_ring(&self, rect: NSRect, seen: f64) {
        let look = self.ivars().look.get();
        let ring = NSRect::new(
            NSPoint::new(rect.origin.x - RING_AIR, rect.origin.y - RING_AIR),
            NSSize::new(
                rect.size.width + 2.0 * RING_AIR,
                rect.size.height + 2.0 * RING_AIR,
            ),
        );
        let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
            ring,
            RADIUS + RING_AIR,
            RADIUS + RING_AIR,
        );
        path.setLineWidth(RING);
        faded(look.accent, seen).color().setStroke();
        path.stroke();
    }

    /// The hairline round a picture at `rect` ([`edge_of`]): one pixel of the display, inside the
    /// picture's rounded edge.
    fn paint_edge(&self, rect: NSRect) {
        let line = 1.0
            / self
                .window()
                .map_or(2.0, |window| window.backingScaleFactor());
        let inset = line / 2.0;
        let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
            NSRect::new(
                NSPoint::new(rect.origin.x + inset, rect.origin.y + inset),
                NSSize::new(rect.size.width - line, rect.size.height - line),
            ),
            RADIUS - inset,
            RADIUS - inset,
        );
        path.setLineWidth(line);
        self.ivars().edge.get().color().setStroke();
        path.stroke();
    }

    /// What ↵ does (`verb`) in a name row, ending at `right` — the same place in every row, beside
    /// the key, whatever the name's length — seen as much as the cell is selected (`chosen`).
    /// Returns where the name's room ends: the verb's room is taken by as much, so the name gives
    /// way softly. A row too short to hold it beside the name from `left` gets none.
    fn paint_verb(&self, verb: &str, chosen: f64, left: f64, right: f64, slot: (f64, f64)) -> f64 {
        if chosen <= 0.0 {
            return right;
        }
        let ink = faded(self.ivars().look.get().accent, chosen);
        let key = attributes(
            &NSFont::systemFontOfSize_weight(SMALL_SIZE, unsafe { NSFontWeightSemibold }),
            ink,
        );
        let word = attributes(&NSFont::systemFontOfSize(SMALL_SIZE), ink);
        let verb = format!(" {verb}");
        let width = text_width(RETURN_KEY, &key) + text_width(&verb, &word);
        if right - width - GAP_AFTER_NAME < left + NAME_LEAST {
            return right;
        }
        let after = draw_text(RETURN_KEY, right - width, slot.0, slot.1, &key);
        draw_text(&verb, after, slot.0, slot.1, &word);
        right - (width + GAP_AFTER_NAME) * chosen
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

    /// The field stands in the plate left of the keys, as low as puts its text on the plate's
    /// middle line, the keys' line (its text sits `FIELD_TEXT_Y` off its own middle).
    fn place_field(&self) {
        let iv = self.ivars();
        let plate = self.plate();
        let height = 22.0;
        iv.field.setFrame(NSRect::new(
            NSPoint::new(
                plate.origin.x + PLATE_PAD,
                plate.origin.y + (PLATE_HEIGHT - height) / 2.0 - FIELD_TEXT_Y,
            ),
            NSSize::new(
                (plate.size.width - 2.0 * PLATE_PAD - self.keys_room(plate)).max(40.0),
                height,
            ),
        ));
    }

    fn paint_plate(&self, state: &Overview) {
        let iv = self.ivars();
        let look = iv.look.get();
        let plate = self.plate();
        rounded(plate, PLATE_RADIUS, look.band, Some(look.line));
        // What the row finds, while nothing is typed: drawn here, under the field where its text
        // starts, not the field's placeholder — setting that ends the editing under way, and a
        // field that lets the keyboard go takes the view down (⇥ to another machine closed it).
        let typed = iv.field.currentEditor().map_or_else(
            || iv.field.stringValue().length(),
            |editor| editor.string().length(),
        );
        if typed == 0 {
            let field = iv.field.frame();
            let quiet = attributes(&NSFont::systemFontOfSize(FIELD_SIZE), look.dim);
            let text = fit(
                &state.placeholder(),
                field.size.width - FIELD_TEXT_X,
                &quiet,
            );
            draw_text(
                &text,
                field.origin.x + FIELD_TEXT_X,
                field.origin.y + FIELD_TEXT_Y,
                field.size.height,
                &quiet,
            );
        }
        let small = attributes(&NSFont::systemFontOfSize(SMALL_SIZE), look.dim);
        let key = attributes(
            &NSFont::systemFontOfSize_weight(SMALL_SIZE, unsafe { NSFontWeightSemibold }),
            look.text,
        );
        let mut right = plate.origin.x + plate.size.width - PLATE_PAD;
        let least = plate.origin.x + plate.size.width - self.keys_room(plate);
        for (glyph, verb) in state.hints().into_iter().rev() {
            let word = format!(" {verb}");
            // Esc says Close or Clear as the search empties and fills: it keeps the wider word's
            // room, so the keys left of it stay where they are while typing.
            let room = if glyph == rules::ESC {
                [rules::CLOSE, rules::CLEAR]
                    .iter()
                    .map(|verb| text_width(&format!(" {verb}"), &small))
                    .fold(0.0, f64::max)
            } else {
                text_width(&word, &small)
            };
            let left = right - room - text_width(glyph, &key);
            if left < least {
                break;
            }
            let after = draw_text(glyph, left, plate.origin.y, PLATE_HEIGHT, &key);
            draw_text(&word, after, plate.origin.y, PLATE_HEIGHT, &small);
            right = left - 14.0;
        }
        if let Some(found) = state.found() {
            draw_text_right(&found, right, plate.origin.y, PLATE_HEIGHT, &small);
        }
    }

    /// The left column: the machines, the selected one wearing the side column's selected row
    /// (its fill glides to it), a server's load under it, the keys that move along it at the
    /// bottom.
    fn paint_column(&self, scene: &Scene) {
        let iv = self.ivars();
        let look = iv.look.get();
        let (fill, line) = iv.row_look.get();
        if let Some(rect) = scene.fill {
            rounded(rect, tabs::TAB_RADIUS, fill, Some(line));
        }
        let name = NSFont::systemFontOfSize_weight(NAME_SIZE, unsafe { NSFontWeightMedium });
        let small = attributes(&NSFont::systemFontOfSize(SMALL_SIZE), look.dim);
        let machine = iv.state.borrow().machine().clone();
        for (row, rect, chosen) in &scene.rows {
            draw_text(
                &row.title,
                rect.origin.x + ROW_TEXT,
                rect.origin.y + 6.0,
                20.0,
                &attributes(&name, mix(look.dim, look.text, *chosen)),
            );
            draw_text(
                &row.count,
                rect.origin.x + ROW_TEXT,
                rect.origin.y + 25.0,
                16.0,
                &small,
            );
            if row.machine == machine && row.account.is_some() {
                self.paint_load(rect.origin.y + rect.size.height + LOAD_PAD, scene.load);
            }
        }
        // On the search row's line, level with the plate's words.
        let plate = self.plate();
        draw_text(
            HINT,
            SIDE_INSET + ROW_TEXT,
            plate.origin.y,
            PLATE_HEIGHT,
            &attributes(&NSFont::systemFontOfSize(SMALL_SIZE - 0.5), look.dim),
        );
    }

    /// A server's load under its row: the CPU's last levels as bars, then label and value for
    /// the CPU, memory, disk and uptime, and the operating system last; seen as much as `seen`.
    fn paint_load(&self, top: f64, seen: f64) {
        let iv = self.ivars();
        let look = iv.look.get();
        let load = iv.load.borrow();
        let Some((load, levels)) = load.as_ref() else {
            return;
        };
        let small = attributes(&NSFont::systemFontOfSize(SMALL_SIZE), faded(look.dim, seen));
        let (left, right) = (SIDE_INSET + ROW_TEXT, SIDE - SIDE_INSET - ROW_TEXT);
        let mut y = top;
        if !levels.is_empty() {
            faded(look.accent, seen).color().setFill();
            for (at, level) in levels.iter().enumerate() {
                let height = (f64::from(*level) + 1.0) / 8.0 * SPARK_HEIGHT;
                let bar = NSRect::new(
                    NSPoint::new(
                        left + at as f64 * (SPARK_BAR + SPARK_GAP),
                        y + SPARK_HEIGHT - height,
                    ),
                    NSSize::new(SPARK_BAR, height),
                );
                NSBezierPath::fillRect(bar);
            }
            y += SPARK_HEIGHT + 4.0;
        }
        let text = attributes(
            &NSFont::systemFontOfSize(SMALL_SIZE),
            faded(look.text, seen),
        );
        for (label, value) in &load.rows {
            draw_text(label, left, y, LOAD_LINE, &small);
            draw_text_right(value, right, y, LOAD_LINE, &text);
            y += LOAD_LINE;
        }
        if let Some(os) = &load.os {
            let os = fit(os, right - left, &small);
            draw_text(&os, left, y, LOAD_LINE, &small);
        }
    }

    /// A terminal a server keeps: its screen as the server built it, dim but for the selected one
    /// (`chosen` is how selected it looks), and under it the name, the folder and what ↵ does.
    /// One another computer has open (`other`) also names that computer at the right and, once
    /// selected, says in a line below what moving it here does (the server card's sentence).
    #[expect(
        clippy::too_many_arguments,
        reason = "the tile's facts, where it is painted and its pictures: one call from `paint_shown`"
    )]
    fn paint_away(
        &self,
        state: &Overview,
        layout: &Layout,
        index: usize,
        entry: &server_card::Entry,
        other: bool,
        origin: f64,
        chosen: f64,
        shots: &HashMap<rules::Key, Retained<NSImage>>,
    ) {
        let look = self.ivars().look.get();
        let (Some(picture), Some(name_row)) = (layout.picture(index), layout.name_row(index))
        else {
            return;
        };
        let at = |rect: Rect| on_view(rect, origin);
        let key = (entry.account.clone(), entry.terminal.clone());
        NSGraphicsContext::saveGraphicsState_class();
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(at(picture), RADIUS, RADIUS)
            .addClip();
        look.band.color().setFill();
        NSBezierPath::fillRect(at(picture));
        if let Some(shot) = shots.get(&key) {
            draw_image(shot, at(picture));
        }
        if chosen < 1.0 {
            Tint::of(look.ground.rgb, DIM_COVER * (1.0 - chosen))
                .color()
                .setFill();
            NSBezierPath::fillRect(at(picture));
        }
        NSGraphicsContext::restoreGraphicsState_class();
        self.paint_edge(at(picture));
        let row = at(name_row);
        let slot = (row.origin.y, row.size.height);
        let left = row.origin.x + 2.0;
        let mut right = row.origin.x + row.size.width - 2.0;
        let small = attributes(&NSFont::systemFontOfSize(SMALL_SIZE), look.dim);
        // The computer that has it open, dim at the right edge (as long as a third of the row).
        let by = entry
            .by
            .as_deref()
            .filter(|_| other)
            .map(|by| fit(by, (right - left) / 3.0, &small));
        if let Some(by) = &by {
            right = draw_text_right(by, right, slot.0, slot.1, &small) - GAP_AFTER_NAME;
        }
        let verb = if other {
            rules::MOVE_HERE
        } else {
            rules::OPEN_IN_TAB
        };
        let right = self.paint_verb(verb, chosen, left, right, slot);
        let strong = attributes(
            &NSFont::systemFontOfSize_weight(NAME_SIZE, unsafe { NSFontWeightMedium }),
            mix(look.dim, look.text, chosen),
        );
        let marked = attributes(
            &NSFont::systemFontOfSize_weight(NAME_SIZE, unsafe { NSFontWeightSemibold }),
            look.accent,
        );
        let room = (right - left).max(0.0);
        let hit = state.away_hit(&key);
        let name_marks = hit.map_or(&[][..], |hit| hit.name_marks.as_slice());
        let end = draw_marked(
            &entry.title,
            name_marks,
            left,
            room,
            slot,
            (&strong, &marked),
            look.ground,
        );
        // The folder it is in, dim, beside the name: `claude  ~/api`.
        if let Some(place) = &entry.place {
            let spare = room - (end - left) - GAP_AFTER_NAME;
            if spare > 24.0 {
                let marks = hit.map_or(&[][..], |hit| hit.place_marks.as_slice());
                draw_marked(
                    place,
                    marks,
                    end + GAP_AFTER_NAME,
                    spare,
                    slot,
                    (&small, &marked),
                    look.ground,
                );
            }
        }
        // What moving it does, in the gap under the row, for the selected tile.
        if by.is_some() && chosen > 0.0 {
            let ink = attributes(
                &NSFont::systemFontOfSize(SMALL_SIZE),
                faded(look.dim, chosen),
            );
            let note = server_card::move_note(entry.by.as_deref().unwrap_or_default());
            draw_text(
                &fit(&note, row.size.width - 4.0, &ink),
                left,
                slot.0 + slot.1 - 4.0,
                NOTE_HEIGHT,
                &ink,
            );
        }
    }

    /// A tab: each pane's picture where the tab's tree puts it (dim as much as `dim` says), and
    /// the name row under it.
    #[expect(
        clippy::too_many_arguments,
        reason = "the tab, where it is painted, its pictures and its panes' dimness: one call from `paint_shown`"
    )]
    fn paint_tab(
        &self,
        state: &Overview,
        layout: &Layout,
        index: usize,
        tab: &rules::Tab,
        origin: f64,
        chosen: f64,
        shots: &HashMap<(u64, u64), Retained<NSImage>>,
        dim: &dyn Fn(u64, &CardPane) -> f64,
    ) {
        let iv = self.ivars();
        let look = iv.look.get();
        let (Some(picture), Some(name_row)) = (layout.picture(index), layout.name_row(index))
        else {
            return;
        };
        let at = |rect: Rect| on_view(rect, origin);
        // The pictures: each pane where the tab's tree puts it, a thin gap between neighbours.
        NSGraphicsContext::saveGraphicsState_class();
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(at(picture), RADIUS, RADIUS)
            .addClip();
        look.band.color().setFill();
        NSBezierPath::fillRect(at(picture));
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
            let dimness = dim(tab.id, pane);
            if dimness > 0.0 {
                Tint::of(look.ground.rgb, DIM_COVER * dimness)
                    .color()
                    .setFill();
                NSBezierPath::fillRect(rect);
            }
            // A terminal being carried would open beside this pane: it is ringed.
            if iv.drop.get() == Some((tab.id, pane.id)) {
                let inset = RING;
                let ring = NSRect::new(
                    NSPoint::new(rect.origin.x + inset, rect.origin.y + inset),
                    NSSize::new(
                        (rect.size.width - 2.0 * inset).max(1.0),
                        (rect.size.height - 2.0 * inset).max(1.0),
                    ),
                );
                let path = NSBezierPath::bezierPathWithRect(ring);
                path.setLineWidth(RING);
                look.accent.color().setStroke();
                path.stroke();
            }
        }
        NSGraphicsContext::restoreGraphicsState_class();
        self.paint_edge(at(picture));
        // The name row: the tab's icon and name from the left, its key at the right and, while it
        // is the selected one, what ↵ does just left of the key — a place of its own, not after
        // the name, so it stands still whatever the name's length. A name longer than its room
        // fades out at its end, as the strip's titles do.
        let row = at(name_row);
        let slot = (row.origin.y, row.size.height);
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
            right = draw_text_right(key, right, slot.0, slot.1, &small) - GAP_AFTER_NAME;
        }
        let right = self.paint_verb(rules::GO_TO_TAB, chosen, left, right, slot);
        let strong = attributes(
            &NSFont::systemFontOfSize_weight(NAME_SIZE, unsafe { NSFontWeightMedium }),
            mix(look.dim, look.text, chosen),
        );
        let marked = attributes(
            &NSFont::systemFontOfSize_weight(NAME_SIZE, unsafe { NSFontWeightSemibold }),
            look.accent,
        );
        let room = (right - left).max(0.0);
        let hit = state.hit(tab.id);
        let name_marks = hit.map_or(&[][..], |hit| hit.name_marks.as_slice());
        let end = draw_marked(
            &tab.name,
            name_marks,
            left,
            room,
            slot,
            (&strong, &marked),
            look.ground,
        );
        // The program the search found in a pane, beside the name: `api  cargo test`.
        if let Some((program, marks)) = hit.and_then(|hit| hit.program.as_ref()) {
            let spare = room - (end - left) - GAP_AFTER_NAME;
            if spare > 24.0 {
                draw_marked(
                    program,
                    marks,
                    end + GAP_AFTER_NAME,
                    spare,
                    slot,
                    (&small, &marked),
                    look.ground,
                );
            }
        }
    }

    // ─── Opening and closing ─────────────────────────────────────────────

    /// The view settles from a little larger about its middle and fades in, once (Core
    /// Animation; none with Reduce Motion).
    fn appear(&self) {
        if reduced(self.mtm()) {
            return;
        }
        self.setWantsLayer(true);
        let Some(layer) = self.layer() else {
            return;
        };
        if let Some(look) = grown(&layer, self.frame()) {
            play_look(&layer, look, OPEN_SECS, Ease::Out);
        }
        play(&layer, "opacity", 0.0, 1.0, OPEN_SECS, Ease::Out, 0.0);
    }

    /// The view leaves the way it came — growing a little about its middle and fading — and is
    /// taken off the window at the end (at once with Reduce Motion): the pictures stay until
    /// then.
    fn leave(&self) {
        self.halt();
        let look = (!reduced(self.mtm()))
            .then(|| self.layer())
            .flatten()
            .and_then(|layer| Some((grown(&layer, self.frame())?, layer)));
        let Some((look, layer)) = look else {
            self.release();
            self.removeFromSuperview();
            return;
        };
        // SAFETY: plain `NSValue`s around `CATransform3D` values.
        let (rest, look) = unsafe {
            (
                NSValue::valueWithCATransform3D(IDENTITY),
                NSValue::valueWithCATransform3D(look),
            )
        };
        hold_at(&layer, "transform", &rest, &look);
        // The fade is AppKit's, whose group says when it is over: the view goes then.
        let fading = self.retain();
        let changes = RcBlock::new(move |context: NonNull<NSAnimationContext>| {
            // SAFETY: AppKit gives the block a live context, for the block's duration.
            let context = unsafe { context.as_ref() };
            context.setDuration(OPEN_SECS);
            context.setTimingFunction(Some(&ease_in()));
            fading.animator().setAlphaValue(0.0);
        });
        let leaving = self.retain();
        let done = RcBlock::new(move || {
            leaving.release();
            leaving.removeFromSuperview();
        });
        NSAnimationContext::runAnimationGroup_completionHandler(&changes, Some(&done));
    }
}

/// Whether the system asks for less motion (no application object: as if it did).
fn reduced(mtm: MainThreadMarker) -> bool {
    app::delegate(mtm).is_none_or(|app| app.reduce_motion())
}

/// How `layer`, standing at `frame`, looks [`OPEN_SCALE`] times larger about its middle — the
/// matrix [`card::fitted`] checked against the layer. A view's layer scales about its corner:
/// scaled there, the view slid in from the side.
fn grown(layer: &CALayer, frame: NSRect) -> Option<CATransform3D> {
    let at = Rect::new(
        frame.origin.x,
        frame.origin.y,
        frame.size.width,
        frame.size.height,
    );
    let (width, height) = (at.width * OPEN_SCALE, at.height * OPEN_SCALE);
    let look = Rect::new(
        at.x - (width - at.width) / 2.0,
        at.y - (height - at.height) / 2.0,
        width,
        height,
    );
    card::fitted(layer, look, at)
}

/// The closing's curve: slow, then quick.
fn ease_in() -> Retained<CAMediaTimingFunction> {
    // SAFETY: a constant name QuartzCore exposes, it lives for the whole process.
    CAMediaTimingFunction::functionWithName(unsafe { kCAMediaTimingFunctionEaseIn })
}

/// Plays `path` of `layer` from `from` to `to` over the opening's time, on the closing's curve,
/// and keeps `to` on screen once it is over: the view is taken away then.
fn hold_at(layer: &CALayer, path: &str, from: &AnyObject, to: &AnyObject) {
    let animation = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str(path)));
    // SAFETY: the values are the objects the key path takes (an `NSValue` of a
    // `CATransform3D`, an `NSNumber`).
    unsafe {
        animation.setFromValue(Some(from));
        animation.setToValue(Some(to));
        animation.setFillMode(kCAFillModeForwards);
    }
    animation.setDuration(OPEN_SECS);
    animation.setTimingFunction(Some(&ease_in()));
    animation.setRemovedOnCompletion(false);
    layer.addAnimation_forKey(&animation, Some(&NSString::from_str(path)));
}

/// `image` in `rect`, the right way up in a flipped drawing too, filling it the way a pane's
/// picture fills its tile ([`bt_gpu::tile_crop`]: what spills over is cut, the left edge and the
/// bottom kept). A picture is taken in its tile's shape; one taken before the window changed shape
/// is cut to the new one rather than squeezed into it until the next comes.
fn draw_image(image: &NSImage, rect: NSRect) {
    let size = image.size();
    // The rule wants pixels; only the shapes count, so points in fine steps do.
    let steps = |points: f64| (points * 8.0).round().clamp(1.0, f64::from(u32::MAX)) as u32;
    let [u0, v0, u1, v1] = bt_gpu::tile_crop(
        (steps(size.width), steps(size.height)),
        (steps(rect.size.width), steps(rect.size.height)),
    );
    // The image's own space is up from its bottom: the crop's top is left out, its bottom kept.
    let from = NSRect::new(
        NSPoint::new(
            f64::from(u0) * size.width,
            (1.0 - f64::from(v1)) * size.height,
        ),
        NSSize::new(
            f64::from(u1 - u0) * size.width,
            f64::from(v1 - v0) * size.height,
        ),
    );
    // SAFETY: no hints.
    unsafe {
        image.drawInRect_fromRect_operation_fraction_respectFlipped_hints(
            rect,
            from,
            NSCompositingOperation::SourceOver,
            1.0,
            true,
            None,
        );
    }
}

/// `text` with the letters at `marks` in the second of `inks` (the rest in the first), from `x`
/// in a room `room` wide, centred in the slot `(top, height)`; returns where it ended. A text
/// longer than its room fades out at the room's end the way the strip's titles do
/// ([`tabs::title_overflows`], [`tabs::title_fade`]) — its end covered with the `ground` it
/// stands on — and never ends in "…".
fn draw_marked(
    text: &str,
    marks: &[usize],
    x: f64,
    room: f64,
    slot: (f64, f64),
    inks: (&Attributes, &Attributes),
    ground: Tint,
) -> f64 {
    if room < 1.0 {
        return x;
    }
    let chars: Vec<char> = text.chars().collect();
    let string = styled(&chars, marks, chars.len(), inks.0, inks.1);
    let size = string.size();
    let at = NSPoint::new(x, slot.0 + (slot.1 - size.height) / 2.0);
    if !tabs::title_overflows(size.width, room) {
        string.drawAtPoint(at);
        return x + size.width;
    }
    let place = NSRect::new(NSPoint::new(x, slot.0), NSSize::new(room, slot.1));
    NSGraphicsContext::saveGraphicsState_class();
    NSBezierPath::clipRect(place);
    string.drawAtPoint(at);
    NSGraphicsContext::restoreGraphicsState_class();
    fade_end(place, ground);
    x + room
}

/// Covers the end of `place` with `ground`, from nothing to all of it over the strip's fade
/// ([`tabs::title_faded`]'s curve, in even steps a gradient joins).
fn fade_end(place: NSRect, ground: Tint) {
    const STOPS: usize = 9;
    let fade = tabs::title_fade(place.size.width);
    let colours: Vec<Retained<NSColor>> = (0..STOPS)
        .map(|stop| {
            let at = fade * stop as f64 / (STOPS - 1) as f64;
            faded(ground, tabs::title_faded(at, fade)).color()
        })
        .collect();
    let gradient =
        NSGradient::initWithColors(NSGradient::alloc(), &NSArray::from_retained_slice(&colours));
    if let Some(gradient) = gradient {
        gradient.drawInRect_angle(
            NSRect::new(
                NSPoint::new(place.origin.x + place.size.width - fade, place.origin.y),
                NSSize::new(fade, place.size.height),
            ),
            0.0,
        );
    }
}

/// `tint` seen as much as `seen`.
fn faded(tint: Tint, seen: f64) -> Tint {
    Tint::of(tint.rgb, tint.alpha * seen.clamp(0.0, 1.0))
}

/// The ink part way from `from` to `to`.
fn mix(from: Tint, to: Tint, k: f64) -> Tint {
    let k = k.clamp(0.0, 1.0);
    let channel = |shift: u32| {
        let (a, b) = (
            f64::from((from.rgb >> shift) & 0xff),
            f64::from((to.rgb >> shift) & 0xff),
        );
        (lerp(a, b, k).round() as u32) << shift
    };
    Tint::of(
        channel(16) | channel(8) | channel(0),
        lerp(from.alpha, to.alpha, k),
    )
}

/// A rectangle of the grid (its origin the grid's top left before any scroll) in the view, the
/// grid's origin `origin` down the view.
fn on_view(rect: Rect, origin: f64) -> NSRect {
    NSRect::new(
        NSPoint::new(rect.x + SIDE, rect.y + origin),
        NSSize::new(rect.width, rect.height),
    )
}

/// Cell `index`'s identity across states: a tab's id, a terminal's key.
fn cell_id(state: &Overview, index: usize) -> Option<Id> {
    match state.cell(index)? {
        Tile::Tab(tab) => Some(Id::Tab(tab.id)),
        Tile::Away { entry, .. } => Some(Id::Away((entry.account.clone(), entry.terminal.clone()))),
    }
}

/// What the view shows at a moment ([`OverviewView::scene`]).
struct Scene {
    /// The cells to draw, the ones on their way out first, under the rest.
    cells: Vec<Shown>,
    /// The selected picture the ring goes round, and how seen the ring is.
    ring: Option<(NSRect, f64)>,
    /// Each pane's dimness, `(tab, pane)`: 0 lit, 1 dim.
    dims: HashMap<(u64, u64), f64>,
    headings: Vec<Heading>,
    /// The column's rows, where they stand and how selected they look; the selected row's fill;
    /// how seen the selected server's load is.
    rows: Vec<(rules::MachineRow, NSRect, f64)>,
    fill: Option<NSRect>,
    load: f64,
}

/// A cell in a scene: a cell of the state by its number (`frame` none), or — on its way out — of
/// the frame it left.
struct Shown {
    id: Id,
    index: usize,
    frame: Option<Rc<Frame>>,
    placed: Placed,
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

/// The servers' terminals in the background as the big view lists them: the server card's own
/// rows ([`server_card::entry_of`]), all of them, grouped by server.
fn background_sections() -> Vec<rules::Section> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let mut sections: Vec<rules::Section> = Vec::new();
    for background in crate::hold_service::background_all() {
        let entry = server_card::entry_of(&background, now, &crate::server_card::local);
        match sections
            .iter_mut()
            .find(|section| section.account == background.account)
        {
            Some(section) => section.entries.push(entry),
            None => sections.push(rules::Section {
                account: background.account.clone(),
                host: background.host.clone(),
                entries: vec![entry],
                elsewhere: Vec::new(),
            }),
        }
    }
    // The terminals other computers have open: the group under the server's own, which a server
    // with nothing of its own in the background has too.
    for background in crate::hold_service::elsewhere_all() {
        let entry = server_card::entry_of(&background, now, &crate::server_card::local);
        match sections
            .iter_mut()
            .find(|section| section.account == background.account)
        {
            Some(section) => section.elsewhere.push(entry),
            None => sections.push(rules::Section {
                account: background.account.clone(),
                host: background.host.clone(),
                entries: Vec::new(),
                elsewhere: vec![entry],
            }),
        }
    }
    sections
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
    // The strip goes under the view: its card, its key hints and a group's list go now.
    window.bar().resigned();
    let theme = window.bar().theme();
    let (tabs, _) = window.overview_facts();
    let here = window.selected_tab_id();
    let mut state = Overview::new(tabs, here);
    state.set_background(background_sections());
    // The servers are asked what they keep now; the view reads what comes on its next wake.
    crate::hold_service::refresh_background();
    let view = OverviewView::new(mtm, window.id(), state, &theme);
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

/// The view over the window with the machine `host` selected in its list (the server's name as
/// the user typed it): opened if it was not, and the machine changed if it was. A server the
/// list does not have leaves All tabs.
pub(crate) fn show_on(window: &TerminalWindow, host: &str) {
    open(window);
    let Some(view) = window.root().overview() else {
        return;
    };
    let machine = {
        let state = view.ivars().state.borrow();
        state
            .machines()
            .into_iter()
            .map(|row| row.machine)
            .find(|machine| {
                matches!(machine, Machine::Host(name)
                    if bt_core::bare_host(name).eq_ignore_ascii_case(bt_core::bare_host(host)))
            })
    };
    // The view opens on the machine: nothing to move from.
    if let Some(machine) = machine
        && view.ivars().state.borrow_mut().set_machine(machine)
    {
        view.machine_changed(None);
    }
}

/// Takes the view down: the window has none from here on and the wake finds nothing to do; the
/// view leaves ([`OverviewView::leave`]) and the panes let go of the pictures they kept for it
/// once it is gone.
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
    view.leave();
}
