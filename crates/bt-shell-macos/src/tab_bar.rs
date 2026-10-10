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
//! chip's span, the separators, `+` and the settings warning, and how `+`
//! meets the window's rounded corner — given here as measured, square in
//! full screen ([`TabBar::corner`]). With one tab the bar is today's title
//! bar — the title centred, a settings diagnostic beside it
//! ([`single_label`]), `+` on the right — and the title takes no click: a
//! press anywhere but `+` moves the window, a double click does what the
//! system says ([`title_double_click`]). With several tabs each tab is a
//! chip: a press selects it (on the press, like macOS's tabs), a middle
//! click closes it, a double click names it, a right click opens its menu,
//! and while the pointer is over it a `×` shows at its left. A diagnostic
//! becomes a `⚠` left of `+`: its tooltip is the text and a click opens the
//! settings window.
//!
//! **When the tabs do not fit** they stop at their narrowest, the strip
//! scrolls (a wheel or a trackpad, a vertical wheel too) and the chips run
//! under its edges, cut by the strip's own view ([`ChipStrip`]) and faded out
//! where more lies beyond ([`EdgeFade`]); Show All Tabs joins the buttons.
//! A **change of the selection** scrolls the strip to the selected chip, and
//! so does the tabs' ceasing to fit. The list is a menu under its button (or
//! where the button would be, from ⇧⌘\): every tab, the selected one ticked,
//! a dot in the colour of what it reports, its ⌘ key. Where a chip is and how
//! far the strip scrolls are `tabs`' arithmetic; this file places views at it.
//!
//! **A tab is named in place** (a double click, Rename Tab…, the chip's menu):
//! a text field over the chip's title, Return keeps the text, Esc leaves the
//! name, the keyboard going elsewhere keeps it. What the text means — empty
//! or the tab's own title is no name — is `tabs::custom_name`, the window's
//! applier writes it ([`TerminalWindow::rename_tab`]). The chip does not
//! draw its title while the field is up, so the layouts that come with every
//! tick and hover never overwrite what is being typed.
//!
//! **What a chip shows besides its title** (several tabs only — one tab is
//! today's title bar and shows none of it): left of the title one
//! indicator, the most urgent of the tab's signals (`tabs::indicator`) — a
//! question waiting, a running command's ring, a tick or a dot for a
//! command that ended while the tab was away, a transfer's arrow; a marked
//! host's colour as a line along its top; a transfer's progress as a line
//! along its bottom. Between two quiet tabs a separator. While ⌘ is held
//! ([`HINT_DELAY`]) each tab ⌘1…⌘9 reaches shows its key. Over a chip for
//! [`CARD_DELAY`] the tab's summary card opens below it (`tabs::card_lines`);
//! with a card open, the next chip's comes at once.
//!
//! **The clock.** A running ring steps once a second and the open card's
//! running time counts: one delayed wake per bar
//! ([`TabBar::arm_clock`]), set only while `tabs::Clock` says something on
//! a visible bar changes with time, at the duration counter's own next tick
//! (`bt_core::next_tick`). It is AppKit's redraw of a few views, never a
//! pane's frame: nothing here touches a pane's `Waker`. It stops when no
//! command runs, when the window is not visible and — for the rings, not
//! the card's seconds — under Reduce Motion, where the ring stands still.
//!
//! **A tab is dragged** by its chip. A press that moves past the slop along the
//! strip lifts the chip: it follows the pointer at once, the others slide to
//! make room (the same slide as any change of order — [`TabBar::lay_out`] lays
//! the tabs out in the preview order and the chip at the pointer), and the
//! window's list changes **once**, at the release, through its applier
//! ([`TerminalWindow::move_tab`]). The arithmetic — where the chip is, the
//! place it would take, when the pointer has left the bar — is `tabs::Grip`.
//! Out of the bar a drag session takes over (`tab_drag`): the chip stays,
//! faint, in its place; another window's bar opens a gap where the tab would
//! go ([`TabBar::open_gap`]) and takes the drop, anywhere else the tab becomes
//! a window of its own (`AppDelegate::tab_drag_ended`).
//!
//! **Motion** is AppKit's (`NSAnimationContext`, the design's curve): the
//! hover fill fades in 80 ms, `×` in 120 ms, chips slide when tabs come,
//! go or move, and a new one fades in, in 200 ms. Only the applier's
//! changes slide — a resize or a hover re-lays out at once. Under Reduce
//! Motion every duration is zero.
//!
//! **Drawing** is `drawRect:` with `NSBezierPath` and `NSShadow` — no layer
//! colours, which would want `CGColor` and with it an
//! `objc2-core-graphics` edge. The colours are the theme's own roles at
//! fixed strengths, so no theme role is added; the strengths are the
//! design's.
//!
//! **Who acts.** The bar knows its window by id and calls the window's one
//! applier (`TerminalWindow::select_tab`, `close_tab_asking`, `rename_tab`,
//! `move_tab`) —
//! the bar never changes the tab list itself. The chip's menu and the list act
//! through the window too: each item's target is the window, its `tag` the
//! tab it was opened for (`tabs::menu_tag`). A close or a new tab is done **one
//! main-queue turn later**: either rebuilds the chips, and the chip whose
//! event is being handled would be taken apart under it.
//!
//! **VoiceOver**: the bar is a tab group, each chip a radio button with the
//! tab-button subrole and its selection as its value; its label says the
//! title, the indicator and a marked host (`tabs::spoken`); `×` and `+`
//! buttons with names. A `×` that is not shown is still an element (drawn
//! invisible, taking no click), so a tab can be closed without a pointer.

use std::cell::{Cell, RefCell};
use std::ptr::NonNull;
use std::time::{Duration, Instant};

use block2::RcBlock;
use bt_core::{HostMark, Theme};
use dispatch2::{DispatchQueue, DispatchTime};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, ProtocolObject, Sel};
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel,
};
use objc2_app_kit::{
    NSAccessibility, NSAccessibilityButtonRole, NSAccessibilityRadioButtonRole,
    NSAccessibilityTabButtonSubrole, NSAccessibilityTabGroupRole, NSAnimatablePropertyContainer,
    NSAnimationContext, NSApplication, NSAutoresizingMaskOptions, NSBezierPath, NSColor, NSControl,
    NSControlStateValueOn, NSControlTextEditingDelegate, NSDragOperation, NSDraggingDestination,
    NSDraggingInfo, NSEvent, NSEventMask, NSEventModifierFlags, NSEventType, NSFocusRingType,
    NSFont, NSFontWeightMedium, NSFontWeightRegular, NSFontWeightSemibold, NSGradient, NSImage,
    NSLineBreakMode, NSLineCapStyle, NSLineJoinStyle, NSMenu, NSMenuItem, NSShadow,
    NSTextAlignment, NSTextField, NSTextFieldDelegate, NSTrackingArea, NSTrackingAreaOptions,
    NSView, NSWindowButton, NSWindowOrderingMode, NSWindowStyleMask,
};
use objc2_foundation::{
    NSArray, NSNotification, NSNumber, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
    NSUserDefaults, ns_string,
};
use objc2_quartz_core::CAMediaTimingFunction;

use crate::app;
use crate::tab_drag;
use crate::tabs::{
    self, BUTTON, Bar, Card, CardCommand, Clock, Drag, FADE, Grip, Indicator, Over,
    RING_STEP_DEGREES, Strip, TAB_RADIUS, Tone,
};
use crate::window::{TerminalWindow, is_dark_background};

/// Space between the zoom button's right edge and the strip — the design's
/// strip starts at 84 pt beside lights ending at 72 pt. In full screen the
/// lights are not in the row and the strip starts this far from the edge.
const LIGHTS_GAP: f64 = 12.0;

/// The radius of the window's top-right corner outside full screen, which
/// `+` is fitted to (`tabs::Fit`). AppKit does not say it; it was measured
/// on macOS 26.4.1 from the alpha of a screenshot of a window built like
/// ours (titled, content under the title row, an empty compact toolbar):
/// 20 pt — and the same zoomed and tiled to a half or to fill the screen.
/// Without the toolbar the corner is 16 pt. In full screen it is square
/// ([`TabBar::corner`]). Not measured: macOS 14 and 15, and a full-screen
/// split. A smaller corner than this lies outside this one, so where the
/// real corner is smaller the gap round `+` only widens.
const WINDOW_CORNER: f64 = 20.0;

/// A chip's height, points (the design's anatomy); its corner radius is
/// `tabs::TAB_RADIUS`, shared with the buttons on a square corner.
const CHIP_HEIGHT: f64 = 28.0;

/// The label's inset from a chip's sides: room for `×` on the left, the
/// same on the right so the title stays centred (and the ⌘ hint sits there).
const CHIP_PAD: f64 = 26.0;

/// The single tab's title box inset — it has no `×` to make room for.
const SINGLE_PAD: f64 = 8.0;

/// The indicator's side and the space between it and the title (the
/// design's 12 pt glyph, 6 pt gap).
const GLYPH_SIDE: f64 = 12.0;
const GLYPH_GAP: f64 = 6.0;

/// The `×` button: its side, its inset from the chip's corner and its
/// corner radius.
const CLOSE_SIDE: f64 = 20.0;
const CLOSE_INSET: f64 = 4.0;
const CLOSE_RADIUS: f64 = 5.0;

/// Type sizes: a chip's title, the single tab's slightly larger one, a ⌘
/// hint and the card's lines below its title.
const CHIP_TEXT: f64 = 12.5;
const SINGLE_TEXT: f64 = 13.0;
const HINT_TEXT: f64 = 11.0;
const CARD_TEXT: f64 = 11.5;

/// The host line along a chip's top and the transfer's along its bottom
/// (the design's 2 pt), and a separator's height between two quiet tabs.
const LINE: f64 = 2.0;
const SEPARATOR_HEIGHT: f64 = 16.0;

/// The design's timings: the hover fill, `×`, and a chip's slide or fade
/// when tabs come, go or move.
const HOVER_FADE: f64 = 0.08;
const CLOSE_FADE: f64 = 0.12;
const REFLOW: f64 = 0.2;

/// How much of a chip shows while its tab is carried away in a drag session: the place it
/// left stays, faint, so the strip does not close up under the pointer that may come back.
const GHOST_ALPHA: f64 = 0.35;

/// How long the pointer rests on a chip before its summary card opens (the
/// design's 450 ms), and how long after a card closed the next chip's opens
/// at once (the prototype's 300 ms "warm" window): moving along the strip
/// with a card open does not wait again.
const CARD_DELAY: Duration = Duration::from_millis(450);
const CARD_WARM: Duration = Duration::from_millis(300);

/// The summary card: its width (the design's), corner, inner margins, the
/// space between its lines and its gap below the chip.
const CARD_WIDTH: f64 = 272.0;
const CARD_RADIUS: f64 = 10.0;
const CARD_PAD_X: f64 = 12.0;
const CARD_PAD_Y: f64 = 10.0;
const CARD_LINE_GAP: f64 = 3.0;
const CARD_DROP: f64 = 8.0;

/// How far a notch of a mouse wheel scrolls the strip, points: a wheel reports
/// lines where a trackpad reports points.
const WHEEL_STEP: f64 = 12.0;

/// How long ⌘ is held alone before the tabs show their keys — a design
/// constant: ⌘ goes down before every shortcut (⌘C, ⌘V), and hints shown
/// at once would flash across the bar at each; a hand that holds ⌘ to read
/// them waits longer than this anyway.
const HINT_DELAY: Duration = Duration::from_millis(300);

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
/// not selected, `warning` for the settings warning, `accent`, `success`
/// and `error` for the indicators, the background for the card. The light
/// theme's selected chip is a white face with a thin shadow instead.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Palette {
    title: u32,
    dim: u32,
    /// The theme's background, which a scrolled strip's edges fade out to.
    ground: u32,
    warning: u32,
    accent: u32,
    success: u32,
    error: u32,
    selected: Tint,
    selected_line: Tint,
    /// The selected chip's drop shadow; `None` on a dark theme.
    shadow: Option<Tint>,
    /// A chip lifted by the pointer (the design's dragged tab): its face, its edge and the shadow
    /// it casts on the row.
    lifted: Tint,
    lifted_line: Tint,
    lift_shadow: Tint,
    hover: Tint,
    close: Tint,
    close_hover: Tint,
    button: Tint,
    button_line: Tint,
    button_hover: Tint,
    /// The line between two quiet tabs (the design's 13 %).
    separator: Tint,
    /// The ring's track under its turning arc, and the transfer line's
    /// track under its progress.
    track: Tint,
    /// The summary card's face, edge and shadow.
    card: Tint,
    card_line: Tint,
    card_shadow: Tint,
}

impl Palette {
    fn of(theme: &Theme) -> Self {
        let fg = theme.foreground;
        let ink = |alpha| Tint::of(fg, alpha);
        let base = Self {
            title: fg,
            dim: theme.dim,
            ground: theme.background,
            warning: theme.warning,
            accent: theme.accent,
            success: theme.success,
            error: theme.error,
            selected: ink(0.10),
            selected_line: ink(0.07),
            shadow: None,
            lifted: ink(0.14),
            lifted_line: ink(0.10),
            lift_shadow: Tint::of(0x000000, 0.6),
            hover: ink(0.05),
            close: ink(0.07),
            close_hover: ink(0.16),
            button: ink(0.06),
            button_line: ink(0.08),
            button_hover: ink(0.13),
            separator: ink(0.13),
            track: Tint::of(theme.accent, 0.25),
            card: Tint::of(theme.background, 1.0),
            card_line: ink(0.12),
            card_shadow: Tint::of(0x000000, 0.35),
        };
        if is_dark_background(theme) {
            base
        } else {
            Self {
                selected: Tint::of(0xffffff, 1.0),
                selected_line: ink(0.08),
                shadow: Some(ink(0.08)),
                lifted: Tint::of(0xffffff, 1.0),
                lifted_line: ink(0.08),
                lift_shadow: ink(0.18),
                close: ink(0.06),
                close_hover: ink(0.14),
                button: ink(0.05),
                button_hover: ink(0.11),
                card_shadow: Tint::of(0x000000, 0.14),
                ..base
            }
        }
    }

    /// A card line's colour: its tone's role, a marked host's own colour
    /// (`mark`, from `Theme::mark_rgb`) for the host line.
    fn tone(self, tone: Tone, mark: u32) -> u32 {
        match tone {
            Tone::Title => self.title,
            Tone::Dim => self.dim,
            Tone::Accent => self.accent,
            Tone::Success => self.success,
            Tone::Error => self.error,
            Tone::Mark => mark,
        }
    }
}

