//! A group's header in the side column: the line that stands above the group's rows.
//!
//! **What it shows.** An arrow (down while the group is open, right while it is closed), the
//! group's name in the column's dim ink — the foreground's while the pointer is over it — and, at
//! the right, what the group has to say: while the pointer is over the header a `+` that opens a
//! tab in the group; while the group is closed, how many tabs it holds and, if one of them asks for
//! the user (a question, a failure, an unseen finish — `tabs::group_indicator`), its dot; and while
//! ⌃⌘ is held the key that reaches the group (⌃⌘1…⌃⌘9). The header is selected by nothing: the tab
//! on screen is a row, and a closed group keeps listing it under its header.
//!
//! **What it does.** A press on the header opens or closes the group; the second press of a double
//! click names it ([`TabBar::begin_group_rename`], which puts the bar's name field over the name),
//! and a right click is the group's menu ([`TabBar::group_menu`]). The `+` is the group's own New
//! Tab. A press that moves [`DRAG_SLOP`] before the button comes up is not a click: it carries the
//! group away in a drag session ([`TabBar::carry_group`]) — among the groups of the column, to a
//! window of its own, to another window — and opens or closes nothing. Nothing here changes the
//! group: the window's appliers do ([`TerminalWindow::set_group_collapsed`],
//! [`TerminalWindow::rename_group`]), and the moves ([`crate::moves`]) a drop comes to.
//!
//! **The arrow turns by crossing over**, not by rotating: two pictures of it, one the other's
//! opacity, over [`TURN_SECS`] — the same Core Animation, on the window server, as every other
//! fade of the bar, and nothing to guess about which way a rotated layer turns in a flipped view.
//! Reduce Motion puts it there at once.
//!
//! A header is a child of the bar's list view, like a chip: the list's mask fades it where rows
//! run under the column's ends, and the bar keeps one per group across changes of the list so a
//! reflow can slide it.

use std::cell::{Cell, RefCell};

use objc2::rc::Retained;
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send,
};
use objc2_app_kit::{
    NSAccessibility, NSAccessibilityButtonRole, NSAnimatablePropertyContainer, NSBezierPath,
    NSCursor, NSEvent, NSFont, NSFontWeightMedium, NSFontWeightSemibold, NSMenu, NSTextAlignment,
    NSTextField, NSTrackingArea, NSTrackingAreaOptions, NSView,
};
use objc2_foundation::{NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};
use objc2_quartz_core::{CALayer, CATransaction};

use super::{
    CLOSE_INSET, CLOSE_RADIUS, CLOSE_SIDE, HINT_TEXT, Heading, Palette, TabBar, Tint, animate,
    contains, label, paint, rounded, stroke,
};
use crate::groups::GroupId;
use crate::tabs::{self, DRAG_SLOP, Indicator};

/// The arrow's square, and the distance of its middle from the header's left edge.
const ARROW: f64 = 8.0;
const ARROW_MIDDLE: f64 = 14.0;
/// Where the name starts, and the name's and the count's type size.
pub(super) const NAME_X: f64 = 24.0;
pub(super) const NAME_TEXT: f64 = 11.5;
const COUNT_TEXT: f64 = 11.0;
/// The space the right end keeps from the header's edge when no `+` stands there, and between the
/// things at the right.
pub(super) const END_PAD: f64 = 10.0;
const END_GAP: f64 = 6.0;
/// The dot of a closed group's indicator: its side.
const DOT: f64 = 6.0;
/// How long the arrow takes to turn (the design's 160 ms).
const TURN_SECS: f64 = 0.16;
/// How much of its ink a header keeps while it is carried away.
const TORN_ALPHA: f64 = 0.45;

/// What a press on the header began as, and so what its release does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Press {
    /// On the header: the group opens or closes.
    Header,
    /// On the `+`: a new tab in the group.
    Plus,
}

/// What the bar tells a header besides its group ([`GroupHeader::set`]).
#[derive(Clone, Copy, Debug)]
pub(super) struct HeaderLook {
    /// ⌃⌘ is held: the group's key shows.
    pub(super) hints: bool,
    /// The name field is over the name.
    pub(super) naming: bool,
    /// Reduce Motion: no turning of the arrow, no sliding.
    pub(super) reduce: bool,
    /// The reflow's duration when the applier changed the list: an old header slides to its place.
    pub(super) slide: Option<f64>,
    /// The header is carried away in a drag session: it waits, faint, in its place until the
    /// session ends.
    pub(super) torn: bool,
}

