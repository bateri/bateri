//! The window's own tab bar: the title row's view, drawn in the theme's
//! colours, holding one chip per tab and the buttons on the right.
//!
//! **Where it sits.** The window's content reaches under its title row
//! (`FullSizeContentView`) and an empty compact toolbar raises that row and
//! centres the traffic lights in it (`window::TerminalWindow`); the bar is
//! the root view's top strip, the row's height tall — the window reads that
//! height from AppKit, it is not a constant here. The strip starts right of
//! the zoom button ([`LIGHTS_GAP`] past its right edge); in full screen the
//! lights leave the row and the strip starts at the gap alone.
//!
//! **What is where** comes from the pure model (`tabs::Bar::layout`): every
//! chip's span, `+` and the settings warning. With one tab the bar is
//! today's title bar — the title centred, a settings diagnostic beside it
//! ([`single_label`]), `+` on the right — and the title takes no click: a
//! press anywhere but `+` moves the window, a double click does what the
//! system says ([`title_double_click`]). With several tabs each tab is a
//! chip: a press selects it (on the press, like macOS's tabs), a middle
//! click closes it, and while the pointer is over it a `×` shows at its
//! left. A diagnostic becomes a `⚠` left of `+`: its tooltip is the text and
//! a click opens the settings window.
//!
//! **Drawing** is `drawRect:` with `NSBezierPath` and an `NSShadow` on the
//! light theme's selected chip — no layer colours, which would want
//! `CGColor` and with it an `objc2-core-graphics` edge. The colours are the
//! theme's own roles (foreground, dim, warning) at fixed strengths, so no
//! theme role is added; the strengths are the design's. No animation here.
//!
//! **Who acts.** The bar knows its window by id and calls the window's one
//! applier (`TerminalWindow::select_tab`, `close_tab_asking`) — the bar never
//! changes the tab list itself. A close or a new tab is done **one
//! main-queue turn later**: either rebuilds the chips, and the chip whose
//! event is being handled would be taken apart under it.
//!
//! **VoiceOver**: the bar is a tab group, each chip a radio button with the
//! tab-button subrole and its selection as its value, `×` and `+` buttons
//! with names. A `×` that is not shown is still an element (drawn invisible,
//! taking no click), so a tab can be closed without a pointer.

use std::cell::{Cell, RefCell};

use bt_core::Theme;
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel,
};
use objc2_app_kit::{
    NSAccessibility, NSAccessibilityButtonRole, NSAccessibilityRadioButtonRole,
    NSAccessibilityTabButtonSubrole, NSAccessibilityTabGroupRole, NSApplication, NSBezierPath,
    NSColor, NSEvent, NSFont, NSFontWeightMedium, NSFontWeightSemibold, NSLineBreakMode,
    NSLineCapStyle, NSLineJoinStyle, NSShadow, NSTextAlignment, NSTextField, NSTrackingArea,
    NSTrackingAreaOptions, NSView, NSWindowButton, NSWindowStyleMask,
};
use objc2_foundation::{
    NSNumber, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSUserDefaults, ns_string,
};

use crate::app;
use crate::tabs::{BUTTON, Bar};
use crate::window::{TerminalWindow, is_dark_background};

/// Space between the zoom button's right edge and the strip — the design's
/// strip starts at 84 pt beside lights ending at 72 pt. In full screen the
/// lights are not in the row and the strip starts this far from the edge.
const LIGHTS_GAP: f64 = 12.0;

/// A chip's height and corner radius, points (the design's anatomy).
const CHIP_HEIGHT: f64 = 28.0;
const CHIP_RADIUS: f64 = 7.0;

/// The label's inset from a chip's sides: room for `×` on the left, the
/// same on the right so the title stays centred.
const CHIP_PAD: f64 = 26.0;

/// The single tab's title box inset — it has no `×` to make room for.
const SINGLE_PAD: f64 = 8.0;

/// The `×` button: its side, its inset from the chip's corner and its
/// corner radius.
const CLOSE_SIDE: f64 = 20.0;
const CLOSE_INSET: f64 = 4.0;
const CLOSE_RADIUS: f64 = 5.0;

/// Type sizes: a chip's title, and the single tab's slightly larger one.
const CHIP_TEXT: f64 = 12.5;
const SINGLE_TEXT: f64 = 13.0;

/// What a double click on the title row does — the system's "Double-click
/// a window's title bar to" setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TitleAction {
    Zoom,
    Minimize,
    Nothing,
}