/// The durations of the bar's motions now — all zero under Reduce Motion
/// (`AppDelegate::reduce_motion`: the setting and the system's answer).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Motion {
    hover: f64,
    close: f64,
    reflow: f64,
}

impl Motion {
    fn of(reduce_motion: bool) -> Self {
        if reduce_motion {
            Self {
                hover: 0.0,
                close: 0.0,
                reflow: 0.0,
            }
        } else {
            Self {
                hover: HOVER_FADE,
                close: CLOSE_FADE,
                reflow: REFLOW,
            }
        }
    }
}

/// Runs `change` as an AppKit animation of `secs` on the design's curve
/// (`cubic-bezier(0.2, 0.8, 0.2, 1)`: quick out, slow to rest).
fn animate(secs: f64, change: impl Fn() + 'static) {
    let changes = RcBlock::new(move |context: NonNull<NSAnimationContext>| {
        // SAFETY: AppKit gives the block a live context, for the block's duration.
        let context = unsafe { context.as_ref() };
        context.setDuration(secs);
        let curve = CAMediaTimingFunction::functionWithControlPoints(0.2, 0.8, 0.2, 1.0);
        context.setTimingFunction(Some(&curve));
        change();
    });
    NSAnimationContext::runAnimationGroup(&changes);
}

/// Brings `view`'s opacity to `to`, over `secs` (at once for zero). A view
/// already at (or heading to) `to` is left alone, so the hover's re-lay-outs
/// do not restart a fade.
fn fade(view: &NSView, to: f64, secs: f64) {
    if view.alphaValue() == to {
        return;
    }
    if secs <= 0.0 {
        view.setAlphaValue(to);
        return;
    }
    let view = view.retain();
    animate(secs, move || view.animator().setAlphaValue(to));
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
fn stroke(path: &NSBezierPath, width: f64, color: Tint) {
    path.setLineWidth(width);
    path.setLineCapStyle(NSLineCapStyle::Round);
    path.setLineJoinStyle(NSLineJoinStyle::Round);
    color.color().setStroke();
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

/// Runs `job` on the main queue after `delay`, with the bar of window
/// `window` — found again by id, as the bar is not `Send` and may be gone
/// by then. `false` when `dispatch` did not take the job (a delay it cannot
/// represent).
fn after(delay: Duration, window: u64, job: impl FnOnce(&TabBar) + Send + 'static) -> bool {
    let Ok(when) = DispatchTime::try_from(delay) else {
        return false;
    };
    DispatchQueue::main()
        .after(when, move || {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(window) = app::delegate(mtm).and_then(|app| app.window(window)) {
                job(window.bar());
            }
        })
        .is_ok()
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

        /// Drawn always; whether it shows is its opacity, which fades
        /// ([`CloseButton::set`]).
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let iv = self.ivars();
            let Some(palette) = iv.palette.get() else {
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
            let color = if hot { palette.title } else { palette.dim };
            stroke(&path, 1.5, Tint::of(color, 1.0));
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
        this.setAlphaValue(0.0);
        this.setAccessibilityElement(true);
        // SAFETY: AppKit's constant role string, alive for the process.
        this.setAccessibilityRole(Some(unsafe { NSAccessibilityButtonRole }));
        this.setAccessibilityLabel(Some(ns_string!("Close Tab")));
        this.setToolTip(Some(ns_string!("Close Tab  \u{2318}W")));
        track_hover(&this);
        this
    }

    /// Shown (clickable, and faded in over `secs`) or not.
    fn set(&self, shown: bool, palette: Palette, secs: f64) {
        let iv = self.ivars();
        iv.shown.set(shown);
        if iv.palette.replace(Some(palette)) != Some(palette) {
            self.setNeedsDisplay(true);
        }
        fade(self, if shown { 1.0 } else { 0.0 }, secs);
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

// ─── A chip's indicator ───────────────────────────────────────────────────

pub(crate) struct GlyphIvars {
    shown: Cell<Option<Indicator>>,
    /// The running ring's step (`tabs::ring_step`).
    step: Cell<u8>,
    /// The transfer is a download: the arrow points down.
    down: Cell<bool>,
    palette: Cell<Option<Palette>>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; Glyph implements no `Drop`
    // and is born with `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriTabGlyph"]
    #[ivars = GlyphIvars]
    pub(crate) struct Glyph;

    unsafe impl NSObjectProtocol for Glyph {}

    impl Glyph {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        /// The chip under it takes the press.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }

        /// The glyph in a [`GLYPH_SIDE`] square, in its role.
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let iv = self.ivars();
            let (Some(indicator), Some(palette)) = (iv.shown.get(), iv.palette.get()) else {
                return;
            };
            // The role is `tabs`' one table, the Show All Tabs dots' too.
            let role = palette.tone(tabs::list_dot(indicator), 0);
            match indicator {
                Indicator::Question => draw_question(role),
                Indicator::Running => draw_ring(iv.step.get(), role, palette),
                Indicator::Finished => draw_tick(role),
                Indicator::Failed => draw_dot(role),
                Indicator::Uploading => draw_arrow(iv.down.get(), role),
            }
        }
    }
);

/// "Waiting for an answer": a ring with a question mark in it, in `accent`
/// — a mark of its own, not a role of its own.
fn draw_question(color: u32) {
    let ink = Tint::of(color, 1.0);
    let ring = NSBezierPath::bezierPathWithOvalInRect(NSRect::new(
        NSPoint::new(0.75, 0.75),
        NSSize::new(GLYPH_SIDE - 1.5, GLYPH_SIDE - 1.5),
    ));
    stroke(&ring, 1.3, ink);
    let mark = NSBezierPath::bezierPath();
    mark.moveToPoint(NSPoint::new(4.3, 4.6));
    mark.curveToPoint_controlPoint1_controlPoint2(
        NSPoint::new(7.7, 4.6),
        NSPoint::new(4.3, 2.5),
        NSPoint::new(7.7, 2.5),
    );
    mark.curveToPoint_controlPoint1_controlPoint2(
        NSPoint::new(6.0, 7.0),
        NSPoint::new(7.7, 5.8),
        NSPoint::new(6.0, 5.8),
    );
    stroke(&mark, 1.3, ink);
    let dot = NSBezierPath::bezierPathWithOvalInRect(NSRect::new(
        NSPoint::new(5.2, 8.05),
        NSSize::new(1.6, 1.6),
    ));
    ink.color().setFill();
    dot.fill();
}

/// "Running": a faint ring and a quarter arc in `accent`, turned
/// `tabs::RING_STEP_DEGREES` per step clockwise from the top.
fn draw_ring(step: u8, color: u32, palette: Palette) {
    let at = NSPoint::new(GLYPH_SIDE / 2.0, GLYPH_SIDE / 2.0);
    let radius = GLYPH_SIDE / 2.0 - 1.4;
    let track = NSBezierPath::bezierPathWithOvalInRect(NSRect::new(
        NSPoint::new(at.x - radius, at.y - radius),
        NSSize::new(2.0 * radius, 2.0 * radius),
    ));
    stroke(&track, 1.6, palette.track);
    // The view is flipped: angles grow clockwise on screen and −90° is the
    // top.
    let start = -90.0 + f64::from(step) * RING_STEP_DEGREES;
    let arc = NSBezierPath::bezierPath();
    arc.appendBezierPathWithArcWithCenter_radius_startAngle_endAngle_clockwise(
        at,
        radius,
        start,
        start + 90.0,
        false,
    );
    stroke(&arc, 1.6, Tint::of(color, 1.0));
}

/// "Finished while you were away": a tick in `success`.
fn draw_tick(color: u32) {
    let path = NSBezierPath::bezierPath();
    path.moveToPoint(NSPoint::new(2.6, 6.4));
    path.lineToPoint(NSPoint::new(5.0, 8.8));
    path.lineToPoint(NSPoint::new(9.4, 3.6));
    stroke(&path, 1.8, Tint::of(color, 1.0));
}

/// "Failed while you were away": a full dot in `error` — a shape apart from
/// the tick, so the two part without their colours too.
fn draw_dot(color: u32) {
    let dot = NSBezierPath::bezierPathWithOvalInRect(NSRect::new(
        NSPoint::new(2.5, 2.5),
        NSSize::new(GLYPH_SIDE - 5.0, GLYPH_SIDE - 5.0),
    ));
    Tint::of(color, 1.0).color().setFill();
    dot.fill();
}

/// "A transfer flows": an arrow in `accent`, up for an upload and down for
/// a download (the title prefix's `↑`/`↓`).
fn draw_arrow(down: bool, color: u32) {
    let (tip, tail, head) = if down {
        (9.8, 2.2, 7.0)
    } else {
        (2.2, 9.8, 5.0)
    };
    let path = NSBezierPath::bezierPath();
    path.moveToPoint(NSPoint::new(6.0, tail));
    path.lineToPoint(NSPoint::new(6.0, tip));
    path.moveToPoint(NSPoint::new(2.8, head));
    path.lineToPoint(NSPoint::new(6.0, tip));
    path.lineToPoint(NSPoint::new(9.2, head));
    stroke(&path, 1.6, Tint::of(color, 1.0));
}

impl Glyph {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(GlyphIvars {
            shown: Cell::new(None),
            step: Cell::new(0),
            down: Cell::new(false),
            palette: Cell::new(None),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        // Said through the chip, after its title.
        this.setAccessibilityElement(false);
        this
    }

    fn set(&self, shown: Option<Indicator>, step: u8, down: bool, palette: Palette) {
        let iv = self.ivars();
        let indicator = iv.shown.replace(shown) != shown;
        let turned = iv.step.replace(step) != step;
        let flipped = iv.down.replace(down) != down;
        let repainted = iv.palette.replace(Some(palette)) != Some(palette);
        if indicator || turned || flipped || repainted {
            self.setNeedsDisplay(true);
        }
        self.setHidden(shown.is_none());
    }
}

/// Where a chip's indicator and title sit across its `inner` width (the
/// chip less its padding on both sides), from the title's own width:
/// `(indicator's x, title's x, title's width)`, relative to the padding's
/// edge. With an indicator the two are centred together and the title
/// gives way to the glyph first; without one the title takes the whole
/// width (it is centred in its frame).
fn title_row(inner: f64, text: f64, glyph: bool) -> (Option<f64>, f64, f64) {
    if !glyph {
        return (None, 0.0, inner);
    }
    let room = (inner - GLYPH_SIDE - GLYPH_GAP).max(0.0);
    let width = text.ceil().min(room);
    let start = ((inner - (GLYPH_SIDE + GLYPH_GAP + width)) / 2.0)
        .max(0.0)
        .round();
    (Some(start), start + GLYPH_SIDE + GLYPH_GAP, width)
}

// ─── A chip's top and bottom lines ───────────────────────────────────────

/// What a chip's lines show ([`ChipLines`]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Lines {
    /// A marked host's colour along the top; `None` for an unmarked host.
    top: Option<u32>,
    /// A transfer's progress along the bottom, `0..=1`.
    progress: Option<f64>,
}

/// Where a 2 pt line runs along a `width` chip: inside its rounded corners.
fn line_span(width: f64) -> (f64, f64) {
    (TAB_RADIUS, (width - 2.0 * TAB_RADIUS).max(0.0))
}

pub(crate) struct LinesIvars {
    lines: Cell<Lines>,
    palette: Cell<Option<Palette>>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; ChipLines implements no
    // `Drop` and is born with `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriTabLines"]
    #[ivars = LinesIvars]
    pub(crate) struct ChipLines;

    unsafe impl NSObjectProtocol for ChipLines {}

    impl ChipLines {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }

        /// The host's line along the top, the transfer's track and progress
        /// along the bottom — above the chip's faces, so a light theme's
        /// white face does not cover them.
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let iv = self.ivars();
            let Some(palette) = iv.palette.get() else {
                return;
            };
            let Lines { top, progress } = iv.lines.get();
            let size = self.bounds().size;
            let (x, width) = line_span(size.width);
            let bar = |y: f64, width: f64, tint: Tint| {
                rounded(
                    NSRect::new(NSPoint::new(x, y), NSSize::new(width, LINE)),
                    LINE / 2.0,
                    tint,
                    None,
                );
            };
            if let Some(rgb) = top {
                bar(0.0, width, Tint::of(rgb, 1.0));
            }
            if let Some(progress) = progress {
                let y = size.height - LINE;
                bar(y, width, palette.track);
                bar(y, width * progress.clamp(0.0, 1.0), Tint::of(palette.accent, 1.0));
            }
        }
    }
);

impl ChipLines {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(LinesIvars {
            lines: Cell::new(Lines::default()),
            palette: Cell::new(None),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // ivars are set.
        unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] }
    }

    fn set(&self, lines: Lines, palette: Palette) {
        let iv = self.ivars();
        let changed =
            iv.lines.replace(lines) != lines || iv.palette.replace(Some(palette)) != Some(palette);
        if changed {
            self.setNeedsDisplay(true);
        }
    }
}

// ─── A chip: one tab ──────────────────────────────────────────────────────

pub(crate) struct ChipIvars {
    /// The tab's id (`TerminalTab::id`): the bar keeps a tab's chip across
    /// changes of the list, so a reflow can slide it.
    tab: Cell<u64>,
    selected: Cell<bool>,
    hovered: Cell<bool>,
    /// The one-tab form: a centred title, no fill, no `×`, no click.
    single: Cell<bool>,
    /// Born in this layout: it fades in rather than sliding from nowhere.
    fresh: Cell<bool>,
    /// The frame the bar last gave it ([`Chip::set`]).
    target: Cell<NSRect>,
    palette: Cell<Option<Palette>>,
    /// The hover fill, below everything: its own view, so it fades.
    hover: Retained<ChipFace>,
    /// The selected fill, below the label: its own view so the light
    /// theme's shadow falls from the face alone, not from the text.
    face: Retained<ChipFace>,
    /// The fill of a chip lifted by the pointer, in place of the selected one.
    lift: Retained<ChipFace>,
    /// Its tab is carried away: only a faint place is left ([`GHOST_ALPHA`]).
    ghost: Cell<bool>,
    /// The host's and the transfer's lines.
    lines: Retained<ChipLines>,
    /// The indicator, left of the title.
    glyph: Retained<Glyph>,
    label: Retained<NSTextField>,
    /// The ⌘ key that reaches the tab, right of the title while ⌘ is held.
    hint: Retained<NSTextField>,
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

