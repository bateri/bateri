//! The arrangement moment: ⌥⌘ held alone for [`HOLD_DELAY`] lifts every pane
//! of the key window's selected tab a little and floats a **capsule** over the
//! top middle of each — its hold, its state, its name, and the four things a
//! pane can do from there: split right, split down, move to a tab of its own
//! and close. Letting go of either key, or any key press (⌥⌘← is Select
//! Split, not an arrangement), sets everything down again.
//!
//! **One watch for ⌘ and ⌥⌘.** `tab_bar::watch_command_key` is the only
//! local monitor of modifier changes; it classifies the flags once
//! ([`hold_of`]) and `AppDelegate::hold_changed` tells the key window's bar
//! (⌘: the tab keys) or the arrangement (⌥⌘). The 300 ms wait is the hint's
//! number and the same cancelling rule: anything else makes a pending lift
//! stale.
//!
//! **The lift is Core Animation's, never a frame's.** A lifted pane keeps its
//! frame — its program is not resized — and `card::lift` shrinks its layer
//! (3 %, the pane under the pointer 1.5 %) with an explicit animation; under
//! Reduce Motion nothing shrinks, the capsule and the frame are the whole
//! signal. Behind each pane sits a *plate*, a rounded fill that casts the
//! shadow: the pane's own layer clips to its corners and would clip a shadow
//! with them. The plate never shows past the pane's frame: it is laid out
//! once, by its **frame**, at the smallest look a lifted pane takes ([`LIFT`])
//! and never moves while lifted. A transform set on a view AppKit has just
//! added does not hold (the plate stood full size around the shrunk pane),
//! and a plate following the pointer's 1.5 % on AppKit's clock trailed the
//! pane's Core Animation for a few frames — both read as a second frame
//! outside the pane's own. Nothing here asks for a GPU frame.
//!
//! **The pointer.** While lifted, a local monitor of mouse motion (installed
//! on the lift, removed on the set-down) tells which pane the pointer is
//! over; the overlay's cursor rect makes it an open hand. The press itself is
//! kept from the program in `BateriView::button_event`, by the flags on the
//! event, so a click just after the keys went down is not a stray report
//! either.
//!
//! **The geometry is pure** ([`plan`], [`place`]): which parts of a capsule
//! fit a pane, where it sits and what it keeps clear of. The AppKit half is
//! the views below and [`Raised`], the tab's record of what it lifted.

use std::cell::{Cell, RefCell};
use std::ptr::NonNull;
use std::time::Duration;

use block2::RcBlock;
use bt_core::Theme;
use dispatch2::{DispatchQueue, DispatchTime};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_app_kit::{
    NSAccessibility, NSAccessibilityButtonRole, NSAnimatablePropertyContainer, NSAnimationContext,
    NSAutoresizingMaskOptions, NSBezierPath, NSBox, NSBoxType, NSColor, NSCursor, NSEvent,
    NSEventMask, NSEventModifierFlags, NSFont, NSFontWeightMedium, NSImage, NSImageScaling,
    NSImageView, NSLineBreakMode, NSShadow, NSTextField, NSTitlePosition, NSView,
    NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView,
    NSWindowOrderingMode, NSWorkspace,
};
use objc2_foundation::{NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};
use objc2_quartz_core::CAMediaTimingFunction;

use crate::app;
use crate::card::{self, corner_pt};
use crate::pane::TerminalPane;
use crate::sheets;
use crate::split::{Axis, Rect};
use crate::split_view::SplitView;
use crate::tab_bar::{self, HINT_DELAY, Tint};

// ─── The hold ────────────────────────────────────────────────────────────

/// Which modifiers are held, as far as the shell's two key moments care.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Hold {
    #[default]
    Nothing,
    /// ⌘ alone: the tabs show their keys after a wait.
    Command,
    /// ⌥⌘ and nothing else: the arrangement, after the same wait.
    Arrange,
}

/// How long ⌥⌘ is held before the panes lift: the ⌘ hint's wait and for the
/// same reason — the keys go down before every shortcut (⌥⌘←) and a lift at
/// once would flash at each.
pub(crate) const HOLD_DELAY: Duration = HINT_DELAY;

/// What `flags` hold: ⌘ alone ([`tab_bar::command_alone`]: Caps Lock and the
/// keypad's and the function key's bits do not count), ⌥⌘ and no ⇧ or ⌃, or
/// neither.
pub(crate) fn hold_of(flags: NSEventModifierFlags) -> Hold {
    let chord = NSEventModifierFlags::Command
        | NSEventModifierFlags::Shift
        | NSEventModifierFlags::Option
        | NSEventModifierFlags::Control;
    if tab_bar::command_alone(flags) {
        Hold::Command
    } else if flags.intersection(chord)
        == NSEventModifierFlags::Command | NSEventModifierFlags::Option
    {
        Hold::Arrange
    } else {
        Hold::Nothing
    }
}

/// Whether a mouse event carrying `flags` is the arrangement's, not the
/// program's: ⌘ and ⌥ both down (a third modifier does not hand it back — a
/// ⇧⌥⌘ press is nothing the program was ever told about).
pub(crate) fn swallows(flags: NSEventModifierFlags) -> bool {
    flags.contains(NSEventModifierFlags::Command) && flags.contains(NSEventModifierFlags::Option)
}

/// Whether ⌘ is down **as a link's key**: with ⌥ it is the arrangement's.
pub(crate) fn link_command(flags: NSEventModifierFlags) -> bool {
    flags.contains(NSEventModifierFlags::Command) && !flags.contains(NSEventModifierFlags::Option)
}

/// Runs `job` on the main queue after `delay`, with the application: found
/// again by the queue, as it is not `Send`.
pub(crate) fn after(delay: Duration, job: impl FnOnce(&app::AppDelegate) + Send + 'static) {
    let Ok(when) = DispatchTime::try_from(delay) else {
        return;
    };
    let _ = DispatchQueue::main().after(when, move || {
        // audit: a block running on the main queue is on the main thread by definition.
        let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
        if let Some(app) = app::delegate(mtm) {
            job(&app);
        }
    });
}

// ─── What a capsule says and does ────────────────────────────────────────

/// A capsule's four tools.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tool {
    SplitRight,
    SplitDown,
    NewTab,
    Close,
}

