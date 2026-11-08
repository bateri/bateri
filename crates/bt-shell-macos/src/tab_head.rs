//! A pane's header: the band [`crate::card::HEAD_PT`] tall over every pane in view — a tab's only
//! one and a zoomed one too, so a pane can always be taken by it, whatever the program in it
//! covers. The program's mark with no tile — a dot on its corner says the pane's own signals —,
//! the pane's title (the focused pane's in full ink), and under the pointer the pane's tools at
//! its right: Split Right, Split Down, Float Pane (Dock Pane while it floats) and Open in New Tab
//! (with other panes beside it) and Close Split. A press on the band gives the pane the keyboard
//! and is the start of carrying it: past the slop the pane is carried (`pane_drag`, no key
//! held) — to a region of a pane, onto a strip, out of the window; the band of a floating pane
//! moves its card instead, inside the tab. A press on a tool runs that command on this pane.
//!
//! **The title is a tab's** ([`super::tab_title::Title`]): a title that does not fit fades at its
//! end instead of ending in "…", and gives the tools its room while they show — the tools come in
//! the way a chip's `×` does ([`TOOLS_SECS`]), and the one under the pointer stands on a face.
//!
//! **Over a program's view the header says what the view is** ([`ViewHead`]): the program's own
//! icon in place of the mark — the picture it made for a dark or a light ground, as the theme
//! is —, where it runs, its name and the view's title in place of the pane's title, and at the
//! right a **Quit ⌃C** that stands there for as long as the view is up, the tools coming in left
//! of it. The view covers the whole pane and could imitate a terminal; the header is outside it,
//! so these words are the application's, and they never wait for the pointer.
//!
//! **One band, the container's.** The header is not the pane's: the split container lays it out
//! over the pane's place (`split::Spacing::with_head`, so the layout, the split limit, a drag
//! and a resize all count it) and the pane starts under it, with its own margin. On a card the
//! header takes the card's top corners and its light, the pane the bottom ones; the focus frame
//! runs round both (the container's, `card::FocusRing`).
//!
//! A child of `tab_bar` for the bar's mark and indicator drawing, which a sibling could not
//! reach.

use std::cell::{Cell, RefCell};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSApplication, NSAttributedStringNSStringDrawing, NSBezierPath, NSEvent,
    NSFont, NSFontWeightMedium, NSFontWeightSemibold, NSGraphicsContext, NSTextAlignment, NSView,
};
use objc2_foundation::{
    NSAttributedString, NSMutableAttributedString, NSObjectProtocol, NSPoint, NSRect, NSSize,
    NSString,
};

use super::tab_title::Title;
use super::{
    Palette, ProgramKind, Tint, covering, draw_mark, draw_picture, fade, fitting, track_moves,
};
use crate::palette::attributes;
use crate::pane::TerminalPane;
use crate::pencere_view::ViewHead;
use crate::pencere_view::face::{self as program_face};
use crate::tabs::{self, Indicator};
use bt_shell_common::view_icon::Lay;

/// The mark's side, points: the program's own glyph, no tile. A capsule's mark is the same.
pub(super) const MARK: f64 = 14.0;
/// The status dot on the mark's corner, and the ring of the ground cut round it.
pub(super) const DOT: f64 = 5.0;
pub(super) const DOT_RING: f64 = 1.0;
/// A tool's box, and the space between two.
const TOOL_WIDTH: f64 = 22.0;
const TOOL_HEIGHT: f64 = 18.0;
const TOOL_GAP: f64 = 2.0;
/// The band's inner margin above and below its parts: the terminal's gutter, so the mark stands
/// on the grid's left line and the pane's text starts the same distance under the band.
const GUTTER: f64 = 8.0;
/// The title's type, points: a row's. A capsule's too.
pub(super) const TITLE_TEXT: f64 = 12.5;
/// How long the tools take to come and go under the pointer: a chip's `×`'s pace.
const TOOLS_SECS: f64 = 0.12;
/// A tool's face's corner radius.
const TOOL_RADIUS: f64 = 5.0;
/// The program's own icon over its view, in place of the mark, points: a little over the mark,
/// on its middle, as an application's icon stands by its name in a menu.
const FACE: f64 = 16.0;
/// The corner of an icon that brings its own ground, points: a tab's tile's (5 on its 18).
const FACE_CORNER: f64 = 4.5;
/// What stands between the place, the program and the view's title: an en space and a thin
/// one, about the strip's 8 points. Told apart by their inks, with no separator.
const WORD_GAP: &str = "\u{2002}\u{2009}";
/// The type of the place and the view's title over a view, points: a step under the program.
const WORD_TEXT: f64 = 12.0;
/// Quit ⌃C over a view: the verb's and the key's sizes (the key a little under the verb, as a
/// menu shows a shortcut), the gap from its edge to its text, and the gap left of it before the
/// tools.
const QUIT_TEXT: f64 = 11.5;
const QUIT_KEY_TEXT: f64 = 11.0;
const QUIT_PAD: f64 = 8.0;
const QUIT_GAP: f64 = 6.0;
/// How much of the foreground Quit's plate and its edge are, over the band. A design constant,
/// tuned by eye on a dark and a light theme: the same veil reads as a button on both.
const QUIT_FILL: f64 = 0.08;
const QUIT_EDGE: f64 = 0.16;

