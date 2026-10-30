//! A pane's header in a split tab: the band [`crate::card::HEAD_PT`] tall over every pane, part
//! of its card. The program's mark with no tile — a dot on its corner says the pane's own
//! signals —, the pane's title (the focused pane's in full ink), and under the pointer the
//! pane's tools at its right: Split Right, Split Down, Open in New Tab, Close Split. A press on
//! the band gives the pane the keyboard and is the start of carrying it: past the slop the pane
//! is carried (`pane_drag`, no key held) — to a region of a pane, onto a strip, out of the
//! window. A press on a tool runs that command on this pane.
//!
//! **One band, the container's.** The header is not the pane's: the split container lays it out
//! over the pane's place (`split::Spacing::with_head`, so the layout, the split limit, a drag
//! and a resize all count it) and the pane starts under it, with its own margin. On a card the
//! header takes the card's top corners and its light, the pane the bottom ones; the focus frame
//! runs round both, the header drawing its top half.
//!
//! A child of `tab_bar` for the bar's mark and indicator drawing, which a sibling could not
//! reach.

use std::cell::{Cell, RefCell};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{NSAccessibility, NSApplication, NSBezierPath, NSColor, NSEvent, NSView};
use objc2_foundation::{NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};

use super::{Palette, ProgramKind, Tint, draw_mark, track_hover};
use crate::tabs::{self, Indicator};

/// The mark's side, points: the program's own glyph, no tile.
const MARK: f64 = 14.0;
/// The status dot on the mark's corner, and the ring of the ground cut round it.
const DOT: f64 = 5.0;
const DOT_RING: f64 = 1.0;
/// A tool's box, and the space between two.
const TOOL_WIDTH: f64 = 22.0;
const TOOL_HEIGHT: f64 = 18.0;
const TOOL_GAP: f64 = 2.0;
/// The band's inner margin above and below its parts: the terminal's gutter, so the mark stands
/// on the grid's left line and the pane's text starts the same distance under the band.
const GUTTER: f64 = 8.0;
/// The title's type, points: a row's.
const TITLE_TEXT: f64 = 12.5;

/// A pane's tools, in the order they stand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tool {
    SplitRight,
    SplitDown,
    NewTab,
    Close,
}

impl Tool {
    const ALL: [Tool; 4] = [Tool::SplitRight, Tool::SplitDown, Tool::NewTab, Tool::Close];

    fn name(self) -> &'static str {
        match self {
            Tool::SplitRight => "Split Right",
            Tool::SplitDown => "Split Down",
            Tool::NewTab => "Open in New Tab",
            Tool::Close => "Close Split",
        }
    }

    /// The command it runs on the pane once the pane has the keyboard: the menu's own action.
    fn action(self) -> objc2::runtime::Sel {
        match self {
            Tool::SplitRight => sel!(splitRight:),
            Tool::SplitDown => sel!(splitDown:),
            Tool::NewTab => sel!(movePaneToNewTab:),
            Tool::Close => sel!(closeTab:),
        }
    }
}

/// What the header shows ([`PaneHead::set`]).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HeadLook {
    pub(crate) kind: ProgramKind,
    pub(crate) title: String,
    pub(crate) indicator: Option<Indicator>,
    pub(crate) focused: bool,
    /// The pane is a card: the band takes its top corners and its light.
    pub(crate) carded: bool,
    /// The focus frame runs round the pane (`[appearance] focus_frame`): the band draws its top
    /// half, in this colour.
    pub(crate) frame: Option<[u8; 3]>,
}

pub(crate) struct HeadIvars {
    pane: Cell<u64>,
    look: RefCell<Option<HeadLook>>,
    palette: Cell<Option<Palette>>,
    /// The theme's background, the band's ground, and whether it is dark (the card's light).
    ground: Cell<u32>,
    dark: Cell<bool>,
    /// The card's corner radius, points.
    radius: Cell<f64>,
    /// The grid's left and right margins under it (`CellMetrics::with_corners`): the mark stands
    /// on the first, the tools end at the second.
    margins: Cell<(f64, f64)>,
    hot: Cell<bool>,
    hot_tool: Cell<Option<Tool>>,
    pressed: Cell<Option<Tool>>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; PaneHead implements no `Drop` and is born with
    // `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriPaneHead"]
    #[ivars = HeadIvars]
    pub(crate) struct PaneHead;

    unsafe impl NSObjectProtocol for PaneHead {}