        /// A press selects — on the press, like the system's tabs — and
        /// puts the summary card away. The second press of a double click
        /// names the tab; any other may become a drag.
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            if let Some(bar) = self.bar() {
                bar.pressed(self.ivars().tab.get());
            }
            let selected = self.select_tab();
            let Some(bar) = self.bar() else {
                return;
            };
            if event.clickCount() == 2 {
                bar.begin_rename(self.ivars().tab.get());
            } else if selected {
                bar.grip(self.ivars().tab.get(), event);
            }
        }

        /// The pointer moves with the button down: the tab follows it along the
        /// strip, or comes away from the bar ([`TabBar::drag`]).
        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            if let Some(bar) = self.bar() {
                bar.drag(self.ivars().tab.get(), event);
            }
        }

        /// The button is let go: a tab moved along the strip settles in its place.
        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            if let Some(bar) = self.bar() {
                bar.release(self.ivars().tab.get(), event);
            }
        }

        /// The chip's own menu, on a right click or a control click: what
        /// can be done to this tab, selected or not.
        #[unsafe(method_id(menuForEvent:))]
        fn menu_for_event(&self, _event: &NSEvent) -> Option<Retained<NSMenu>> {
            // No `?`: `define_class!` converts the body's last expression.
            self.bar()
                .and_then(|bar| bar.context_menu(self.ivars().tab.get()))
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
            let _ = self.select_tab();
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
    fn new(mtm: MainThreadMarker, tab: u64) -> Retained<Self> {
        let hover = ChipFace::new(mtm, Face::Hovered);
        let face = ChipFace::new(mtm, Face::Selected);
        let lift = ChipFace::new(mtm, Face::Lifted);
        let lines = ChipLines::new(mtm);
        let glyph = Glyph::new(mtm);
        let text = label(mtm);
        let hint = label(mtm);
        let close = CloseButton::new(mtm);
        let this = Self::alloc(mtm).set_ivars(ChipIvars {
            tab: Cell::new(tab),
            selected: Cell::new(false),
            hovered: Cell::new(false),
            single: Cell::new(false),
            fresh: Cell::new(true),
            target: Cell::new(NSRect::ZERO),
            palette: Cell::new(None),
            hover: hover.clone(),
            face: face.clone(),
            lift: lift.clone(),
            ghost: Cell::new(false),
            lines: lines.clone(),
            glyph: glyph.clone(),
            label: text.clone(),
            hint: hint.clone(),
            close: close.clone(),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        hover.setAlphaValue(0.0);
        lift.setHidden(true);
        let fill = NSAutoresizingMaskOptions::ViewWidthSizable
            | NSAutoresizingMaskOptions::ViewHeightSizable;
        for view in [&**hover as &NSView, &**face, &**lift, &**lines] {
            view.setAutoresizingMask(fill);
        }
        this.addSubview(&hover);
        this.addSubview(&face);
        this.addSubview(&lift);
        this.addSubview(&lines);
        this.addSubview(&glyph);
        this.addSubview(&text);
        this.addSubview(&hint);
        this.addSubview(&close);
        hint.setHidden(true);
        hint.setAccessibilityElement(false);
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

    fn tab(&self) -> u64 {
        self.ivars().tab.get()
    }

    /// The bar the chip is in: its strip's superview.
    fn bar(&self) -> Option<Retained<TabBar>> {
        // SAFETY: reading the superviews; we are on the main thread.
        unsafe { self.superview()?.superview() }?
            .downcast::<TabBar>()
            .ok()
    }

    /// `hitTest:`'s body: `point` is in the strip's space.
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

    /// Moves the chip to `x` in the strip's space and nothing else: the held chip following the
    /// pointer while the others stay where the last layout put them. The frame is recorded as the
    /// one asked for, so the next layout moves it from here.
    fn follow(&self, x: f64) {
        let mut frame = self.frame();
        frame.origin.x = x;
        self.ivars().target.set(frame);
        self.setFrame(frame);
    }

    /// Selects the chip's tab; `false` if the window would not (a question of its own is up).
    fn select_tab(&self) -> bool {
        self.bar()
            .is_some_and(|bar| bar.select(self.ivars().tab.get()))
    }

    fn close_tab(&self) {
        if let Some(bar) = self.bar() {
            bar.close(self.ivars().tab.get());
        }
    }

    /// Places the chip at `frame` (the bar's space) and gives it its tab's
    /// look. `slide` is the reflow's duration when the applier changed the
    /// list: an old chip slides to its place, a new one fades in; `None`
    /// puts it there at once.
    fn set(
        &self,
        frame: NSRect,
        look: &Look<'_>,
        palette: Palette,
        motion: Motion,
        slide: Option<f64>,
    ) {
        let Look {
            title,
            indicator,
            step,
            down,
            lines,
            hint,
            spoken,
            selected,
            hovered,
            single,
            lifted,
            ghost,
        } = look;
        let (selected, hovered, single) = (*selected, *hovered, *single);
        let (lifted, ghost) = (*lifted && !single, *ghost && !single);
        // The one-tab form is today's title bar: no indicator, no lines, no
        // hint.
        let indicator = indicator.filter(|_| !single);
        let lines = if single { Lines::default() } else { *lines };
        let hint = hint.as_deref().filter(|_| !single);
        let iv = self.ivars();
        let changed = iv.selected.replace(selected) != selected
            || iv.hovered.replace(hovered) != hovered
            || iv.single.replace(single) != single
            || iv.palette.replace(Some(palette)) != Some(palette);
        let fresh = iv.fresh.replace(false);
        // The frame last asked for: a slide in flight is heading there, and
        // a layout that asks for the same place (a clock tick, a hover, a
        // hint) leaves it to arrive rather than snapping it.
        let moved = iv.target.replace(frame) != frame;
        // Whether the chip was put at `frame` at once in this call: only then
        // are the faces and lines sized here. Otherwise they fill the chip
        // by their autoresizing — a sliding chip carries them, and sizing
        // them to the end of a slide in flight would add its rest twice.
        let placed = match slide {
            Some(secs) if fresh => {
                self.setFrame(frame);
                self.setAlphaValue(0.0);
                fade(self, 1.0, secs);
                true
            }
            Some(secs) if moved => {
                let this = self.retain();
                animate(secs, move || this.animator().setFrame(frame));
                false
            }
            Some(_) => false,
            None if moved => {
                // A slide still on its way (the chip was brought into view a
                // moment ago) would carry on to its old place and snap back.
                if let Some(layer) = self.layer() {
                    layer.removeAllAnimations();
                }
                self.setFrame(frame);
                true
            }
            None => false,
        };
        let size = frame.size;
        if placed {
            let whole = NSRect::new(NSPoint::ZERO, size);
            iv.hover.setFrame(whole);
            iv.face.setFrame(whole);
            iv.lift.setFrame(whole);
            iv.lines.setFrame(whole);
        }
        iv.hover.set(palette);
        fade(
            &iv.hover,
            f64::from(u8::from(hovered && !selected && !single)),
            motion.hover,
        );
        iv.face.set(palette);
        iv.face.setHidden(single || !selected || lifted);
        iv.lift.set(palette);
        iv.lift.setHidden(!lifted);
        if iv.ghost.replace(ghost) != ghost {
            self.setAlphaValue(if ghost { GHOST_ALPHA } else { 1.0 });
        }
        iv.lines.set(lines, palette);
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
        if changed || text.stringValue().to_string() != *title {
            text.setFont(Some(&NSFont::systemFontOfSize_weight(size_pt, weight)));
            text.setTextColor(Some(&Tint::of(color, 1.0).color()));
            text.setStringValue(&NSString::from_str(title));
        }
        // The frame the whole title draws in, not `intrinsicContentSize`:
        // that is the alignment rect, 2 pt in from each side, and a frame
        // that narrow truncates the text it was measured for to "…".
        let natural = text.fittingSize();
        let inner = (size.width - 2.0 * pad).max(0.0);
        let (glyph_x, text_x, text_width) = title_row(inner, natural.width, indicator.is_some());
        text.setFrame(NSRect::new(
            NSPoint::new(
                pad + text_x,
                ((size.height - natural.height) / 2.0).max(0.0),
            ),
            NSSize::new(text_width, natural.height),
        ));
        if let Some(x) = glyph_x {
            iv.glyph.setFrame(NSRect::new(
                NSPoint::new(pad + x, ((size.height - GLYPH_SIDE) / 2.0).round()),
                NSSize::new(GLYPH_SIDE, GLYPH_SIDE),
            ));
        }
        iv.glyph.set(indicator, *step, *down, palette);
        self.set_hint(hint, size, palette);
        iv.close.set(!single && hovered, palette, motion.close);
        iv.close.setHidden(single);
        self.setAccessibilityLabel(Some(&NSString::from_str(spoken)));
        let value = NSNumber::numberWithBool(selected);
        // SAFETY: an `NSNumber` is a radio button's accessibility value.
        unsafe { self.setAccessibilityValue(Some(&value)) };
        self.setAccessibilityElement(!single);
    }

    /// The ⌘ key that reaches the tab, right-aligned in the chip's right
    /// padding while ⌘ is held; hidden otherwise.
    fn set_hint(&self, hint: Option<&str>, size: NSSize, palette: Palette) {
        let field = &self.ivars().hint;
        let Some(hint) = hint else {
            field.setHidden(true);
            return;
        };
        if field.stringValue().to_string() != hint {
            // SAFETY: AppKit's constant font weight.
            let weight = unsafe { NSFontWeightMedium };
            field.setFont(Some(&NSFont::systemFontOfSize_weight(HINT_TEXT, weight)));
            field.setStringValue(&NSString::from_str(hint));
        }
        field.setTextColor(Some(&Tint::of(palette.dim, 1.0).color()));
        // The drawing frame, as the title's (an alignment-rect width cut
        // "⌘1" to "…").
        let natural = field.fittingSize();
        let width = natural.width.ceil().min(CHIP_PAD);
        field.setFrame(NSRect::new(
            NSPoint::new(
                (size.width - CLOSE_INSET - width).max(0.0),
                ((size.height - natural.height) / 2.0).max(0.0),
            ),
            NSSize::new(width, natural.height),
        ));
        field.setHidden(false);
    }
}

/// A chip's tab and state ([`Chip::set`]).
#[derive(Clone, Debug)]
struct Look<'a> {
    title: &'a str,
    indicator: Option<Indicator>,
    /// The running ring's step.
    step: u8,
    /// A transfer's arrow points down.
    down: bool,
    lines: Lines,
    /// The ⌘ key that reaches the tab, while ⌘ is held.
    hint: Option<String>,
    /// What VoiceOver says (`tabs::spoken`).
    spoken: String,
    selected: bool,
    hovered: bool,
    /// The one-tab form: a centred title, no fill, no `×`, no click.
    single: bool,
    /// Lifted by the pointer: the tab is being dragged along the strip.
    lifted: bool,
    /// Its tab is carried away from the strip in a drag session.
    ghost: bool,
}

/// Which fill a chip's face view draws: the hover's, which fades in and
/// out, the selected tab's, shown or hidden, or the dragged tab's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Face {
    Hovered,
    Selected,
    /// A chip lifted by the pointer: the fill of a tab being dragged.
    Lifted,
}

pub(crate) struct FaceIvars {
    face: Face,
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
            match self.ivars().face {
                Face::Hovered => rounded(bounds, TAB_RADIUS, palette.hover, None),
                Face::Selected => rounded(
                    bounds,
                    TAB_RADIUS,
                    palette.selected,
                    Some(palette.selected_line),
                ),
                Face::Lifted => rounded(
                    bounds,
                    TAB_RADIUS,
                    palette.lifted,
                    Some(palette.lifted_line),
                ),
            }
        }
    }
);

impl ChipFace {
    fn new(mtm: MainThreadMarker, face: Face) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(FaceIvars {
            face,
            palette: Cell::new(None),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // ivars are set.
        unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] }
    }

    fn set(&self, palette: Palette) {
        let iv = self.ivars();
        if iv.palette.replace(Some(palette)) == Some(palette) {
            return;
        }
        // The light theme's selected face lifts off the row with a thin
        // shadow, a dragged one with a wider, softer one (as much as the
        // strip's margin lets fall); every other face lies flat.
        let cast = |tint: Tint, blur: f64, drop: f64| {
            let shadow = NSShadow::new();
            shadow.setShadowOffset(NSSize::new(0.0, -drop));
            shadow.setShadowBlurRadius(blur);
            shadow.setShadowColor(Some(&tint.color()));
            shadow
        };
        let shadow = match iv.face {
            Face::Selected => palette.shadow.map(|tint| cast(tint, 2.0, 1.0)),
            Face::Lifted => Some(cast(palette.lift_shadow, 5.0, 2.0)),
            Face::Hovered => None,
        };
        self.setShadow(shadow.as_deref());
        self.setNeedsDisplay(true);
    }
}

// ─── The summary card ─────────────────────────────────────────────────────

pub(crate) struct CardIvars {
    labels: RefCell<Vec<Retained<NSTextField>>>,
    palette: Cell<Option<Palette>>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; SummaryCard implements no
    // `Drop` and is born with `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriTabCard"]
    #[ivars = CardIvars]
    pub(crate) struct SummaryCard;

    unsafe impl NSObjectProtocol for SummaryCard {}

    impl SummaryCard {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        /// It only tells: the pointer and its clicks go to what is under it.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            if let Some(palette) = self.ivars().palette.get() {
                rounded(self.bounds(), CARD_RADIUS, palette.card, Some(palette.card_line));
            }
        }
    }
);