/// What a press on the band can be on: a tool, or Quit over a view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    Tool(Tool),
    Quit,
}

/// A pane's tools, in the order they stand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tool {
    SplitRight,
    SplitDown,
    /// A flat pane floats (dim when it is the flat area's only pane).
    Float,
    /// A floating pane docks (lit while it floats).
    Dock,
    /// A floating pane's card becomes a capsule.
    Minimize,
    NewTab,
    Close,
}

impl Tool {
    /// The tools of a pane: Float on a flat pane and Dock on a floating one — one button, the
    /// menu item's own; Open in New Tab only on a flat pane beside other panes — a tab's only
    /// pane is the tab already, and a floating pane docks before it leaves — and is the only one
    /// that minimizes.
    fn of(alone: bool, floating: bool) -> &'static [Tool] {
        match (floating, alone) {
            (true, _) => &[
                Tool::SplitRight,
                Tool::SplitDown,
                Tool::Dock,
                Tool::Minimize,
                Tool::Close,
            ],
            (false, true) => &[Tool::SplitRight, Tool::SplitDown, Tool::Float, Tool::Close],
            (false, false) => &[
                Tool::SplitRight,
                Tool::SplitDown,
                Tool::Float,
                Tool::NewTab,
                Tool::Close,
            ],
        }
    }

    fn name(self) -> &'static str {
        match self {
            Tool::SplitRight => "Split Right",
            Tool::SplitDown => "Split Down",
            Tool::Float => "Float Pane",
            Tool::Dock => "Dock Pane",
            Tool::Minimize => "Minimize Floating Pane",
            Tool::NewTab => "Open in New Tab",
            Tool::Close => "Close Split",
        }
    }

    /// The command of the shortcut table the tool's tip names the key of, if it has one.
    fn command(self) -> Option<&'static str> {
        match self {
            Tool::Float | Tool::Dock => Some("float_pane"),
            Tool::Minimize => Some("minimize_floating_pane"),
            Tool::SplitRight | Tool::SplitDown | Tool::NewTab | Tool::Close => None,
        }
    }

    /// The command it runs on the pane once the pane has the keyboard: the menu's own action.
    fn action(self) -> objc2::runtime::Sel {
        match self {
            Tool::SplitRight => sel!(splitRight:),
            Tool::SplitDown => sel!(splitDown:),
            Tool::Float | Tool::Dock => sel!(toggleFloatPane:),
            Tool::Minimize => sel!(minimizeFloatingPane:),
            Tool::NewTab => sel!(movePaneToNewTab:),
            Tool::Close => sel!(closeTab:),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flat_pane_has_float_and_a_floating_one_dock_in_the_same_place() {
        use Tool::*;
        assert_eq!(
            Tool::of(false, false),
            [SplitRight, SplitDown, Float, NewTab, Close]
        );
        // A tab's only pane is the tab: no Open in New Tab, and Float stays — dim.
        assert_eq!(Tool::of(true, false), [SplitRight, SplitDown, Float, Close]);
        // A floating pane docks before it leaves its tab, and minimizes.
        assert_eq!(
            Tool::of(false, true),
            [SplitRight, SplitDown, Dock, Minimize, Close]
        );
        assert_eq!(Tool::of(true, true), Tool::of(false, true));
    }

    #[test]
    fn float_and_dock_are_one_command_with_one_key_and_two_names() {
        assert_eq!(Tool::Float.action(), Tool::Dock.action());
        assert_eq!(Tool::Float.command(), Tool::Dock.command());
        assert_eq!(Tool::Float.name(), "Float Pane");
        assert_eq!(Tool::Dock.name(), "Dock Pane");
        // The command is the table's, and its key is the tip's.
        let command = bt_shell_common::shortcuts::by_name("float_pane").expect("in the table");
        assert_eq!(command.defaults, ["option+cmd+return"]);
        // The other tools name no key: the tip says what they do.
        assert_eq!(Tool::Close.command(), None);
    }

    #[test]
    fn minimize_is_the_menus_item_with_its_key_on_the_tip() {
        assert_eq!(Tool::Minimize.action(), sel!(minimizeFloatingPane:));
        assert_eq!(Tool::Minimize.name(), "Minimize Floating Pane");
        let command =
            bt_shell_common::shortcuts::by_name("minimize_floating_pane").expect("in the table");
        assert_eq!(Tool::Minimize.command(), Some(command.name));
        assert_eq!(command.defaults, ["option+cmd+m"]);
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
    /// The pane floats: its Float tool is Dock, and lit.
    pub(crate) floating: bool,
    /// The tab's only pane: no Open in New Tab among its tools.
    pub(crate) alone: bool,
    /// The flat area's only pane: it cannot float, and its Float tool is dim.
    pub(crate) flat_alone: bool,
    /// What covers the pane, when a program's view does: its words, icon and way out stand in
    /// the band in place of the pane's title and mark.
    pub(crate) view: Option<ViewHead>,
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
    /// Which top corners take it, left and right: both on a card of its own, only the card's
    /// own on the header of a split floating card, whose panes meet square.
    corners: Cell<(bool, bool)>,
    /// The grid's left and right margins under it (`CellMetrics::with_corners`): the mark stands
    /// on the first, the tools end at the second.
    margins: Cell<(f64, f64)>,
    /// The pointer is on the band: the tools show and the title gives them its room.
    hot: Cell<bool>,
    /// The pointer is on Quit: it stands on a brighter face.
    quit_lit: Cell<bool>,
    pressed: Cell<Option<Part>>,
    title: Title,
    tools: Retained<HeadTools>,
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
        fn mouse_entered(&self, event: &NSEvent) {
            self.ivars().hot.set(true);
            self.lay_out_parts();
            self.point_at(event);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            self.ivars().hot.set(false);
            self.ivars().tools.set_hot(None);
            if self.ivars().quit_lit.replace(false) {
                self.setNeedsDisplay(true);
            }
            self.lay_out_parts();
        }

        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, event: &NSEvent) {
            self.point_at(event);
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let part = self.part_at(event);
            self.ivars().pressed.set(part);
            self.focus_pane();
            if part.is_some() {
                return;
            }
            // Off the tools and Quit a floating pane's band moves its card, and from the press on:
            // the card is already where the pointer is taking it.
            if let Some(container) = self.container()
                && container.floating(self.pane())
            {
                container.begin_card_drag(self.pane(), None, event.locationInWindow());
                return;
            }
            // Otherwise the press may become a carry: the session watches the pointer from
            // here and takes over once it travels.
            if let Some(app) = crate::app::delegate(self.mtm()) {
                app.pane_press(self.pane(), event.locationInWindow());
            }
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            if let Some(container) = self.container() {
                container.drag_card(event.locationInWindow());
            }
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            if let Some(container) = self.container() {
                container.end_card_drag();
            }
            if let Some(part) = self.ivars().pressed.take()
                && self.part_at(event) == Some(part)
            {
                match part {
                    Part::Tool(tool) => self.run(tool),
                    Part::Quit => self.quit(),
                }
            }
        }

        /// A new width: the title and the tools take their places in it again.
        #[unsafe(method(resizeSubviewsWithOldSize:))]
        fn resize_subviews(&self, _old: NSSize) {
            self.lay_out_parts();
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            self.draw();
        }
    }
);