/// The setting's answer from its two defaults keys: `AppleActionOnDoubleClick`
/// (`"Minimize"`, `"None"`, and the zoom and fill values), and the older
/// `AppleMiniaturizeOnDoubleClick` that only counts when the newer key is
/// absent. Absent both, the system zooms. The row is our own view, and
/// AppKit's title-bar handling does not reach it — measured: a double click
/// on a view in the row, taking the press or letting it through, neither
/// zoomed nor minimized the window.
pub(crate) fn title_double_click(action: Option<&str>, legacy_minimize: bool) -> TitleAction {
    match action {
        Some("Minimize") => TitleAction::Minimize,
        Some("None") => TitleAction::Nothing,
        Some(_) => TitleAction::Zoom,
        None if legacy_minimize => TitleAction::Minimize,
        None => TitleAction::Zoom,
    }
}

/// The single tab's title line: the title, and a settings diagnostic
/// beside it the way the window's subtitle used to sit
/// ("bateri – settings.toml: …").
pub(crate) fn single_label(title: &str, notice: &str) -> String {
    if notice.is_empty() {
        title.to_owned()
    } else {
        format!("{title} \u{2013} {notice}")
    }
}

/// An sRGB colour with its strength.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Tint {
    rgb: u32,
    alpha: f64,
}

impl Tint {
    const fn of(rgb: u32, alpha: f64) -> Self {
        Self { rgb, alpha }
    }

    fn color(self) -> Retained<NSColor> {
        let byte = |shift: u32| f64::from((self.rgb >> shift) & 0xff) / 255.0;
        NSColor::colorWithSRGBRed_green_blue_alpha(byte(16), byte(8), byte(0), self.alpha)
    }
}

/// The bar's colours, from the theme's roles: the foreground at the
/// design's strengths for fills and lines, the dim role for a tab that is
/// not selected, `warning` for the settings warning. The light theme's
/// selected chip is a white face with a thin shadow instead.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Palette {
    title: u32,
    dim: u32,
    warning: u32,
    selected: Tint,
    selected_line: Tint,
    /// The selected chip's drop shadow; `None` on a dark theme.
    shadow: Option<Tint>,
    hover: Tint,
    close: Tint,
    close_hover: Tint,
    button: Tint,
    button_line: Tint,
    button_hover: Tint,
}

impl Palette {
    fn of(theme: &Theme) -> Self {
        let fg = theme.foreground;
        let ink = |alpha| Tint::of(fg, alpha);
        let base = Self {
            title: fg,
            dim: theme.dim,
            warning: theme.warning,
            selected: ink(0.10),
            selected_line: ink(0.07),
            shadow: None,
            hover: ink(0.05),
            close: ink(0.07),
            close_hover: ink(0.16),
            button: ink(0.06),
            button_line: ink(0.08),
            button_hover: ink(0.13),
        };
        if is_dark_background(theme) {
            base
        } else {
            Self {
                selected: Tint::of(0xffffff, 1.0),
                selected_line: ink(0.08),
                shadow: Some(ink(0.08)),
                close: ink(0.06),
                close_hover: ink(0.14),
                button: ink(0.05),
                button_hover: ink(0.11),
                ..base
            }
        }
    }
}

/// A tracking area over `view`'s visible rect for its enter and exit —
/// in an inactive window too, like the system's tabs.
fn track_hover(view: &NSView) {
    let options = NSTrackingAreaOptions::MouseEnteredAndExited
        | NSTrackingAreaOptions::ActiveInActiveApp
        | NSTrackingAreaOptions::InVisibleRect;
    // SAFETY: `owner` is the view, which owns the area and outlives it; no
    // user info. `InVisibleRect` follows the view's bounds, so the rect is
    // ignored and the area never needs replacing.
    let area = unsafe {
        NSTrackingArea::initWithRect_options_owner_userInfo(
            NSTrackingArea::alloc(),
            NSRect::ZERO,
            options,
            Some(view),
            None,
        )
    };
    view.addTrackingArea(&area);
}

/// A label that sits inside a bar view and never takes a click: the view
/// under it decides what a press means.
fn label(mtm: MainThreadMarker) -> Retained<NSTextField> {
    let text = NSTextField::labelWithString(ns_string!(""), mtm);
    text.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    text.setAlignment(NSTextAlignment::Center);
    text.setMaximumNumberOfLines(1);
    text
}

/// A rounded rect `rect` filled with `fill`, its inside edge stroked with
/// `line` if any.
fn rounded(rect: NSRect, radius: f64, fill: Tint, line: Option<Tint>) {
    let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect, radius, radius);
    fill.color().setFill();
    path.fill();
    if let Some(line) = line {
        let inner = NSRect::new(
            NSPoint::new(rect.origin.x + 0.5, rect.origin.y + 0.5),
            NSSize::new(rect.size.width - 1.0, rect.size.height - 1.0),
        );
        let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
            inner,
            radius - 0.5,
            radius - 0.5,
        );
        path.setLineWidth(1.0);
        line.color().setStroke();
        path.stroke();
    }
}