impl Tool {
    fn symbol(self) -> &'static str {
        match self {
            Self::SplitRight => "rectangle.split.2x1",
            Self::SplitDown => "rectangle.split.1x2",
            Self::NewTab => "arrow.up.right.square",
            Self::Close => "xmark",
        }
    }

    /// The name and the key, as the tool tip and VoiceOver say it.
    fn name(self) -> &'static str {
        match self {
            Self::SplitRight => "Split Right",
            Self::SplitDown => "Split Down",
            Self::NewTab => "Open in New Tab",
            Self::Close => "Close Split",
        }
    }

    fn key(self) -> Option<&'static str> {
        match self {
            Self::SplitRight => Some("\u{2318}D"),
            Self::SplitDown => Some("\u{21e7}\u{2318}D"),
            Self::NewTab => None,
            Self::Close => Some("\u{2318}W"),
        }
    }

    pub(crate) fn axis(self) -> Option<Axis> {
        match self {
            Self::SplitRight => Some(Axis::Horizontal),
            Self::SplitDown => Some(Axis::Vertical),
            _ => None,
        }
    }
}

/// What a pane is doing, as the capsule's glyph shows it: the sibling of the
/// tab chip's indicator, for one pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    /// A shell at its prompt.
    Shell,
    /// A command or program is running: the ring.
    Running,
    /// An ssh session: ⇄.
    Remote,
    /// A question waits for an answer.
    Question,
}

/// One pane's capsule: what it says. The tab builds these from its panes.
pub(crate) struct Entry {
    pub pane: u64,
    pub name: String,
    pub dir: String,
    pub status: Status,
}

/// What the tab allows its capsules.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Wants {
    /// The hold: the pane can go somewhere (another tab, another window, a
    /// place beside another pane). With one tab, one pane and no other
    /// window there is nowhere to take it, and the hold does not show.
    pub grip: bool,
    /// Open in New Tab: only a split tab has a pane to take out.
    pub new_tab: bool,
}

// ─── The capsule's geometry (pure) ───────────────────────────────────────

/// The capsule's height, in points. A design constant, the canvas's.
pub(crate) const HEIGHT: f64 = 38.0;
const RADIUS: f64 = 12.0;
/// From the lifted pane's top to the capsule's, and the least room kept
/// between the capsule and what it slides below.
const TOP: f64 = 8.0;
const EDGE: f64 = 8.0;
const GRIP_W: f64 = 18.0;
const STATUS_W: f64 = 14.0;
const GAP: f64 = 8.0;
const TEXT_GAP: f64 = 6.0;
const TOOL: f64 = 28.0;
const TOOL_GAP: f64 = 2.0;
/// The widest the name and the directory are given.
const NAME_MAX: f64 = 170.0;
const DIR_MAX: f64 = 120.0;
/// A pane's scale lifted, and under the pointer: 3 % and 1.5 % smaller.
const LIFT: f64 = 0.97;
const LIFT_HOVER: f64 = 0.985;
/// The lift and the set-down, in seconds; the capsule's float in.
const LIFT_SECS: f64 = 0.16;
const CAPSULE_SECS: f64 = 0.18;
const FLOAT: f64 = 6.0;

/// Which parts a capsule has. Narrowing a pane takes them away in one order:
/// the directory, then the name, then the split tools, and at the narrowest
/// only the hold and the close tool remain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Parts {
    pub grip: bool,
    pub status: bool,
    pub name: bool,
    pub dir: bool,
    pub splits: bool,
    pub new_tab: bool,
}

/// Where the parts stand, left edges in points from the capsule's left, and
/// its width.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Spans {
    pub width: f64,
    pub grip: Option<f64>,
    pub status: Option<f64>,
    pub name: Option<f64>,
    pub dir: Option<f64>,
    pub separator: Option<f64>,
    pub tools: Vec<(Tool, f64)>,
}

/// Lays `parts` out, with the name and the directory `name_w` and `dir_w`
/// wide.
pub(crate) fn spans(parts: Parts, name_w: f64, dir_w: f64) -> Spans {
    // Each part takes its width from `x` and returns where it began.
    fn put(x: &mut f64, width: f64) -> f64 {
        let at = *x;
        *x += width;
        at
    }
    let mut x = EDGE;
    let mut grip = None;
    if parts.grip {
        grip = Some(put(&mut x, GRIP_W));
        put(&mut x, GAP);
    }
    let mut status = None;
    if parts.status {
        status = Some(put(&mut x, STATUS_W));
        put(&mut x, GAP);
    }
    let mut name = None;
    let mut dir = None;
    if parts.name {
        name = Some(put(&mut x, name_w));
        if parts.dir {
            put(&mut x, TEXT_GAP);
            dir = Some(put(&mut x, dir_w));
        }
        put(&mut x, GAP);
    }
    let mut separator = None;
    if x > EDGE {
        separator = Some(put(&mut x, 1.0));
        put(&mut x, GAP);
    }
    let mut tools = Vec::new();
    let order = [
        (Tool::SplitRight, parts.splits),
        (Tool::SplitDown, parts.splits),
        (Tool::NewTab, parts.new_tab),
        (Tool::Close, true),
    ];
    for (tool, shown) in order {
        if shown {
            let at = put(&mut x, TOOL);
            put(&mut x, TOOL_GAP);
            tools.push((tool, at));
        }
    }
    Spans {
        width: x - TOOL_GAP + EDGE,
        grip,
        status,
        name,
        dir,
        separator,
        tools,
    }
}

/// The fullest capsule `room` points hold: the first of the narrowing
/// steps whose width fits, the narrowest if none does.
pub(crate) fn plan(room: f64, wants: Wants, name_w: f64, dir_w: f64) -> (Parts, Spans) {
    let steps = [
        (true, true, true, true, wants.new_tab),
        (true, true, false, true, wants.new_tab),
        (true, false, false, true, wants.new_tab),
        (true, false, false, false, wants.new_tab),
        (false, false, false, false, false),
    ];
    let mut last = None;
    for (status, name, dir, splits, new_tab) in steps {
        let parts = Parts {
            grip: wants.grip,
            status,
            name,
            dir,
            splits,
            new_tab,
        };
        let laid = spans(parts, name_w, dir_w);
        let fits = laid.width <= room;
        last = Some((parts, laid));
        if fits {
            break;
        }
    }
    // audit: the array above is not empty, so the loop ran at least once.
    last.expect("the narrowing steps are not empty")
}