impl PaneHead {
    pub(crate) fn new(mtm: MainThreadMarker, pane: u64) -> Retained<Self> {
        let title = Title::new(mtm, NSTextAlignment::Left);
        let tools = HeadTools::new(mtm);
        let this = Self::alloc(mtm).set_ivars(HeadIvars {
            pane: Cell::new(pane),
            look: RefCell::new(None),
            palette: Cell::new(None),
            ground: Cell::new(0),
            dark: Cell::new(true),
            radius: Cell::new(0.0),
            corners: Cell::new((true, true)),
            margins: Cell::new((GUTTER, GUTTER)),
            hot: Cell::new(false),
            quit_lit: Cell::new(false),
            pressed: Cell::new(None),
            title,
            tools: tools.clone(),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.addSubview(this.ivars().title.view());
        this.ivars().title.field().setAccessibilityElement(false);
        tools.setAlphaValue(0.0);
        this.addSubview(&tools);
        // Entering, moving over the tools and leaving.
        track_moves(&this);
        this
    }

    /// The pane the header stands over.
    pub(crate) fn pane(&self) -> u64 {
        self.ivars().pane.get()
    }

    /// The grid's margins under it, from the pane's corners (the container's layout).
    pub(crate) fn set_margins(&self, margins: (f64, f64)) {
        if self.ivars().margins.replace(margins) != margins {
            self.lay_out_parts();
            self.setNeedsDisplay(true);
        }
    }

    /// What it shows, in the theme's colours; redrawn only when something changed.
    pub(crate) fn set(&self, look: HeadLook, theme: &bt_core::Theme, radius: f64) {
        let iv = self.ivars();
        let palette = Palette::of(theme);
        // Every field is written whatever came before it: `|`, not `||` — a new theme that also
        // changed the look kept the old ground.
        let changed = (iv.look.borrow().as_ref() != Some(&look))
            | (iv.palette.replace(Some(palette)) != Some(palette))
            | (iv.ground.replace(theme.background) != theme.background)
            | (iv.dark.replace(theme.is_dark()) != theme.is_dark())
            | (iv.radius.replace(radius) != radius);
        let tip = NSString::from_str(&said(&look));
        self.setAccessibilityLabel(Some(&tip));
        iv.look.replace(Some(look));
        if changed {
            self.lay_out_parts();
            self.setNeedsDisplay(true);
        }
    }

    /// Which of its top corners are round when the look is a card's (left, right); redrawn
    /// only when that changed.
    pub(crate) fn set_corners(&self, corners: (bool, bool)) {
        if self.ivars().corners.replace(corners) != corners {
            self.setNeedsDisplay(true);
        }
    }

    /// Quit's box over a view, at the grid's right margin, in the band's space; `None` over the
    /// terminal.
    fn quit_box(&self) -> Option<NSRect> {
        let iv = self.ivars();
        let look = iv.look.borrow();
        look.as_ref()?.view.as_ref()?;
        let width = quit_label(iv.palette.get()?).size().width.ceil() + 2.0 * QUIT_PAD;
        Some(NSRect::new(
            NSPoint::new(
                self.bounds().size.width - iv.margins.get().1 - width,
                GUTTER,
            ),
            NSSize::new(width, TOOL_HEIGHT),
        ))
    }

    /// The tools' boxes, right to left from the grid's right margin — or from Quit, over a
    /// view —, in the band's space.
    fn tool_boxes(&self) -> Vec<(Tool, NSRect)> {
        let iv = self.ivars();
        let (alone, floating) = iv
            .look
            .borrow()
            .as_ref()
            .map_or((false, false), |look| (look.alone, look.floating));
        let tools = Tool::of(alone, floating);
        let right = self.quit_box().map_or_else(
            || self.bounds().size.width - iv.margins.get().1,
            |quit| quit.origin.x - QUIT_GAP,
        );
        let count = tools.len() as f64;
        let all = count * TOOL_WIDTH + (count - 1.0) * TOOL_GAP;
        let start = right - all;
        tools
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

    /// Where the mark stands: on the grid's left line, centred in the row.
    fn mark_rect(&self) -> NSRect {
        NSRect::new(
            NSPoint::new(
                self.ivars().margins.get().0,
                GUTTER + (TOOL_HEIGHT - MARK) / 2.0,
            ),
            NSSize::new(MARK, MARK),
        )
    }

    /// Places the title and the tools: the title after the mark, to the grid's right margin or
    /// Quit — or, while the tools show, to just before them; the tools come in or go.
    fn lay_out_parts(&self) {
        let iv = self.ivars();
        let look = iv.look.borrow();
        let (Some(look), Some(palette)) = (look.as_ref(), iv.palette.get()) else {
            return;
        };
        let width = self.bounds().size.width;
        let hot = iv.hot.get();
        let boxes = self.tool_boxes();
        let x = self.mark_rect().origin.x + MARK + tabs::ICON_GAP + 2.0;
        let end = if hot {
            boxes
                .first()
                .map_or(width, |(_, rect)| rect.origin.x - tabs::ICON_GAP)
        } else {
            self.quit_box().map_or_else(
                || width - iv.margins.get().1,
                |quit| quit.origin.x - tabs::ICON_GAP,
            )
        };
        let text = iv.title.field();
        text.setAttributedStringValue(&title_text(look, palette));
        let natural = text.fittingSize();
        iv.title.place(
            NSRect::new(
                NSPoint::new(x, GUTTER + ((TOOL_HEIGHT - natural.height) / 2.0).max(0.0)),
                NSSize::new((end - x).max(0.0), natural.height),
            ),
            natural.width,
        );
        // The tools stand in their row; their own view draws them and the face under the pointer.
        let first = boxes.first().map_or(width, |(_, rect)| rect.origin.x);
        let last = boxes
            .last()
            .map_or(width, |(_, rect)| rect.origin.x + rect.size.width);
        iv.tools.setFrame(NSRect::new(
            NSPoint::new(first, GUTTER),
            NSSize::new((last - first).max(0.0), TOOL_HEIGHT),
        ));
        iv.tools.set(
            boxes
                .iter()
                .map(|(tool, rect)| {
                    (
                        *tool,
                        NSRect::new(NSPoint::new(rect.origin.x - first, 0.0), rect.size),
                    )
                })
                .collect(),
            palette,
            // Dock stands lit while its pane floats; Float stands dim where nothing can float.
            Marks {
                lit: look.floating.then_some(Tool::Dock),
                dim: (!look.floating && look.flat_alone).then_some(Tool::Float),
            },
        );
        let still = crate::app::delegate(self.mtm()).is_some_and(|app| app.reduce_motion());
        fade(
            &iv.tools,
            f64::from(u8::from(hot)),
            if still { 0.0 } else { TOOLS_SECS },
        );
    }

    /// The pointer moved on the band: the tool or Quit under it is lit and named.
    fn point_at(&self, event: &NSEvent) {
        let part = self.part_at(event);
        let iv = self.ivars();
        let tool = match part {
            Some(Part::Tool(tool)) => Some(tool),
            _ => None,
        };
        let quit = part == Some(Part::Quit);
        let lit = iv.quit_lit.replace(quit) != quit;
        if lit {
            self.setNeedsDisplay(true);
        }
        if iv.tools.set_hot(tool) | lit {
            let look = iv.look.borrow();
            let tip = match part {
                Some(Part::Tool(tool)) => Some(
                    match tool
                        .command()
                        .and_then(|command| crate::menu::keys::glyphs_of(self.mtm(), command))
                    {
                        // The name and, after two spaces, the key the command has now.
                        Some(key) => format!("{}  {key}", tool.name()),
                        None => tool.name().to_owned(),
                    },
                ),
                Some(Part::Quit) => look
                    .as_ref()
                    .and_then(|look| look.view.as_ref())
                    .map(|view| view.words.quit_tip.clone()),
                // A title that fades at its end says the whole of itself here.
                None => iv
                    .title
                    .overflows()
                    .then(|| look.as_ref().map(said))
                    .flatten(),
            };
            self.setToolTip(tip.as_deref().map(NSString::from_str).as_deref());
        }
    }

    /// The part under the event's point: Quit whenever a view is up, a tool while the tools
    /// show (the pointer is on the band).
    fn part_at(&self, event: &NSEvent) -> Option<Part> {
        let at = self.convertPoint_fromView(event.locationInWindow(), None);
        let inside = |rect: NSRect| {
            at.x >= rect.origin.x
                && at.x <= rect.origin.x + rect.size.width
                && at.y >= rect.origin.y
                && at.y <= rect.origin.y + rect.size.height
        };
        if self.quit_box().is_some_and(inside) {
            return Some(Part::Quit);
        }
        if !self.ivars().hot.get() {
            return None;
        }
        self.tool_boxes()
            .into_iter()
            .find_map(|(tool, rect)| inside(rect).then_some(Part::Tool(tool)))
    }

    /// The container the band and its pane are in.
    fn container(&self) -> Option<Retained<crate::split_view::SplitView>> {
        // SAFETY: reading the superview; we are on the main thread.
        unsafe { self.superview() }
            .and_then(|view| view.downcast::<crate::split_view::SplitView>().ok())
    }

    /// The pane the band stands over, from the container both are in.
    fn pane_view(&self) -> Option<Retained<TerminalPane>> {
        self.container()
            .and_then(|container| container.pane(self.pane()))
    }

    /// The pane under the band takes the keyboard.
    fn focus_pane(&self) {
        if let Some(pane) = self.pane_view() {
            // Through the pane's own gate, as a click in it does.
            let _ = pane.focus();
        }
    }

    /// Quit over a view: the pane's own way out of it ([`crate::pencere_view::Exit`]) — ⌃C to
    /// the program, as the key —, sent to the pane itself.
    fn quit(&self) {
        let exit = self
            .ivars()
            .look
            .borrow()
            .as_ref()
            .and_then(|look| look.view.as_ref())
            .map(|view| view.exit);
        let (Some(exit), Some(pane)) = (exit, self.pane_view()) else {
            return;
        };
        let app = NSApplication::sharedApplication(self.mtm());
        let target: &AnyObject = pane.as_ref();
        let sender: &AnyObject = self;
        // SAFETY: the pane's way-out actions take one optional sender argument.
        unsafe { app.sendAction_to_from(exit.action(), Some(target), Some(sender)) };
    }

    /// Runs `tool` on the pane: it has the keyboard from the press, so the menu's own action
    /// down the responder chain acts on it.
    fn run(&self, tool: Tool) {
        let app = NSApplication::sharedApplication(self.mtm());
        let sender: &AnyObject = self;
        // SAFETY: every tool's action takes one optional sender argument.
        unsafe { app.sendAction_to_from(tool.action(), None, Some(sender)) };
        self.ivars().tools.set_hot(None);
    }

    /// The band: its ground, the card's light on its top edge, the mark and its dot. The title
    /// and the tools are views of their own over it.
    fn draw(&self) {
        let iv = self.ivars();
        let look = iv.look.borrow();
        let (Some(look), Some(palette)) = (look.as_ref(), iv.palette.get()) else {
            return;
        };
        let bounds = self.bounds();
        let radius = if look.carded { iv.radius.get() } else { 0.0 };
        let (left, right) = iv.corners.get();
        let radius = (
            if left { radius } else { 0.0 },
            if right { radius } else { 0.0 },
        );
        // The band's ground: the pane's own, the card's top corners rounded.
        let band = top_rounded(bounds, radius);
        Tint::of(iv.ground.get(), 1.0).color().setFill();
        band.fill();
        if look.carded {
            // The card's light on its top edge, as the pane's frame had it without a header: the
            // band less itself moved one line down, clipped to the band — unclipped, the moved
            // band's foot drew a line under the header, over the pane.
            let (tone, alpha) = match (iv.dark.get(), look.focused) {
                (true, true) => (0xfff6e8, 0.32),
                (true, false) => (0xffffff, 0.13),
                (false, _) => (0xffffff, 1.0),
            };
            NSGraphicsContext::saveGraphicsState_class();
            band.addClip();
            let edge = top_rounded(bounds, radius);
            let below = NSRect::new(
                NSPoint::new(bounds.origin.x, bounds.origin.y + 1.0),
                bounds.size,
            );
            edge.appendBezierPath(&top_rounded(below, radius));
            edge.setWindingRule(objc2_app_kit::NSWindingRule::EvenOdd);
            Tint::of(tone, alpha).color().setFill();
            edge.fill();
            NSGraphicsContext::restoreGraphicsState_class();
        }
        // The mark on the grid's left line — over a view the program's own icon, when it gave
        // one —, the dot on its corner.
        let ink = if look.focused {
            palette.title
        } else {
            palette.dim
        };
        let mark = self.mark_rect();
        match look
            .view
            .as_ref()
            .and_then(|view| view.face)
            .and_then(program_face::face)
        {
            Some(face) => draw_face(&face, mark),
            None => draw_mark(look.kind, mark, &Tint::of(ink, 1.0).color()),
        }
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
        if let Some(quit) = self.quit_box() {
            draw_quit(quit, iv.quit_lit.get(), palette);
        }
    }
}

/// What the band says, whole: the pane's title, or over a view its place, program and title.
fn said(look: &HeadLook) -> String {
    match &look.view {
        Some(view) => [&view.words.place, &view.words.program, &view.words.title]
            .into_iter()
            .flatten()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("  "),
        None => look.title.clone(),
    }
}

/// The band's title as it is drawn: the pane's title in the focus's ink and weight; over a view,
/// where the program runs (the info role, the dock's `host`), the program in the title's ink
/// and weight, and the view's title in the dim role — in that order, so the title is what a
/// fade cuts first and the place what it cuts last.
fn title_text(look: &HeadLook, palette: Palette) -> Retained<NSMutableAttributedString> {
    let ink = if look.focused {
        palette.title
    } else {
        palette.dim
    };
    // SAFETY: AppKit's constant font weights.
    let weight = unsafe {
        if look.focused {
            NSFontWeightSemibold
        } else {
            NSFontWeightMedium
        }
    };
    let strong = attributes(
        &NSFont::systemFontOfSize_weight(TITLE_TEXT, weight),
        Tint::of(ink, 1.0),
    );
    let out = NSMutableAttributedString::new();
    let add = |text: &str, attributes: &crate::palette::Attributes| {
        // SAFETY: the attributes are `NSColor`/`NSFont` under AppKit's two keys.
        let piece = unsafe {
            NSAttributedString::new_with_attributes(&NSString::from_str(text), attributes)
        };
        out.appendAttributedString(&piece);
    };
    let Some(view) = &look.view else {
        add(&look.title, &strong);
        return out;
    };
    let quiet = |color: u32| attributes(&NSFont::systemFontOfSize(WORD_TEXT), Tint::of(color, 1.0));
    let words = [
        (&view.words.place, quiet(palette.info)),
        (&view.words.program, strong.clone()),
        (&view.words.title, quiet(palette.dim)),
    ];
    let mut first = true;
    for (text, attributes) in words
        .iter()
        .filter_map(|(text, a)| Some((text.as_ref()?, a)))
    {
        if !first {
            add(WORD_GAP, attributes);
        }
        add(text, attributes);
        first = false;
    }
    out
}

/// A program's own icon in the mark's place, [`FACE`] across on the mark's middle: one that
/// brings its own ground fills it with its corners cut as a tile's are, a shape stands as it is.
fn draw_face(face: &program_face::Face, mark: NSRect) {
    let rect = NSRect::new(
        NSPoint::new(
            mark.origin.x + (mark.size.width - FACE) / 2.0,
            mark.origin.y + (mark.size.height - FACE) / 2.0,
        ),
        NSSize::new(FACE, FACE),
    );
    match face.lay {
        Lay::Ground => {
            NSGraphicsContext::saveGraphicsState_class();
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect, FACE_CORNER, FACE_CORNER)
                .addClip();
            draw_picture(&face.image, covering(rect, face.image.size()));
            NSGraphicsContext::restoreGraphicsState_class();
        }
        Lay::Mark => draw_picture(&face.image, fitting(rect, face.image.size())),
    }
}

/// Quit's label: the verb in the foreground and its key in the dim role, as a menu shows a
/// shortcut.
fn quit_label(palette: Palette) -> Retained<NSMutableAttributedString> {
    let out = NSMutableAttributedString::new();
    for (text, color, font) in [
        (
            format!("{}\u{2002}", bt_shell_common::view_strip::QUIT),
            palette.title,
            // SAFETY: a constant AppKit exposes, it lives for the whole process.
            NSFont::systemFontOfSize_weight(QUIT_TEXT, unsafe { NSFontWeightMedium }),
        ),
        (
            bt_shell_common::view_strip::QUIT_KEY.to_owned(),
            palette.dim,
            NSFont::systemFontOfSize(QUIT_KEY_TEXT),
        ),
    ] {
        // SAFETY: the attributes are `NSColor`/`NSFont` under AppKit's two keys.
        let piece = unsafe {
            NSAttributedString::new_with_attributes(
                &NSString::from_str(&text),
                &attributes(&font, Tint::of(color, 1.0)),
            )
        };
        out.appendAttributedString(&piece);
    }
    out
}

/// Quit in `rect`: a plate of the foreground's veil with its edge, brighter under the pointer,
/// and its label on its middle.
fn draw_quit(rect: NSRect, lit: bool, palette: Palette) {
    let fill = if lit {
        palette.close_hover
    } else {
        Tint::of(palette.title, QUIT_FILL)
    };
    let inset = NSRect::new(
        NSPoint::new(rect.origin.x + 0.5, rect.origin.y + 0.5),
        NSSize::new(rect.size.width - 1.0, rect.size.height - 1.0),
    );
    let plate =
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(inset, TOOL_RADIUS, TOOL_RADIUS);
    fill.color().setFill();
    plate.fill();
    Tint::of(palette.title, QUIT_EDGE).color().setStroke();
    plate.setLineWidth(1.0);
    plate.stroke();
    let label = quit_label(palette);
    let size = label.size();
    label.drawAtPoint(NSPoint::new(
        rect.origin.x + (rect.size.width - size.width) / 2.0,
        rect.origin.y + (rect.size.height - size.height) / 2.0,
    ));
}

/// Which of a pane's tools stand out: one in the accent, one faded.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Marks {
    lit: Option<Tool>,
    dim: Option<Tool>,
}

/// A dim tool's strength: the design's 0.35.
const DIM_TOOL: f64 = 0.35;
/// The ground a lit tool stands on: the accent at the design's 0.14.
const LIT_FILL: f64 = 0.14;

pub(crate) struct ToolsIvars {
    /// Each tool and its box, in the view's space.
    boxes: RefCell<Vec<(Tool, NSRect)>>,
    marks: Cell<Marks>,
    palette: Cell<Option<Palette>>,
    /// The tool under the pointer: on a face, in full ink.
    hot: Cell<Option<Tool>>,
}

define_class!(
    /// A header's tools: drawn in a row, the one under the pointer on a face. Their own view, so
    /// they come and go with its opacity; the header takes every press.
    // SAFETY: NSView is designed for subclassing; HeadTools implements no `Drop` and is born with
    // `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriPaneHeadTools"]
    #[ivars = ToolsIvars]
    pub(crate) struct HeadTools;

    unsafe impl NSObjectProtocol for HeadTools {}

    impl HeadTools {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let iv = self.ivars();
            let Some(palette) = iv.palette.get() else {
                return;
            };
            let marks = iv.marks.get();
            for &(tool, rect) in iv.boxes.borrow().iter() {
                let dim = marks.dim == Some(tool);
                let lit = marks.lit == Some(tool);
                let hot = iv.hot.get() == Some(tool) && !dim;
                let plate = if lit {
                    Some(Tint::of(palette.accent, LIT_FILL))
                } else {
                    hot.then_some(palette.close_hover)
                };
                if let Some(plate) = plate {
                    plate.color().setFill();
                    NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                        rect,
                        TOOL_RADIUS,
                        TOOL_RADIUS,
                    )
                    .fill();
                }
                let ink = match (lit, hot) {
                    (true, _) => palette.accent,
                    (false, true) => palette.title,
                    (false, false) => palette.dim,
                };
                draw_tool(tool, rect, ink, if dim { DIM_TOOL } else { 1.0 });
            }
        }
    }
);