/// A glyph's stroked path in `color`, round caps and joins.
fn stroke(path: &NSBezierPath, width: f64, color: u32) {
    path.setLineWidth(width);
    path.setLineCapStyle(NSLineCapStyle::Round);
    path.setLineJoinStyle(NSLineJoinStyle::Round);
    Tint::of(color, 1.0).color().setStroke();
    path.stroke();
}

/// Whether `rect` holds `point` (the half-open rule AppKit's hit test uses).
fn contains(rect: NSRect, point: NSPoint) -> bool {
    point.x >= rect.origin.x
        && point.y >= rect.origin.y
        && point.x < rect.origin.x + rect.size.width
        && point.y < rect.origin.y + rect.size.height
}

/// The centre of `rect`.
fn centre(rect: NSRect) -> NSPoint {
    NSPoint::new(
        rect.origin.x + rect.size.width / 2.0,
        rect.origin.y + rect.size.height / 2.0,
    )
}

// ─── The `×` of a chip ────────────────────────────────────────────────────

pub(crate) struct CloseIvars {
    /// The pointer is over the chip: drawn and clickable.
    shown: Cell<bool>,
    /// The pointer is over the `×` itself: the stronger fill.
    hot: Cell<bool>,
    /// A press began here; the release inside closes.
    pressed: Cell<bool>,
    palette: Cell<Option<Palette>>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; CloseButton implements no
    // `Drop` and is born with `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriTabClose"]
    #[ivars = CloseIvars]
    pub(crate) struct CloseButton;

    unsafe impl NSObjectProtocol for CloseButton {}

    impl CloseButton {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let iv = self.ivars();
            let Some(palette) = iv.palette.get().filter(|_| iv.shown.get()) else {
                return;
            };
            let hot = iv.hot.get();
            let fill = if hot { palette.close_hover } else { palette.close };
            rounded(self.bounds(), CLOSE_RADIUS, fill, None);
            let at = centre(self.bounds());
            let path = NSBezierPath::bezierPath();
            path.moveToPoint(NSPoint::new(at.x - 3.0, at.y - 3.0));
            path.lineToPoint(NSPoint::new(at.x + 3.0, at.y + 3.0));
            path.moveToPoint(NSPoint::new(at.x + 3.0, at.y - 3.0));
            path.lineToPoint(NSPoint::new(at.x - 3.0, at.y + 3.0));
            stroke(&path, 1.5, if hot { palette.title } else { palette.dim });
        }

        /// Only while shown: a hidden `×` must not swallow the chip's press.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
            self.hit_test_local(point)
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            self.ivars().pressed.set(true);
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            let pressed = self.ivars().pressed.replace(false);
            let at = self.convertPoint_fromView(event.locationInWindow(), None);
            if pressed && contains(self.bounds(), at) {
                self.close();
            }
        }

        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            self.ivars().hot.set(true);
            self.setNeedsDisplay(true);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            self.ivars().hot.set(false);
            self.setNeedsDisplay(true);
        }

        #[unsafe(method(accessibilityPerformPress))]
        fn accessibility_press(&self) -> bool {
            self.close();
            true
        }
    }
);

impl CloseButton {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(CloseIvars {
            shown: Cell::new(false),
            hot: Cell::new(false),
            pressed: Cell::new(false),
            palette: Cell::new(None),
        });
        let frame = NSRect::new(
            NSPoint::new(CLOSE_INSET, CLOSE_INSET),
            NSSize::new(CLOSE_SIDE, CLOSE_SIDE),
        );
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        this.setAccessibilityElement(true);
        // SAFETY: AppKit's constant role string, alive for the process.
        this.setAccessibilityRole(Some(unsafe { NSAccessibilityButtonRole }));
        this.setAccessibilityLabel(Some(ns_string!("Close Tab")));
        this.setToolTip(Some(ns_string!("Close Tab  \u{2318}W")));
        track_hover(&this);
        this
    }

    fn set(&self, shown: bool, palette: Palette) {
        let iv = self.ivars();
        if iv.shown.replace(shown) != shown || iv.palette.replace(Some(palette)) != Some(palette) {
            self.setNeedsDisplay(true);
        }
    }

    /// Closes the chip's tab, through the chip.
    fn close(&self) {
        // SAFETY: reading the superview; we are on the main thread.
        if let Some(chip) =
            unsafe { self.superview() }.and_then(|view| view.downcast::<Chip>().ok())
        {
            chip.close_tab();
        }
    }
}

// ─── A chip: one tab ──────────────────────────────────────────────────────

