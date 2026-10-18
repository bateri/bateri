//! The group button of the title row and the list of the window's groups under it.
//!
//! **The button** leads the bar of a window that has groups: a stack and, for a group, its name;
//! on the ungrouped tabs the stack alone. It is a button, not a tab — filled, a few points from the
//! first chip — and the tabs after it are the ones of the group it names (`TerminalWindow::refresh_bar`
//! lists no other). A dot on its corner says that some other group (or the ungrouped tabs) has
//! something that asks for the user: a question, a failure, an unseen finish
//! (`tabs::group_indicator`), whichever group it is — the list says which. A press opens the list
//! ([`TabBar::toggle_group_list`]); a double click names the group (the bar's name field over the
//! name, [`TabBar::begin_group_rename`]); a right click is the group's menu; pulled out of the bar
//! it carries the group away in a drag session ([`TabBar::carry_group`]) — to a window of its
//! own, to another window — which a press that wanders a few points is not. The button's width
//! follows its name ([`tabs::group_button_width`]) and grows while the name is typed, so the
//! tabs move aside rather than being covered.
//!
//! **The list** is a card under the button, the summary card's own face: the ungrouped tabs'
//! row first, a rule, then each group by name with what asks for the user in it and the key that
//! reaches it (⌃⌘0…⌃⌘9), the group on screen checked; and New Group last. A press on a row brings
//! the group on screen ([`TerminalWindow::reach_cluster`]); a press on a group's row that moves
//! [`tabs::DRAG_SLOP`] before the button comes up is a pull — the groups are put in order by
//! carrying a row to the line between two of them, which is one move ([`Move::MoveGroup`]),
//! the ungrouped tabs' row staying first; a press anywhere else closes the list
//! and is taken by it — except on the button itself, which is let through, so a second press
//! closes it and a double click still names the group. Esc closes it, ↑ ↓ and Return choose. It
//! stands on a transparent view over the whole window ([`ListScrim`]), which is what catches the
//! presses, and the card has the keyboard while it is up.
//!
//! Nothing here changes a group or the window's list: the window's appliers do.

use std::cell::{Cell, RefCell};

use objc2::rc::Retained;
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send,
};
use objc2_app_kit::{
    NSAccessibility, NSAccessibilityButtonRole, NSAnimatablePropertyContainer,
    NSAutoresizingMaskOptions, NSBezierPath, NSBox, NSEvent, NSFont, NSFontWeightMedium,
    NSFontWeightSemibold, NSMenu, NSShadow, NSTextAlignment, NSTextField, NSTrackingArea,
    NSTrackingAreaOptions, NSView, NSWindowOrderingMode,
};
use objc2_foundation::{NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};

use super::tab_title::Title;
use super::{
    CARD_RADIUS, Palette, SwitchRow, TabBar, Tint, animate, contains, fade, label, plain_box,
    rounded, still, stroke, track_hover,
};
use crate::app;
use crate::groups::{GroupId, group_place};
use crate::moves::Move;
use crate::tabs::{self, DRAG_SLOP, Indicator, TAB_RADIUS};

/// The name's type size, in the button and in the list's rows; the keys' and the rows' hint.
pub(super) const NAME_TEXT: f64 = 12.5;
const KEY_TEXT: f64 = 11.0;
/// Where the stack stands in the button (left edge), by what it names, and its side: the stack is
/// a 12-point picture drawn at [`STACK`] points.
const STACK_X_NAMED: f64 = 10.0;
const STACK_X_LOOSE: f64 = 9.0;
const STACK: f64 = 14.0;
/// Where the name starts in the button, and what it keeps from the right end.
pub(super) const NAME_X: f64 = 30.0;
pub(super) const NAME_END: f64 = 14.0;
/// The dot on the button's corner: its radius and the halo round it.
const DOT_RADIUS: f64 = 4.0;
const DOT_HALO: f64 = 1.5;
/// A row of the list: the left of its name (the check stands before it), what the right end
/// holds (the indicator's square, the key's width, the space between and the margin).
const ROW_NAME_X: f64 = 28.0;
const ROW_RIGHT: f64 = 70.0;
const INDICATOR: f64 = 12.0;
const KEY_WIDTH: f64 = 34.0;
const ROW_END: f64 = 8.0;
const ROW_RADIUS: f64 = 6.0;
/// How long the highlight takes to go from one row to the next, and to come and go: short, so
/// the keys and the pointer never wait for it.
const LIT_SLIDE: f64 = 0.14;
const LIT_FADE: f64 = 0.08;
/// What stands between the button's bottom and the list.
const LIST_DROP: f64 = 6.0;
/// The list's margin from the window's edge.
const LIST_MARGIN: f64 = 8.0;
/// The key code of Esc, Return, Enter, ↓ and ↑.
const ESCAPE: u16 = 53;
const RETURN: u16 = 36;
const ENTER: u16 = 76;
const DOWN: u16 = 125;
const UP: u16 = 126;

// ─── The button ──────────────────────────────────────────────────────────

/// What the bar tells the button besides where it stands ([`GroupButton::set`]).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ButtonLook {
    /// The group on screen; `None` on the ungrouped tabs.
    pub(super) group: Option<GroupId>,
    /// Its name, which the button shows; `None` on the ungrouped tabs.
    pub(super) name: Option<String>,
    /// The list is open under it.
    pub(super) open: bool,
    /// The name is being typed over the name.
    pub(super) naming: bool,
    /// What some other group says of itself.
    pub(super) elsewhere: Option<Indicator>,
}