impl SummaryCard {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(CardIvars {
            labels: RefCell::new(Vec::new()),
            palette: Cell::new(None),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.setAccessibilityElement(false);
        this
    }

    /// Writes `lines` (text and colour) top to bottom and sizes the card to
    /// them, [`CARD_WIDTH`] wide; returns its height.
    fn write(&self, lines: &[(String, u32)], palette: Palette) -> f64 {
        let iv = self.ivars();
        if iv.palette.replace(Some(palette)) != Some(palette) {
            let shadow = NSShadow::new();
            shadow.setShadowOffset(NSSize::new(0.0, -3.0));
            shadow.setShadowBlurRadius(12.0);
            shadow.setShadowColor(Some(&palette.card_shadow.color()));
            self.setShadow(Some(&shadow));
            self.setNeedsDisplay(true);
        }
        let mut labels = iv.labels.borrow_mut();
        while labels.len() > lines.len() {
            if let Some(gone) = labels.pop() {
                gone.removeFromSuperview();
            }
        }
        while labels.len() < lines.len() {
            let field = label(self.mtm());
            field.setAlignment(NSTextAlignment::Left);
            self.addSubview(&field);
            labels.push(field);
        }
        let mut y = CARD_PAD_Y;
        for (index, (field, (text, rgb))) in labels.iter().zip(lines).enumerate() {
            // SAFETY: AppKit's constant font weights.
            let (size, weight) = if index == 0 {
                (CHIP_TEXT, unsafe { NSFontWeightSemibold })
            } else {
                (CARD_TEXT, unsafe { NSFontWeightRegular })
            };
            field.setFont(Some(&NSFont::systemFontOfSize_weight(size, weight)));
            field.setTextColor(Some(&Tint::of(*rgb, 1.0).color()));
            field.setStringValue(&NSString::from_str(text));
            let height = field.intrinsicContentSize().height;
            field.setFrame(NSRect::new(
                NSPoint::new(CARD_PAD_X, y),
                NSSize::new(CARD_WIDTH - 2.0 * CARD_PAD_X, height),
            ));
            y += height + CARD_LINE_GAP;
        }
        y - CARD_LINE_GAP + CARD_PAD_Y
    }
}

/// Where the summary card of a chip spanning `chip_x`…`chip_x + chip_width`
/// sits in a `bar_width` bar: under the chip's left edge, kept
/// [`CARD_PAD_X`] inside both window edges.
fn card_x(chip_x: f64, bar_width: f64) -> f64 {
    chip_x
        .min(bar_width - CARD_WIDTH - CARD_PAD_X)
        .max(CARD_PAD_X)
}

// ─── `+` and the settings warning ────────────────────────────────────────

/// What a bar button is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    NewTab,
    /// Show All Tabs, only while the tabs do not fit.
    List,
    Warning,
}

pub(crate) struct ButtonIvars {
    kind: Kind,
    hot: Cell<bool>,
    pressed: Cell<bool>,
    palette: Cell<Option<Palette>>,
    /// The corner radius, from how the bar meets the window's corner.
    radius: Cell<f64>,
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
            rounded(self.bounds(), iv.radius.get(), fill, Some(palette.button_line));
            let at = centre(self.bounds());
            let path = NSBezierPath::bezierPath();
            match iv.kind {
                Kind::NewTab => {
                    path.moveToPoint(NSPoint::new(at.x, at.y - 4.5));
                    path.lineToPoint(NSPoint::new(at.x, at.y + 4.5));
                    path.moveToPoint(NSPoint::new(at.x - 4.5, at.y));
                    path.lineToPoint(NSPoint::new(at.x + 4.5, at.y));
                    let color = if hot { palette.title } else { palette.dim };
                    stroke(&path, 1.6, Tint::of(color, 1.0));
                }
                Kind::List => {
                    // A chevron pointing down: the list opens below.
                    path.moveToPoint(NSPoint::new(at.x - 4.5, at.y - 2.0));
                    path.lineToPoint(NSPoint::new(at.x, at.y + 2.5));
                    path.lineToPoint(NSPoint::new(at.x + 4.5, at.y - 2.0));
                    let color = if hot { palette.title } else { palette.dim };
                    stroke(&path, 1.6, Tint::of(color, 1.0));
                }
                Kind::Warning => {
                    // A triangle with an exclamation mark, in `warning`.
                    path.moveToPoint(NSPoint::new(at.x, at.y - 5.0));
                    path.lineToPoint(NSPoint::new(at.x + 5.5, at.y + 4.5));
                    path.lineToPoint(NSPoint::new(at.x - 5.5, at.y + 4.5));
                    path.closePath();
                    path.moveToPoint(NSPoint::new(at.x, at.y - 1.5));
                    path.lineToPoint(NSPoint::new(at.x, at.y + 1.0));
                    stroke(&path, 1.4, Tint::of(palette.warning, 1.0));
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
            radius: Cell::new(TAB_RADIUS),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.setAccessibilityElement(true);
        // SAFETY: AppKit's constant role string, alive for the process.
        this.setAccessibilityRole(Some(unsafe { NSAccessibilityButtonRole }));
        match kind {
            Kind::NewTab => {
                this.setAccessibilityLabel(Some(ns_string!("New Tab")));
                this.setToolTip(Some(ns_string!("New Tab  \u{2318}T")));
            }
            Kind::List => {
                this.setAccessibilityLabel(Some(ns_string!("Show All Tabs")));
                this.setToolTip(Some(ns_string!("Show All Tabs  \u{21e7}\u{2318}\\")));
            }
            Kind::Warning => {}
        }
        track_hover(&this);
        this
    }

    fn set_look(&self, palette: Palette, radius: f64) {
        let iv = self.ivars();
        let repaint = iv.palette.replace(Some(palette)) != Some(palette);
        if iv.radius.replace(radius) != radius || repaint {
            self.setNeedsDisplay(true);
        }
    }

    fn act(&self) {
        // SAFETY: reading the superview; we are on the main thread.
        let bar = || unsafe { self.superview() }.and_then(|view| view.downcast::<TabBar>().ok());
        match self.ivars().kind {
            Kind::NewTab => {
                if let Some(bar) = bar() {
                    bar.new_tab();
                }
            }
            Kind::List => {
                if let Some(bar) = bar() {
                    bar.show_list();
                }
                // The menu's tracking swallowed the pointer's exit.
                self.ivars().hot.set(false);
                self.setNeedsDisplay(true);
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

/// A menu item that acts through `window`'s applier: `action` is one of the
/// window's tab selectors, `tag` names the tab ([`tabs::menu_tag`]).
fn window_item(
    mtm: MainThreadMarker,
    window: &TerminalWindow,
    title: &str,
    action: Sel,
    tag: isize,
) -> Retained<NSMenuItem> {
    let item = NSMenuItem::new(mtm);
    item.setTitle(&NSString::from_str(title));
    item.setTag(tag);
    let target: &AnyObject = window;
    // SAFETY: every `action` passed is defined by `TerminalWindow` as a
    // no-return action with a single `Option<&AnyObject>` argument; the
    // item holds its target weakly and the application's window list owns it.
    unsafe {
        item.setAction(Some(action));
        item.setTarget(Some(target));
    }
    item
}

/// A status dot for a menu row, a few points across, in `rgb`: the colour of
/// what the tab reports, as the chip's own indicator wears it. `None` is a
/// clear image of the same size.
fn dot_image(rgb: Option<u32>) -> Retained<NSImage> {
    let draw = RcBlock::new(move |rect: NSRect| -> Bool {
        if let Some(rgb) = rgb {
            Tint::of(rgb, 1.0).color().setFill();
            NSBezierPath::bezierPathWithOvalInRect(rect).fill();
        }
        Bool::YES
    });
    NSImage::imageWithSize_flipped_drawingHandler(NSSize::new(8.0, 8.0), false, &draw)
}

// ─── The strip the chips scroll in ───────────────────────────────────────

pub(crate) struct StripIvars {
    palette: Cell<Option<Palette>>,
    /// The separators' centres, in the strip's own space.
    separators: RefCell<Vec<f64>>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; ChipStrip implements no
    // `Drop` and is born with `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriTabStrip"]
    #[ivars = StripIvars]
    pub(crate) struct ChipStrip;

    unsafe impl NSObjectProtocol for ChipStrip {}

    impl ChipStrip {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        /// The separators between two quiet tabs; the chips draw over the
        /// rest. Drawn here, not in the bar, so the strip's clip cuts a
        /// separator of a scrolled-away tab too.
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let iv = self.ivars();
            let Some(palette) = iv.palette.get() else {
                return;
            };
            let height = self.bounds().size.height;
            let y = ((height - SEPARATOR_HEIGHT) / 2.0).max(0.0);
            palette.separator.color().setFill();
            for &x in iv.separators.borrow().iter() {
                let line = NSBezierPath::bezierPathWithRect(NSRect::new(
                    NSPoint::new(x - 0.5, y),
                    NSSize::new(1.0, SEPARATOR_HEIGHT.min(height)),
                ));
                line.fill();
            }
        }

        /// Like the bar under it: the first click on an inactive window
        /// moves it (or selects a chip).
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        /// A press on the strip's empty part goes on to the bar, which moves
        /// the window itself ([`TabBar`]'s `mouseDown:`).
        #[unsafe(method(mouseDownCanMoveWindow))]
        fn can_move_window(&self) -> bool {
            false
        }
    }
);

impl ChipStrip {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(StripIvars {
            palette: Cell::new(None),
            separators: RefCell::new(Vec::new()),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        // The chips of a scrolled strip run under its edges and are cut
        // there, not drawn over the traffic lights or the buttons.
        this.setClipsToBounds(true);
        this
    }

    /// The separators (centres in the strip's space) and the colours they
    /// are drawn in.
    fn set_separators(&self, separators: Vec<f64>, palette: Palette) {
        let iv = self.ivars();
        let repaint = iv.palette.replace(Some(palette)) != Some(palette)
            || *iv.separators.borrow() != separators;
        if repaint {
            iv.separators.replace(separators);
            self.setNeedsDisplay(true);
        }
    }
}

// ─── The fade at a scrolled strip's edge ─────────────────────────────────

pub(crate) struct FadeIvars {
    /// The strip's leading edge: opaque on the left. Otherwise the trailing
    /// edge, opaque on the right.
    leading: bool,
    palette: Cell<Option<Palette>>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; EdgeFade implements no
    // `Drop` and is born with `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriTabFade"]
    #[ivars = FadeIvars]
    pub(crate) struct EdgeFade;

    unsafe impl NSObjectProtocol for EdgeFade {}

    impl EdgeFade {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        /// Takes no click: the chips under it do.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }

        /// The theme's background, running out to nothing towards the strip's
        /// middle: what runs under the edge dissolves into the window.
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let iv = self.ivars();
            let Some(palette) = iv.palette.get() else {
                return;
            };
            let solid = Tint::of(palette.ground, 1.0).color();
            let clear = Tint::of(palette.ground, 0.0).color();
            let (from, to) = if iv.leading {
                (&solid, &clear)
            } else {
                (&clear, &solid)
            };
            let gradient = NSGradient::initWithStartingColor_endingColor(NSGradient::alloc(), from, to);
            if let Some(gradient) = gradient {
                gradient.drawInRect_angle(self.bounds(), 0.0);
            }
        }
    }
);

impl EdgeFade {
    fn new(mtm: MainThreadMarker, leading: bool) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(FadeIvars {
            leading,
            palette: Cell::new(None),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.setHidden(true);
        this.setAccessibilityElement(false);
        this
    }

    fn set_palette(&self, palette: Palette) {
        if self.ivars().palette.replace(Some(palette)) != Some(palette) {
            self.setNeedsDisplay(true);
        }
    }
}

// ─── The bar's claim on the title row ────────────────────────────────────

define_class!(
    /// An empty control the bar's size, under everything in it, taking no click: it keeps the
    /// title row from the window server, so a press in the bar is the bar's to decide.
    ///
    /// **Why.** The window's content reaches under the title row (`FullSizeContentView`), and
    /// there the window server moves the window itself, before any view sees the drag. It moves
    /// it from the whole row less what the views in it claim for themselves — AppKit asks each
    /// visible view (`_opaqueRectForWindowMoveWhenInTitlebar`, a private method) — and a plain
    /// `NSView` claims nothing; its `mouseDownCanMoveWindow` returning `NO` does not count
    /// there. So a press on a chip reached the chip, but the drag that followed moved the
    /// window: a tab could not be moved along the strip, torn off or dropped on another
    /// window's bar. An enabled `NSControl` that accepts the keyboard claims its bounds —
    /// measured on macOS 26.4.1: `NSView` claims nothing, `NSControl`, `NSButton` and
    /// `NSTextField` their bounds, and a control disabled or refusing the keyboard nothing.
    /// This one claims the whole bar, and every press goes on to the bar as
    /// before ([`TabBar`]'s `mouseDown:`): a chip's drag is the tab's, the empty part's moves
    /// the window (`performWindowDragWithEvent:`), a double click does the system's setting.
    ///
    /// **Not** `NSWindow.isMovable = NO`, which also stops the server: the system then greys out
    /// Window ▸ Move & Resize, taking a window it believes cannot move. **Not** the bar itself
    /// as a control: `NSControl` changes how a view takes the mouse and the keyboard; this one
    /// only claims, its `hitTest:` is `nil`, so neither a click nor the keyboard reaches it.
    ///
    /// Should a macOS stop honouring the claim, the tabs move the window again and no test sees
    /// it: a synthetic event never reaches the window server's drag, only a real pointer does.
    // SAFETY: NSControl is designed for subclassing; RowClaim implements no
    // `Drop`, has no ivar and is born with `initWithFrame:`.
    #[unsafe(super(NSControl))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriTabRowClaim"]
    pub(crate) struct RowClaim;

    unsafe impl NSObjectProtocol for RowClaim {}

    impl RowClaim {
        /// Takes no click: the bar and what is in it do.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }
    }
);

impl RowClaim {
    /// The claim, following the size of the view it is put in.
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: `initWithFrame:` is NSView's designated initializer; the
        // subclass has no ivar.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        // It still accepts the keyboard, as a bare control does: the claim
        // holds only while it does (measured — refusing it, by the flag or by
        // `acceptsFirstResponder`, drops the claim to nothing). The keyboard
        // never comes: no click reaches it and it is no key view.
        this.setAccessibilityElement(false);
        this
    }
}

// ─── The bar ─────────────────────────────────────────────────────────────

/// A transfer flowing in a tab ([`Label::upload`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Upload {
    /// Only downloads flow: the arrow points down (the title prefix's `↓`).
    pub(crate) down: bool,
    /// The title prefix's percentage — what VoiceOver says.
    pub(crate) percent: u8,
    /// How far all of the tab's transfers are, `0..=1` — the underline.
    pub(crate) fraction: f64,
}

/// One tab as the bar shows it ([`TabBar::show`]).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Label {
    /// The tab's id (`TerminalTab::id`).
    pub(crate) tab: u64,
    pub(crate) title: String,
    /// What its chip shows left of the title (`TerminalTab::indicator`).
    pub(crate) indicator: Option<Indicator>,
    /// How long its running command has run (`TerminalTab::running_for`):
    /// the ring's step and the bar's clock.
    pub(crate) running: Option<Duration>,
    /// A transfer flowing in it (`TerminalTab::upload`).
    pub(crate) upload: Option<Upload>,
    /// Its focused pane's marked host (`TerminalTab::host_mark`).
    pub(crate) mark: HostMark,
}

/// What the bar shows; the window gives it ([`TabBar::show`]).
#[derive(Clone, Debug, Default)]
struct Shown {
    /// Every tab, in strip order.
    tabs: Vec<Label>,
    selected: usize,
    /// The settings diagnostic; empty without one.
    notice: String,
}

/// The open summary card: whose, and what it tells (`TerminalTab::card`,
/// read again when what the tab's chip shows changes; between two reads its
/// running time moves by the time since the read — the command's own clock
/// is monotonic, so no pane is read for it).
#[derive(Clone, Debug)]
struct OpenCard {
    tab: u64,
    card: Card,
    read_at: Instant,
}

impl OpenCard {
    /// How long the card's command has run now; `None` unless it runs.
    fn running(&self) -> Option<Duration> {
        match &self.card.command {
            Some(CardCommand::Running { elapsed, .. }) => {
                Some(elapsed.saturating_add(self.read_at.elapsed()))
            }
            _ => None,
        }
    }

    /// The card as it reads now: its running time moved on.
    fn now(&self) -> Card {
        let mut card = self.card.clone();
        if let (Some(CardCommand::Running { elapsed, .. }), Some(now)) =
            (&mut card.command, self.running())
        {
            *elapsed = now;
        }
        card
    }
}

/// A press on a chip that may be, or has become, a drag ([`TabBar::grip`]).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Dragging {
    tab: u64,
    grip: Grip,
    /// Once the tab moves along the strip: where its chip is drawn (the bar's space) and the
    /// place it would take if let go now. The other chips lay out around that place.
    at: Option<(f64, usize)>,
    /// The tab came away from the bar: a drag session carries it and its chip waits, faint,
    /// in the place it left until the session ends ([`TabBar::drag_ended`]).
    torn: bool,
}

pub(crate) struct BarIvars {
    /// The window's id (`TerminalWindow::id`) — the way to its applier.
    window: u64,
    shown: RefCell<Shown>,
    /// The tab under the pointer.
    hovered: Cell<Option<u64>>,
    /// The press on a chip that is, or may become, a drag.
    drag: Cell<Option<Dragging>>,
    /// A tab carried over this bar from another window: the place it would be
    /// inserted at, where the chips open a gap ([`TabBar::open_gap`]).
    gap: Cell<Option<usize>>,
    /// The next layout slides every chip to its place, though the order is the
    /// same: a drag ended, or a gap opened or closed.
    glide: Cell<bool>,
    palette: Cell<Option<Palette>>,
    /// The theme the palette came from — a host mark's colour is its
    /// mapping (`Theme::mark_rgb`), the dock's.
    theme: Cell<Option<Theme>>,
    /// One chip per tab, in strip order, each keeping its tab.
    chips: RefCell<Vec<Retained<Chip>>>,
    /// The view the chips sit in and are cut by.
    strip: Retained<ChipStrip>,
    /// The fades at the strip's two edges, lit where tabs run under them.
    fade_leading: Retained<EdgeFade>,
    fade_trailing: Retained<EdgeFade>,
    /// The strip's scroll as last applied (the layout clamps it).
    scroll: Cell<f64>,
    /// The layout last made: the wheel and the list work from it.
    laid: RefCell<Option<Strip>>,
    /// The tab whose selection was last brought into view — a layout scrolls
    /// to the selection only when it changed (or when the tabs stopped
    /// fitting), so a hover or a clock tick never undoes the user's
    /// scrolling.
    revealed: Cell<Option<u64>>,
    /// The tabs did not fit at the last layout.
    overflowed: Cell<bool>,
    new_tab: Retained<BarButton>,
    /// Show All Tabs, only while the tabs do not fit.
    list: Retained<BarButton>,
    warning: Retained<BarButton>,
    /// The field a tab is named in, in the bar while `renaming` says whose.
    field: Retained<NSTextField>,
    renaming: Cell<Option<u64>>,
    /// What the field held when it opened: unchanged text is not a name
    /// (the title it showed may be stale by the time the field closes).
    opened_with: RefCell<String>,
    /// The bar's width at the last layout: narrowing a window with
    /// overflowing tabs brings the selected one back into view.
    width: Cell<f64>,
    /// The bar's one delayed wake ([`TabBar::arm_clock`]): the generation a
    /// fire must carry to count, and when the live one is due.
    clock: Cell<u64>,
    clock_due: Cell<Option<Instant>>,
    card_view: Retained<SummaryCard>,
    card: RefCell<Option<OpenCard>>,
    /// A card's pending opening — a hover change makes an older one stale.
    card_wait: Cell<u64>,
    /// When the last card closed: the warm window ([`CARD_WARM`]).
    card_closed: Cell<Option<Instant>>,
    /// The tab a press put its card away for, until the pointer leaves it.
    card_quiet: Cell<Option<u64>>,
    /// ⌘ is held (past [`HINT_DELAY`]): the tabs show their keys.
    hints: Cell<bool>,
    /// A pending hint — ⌘'s release or a chord makes it stale.
    hint_wait: Cell<u64>,
    /// A show is pending ([`HINT_DELAY`]).
    hint_pending: Cell<bool>,
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