pub(crate) struct ChipIvars {
    /// The tab's id (`TerminalTab::id`).
    tab: Cell<u64>,
    selected: Cell<bool>,
    hovered: Cell<bool>,
    /// The one-tab form: a centred title, no fill, no `×`, no click.
    single: Cell<bool>,
    palette: Cell<Option<Palette>>,
    /// The fill, below the label: its own view so the light theme's shadow
    /// falls from the face alone, not from the text.
    face: Retained<ChipFace>,
    label: Retained<NSTextField>,
    close: Retained<CloseButton>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; Chip implements no `Drop`
    // and is born with `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriTabChip"]
    #[ivars = ChipIvars]
    pub(crate) struct Chip;

    unsafe impl NSObjectProtocol for Chip {}

    impl Chip {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        /// The chip takes every press inside it — its label never does —
        /// except on a shown `×`. The single tab's title takes none: the row
        /// under it moves the window.
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

        /// A press selects — on the press, like the system's tabs.
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            self.select_tab();
        }

        /// The middle button closes the tab.
        #[unsafe(method(otherMouseDown:))]
        fn other_mouse_down(&self, event: &NSEvent) {
            if event.buttonNumber() == 2 {
                self.close_tab();
            }
        }

        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            if let Some(bar) = self.bar() {
                bar.hover(Some(self.ivars().tab.get()));
            }
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            if let Some(bar) = self.bar() {
                bar.unhover(self.ivars().tab.get());
            }
        }

        #[unsafe(method(accessibilityPerformPress))]
        fn accessibility_press(&self) -> bool {
            self.select_tab();
            true
        }
    }
);

impl CloseButton {
    /// `hitTest:`'s answer for a point in the chip's space (the `×`'s
    /// superview): itself while shown and under the point.
    fn hit_test_local(&self, point: NSPoint) -> Option<Retained<NSView>> {
        let inside = self.ivars().shown.get() && contains(self.frame(), point);
        inside.then(|| Retained::into_super(self.retain()))
    }
}

impl Chip {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let face = ChipFace::new(mtm);
        let text = label(mtm);
        let close = CloseButton::new(mtm);
        let this = Self::alloc(mtm).set_ivars(ChipIvars {
            tab: Cell::new(0),
            selected: Cell::new(false),
            hovered: Cell::new(false),
            single: Cell::new(false),
            palette: Cell::new(None),
            face: face.clone(),
            label: text.clone(),
            close: close.clone(),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.addSubview(&face);
        this.addSubview(&text);
        this.addSubview(&close);
        this.setAccessibilityElement(true);
        // SAFETY: AppKit's constant role strings, alive for the process.
        unsafe {
            this.setAccessibilityRole(Some(NSAccessibilityRadioButtonRole));
            this.setAccessibilitySubrole(Some(NSAccessibilityTabButtonSubrole));
        }
        // The label is read through the chip; its own element would say the
        // title twice.
        text.setAccessibilityElement(false);
        track_hover(&this);
        this
    }

    fn bar(&self) -> Option<Retained<TabBar>> {
        // SAFETY: reading the superview; we are on the main thread.
        unsafe { self.superview() }?.downcast::<TabBar>().ok()
    }

    /// `hitTest:`'s body: `point` is in the bar's space.
    fn hit(&self, point: NSPoint) -> Option<Retained<NSView>> {
        let iv = self.ivars();
        if iv.single.get() || self.isHidden() || !contains(self.frame(), point) {
            return None;
        }
        // SAFETY: reading the superview; we are on the main thread.
        let local = self.convertPoint_fromView(point, unsafe { self.superview() }.as_deref());
        iv.close
            .hit_test_local(local)
            .or_else(|| Some(Retained::into_super(self.retain())))
    }

    fn select_tab(&self) {
        if let Some(bar) = self.bar() {
            bar.select(self.ivars().tab.get());
        }
    }

    fn close_tab(&self) {
        if let Some(bar) = self.bar() {
            bar.close(self.ivars().tab.get());
        }
    }

    /// Places the chip at `frame` (the bar's space) and gives it its tab.
    fn set(&self, frame: NSRect, look: &Look<'_>, palette: Palette) {
        let Look {
            tab,
            title,
            selected,
            hovered,
            single,
        } = *look;
        let iv = self.ivars();
        iv.tab.set(tab);
        let changed = iv.selected.replace(selected) != selected
            || iv.hovered.replace(hovered) != hovered
            || iv.single.replace(single) != single
            || iv.palette.replace(Some(palette)) != Some(palette);
        self.setFrame(frame);
        let size = frame.size;
        iv.face.setFrame(NSRect::new(NSPoint::ZERO, size));
        let face = match (single, selected, hovered) {
            (true, ..) => Face::Bare,
            (false, true, _) => Face::Selected,
            (false, false, true) => Face::Hovered,
            (false, false, false) => Face::Bare,
        };
        iv.face.set(face, palette);
        let text = &iv.label;
        let (size_pt, weight, color, pad) = if single {
            // SAFETY: AppKit's constant font weight.
            (
                SINGLE_TEXT,
                unsafe { NSFontWeightSemibold },
                palette.dim,
                SINGLE_PAD,
            )
        } else if selected {
            // SAFETY: as above.
            (
                CHIP_TEXT,
                unsafe { NSFontWeightSemibold },
                palette.title,
                CHIP_PAD,
            )
        } else {
            let color = if hovered { palette.title } else { palette.dim };
            // SAFETY: as above.
            (CHIP_TEXT, unsafe { NSFontWeightMedium }, color, CHIP_PAD)
        };
        if changed || text.stringValue().to_string() != title {
            text.setFont(Some(&NSFont::systemFontOfSize_weight(size_pt, weight)));
            text.setTextColor(Some(&Tint::of(color, 1.0).color()));
            text.setStringValue(&NSString::from_str(title));
        }
        let height = text.intrinsicContentSize().height;
        text.setFrame(NSRect::new(
            NSPoint::new(pad, ((size.height - height) / 2.0).max(0.0)),
            NSSize::new((size.width - 2.0 * pad).max(0.0), height),
        ));
        iv.close.set(!single && hovered, palette);
        iv.close.setHidden(single);
        self.setAccessibilityLabel(Some(&NSString::from_str(title)));
        let value = NSNumber::numberWithBool(selected);
        // SAFETY: an `NSNumber` is a radio button's accessibility value.
        unsafe { self.setAccessibilityValue(Some(&value)) };
        self.setAccessibilityElement(!single);
    }
}

/// A chip's tab and state ([`Chip::set`]).
#[derive(Clone, Copy, Debug)]
struct Look<'a> {
    tab: u64,
    title: &'a str,
    selected: bool,
    hovered: bool,
    /// The one-tab form: a centred title, no fill, no `×`, no click.
    single: bool,
}