pub(super) struct ButtonIvars {
    palette: Cell<Option<Palette>>,
    look: RefCell<Option<ButtonLook>>,
    /// The pointer is over it.
    hot: Cell<bool>,
    pressed: Cell<bool>,
    name: Title,
    /// A label outside every view that texts are measured on, and the last measure: the bar asks
    /// for the button's width at every layout, and what is shown is not touched by it.
    gauge: Retained<NSTextField>,
    measured: RefCell<Option<(String, f64)>>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; GroupButton implements no `Drop` and is born
    // with `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriGroupButton"]
    #[ivars = ButtonIvars]
    pub(super) struct GroupButton;

    unsafe impl NSObjectProtocol for GroupButton {}

    impl GroupButton {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            self.draw();
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        /// The button is not the window's to drag.
        #[unsafe(method(mouseDownCanMoveWindow))]
        fn can_move_window(&self) -> bool {
            false
        }

        /// A press opens or closes the list (on release); the second press of a double click
        /// names the group instead.
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let iv = self.ivars();
            if event.clickCount() == 2 {
                iv.pressed.set(false);
                self.rename();
                return;
            }
            iv.pressed.set(true);
        }

        /// The pointer moves with the button down: out of the bar by [`tabs::TEAR_DISTANCE`] the
        /// group is carried away, as a tab is. Within the bar nothing is a pull — the button stands
        /// where it does, and a press that wanders is still a click.
        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            let iv = self.ivars();
            if !iv.pressed.get() {
                return;
            }
            let group = iv.look.borrow().as_ref().and_then(|look| look.group);
            let Some((bar, group)) = self.bar().zip(group) else {
                return;
            };
            let at = bar.convertPoint_fromView(event.locationInWindow(), None);
            let size = bar.bounds().size;
            if tabs::distance_outside(at.x, at.y, size.width, size.height) <= tabs::TEAR_DISTANCE {
                return;
            }
            iv.pressed.set(false);
            bar.carry_group(group, self, event, false);
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            let pressed = self.ivars().pressed.replace(false);
            let at = self.convertPoint_fromView(event.locationInWindow(), None);
            if pressed
                && contains(self.bounds(), at)
                && let Some(bar) = self.bar()
            {
                bar.toggle_group_list();
            }
        }

        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            let iv = self.ivars();
            iv.hot.set(true);
            self.setNeedsDisplay(true);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            let iv = self.ivars();
            iv.hot.set(false);
            self.setNeedsDisplay(true);
        }

        /// The group's own menu, on a right click or a control click; the ungrouped tabs have
        /// none.
        #[unsafe(method_id(menuForEvent:))]
        fn menu_for_event(&self, _event: &NSEvent) -> Option<Retained<NSMenu>> {
            // No `?`: `define_class!` converts the body's last expression.
            let group = self.ivars().look.borrow().as_ref().and_then(|look| look.group);
            self.bar().zip(group).and_then(|(bar, group)| bar.group_menu(group))
        }

        #[unsafe(method(accessibilityPerformPress))]
        fn accessibility_press(&self) -> bool {
            if let Some(bar) = self.bar() {
                bar.toggle_group_list();
            }
            true
        }
    }
);

impl GroupButton {
    pub(super) fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ButtonIvars {
            palette: Cell::new(None),
            look: RefCell::new(None),
            hot: Cell::new(false),
            pressed: Cell::new(false),
            name: Title::new(mtm, NSTextAlignment::Left),
            gauge: label(mtm),
            measured: RefCell::new(None),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.ivars().name.field().setFont(Some(&name_font()));
        this.ivars().gauge.setFont(Some(&name_font()));
        this.addSubview(this.ivars().name.view());
        // A longer name is set at once and the button widens under it: it shows as the button
        // comes to it, not past the button's edge.
        this.setClipsToBounds(true);
        this.setAccessibilityElement(true);
        // SAFETY: AppKit's constant role string, alive for the process.
        this.setAccessibilityRole(Some(unsafe { NSAccessibilityButtonRole }));
        track_hover(&this);
        this
    }

    /// The bar the button is in.
    fn bar(&self) -> Option<Retained<TabBar>> {
        // SAFETY: reading the superview; we are on the main thread.
        unsafe { self.superview() }?.downcast::<TabBar>().ok()
    }

    /// A double click: the group's name opens under the field.
    fn rename(&self) {
        let group = self
            .ivars()
            .look
            .borrow()
            .as_ref()
            .and_then(|look| look.group);
        if let (Some(bar), Some(group)) = (self.bar(), group) {
            bar.close_group_list(false);
            bar.begin_group_rename(group, false);
        }
    }

    /// The width `text` has in the button's type, in points: what the bar sizes the button by
    /// ([`tabs::group_button_width`]).
    pub(super) fn text_width(&self, text: &str) -> f64 {
        let iv = self.ivars();
        if let Some((was, width)) = iv.measured.borrow().as_ref()
            && was == text
        {
            return *width;
        }
        iv.gauge.setStringValue(&NSString::from_str(text));
        let width = iv.gauge.intrinsicContentSize().width.ceil();
        iv.measured.replace(Some((text.to_owned(), width)));
        width
    }