        /// A wheel or a trackpad moves the strip once the tabs no longer fit
        /// ([`Strip::wheeled`]: a vertical wheel scrolls it too). Without
        /// overflow nothing scrolls and the event goes on.
        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, event: &NSEvent) {
            if !self.wheel(event) {
                // SAFETY: `NSView`'s `scrollWheel:` takes the event.
                let _: () = unsafe { msg_send![super(self), scrollWheel: event] };
            }
        }

        /// The rename field's command hook: Return keeps the name, Esc
        /// leaves the tab's as it was. The rest is the field's own.
        #[unsafe(method(control:textView:doCommandBySelector:))]
        fn control_do_command(
            &self,
            _control: &AnyObject,
            _text_view: &AnyObject,
            command: Sel,
        ) -> bool {
            if command == sel!(insertNewline:) {
                self.end_rename(true);
                true
            } else if command == sel!(cancelOperation:) {
                self.end_rename(false);
                true
            } else {
                false
            }
        }

        /// The field lost the keyboard to something else — a click in a
        /// pane, another tab: the name typed is kept.
        #[unsafe(method(controlTextDidEndEditing:))]
        fn control_end_editing(&self, _notification: &NSNotification) {
            self.end_rename(true);
        }
    }

    // The rename field's delegate: both protocols' methods are optional;
    // the ones used are above.
    unsafe impl NSControlTextEditingDelegate for TabBar {}
    unsafe impl NSTextFieldDelegate for TabBar {}

    /// The side of a tab dropped on this bar from a drag session ([`tab_drag`]): a tab of this
    /// process only ([`tab_drag::carried`]). The pointer over the bar opens a gap in the strip
    /// at the place the tab would take; letting go records the landing, which is carried out
    /// when the session has ended (`AppDelegate::tab_drag_ended`).
    unsafe impl NSDraggingDestination for TabBar {
        #[unsafe(method(draggingEntered:))]
        fn dragging_entered(
            &self,
            info: &ProtocolObject<dyn NSDraggingInfo>,
        ) -> NSDragOperation {
            self.carried_over(info)
        }

        #[unsafe(method(draggingUpdated:))]
        fn dragging_updated(
            &self,
            info: &ProtocolObject<dyn NSDraggingInfo>,
        ) -> NSDragOperation {
            self.carried_over(info)
        }

        /// The tab went away from this bar (or was taken back): the gap closes.
        #[unsafe(method(draggingExited:))]
        fn dragging_exited(&self, _info: Option<&ProtocolObject<dyn NSDraggingInfo>>) {
            self.open_gap(None);
        }

        #[unsafe(method(performDragOperation:))]
        fn perform_drag(&self, info: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
            self.accept_drop(info)
        }
    }
);