    impl PaneHead {
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

        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            self.ivars().hot.set(true);
            self.setNeedsDisplay(true);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            self.ivars().hot.set(false);
            self.ivars().hot_tool.set(None);
            self.setNeedsDisplay(true);
        }

        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, event: &NSEvent) {
            let at = self.convertPoint_fromView(event.locationInWindow(), None);
            let tool = self.tool_at(at);
            if self.ivars().hot_tool.replace(tool) != tool {
                let tip = tool.map(|tool| NSString::from_str(tool.name()));
                self.setToolTip(tip.as_deref());
                self.setNeedsDisplay(true);
            }
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let at = self.convertPoint_fromView(event.locationInWindow(), None);
            let tool = self.tool_at(at);
            self.ivars().pressed.set(tool);
            self.focus_pane();
            // Off the tools the press may become a carry: the session watches the pointer from
            // here and takes over once it travels.
            if tool.is_none()
                && let Some(app) = crate::app::delegate(self.mtm())
            {
                app.pane_press(self.pane(), event.locationInWindow());
            }
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            let at = self.convertPoint_fromView(event.locationInWindow(), None);
            if let Some(tool) = self.ivars().pressed.take()
                && self.tool_at(at) == Some(tool)
            {
                self.run(tool);
            }
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            self.draw();
        }
    }
);