/// What a chip's face shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Face {
    Bare,
    Hovered,
    Selected,
}

pub(crate) struct FaceIvars {
    face: Cell<Face>,
    palette: Cell<Option<Palette>>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; ChipFace implements no
    // `Drop` and is born with `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriTabFace"]
    #[ivars = FaceIvars]
    pub(crate) struct ChipFace;

    unsafe impl NSObjectProtocol for ChipFace {}

    impl ChipFace {
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let Some(palette) = self.ivars().palette.get() else {
                return;
            };
            let bounds = self.bounds();
            match self.ivars().face.get() {
                Face::Bare => {}
                Face::Hovered => rounded(bounds, CHIP_RADIUS, palette.hover, None),
                Face::Selected => rounded(
                    bounds,
                    CHIP_RADIUS,
                    palette.selected,
                    Some(palette.selected_line),
                ),
            }
        }
    }
);

impl ChipFace {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(FaceIvars {
            face: Cell::new(Face::Bare),
            palette: Cell::new(None),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // ivars are set.
        unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] }
    }

    fn set(&self, face: Face, palette: Palette) {
        let iv = self.ivars();
        let changed =
            iv.face.replace(face) != face || iv.palette.replace(Some(palette)) != Some(palette);
        if !changed {
            return;
        }
        // The light theme's selected face lifts off the row with a thin
        // shadow; every other face lies flat.
        let shadow = palette
            .shadow
            .filter(|_| face == Face::Selected)
            .map(|tint| {
                let shadow = NSShadow::new();
                shadow.setShadowOffset(NSSize::new(0.0, -1.0));
                shadow.setShadowBlurRadius(2.0);
                shadow.setShadowColor(Some(&tint.color()));
                shadow
            });
        self.setShadow(shadow.as_deref());
        self.setNeedsDisplay(true);
    }
}

// ─── `+` and the settings warning ────────────────────────────────────────

/// What a bar button is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    NewTab,
    Warning,
}

pub(crate) struct ButtonIvars {
    kind: Kind,
    hot: Cell<bool>,
    pressed: Cell<bool>,
    palette: Cell<Option<Palette>>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; BarButton implements no
    // `Drop` and is born with `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriTabBarButton"]
    #[ivars = ButtonIvars]
    pub(crate) struct BarButton;

    unsafe impl NSObjectProtocol for BarButton {}