pub(super) struct HeaderIvars {
    group: Cell<Option<GroupId>>,
    heading: RefCell<Option<Heading>>,
    palette: Cell<Option<Palette>>,
    look: Cell<Option<HeaderLook>>,
    /// The pointer is over the header: the name is in full ink and the `+` shows.
    hovered: Cell<bool>,
    /// The pointer is over the `+` itself: the stronger fill.
    over_plus: Cell<bool>,
    pressed: Cell<Option<Press>>,
    /// Where the press that is `pressed` went down, in the window's space: how far the pointer
    /// has gone from it says whether it is a drag.
    press_at: Cell<NSPoint>,
    /// Whether the group was open before the last release closed it or opened it, so a double
    /// click can put it back: the first click of a double click toggled the group.
    toggled_from: Cell<Option<bool>>,
    /// Born in this layout: it is put in place at once.
    fresh: Cell<bool>,
    /// The frame the bar last gave it.
    target: Cell<NSRect>,
    /// What the arrow's two pictures were last painted in: the ink and the screen's scale.
    painted: Cell<Option<(Tint, f64)>>,
    /// Where the closed group's indicator dot is drawn, if it has one.
    dot: Cell<Option<NSPoint>>,
    name: Retained<NSTextField>,
    count: Retained<NSTextField>,
    hint: Retained<NSTextField>,
    /// The arrow of a closed group, and of an open one: one is clear while the other shows.
    right: Retained<CALayer>,
    down: Retained<CALayer>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; GroupHeader implements no `Drop` and is born
    // with `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriGroupHeader"]
    #[ivars = HeaderIvars]
    pub(super) struct GroupHeader;

    unsafe impl NSObjectProtocol for GroupHeader {}

    impl GroupHeader {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        /// The header takes every press inside it; its labels never do.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
            // No `return`: `define_class!` converts the body's last expression.
            self.hit(point)
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        /// The header is not the window's to drag.
        #[unsafe(method(mouseDownCanMoveWindow))]
        fn can_move_window(&self) -> bool {
            false
        }

        /// The `+` is drawn only while the pointer is over the header.
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            self.draw();
        }

        /// A press may open or close the group (on release); the second press of a double click
        /// names it.
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let iv = self.ivars();
            let at = self.convertPoint_fromView(event.locationInWindow(), None);
            let press = if self.over_plus_at(at) {
                Press::Plus
            } else {
                Press::Header
            };
            if press == Press::Header && event.clickCount() == 2 {
                // The first click of this double click toggled the group; it was a name that was
                // asked for, so the group goes back.
                iv.pressed.set(None);
                self.rename();
                return;
            }
            if event.clickCount() < 2 {
                // A new run of clicks: nothing of the last one is left to put back.
                iv.toggled_from.set(None);
            }
            iv.pressed.set(Some(press));
            iv.press_at.set(event.locationInWindow());
        }

        /// The pointer moves with the button down on the header: past [`DRAG_SLOP`] the press is a
        /// pull, and the group is carried away — the button's release will find no press to
        /// open or close the group with.
        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            let iv = self.ivars();
            if iv.pressed.get() != Some(Press::Header) {
                return;
            }
            let (now, from) = (event.locationInWindow(), iv.press_at.get());
            if (now.x - from.x).hypot(now.y - from.y) < DRAG_SLOP {
                return;
            }
            iv.pressed.set(None);
            iv.toggled_from.set(None);
            if let (Some(bar), Some(group)) = (self.bar(), iv.group.get()) {
                bar.carry_group(group, self, event, true);
            }
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            let iv = self.ivars();
            let Some(press) = iv.pressed.take() else {
                return;
            };
            let at = self.convertPoint_fromView(event.locationInWindow(), None);
            if !contains(self.bounds(), at) {
                return;
            }
            match press {
                Press::Plus if self.over_plus_at(at) => self.new_tab(),
                Press::Header if !self.over_plus_at(at) => self.toggle(),
                _ => {}
            }
        }

        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            self.ivars().hovered.set(true);
            self.refresh();
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            let iv = self.ivars();
            iv.hovered.set(false);
            iv.over_plus.set(false);
            self.refresh();
        }

        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, event: &NSEvent) {
            let at = self.convertPoint_fromView(event.locationInWindow(), None);
            let over = self.over_plus_at(at);
            if self.ivars().over_plus.replace(over) != over {
                self.setNeedsDisplay(true);
            }
        }

        /// The group's own menu, on a right click or a control click.
        #[unsafe(method_id(menuForEvent:))]
        fn menu_for_event(&self, _event: &NSEvent) -> Option<Retained<NSMenu>> {
            // No `?`: `define_class!` converts the body's last expression.
            let group = self.ivars().group.get();
            self.bar().zip(group).and_then(|(bar, group)| bar.group_menu(group))
        }

        /// A header is a clickable thing: the hand over all of it.
        #[unsafe(method(resetCursorRects))]
        fn reset_cursor_rects(&self) {
            self.addCursorRect_cursor(self.bounds(), &NSCursor::pointingHandCursor());
        }

        #[unsafe(method(accessibilityPerformPress))]
        fn accessibility_press(&self) -> bool {
            self.toggle();
            true
        }
    }
);