impl HeadTools {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ToolsIvars {
            boxes: RefCell::new(Vec::new()),
            marks: Cell::new(Marks::default()),
            palette: Cell::new(None),
            hot: Cell::new(None),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the ivars are set.
        unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] }
    }

    /// The tools' boxes, the palette they are drawn in and which of them stand out.
    fn set(&self, boxes: Vec<(Tool, NSRect)>, palette: Palette, marks: Marks) {
        let iv = self.ivars();
        let changed = (*iv.boxes.borrow() != boxes)
            | (iv.palette.replace(Some(palette)) != Some(palette))
            | (iv.marks.replace(marks) != marks);
        iv.boxes.replace(boxes);
        if changed {
            self.setNeedsDisplay(true);
        }
    }

    /// The tool under the pointer; `true` if it changed.
    fn set_hot(&self, tool: Option<Tool>) -> bool {
        let changed = self.ivars().hot.replace(tool) != tool;
        if changed {
            self.setNeedsDisplay(true);
        }
        changed
    }
}

/// `rect` (top-down) with its top left and top right corners rounded by `radius`'s two radii
/// (0 is square).
fn top_rounded(rect: NSRect, (left, right): (f64, f64)) -> Retained<NSBezierPath> {
    let (x0, y0) = (rect.origin.x, rect.origin.y);
    let (x1, y1) = (x0 + rect.size.width, y0 + rect.size.height);
    let fit = |radius: f64| {
        radius
            .min(rect.size.width / 2.0)
            .min(rect.size.height)
            .max(0.0)
    };
    let path = NSBezierPath::bezierPath();
    path.moveToPoint(NSPoint::new(x0, y1));
    path.appendBezierPathWithArcFromPoint_toPoint_radius(
        NSPoint::new(x0, y0),
        NSPoint::new(x1, y0),
        fit(left),
    );
    path.appendBezierPathWithArcFromPoint_toPoint_radius(
        NSPoint::new(x1, y0),
        NSPoint::new(x1, y1),
        fit(right),
    );
    path.lineToPoint(NSPoint::new(x1, y1));
    path.closePath();
    path
}