    /// Places the button at `frame` (the bar's space) and gives it its look.
    /// Over `secs` when its width changes (another group's name), at once for zero.
    pub(super) fn set(&self, frame: NSRect, secs: f64, look: &ButtonLook, palette: Palette) {
        let iv = self.ivars();
        if self.frame() != frame {
            if secs > 0.0 {
                let this = self.retain();
                animate(secs, move || this.animator().setFrame(frame));
            } else {
                self.setFrame(frame);
            }
        }
        let changed = iv.look.borrow().as_ref() != Some(look)
            || iv.palette.replace(Some(palette)) != Some(palette);
        iv.palette.set(Some(palette));
        if !changed {
            return;
        }
        iv.look.replace(Some(look.clone()));
        let name = look.name.as_deref().unwrap_or("");
        let field = iv.name.field();
        field.setStringValue(&NSString::from_str(name));
        field.setTextColor(Some(&Tint::of(palette.title, 1.0).color()));
        iv.name.view().setHidden(look.name.is_none() || look.naming);
        // The label's frame is the drawing one, the fitting size, 4 points wider than the type's
        // own width the button is sized by.
        let natural = field.fittingSize();
        let (x, width) = name_span(frame.size.width);
        iv.name.place(
            NSRect::new(
                NSPoint::new(x, ((frame.size.height - natural.height) / 2.0).max(0.0)),
                NSSize::new(width, natural.height),
            ),
            natural.width,
        );
        let words = match &look.name {
            Some(name) => format!("Tab group {name}, show the window\u{2019}s groups"),
            None => "Ungrouped tabs, show the window\u{2019}s groups".to_owned(),
        };
        self.setAccessibilityLabel(Some(&NSString::from_str(&words)));
        self.setToolTip(Some(&NSString::from_str(
            look.name.as_deref().unwrap_or("Ungrouped"),
        )));
        self.setNeedsDisplay(true);
    }

    /// The list is open (or no longer) under the button: its fill says so.
    pub(super) fn set_open(&self, open: bool) {
        let mut look = self.ivars().look.borrow_mut();
        if let Some(look) = look.as_mut()
            && look.open != open
        {
            look.open = open;
            self.setNeedsDisplay(true);
        }
    }

    fn draw(&self) {
        let iv = self.ivars();
        let (Some(palette), Some(look)) = (iv.palette.get(), iv.look.borrow().clone()) else {
            return;
        };
        let lit = look.open || iv.hot.get();
        let fill = if lit {
            palette.button_hover
        } else {
            palette.button
        };
        let bounds = self.bounds();
        rounded(bounds, TAB_RADIUS, fill, Some(palette.button_line));
        let named = look.name.is_some();
        let x = if named { STACK_X_NAMED } else { STACK_X_LOOSE };
        let at = NSPoint::new(x, (bounds.size.height - STACK) / 2.0);
        draw_stack(
            at,
            Tint::of(if named { palette.dim } else { palette.title }, 1.0),
        );
        if let Some(indicator) = look.elsewhere {
            let centre = NSPoint::new(
                bounds.size.width - DOT_RADIUS - DOT_HALO - 0.5,
                DOT_RADIUS + DOT_HALO + 0.5,
            );
            for (radius, color) in [
                (DOT_RADIUS + DOT_HALO, Tint::of(palette.ground, 1.0)),
                (
                    DOT_RADIUS,
                    Tint::of(palette.tone(tabs::list_dot(indicator), 0), 1.0),
                ),
            ] {
                let path = NSBezierPath::bezierPathWithOvalInRect(NSRect::new(
                    NSPoint::new(centre.x - radius, centre.y - radius),
                    NSSize::new(2.0 * radius, 2.0 * radius),
                ));
                color.color().setFill();
                path.fill();
            }
        }
    }
}

/// Where the name stands in a button `width` wide: `(x, width)` from its left edge. The one
/// place for the label and for the rename field that opens over it ([`TabBar::place_switch`]):
/// two points left of the name's own x, which is the label's inset, and as far right as the field
/// goes.
pub(super) fn name_span(width: f64) -> (f64, f64) {
    let x = NAME_X - 2.0;
    (x, (width - NAME_END / 2.0 - x).max(0.0))
}

fn name_font() -> Retained<NSFont> {
    // SAFETY: AppKit's constant font weight.
    NSFont::systemFontOfSize_weight(NAME_TEXT, unsafe { NSFontWeightSemibold })
}

/// The stack — a folder's back with two tabs standing over it — at `at` (the top left of its
/// [`STACK`]-point square), the design's 12-point picture scaled up.
fn draw_stack(at: NSPoint, ink: Tint) {
    let scale = STACK / 12.0;
    let point = |x: f64, y: f64| NSPoint::new(at.x + x * scale, at.y + y * scale);
    let body = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
        NSRect::new(point(1.5, 4.6), NSSize::new(9.0 * scale, 6.2 * scale)),
        1.4 * scale,
        1.4 * scale,
    );
    stroke(&body, 1.2 * scale, ink);
    let tabs = NSBezierPath::bezierPath();
    tabs.moveToPoint(point(2.9, 3.05));
    tabs.lineToPoint(point(9.1, 3.05));
    tabs.moveToPoint(point(4.3, 1.5));
    tabs.lineToPoint(point(7.7, 1.5));
    stroke(&tabs, 1.2 * scale, ink);
}

// ─── What catches the presses ────────────────────────────────────────────

pub(super) struct ScrimIvars {
    /// The window whose bar the list belongs to: where a press that closes it goes.
    window: Cell<u64>,
    /// The button's rectangle in the window's space: a press there is let through.
    hole: Cell<NSRect>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; ListScrim implements no `Drop` and is born with
    // `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriGroupListScrim"]
    #[ivars = ScrimIvars]
    pub(super) struct ListScrim;

    unsafe impl NSObjectProtocol for ListScrim {}