/// Where a capsule `width` wide sits over a lifted pane: centred along its
/// top, `TOP` below it — and below `clear`, the lowest edge of whatever it
/// must not cover (a question's sheet, the search panel), when that is
/// beneath. Never lower than the pane's bottom allows. Returns the capsule's
/// top-left corner.
pub(crate) fn place(pane: Rect, width: f64, clear: Option<f64>) -> (f64, f64) {
    let x = pane.x + (pane.width - width) / 2.0;
    let mut y = pane.y + TOP;
    if let Some(edge) = clear {
        y = y.max(edge + TOP / 2.0);
    }
    let lowest = pane.y + pane.height - HEIGHT - TOP / 2.0;
    (x, y.min(lowest.max(pane.y)))
}

/// The lowest edge of `obstacle` if it overlaps a capsule at `x`, `width`
/// wide, along the way (the panel at the top right covers a narrow pane's
/// capsule; a wide pane's capsule sits clear of it).
pub(crate) fn below_if_over(obstacle: Rect, x: f64, width: f64) -> Option<f64> {
    let overlaps = x < obstacle.x + obstacle.width && obstacle.x < x + width;
    overlaps.then_some(obstacle.y + obstacle.height)
}

/// `rect` shrunk by `scale` about its centre.
pub(crate) fn shrunk(rect: Rect, scale: f64) -> Rect {
    let (width, height) = (rect.width * scale, rect.height * scale);
    Rect::new(
        rect.x + (rect.width - width) / 2.0,
        rect.y + (rect.height - height) / 2.0,
        width,
        height,
    )
}

fn ns_rect(rect: Rect) -> NSRect {
    NSRect::new(
        NSPoint::new(rect.x, rect.y),
        NSSize::new(rect.width, rect.height),
    )
}

fn rect_of(frame: NSRect) -> Rect {
    Rect::new(
        frame.origin.x,
        frame.origin.y,
        frame.size.width,
        frame.size.height,
    )
}

fn contains(rect: NSRect, point: NSPoint) -> bool {
    point.x >= rect.origin.x
        && point.y >= rect.origin.y
        && point.x < rect.origin.x + rect.size.width
        && point.y < rect.origin.y + rect.size.height
}

// ─── Colours ─────────────────────────────────────────────────────────────

/// The capsule's colours, from the theme's roles: the foreground at the
/// strengths a quiet surface takes, `dim` for what is secondary, `accent`
/// for work in progress and `info` for an ssh session.
#[derive(Clone, Copy, Debug)]
struct Colors {
    title: u32,
    dim: u32,
    accent: u32,
    info: u32,
    line: Tint,
    hover: Tint,
    track: Tint,
}

impl Colors {
    /// `contrast`: Increase Contrast is on — the capsule's line and fills
    /// are far stronger, so its edge reads without the blur's help.
    fn of(theme: &Theme, contrast: bool) -> Self {
        let (line, hover) = if contrast { (0.6, 0.22) } else { (0.14, 0.12) };
        Self {
            title: theme.foreground,
            dim: theme.dim,
            accent: theme.accent,
            info: theme.info,
            line: Tint::of(theme.foreground, line),
            hover: Tint::of(theme.foreground, hover),
            track: Tint::of(theme.accent, 0.25),
        }
    }
}

// ─── The face: line, hold, glyph, separator ──────────────────────────────

struct FaceIvars {
    colors: Colors,
    status: Status,
    spans: Spans,
    /// The pane the capsule is over: a press on the capsule takes it.
    pane: u64,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; Face implements no `Drop`
    // and is born with `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriArrangeFace"]
    #[ivars = FaceIvars]
    struct Face;

    unsafe impl NSObjectProtocol for Face {}

    impl Face {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        /// The capsule's own part, the tools aside: a press on the text or
        /// the hold is the capsule's, never the label's.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
            // SAFETY: reading the superview; we are on the main thread.
            let local = match unsafe { self.superview() } {
                Some(parent) => self.convertPoint_fromView(point, Some(&parent)),
                None => point,
            };
            if contains(self.bounds(), local) {
                let tool = self.subviews().iter().find(|view| {
                    view.downcast_ref::<ToolButton>().is_some() && contains(view.frame(), local)
                });
                Some(tool.unwrap_or_else(|| Retained::into_super(self.retain())))
            } else {
                None
            }
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        /// A press on the capsule — its hold or its text, not a tool — takes
        /// the pane: it is carried once the pointer travels
        /// ([`crate::pane_drag`]). The rest of the gesture is the carry's own.
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            if let Some(app) = app::delegate(self.mtm()) {
                app.pane_press(self.ivars().pane, event.locationInWindow());
            }
        }

        #[unsafe(method(resetCursorRects))]
        fn reset_cursor_rects(&self) {
            if let Some(at) = self.ivars().spans.grip {
                let rect = NSRect::new(
                    NSPoint::new(at - 2.0, 0.0),
                    NSSize::new(GRIP_W + 4.0, HEIGHT),
                );
                self.addCursorRect_cursor(rect, &NSCursor::openHandCursor());
            }
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let iv = self.ivars();
            let colors = iv.colors;
            tab_bar::rounded(
                self.bounds(),
                RADIUS,
                Tint::of(0x000000, 0.0),
                Some(colors.line),
            );
            let middle = HEIGHT / 2.0;
            if let Some(at) = iv.spans.grip {
                draw_grip(at, middle, Tint::of(colors.dim, 1.0));
            }
            if let Some(at) = iv.spans.status {
                draw_status(iv.status, at, middle, colors);
            }
            if let Some(at) = iv.spans.separator {
                let path = NSBezierPath::bezierPath();
                path.moveToPoint(NSPoint::new(at + 0.5, middle - 10.0));
                path.lineToPoint(NSPoint::new(at + 0.5, middle + 10.0));
                tab_bar::stroke(&path, 1.0, colors.line);
            }
        }
    }
);