impl PaneHead {
    pub(crate) fn new(mtm: MainThreadMarker, pane: u64) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(HeadIvars {
            pane: Cell::new(pane),
            look: RefCell::new(None),
            palette: Cell::new(None),
            ground: Cell::new(0),
            dark: Cell::new(true),
            radius: Cell::new(0.0),
            margins: Cell::new((GUTTER, GUTTER)),
            hot: Cell::new(false),
            hot_tool: Cell::new(None),
            pressed: Cell::new(None),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        track_hover(&this);
        this
    }

    /// The pane the header stands over.
    pub(crate) fn pane(&self) -> u64 {
        self.ivars().pane.get()
    }

    /// The grid's margins under it, from the pane's corners (the container's layout).
    pub(crate) fn set_margins(&self, margins: (f64, f64)) {
        if self.ivars().margins.replace(margins) != margins {
            self.setNeedsDisplay(true);
        }
    }

    /// What it shows, in the theme's colours; redrawn only when something changed.
    pub(crate) fn set(&self, look: HeadLook, theme: &bt_core::Theme, radius: f64) {
        let iv = self.ivars();
        let palette = Palette::of(theme);
        let changed = iv.look.borrow().as_ref() != Some(&look)
            || iv.palette.replace(Some(palette)) != Some(palette)
            || iv.ground.replace(theme.background) != theme.background
            || iv.dark.replace(theme.is_dark()) != theme.is_dark()
            || iv.radius.replace(radius) != radius;
        let tip = NSString::from_str(&look.title);
        self.setAccessibilityLabel(Some(&tip));
        iv.look.replace(Some(look));
        if changed {
            self.setNeedsDisplay(true);
        }
    }

    /// The tools' boxes, right to left from the grid's right margin.
    fn tools(&self) -> Vec<(Tool, NSRect)> {
        let width = self.bounds().size.width;
        let count = Tool::ALL.len() as f64;
        let all = count * TOOL_WIDTH + (count - 1.0) * TOOL_GAP;
        let start = width - self.ivars().margins.get().1 - all;
        Tool::ALL
            .iter()
            .enumerate()
            .map(|(index, &tool)| {
                let x = start + index as f64 * (TOOL_WIDTH + TOOL_GAP);
                (
                    tool,
                    NSRect::new(
                        NSPoint::new(x, GUTTER),
                        NSSize::new(TOOL_WIDTH, TOOL_HEIGHT),
                    ),
                )
            })
            .collect()
    }

    /// The tool under `at`, while the tools show (the pointer is on the band).
    fn tool_at(&self, at: NSPoint) -> Option<Tool> {
        self.ivars().look.borrow().as_ref()?;
        self.tools().into_iter().find_map(|(tool, rect)| {
            let inside = at.x >= rect.origin.x
                && at.x <= rect.origin.x + rect.size.width
                && at.y >= rect.origin.y
                && at.y <= rect.origin.y + rect.size.height;
            inside.then_some(tool)
        })
    }

    /// The pane under the band takes the keyboard.
    fn focus_pane(&self) {
        let pane = self.pane();
        // SAFETY: reading the superview; we are on the main thread.
        let container = unsafe { self.superview() }
            .and_then(|view| view.downcast::<crate::split_view::SplitView>().ok());
        if let Some(pane) = container.and_then(|container| container.pane(pane)) {
            // Through the pane's own gate, as a click in it does.
            let _ = pane.focus();
        }
    }

    /// Runs `tool` on the pane: it has the keyboard from the press, so the menu's own action
    /// down the responder chain acts on it.
    fn run(&self, tool: Tool) {
        let app = NSApplication::sharedApplication(self.mtm());
        let sender: &AnyObject = self;
        // SAFETY: every tool's action takes one optional sender argument.
        unsafe { app.sendAction_to_from(tool.action(), None, Some(sender)) };
        self.ivars().hot_tool.set(None);
        self.setNeedsDisplay(true);
    }

    fn draw(&self) {
        let iv = self.ivars();
        let look = iv.look.borrow();
        let (Some(look), Some(palette)) = (look.as_ref(), iv.palette.get()) else {
            return;
        };
        let bounds = self.bounds();
        let radius = if look.carded { iv.radius.get() } else { 0.0 };
        // The band's ground: the pane's own, the card's top corners rounded.
        let band = top_rounded(bounds, radius);
        Tint::of(iv.ground.get(), 1.0).color().setFill();
        band.fill();
        if look.carded {
            // The card's light on its top edge, as the pane's frame had it without a header.
            let (tone, alpha) = match (iv.dark.get(), look.focused) {
                (true, true) => (0xfff6e8, 0.32),
                (true, false) => (0xffffff, 0.13),
                (false, _) => (0xffffff, 1.0),
            };
            let edge = top_rounded(bounds, radius);
            let below = NSRect::new(
                NSPoint::new(bounds.origin.x, bounds.origin.y + 1.0),
                bounds.size,
            );
            edge.appendBezierPath(&top_rounded(below, radius));
            edge.setWindingRule(objc2_app_kit::NSWindingRule::EvenOdd);
            Tint::of(tone, alpha).color().setFill();
            edge.fill();
        }
        // The mark on the grid's left line, the dot on its corner.
        let ink = if look.focused {
            palette.title
        } else {
            palette.dim
        };
        let margins = iv.margins.get();
        let mark = NSRect::new(
            NSPoint::new(margins.0, GUTTER + (TOOL_HEIGHT - MARK) / 2.0),
            NSSize::new(MARK, MARK),
        );
        draw_mark(look.kind, mark, &Tint::of(ink, 1.0).color());
        if let Some(indicator) = look.indicator {
            let tone = palette.tone(tabs::list_dot(indicator), 0);
            let centre = NSPoint::new(mark.origin.x + MARK - DOT / 2.0, mark.origin.y + DOT / 2.0);
            for (side, color) in [(DOT + 2.0 * DOT_RING, iv.ground.get()), (DOT, tone)] {
                Tint::of(color, 1.0).color().setFill();
                NSBezierPath::bezierPathWithOvalInRect(NSRect::new(
                    NSPoint::new(centre.x - side / 2.0, centre.y - side / 2.0),
                    NSSize::new(side, side),
                ))
                .fill();
            }
        }
        // The title after the mark; while the tools show it keeps clear of them.
        let x = mark.origin.x + MARK + tabs::ICON_GAP + 2.0;
        let tools = self.tools();
        let end = if iv.hot.get() {
            tools.first().map_or(bounds.size.width, |(_, rect)| {
                rect.origin.x - tabs::ICON_GAP
            })
        } else {
            bounds.size.width - margins.1
        };
        super::tab_side::draw_title(
            &look.title,
            NSRect::new(
                NSPoint::new(x, GUTTER),
                NSSize::new((end - x).max(0.0), TOOL_HEIGHT),
            ),
            TITLE_TEXT,
            look.focused,
            ink,
        );
        if iv.hot.get() {
            for (tool, rect) in tools {
                let hot = iv.hot_tool.get() == Some(tool);
                if hot {
                    let face =
                        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect, 5.0, 5.0);
                    palette.close_hover.color().setFill();
                    face.fill();
                }
                draw_tool(tool, rect, if hot { palette.title } else { palette.dim });
            }
        }
        if let Some([r, g, b]) = look.frame {
            // The focus frame's top half: down both sides and round the top corners.
            let half = 1.0;
            let path = NSBezierPath::bezierPath();
            let (x0, x1) = (
                bounds.origin.x + half,
                bounds.origin.x + bounds.size.width - half,
            );
            let (y0, y1) = (bounds.origin.y + half, bounds.origin.y + bounds.size.height);
            let corner = (radius - half).max(0.0);
            path.moveToPoint(NSPoint::new(x0, y1));
            path.appendBezierPathWithArcFromPoint_toPoint_radius(
                NSPoint::new(x0, y0),
                NSPoint::new(x1, y0),
                corner,
            );
            path.appendBezierPathWithArcFromPoint_toPoint_radius(
                NSPoint::new(x1, y0),
                NSPoint::new(x1, y1),
                corner,
            );
            path.lineToPoint(NSPoint::new(x1, y1));
            path.setLineWidth(2.0);
            NSColor::colorWithSRGBRed_green_blue_alpha(
                f64::from(r) / 255.0,
                f64::from(g) / 255.0,
                f64::from(b) / 255.0,
                1.0,
            )
            .setStroke();
            path.stroke();
        }
    }
}