impl TabBar {
    pub(crate) fn new(mtm: MainThreadMarker, window: u64) -> Retained<Self> {
        let new_tab = BarButton::new(mtm, Kind::NewTab);
        let list = BarButton::new(mtm, Kind::List);
        let warning = BarButton::new(mtm, Kind::Warning);
        let strip = ChipStrip::new(mtm);
        let fade_leading = EdgeFade::new(mtm, true);
        let fade_trailing = EdgeFade::new(mtm, false);
        let field = NSTextField::textFieldWithString(ns_string!(""), mtm);
        // A tab's name is one line that scrolls as it is typed, on the chip's
        // own face: no bezel, no ring, no ground of its own.
        field.setBezeled(false);
        field.setDrawsBackground(false);
        field.setFocusRingType(NSFocusRingType::None);
        field.setAlignment(NSTextAlignment::Center);
        if let Some(cell) = field.cell() {
            cell.setUsesSingleLineMode(true);
            cell.setScrollable(true);
        }
        field.setAccessibilityLabel(Some(ns_string!("Tab name")));
        let this = Self::alloc(mtm).set_ivars(BarIvars {
            window,
            shown: RefCell::new(Shown::default()),
            hovered: Cell::new(None),
            drag: Cell::new(None),
            gap: Cell::new(None),
            glide: Cell::new(false),
            palette: Cell::new(None),
            theme: Cell::new(None),
            chips: RefCell::new(Vec::new()),
            strip: strip.clone(),
            fade_leading: fade_leading.clone(),
            fade_trailing: fade_trailing.clone(),
            scroll: Cell::new(0.0),
            laid: RefCell::new(None),
            revealed: Cell::new(None),
            overflowed: Cell::new(false),
            new_tab: new_tab.clone(),
            list: list.clone(),
            warning: warning.clone(),
            field: field.clone(),
            renaming: Cell::new(None),
            opened_with: RefCell::new(String::new()),
            width: Cell::new(0.0),
            clock: Cell::new(0),
            clock_due: Cell::new(None),
            card_view: SummaryCard::new(mtm),
            card: RefCell::new(None),
            card_wait: Cell::new(0),
            card_closed: Cell::new(None),
            card_quiet: Cell::new(None),
            hints: Cell::new(false),
            hint_wait: Cell::new(0),
            hint_pending: Cell::new(false),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        // Under everything: the title row is the bar's, not the window server's.
        this.addSubview(&RowClaim::new(mtm));
        this.addSubview(&new_tab);
        this.addSubview(&list);
        this.addSubview(&warning);
        // The fades over the strip: what runs under an edge dissolves.
        this.addSubview(&strip);
        this.addSubview(&fade_leading);
        this.addSubview(&fade_trailing);
        list.setHidden(true);
        warning.setHidden(true);
        // SAFETY: `TabBar` implements the field's delegate protocols (the
        // two optional methods it uses are defined above); the field holds
        // its delegate weakly and the bar owns the field.
        unsafe { field.setDelegate(Some(ProtocolObject::from_ref(&*this))) };
        this.setAccessibilityElement(true);
        // SAFETY: AppKit's constant role string, alive for the process.
        this.setAccessibilityRole(Some(unsafe { NSAccessibilityTabGroupRole }));
        this.setAccessibilityLabel(Some(ns_string!("Tabs")));
        // A tab carried out of another window's strip may be let go here.
        this.registerForDraggedTypes(&NSArray::from_slice(&[tab_drag::tab_type()]));
        this
    }

    fn terminal_window(&self) -> Option<Retained<TerminalWindow>> {
        app::delegate(self.mtm())?.window(self.ivars().window)
    }

    /// Reduce Motion now — the setting and the system's answer, one `bool`.
    fn reduce_motion(&self) -> bool {
        app::delegate(self.mtm()).is_some_and(|app| app.reduce_motion())
    }

    /// What to show: the tabs in order, the selected one's position and the
    /// settings diagnostic (empty without one). An open card reads its tab
    /// again (`TerminalTab::card`) when what its chip shows changed — a
    /// command started or ended, a title, a transfer's percent, a mark: the
    /// read is a `Term` round, and a title news can come often while output
    /// streams. Lays out.
    pub(crate) fn show(&self, tabs: Vec<Label>, selected: usize, notice: String) {
        let iv = self.ivars();
        let open = iv.card.borrow().as_ref().map(|open| open.tab);
        let told = |shown: &Shown, tab: u64| {
            shown
                .tabs
                .iter()
                .find(|label| label.tab == tab)
                .map(|label| {
                    (
                        label.title.clone(),
                        label.indicator,
                        label.running.is_some(),
                        label.upload,
                        label.mark,
                    )
                })
        };
        let before = open.and_then(|tab| told(&iv.shown.borrow(), tab));
        iv.shown.replace(Shown {
            tabs,
            selected,
            notice,
        });
        if let Some(tab) = open
            && before != told(&iv.shown.borrow(), tab)
        {
            self.read_card(tab);
            self.draw_card();
        }
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
        self.ivars().theme.set(Some(*theme));
        self.lay_out();
        self.draw_card();
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

    /// The radius of the window's top-right corner: square in full screen,
    /// [`WINDOW_CORNER`] otherwise. The window lays the bar out again when
    /// it enters and leaves full screen, so `+` follows the corner.
    fn corner(&self) -> f64 {
        match self.window() {
            Some(window) if window.styleMask().contains(NSWindowStyleMask::FullScreen) => 0.0,
            _ => WINDOW_CORNER,
        }
    }

    /// One chip per label, in its order: a tab keeps its chip, a new tab
    /// gets a new one and a closed tab's goes. `true` when the list changed
    /// and there were chips before — the applier's change, which slides.
    fn chips_for(&self, tabs: &[Label]) -> (Vec<Retained<Chip>>, bool) {
        let iv = self.ivars();
        let mut old = iv.chips.take();
        let before: Vec<u64> = old.iter().map(|chip| chip.tab()).collect();
        let mut chips = Vec::with_capacity(tabs.len());
        for label in tabs {
            match old.iter().position(|chip| chip.tab() == label.tab) {
                Some(at) => chips.push(old.remove(at)),
                None => {
                    let chip = Chip::new(self.mtm(), label.tab);
                    iv.strip.addSubview(&chip);
                    chips.push(chip);
                }
            }
        }
        for gone in old {
            gone.removeFromSuperview();
        }
        let changed = !before.is_empty()
            && !before
                .iter()
                .copied()
                .eq(tabs.iter().map(|label| label.tab));
        iv.chips.replace(chips.clone());
        (chips, changed)
    }

    /// The pure layout's input for `count` places: the bar's width, where the strip may start,
    /// the settings diagnostic and the corner, and the scroll as it stands.
    fn bar_for(
        &self,
        count: usize,
        selected: usize,
        hovered: Option<usize>,
        dragged: Option<usize>,
    ) -> Bar {
        let iv = self.ivars();
        Bar {
            width: self.bounds().size.width,
            leading: self.leading(),
            count,
            selected,
            hovered,
            dragged,
            scroll: iv.scroll.get(),
            warning: !iv.shown.borrow().notice.is_empty(),
            corner: self.corner(),
        }
    }

    /// Places every part from the pure layout ([`Bar::layout`]) and gives
    /// the chips their tabs; then the card follows its chip and the clock is
    /// set again ([`Self::arm_clock`]).
    ///
    /// The strip scrolls by [`BarIvars::scroll`]. A **change of the selection**
    /// scrolls it to the selected chip (`Strip::revealing`) and, like the
    /// applier's changes, slides there; nothing else moves it but the wheel,
    /// which does not slide.
    ///
    /// **Two things in flight change what is laid out, neither of them the window's list.** A tab
    /// dragged along the strip is laid out at the place it would take, and its chip at the
    /// pointer, not at its place ([`Dragging::at`]); the others slide around it the way they slide
    /// for any change of order, so a drag has no animation path of its own. A tab carried over
    /// from another window opens a gap at its place ([`BarIvars::gap`]): the layout has one more
    /// place and the chips from the gap on take the next. The list itself changes once, when the
    /// tab is let go, through the window's applier.
    pub(crate) fn lay_out(&self) {
        let iv = self.ivars();
        let (Some(palette), Some(theme)) = (iv.palette.get(), iv.theme.get()) else {
            return;
        };
        let mut shown = iv.shown.borrow().clone();
        let count = shown.tabs.len();
        if count == 0 {
            return;
        }
        // A tab gone takes its name field away, and its drag.
        if let Some(tab) = iv.renaming.get()
            && !shown.tabs.iter().any(|label| label.tab == tab)
        {
            self.end_rename(false);
            return;
        }
        let drag = iv
            .drag
            .get()
            .filter(|drag| shown.tabs.iter().any(|label| label.tab == drag.tab));
        iv.drag.set(drag);
        // The tabs in the order the strip shows them now: the dragged one at its place.
        if let Some(Dragging {
            tab,
            at: Some((_, to)),
            torn: false,
            ..
        }) = drag
        {
            let selected = shown.tabs.get(shown.selected).map(|label| label.tab);
            if let Some(from) = shown.tabs.iter().position(|label| label.tab == tab) {
                tabs::reordered(&mut shown.tabs, from, to);
            }
            shown.selected = shown
                .tabs
                .iter()
                .position(|label| Some(label.tab) == selected)
                .unwrap_or(0);
        }
        let position = |tab: u64| shown.tabs.iter().position(|label| label.tab == tab);
        let dragged = drag.and_then(|drag| position(drag.tab));
        let hovered = iv.hovered.get().and_then(position);
        let gap = iv.gap.get().map(|gap| gap.min(count));
        // The lone tab is today's title bar; with a tab on its way in it is the first chip.
        let single = count == 1 && gap.is_none();
        // The layout's place for the tab at `index`: past the gap, one on.
        let seat = |index: usize| tabs::seat(index, gap);
        let bar = self.bar_for(
            count + usize::from(gap.is_some()),
            seat(shown.selected),
            hovered.map(seat),
            dragged.map(seat),
        );
        let mut strip = bar.layout();
        let selected_tab = shown.tabs.get(shown.selected).map(|label| label.tab);
        let mut revealed = false;
        let was_overflowing = iv.overflowed.replace(strip.overflow);
        let resized = (iv.width.replace(bar.width) - bar.width).abs() > 0.5;
        if single {
            iv.revealed.set(None);
        } else {
            let selection_changed = iv.revealed.replace(selected_tab) != selected_tab;
            // The tabs stopped fitting, or the room they scroll in changed
            // while they do not: the selected one stays in sight. Only a
            // change of the selection slides there.
            let kept_in_sight = strip.overflow && (!was_overflowing || resized);
            if selection_changed || kept_in_sight {
                let target = strip.revealing(seat(shown.selected));
                if target != strip.scroll {
                    strip = Bar {
                        scroll: target,
                        ..bar
                    }
                    .layout();
                    revealed = selection_changed;
                }
            }
        }
        iv.scroll.set(strip.scroll);
        let bounds = self.bounds();
        let top = ((bounds.size.height - CHIP_HEIGHT) / 2.0).max(0.0);
        // The chips live in the strip's space: its left edge is theirs.
        iv.strip.setFrame(NSRect::new(
            NSPoint::new(strip.span.x, 0.0),
            NSSize::new(strip.span.width, bounds.size.height),
        ));
        let (chips, reflow) = self.chips_for(&shown.tabs);
        let reduce = self.reduce_motion();
        let motion = Motion::of(reduce);
        let glide = iv.glide.take();
        let slide = ((reflow || revealed || glide) && motion.reflow > 0.0).then_some(motion.reflow);
        let hints = iv.hints.get();
        for (index, chip) in chips.iter().enumerate() {
            let label = &shown.tabs[index];
            let span = strip.chips[seat(index)];
            // The tab being named shows its field instead of title and indicator.
            let naming = iv.renaming.get() == Some(label.tab);
            let title = if naming {
                String::new()
            } else if single {
                single_label(&label.title, &shown.notice)
            } else {
                label.title.clone()
            };
            // A tab dragged along the strip is at the pointer, at once; the rest slide.
            let carried = drag
                .filter(|drag| !drag.torn && dragged == Some(index))
                .and_then(|drag| drag.at);
            let frame = NSRect::new(
                NSPoint::new(carried.map_or(span.x, |(left, _)| left) - strip.span.x, top),
                NSSize::new(span.width, CHIP_HEIGHT),
            );
            let mark = (label.mark != HostMark::None).then(|| theme.mark_rgb(label.mark));
            let look = Look {
                title: &title,
                indicator: label.indicator.filter(|_| !naming),
                step: label
                    .running
                    .map_or(0, |elapsed| tabs::ring_step(elapsed, reduce)),
                down: label.upload.is_some_and(|upload| upload.down),
                lines: Lines {
                    top: mark,
                    progress: label.upload.map(|upload| upload.fraction),
                },
                hint: hints.then(|| tabs::shortcut_hint(index, count)).flatten(),
                spoken: tabs::spoken(
                    &label.title,
                    label.indicator,
                    label.upload.map(|upload| upload.percent),
                    label.mark,
                ),
                selected: index == shown.selected,
                hovered: (hovered == Some(index) || carried.is_some()) && !naming,
                single,
                lifted: carried.is_some(),
                ghost: drag.is_some_and(|drag| drag.torn && dragged == Some(index)),
            };
            chip.set(
                frame,
                &look,
                palette,
                motion,
                slide.filter(|_| carried.is_none()),
            );
            if naming {
                self.place_field(span, top, palette, single);
            }
        }
        let inside = |x: f64| x - strip.span.x;
        iv.strip.set_separators(
            strip.separators.iter().map(|&x| inside(x)).collect(),
            palette,
        );
        let height = bounds.size.height;
        let edge = |x: f64| NSRect::new(NSPoint::new(x, 0.0), NSSize::new(FADE, height));
        for (fade, on, x) in [
            (&iv.fade_leading, strip.fade_leading, strip.span.x),
            (
                &iv.fade_trailing,
                strip.fade_trailing,
                strip.span.end() - FADE,
            ),
        ] {
            fade.setFrame(edge(x));
            fade.set_palette(palette);
            fade.setHidden(!on);
        }
        let button = |x: f64| NSRect::new(NSPoint::new(x, top), NSSize::new(BUTTON, BUTTON));
        iv.new_tab.setFrame(button(strip.new_tab));
        iv.new_tab.set_look(palette, strip.fit.radius);
        match strip.list {
            Some(x) => {
                iv.list.setFrame(button(x));
                iv.list.set_look(palette, strip.fit.radius);
                iv.list.setHidden(false);
            }
            None => iv.list.setHidden(true),
        }
        match strip.warning {
            Some(x) => {
                iv.warning.setFrame(button(x));
                iv.warning.set_look(palette, strip.fit.radius);
                let text = NSString::from_str(&shown.notice);
                iv.warning.setToolTip(Some(&text));
                iv.warning.setAccessibilityLabel(Some(&text));
                iv.warning.setHidden(false);
            }
            None => iv.warning.setHidden(true),
        }
        iv.laid.replace(Some(strip));
        self.place_card();
        self.arm_clock();
    }

    /// A wheel or trackpad step over the bar ([`Strip::wheeled`]); `false`
    /// when nothing scrolls (the tabs fit) and the event is not the bar's.
    fn wheel(&self, event: &NSEvent) -> bool {
        let iv = self.ivars();
        // A trackpad reports points, a wheel lines. The strip moves the way
        // the fingers (or the wheel) take the content: positive towards the
        // later tabs, the opposite of AppKit's delta.
        let unit = if event.hasPreciseScrollingDeltas() {
            1.0
        } else {
            WHEEL_STEP
        };
        let step = {
            let laid = iv.laid.borrow();
            let Some(laid) = laid.as_ref().filter(|laid| laid.overflow) else {
                return false;
            };
            let to = laid.wheeled(
                -event.scrollingDeltaX() * unit,
                -event.scrollingDeltaY() * unit,
            );
            (to != laid.scroll).then_some(to)
        };
        if let Some(to) = step {
            iv.scroll.set(to);
            self.lay_out();
        }
        true
    }

    // ─── Naming a tab ────────────────────────────────────────────────────

    /// Opens the name field over tab `tab`'s chip, its title selected: a
    /// double click, Rename Tab…. A lone tab's title is the window's and is
    /// named from the menu only (the title takes no click). A name under way
    /// elsewhere is kept first, and the chip is brought into view.
    pub(crate) fn begin_rename(&self, tab: u64) {
        let iv = self.ivars();
        let (index, shown_title) = {
            let shown = iv.shown.borrow();
            let Some(index) = shown.tabs.iter().position(|label| label.tab == tab) else {
                return;
            };
            (index, shown.tabs[index].title.clone())
        };
        // The window's title carries a transfer's prefix; a name does not.
        let title = self
            .terminal_window()
            .and_then(|window| window.tab_title(tab))
            .unwrap_or(shown_title);
        if iv.renaming.get() == Some(tab) {
            return;
        }
        self.end_rename(true);
        self.pressed(tab);
        let to = iv
            .laid
            .borrow()
            .as_ref()
            .map(|laid| laid.revealing(index))
            .filter(|_| iv.shown.borrow().tabs.len() > 1);
        if let Some(to) = to {
            iv.scroll.set(to);
        }
        iv.renaming.set(Some(tab));
        iv.opened_with.replace(title.clone());
        iv.field.setStringValue(&NSString::from_str(&title));
        self.addSubview(&iv.field);
        self.lay_out();
        // SAFETY: `selectText:` takes an optional sender, unused.
        unsafe { iv.field.selectText(None) };
    }

    /// Takes the name field away — Return and a lost keyboard keep what was
    /// typed (`commit`), Esc leaves the tab as it was. The window's applier
    /// decides what the text means ([`TerminalWindow::rename_tab`]).
    fn end_rename(&self, commit: bool) {
        let iv = self.ivars();
        // Taken first: handing the keyboard back ends the field's editing,
        // which says so again ([`TabBar`]'s `controlTextDidEndEditing:`).
        let Some(tab) = iv.renaming.take() else {
            return;
        };
        let draft = iv.field.stringValue().to_string();
        // Text nobody changed is not a name: the title it was opened with may
        // be stale by now (a shell that rewrites it on every command).
        let commit = commit && draft != *iv.opened_with.borrow();
        let window = self.terminal_window();
        if iv.field.currentEditor().is_some()
            && let Some(window) = &window
        {
            window.focus_selected_tab();
        }
        iv.field.removeFromSuperview();
        if commit && let Some(window) = &window {
            window.rename_tab(tab, &draft);
        }
        self.lay_out();
    }

    /// Puts the name field over `chip` (the bar's space), in the type the
    /// title wears there: the selected chip's, or the lone tab's.
    fn place_field(&self, chip: tabs::Span, top: f64, palette: Palette, single: bool) {
        let field = &self.ivars().field;
        // SAFETY: AppKit's constant font weight.
        let weight = unsafe { NSFontWeightSemibold };
        let (size, color, pad) = if single {
            (SINGLE_TEXT, palette.dim, SINGLE_PAD)
        } else {
            (CHIP_TEXT, palette.title, CHIP_PAD)
        };
        field.setFont(Some(&NSFont::systemFontOfSize_weight(size, weight)));
        field.setTextColor(Some(&Tint::of(color, 1.0).color()));
        let height = field.intrinsicContentSize().height;
        field.setFrame(NSRect::new(
            NSPoint::new(chip.x + pad, top + ((CHIP_HEIGHT - height) / 2.0).max(0.0)),
            NSSize::new((chip.width - 2.0 * pad).max(0.0), height),
        ));
    }

    // ─── The menus ───────────────────────────────────────────────────────

    /// The chip's menu for tab `tab` — what can be done to that tab whether
    /// or not it is selected. Every item names it in its `tag`
    /// ([`tabs::menu_tag`]) and acts through the window's applier.
    fn context_menu(&self, tab: u64) -> Option<Retained<NSMenu>> {
        let window = self.terminal_window()?;
        if self.ivars().shown.borrow().tabs.len() < 2 {
            return None;
        }
        // A menu is no place for the card; and the pointer resting on the
        // chip after it closes is not a reason to open it.
        self.pressed(tab);
        let mtm = self.mtm();
        let menu = NSMenu::new(mtm);
        let tag = tabs::menu_tag(tab);
        let add = |title: &str, action: Sel| {
            menu.addItem(&window_item(mtm, &window, title, action, tag));
        };
        add("Close Tab", sel!(closeChipTab:));
        add("Close Other Tabs", sel!(closeOtherTabs:));
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        add("Move Tab to New Window", sel!(detachTab:));
        // The tab on screen is the one it would join: its own chip has
        // nothing to merge into.
        if !window.is_selected(tab) {
            let merge = NSMenu::new(mtm);
            for (title, action) in [
                ("Split Right", sel!(mergeTabRight:)),
                ("Split Down", sel!(mergeTabDown:)),
            ] {
                merge.addItem(&window_item(mtm, &window, title, action, tag));
            }
            let holder = NSMenuItem::new(mtm);
            holder.setTitle(&NSString::from_str("Merge into Current Tab"));
            holder.setSubmenu(Some(&merge));
            menu.addItem(&holder);
        }
        add("Rename Tab\u{2026}", sel!(renameTab:));
        Some(menu)
    }

    /// Show All Tabs: every tab in a menu under the list button (or where it
    /// would be), the selected one ticked, a status dot in the colour of what
    /// the tab reports, its ⌘ key — a click selects it, the strip scrolls to
    /// it. Among several tabs only.
    pub(crate) fn show_list(&self) {
        let iv = self.ivars();
        let (shown, slot, palette) = (
            iv.shown.borrow().clone(),
            iv.laid.borrow().as_ref().map(Strip::list_slot),
            iv.palette.get(),
        );
        let (Some(window), Some(slot), Some(palette)) = (self.terminal_window(), slot, palette)
        else {
            return;
        };
        let count = shown.tabs.len();
        if count < 2 {
            return;
        }
        self.close_card();
        let mtm = self.mtm();
        let menu = NSMenu::new(mtm);
        for (index, label) in shown.tabs.iter().enumerate() {
            let item = window_item(
                mtm,
                &window,
                &label.title,
                sel!(pickTab:),
                tabs::menu_tag(label.tab),
            );
            if index == shown.selected {
                item.setState(NSControlStateValueOn);
            }
            // Every row has an image, a clear one where the tab reports
            // nothing: the titles start at one x.
            let dot = label
                .indicator
                .map(|indicator| palette.tone(tabs::list_dot(indicator), 0));
            item.setImage(Some(&dot_image(dot)));
            if let Some(digit) = tabs::shortcut_digit(index, count) {
                item.setKeyEquivalent(&NSString::from_str(&digit.to_string()));
                item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
            }
            menu.addItem(&item);
        }
        let height = self.bounds().size.height;
        let at = NSPoint::new(slot, height);
        menu.popUpMenuPositioningItem_atLocation_inView(None, at, Some(self));
    }

    /// The pointer came over tab `tab`'s chip (`Some`), or left the bar;
    /// the summary card follows it ([`Self::hover_card`]).
    fn hover(&self, tab: Option<u64>) {
        if self.ivars().hovered.replace(tab) != tab {
            self.lay_out();
            self.hover_card(tab);
        }
    }

    /// The pointer left tab `tab`'s chip — unless it already entered
    /// another, whose enter can come first.
    fn unhover(&self, tab: u64) {
        if self.ivars().hovered.get() == Some(tab) {
            self.hover(None);
        }
    }

    /// A chip was pressed: the window's applier selects it; `false` if it would not.
    fn select(&self, tab: u64) -> bool {
        self.terminal_window()
            .is_some_and(|window| window.select_tab(tab))
    }

    // ─── Dragging a tab ──────────────────────────────────────────────────

    /// The chip of tab `tab`.
    fn chip(&self, tab: u64) -> Option<Retained<Chip>> {
        self.ivars()
            .chips
            .borrow()
            .iter()
            .find(|chip| chip.tab() == tab)
            .cloned()
    }

    /// The tab whose chip is under `point` (the bar's space), if any — a chip scrolled out of
    /// the strip's sight is not under anything.
    fn tab_under(&self, point: NSPoint) -> Option<u64> {
        let iv = self.ivars();
        let local = self.convertPoint_toView(point, Some(&iv.strip));
        if !contains(iv.strip.bounds(), local) {
            return None;
        }
        let chips = iv.chips.borrow();
        chips
            .iter()
            .find(|chip| contains(chip.frame(), local))
            .map(|chip| chip.tab())
    }

    /// The strip as the window's list lays it out — without a drag's preview or a gap — with
    /// `room` more places than it has tabs: 0 is the strip as it is, 1 the strip a carried tab
    /// would make. Places are measured on the one that is on screen when the tab lets go
    /// ([`Strip::slot_at`]), so the room that opens is under the pointer.
    fn strip_with_room(&self, room: usize) -> Option<Strip> {
        let count = self.ivars().shown.borrow().tabs.len();
        (count > 0).then(|| self.bar_for(count + room, 0, None, None).layout())
    }

    /// The press on tab `tab`'s chip, as `event` reports it, may become a drag. Not among
    /// fewer than two tabs (a lone title is the window's), not while a name is being typed, and
    /// not while the window holds a question of its own: the tab could not move then.
    fn grip(&self, tab: u64, event: &NSEvent) {
        let iv = self.ivars();
        iv.drag.set(None);
        let free = self
            .terminal_window()
            .is_some_and(|window| window.selection_free());
        if iv.renaming.get().is_some() || !free {
            return;
        }
        let press = self.convertPoint_fromView(event.locationInWindow(), None);
        let index = iv
            .shown
            .borrow()
            .tabs
            .iter()
            .position(|label| label.tab == tab);
        let grip = index.and_then(|index| {
            let laid = iv.laid.borrow();
            Grip::new(laid.as_ref()?, index, press.x)
        });
        if let Some(grip) = grip {
            iv.drag.set(Some(Dragging {
                tab,
                grip,
                at: None,
                torn: false,
            }));
        }
    }

    /// The pointer moved with the button down on tab `tab`'s chip ([`Grip::track`]): past
    /// the slop the chip follows it along the strip and the others make room; out of the bar the
    /// tab is carried away in a drag session.
    fn drag(&self, tab: u64, event: &NSEvent) {
        let iv = self.ivars();
        let Some(mut drag) = iv.drag.get().filter(|drag| drag.tab == tab && !drag.torn) else {
            return;
        };
        let point = self.convertPoint_fromView(event.locationInWindow(), None);
        let size = self.bounds().size;
        let (tracked, strip_x) = {
            let laid = iv.laid.borrow();
            let Some(laid) = laid.as_ref() else {
                return;
            };
            (
                drag.grip
                    .track(laid, (point.x, point.y), (size.width, size.height)),
                laid.span.x,
            )
        };
        match tracked {
            Drag::Press => iv.drag.set(Some(drag)),
            Drag::Reorder { left, to } => {
                let first = drag.at.is_none();
                let same_place = drag.at.is_some_and(|(_, place)| place == to);
                drag.at = Some((left, to));
                iv.drag.set(Some(drag));
                let chip = self.chip(tab);
                if first && let Some(chip) = &chip {
                    // Above its neighbours for as long as it is held.
                    iv.strip.addSubview_positioned_relativeTo(
                        chip,
                        NSWindowOrderingMode::Above,
                        None,
                    );
                }
                match chip {
                    // Only the held chip moved: the rest of the strip is as it was laid out.
                    Some(chip) if same_place && !first => chip.follow(left - strip_x),
                    _ => self.lay_out(),
                }
            }
            Drag::TearOff => {
                // Only a tab a session has taken is torn: were none started, the press goes on
                // as a drag along the strip and no faint place is left behind.
                if self.tear_off(tab, event) {
                    drag.at = None;
                    drag.torn = true;
                    iv.drag.set(Some(drag));
                    iv.glide.set(true);
                    self.lay_out();
                }
            }
        }
    }

    /// The tab leaves the bar: a drag session carries it (`tab_drag`), and the application keeps
    /// the session's source until it ends. `false` if none could start.
    fn tear_off(&self, tab: u64, event: &NSEvent) -> bool {
        let (Some(chip), Some(app)) = (self.chip(tab), app::delegate(self.mtm())) else {
            return false;
        };
        app.hold_tab_drag(tab_drag::begin(&chip, tab, event));
        true
    }

    /// The button is let go on tab `tab`'s chip: a tab moved along the strip settles in the place
    /// it was carried to — the window's applier orders it, once, however many places it passed
    /// — and the chip glides there from the pointer.
    fn release(&self, tab: u64, event: &NSEvent) {
        let iv = self.ivars();
        let Some(drag) = iv.drag.get().filter(|drag| drag.tab == tab) else {
            return;
        };
        if drag.torn {
            // The session has it; this release is not its end.
            return;
        }
        iv.drag.set(None);
        // A click that never moved the tab is only a click.
        let Some((_, to)) = drag.at else {
            return;
        };
        iv.glide.set(true);
        // Where the tab stands in the list now, not where it stood at the press: the list can
        // change under a held button (a background tab's shell exits).
        let now = iv
            .shown
            .borrow()
            .tabs
            .iter()
            .position(|label| label.tab == tab);
        match self.terminal_window() {
            Some(window) if now.is_some_and(|now| now != to) => window.move_tab(tab, to),
            _ => self.lay_out(),
        }
        // No enter or exit comes while the button is down: the chip under the pointer now is
        // the hovered one.
        let point = self.convertPoint_fromView(event.locationInWindow(), None);
        self.hover(self.tab_under(point));
    }

    /// The drag session that carried a tab away from this bar is over, wherever the tab went: its
    /// chip returns to full strength in its place, or is gone with its tab, and the chip under
    /// the pointer is the hovered one (no enter came during the drag).
    pub(crate) fn drag_ended(&self) {
        let iv = self.ivars();
        iv.drag.set(None);
        let under = self.window().and_then(|window| {
            let point =
                self.convertPoint_fromView(window.mouseLocationOutsideOfEventStream(), None);
            self.tab_under(point)
        });
        iv.hovered.set(under);
        iv.glide.set(true);
        self.lay_out();
    }

    /// A tab carried over this bar opens room at slot `gap` (`None` closes it).
    pub(crate) fn open_gap(&self, gap: Option<usize>) {
        let iv = self.ivars();
        if iv.gap.replace(gap) != gap {
            iv.glide.set(true);
            self.lay_out();
        }
    }

    /// Closes the gap **without** a layout of its own — the window's list is about to take the
    /// tab in, and that layout is the one that matters: the chips move from where they stand
    /// (gap open) to where they will stand (the tab there), not through a gap closing first.
    pub(crate) fn close_gap_quietly(&self) {
        self.ivars().gap.set(None);
    }

    /// `draggingEntered:` and `draggingUpdated:`: the carried tab over this bar. A tab from
    /// another window opens room at the slot under the pointer **of the strip it would make**, so
    /// the room is under the pointer and the answer does not depend on the room; one of this
    /// window's own shows no gap (its faint chip is its place) and is simply taken. Anything that
    /// is not a tab of this process, or that this window cannot take now (it holds a question
    /// of its own), is not taken: the drop is then on nothing, and the tab becomes a window.
    fn carried_over(&self, info: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
        let Some(tab) = tab_drag::carried(info) else {
            return NSDragOperation::None;
        };
        let Some(window) = self
            .terminal_window()
            .filter(|window| window.selection_free())
        else {
            self.open_gap(None);
            return NSDragOperation::None;
        };
        let gap = window.index_of(tab).is_none().then(|| {
            let point = self.convertPoint_fromView(info.draggingLocation(), None);
            self.strip_with_room(1)
                .map_or(0, |strip| strip.slot_at(point.x))
        });
        self.open_gap(gap);
        NSDragOperation::Move
    }

    /// `performDragOperation:`: the carried tab is let go here. What it comes to is decided now
    /// ([`tabs::landing`]) and carried out when the session has ended, in a turn of its own; the
    /// gap stays open until then, so the strip does not close up and open again. Always
    /// accepted if it is a tab this window takes: a refusal would read as a drop on nothing and
    /// make a window.
    fn accept_drop(&self, info: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
        let Some(tab) = tab_drag::carried(info) else {
            return false;
        };
        let (Some(window), Some(app)) = (self.terminal_window(), app::delegate(self.mtm())) else {
            return false;
        };
        let point = self.convertPoint_fromView(info.draggingLocation(), None);
        let over = if window.index_of(tab).is_some() {
            Over::Own {
                place: self
                    .strip_with_room(0)
                    .map_or(0, |strip| strip.slot_at(point.x)),
            }
        } else {
            Over::Other {
                gap: self
                    .strip_with_room(1)
                    .map_or(0, |strip| strip.slot_at(point.x)),
            }
        };
        app.tab_dropped(window.id(), tabs::landing(Some(over)));
        true
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

    // ─── The clock ───────────────────────────────────────────────────────

    /// Sets the bar's one delayed wake from what changes with time on it
    /// (`tabs::Clock::delay`): the running rings of several tabs on a
    /// visible window outside Reduce Motion, and the open card's running
    /// time. A wake already due sooner is kept; one due later gives way —
    /// `after` cannot be cancelled, so a wake that is not wanted any more is
    /// made stale by the generation and does nothing when it comes. Nothing
    /// to show: no wake.
    pub(crate) fn arm_clock(&self) {
        let iv = self.ivars();
        let visible = self
            .terminal_window()
            .is_some_and(|window| window.window_visible());
        let shown = iv.shown.borrow();
        // The rings that show: a running tab whose glyph is not a more
        // urgent one (a question), among several tabs.
        let running = if shown.tabs.len() > 1 {
            shown
                .tabs
                .iter()
                .filter(|label| label.indicator == Some(Indicator::Running))
                .filter_map(|label| label.running)
                .collect()
        } else {
            Vec::new()
        };
        drop(shown);
        let card = iv.card.borrow().as_ref().and_then(OpenCard::running);
        let delay = Clock {
            visible,
            reduce_motion: self.reduce_motion(),
            running,
            card,
        }
        .delay();
        let Some(delay) = delay else {
            if iv.clock_due.take().is_some() {
                iv.clock.set(iv.clock.get().wrapping_add(1));
            }
            return;
        };
        let now = Instant::now();
        let due = now + delay;
        // A wake due in the past was lost (its job found no window, or
        // `dispatch` could not take it): it does not hold the place.
        if iv
            .clock_due
            .get()
            .is_some_and(|armed| armed > now && armed <= due)
        {
            return;
        }
        let generation = iv.clock.get().wrapping_add(1);
        iv.clock.set(generation);
        let armed = after(delay, iv.window, move |bar| bar.clock_fired(generation));
        iv.clock_due.set(armed.then_some(due));
    }

    /// The wake came: if it is the live one, every tab's running time is
    /// read again (`TerminalWindow::running_times`, a leaf lock per pane),
    /// the rings step, the open card's time moves and the next wake is set.
    fn clock_fired(&self, generation: u64) {
        let iv = self.ivars();
        if iv.clock.get() != generation {
            return;
        }
        iv.clock_due.set(None);
        if let Some(window) = self.terminal_window() {
            let times = window.running_times();
            let mut shown = iv.shown.borrow_mut();
            for label in &mut shown.tabs {
                if let Some((_, running)) = times.iter().find(|(tab, _)| *tab == label.tab) {
                    label.running = *running;
                }
            }
        }
        self.time_card();
        self.lay_out();
    }

    // ─── The summary card ────────────────────────────────────────────────

    /// The pointer's chip changed: the card opens for the new one after
    /// [`CARD_DELAY`] — at once while a card is open or one closed within
    /// [`CARD_WARM`] — and closes when the pointer leaves the chips. A
    /// pending opening for another chip is made stale.
    fn hover_card(&self, tab: Option<u64>) {
        let iv = self.ivars();
        let generation = iv.card_wait.get().wrapping_add(1);
        iv.card_wait.set(generation);
        if iv.card_quiet.get() != tab {
            iv.card_quiet.set(None);
        }
        let Some(tab) = tab else {
            self.close_card();
            return;
        };
        if iv.card_quiet.get() == Some(tab) {
            return;
        }
        let warm = iv.card.borrow().is_some()
            || iv
                .card_closed
                .get()
                .is_some_and(|closed| closed.elapsed() < CARD_WARM);
        if warm {
            self.open_card(tab);
            return;
        }
        after(CARD_DELAY, iv.window, move |bar| {
            let iv = bar.ivars();
            if iv.card_wait.get() == generation && iv.hovered.get() == Some(tab) {
                bar.open_card(tab);
            }
        });
    }

    /// A chip was pressed: its card goes and stays away until the pointer
    /// leaves the chip.
    fn pressed(&self, tab: u64) {
        let iv = self.ivars();
        iv.card_wait.set(iv.card_wait.get().wrapping_add(1));
        iv.card_quiet.set(Some(tab));
        self.close_card();
    }

    /// Opens (or turns) the card to tab `tab`'s — only among several tabs,
    /// where a tab is a chip.
    fn open_card(&self, tab: u64) {
        if self.ivars().shown.borrow().tabs.len() < 2 {
            return;
        }
        self.read_card(tab);
        self.draw_card();
        self.place_card();
        self.arm_clock();
    }

    /// Reads tab `tab`'s card (`TerminalTab::card`: a `Term` round for its
    /// newest command's row) into the open card; a tab no longer in the
    /// window has none, and the card goes.
    fn read_card(&self, tab: u64) {
        let card = self
            .terminal_window()
            .and_then(|window| window.tab_card(tab));
        match card {
            Some(card) => {
                self.ivars().card.replace(Some(OpenCard {
                    tab,
                    card,
                    read_at: Instant::now(),
                }));
            }
            None => self.close_card(),
        }
    }

    /// The clock moved: an open card's running time follows its tab's,
    /// without reading its row again.
    fn time_card(&self) {
        if self
            .ivars()
            .card
            .borrow()
            .as_ref()
            .is_some_and(|open| open.running().is_some())
        {
            self.draw_card();
        }
    }

    /// Writes the open card's lines into its view, in the theme's colours.
    fn draw_card(&self) {
        let iv = self.ivars();
        let (Some(palette), Some(theme)) = (iv.palette.get(), iv.theme.get()) else {
            return;
        };
        let card = iv.card.borrow();
        let Some(open) = card.as_ref() else {
            return;
        };
        let mark = theme.mark_rgb(open.card.mark);
        let lines: Vec<(String, u32)> = tabs::card_lines(&open.now())
            .into_iter()
            .map(|(text, tone)| (text, palette.tone(tone, mark)))
            .collect();
        let height = iv.card_view.write(&lines, palette);
        let mut frame = iv.card_view.frame();
        frame.size = NSSize::new(CARD_WIDTH, height);
        iv.card_view.setFrame(frame);
    }

    /// Puts the open card under its chip, over the content (the root view's
    /// top subview); closes it when its tab is no longer a chip.
    fn place_card(&self) {
        let iv = self.ivars();
        let Some(tab) = iv.card.borrow().as_ref().map(|open| open.tab) else {
            return;
        };
        let chip = iv
            .chips
            .borrow()
            .iter()
            .find(|chip| chip.tab() == tab)
            .cloned();
        // SAFETY: reading the superview; we are on the main thread.
        let root = unsafe { self.superview() };
        let (Some(chip), Some(root), true) = (chip, root, iv.chips.borrow().len() > 1) else {
            self.close_card();
            return;
        };
        // The chip's frame is in the strip's space.
        let at = chip.frame();
        let size = iv.card_view.frame().size;
        let origin = NSPoint::new(
            card_x(
                iv.strip.frame().origin.x + at.origin.x,
                self.bounds().size.width,
            ),
            self.frame().origin.y + at.origin.y + at.size.height + CARD_DROP,
        );
        let card = &iv.card_view;
        card.setFrame(NSRect::new(origin, size));
        // Over every container — a tab added since joined above it.
        let card_ptr: *const NSView = &***card;
        let on_top = root
            .subviews()
            .lastObject()
            .is_some_and(|top| std::ptr::eq(Retained::as_ptr(&top), card_ptr));
        if !on_top {
            card.removeFromSuperview();
            root.addSubview_positioned_relativeTo(card, NSWindowOrderingMode::Above, None);
        }
    }

    /// Takes the card away; the warm window starts.
    fn close_card(&self) {
        let iv = self.ivars();
        if iv.card.take().is_some() {
            iv.card_closed.set(Some(Instant::now()));
        }
        iv.card_view.removeFromSuperview();
    }

    /// The window stopped being key: the key hints and the card go — the
    /// release of ⌘ or the pointer's leaving may reach another application.
    pub(crate) fn resigned(&self) {
        self.command_held(false);
        let iv = self.ivars();
        iv.card_wait.set(iv.card_wait.get().wrapping_add(1));
        self.close_card();
    }

    // ─── ⌘ hints ─────────────────────────────────────────────────────────

    /// ⌘ is held alone in this window (`true`) or not — from the
    /// application's key watch (`AppDelegate::command_held`) and the window
    /// resigning key. Holding shows the keys after [`HINT_DELAY`]; anything
    /// else hides them at once and makes a pending show stale.
    pub(crate) fn command_held(&self, held: bool) {
        let iv = self.ivars();
        if !held {
            // A pending show goes stale; nothing shown or pending, nothing
            // to do (every key press comes here).
            if iv.hint_pending.replace(false) {
                iv.hint_wait.set(iv.hint_wait.get().wrapping_add(1));
            }
            if iv.hints.replace(false) {
                self.lay_out();
            }
            return;
        }
        if iv.hints.get() || iv.hint_pending.replace(true) {
            return;
        }
        let generation = iv.hint_wait.get().wrapping_add(1);
        iv.hint_wait.set(generation);
        after(HINT_DELAY, iv.window, move |bar| {
            let iv = bar.ivars();
            if iv.hint_wait.get() == generation {
                iv.hint_pending.set(false);
                if !iv.hints.replace(true) {
                    bar.lay_out();
                }
            }
        });
    }
}

/// Whether `flags` hold ⌘ and no other modifier a shortcut uses (⇧, ⌥,
/// ⌃) — Caps Lock and the keypad's and the function key's bits do not
/// count: with Caps Lock on, ⌘ alone still shows the hints.
fn command_alone(flags: NSEventModifierFlags) -> bool {
    let chord = NSEventModifierFlags::Command
        | NSEventModifierFlags::Shift
        | NSEventModifierFlags::Option
        | NSEventModifierFlags::Control;
    flags.intersection(chord) == NSEventModifierFlags::Command
}

/// Watches ⌘ for the tabs' key hints: a local monitor of flag changes and
/// key presses, the application's for its lifetime (the caller keeps the
/// token). ⌘ alone held is a hint's start; any other modifier, its release
/// or a key press (a chord: ⌘C) is its end
/// (`AppDelegate::command_held`, which tells the key window's bar). The
/// event passes on unchanged.
pub(crate) fn watch_command_key() -> Option<Retained<AnyObject>> {
    let block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
        // SAFETY: AppKit gives the monitor a valid event.
        let event_ref = unsafe { event.as_ref() };
        let held = event_ref.r#type() == NSEventType::FlagsChanged
            && command_alone(event_ref.modifierFlags());
        // audit: a local monitor runs on the main thread, before `sendEvent:`.
        let mtm = MainThreadMarker::new().expect("a local event monitor runs on the main thread");
        if let Some(app) = app::delegate(mtm) {
            app.command_held(held);
        }
        event.as_ptr()
    });
    // SAFETY: the block returns the valid event it was given.
    unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(
            NSEventMask::FlagsChanged | NSEventMask::KeyDown,
            &block,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CARD_PAD_X, CARD_WIDTH, GLYPH_GAP, GLYPH_SIDE, Motion, Palette, TAB_RADIUS, TitleAction,
        card_x, command_alone, line_span, single_label, title_double_click, title_row,
    };
    use crate::tabs::{self, Indicator, Tone};
    use bt_core::Theme;
    use objc2_app_kit::NSEventModifierFlags;

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
    /// theme gives it a white face with a shadow instead. No role is new:
    /// the indicators and the card are the theme's own roles.
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
        assert_eq!(light.accent, Theme::BATERI_LIGHT.accent);
        assert_eq!(light.success, Theme::BATERI_LIGHT.success);
        assert_eq!(light.error, Theme::BATERI_LIGHT.error);
        assert_eq!(light.card.rgb, Theme::BATERI_LIGHT.background);
        assert_eq!(dark.separator.rgb, Theme::BATERI.foreground);
    }

    /// A scrolled strip's edge fades out to the window's own background —
    /// the theme's, light or dark — or it would show as a band.
    #[test]
    fn the_strips_edges_fade_to_the_themes_background() {
        for theme in [Theme::BATERI, Theme::BATERI_LIGHT, Theme::LINEN] {
            assert_eq!(Palette::of(&theme).ground, theme.background);
        }
    }

    /// A Show All Tabs row's dot is drawn in the same role the chip's
    /// indicator wears for what it reports.
    #[test]
    fn a_list_dot_takes_the_colour_of_its_role() {
        let palette = Palette::of(&Theme::BATERI);
        assert_eq!(
            palette.tone(tabs::list_dot(Indicator::Running), 0),
            palette.accent
        );
        assert_eq!(
            palette.tone(tabs::list_dot(Indicator::Failed), 0),
            palette.error
        );
        assert_eq!(
            palette.tone(tabs::list_dot(Indicator::Finished), 0),
            palette.success
        );
    }

    /// A card line's colour is its tone's role; the host line is the mark's
    /// own colour, the dock's mapping.
    #[test]
    fn card_lines_wear_their_roles() {
        let palette = Palette::of(&Theme::BATERI);
        let mark = Theme::BATERI.mark_rgb(bt_core::HostMark::Production);
        assert_eq!(palette.tone(Tone::Error, mark), Theme::BATERI.error);
        assert_eq!(palette.tone(Tone::Accent, mark), Theme::BATERI.accent);
        assert_eq!(palette.tone(Tone::Dim, mark), Theme::BATERI.dim);
        assert_eq!(palette.tone(Tone::Mark, mark), Theme::BATERI.error);
    }

    /// The indicator and a short title are centred together; a long title
    /// gives way to the glyph and is cut, never the glyph; without an
    /// indicator the title has the whole width, as before.
    #[test]
    fn the_indicator_sits_left_of_the_title_and_both_are_centred() {
        let step = GLYPH_SIDE + GLYPH_GAP;
        assert_eq!(title_row(132.0, 40.0, false), (None, 0.0, 132.0));
        let (glyph, text, width) = title_row(132.0, 40.0, true);
        assert_eq!(glyph, Some(((132.0 - step - 40.0) / 2.0_f64).round()));
        assert_eq!(text, glyph.unwrap() + step);
        assert_eq!(width, 40.0);
        let (glyph, text, width) = title_row(132.0, 300.0, true);
        assert_eq!(glyph, Some(0.0));
        assert_eq!(text, step);
        assert_eq!(width, 132.0 - step, "the title is cut, not the glyph");
        let (_, _, width) = title_row(10.0, 300.0, true);
        assert_eq!(width, 0.0, "a chip too narrow keeps the glyph alone");
    }

    /// The lines run inside a chip's rounded corners and never go negative.
    #[test]
    fn a_chips_lines_stay_inside_its_corners() {
        assert_eq!(line_span(184.0), (TAB_RADIUS, 184.0 - 2.0 * TAB_RADIUS));
        assert_eq!(line_span(4.0).1, 0.0);
    }

    /// The card hangs under its chip and stays inside the window.
    #[test]
    fn the_card_stays_inside_the_window() {
        assert_eq!(card_x(270.0, 1000.0), 270.0);
        assert_eq!(card_x(900.0, 1000.0), 1000.0 - CARD_WIDTH - CARD_PAD_X);
        assert_eq!(card_x(-40.0, 1000.0), CARD_PAD_X, "a strip scrolled off");
        assert_eq!(
            card_x(100.0, 200.0),
            CARD_PAD_X,
            "a window narrower than the card"
        );
    }

    /// Reduce Motion: every duration is zero, the bar changes at once.
    #[test]
    fn reduce_motion_makes_every_motion_instant() {
        assert_eq!(
            Motion::of(true),
            Motion {
                hover: 0.0,
                close: 0.0,
                reflow: 0.0
            }
        );
        let motion = Motion::of(false);
        assert_eq!(
            (motion.hover, motion.close, motion.reflow),
            (0.08, 0.12, 0.2),
            "the design's 80, 120 and 200 ms"
        );
    }

    /// ⌘ alone shows the hints, Caps Lock or not; a chord's other modifier
    /// hides them.
    #[test]
    fn command_alone_ignores_caps_lock() {
        let command = NSEventModifierFlags::Command;
        assert!(command_alone(command));
        assert!(command_alone(command | NSEventModifierFlags::CapsLock));
        assert!(command_alone(command | NSEventModifierFlags::Function));
        assert!(!command_alone(command | NSEventModifierFlags::Shift));
        assert!(!command_alone(command | NSEventModifierFlags::Option));
        assert!(!command_alone(NSEventModifierFlags::empty()));
    }
}