/// Six dots in two columns: the hold.
pub(crate) fn draw_grip(left: f64, middle: f64, ink: Tint) {
    ink.color().setFill();
    for column in 0..2 {
        for row in 0..3 {
            let x = left + 4.5 + f64::from(column) * 6.0;
            let y = middle - 5.5 + f64::from(row) * 5.5;
            let dot = NSBezierPath::bezierPathWithOvalInRect(NSRect::new(
                NSPoint::new(x - 1.25, y - 1.25),
                NSSize::new(2.5, 2.5),
            ));
            dot.fill();
        }
    }
}

/// The glyph of a pane's state, in a `STATUS_W` square at `left`.
fn draw_status(status: Status, left: f64, middle: f64, colors: Colors) {
    let top = middle - STATUS_W / 2.0;
    let at = |x: f64, y: f64| NSPoint::new(left + x, top + y);
    match status {
        Status::Shell => {
            // `>_`.
            let path = NSBezierPath::bezierPath();
            path.moveToPoint(at(2.0, 3.5));
            path.lineToPoint(at(6.0, 7.0));
            path.lineToPoint(at(2.0, 10.5));
            path.moveToPoint(at(8.0, 11.0));
            path.lineToPoint(at(12.5, 11.0));
            tab_bar::stroke(&path, 1.5, Tint::of(colors.dim, 1.0));
        }
        Status::Running => {
            let centre = at(STATUS_W / 2.0, STATUS_W / 2.0);
            let radius = STATUS_W / 2.0 - 1.4;
            let track = NSBezierPath::bezierPathWithOvalInRect(NSRect::new(
                NSPoint::new(centre.x - radius, centre.y - radius),
                NSSize::new(2.0 * radius, 2.0 * radius),
            ));
            tab_bar::stroke(&track, 1.6, colors.track);
            // The view is flipped: −90° is the top, clockwise on screen.
            let arc = NSBezierPath::bezierPath();
            arc.appendBezierPathWithArcWithCenter_radius_startAngle_endAngle_clockwise(
                centre, radius, -90.0, 0.0, false,
            );
            tab_bar::stroke(&arc, 1.6, Tint::of(colors.accent, 1.0));
        }
        Status::Remote => {
            // ⇄.
            let path = NSBezierPath::bezierPath();
            path.moveToPoint(at(1.5, 4.5));
            path.lineToPoint(at(12.5, 4.5));
            path.moveToPoint(at(9.5, 2.0));
            path.lineToPoint(at(12.5, 4.5));
            path.lineToPoint(at(9.5, 7.0));
            path.moveToPoint(at(12.5, 9.5));
            path.lineToPoint(at(1.5, 9.5));
            path.moveToPoint(at(4.5, 7.0));
            path.lineToPoint(at(1.5, 9.5));
            path.lineToPoint(at(4.5, 12.0));
            tab_bar::stroke(&path, 1.4, Tint::of(colors.info, 1.0));
        }
        Status::Question => {
            let centre = at(STATUS_W / 2.0, STATUS_W / 2.0);
            let radius = STATUS_W / 2.0 - 1.4;
            let ring = NSBezierPath::bezierPathWithOvalInRect(NSRect::new(
                NSPoint::new(centre.x - radius, centre.y - radius),
                NSSize::new(2.0 * radius, 2.0 * radius),
            ));
            tab_bar::stroke(&ring, 1.6, Tint::of(colors.accent, 1.0));
            let dot = NSBezierPath::bezierPathWithOvalInRect(NSRect::new(
                NSPoint::new(centre.x - 1.4, centre.y - 1.4),
                NSSize::new(2.8, 2.8),
            ));
            Tint::of(colors.accent, 1.0).color().setFill();
            dot.fill();
        }
    }
}

// ─── A tool ──────────────────────────────────────────────────────────────

struct ToolIvars {
    tool: Tool,
    window: u64,
    pane: u64,
    colors: Colors,
    /// The pointer is over the tool: the fill.
    hot: Cell<bool>,
    /// A press began here; the release inside does the work.
    pressed: Cell<bool>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; ToolButton implements no
    // `Drop` and is born with `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriArrangeTool"]
    #[ivars = ToolIvars]
    struct ToolButton;

    unsafe impl NSObjectProtocol for ToolButton {}

    impl ToolButton {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let iv = self.ivars();
            if iv.hot.get() || iv.pressed.get() {
                let fill = if iv.pressed.get() {
                    Tint::of(iv.colors.title, 0.2)
                } else {
                    iv.colors.hover
                };
                tab_bar::rounded(self.bounds(), 8.0, fill, None);
            }
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(resetCursorRects))]
        fn reset_cursor_rects(&self) {
            self.addCursorRect_cursor(self.bounds(), &NSCursor::pointingHandCursor());
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            self.ivars().pressed.set(true);
            self.setNeedsDisplay(true);
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            let pressed = self.ivars().pressed.replace(false);
            self.setNeedsDisplay(true);
            let at = self.convertPoint_fromView(event.locationInWindow(), None);
            if pressed && contains(self.bounds(), at) {
                self.fire();
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
            self.fire();
            true
        }
    }
);

impl ToolButton {
    fn new(
        mtm: MainThreadMarker,
        tool: Tool,
        window: u64,
        pane: u64,
        colors: Colors,
        frame: NSRect,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ToolIvars {
            tool,
            window,
            pane,
            colors,
            hot: Cell::new(false),
            pressed: Cell::new(false),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        let tip = match tool.key() {
            Some(key) => format!("{}  {key}", tool.name()),
            None => tool.name().to_owned(),
        };
        this.setToolTip(Some(&NSString::from_str(&tip)));
        this.setAccessibilityElement(true);
        // SAFETY: AppKit's constant role string, alive for the process.
        this.setAccessibilityRole(Some(unsafe { NSAccessibilityButtonRole }));
        this.setAccessibilityLabel(Some(&NSString::from_str(tool.name())));
        tab_bar::track_hover(&this);
        let icon = NSImageView::new(mtm);
        icon.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), frame.size));
        icon.setImageScaling(NSImageScaling::ScaleNone);
        icon.setContentTintColor(Some(&Tint::of(colors.title, 0.85).color()));
        if let Some(image) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
            &NSString::from_str(tool.symbol()),
            Some(&NSString::from_str(tool.name())),
        ) {
            icon.setImage(Some(&image));
        }
        this.addSubview(&icon);
        this
    }

    /// The tool was pressed: the work is the application's, a turn later —
    /// it takes the capsule itself away.
    fn fire(&self) {
        let iv = self.ivars();
        let (window, pane, tool) = (iv.window, iv.pane, iv.tool);
        after(Duration::from_millis(1), move |app| {
            app.arrange_act(window, pane, tool);
        });
    }
}