    impl ListScrim {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        /// Every press on the window is the scrim's but the button's: `point` is in the window's
        /// space, which is the scrim's own.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
            if contains(self.ivars().hole.get(), point) {
                None
            } else {
                // SAFETY: `NSView`'s `hitTest:` takes a point in the superview's space.
                unsafe { msg_send![super(self), hitTest: point] }
            }
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(mouseDownCanMoveWindow))]
        fn can_move_window(&self) -> bool {
            false
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            self.dismiss();
        }

        #[unsafe(method(rightMouseDown:))]
        fn right_mouse_down(&self, _event: &NSEvent) {
            self.dismiss();
        }

        #[unsafe(method(otherMouseDown:))]
        fn other_mouse_down(&self, _event: &NSEvent) {
            self.dismiss();
        }
    }
);

impl ListScrim {
    pub(super) fn new(mtm: MainThreadMarker, window: u64) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ScrimIvars {
            window: Cell::new(window),
            hole: Cell::new(NSRect::ZERO),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.setAccessibilityElement(false);
        this
    }

    pub(super) fn set_hole(&self, hole: NSRect) {
        self.ivars().hole.set(hole);
    }

    /// A press away from the list: it closes, and the keyboard goes back to the tab.
    fn dismiss(&self) {
        if let Some(window) =
            app::delegate(self.mtm()).and_then(|app| app.window(self.ivars().window.get()))
        {
            window.bar().close_group_list(true);
        }
    }
}

// ─── The list ────────────────────────────────────────────────────────────

/// What the pointer or the keys are on in the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hot {
    Row(usize),
    New,
}

/// A group's row carried along the list: which row, and the slot among the groups the pointer
/// would let it go at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Carry {
    row: usize,
    slot: usize,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; ListMarks has no ivars, implements no `Drop`
    // and is born with `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriGroupListMarks"]
    struct ListMarks;

    unsafe impl NSObjectProtocol for ListMarks {}

    impl ListMarks {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        /// The list's marks, drawn over the highlight that moves under them: the check, the
        /// indicators, New Group's plus, the rules and a carried row's line.
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            // SAFETY: reading the superview; we are on the main thread.
            if let Some(list) = unsafe { self.superview() }
                && let Some(list) = list.downcast_ref::<GroupList>()
            {
                list.draw_marks();
            }
        }
    }
);

impl ListMarks {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: `initWithFrame:` is NSView's designated initializer and there are no ivars.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.setAccessibilityElement(false);
        this
    }
}

pub(super) struct ListIvars {
    window: Cell<u64>,
    palette: Cell<Option<Palette>>,
    rows: RefCell<Vec<SwitchRow>>,
    /// The group on screen: its row is checked. `None`: the ungrouped tabs'.
    shown: Cell<Option<GroupId>>,
    layout: RefCell<Option<tabs::GroupList>>,
    hot: Cell<Option<Hot>>,
    /// The highlight under the row the pointer or the keys are on: a box of its own that slides
    /// from row to row, under the marks and the names.
    lit: Retained<NSBox>,
    marks: Retained<ListMarks>,
    pressed: Cell<Option<Hot>>,
    /// Where the press that is `pressed` went down, in the window's space, and the row it
    /// carries once it has become a pull.
    press_at: Cell<NSPoint>,
    carrying: Cell<Option<Carry>>,
    /// A name and a key per row, and New Group's last.
    names: RefCell<Vec<Retained<NSTextField>>>,
    keys: RefCell<Vec<Retained<NSTextField>>>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; GroupList implements no `Drop` and is born with
    // `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriGroupList"]
    #[ivars = ListIvars]
    pub(super) struct GroupList;

    unsafe impl NSObjectProtocol for GroupList {}

    impl GroupList {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        /// The card takes every press inside it; its labels never do.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
            // No `return`: `define_class!` converts the body's last expression.
            self.hit(point)
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(mouseDownCanMoveWindow))]
        fn can_move_window(&self) -> bool {
            false
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        /// The keyboard went to something else (a sheet, another responder): the list closes.
        #[unsafe(method(resignFirstResponder))]
        fn resign_first_responder(&self) -> bool {
            if let Some(window) = self.owner() {
                window.bar().close_group_list(false);
            }
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            self.draw();
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let at = self.convertPoint_fromView(event.locationInWindow(), None);
            let iv = self.ivars();
            iv.pressed.set(self.hot_at(at));
            self.light(self.hot_at(at));
            iv.press_at.set(event.locationInWindow());
            iv.carrying.set(None);
            self.redraw();
        }

        /// The pointer moves with the button down on a group's row: past [`DRAG_SLOP`] the row is
        /// carried, and the line between two groups says where it would be let go. The ungrouped
        /// tabs' row, New Group and the margin are no pulls.
        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            let iv = self.ivars();
            let Some(Hot::Row(row)) = iv.pressed.get() else {
                return;
            };
            let carrying = iv.carrying.get();
            let (now, from) = (event.locationInWindow(), iv.press_at.get());
            if carrying.is_none() && (now.x - from.x).hypot(now.y - from.y) < DRAG_SLOP {
                return;
            }
            let group = iv.rows.borrow().get(row).and_then(|row| row.group);
            let at = self.convertPoint_fromView(now, None);
            let slot = iv
                .layout
                .borrow()
                .as_ref()
                .map(|layout| layout.group_slot(at.y));
            let (Some(_), Some(slot)) = (group, slot) else {
                return;
            };
            let carry = Carry { row, slot };
            if iv.carrying.replace(Some(carry)) != Some(carry) {
                self.redraw();
            }
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            let at = self.convertPoint_fromView(event.locationInWindow(), None);
            let iv = self.ivars();
            let pressed = iv.pressed.take();
            if let Some(carry) = iv.carrying.take() {
                self.redraw();
                self.put(carry);
                return;
            }
            if let Some(pressed) = pressed
                && self.hot_at(at) == Some(pressed)
            {
                self.choose(pressed);
            }
        }

        /// The highlight follows the pointer from row to row; over a rule or the margin it stays
        /// where it was, so going down the list it slides on and does not blink out between two
        /// rows. It goes when the pointer leaves the card.
        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, event: &NSEvent) {
            let at = self.convertPoint_fromView(event.locationInWindow(), None);
            if let Some(hot) = self.hot_at(at) {
                self.light(Some(hot));
            }
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            self.light(None);
        }

        /// Esc closes; ↓ and ↑ walk the rows; Return chooses. The keys that go to no row are no
        /// one's: the list holds the keyboard while it is up.
        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            match event.keyCode() {
                ESCAPE => {
                    // Esc while a row is carried takes the pull back; the list stays up.
                    let iv = self.ivars();
                    if iv.carrying.take().is_some() {
                        iv.pressed.set(None);
                        self.redraw();
                    } else if let Some(window) = self.owner() {
                        window.bar().close_group_list(true);
                    }
                }
                RETURN | ENTER => {
                    if let Some(hot) = self.ivars().hot.get() {
                        self.choose(hot);
                    }
                }
                DOWN => self.step(1),
                UP => self.step(-1),
                _ => {}
            }
        }

        #[unsafe(method(accessibilityPerformPress))]
        fn accessibility_press(&self) -> bool {
            true
        }
    }
);