/// `rect` (top-down) with its two top corners rounded by `radius`.
fn top_rounded(rect: NSRect, radius: f64) -> Retained<NSBezierPath> {
    let (x0, y0) = (rect.origin.x, rect.origin.y);
    let (x1, y1) = (x0 + rect.size.width, y0 + rect.size.height);
    let radius = radius
        .min(rect.size.width / 2.0)
        .min(rect.size.height)
        .max(0.0);
    let path = NSBezierPath::bezierPath();
    path.moveToPoint(NSPoint::new(x0, y1));
    path.appendBezierPathWithArcFromPoint_toPoint_radius(
        NSPoint::new(x0, y0),
        NSPoint::new(x1, y0),
        radius,
    );
    path.appendBezierPathWithArcFromPoint_toPoint_radius(
        NSPoint::new(x1, y0),
        NSPoint::new(x1, y1),
        radius,
    );
    path.lineToPoint(NSPoint::new(x1, y1));
    path.closePath();
    path
}

/// A tool's glyph in its box, on the design's 12-unit grid drawn 14 pt.
fn draw_tool(tool: Tool, rect: NSRect, color: u32) {
    let unit = 14.0 / 12.0;
    let middle = NSPoint::new(
        rect.origin.x + rect.size.width / 2.0,
        rect.origin.y + rect.size.height / 2.0,
    );
    let at =
        |x: f64, y: f64| NSPoint::new(middle.x + (x - 6.0) * unit, middle.y + (y - 6.0) * unit);
    let path = NSBezierPath::bezierPath();
    let window = |path: &NSBezierPath| {
        path.appendBezierPathWithRoundedRect_xRadius_yRadius(
            NSRect::new(at(1.0, 2.25), NSSize::new(10.0 * unit, 7.5 * unit)),
            1.6 * unit,
            1.6 * unit,
        );
    };
    match tool {
        Tool::SplitRight => {
            window(&path);
            path.moveToPoint(at(6.0, 2.4));
            path.lineToPoint(at(6.0, 9.6));
        }
        Tool::SplitDown => {
            window(&path);
            path.moveToPoint(at(1.15, 6.0));
            path.lineToPoint(at(10.85, 6.0));
        }
        Tool::NewTab => {
            path.moveToPoint(at(5.2, 1.75));
            path.lineToPoint(at(3.1, 1.75));
            path.appendBezierPathWithArcFromPoint_toPoint_radius(
                at(1.5, 1.75),
                at(1.5, 3.35),
                1.6 * unit,
            );
            path.lineToPoint(at(1.5, 8.9));
            path.appendBezierPathWithArcFromPoint_toPoint_radius(
                at(1.5, 10.5),
                at(3.1, 10.5),
                1.6 * unit,
            );
            path.lineToPoint(at(8.65, 10.5));
            path.appendBezierPathWithArcFromPoint_toPoint_radius(
                at(10.25, 10.5),
                at(10.25, 8.9),
                1.6 * unit,
            );
            path.lineToPoint(at(10.25, 6.8));
            path.moveToPoint(at(6.9, 1.75));
            path.lineToPoint(at(10.25, 1.75));
            path.lineToPoint(at(10.25, 5.1));
            path.moveToPoint(at(10.1, 1.9));
            path.lineToPoint(at(5.6, 6.4));
        }
        Tool::Close => {
            path.moveToPoint(at(3.4, 3.4));
            path.lineToPoint(at(8.6, 8.6));
            path.moveToPoint(at(8.6, 3.4));
            path.lineToPoint(at(3.4, 8.6));
        }
    }
    path.setLineWidth(1.15 * unit);
    path.setLineCapStyle(objc2_app_kit::NSLineCapStyle::Round);
    path.setLineJoinStyle(objc2_app_kit::NSLineJoinStyle::Round);
    Tint::of(color, 1.0).color().setStroke();
    path.stroke();
}