// ─── The overlay ─────────────────────────────────────────────────────────

define_class!(
    // SAFETY: NSView is designed for subclassing; Overlay implements no
    // `Drop` and has no ivars.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriArrangeOverlay"]
    #[ivars = ()]
    struct Overlay;

    unsafe impl NSObjectProtocol for Overlay {}

    impl Overlay {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        /// Only the capsules take a press; everywhere else the pane under
        /// the overlay is hit (and `BateriView` keeps the press from its
        /// program while ⌥⌘ is down).
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
            // SAFETY: reading the superview; we are on the main thread.
            let local = match unsafe { self.superview() } {
                Some(parent) => self.convertPoint_fromView(point, Some(&parent)),
                None => point,
            };
            let pills: Vec<_> = self.subviews().iter().collect();
            pills.into_iter().rev().find_map(|pill| {
                if contains(pill.frame(), local) {
                    pill.hitTest(local)
                } else {
                    None
                }
            })
        }

        /// An open hand over the whole tab: the panes are being held.
        #[unsafe(method(resetCursorRects))]
        fn reset_cursor_rects(&self) {
            self.addCursorRect_cursor(self.bounds(), &NSCursor::openHandCursor());
        }
    }
);

impl Overlay {
    fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: `initWithFrame:` is NSView's designated initializer.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        this.setWantsLayer(true);
        this.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        this
    }
}

// ─── Building a capsule and a plate ──────────────────────────────────────

/// A label that never takes a press: the face under it decides.
pub(crate) fn label(
    mtm: MainThreadMarker,
    text: &str,
    size: f64,
    medium: bool,
    ink: Tint,
) -> Retained<NSTextField> {
    let field = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    let font = if medium {
        // SAFETY: AppKit's constant font weight.
        NSFont::systemFontOfSize_weight(size, unsafe { NSFontWeightMedium })
    } else {
        NSFont::systemFontOfSize(size)
    };
    field.setFont(Some(&font));
    field.setTextColor(Some(&ink.color()));
    field.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    field.setMaximumNumberOfLines(1);
    field
}

/// A capsule for `entry`, in `room` points of width: the view and its size.
/// Its tools name `window` and the pane.
fn capsule(
    mtm: MainThreadMarker,
    entry: &Entry,
    window: u64,
    theme: &Theme,
    room: f64,
    wants: Wants,
) -> (Retained<NSView>, NSSize) {
    let colors = Colors::of(
        theme,
        NSWorkspace::sharedWorkspace().accessibilityDisplayShouldIncreaseContrast(),
    );
    let name = label(mtm, &entry.name, 12.5, true, Tint::of(colors.title, 1.0));
    let dir = label(mtm, &entry.dir, 11.5, false, Tint::of(colors.dim, 1.0));
    let name_w = name.fittingSize().width.ceil().min(NAME_MAX);
    let dir_w = if entry.dir.is_empty() {
        0.0
    } else {
        dir.fittingSize().width.ceil().min(DIR_MAX)
    };
    let (parts, laid) = plan(room, wants, name_w, dir_w);
    let size = NSSize::new(laid.width, HEIGHT);
    let bounds = NSRect::new(NSPoint::new(0.0, 0.0), size);

    let pill = NSView::initWithFrame(NSView::alloc(mtm), bounds);
    pill.setWantsLayer(true);
    let shadow = NSShadow::new();
    shadow.setShadowBlurRadius(16.0);
    shadow.setShadowOffset(NSSize::new(0.0, -4.0));
    shadow.setShadowColor(Some(&NSColor::colorWithSRGBRed_green_blue_alpha(
        0.0, 0.0, 0.0, 0.32,
    )));
    pill.setShadow(Some(&shadow));

    let effect = NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(mtm), bounds);
    effect.setMaterial(NSVisualEffectMaterial::Popover);
    effect.setBlendingMode(NSVisualEffectBlendingMode::WithinWindow);
    effect.setState(NSVisualEffectState::Active);
    effect.setWantsLayer(true);
    if let Some(layer) = effect.layer() {
        layer.setCornerRadius(RADIUS);
        layer.setMasksToBounds(true);
    }
    pill.addSubview(&effect);

    let face = Face::alloc(mtm).set_ivars(FaceIvars {
        colors,
        status: entry.status,
        spans: laid.clone(),
        pane: entry.pane,
    });
    // SAFETY: `initWithFrame:` is NSView's designated initializer and the
    // ivars are set.
    let face: Retained<Face> = unsafe { msg_send![super(face), initWithFrame: bounds] };
    pill.addSubview(&face);

    let text = |field: &NSTextField, at: Option<f64>, width: f64| {
        let Some(at) = at else {
            return;
        };
        let height = field.fittingSize().height.ceil();
        field.setFrame(NSRect::new(
            NSPoint::new(at, ((HEIGHT - height) / 2.0).floor()),
            NSSize::new(width, height),
        ));
        face.addSubview(field);
    };
    text(&name, laid.name, name_w);
    text(&dir, laid.dir, dir_w);
    for (tool, at) in &laid.tools {
        let frame = NSRect::new(
            NSPoint::new(*at, (HEIGHT - TOOL) / 2.0),
            NSSize::new(TOOL, TOOL),
        );
        face.addSubview(&ToolButton::new(
            mtm, *tool, window, entry.pane, colors, frame,
        ));
    }
    let _ = parts;
    (pill, size)
}