    impl BarButton {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let iv = self.ivars();
            let Some(palette) = iv.palette.get() else {
                return;
            };
            let hot = iv.hot.get();
            let fill = if hot { palette.button_hover } else { palette.button };
            rounded(self.bounds(), CHIP_RADIUS, fill, Some(palette.button_line));
            let at = centre(self.bounds());
            let path = NSBezierPath::bezierPath();
            match iv.kind {
                Kind::NewTab => {
                    path.moveToPoint(NSPoint::new(at.x, at.y - 4.5));
                    path.lineToPoint(NSPoint::new(at.x, at.y + 4.5));
                    path.moveToPoint(NSPoint::new(at.x - 4.5, at.y));
                    path.lineToPoint(NSPoint::new(at.x + 4.5, at.y));
                    stroke(&path, 1.6, if hot { palette.title } else { palette.dim });
                }
                Kind::Warning => {
                    // A triangle with an exclamation mark, in `warning`.
                    path.moveToPoint(NSPoint::new(at.x, at.y - 5.0));
                    path.lineToPoint(NSPoint::new(at.x + 5.5, at.y + 4.5));
                    path.lineToPoint(NSPoint::new(at.x - 5.5, at.y + 4.5));
                    path.closePath();
                    path.moveToPoint(NSPoint::new(at.x, at.y - 1.5));
                    path.lineToPoint(NSPoint::new(at.x, at.y + 1.0));
                    stroke(&path, 1.4, palette.warning);
                    let dot = NSBezierPath::bezierPathWithOvalInRect(NSRect::new(
                        NSPoint::new(at.x - 0.75, at.y + 2.4),
                        NSSize::new(1.5, 1.5),
                    ));
                    Tint::of(palette.warning, 1.0).color().setFill();
                    dot.fill();
                }
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
            self.ivars().pressed.set(true);
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            let pressed = self.ivars().pressed.replace(false);
            let at = self.convertPoint_fromView(event.locationInWindow(), None);
            if pressed && contains(self.bounds(), at) {
                self.act();
            }
        }

        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            self.ivars().hot.set(true);
            self.setNeedsDisplay(true);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            self.ivars().hot.set(false);
            self.setNeedsDisplay(true);
        }

        #[unsafe(method(accessibilityPerformPress))]
        fn accessibility_press(&self) -> bool {
            self.act();
            true
        }
    }
);

impl BarButton {
    fn new(mtm: MainThreadMarker, kind: Kind) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ButtonIvars {
            kind,
            hot: Cell::new(false),
            pressed: Cell::new(false),
            palette: Cell::new(None),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.setAccessibilityElement(true);
        // SAFETY: AppKit's constant role string, alive for the process.
        this.setAccessibilityRole(Some(unsafe { NSAccessibilityButtonRole }));
        if kind == Kind::NewTab {
            this.setAccessibilityLabel(Some(ns_string!("New Tab")));
            this.setToolTip(Some(ns_string!("New Tab  \u{2318}T")));
        }
        track_hover(&this);
        this
    }

    fn set_palette(&self, palette: Palette) {
        if self.ivars().palette.replace(Some(palette)) != Some(palette) {
            self.setNeedsDisplay(true);
        }
    }

    fn act(&self) {
        match self.ivars().kind {
            Kind::NewTab => {
                // SAFETY: reading the superview; we are on the main thread.
                let bar =
                    unsafe { self.superview() }.and_then(|view| view.downcast::<TabBar>().ok());
                if let Some(bar) = bar {
                    bar.new_tab();
                }
            }
            // The settings window shows the file's state; the target-less
            // action reaches the application delegate like the menu's.
            Kind::Warning => {
                let app = NSApplication::sharedApplication(self.mtm());
                let sender: &AnyObject = self;
                // SAFETY: `openSettings:` takes one optional sender argument.
                unsafe { app.sendAction_to_from(sel!(openSettings:), None, Some(sender)) };
            }
        }
    }
}

// ─── The bar ─────────────────────────────────────────────────────────────

/// What the bar shows; the window gives it ([`TabBar::show`]).
#[derive(Clone, Debug, Default)]
struct Shown {
    /// Every tab's id and label, in strip order.
    tabs: Vec<(u64, String)>,
    selected: usize,
    /// The settings diagnostic; empty without one.
    notice: String,
}

pub(crate) struct BarIvars {
    /// The window's id (`TerminalWindow::id`) — the way to its applier.
    window: u64,
    shown: RefCell<Shown>,
    /// The tab under the pointer.
    hovered: Cell<Option<u64>>,
    palette: Cell<Option<Palette>>,
    chips: RefCell<Vec<Retained<Chip>>>,
    new_tab: Retained<BarButton>,
    warning: Retained<BarButton>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; TabBar implements no `Drop`
    // and is born with `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriTabBar"]
    #[ivars = BarIvars]
    pub(crate) struct TabBar;

    unsafe impl NSObjectProtocol for TabBar {}