impl GroupHeader {
    pub(super) fn new(mtm: MainThreadMarker, group: GroupId) -> Retained<Self> {
        let name = label(mtm);
        let count = label(mtm);
        let hint = label(mtm);
        let this = Self::alloc(mtm).set_ivars(HeaderIvars {
            group: Cell::new(Some(group)),
            heading: RefCell::new(None),
            palette: Cell::new(None),
            look: Cell::new(None),
            hovered: Cell::new(false),
            over_plus: Cell::new(false),
            pressed: Cell::new(None),
            press_at: Cell::new(NSPoint::ZERO),
            toggled_from: Cell::new(None),
            fresh: Cell::new(true),
            target: Cell::new(NSRect::ZERO),
            painted: Cell::new(None),
            dot: Cell::new(None),
            name: name.clone(),
            count: count.clone(),
            hint: hint.clone(),
            right: CALayer::new(),
            down: CALayer::new(),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        // SAFETY: AppKit's constant font weights.
        let (semibold, medium) = unsafe { (NSFontWeightSemibold, NSFontWeightMedium) };
        name.setFont(Some(&NSFont::systemFontOfSize_weight(NAME_TEXT, semibold)));
        name.setAlignment(NSTextAlignment::Left);
        count.setFont(Some(&NSFont::systemFontOfSize_weight(COUNT_TEXT, medium)));
        count.setAlignment(NSTextAlignment::Right);
        hint.setFont(Some(&NSFont::systemFontOfSize_weight(HINT_TEXT, medium)));
        hint.setAlignment(NSTextAlignment::Right);
        this.addSubview(&name);
        this.addSubview(&count);
        this.addSubview(&hint);
        this.setWantsLayer(true);
        if let Some(layer) = this.layer() {
            let ivars = this.ivars();
            layer.addSublayer(&ivars.right);
            layer.addSublayer(&ivars.down);
        }
        // Enter, exit and moves, in an inactive window too, as the bar's other hovers.
        let options = NSTrackingAreaOptions::MouseEnteredAndExited
            | NSTrackingAreaOptions::MouseMoved
            | NSTrackingAreaOptions::ActiveInActiveApp
            | NSTrackingAreaOptions::InVisibleRect;
        // SAFETY: `owner` is the header, which owns the area and outlives it; no user info.
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
        this.setAccessibilityElement(true);
        // SAFETY: AppKit's constant role.
        this.setAccessibilityRole(Some(unsafe { NSAccessibilityButtonRole }));
        this
    }

    /// The group this header stands above.
    pub(super) fn group(&self) -> Option<GroupId> {
        self.ivars().group.get()
    }

    /// The bar the header is in: its list's superview.
    fn bar(&self) -> Option<Retained<TabBar>> {
        // SAFETY: reading the superviews; we are on the main thread.
        unsafe { self.superview()?.superview() }?
            .downcast::<TabBar>()
            .ok()
    }

    /// `hitTest:`'s body: `point` is in the list's space.
    fn hit(&self, point: NSPoint) -> Option<Retained<NSView>> {
        if self.isHidden() || !contains(self.frame(), point) {
            return None;
        }
        Some(self.retain_view())
    }

    fn retain_view(&self) -> Retained<NSView> {
        let view: &NSView = self;
        view.retain()
    }

    /// Places the header at `frame` (the list's space) and gives it its group's look.
    pub(super) fn set(&self, frame: NSRect, heading: &Heading, palette: Palette, look: HeaderLook) {
        let iv = self.ivars();
        let fresh = iv.fresh.replace(false);
        let moved = iv.target.replace(frame) != frame;
        match look.slide {
            Some(secs) if fresh => {
                self.setFrame(frame);
                self.setAlphaValue(0.0);
                let this = self.retain_view();
                animate(secs, move || this.animator().setAlphaValue(1.0));
            }
            Some(secs) if moved => {
                let this = self.retain_view();
                animate(secs, move || this.animator().setFrame(frame));
            }
            _ if moved || fresh => self.setFrame(frame),
            _ => {}
        }
        // Faint while carried, full ink again the layout after; a header that fades in is left
        // to its own fade.
        if !(fresh && look.slide.is_some()) {
            self.setAlphaValue(if look.torn { TORN_ALPHA } else { 1.0 });
        }
        iv.group.set(Some(heading.group));
        iv.heading.replace(Some(heading.clone()));
        iv.palette.set(Some(palette));
        iv.look.set(Some(look));
        self.refresh();
    }

    /// The header's size is the last frame given, not the one a slide is passing through: its
    /// parts are placed for where it is going.
    fn size(&self) -> NSSize {
        self.ivars().target.get().size
    }

    /// The `+`'s square, at the right end, vertically centred.
    fn plus_rect(&self) -> NSRect {
        let size = self.size();
        NSRect::new(
            NSPoint::new(
                size.width - CLOSE_INSET - CLOSE_SIDE,
                ((size.height - CLOSE_SIDE) / 2.0).max(0.0),
            ),
            NSSize::new(CLOSE_SIDE, CLOSE_SIDE),
        )
    }

    /// Whether `point` (the header's space) is on the `+` — which is there only while the pointer
    /// is over the header and no name is being typed.
    fn over_plus_at(&self, point: NSPoint) -> bool {
        self.plus_shown() && contains(self.plus_rect(), point)
    }

    fn plus_shown(&self) -> bool {
        let iv = self.ivars();
        iv.hovered.get() && iv.look.get().is_some_and(|look| !look.naming)
    }

    /// Lays the parts out for the state they are in and draws what is drawn: the labels' text,
    /// colour and places, the arrow's pictures, the `+` and the dot.
    fn refresh(&self) {
        let iv = self.ivars();
        let (Some(palette), Some(look)) = (iv.palette.get(), iv.look.get()) else {
            return;
        };
        let heading = iv.heading.borrow().clone();
        let Some(heading) = heading else {
            return;
        };
        let size = self.size();
        let hovered = iv.hovered.get();
        let ink = Tint::of(if hovered { palette.title } else { palette.dim }, 1.0);
        let mid = size.height / 2.0;

        // The right end, from the edge in: the `+` while the pointer is over, else the key under
        // ⌃⌘, else the count and the dot of a closed group.
        let mut right = size.width - END_PAD;
        if self.plus_shown() {
            right = self.plus_rect().origin.x - END_GAP;
        }
        let ended = |field: &NSTextField, text: &str, from: f64| -> f64 {
            field.setStringValue(&NSString::from_str(text));
            field.setTextColor(Some(&Tint::of(palette.dim, 1.0).color()));
            let width = field.intrinsicContentSize().width.ceil();
            let height = field.intrinsicContentSize().height;
            field.setFrame(NSRect::new(
                NSPoint::new(from - width, mid - height / 2.0),
                NSSize::new(width, height),
            ));
            field.setHidden(false);
            from - width
        };
        iv.count.setHidden(true);
        iv.hint.setHidden(true);
        let mut dot_at = None;
        if look.hints
            && let Some(hint) = &heading.hint
        {
            right = ended(&iv.hint, hint, right);
        } else if heading.collapsed {
            right = ended(&iv.count, &heading.tabs.to_string(), right);
            if heading.indicator.is_some() {
                dot_at = Some(NSPoint::new(right - END_GAP - DOT, mid - DOT / 2.0));
                right -= END_GAP + DOT;
            }
        }
        let naming = look.naming;
        // The name keeps what the right end leaves it.
        iv.name.setHidden(naming);
        iv.name.setStringValue(&NSString::from_str(&heading.name));
        iv.name.setTextColor(Some(&ink.color()));
        let height = iv.name.intrinsicContentSize().height;
        iv.name.setFrame(NSRect::new(
            NSPoint::new(NAME_X, mid - height / 2.0),
            NSSize::new((right - END_GAP - NAME_X).max(0.0), height),
        ));
        iv.dot.set(dot_at);
        self.setAccessibilityLabel(Some(&NSString::from_str(&format!(
            "{}, {} {}",
            heading.name,
            heading.tabs,
            if heading.tabs == 1 { "tab" } else { "tabs" }
        ))));
        self.setToolTip(Some(&NSString::from_str(if heading.collapsed {
            "Open group"
        } else {
            "Close group"
        })));
        self.place_arrow(&heading, ink, look.reduce);
        self.setNeedsDisplay(true);
    }

    /// The arrow: its two pictures at the left, and which of them shows.
    fn place_arrow(&self, heading: &Heading, ink: Tint, reduce: bool) {
        let iv = self.ivars();
        let size = self.size();
        let scale = self
            .window()
            .map_or(2.0, |window| window.backingScaleFactor());
        let frame = NSRect::new(
            NSPoint::new(ARROW_MIDDLE - ARROW / 2.0, (size.height - ARROW) / 2.0),
            NSSize::new(ARROW, ARROW),
        );
        let repaint = iv.painted.replace(Some((ink, scale))) != Some((ink, scale));
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        for layer in [&iv.right, &iv.down] {
            layer.setFrame(frame);
        }
        CATransaction::commit();
        if repaint {
            paint(&iv.right, ARROW, scale, move || chevron(ink, false));
            paint(&iv.down, ARROW, scale, move || chevron(ink, true));
        }
        let (right, down) = if heading.collapsed {
            (1.0, 0.0)
        } else {
            (0.0, 1.0)
        };
        CATransaction::begin();
        if reduce {
            CATransaction::setDisableActions(true);
        } else {
            CATransaction::setAnimationDuration(TURN_SECS);
        }
        iv.right.setOpacity(right);
        iv.down.setOpacity(down);
        CATransaction::commit();
    }

    /// The `+` and the dot, over everything the header's own layer holds.
    fn draw(&self) {
        let iv = self.ivars();
        let Some(palette) = iv.palette.get() else {
            return;
        };
        if self.plus_shown() {
            let rect = self.plus_rect();
            let hot = iv.over_plus.get();
            let fill = if hot {
                palette.close_hover
            } else {
                palette.close
            };
            rounded(rect, CLOSE_RADIUS, fill, None);
            let (x, y) = (
                rect.origin.x + rect.size.width / 2.0,
                rect.origin.y + rect.size.height / 2.0,
            );
            let path = NSBezierPath::bezierPath();
            path.moveToPoint(NSPoint::new(x - 3.5, y));
            path.lineToPoint(NSPoint::new(x + 3.5, y));
            path.moveToPoint(NSPoint::new(x, y - 3.5));
            path.lineToPoint(NSPoint::new(x, y + 3.5));
            stroke(
                &path,
                1.5,
                Tint::of(if hot { palette.title } else { palette.dim }, 1.0),
            );
        }
        let indicator = iv
            .heading
            .borrow()
            .as_ref()
            .and_then(|heading| heading.indicator);
        if let (Some(at), Some(indicator)) = (iv.dot.get(), indicator) {
            draw_dot(indicator, at, palette);
        }
    }

    /// A press on the header: the group opens or closes.
    fn toggle(&self) {
        let iv = self.ivars();
        let Some(group) = iv.group.get() else {
            return;
        };
        let was = iv
            .heading
            .borrow()
            .as_ref()
            .is_some_and(|heading| heading.collapsed);
        iv.toggled_from.set(Some(was));
        if let Some(window) = self.bar().and_then(|bar| bar.terminal_window()) {
            window.set_group_collapsed(group, !was);
        }
    }

    /// The `+`: a tab at the end of the group.
    fn new_tab(&self) {
        if let (Some(bar), Some(group)) = (self.bar(), self.ivars().group.get()) {
            bar.new_tab_in_group(group);
        }
    }

    /// A double click: the group is as it was before the click, and its name opens.
    fn rename(&self) {
        let iv = self.ivars();
        let Some(group) = iv.group.get() else {
            return;
        };
        let Some(bar) = self.bar() else {
            return;
        };
        if let (Some(was), Some(window)) = (iv.toggled_from.take(), bar.terminal_window()) {
            window.set_group_collapsed(group, was);
        }
        bar.begin_group_rename(group, false);
    }
}

/// The arrow's picture in the arrow's square (flipped): a chevron pointing right, or down.
fn chevron(ink: Tint, down: bool) {
    let path = NSBezierPath::bezierPath();
    let points = if down {
        [(1.0, 2.5), (4.0, 5.5), (7.0, 2.5)]
    } else {
        [(2.5, 1.0), (5.5, 4.0), (2.5, 7.0)]
    };
    for (index, (x, y)) in points.into_iter().enumerate() {
        let at = NSPoint::new(x, y);
        if index == 0 {
            path.moveToPoint(at);
        } else {
            path.lineToPoint(at);
        }
    }
    stroke(&path, 1.5, ink);
}

/// A closed group's dot, in the role of what it says ([`tabs::list_dot`]), its corner at `at`.
fn draw_dot(indicator: Indicator, at: NSPoint, palette: Palette) {
    let role = palette.tone(tabs::list_dot(indicator), 0);
    let path = NSBezierPath::bezierPathWithOvalInRect(NSRect::new(at, NSSize::new(DOT, DOT)));
    Tint::of(role, 1.0).color().setFill();
    path.fill();
}