/// The rounded fill behind a lifted pane, which casts the shadow the pane's
/// own clipped layer cannot.
fn plate(mtm: MainThreadMarker, theme: &Theme, frame: NSRect) -> Retained<NSBox> {
    let plate = NSBox::new(mtm);
    plate.setWantsLayer(true);
    plate.setBoxType(NSBoxType::Custom);
    plate.setTitlePosition(NSTitlePosition::NoTitle);
    plate.setBorderWidth(0.0);
    plate.setCornerRadius(corner_pt());
    plate.setFillColor(&Tint::of(theme.background, 1.0).color());
    let shadow = NSShadow::new();
    shadow.setShadowBlurRadius(22.0);
    shadow.setShadowOffset(NSSize::new(0.0, -7.0));
    shadow.setShadowColor(Some(&NSColor::colorWithSRGBRed_green_blue_alpha(
        0.0, 0.0, 0.0, 0.42,
    )));
    plate.setShadow(Some(&shadow));
    plate.setFrame(frame);
    plate.setAlphaValue(0.0);
    plate
}

/// Runs `change` as an AppKit animation of `secs` on the app's curve
/// (`cubic-bezier(0.2, 0.8, 0.2, 1)`), then `done`.
pub(crate) fn animate(secs: f64, change: impl Fn() + 'static, done: impl Fn() + 'static) {
    let changes = RcBlock::new(move |context: NonNull<NSAnimationContext>| {
        // SAFETY: AppKit gives the block a live context, for the block's duration.
        let context = unsafe { context.as_ref() };
        context.setDuration(secs);
        let curve = CAMediaTimingFunction::functionWithControlPoints(0.2, 0.8, 0.2, 1.0);
        context.setTimingFunction(Some(&curve));
        change();
    });
    let finished = RcBlock::new(done);
    NSAnimationContext::runAnimationGroup_completionHandler(&changes, Some(&finished));
}

// ─── What the tab lifted ─────────────────────────────────────────────────

/// What `Raised::raise` is told.
pub(crate) struct Scene<'a> {
    pub window: u64,
    pub tab: u64,
    pub container: &'a SplitView,
    pub panes: &'a [Retained<TerminalPane>],
    pub entries: Vec<Entry>,
    pub theme: Theme,
    pub wants: Wants,
    /// Reduce Motion: nothing shrinks, nothing floats.
    pub still: bool,
    /// The pointer in the container's coordinates, if it is inside.
    pub pointer: Option<NSPoint>,
}

/// A tab lifted for arranging: its overlay of capsules, the plates under its
/// panes, the panes it forced to read as cards, and the pointer watch.
pub(crate) struct Raised {
    overlay: Retained<Overlay>,
    plates: Vec<Retained<NSBox>>,
    forced: Vec<u64>,
    hover: Cell<Option<u64>>,
    still: bool,
    monitor: RefCell<Option<Retained<AnyObject>>>,
}

impl Raised {
    /// Lifts the tab: plates, panes, capsules, the pointer watch.
    pub(crate) fn raise(mtm: MainThreadMarker, scene: Scene<'_>) -> Self {
        let Scene {
            window,
            tab,
            container,
            panes,
            entries,
            theme,
            wants,
            still,
            pointer,
        } = scene;
        let overlay = Overlay::new(mtm, container.bounds());
        let scale = container.scale();
        let sheet = sheets::question_bottom(container);
        let hover = pointer.and_then(|at| pane_at(panes, at));
        let mut plates = Vec::new();
        let mut forced = Vec::new();
        let mut floats: Vec<(Retained<NSView>, NSPoint)> = Vec::new();

        for pane in panes.iter().filter(|pane| !pane.isHidden()) {
            let frame = pane.frame();
            let at = rect_of(frame);
            if !pane.is_card() {
                pane.set_card(true, scale);
                forced.push(pane.id());
            }
            pane.set_raised(true);

            let shrink = if still {
                1.0
            } else if hover == Some(pane.id()) {
                LIFT_HOVER
            } else {
                LIFT
            };
            // At the smallest lift, whatever the pointer: no pane look is
            // smaller, so no lift or set-down shows the plate past the frame.
            let plate = plate(
                mtm,
                &theme,
                ns_rect(shrunk(at, if still { 1.0 } else { LIFT })),
            );
            let below: &NSView = pane;
            container.addSubview_positioned_relativeTo(
                &plate,
                NSWindowOrderingMode::Below,
                Some(below),
            );
            if let Some(layer) = pane.layer() {
                card::lift(&layer, at, shrink, if still { 0.0 } else { LIFT_SECS });
            }
            plates.push(plate);

            let Some(entry) = entries.iter().find(|entry| entry.pane == pane.id()) else {
                continue;
            };
            let lifted = shrunk(at, LIFT);
            let (pill, size) = capsule(
                mtm,
                entry,
                window,
                &theme,
                lifted.width - 2.0 * TOP - 8.0,
                wants,
            );
            // The sheet and the search panel cover the top: the capsule
            // slides below whichever it would meet.
            let x = lifted.x + (lifted.width - size.width) / 2.0;
            let mut clear = sheet;
            if let Some(search) = pane.search_frame() {
                let over = container.convertRect_fromView(search, Some(pane));
                if let Some(edge) = below_if_over(rect_of(over), x, size.width) {
                    clear = Some(clear.map_or(edge, |clear| clear.max(edge)));
                }
            }
            let (x, y) = place(lifted, size.width, clear);
            pill.setFrameOrigin(NSPoint::new(x, y + if still { 0.0 } else { FLOAT }));
            pill.setAlphaValue(if still { 1.0 } else { 0.0 });
            overlay.addSubview(&pill);
            floats.push((pill, NSPoint::new(x, y)));
        }
        container.addSubview(&overlay);
        if let Some(window) = container.window() {
            window.invalidateCursorRectsForView(&overlay);
        }

        let fade_in = plates.clone();
        if still {
            for plate in &fade_in {
                plate.setAlphaValue(1.0);
            }
        } else {
            animate(
                CAPSULE_SECS,
                move || {
                    for plate in &fade_in {
                        plate.animator().setAlphaValue(1.0);
                    }
                    for (pill, to) in &floats {
                        pill.animator().setAlphaValue(1.0);
                        pill.animator().setFrameOrigin(*to);
                    }
                },
                || {},
            );
        }
        Self {
            overlay,
            plates,
            forced,
            hover: Cell::new(hover),
            still,
            monitor: RefCell::new(watch_pointer(tab)),
        }
    }