    impl TabBar {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        /// The first click on an inactive window moves it (or selects a
        /// chip), as on the system's title bar.
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        /// The bar decides itself ([`Self::mouse_down`]): AppKit's own
        /// move would also swallow the double click.
        #[unsafe(method(mouseDownCanMoveWindow))]
        fn can_move_window(&self) -> bool {
            false
        }

        /// A press on the row's empty part (and on the single tab's title):
        /// one click moves the window, a double click does what the
        /// system's setting says.
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let Some(window) = self.window() else {
                return;
            };
            if event.clickCount() < 2 {
                window.performWindowDragWithEvent(event);
                return;
            }
            let defaults = NSUserDefaults::standardUserDefaults();
            let action = defaults
                .stringForKey(ns_string!("AppleActionOnDoubleClick"))
                .map(|action| action.to_string());
            let legacy = defaults.boolForKey(ns_string!("AppleMiniaturizeOnDoubleClick"));
            match title_double_click(action.as_deref(), legacy) {
                TitleAction::Zoom => window.performZoom(None),
                TitleAction::Minimize => window.performMiniaturize(None),
                TitleAction::Nothing => {}
            }
        }
    }
);

impl TabBar {
    pub(crate) fn new(mtm: MainThreadMarker, window: u64) -> Retained<Self> {
        let new_tab = BarButton::new(mtm, Kind::NewTab);
        let warning = BarButton::new(mtm, Kind::Warning);
        let this = Self::alloc(mtm).set_ivars(BarIvars {
            window,
            shown: RefCell::new(Shown::default()),
            hovered: Cell::new(None),
            palette: Cell::new(None),
            chips: RefCell::new(Vec::new()),
            new_tab: new_tab.clone(),
            warning: warning.clone(),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.addSubview(&new_tab);
        this.addSubview(&warning);
        warning.setHidden(true);
        this.setAccessibilityElement(true);
        // SAFETY: AppKit's constant role string, alive for the process.
        this.setAccessibilityRole(Some(unsafe { NSAccessibilityTabGroupRole }));
        this.setAccessibilityLabel(Some(ns_string!("Tabs")));
        this
    }

    fn terminal_window(&self) -> Option<Retained<TerminalWindow>> {
        app::delegate(self.mtm())?.window(self.ivars().window)
    }

    /// What to show: the tabs' ids and labels in order, the selected one's
    /// position and the settings diagnostic (empty without one). Lays out.
    pub(crate) fn show(&self, tabs: Vec<(u64, String)>, selected: usize, notice: String) {
        self.ivars().shown.replace(Shown {
            tabs,
            selected,
            notice,
        });
        self.lay_out();
    }

    /// The settings diagnostic alone changed.
    pub(crate) fn set_notice(&self, notice: String) {
        self.ivars().shown.borrow_mut().notice = notice;
        self.lay_out();
    }

    /// The theme changed: every part repaints in its roles.
    pub(crate) fn set_theme(&self, theme: &Theme) {
        self.ivars().palette.set(Some(Palette::of(theme)));
        self.lay_out();
    }

    /// Where the strip may start: past the zoom button, or at the gap alone
    /// in full screen, where the lights live in another window.
    fn leading(&self) -> f64 {
        let Some(window) = self.window() else {
            return LIGHTS_GAP;
        };
        if window.styleMask().contains(NSWindowStyleMask::FullScreen) {
            return LIGHTS_GAP;
        }
        window
            .standardWindowButton(NSWindowButton::ZoomButton)
            .map_or(LIGHTS_GAP, |zoom| {
                let in_window = zoom.convertRect_toView(zoom.bounds(), None);
                let here = self.convertRect_fromView(in_window, None);
                here.origin.x + here.size.width + LIGHTS_GAP
            })
    }

    /// Places every part from the pure layout ([`Bar::layout`]) and gives
    /// the chips their tabs. Chips are made or taken apart only when the
    /// count changes.
    pub(crate) fn lay_out(&self) {
        let iv = self.ivars();
        let Some(palette) = iv.palette.get() else {
            return;
        };
        let shown = iv.shown.borrow().clone();
        let count = shown.tabs.len();
        if count == 0 {
            return;
        }
        let bounds = self.bounds();
        let hovered = iv
            .hovered
            .get()
            .and_then(|id| shown.tabs.iter().position(|(tab, _)| *tab == id));
        let single = count == 1;
        let strip = Bar {
            width: bounds.size.width,
            leading: self.leading(),
            count,
            selected: shown.selected,
            hovered,
            dragged: None,
            scroll: 0.0,
            warning: !shown.notice.is_empty(),
        }
        .layout();
        let top = ((bounds.size.height - CHIP_HEIGHT) / 2.0).max(0.0);
        {
            let mut chips = iv.chips.borrow_mut();
            while chips.len() > count {
                if let Some(chip) = chips.pop() {
                    chip.removeFromSuperview();
                }
            }
            while chips.len() < count {
                let chip = Chip::new(self.mtm());
                self.addSubview(&chip);
                chips.push(chip);
            }
        }
        let chips = iv.chips.borrow().clone();
        for (index, (chip, span)) in chips.iter().zip(&strip.chips).enumerate() {
            let (tab, title) = &shown.tabs[index];
            let title = if single {
                single_label(title, &shown.notice)
            } else {
                title.clone()
            };
            let frame = NSRect::new(
                NSPoint::new(span.x, top),
                NSSize::new(span.width, CHIP_HEIGHT),
            );
            let look = Look {
                tab: *tab,
                title: &title,
                selected: index == shown.selected,
                hovered: hovered == Some(index),
                single,
            };
            chip.set(frame, &look, palette);
        }
        let button = |x: f64| NSRect::new(NSPoint::new(x, top), NSSize::new(BUTTON, BUTTON));
        iv.new_tab.setFrame(button(strip.new_tab));
        iv.new_tab.set_palette(palette);
        match strip.warning {
            Some(x) => {
                iv.warning.setFrame(button(x));
                iv.warning.set_palette(palette);
                let text = NSString::from_str(&shown.notice);
                iv.warning.setToolTip(Some(&text));
                iv.warning.setAccessibilityLabel(Some(&text));
                iv.warning.setHidden(false);
            }
            None => iv.warning.setHidden(true),
        }
    }

    /// The pointer came over tab `tab`'s chip (`Some`), or left the bar.
    fn hover(&self, tab: Option<u64>) {
        if self.ivars().hovered.replace(tab) != tab {
            self.lay_out();
        }
    }

    /// The pointer left tab `tab`'s chip — unless it already entered
    /// another, whose enter can come first.
    fn unhover(&self, tab: u64) {
        if self.ivars().hovered.get() == Some(tab) {
            self.hover(None);
        }
    }

    /// A chip was pressed: the window's applier selects it.
    fn select(&self, tab: u64) {
        if let Some(window) = self.terminal_window() {
            window.select_tab(tab);
        }
    }

    /// A chip's `×` or middle click: the window asks and closes, a turn
    /// later (the module header).
    fn close(&self, tab: u64) {
        let window = self.ivars().window;
        DispatchQueue::main().exec_async(move || {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(window) = app::delegate(mtm).and_then(|app| app.window(window)) {
                window.close_tab_asking(tab);
            }
        });
    }

    /// `+`: ⌘T's job, in this window, a turn later.
    fn new_tab(&self) {
        let window = self.ivars().window;
        DispatchQueue::main().exec_async(move || {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(app) = app::delegate(mtm) {
                app.new_tab_in(window);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{Palette, TitleAction, single_label, title_double_click};
    use bt_core::Theme;

    /// The system's setting decides; the newer key wins over the older one,
    /// and absent both the row zooms.
    #[test]
    fn a_double_click_on_the_row_does_what_the_system_says() {
        assert_eq!(
            title_double_click(Some("Maximize"), false),
            TitleAction::Zoom
        );
        assert_eq!(title_double_click(Some("Fill"), true), TitleAction::Zoom);
        assert_eq!(
            title_double_click(Some("Minimize"), false),
            TitleAction::Minimize
        );
        assert_eq!(title_double_click(Some("None"), true), TitleAction::Nothing);
        assert_eq!(title_double_click(None, true), TitleAction::Minimize);
        assert_eq!(title_double_click(None, false), TitleAction::Zoom);
    }

    /// One tab keeps the diagnostic beside its title, the old subtitle's
    /// place; without one the title stands alone.
    #[test]
    fn the_single_title_carries_the_settings_diagnostic() {
        assert_eq!(single_label("bateri", ""), "bateri");
        assert_eq!(
            single_label("bateri", "settings.toml: line 2: bad"),
            "bateri \u{2013} settings.toml: line 2: bad"
        );
    }

    /// A dark theme lights the selected chip with the foreground; a light
    /// theme gives it a white face with a shadow instead. No role is new.
    #[test]
    fn the_selected_chip_follows_the_themes_lightness() {
        let dark = Palette::of(&Theme::BATERI);
        assert_eq!(dark.selected.rgb, Theme::BATERI.foreground);
        assert_eq!(dark.shadow, None);
        let light = Palette::of(&Theme::BATERI_LIGHT);
        assert_eq!(light.selected.rgb, 0xffffff);
        assert!(light.shadow.is_some());
        assert_eq!(light.title, Theme::BATERI_LIGHT.foreground);
        assert_eq!(light.warning, Theme::BATERI_LIGHT.warning);
    }
}