impl GroupList {
    pub(super) fn new(mtm: MainThreadMarker, window: u64) -> Retained<Self> {
        let lit = plain_box(mtm, Tint::of(0, 0.0), ROW_RADIUS);
        lit.setAlphaValue(0.0);
        lit.setAccessibilityElement(false);
        let marks = ListMarks::new(mtm);
        let this = Self::alloc(mtm).set_ivars(ListIvars {
            window: Cell::new(window),
            palette: Cell::new(None),
            rows: RefCell::new(Vec::new()),
            shown: Cell::new(None),
            layout: RefCell::new(None),
            hot: Cell::new(None),
            lit,
            marks,
            pressed: Cell::new(None),
            press_at: Cell::new(NSPoint::ZERO),
            carrying: Cell::new(None),
            names: RefCell::new(Vec::new()),
            keys: RefCell::new(Vec::new()),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        // Its own layer, which carries the shadow, as the summary card's.
        this.setWantsLayer(true);
        this.setAccessibilityElement(true);
        // The highlight first and the marks over it; the names and keys come over both.
        this.addSubview(&this.ivars().lit);
        this.addSubview(&this.ivars().marks);
        let options = NSTrackingAreaOptions::MouseEnteredAndExited
            | NSTrackingAreaOptions::MouseMoved
            | NSTrackingAreaOptions::ActiveAlways
            | NSTrackingAreaOptions::InVisibleRect;
        // SAFETY: `owner` is the card, which owns the area and outlives it; no user info.
        // `InVisibleRect` follows the view's bounds, so the rect is ignored.
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
        this
    }

    /// The window whose bar the list belongs to.
    fn owner(&self) -> Option<Retained<crate::window::TerminalWindow>> {
        app::delegate(self.mtm())?.window(self.ivars().window.get())
    }

    /// `hitTest:`'s body: `point` is in the superview's space.
    fn hit(&self, point: NSPoint) -> Option<Retained<NSView>> {
        if self.isHidden() || !contains(self.frame(), point) {
            return None;
        }
        let view: &NSView = self;
        Some(view.retain())
    }

    /// Writes the rows — and so the card's size, which is returned: the longest name sets the
    /// width. The group on screen is checked.
    pub(super) fn set(
        &self,
        rows: &[SwitchRow],
        shown: Option<GroupId>,
        palette: Palette,
    ) -> NSSize {
        let iv = self.ivars();
        if iv.palette.replace(Some(palette)) != Some(palette) {
            let shadow = NSShadow::new();
            shadow.setShadowOffset(NSSize::new(0.0, -3.0));
            shadow.setShadowBlurRadius(12.0);
            shadow.setShadowColor(Some(&palette.card_shadow.color()));
            self.setShadow(Some(&shadow));
            iv.lit.setFillColor(&palette.hover.color());
        }
        iv.rows.replace(rows.to_vec());
        iv.shown.set(shown);
        // One name and one key per row, and New Group's own.
        let wanted = rows.len() + 1;
        let (mut names, mut keys) = (iv.names.borrow_mut(), iv.keys.borrow_mut());
        while names.len() > wanted {
            for field in [names.pop(), keys.pop()].into_iter().flatten() {
                field.removeFromSuperview();
            }
        }
        while names.len() < wanted {
            let (name, key) = (label(self.mtm()), label(self.mtm()));
            name.setAlignment(NSTextAlignment::Left);
            key.setAlignment(NSTextAlignment::Right);
            self.addSubview(&name);
            self.addSubview(&key);
            names.push(name);
            keys.push(key);
        }
        // SAFETY: AppKit's constant font weights.
        let (semibold, medium) = unsafe { (NSFontWeightSemibold, NSFontWeightMedium) };
        let widest = rows
            .iter()
            .map(|row| {
                let field = &names[0];
                field.setFont(Some(&NSFont::systemFontOfSize_weight(NAME_TEXT, semibold)));
                field.setStringValue(&NSString::from_str(&row.name));
                field.intrinsicContentSize().width.ceil()
            })
            .fold(0.0, f64::max);
        let layout = tabs::group_list(rows.len(), widest);
        let width = layout.width - 2.0 * tabs::GROUP_LIST_PAD;
        let left = tabs::GROUP_LIST_PAD;
        let put = |at: usize, span: tabs::Span, text: &str, key: &str, current: bool| {
            let (name, hint) = (&names[at], &keys[at]);
            name.setFont(Some(&NSFont::systemFontOfSize_weight(
                NAME_TEXT,
                if current { semibold } else { medium },
            )));
            name.setStringValue(&NSString::from_str(text));
            name.setTextColor(Some(&Tint::of(palette.title, 1.0).color()));
            let height = name.intrinsicContentSize().height;
            name.setFrame(NSRect::new(
                NSPoint::new(left + ROW_NAME_X, span.x + (tabs::GROUP_ROW - height) / 2.0),
                NSSize::new((width - ROW_NAME_X - ROW_RIGHT).max(0.0), height),
            ));
            hint.setFont(Some(&NSFont::systemFontOfSize_weight(KEY_TEXT, medium)));
            hint.setStringValue(&NSString::from_str(key));
            hint.setTextColor(Some(&Tint::of(palette.dim, 1.0).color()));
            let height = hint.intrinsicContentSize().height;
            hint.setFrame(NSRect::new(
                NSPoint::new(
                    left + width - ROW_END - KEY_WIDTH,
                    span.x + (tabs::GROUP_ROW - height) / 2.0,
                ),
                NSSize::new(KEY_WIDTH, height),
            ));
        };
        for (at, (row, span)) in rows.iter().zip(&layout.rows).enumerate() {
            let key = row.hint.clone().unwrap_or_default();
            put(at, *span, &row.name, &key, row.group == shown);
        }
        put(
            rows.len(),
            layout.new_row,
            "New Group",
            &crate::menu::keys::glyphs_of(self.mtm(), "new_group").unwrap_or_default(),
            false,
        );
        let size = NSSize::new(layout.width, layout.height);
        iv.layout.replace(Some(layout));
        // The row the pointer or the keys were on may not be there any more.
        if let Some(Hot::Row(row)) = iv.hot.get()
            && row >= rows.len()
        {
            iv.hot.set(None);
        }
        // The rows may stand elsewhere now: the highlight goes with its row at once.
        iv.marks.setFrame(NSRect::new(NSPoint::ZERO, size));
        self.place_lit(iv.hot.get(), 0.0);
        self.setAccessibilityLabel(Some(&NSString::from_str("Tab groups")));
        self.redraw();
        size
    }

    /// The row at `point` (the card's space); `None` over a rule or the margin.
    fn hot_at(&self, point: NSPoint) -> Option<Hot> {
        let layout = self.ivars().layout.borrow();
        let layout = layout.as_ref()?;
        let inside =
            point.x >= tabs::GROUP_LIST_PAD && point.x < layout.width - tabs::GROUP_LIST_PAD;
        let over = |span: &tabs::Span| point.y >= span.x && point.y < span.x + span.width;
        if !inside {
            return None;
        }
        if over(&layout.new_row) {
            return Some(Hot::New);
        }
        layout.rows.iter().position(over).map(Hot::Row)
    }

    /// The pointer or the keys went to `hot`: the highlight follows — it slides from the row it
    /// was on, comes in place where there was none and fades where there is none now; at once
    /// under Reduce Motion.
    fn light(&self, hot: Option<Hot>) {
        let was = self.ivars().hot.replace(hot);
        if was == hot {
            return;
        }
        let reduce = self
            .owner()
            .is_some_and(|window| window.bar().reduce_motion());
        let slide = if reduce || was.is_none() {
            0.0
        } else {
            LIT_SLIDE
        };
        self.place_lit(hot, slide);
    }

    /// Puts the highlight under `hot`'s row over `secs` (at once for zero), or fades it out for
    /// none.
    fn place_lit(&self, hot: Option<Hot>, secs: f64) {
        let iv = self.ivars();
        let reduce = self
            .owner()
            .is_some_and(|window| window.bar().reduce_motion());
        let fading = if reduce { 0.0 } else { LIT_FADE };
        let rect = hot.and_then(|hot| self.row_rect(hot));
        let Some(rect) = rect else {
            fade(&iv.lit, 0.0, fading);
            return;
        };
        if secs > 0.0 {
            let lit = iv.lit.retain();
            animate(secs, move || lit.animator().setFrame(rect));
        } else {
            // A move under way would carry on to the row it left.
            if let Some(layer) = iv.lit.layer() {
                layer.removeAllAnimations();
            }
            still(|| iv.lit.setFrame(rect));
        }
        fade(&iv.lit, 1.0, fading);
    }

    /// The box `hot`'s row is highlighted in, in the card's space.
    fn row_rect(&self, hot: Hot) -> Option<NSRect> {
        let layout = self.ivars().layout.borrow();
        let layout = layout.as_ref()?;
        let span = match hot {
            Hot::Row(row) => *layout.rows.get(row)?,
            Hot::New => layout.new_row,
        };
        let left = tabs::GROUP_LIST_PAD;
        Some(NSRect::new(
            NSPoint::new(left, span.x),
            NSSize::new(layout.width - 2.0 * left, span.width),
        ))
    }

    /// The card and its marks drawn again (the highlight is a view of its own).
    fn redraw(&self) {
        self.setNeedsDisplay(true);
        self.ivars().marks.setNeedsDisplay(true);
    }

    /// ↓ (`1`) and ↑ (`-1`): the next row, and New Group after the last.
    fn step(&self, by: isize) {
        let iv = self.ivars();
        let rows = iv.rows.borrow().len();
        let at = match iv.hot.get() {
            None => None,
            Some(Hot::Row(row)) => Some(row),
            Some(Hot::New) => Some(rows),
        };
        let next = match (at, by) {
            (None, 1) => 0,
            (None, _) => rows,
            (Some(at), by) => at.saturating_add_signed(by).min(rows),
        };
        self.light(Some(if next >= rows {
            Hot::New
        } else {
            Hot::Row(next)
        }));
    }

    /// A group's row let go at its slot: the group takes that place among the groups, as the move
    /// that can be undone ([`Move::MoveGroup`]); the list stays up and shows the new order.
    fn put(&self, carry: Carry) {
        let group = self
            .ivars()
            .rows
            .borrow()
            .get(carry.row)
            .and_then(|row| row.group);
        let Some((window, group)) = self.owner().zip(group) else {
            return;
        };
        // Row 0 is the ungrouped tabs': the group at row `n` is the `n - 1`th.
        window.make_move(Move::MoveGroup {
            window: window.id(),
            group,
            to: group_place(carry.row - 1, carry.slot),
        });
    }

    /// A row chosen: the list closes and the window goes where the row says.
    fn choose(&self, hot: Hot) {
        let group = match hot {
            Hot::Row(row) => match self.ivars().rows.borrow().get(row) {
                Some(row) => Some(row.group),
                None => return,
            },
            Hot::New => None,
        };
        let Some(window) = self.owner() else {
            return;
        };
        window.bar().close_group_list(false);
        match group {
            Some(group) => window.reach_cluster(group),
            None => {
                if let Some(app) = app::delegate(self.mtm()) {
                    app.new_group_in(&window);
                }
            }
        }
    }

    /// The card itself; the highlight and the marks are views over it.
    fn draw(&self) {
        let Some(palette) = self.ivars().palette.get() else {
            return;
        };
        rounded(
            self.bounds(),
            CARD_RADIUS,
            palette.card,
            Some(palette.card_line),
        );
    }

    /// What stands over the highlight ([`ListMarks`]): the check of the group on screen, the
    /// indicators, New Group's plus, the rules, and the line a carried row would be let go at.
    fn draw_marks(&self) {
        let iv = self.ivars();
        let (Some(palette), Some(layout)) = (iv.palette.get(), iv.layout.borrow().clone()) else {
            return;
        };
        let left = tabs::GROUP_LIST_PAD;
        let width = layout.width - 2.0 * left;
        let row_rect = |span: &tabs::Span| {
            NSRect::new(NSPoint::new(left, span.x), NSSize::new(width, span.width))
        };
        let rows = iv.rows.borrow();
        let shown = iv.shown.get();
        let carrying = iv.carrying.get();
        for (row, span) in rows.iter().zip(&layout.rows) {
            let rect = row_rect(span);
            let middle = rect.origin.y + rect.size.height / 2.0;
            if row.group == shown {
                draw_check(
                    NSPoint::new(left + 9.0, middle - 6.0),
                    Tint::of(palette.title, 1.0),
                );
            }
            if let Some(indicator) = row.indicator {
                let at = NSPoint::new(
                    left + width - ROW_END - KEY_WIDTH - ROW_END - INDICATOR,
                    middle - INDICATOR / 2.0,
                );
                draw_indicator(indicator, at, palette);
            }
        }
        let rect = row_rect(&layout.new_row);
        let (x, y) = (left + 9.0 + 5.5, rect.origin.y + rect.size.height / 2.0);
        let plus = NSBezierPath::bezierPath();
        plus.moveToPoint(NSPoint::new(x - 4.0, y));
        plus.lineToPoint(NSPoint::new(x + 4.0, y));
        plus.moveToPoint(NSPoint::new(x, y - 4.0));
        plus.lineToPoint(NSPoint::new(x, y + 4.0));
        stroke(&plus, 1.5, Tint::of(palette.title, 1.0));
        for &rule in &layout.rules {
            let line = NSBezierPath::bezierPath();
            line.moveToPoint(NSPoint::new(left + 6.0, rule));
            line.lineToPoint(NSPoint::new(layout.width - left - 6.0, rule));
            stroke(&line, 1.0, palette.separator);
        }
        // A group's row on its way: the line between two groups where it would be let go.
        if let Some(carry) = carrying {
            let y = layout.slot_line(carry.slot);
            rounded(
                NSRect::new(
                    NSPoint::new(left + 6.0, y - 1.0),
                    NSSize::new(width - 12.0, 2.0),
                ),
                1.0,
                Tint::of(palette.accent, 1.0),
                None,
            );
        }
    }
}

/// The check at the left of the row on screen: the design's tick in a 12-point square whose top
/// left is `at`.
fn draw_check(at: NSPoint, ink: Tint) {
    let path = NSBezierPath::bezierPath();
    path.moveToPoint(NSPoint::new(at.x + 2.6, at.y + 6.4));
    path.lineToPoint(NSPoint::new(at.x + 5.0, at.y + 8.8));
    path.lineToPoint(NSPoint::new(at.x + 9.4, at.y + 3.6));
    stroke(&path, 1.6, ink);
}

/// What a group asks of the user, in the row's 12-point square at `at`: a question, a failure, a
/// finish not yet seen ([`tabs::group_indicator`] says nothing else).
fn draw_indicator(indicator: Indicator, at: NSPoint, palette: Palette) {
    let role = Tint::of(palette.tone(tabs::list_dot(indicator), 0), 1.0);
    let point = |x: f64, y: f64| NSPoint::new(at.x + x, at.y + y);
    match indicator {
        Indicator::Question => {
            let ring = NSBezierPath::bezierPathWithOvalInRect(NSRect::new(
                point(0.75, 0.75),
                NSSize::new(10.5, 10.5),
            ));
            stroke(&ring, 1.3, role);
            let mark = NSBezierPath::bezierPath();
            mark.moveToPoint(point(4.3, 4.6));
            mark.curveToPoint_controlPoint1_controlPoint2(
                point(7.7, 4.6),
                point(4.3, 2.5),
                point(7.7, 2.5),
            );
            mark.curveToPoint_controlPoint1_controlPoint2(
                point(6.0, 7.0),
                point(7.7, 5.8),
                point(6.0, 5.8),
            );
            stroke(&mark, 1.3, role);
            let dot = NSBezierPath::bezierPathWithOvalInRect(NSRect::new(
                point(5.2, 8.05),
                NSSize::new(1.6, 1.6),
            ));
            role.color().setFill();
            dot.fill();
        }
        Indicator::Failed => {
            let dot = NSBezierPath::bezierPathWithOvalInRect(NSRect::new(
                point(2.5, 2.5),
                NSSize::new(7.0, 7.0),
            ));
            role.color().setFill();
            dot.fill();
        }
        Indicator::Finished => {
            let tick = NSBezierPath::bezierPath();
            tick.moveToPoint(point(2.6, 6.4));
            tick.lineToPoint(point(5.0, 8.8));
            tick.lineToPoint(point(9.4, 3.6));
            stroke(&tick, 1.8, role);
        }
        // A group shows none of these: they say nothing the user has to do.
        Indicator::Running | Indicator::Uploading => {}
    }
}

// ─── The bar's side of it ────────────────────────────────────────────────

impl TabBar {
    /// A press on the group button: the list of the window's groups opens under it, or closes.
    pub(super) fn toggle_group_list(&self) {
        if self.ivars().list_open.get() {
            self.close_group_list(true);
        } else {
            self.open_group_list();
        }
    }

    /// Puts the list up under the group button, over everything the window holds, with the view
    /// that takes the presses away from it behind it; the card has the keyboard. A name being
    /// typed is kept first, and so is no card: a card and a list would be two things on the
    /// same row.
    fn open_group_list(&self) {
        let iv = self.ivars();
        if iv.side.get().is_some() || iv.list_open.get() {
            return;
        }
        // Before the list is read: the name kept may be the one it says.
        self.end_rename(true);
        self.close_card();
        let (Some(palette), Some(switcher)) =
            (iv.palette.get(), iv.shown.borrow().switcher.clone())
        else {
            return;
        };
        // SAFETY: reading the superview; we are on the main thread.
        let Some(root) = (unsafe { self.superview() }) else {
            return;
        };
        let size = iv.group_list.set(&switcher.rows, switcher.shown, palette);
        let button = iv.switch.frame();
        let bar = self.frame().origin;
        let room = root.bounds().size;
        let left = (bar.x + button.origin.x)
            .min(room.width - size.width - LIST_MARGIN)
            .max(LIST_MARGIN);
        let top = bar.y + button.origin.y + button.size.height + LIST_DROP;
        let scrim = &iv.scrim;
        scrim.setFrame(root.bounds());
        scrim.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        scrim.set_hole(NSRect::new(
            NSPoint::new(bar.x + button.origin.x, bar.y + button.origin.y),
            button.size,
        ));
        iv.group_list
            .setFrame(NSRect::new(NSPoint::new(left, top), size));
        scrim.addSubview(&iv.group_list);
        root.addSubview_positioned_relativeTo(scrim, NSWindowOrderingMode::Above, None);
        iv.list_open.set(true);
        iv.switch.set_open(true);
        if let Some(window) = self.window() {
            window.makeFirstResponder(Some(&*iv.group_list));
        }
    }

    /// Takes the list down; the keyboard goes back to the tab on screen when `refocus` (a choice
    /// that brings a tab on screen does that itself, and a keyboard that went elsewhere is where
    /// it went).
    pub(crate) fn close_group_list(&self, refocus: bool) {
        let iv = self.ivars();
        // First: taking the card away from the keyboard says so again (`resignFirstResponder`).
        if !iv.list_open.replace(false) {
            return;
        }
        iv.group_list.removeFromSuperview();
        iv.scrim.removeFromSuperview();
        iv.switch.set_open(false);
        if refocus && let Some(window) = self.terminal_window() {
            window.focus_selected_tab();
        }
    }

    /// What the list is up for: the group on screen and its tabs.
    pub(super) fn listing(&self) -> (Option<GroupId>, Vec<u64>) {
        let shown = self.ivars().shown.borrow();
        (
            shown.switcher.as_ref().and_then(|switcher| switcher.shown),
            shown.tabs.iter().map(|label| label.tab).collect(),
        )
    }

    /// The window's groups changed while the list is up: it comes down with the last group, the
    /// title row's going, or a change of what is on screen — a tab opened or closed, another group
    /// reached by a key, which a view put over the window since would hide it — and otherwise
    /// its rows say what is there now (a name, a dot).
    pub(super) fn refresh_group_list(&self, before: Option<(Option<GroupId>, Vec<u64>)>) {
        let iv = self.ivars();
        if !iv.list_open.get() {
            return;
        }
        let changed = before.is_some_and(|before| before != self.listing());
        match (iv.palette.get(), iv.shown.borrow().switcher.clone()) {
            (Some(palette), Some(switcher)) if !changed && iv.side.get().is_none() => {
                let size = iv.group_list.set(&switcher.rows, switcher.shown, palette);
                iv.group_list.setFrameSize(size);
            }
            _ => self.close_group_list(false),
        }
    }
}