    /// The pointer moved over the container, at `at` in its coordinates: the
    /// pane under it lifts a little less than the others.
    pub(crate) fn point(&self, panes: &[Retained<TerminalPane>], at: NSPoint) {
        if self.still {
            return;
        }
        let now = pane_at(panes, at);
        let before = self.hover.replace(now);
        if before == now {
            return;
        }
        for (id, scale) in [(before, LIFT), (now, LIFT_HOVER)] {
            let Some(id) = id else {
                continue;
            };
            let Some(pane) = panes.iter().find(|pane| pane.id() == id) else {
                continue;
            };
            if let Some(layer) = pane.layer() {
                card::lift(&layer, rect_of(pane.frame()), scale, 0.12);
            }
        }
    }

    /// Sets the panes down: they grow back, the plates and capsules go
    /// (with a fade when `animate`).
    pub(crate) fn lower(
        self,
        container: &SplitView,
        panes: &[Retained<TerminalPane>],
        animate: bool,
    ) {
        if let Some(token) = self.monitor.take() {
            // SAFETY: the token is the one `addLocalMonitor…` gave.
            unsafe { NSEvent::removeMonitor(&token) };
        }
        let scale = container.scale();
        let secs = if animate && !self.still {
            LIFT_SECS
        } else {
            0.0
        };
        for pane in panes {
            if let Some(layer) = pane.layer() {
                card::lift(&layer, rect_of(pane.frame()), 1.0, secs);
            }
            pane.set_raised(false);
            if self.forced.contains(&pane.id()) && !container.carded() {
                pane.set_card(false, scale);
            }
        }
        let views: Vec<Retained<NSView>> = self
            .plates
            .iter()
            .map(|plate| Retained::into_super(plate.clone()))
            .chain([Retained::into_super(self.overlay.clone())])
            .collect();
        if secs <= 0.0 {
            for view in &views {
                view.removeFromSuperview();
            }
        } else {
            let fading = views.clone();
            animate_out(secs, fading, views);
        }
        if let Some(window) = container.window() {
            window.invalidateCursorRectsForView(container);
        }
    }
}

/// Fades `fading` out and then takes `views` out of their superview.
fn animate_out(secs: f64, fading: Vec<Retained<NSView>>, views: Vec<Retained<NSView>>) {
    animate(
        secs,
        move || {
            for view in &fading {
                view.animator().setAlphaValue(0.0);
            }
        },
        move || {
            for view in &views {
                view.removeFromSuperview();
            }
        },
    );
}

impl Drop for Raised {
    fn drop(&mut self) {
        // A tab that goes away lifted: the watch must not outlive it.
        if let Some(token) = self.monitor.take() {
            // SAFETY: the token is the one `addLocalMonitor…` gave.
            unsafe { NSEvent::removeMonitor(&token) };
        }
    }
}

/// The pane whose frame holds `at` (the container's coordinates).
fn pane_at(panes: &[Retained<TerminalPane>], at: NSPoint) -> Option<u64> {
    panes
        .iter()
        .filter(|pane| !pane.isHidden())
        .find(|pane| contains(pane.frame(), at))
        .map(|pane| pane.id())
}

/// The pointer watch of a lifted tab: a local monitor of mouse motion,
/// there only while the panes are up.
fn watch_pointer(tab: u64) -> Option<Retained<AnyObject>> {
    let block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
        // audit: a local monitor runs on the main thread, before `sendEvent:`.
        let mtm = MainThreadMarker::new().expect("a local event monitor runs on the main thread");
        if let Some(tab) = app::delegate(mtm).and_then(|app| app.tab(tab)) {
            // SAFETY: AppKit gives the monitor a valid event.
            tab.arrange_pointer(unsafe { event.as_ref() });
        }
        event.as_ptr()
    });
    // SAFETY: the block returns the valid event it was given.
    unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::MouseMoved, &block)
    }
}