/// A tool's glyph in its box, on the design's 12-unit grid drawn 14 pt, at `strength`.
fn draw_tool(tool: Tool, rect: NSRect, color: u32, strength: f64) {
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
        // One glyph for both: a card standing over another.
        Tool::Float | Tool::Dock => {
            path.appendBezierPathWithRoundedRect_xRadius_yRadius(
                NSRect::new(at(1.0, 4.25), NSSize::new(7.5 * unit, 7.5 * unit)),
                1.6 * unit,
                1.6 * unit,
            );
            path.moveToPoint(at(4.25, 4.1));
            path.appendBezierPathWithArcFromPoint_toPoint_radius(
                at(4.25, 1.25),
                at(11.0, 1.25),
                1.6 * unit,
            );
            path.appendBezierPathWithArcFromPoint_toPoint_radius(
                at(11.0, 1.25),
                at(11.0, 8.0),
                1.6 * unit,
            );
            path.appendBezierPathWithArcFromPoint_toPoint_radius(
                at(11.0, 8.0),
                at(8.6, 8.0),
                1.6 * unit,
            );
            path.lineToPoint(at(8.6, 8.0));
        }
        // A short bar.
        Tool::Minimize => {
            path.moveToPoint(at(3.2, 6.4));
            path.lineToPoint(at(8.8, 6.4));
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
    Tint::of(color, strength).color().setStroke();
    path.stroke();
}