/// A working directory as the capsule says it: the home directory as `~`.
pub(crate) fn abbreviate(path: &str, home: Option<&str>) -> String {
    match home.filter(|home| !home.is_empty() && *home != "/") {
        Some(home) if path == home => "~".to_owned(),
        Some(home) => match path.strip_prefix(home) {
            Some(rest) if rest.starts_with('/') => format!("~{rest}"),
            _ => path.to_owned(),
        },
        None => path.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WANTS: Wants = Wants {
        grip: true,
        new_tab: true,
    };

    #[test]
    fn the_holds_are_command_alone_and_option_command_alone() {
        let command = NSEventModifierFlags::Command;
        let option = NSEventModifierFlags::Option;
        assert_eq!(hold_of(command), Hold::Command);
        assert_eq!(hold_of(command | option), Hold::Arrange);
        // Caps Lock, the keypad and the function key are not chords.
        assert_eq!(
            hold_of(command | NSEventModifierFlags::CapsLock),
            Hold::Command
        );
        assert_eq!(
            hold_of(command | option | NSEventModifierFlags::CapsLock),
            Hold::Arrange
        );
        assert_eq!(
            hold_of(command | option | NSEventModifierFlags::Function),
            Hold::Arrange
        );
        // A third modifier is a shortcut of its own.
        assert_eq!(
            hold_of(command | option | NSEventModifierFlags::Shift),
            Hold::Nothing
        );
        assert_eq!(
            hold_of(command | option | NSEventModifierFlags::Control),
            Hold::Nothing
        );
        assert_eq!(hold_of(option), Hold::Nothing);
        assert_eq!(hold_of(NSEventModifierFlags::empty()), Hold::Nothing);
    }

    #[test]
    fn a_press_with_option_and_command_is_the_arrangements() {
        let both = NSEventModifierFlags::Command | NSEventModifierFlags::Option;
        assert!(swallows(both));
        assert!(swallows(both | NSEventModifierFlags::Shift));
        assert!(!swallows(NSEventModifierFlags::Command));
        assert!(!swallows(NSEventModifierFlags::Option));
        // The link's key is ⌘ alone of the two.
        assert!(link_command(NSEventModifierFlags::Command));
        assert!(!link_command(both));
        assert!(!link_command(NSEventModifierFlags::empty()));
    }

    #[test]
    fn a_wide_pane_keeps_the_whole_capsule() {
        let (parts, laid) = plan(2000.0, WANTS, 80.0, 60.0);
        assert!(parts.grip && parts.status && parts.name && parts.dir);
        assert!(parts.splits && parts.new_tab);
        assert_eq!(laid.tools.len(), 4);
        assert!(laid.separator.is_some());
    }

    #[test]
    fn narrowing_takes_the_directory_then_the_name_then_the_splits() {
        // The width of each step, from the fullest.
        let widths: Vec<f64> = (0..5)
            .map(|step| {
                let parts = Parts {
                    grip: true,
                    status: step < 4,
                    name: step < 2,
                    dir: step < 1,
                    splits: step < 3,
                    new_tab: step < 4,
                };
                spans(parts, 80.0, 60.0).width
            })
            .collect();
        assert!(
            widths.windows(2).all(|pair| pair[0] > pair[1]),
            "{widths:?}"
        );
        let (parts, _) = plan(widths[0], WANTS, 80.0, 60.0);
        assert!(parts.dir && parts.name);
        let (parts, _) = plan(widths[0] - 1.0, WANTS, 80.0, 60.0);
        assert!(!parts.dir && parts.name && parts.splits);
        let (parts, _) = plan(widths[1] - 1.0, WANTS, 80.0, 60.0);
        assert!(!parts.name && parts.status && parts.splits);
        let (parts, laid) = plan(widths[2] - 1.0, WANTS, 80.0, 60.0);
        assert!(!parts.splits && parts.status && parts.new_tab);
        assert!(
            laid.tools
                .iter()
                .all(|(tool, _)| !matches!(tool, Tool::SplitRight | Tool::SplitDown))
        );
        let (parts, _) = plan(widths[3] - 1.0, WANTS, 80.0, 60.0);
        assert!(!parts.status && !parts.new_tab);
    }

    #[test]
    fn the_narrowest_capsule_is_the_hold_and_the_close_tool() {
        let (parts, laid) = plan(0.0, WANTS, 80.0, 60.0);
        assert!(parts.grip);
        assert!(!parts.status && !parts.name && !parts.dir && !parts.splits && !parts.new_tab);
        assert_eq!(laid.tools.len(), 1);
        assert_eq!(laid.tools[0].0, Tool::Close);
        assert!(laid.grip.is_some());
        // Without the hold only the close tool remains, and no separator.
        let (_, alone) = plan(
            0.0,
            Wants {
                grip: false,
                new_tab: false,
            },
            80.0,
            60.0,
        );
        assert_eq!(alone.tools.len(), 1);
        assert!(alone.grip.is_none() && alone.separator.is_none());
    }

    #[test]
    fn a_split_tab_only_offers_to_open_in_a_new_tab() {
        let (_, laid) = plan(
            2000.0,
            Wants {
                grip: true,
                new_tab: false,
            },
            80.0,
            60.0,
        );
        assert!(laid.tools.iter().all(|(tool, _)| *tool != Tool::NewTab));
        assert_eq!(laid.tools.len(), 3);
    }

    #[test]
    fn the_parts_tile_left_to_right_inside_the_width() {
        let (_, laid) = plan(2000.0, WANTS, 80.0, 60.0);
        let mut at = vec![laid.grip.unwrap(), laid.status.unwrap(), laid.name.unwrap()];
        at.push(laid.dir.unwrap());
        at.push(laid.separator.unwrap());
        at.extend(laid.tools.iter().map(|(_, x)| *x));
        assert!(at.windows(2).all(|pair| pair[0] < pair[1]), "{at:?}");
        let last = laid.tools.last().unwrap().1;
        assert_eq!(laid.width, last + TOOL + EDGE);
    }

    #[test]
    fn the_capsule_sits_at_the_top_middle_and_below_what_it_must_not_cover() {
        let pane = Rect::new(100.0, 50.0, 600.0, 400.0);
        let (x, y) = place(pane, 300.0, None);
        assert_eq!((x, y), (250.0, 50.0 + TOP));
        // Below a sheet that ends under the capsule's top, not above it.
        let (_, lower) = place(pane, 300.0, Some(120.0));
        assert_eq!(lower, 120.0 + TOP / 2.0);
        let (_, same) = place(pane, 300.0, Some(10.0));
        assert_eq!(same, y);
        // Never past the bottom of a short pane.
        let short = Rect::new(0.0, 0.0, 600.0, 60.0);
        let (_, fits) = place(short, 300.0, Some(500.0));
        assert!(fits <= 60.0 - HEIGHT);
    }

    #[test]
    fn the_search_panel_is_met_only_by_a_capsule_that_reaches_it() {
        let panel = Rect::new(400.0, 10.0, 200.0, 36.0);
        // A narrow pane's capsule reaches across the panel's columns.
        assert_eq!(below_if_over(panel, 350.0, 120.0), Some(46.0));
        // A wide pane's sits clear to its left.
        assert_eq!(below_if_over(panel, 100.0, 200.0), None);
        assert_eq!(below_if_over(panel, 600.0, 100.0), None);
    }

    #[test]
    fn a_lifted_rect_shrinks_about_its_centre() {
        let look = shrunk(Rect::new(0.0, 0.0, 200.0, 100.0), 0.97);
        assert!((look.width - 194.0).abs() < 1e-9 && (look.height - 97.0).abs() < 1e-9);
        assert!((look.x - 3.0).abs() < 1e-9 && (look.y - 1.5).abs() < 1e-9);
        assert_eq!(
            shrunk(Rect::new(5.0, 6.0, 7.0, 8.0), 1.0),
            Rect::new(5.0, 6.0, 7.0, 8.0)
        );
    }

    #[test]
    fn the_home_directory_is_a_tilde() {
        let home = Some("/Users/a");
        assert_eq!(abbreviate("/Users/a", home), "~");
        assert_eq!(abbreviate("/Users/a/Projects/x", home), "~/Projects/x");
        // A sibling that merely starts with the same letters is not inside.
        assert_eq!(abbreviate("/Users/ab/x", home), "/Users/ab/x");
        assert_eq!(abbreviate("/tmp", home), "/tmp");
        assert_eq!(abbreviate("/tmp", None), "/tmp");
        assert_eq!(abbreviate("/tmp", Some("/")), "/tmp");
    }
}
