//! A pane's header: the band [`crate::card::HEAD_PT`] tall over every pane in view — a tab's only
//! one and a zoomed one too, so a pane can always be taken by it, whatever the program in it
//! covers. The program's mark with no tile — a dot on its corner says the pane's own signals —,
//! the pane's title (the focused pane's in full ink), and under the pointer the pane's tools at
//! its right: Split Right, Split Down, Float Pane (Dock Pane while it floats), Open in New Tab
//! (with other panes beside it), Zoom Pane (Unzoom Pane while it fills the tab) and Close Split.
//! The tools of a pane that fills the tab stay in view, the lit one among them: nothing else on
//! the screen says that panes are out of sight. A press on the band gives the pane the keyboard.
//! A press on the pane's **name** — its mark and its title, from the band's left edge to where
//! the title ends — is the start of carrying it: past the slop the pane is carried (`pane_drag`,
//! no key held) — to a region of a pane, onto a strip, out of the window; the band of a floating
//! pane moves its card instead, inside the tab. Under the pointer, on a name that can be held
//! (the pane has somewhere to go, `pane_drag::can_carry`), a pill eases in round the mark and the
//! title and the mark — a program's own icon too, and its dot — turns into the carry's glyph
//! (`Handle`: three layer-backed pieces, Core Animation's, [`HANDLE_SECS`]; asked again after
//! every layout, so a layout under a still pointer does not play it again). The rest of the
//! band is not the pane's: where the tab column stands beside the content, a band in the
//! content's top row takes the window (`split::head_press`, the same band as the cards' top
//! margin and the gap between two such bands), and any other band's blank only gives the
//! keyboard, so one blank never does two things by where its pane stands. A press on a tool
//! runs that command on this pane.
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
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSAnimatablePropertyContainer, NSApplication,
    NSAttributedStringNSStringDrawing, NSBezierPath, NSEvent, NSFont, NSFontWeightMedium,
    NSFontWeightSemibold, NSGraphicsContext, NSTextAlignment, NSView,
};
use objc2_foundation::{
    NSAttributedString, NSMutableAttributedString, NSNumber, NSObjectProtocol, NSPoint, NSRect,
    NSSize, NSString, NSValue, ns_string,
};
use objc2_quartz_core::{
    CALayer, CAMediaTimingFunction, CATransaction, CATransform3D, NSValueCATransform3DAdditions,
};

use super::tab_title::Title;
use super::{
    Palette, ProgramKind, Tint, animate, centre, contains, covering, draw_mark, draw_picture, fade,
    fitting, stroke, track_moves,
};
use crate::card::{self, IDENTITY};
use crate::palette::attributes;
use crate::pane::TerminalPane;
use crate::pencere_view::ViewHead;
use crate::pencere_view::face::{self as program_face};
use crate::split::{HeadPress, Rect};
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
/// The handle: under the pointer on a pane's name a pill comes round the mark and the title
/// and the mark turns into the carry's glyph. How long it takes to come and go, seconds — a
/// design constant, the canvas's 240 ms —, and the curve it takes (`cubic-bezier(0.2, 0, 0, 1)`).
const HANDLE_SECS: f64 = 0.24;
const HANDLE_CURVE: (f32, f32, f32, f32) = (0.2, 0.0, 0.0, 1.0);
/// The pill's height, points: the band less 4.5 above and below, give or take the row's half
/// point.
const HANDLE_TALL: f64 = 24.0;
/// The pill's room left of the mark (the mark stands on the grid's line, the pill 4.5 from
/// the card's side, as far as from its top) and right of where the title ends.
const HANDLE_PAD: f64 = 3.5;
const HANDLE_END: f64 = 6.0;
/// The pill's ground: the title's ink at this share, the tools' hover face being about twice.
const HANDLE_FILL: f64 = 0.07;
/// The pill comes in from this share of its size; the mark goes out to this share of its,
/// and the carry's glyph comes in from it. Reduce Motion changes the opacity only.
const HANDLE_SWELL: f64 = 0.96;
const HANDLE_GROW: f64 = 0.82;
/// How far a piece of the handle that draws the mark stands out of the mark's box on every side:
/// the program's own icon is a little larger than the mark and the dot's ring reaches past it.
const PIECE_ROOM: f64 = 3.0;
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
    /// The pane fills the room its kind has: a flat pane the splits' area, a floating pane's
    /// card the tab (dim on the flat area's only pane, which fills it already).
    Zoom,
    /// The pane that fills the tab gives the room back (lit while it does).
    Unzoom,
    Close,
}

impl Tool {
    /// The tools of a pane: Float on a flat pane and Dock on a floating one — one button, the
    /// menu item's own; Open in New Tab only on a flat pane beside other panes — a tab's only
    /// pane is the tab already, and a floating pane docks before it leaves — and is the only one
    /// that minimizes. Zoom stands left of Close on every pane, so it is in one place whatever
    /// the pane is, and is Unzoom while the pane fills the tab (`zoomed`).
    fn of(alone: bool, floating: bool, zoomed: bool) -> Vec<Tool> {
        let zoom = if zoomed { Tool::Unzoom } else { Tool::Zoom };
        match (floating, alone) {
            (true, _) => vec![
                Tool::SplitRight,
                Tool::SplitDown,
                Tool::Dock,
                Tool::Minimize,
                zoom,
                Tool::Close,
            ],
            (false, true) => vec![
                Tool::SplitRight,
                Tool::SplitDown,
                Tool::Float,
                zoom,
                Tool::Close,
            ],
            (false, false) => vec![
                Tool::SplitRight,
                Tool::SplitDown,
                Tool::Float,
                Tool::NewTab,
                zoom,
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
            Tool::Zoom => "Zoom Pane",
            Tool::Unzoom => "Unzoom Pane",
            Tool::Close => "Close Split",
        }
    }

    /// The command of the shortcut table the tool's tip names the key of, if it has one.
    fn command(self) -> Option<&'static str> {
        match self {
            Tool::Float | Tool::Dock => Some("float_pane"),
            Tool::Minimize => Some("minimize_floating_pane"),
            Tool::Zoom | Tool::Unzoom => Some("zoom_split"),
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
            Tool::Zoom | Tool::Unzoom => sel!(toggleSplitZoom:),
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
            Tool::of(false, false, false),
            [SplitRight, SplitDown, Float, NewTab, Zoom, Close]
        );
        // A tab's only pane is the tab: no Open in New Tab, and Float stays — dim.
        assert_eq!(
            Tool::of(true, false, false),
            [SplitRight, SplitDown, Float, Zoom, Close]
        );
        // A floating pane docks before it leaves its tab, and minimizes.
        assert_eq!(
            Tool::of(false, true, false),
            [SplitRight, SplitDown, Dock, Minimize, Zoom, Close]
        );
        assert_eq!(Tool::of(true, true, false), Tool::of(false, true, false));
    }

    #[test]
    fn the_pill_stands_as_far_from_the_cards_side_as_from_its_top_and_turns_round_its_corner() {
        // The mark on the grid's line, 8 from the card's side, on a card with a 12.5 corner.
        let pill = Pill::of(8.0, 60.0, 200.0, 12.5);
        assert_eq!((pill.x, pill.y), (4.5, 4.5));
        // Its corner and its distance from the edges add up to the card's: one centre.
        assert_eq!(pill.radius + pill.x.min(pill.y), 12.5);
        // It ends 6 after the title and is the band less its distance above and below tall.
        assert_eq!(pill.x + pill.width, 60.0 + HANDLE_END);
        assert_eq!(pill.rect().size.height, HANDLE_TALL);
        // A card with no corner (full screen) leaves it square, and it is never rounder than a
        // pill: half its height.
        assert_eq!(Pill::of(8.0, 60.0, 200.0, 0.0).radius, 0.0);
        assert_eq!(Pill::of(8.0, 60.0, 200.0, 40.0).radius, HANDLE_TALL / 2.0);
    }

    #[test]
    fn the_handle_changes_the_marks_for_the_pill_and_the_glyph_and_keeps_its_size_when_still() {
        let [pill, mark, carry] = handle_poses(true, false);
        assert_eq!((pill.opacity, pill.scale), (1.0, 1.0));
        assert_eq!((mark.opacity, mark.scale), (0.0, HANDLE_GROW));
        assert_eq!((carry.opacity, carry.scale), (1.0, 1.0));
        let [pill, mark, carry] = handle_poses(false, false);
        assert_eq!((pill.opacity, pill.scale), (0.0, HANDLE_SWELL));
        assert_eq!((mark.opacity, mark.scale), (1.0, 1.0));
        assert_eq!((carry.opacity, carry.scale), (0.0, HANDLE_GROW));
        // Under Reduce Motion only the opacity changes: every piece keeps its whole size, the
        // pieces' opacities are the same as with motion.
        for on in [true, false] {
            let moving = handle_poses(on, false);
            let still = handle_poses(on, true);
            for (still, moving) in still.iter().zip(&moving) {
                assert_eq!(still.scale, 1.0);
                assert_eq!(still.opacity, moving.opacity);
            }
        }
    }

    #[test]
    fn the_pill_ends_where_the_names_place_ends_at_the_latest() {
        // A title that fits, with room to spare, ends 6 after it.
        let roomy = Pill::of(8.0, 60.0, 200.0, 12.5);
        assert_eq!(roomy.x + roomy.width, 60.0 + HANDLE_END);
        // A title cut short by the tools fills its place, and the pill stops with it.
        let cut = Pill::of(8.0, 120.0, 120.0, 12.5);
        assert_eq!(cut.x + cut.width, 120.0);
        // A title that ends 2 short of its place: the place's end, not 6 past the title.
        let near = Pill::of(8.0, 118.0, 120.0, 12.5);
        assert_eq!(near.x + near.width, 120.0);
        // The place leaves less than the mark: the mark keeps its room on both sides.
        let none = Pill::of(8.0, 8.0, 0.0, 12.5);
        assert_eq!(none.x + none.width, 8.0 + MARK + HANDLE_PAD);
    }

    #[test]
    fn zoom_stands_left_of_close_on_every_pane_and_is_unzoom_while_the_pane_fills_the_tab() {
        for (alone, floating) in [(false, false), (true, false), (false, true), (true, true)] {
            for (zoomed, tool) in [(false, Tool::Zoom), (true, Tool::Unzoom)] {
                let tools = Tool::of(alone, floating, zoomed);
                let at = tools.len() - 2;
                assert_eq!(tools[at], tool, "{alone} {floating} {zoomed}");
                assert_eq!(tools[at + 1], Tool::Close);
                assert!(!tools.contains(if zoomed { &Tool::Zoom } else { &Tool::Unzoom }));
            }
        }
        // One command with one key and two names, as Float and Dock are.
        assert_eq!(Tool::Zoom.action(), Tool::Unzoom.action());
        assert_eq!(Tool::Zoom.action(), sel!(toggleSplitZoom:));
        assert_eq!(Tool::Zoom.name(), "Zoom Pane");
        assert_eq!(Tool::Unzoom.name(), "Unzoom Pane");
        let command = bt_shell_common::shortcuts::by_name("zoom_split").expect("in the table");
        assert_eq!(Tool::Zoom.command(), Some(command.name));
        assert_eq!(Tool::Unzoom.command(), Some(command.name));
        // The key is one of the panes' own, ⌥⌘ and a key.
        assert_eq!(command.defaults, ["option+cmd+."]);
    }

    #[test]
    fn a_floating_pane_that_fills_the_tab_has_two_tools_lit_and_the_lone_flat_pane_two_dim() {
        let lit = Marks::of(true, false, true);
        assert!(lit.lit(Tool::Dock) && lit.lit(Tool::Unzoom));
        assert!(!lit.lit(Tool::Close) && !lit.dim(Tool::Unzoom));
        // The flat area's only pane can neither float nor grow: both tools stand dim.
        let alone = Marks::of(false, true, false);
        assert!(alone.dim(Tool::Float) && alone.dim(Tool::Zoom));
        assert!(!alone.lit(Tool::Zoom));
        // Beside other panes nothing is dim, and nothing lit until it floats or fills the tab.
        let plain = Marks::of(false, false, false);
        assert!(!plain.dim(Tool::Float) && !plain.dim(Tool::Zoom) && !plain.lit(Tool::Zoom));
        assert!(Marks::of(false, false, true).lit(Tool::Unzoom));
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
    /// The flat area's only pane: it cannot float, and its Float tool is dim. Its Zoom tool
    /// too: it fills the splits already.
    pub(crate) flat_alone: bool,
    /// The pane fills the tab (Zoom Pane): its Zoom tool is Unzoom, and lit, and its tools stay
    /// in view without the pointer.
    pub(crate) zoomed: bool,
    /// What covers the pane, when a program's view does: its words, icon and way out stand in
    /// the band in place of the pane's title and mark.
    pub(crate) view: Option<ViewHead>,
}

/// What a piece of the handle draws, in its own box.
#[derive(Clone, Copy, PartialEq)]
enum Paint {
    /// The pill: `fill`'s ink at [`HANDLE_FILL`], round by `radius`.
    Pill { fill: u32, radius: f64 },
    /// The mark — over a view the program's own icon, when it gave one —, and the dot on its
    /// corner: the status's `tone` and the `ground` the ring is cut in.
    Mark {
        kind: ProgramKind,
        face: Option<program_face::FaceId>,
        ink: u32,
        dot: Option<(u32, u32)>,
    },
    /// The carry's glyph, where the mark stands.
    Carry { ink: u32 },
}

/// Where a piece of the handle stands in the change: its opacity and its size's share.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pose {
    opacity: f32,
    scale: f64,
}

/// The pill round the name: where it stands in the band and how round it is.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pill {
    x: f64,
    y: f64,
    width: f64,
    radius: f64,
}

impl Pill {
    /// The pill for a mark at `mark_x` and a title that ends at `name_end` (both from the
    /// band's left edge, the card's side), in a name's place that ends at `room_end`, on a card
    /// whose corner is `corner`. It ends [`HANDLE_END`] after the title, but never past its
    /// place: a title that did not fit stops at the tools or Quit, and so does the pill. It
    /// stands as far from the card's side as from its top, and its corner is the card's less that
    /// distance: the two turn round one centre. The number is the card's, never written here.
    fn of(mark_x: f64, name_end: f64, room_end: f64, corner: f64) -> Self {
        let x = mark_x - HANDLE_PAD;
        let y = GUTTER + (TOOL_HEIGHT - HANDLE_TALL) / 2.0 - 0.5;
        // The mark's own room is kept, whatever the place left.
        let end = (name_end + HANDLE_END)
            .min(room_end)
            .max(mark_x + MARK + HANDLE_PAD);
        Self {
            x,
            y,
            width: end - x,
            radius: (corner - x.min(y).max(0.0)).clamp(0.0, HANDLE_TALL / 2.0),
        }
    }

    fn rect(&self) -> NSRect {
        NSRect::new(
            NSPoint::new(self.x, self.y),
            NSSize::new(self.width, HANDLE_TALL),
        )
    }
}

pub(crate) struct PieceIvars {
    paint: Cell<Option<Paint>>,
    /// The pose it was last sent to: what it stands in when it is not on its way.
    pose: Cell<Pose>,
}

define_class!(
    /// A piece of the handle: a layer-backed view of its own, so the change is Core Animation's —
    /// no frame is asked for and no waker touched. It never takes a press: the header does.
    // SAFETY: NSView is designed for subclassing; HeadPiece implements no `Drop` and is born with
    // `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriPaneHeadPiece"]
    #[ivars = PieceIvars]
    struct HeadPiece;

    unsafe impl NSObjectProtocol for HeadPiece {}

    impl HeadPiece {
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
            let Some(paint) = self.ivars().paint.get() else {
                return;
            };
            // The mark's box in the piece's space: the piece stands [`PIECE_ROOM`] out of it.
            let mark = NSRect::new(
                NSPoint::new(PIECE_ROOM, PIECE_ROOM),
                NSSize::new(MARK, MARK),
            );
            match paint {
                // The layer's own fill ([`HeadPiece::paint`]): nothing to draw.
                Paint::Pill { .. } => {}
                Paint::Mark {
                    kind,
                    face,
                    ink,
                    dot,
                } => {
                    match face.and_then(program_face::face) {
                        Some(face) => draw_face(&face, mark),
                        None => draw_mark(kind, mark, &Tint::of(ink, 1.0).color()),
                    }
                    if let Some((tone, ground)) = dot {
                        let centre = NSPoint::new(
                            mark.origin.x + MARK - DOT / 2.0,
                            mark.origin.y + DOT / 2.0,
                        );
                        for (side, color) in [(DOT + 2.0 * DOT_RING, ground), (DOT, tone)] {
                            Tint::of(color, 1.0).color().setFill();
                            NSBezierPath::bezierPathWithOvalInRect(NSRect::new(
                                NSPoint::new(centre.x - side / 2.0, centre.y - side / 2.0),
                                NSSize::new(side, side),
                            ))
                            .fill();
                        }
                    }
                }
                Paint::Carry { ink } => draw_carry(mark, ink),
            }
        }
    }
);

impl HeadPiece {
    fn new(mtm: MainThreadMarker, pose: Pose) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(PieceIvars {
            paint: Cell::new(None),
            pose: Cell::new(pose),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.setWantsLayer(true);
        this.setAccessibilityElement(false);
        if let Some(layer) = this.layer() {
            set_pose(&layer, pose);
        }
        this
    }

    /// What it draws; redrawn only when that changed. The pill is the layer's own fill and
    /// corner, so that a width that glides keeps its corner round.
    fn paint(&self, paint: Paint) {
        if self.ivars().paint.replace(Some(paint)) == Some(paint) {
            return;
        }
        match (paint, self.layer()) {
            (Paint::Pill { fill, radius }, Some(layer)) => {
                CATransaction::begin();
                CATransaction::setDisableActions(true);
                layer.setBackgroundColor(Some(&Tint::of(fill, HANDLE_FILL).color().CGColor()));
                layer.setCornerRadius(radius);
                CATransaction::commit();
            }
            _ => self.setNeedsDisplay(true),
        }
    }

    /// Whether any of it is to be seen now: the layer as it is on screen, not as it was told
    /// to be. A piece that was never drawn shows nothing.
    fn shows(&self) -> bool {
        // SAFETY: reading the layer's presentation layer, on the main thread; it is `None`
        // before the layer has been drawn.
        self.layer()
            .and_then(|layer| unsafe { layer.presentationLayer() })
            .is_some_and(|now| now.opacity() > 0.01)
    }

    /// Where it stands, in the band's space. A new frame puts its pose on again: AppKit gives a
    /// layer-backed view's layer no transform when its frame changes — a move alone too —, and the
    /// shift of a layer scaled about its middle is of its size. With `glide` a piece that shows
    /// goes to its new place over [`TOOLS_SECS`], the pace of the tools that moved it.
    fn stand(&self, rect: NSRect, glide: bool) {
        if self.frame() == rect {
            return;
        }
        if glide && self.shows() {
            let piece = self.retain();
            animate(TOOLS_SECS, move || piece.animator().setFrame(rect));
        } else {
            self.setFrame(rect);
        }
        if let Some(layer) = self.layer() {
            set_pose(&layer, self.ivars().pose.get());
        }
    }

    /// Goes to `pose` from wherever it is now, over [`HANDLE_SECS`]; Reduce Motion (`still`)
    /// leaves its size as it was and changes only its opacity.
    fn go(&self, pose: Pose, still: bool) {
        self.ivars().pose.set(pose);
        let Some(layer) = self.layer() else {
            return;
        };
        // SAFETY: reading the layer's presentation layer, on the main thread; it is `None` before
        // the layer has been drawn.
        let now = unsafe { layer.presentationLayer() };
        let from = (
            now.as_ref()
                .map_or_else(|| layer.opacity(), |now| now.opacity()),
            if still {
                IDENTITY
            } else {
                now.as_ref()
                    .map_or_else(|| layer.transform(), |now| now.transform())
            },
        );
        set_pose(&layer, pose);
        glide(&layer, from, (layer.opacity(), layer.transform()));
    }
}

/// The handle's three pieces and whether it is in.
struct Handle {
    pill: Retained<HeadPiece>,
    mark: Retained<HeadPiece>,
    carry: Retained<HeadPiece>,
    on: Cell<bool>,
}

impl Handle {
    fn new(mtm: MainThreadMarker) -> Self {
        Self {
            pill: HeadPiece::new(
                mtm,
                Pose {
                    opacity: 0.0,
                    scale: HANDLE_SWELL,
                },
            ),
            mark: HeadPiece::new(
                mtm,
                Pose {
                    opacity: 1.0,
                    scale: 1.0,
                },
            ),
            carry: HeadPiece::new(
                mtm,
                Pose {
                    opacity: 0.0,
                    scale: HANDLE_GROW,
                },
            ),
            on: Cell::new(false),
        }
    }

    /// Each piece with its pose, the handle `on` or off ([`handle_poses`]).
    fn poses(&self, on: bool, still: bool) -> [(&HeadPiece, Pose); 3] {
        let [pill, mark, carry] = handle_poses(on, still);
        [(&self.pill, pill), (&self.mark, mark), (&self.carry, carry)]
    }
}

/// The poses of the pill, the mark and the glyph with the handle `on` or off: the pill comes in
/// and swells, the mark goes out and shrinks, the glyph comes in and grows — the same way back.
/// Under Reduce Motion (`still`) no size changes.
fn handle_poses(on: bool, still: bool) -> [Pose; 3] {
    let size = |scale: f64| if still { 1.0 } else { scale };
    let pose = |opacity: f32, scale: f64| Pose { opacity, scale };
    if on {
        [pose(1.0, 1.0), pose(0.0, size(HANDLE_GROW)), pose(1.0, 1.0)]
    } else {
        [
            pose(0.0, size(HANDLE_SWELL)),
            pose(1.0, 1.0),
            pose(0.0, size(HANDLE_GROW)),
        ]
    }
}

/// The transform that scales `layer` by `scale` about its middle: [`card::fitted`]'s, which asks
/// the layer itself which way its anchor and its axes lie, from the layer's frame scaled about
/// its centre to the frame it has. `IDENTITY` when no reading lands — a piece that does not
/// swell beats one that jumps.
fn centred(layer: &CALayer, scale: f64) -> CATransform3D {
    if (scale - 1.0).abs() < f64::EPSILON {
        return IDENTITY;
    }
    // The layer's own frame, with none of the transform it still has on it.
    CATransaction::begin();
    CATransaction::setDisableActions(true);
    layer.setTransform(IDENTITY);
    let frame = layer.frame();
    CATransaction::commit();
    let whole = Rect::new(
        frame.origin.x,
        frame.origin.y,
        frame.size.width,
        frame.size.height,
    );
    let shrunk = Rect::new(
        whole.x + whole.width * (1.0 - scale) / 2.0,
        whole.y + whole.height * (1.0 - scale) / 2.0,
        whole.width * scale,
        whole.height * scale,
    );
    card::fitted(layer, shrunk, whole).unwrap_or(IDENTITY)
}

/// Puts `layer` in `pose` at once: the model values the animation ends in.
fn set_pose(layer: &CALayer, pose: Pose) {
    let transform = centred(layer, pose.scale);
    CATransaction::begin();
    CATransaction::setDisableActions(true);
    layer.setOpacity(pose.opacity);
    layer.setTransform(transform);
    CATransaction::commit();
}

/// Plays `layer` from `from` to `to` (opacity, transform) over [`HANDLE_SECS`] on
/// [`HANDLE_CURVE`]. The model is already at `to`; the animations remove themselves.
fn glide(layer: &CALayer, from: (f32, CATransform3D), to: (f32, CATransform3D)) {
    let (a, b, c, d) = HANDLE_CURVE;
    let curve = CAMediaTimingFunction::functionWithControlPoints(a, b, c, d);
    let opacity = |value: f32| NSNumber::numberWithFloat(value);
    card::play_for(
        layer,
        ns_string!("opacity"),
        &opacity(from.0),
        &opacity(to.0),
        ns_string!("bateri.handle.opacity"),
        HANDLE_SECS,
        &curve,
    );
    // SAFETY: plain `NSValue`s around `CATransform3D` values, what `transform` takes.
    let (from, to) = unsafe {
        (
            NSValue::valueWithCATransform3D(from.1),
            NSValue::valueWithCATransform3D(to.1),
        )
    };
    card::play_for(
        layer,
        ns_string!("transform"),
        &from,
        &to,
        ns_string!("bateri.handle.transform"),
        HANDLE_SECS,
        &curve,
    );
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
    /// The pointer is on the pane's name ([`PaneHead::name_rect`]) and on no tool or Quit: where
    /// a press holds the pane.
    over_name: Cell<bool>,
    /// The pill and the two glyphs it changes the mark for: what the pointer on a name that can
    /// be held brings in.
    handle: Handle,
    pressed: Cell<Option<Part>>,
    /// Where the title ends, from the band's left edge: the right end of the pane's name
    /// ([`PaneHead::name_rect`]), as the last layout of the parts left it.
    name_end: Cell<f64>,
    /// Where the title's place ends, as the last layout left it: just before the tools while
    /// they show, before Quit or the grid's margin when they do not. The pill ends there at the
    /// latest ([`Pill::of`]).
    room_end: Cell<f64>,
    /// The band's width and `room_end` when the pill was last placed: a pill that is in and whose
    /// place changed under a band of the same width — the tools came or went — glides to its new
    /// end ([`PaneHead::place_handle`]), a resize does not.
    placed: Cell<(f64, f64)>,
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
            self.leave();
        }

        // What takes the band off the screen — its tab, its window, a hiding — sends no
        // `mouseExited:`; the pointer is asked where it is instead ([`PaneHead::settle_later`]).
        #[unsafe(method(viewDidMoveToWindow))]
        fn view_did_move_to_window(&self) {
            // SAFETY: the superclass's own method, with no arguments.
            let _: () = unsafe { msg_send![super(self), viewDidMoveToWindow] };
            self.settle_later();
        }

        #[unsafe(method(viewDidHide))]
        fn view_did_hide(&self) {
            // SAFETY: the superclass's own method, with no arguments.
            let _: () = unsafe { msg_send![super(self), viewDidHide] };
            self.settle_later();
        }

        #[unsafe(method(viewDidUnhide))]
        fn view_did_unhide(&self) {
            // SAFETY: the superclass's own method, with no arguments.
            let _: () = unsafe { msg_send![super(self), viewDidUnhide] };
            self.settle_later();
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
            // Otherwise the press is decided by the rule: the name of a pane that can be carried
            // may become a carry (the session watches the pointer from here and takes over once
            // it travels), the rest of a band at the content's top takes the window — a pane
            // with nowhere to go has no name to hold —, any other blank only focuses.
            let Some(container) = self.container() else {
                return;
            };
            let on_name = self.on_name(event);
            match container.head_press(self.frame().origin.y, on_name, self.can_carry()) {
                HeadPress::CarryPane => {
                    if let Some(app) = crate::app::delegate(self.mtm()) {
                        app.pane_press(self.pane(), event.locationInWindow());
                    }
                }
                HeadPress::MoveWindow => {
                    if let Some(window) = self.window() {
                        super::move_window(&window, event);
                    }
                }
                HeadPress::Focus => {}
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
        let handle = Handle::new(mtm);
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
            over_name: Cell::new(false),
            handle,
            quit_lit: Cell::new(false),
            pressed: Cell::new(None),
            name_end: Cell::new(0.0),
            room_end: Cell::new(0.0),
            placed: Cell::new((0.0, 0.0)),
            title,
            tools: tools.clone(),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        // The pill under the title, the mark and the glyph over it, the tools over all.
        this.addSubview(&this.ivars().handle.pill);
        this.addSubview(this.ivars().title.view());
        this.ivars().title.field().setAccessibilityElement(false);
        this.addSubview(&this.ivars().handle.mark);
        this.addSubview(&this.ivars().handle.carry);
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

    /// Where the tools stand in the band, in its own space: what a picture of the band is cut
    /// at for the part that ends at the pane's right edge ([`crate::zoom_motion::Hold`]).
    pub(crate) fn tools_frame(&self) -> NSRect {
        self.ivars().tools.frame()
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
        let (alone, floating, zoomed) = iv
            .look
            .borrow()
            .as_ref()
            .map_or((false, false, false), |look| {
                (look.alone, look.floating, look.zoomed)
            });
        let tools = Tool::of(alone, floating, zoomed);
        let right = self.quit_box().map_or_else(
            || self.bounds().size.width - iv.margins.get().1,
            |quit| quit.origin.x - QUIT_GAP,
        );
        let count = tools.len() as f64;
        let all = count * TOOL_WIDTH + (count - 1.0) * TOOL_GAP;
        let start = right - all;
        tools
            .into_iter()
            .enumerate()
            .map(|(index, tool)| {
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

    /// The pane's name, in the band's space: the mark and the title, from the band's left edge
    /// (the card's corner is the pane's too) to where the title ends — its natural width, or
    /// what is left of it for the tools or Quit — and the band's whole height. A pane without a
    /// title keeps the mark's. What a press must be on to hold the pane.
    fn name_rect(&self) -> NSRect {
        let mark = self.mark_rect();
        let right = self.ivars().name_end.get().max(mark.origin.x + MARK);
        NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(right, self.bounds().size.height),
        )
    }

    /// The pointer is off the band: no tool is lit, no tools show, no name is under it.
    fn leave(&self) {
        let iv = self.ivars();
        iv.hot.set(false);
        iv.tools.set_hot(None);
        if iv.quit_lit.replace(false) {
            self.setNeedsDisplay(true);
        }
        iv.over_name.set(false);
        self.lay_out_parts();
    }

    /// Where the pointer is on the band, if it is on it: the band is on screen in a visible
    /// window of the active application, which is when AppKit sends its pointer events.
    fn pointer_here(&self) -> Option<NSPoint> {
        let window = self.window()?;
        if self.isHiddenOrHasHiddenAncestor()
            || !window.isVisible()
            || !NSApplication::sharedApplication(self.mtm()).isActive()
        {
            return None;
        }
        let at = self.convertPoint_fromView(window.mouseLocationOutsideOfEventStream(), None);
        contains(self.bounds(), at).then_some(at)
    }

    /// Asks the pointer where it is, for what happens under it with no mouse event: a band that
    /// goes with its tab, its window or a hiding, a window or the application that stops being
    /// the one in front, a carry that ends over it, a pane that can or can no longer be carried.
    /// Whatever the pointer's place decided — the tools, the lit part, the name under it — is
    /// decided again, and the handle with it ([`PaneHead::sync_handle`]).
    pub(crate) fn settle_pointer(&self) {
        let iv = self.ivars();
        match self.pointer_here() {
            Some(at) => {
                if !iv.hot.get() {
                    iv.hot.set(true);
                    self.lay_out_parts();
                }
                self.point_over(at);
            }
            None => {
                if iv.hot.get() || iv.over_name.get() {
                    self.leave();
                }
            }
        }
        self.sync_handle();
    }

    /// [`PaneHead::settle_pointer`] one turn of the main queue later, for what calls it from
    /// inside AppKit's own changes of the views, where the pane's state is being written.
    fn settle_later(&self) {
        if let Some(app) = crate::app::delegate(self.mtm()) {
            app.settle_heads_later();
        }
    }

    /// Whether the event's point is on the pane's name ([`PaneHead::name_rect`]).
    fn on_name(&self, event: &NSEvent) -> bool {
        contains(self.name_rect(), self.point_of(event))
    }

    /// The event's point, in the band's space.
    fn point_of(&self, event: &NSEvent) -> NSPoint {
        self.convertPoint_fromView(event.locationInWindow(), None)
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
        // The tools show under the pointer, and all the while their pane fills the tab.
        let shown = iv.hot.get() || look.zoomed;
        let boxes = self.tool_boxes();
        let x = self.mark_rect().origin.x + MARK + tabs::ICON_GAP + 2.0;
        let end = if shown {
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
        iv.name_end
            .set(x + natural.width.ceil().min((end - x).max(0.0)));
        iv.room_end.set(end);
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
            Marks::of(look.floating, look.flat_alone, look.zoomed),
        );
        let still = crate::app::delegate(self.mtm()).is_some_and(|app| app.reduce_motion());
        fade(
            &iv.tools,
            f64::from(u8::from(shown)),
            if still { 0.0 } else { TOOLS_SECS },
        );
        self.place_handle(look, palette, still);
    }

    /// Where the handle's pieces stand and what they draw, after every layout (`still`: Reduce
    /// Motion is on); then whether the pointer brings the handle in or out is asked again
    /// ([`PaneHead::sync_handle`]).
    fn place_handle(&self, look: &HeadLook, palette: Palette, still: bool) {
        let iv = self.ivars();
        let handle = &iv.handle;
        let mark = self.mark_rect();
        let name_end = iv.name_end.get().max(mark.origin.x + MARK);
        let pill = Pill::of(
            mark.origin.x,
            name_end,
            iv.room_end.get(),
            card::corner_pt(),
        );
        // The tools coming or going move the pill's end; it follows them at their pace while it
        // shows, and at once when it does not or when the band itself changed size.
        let span = (self.bounds().size.width, iv.room_end.get());
        let before = iv.placed.replace(span);
        let glide = !still && before.0 == span.0 && before.1 != span.1;
        let ink = if look.focused {
            palette.title
        } else {
            palette.dim
        };
        let room = |rect: NSRect| card::grown(rect, PIECE_ROOM);
        handle.pill.stand(pill.rect(), glide);
        handle.pill.paint(Paint::Pill {
            fill: palette.title,
            radius: pill.radius,
        });
        handle.mark.stand(room(mark), false);
        handle.mark.paint(Paint::Mark {
            kind: look.kind,
            face: look.view.as_ref().and_then(|view| view.face),
            ink,
            dot: look
                .indicator
                .map(|indicator| (palette.tone(tabs::list_dot(indicator), 0), iv.ground.get())),
        });
        handle.carry.stand(room(mark), false);
        handle.carry.paint(Paint::Carry { ink });
        self.sync_handle();
    }

    /// Brings the handle in when the pointer is on a name that can be held and out when it is
    /// not, over [`HANDLE_SECS`]. Asked after every layout too, and a handle already where it
    /// is asked to be is left alone: a layout under a still pointer does not play it again.
    fn sync_handle(&self) {
        let iv = self.ivars();
        let on = iv.over_name.get() && self.can_carry();
        let handle = &iv.handle;
        if handle.on.replace(on) == on {
            return;
        }
        let still = crate::app::delegate(self.mtm()).is_some_and(|app| app.reduce_motion());
        for (piece, pose) in handle.poses(on, still) {
            piece.go(pose, still);
        }
    }

    /// The pointer moved on the band: the tool or Quit under it is lit and named, and the name
    /// under it takes the carry's glyph.
    fn point_at(&self, event: &NSEvent) {
        self.point_over(self.point_of(event));
    }

    /// [`PaneHead::point_at`] for the point `at`, in the band's space.
    fn point_over(&self, at: NSPoint) {
        let part = self.part_under(at);
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
        let named = part.is_none() && contains(self.name_rect(), at);
        if iv.over_name.replace(named) != named {
            self.sync_handle();
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
    /// show (the pointer is on the band, or the pane fills the tab).
    fn part_at(&self, event: &NSEvent) -> Option<Part> {
        self.part_under(self.point_of(event))
    }

    /// [`PaneHead::part_at`] for the point `at`, in the band's space.
    fn part_under(&self, at: NSPoint) -> Option<Part> {
        let inside = |rect: NSRect| {
            at.x >= rect.origin.x
                && at.x <= rect.origin.x + rect.size.width
                && at.y >= rect.origin.y
                && at.y <= rect.origin.y + rect.size.height
        };
        if self.quit_box().is_some_and(inside) {
            return Some(Part::Quit);
        }
        let zoomed = self
            .ivars()
            .look
            .borrow()
            .as_ref()
            .is_some_and(|look| look.zoomed);
        if !(self.ivars().hot.get() || zoomed) {
            return None;
        }
        self.tool_boxes()
            .into_iter()
            .find_map(|(tool, rect)| inside(rect).then_some(Part::Tool(tool)))
    }

    /// Whether a press on the name would carry the pane: it has somewhere to go and does not
    /// float (a floating pane's band moves its card) — the carry's own answer.
    fn can_carry(&self) -> bool {
        crate::app::delegate(self.mtm())
            .is_some_and(|app| crate::pane_drag::can_carry(&app, self.pane()))
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

    /// The band: its ground, the card's light on its top edge, and Quit over a view. The pill,
    /// the mark with its dot, the glyph, the title and the tools are views of their own over it.
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

/// Which of a pane's tools stand out: in the accent, or faded. Two of each at the most, since
/// a pane's two states are apart: it floats or not, it fills the tab or not.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Marks {
    lit: [Option<Tool>; 2],
    dim: [Option<Tool>; 2],
}

impl Marks {
    /// Dock stands lit while its pane floats and Unzoom while it fills the tab; Float and Zoom
    /// stand dim on the flat area's only pane, which can neither leave the splits nor grow in
    /// them.
    fn of(floating: bool, flat_alone: bool, zoomed: bool) -> Self {
        let lone = !floating && flat_alone;
        Self {
            lit: [
                floating.then_some(Tool::Dock),
                zoomed.then_some(Tool::Unzoom),
            ],
            dim: [
                lone.then_some(Tool::Float),
                (lone && !zoomed).then_some(Tool::Zoom),
            ],
        }
    }

    fn lit(self, tool: Tool) -> bool {
        self.lit.contains(&Some(tool))
    }

    fn dim(self, tool: Tool) -> bool {
        self.dim.contains(&Some(tool))
    }
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
                let dim = marks.dim(tool);
                let lit = marks.lit(tool);
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

/// The design's 12-unit grid drawn 14 pt ([`MARK`]) and centred on a box: where a glyph's points
/// stand and how its strokes are made, for the tools' glyphs and the carry's alike.
struct Grid {
    middle: NSPoint,
}

impl Grid {
    /// Points to a grid unit.
    const UNIT: f64 = 14.0 / 12.0;

    fn of(rect: NSRect) -> Self {
        Self {
            middle: centre(rect),
        }
    }

    /// The point at grid coordinates `x`, `y`.
    fn at(&self, x: f64, y: f64) -> NSPoint {
        NSPoint::new(
            self.middle.x + (x - 6.0) * Self::UNIT,
            self.middle.y + (y - 6.0) * Self::UNIT,
        )
    }

    /// Strokes `path` in the glyphs' one line: round, 1.15 units thick, at `strength`.
    fn stroke(&self, path: &NSBezierPath, color: u32, strength: f64) {
        stroke(path, 1.15 * Self::UNIT, Tint::of(color, strength));
    }
}

/// The glyph of a pane that can be taken: two lines across the box with an arrowhead at each of
/// their four ends, in the mark's place.
fn draw_carry(rect: NSRect, color: u32) {
    let grid = Grid::of(rect);
    let path = NSBezierPath::bezierPath();
    // How far in from the grid's edge an arrow's tip is, and how far back each side of its head.
    const REACH: f64 = 0.9;
    const HEAD: f64 = 1.6;
    path.moveToPoint(grid.at(6.0, REACH));
    path.lineToPoint(grid.at(6.0, 12.0 - REACH));
    path.moveToPoint(grid.at(REACH, 6.0));
    path.lineToPoint(grid.at(12.0 - REACH, 6.0));
    for (tip, inward) in [
        ((6.0, REACH), (0.0, 1.0)),
        ((6.0, 12.0 - REACH), (0.0, -1.0)),
        ((REACH, 6.0), (1.0, 0.0)),
        ((12.0 - REACH, 6.0), (-1.0, 0.0)),
    ] {
        let (x, y) = tip;
        let (dx, dy) = inward;
        path.moveToPoint(grid.at(x + (dx - dy) * HEAD, y + (dy + dx) * HEAD));
        path.lineToPoint(grid.at(x, y));
        path.lineToPoint(grid.at(x + (dx + dy) * HEAD, y + (dy - dx) * HEAD));
    }
    grid.stroke(&path, color, 1.0);
}

/// A tool's glyph in its box, on the design's 12-unit grid drawn 14 pt, at `strength`.
fn draw_tool(tool: Tool, rect: NSRect, color: u32, strength: f64) {
    let unit = Grid::UNIT;
    let grid = Grid::of(rect);
    let at = |x: f64, y: f64| grid.at(x, y);
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
        // Two arrows along one diagonal: apart, out to the corners.
        Tool::Zoom => {
            path.moveToPoint(at(4.9, 1.75));
            path.lineToPoint(at(1.75, 1.75));
            path.lineToPoint(at(1.75, 4.9));
            path.moveToPoint(at(1.9, 1.9));
            path.lineToPoint(at(5.1, 5.1));
            path.moveToPoint(at(7.1, 10.25));
            path.lineToPoint(at(10.25, 10.25));
            path.lineToPoint(at(10.25, 7.1));
            path.moveToPoint(at(10.1, 10.1));
            path.lineToPoint(at(6.9, 6.9));
        }
        // The same two, turned to meet in the middle.
        Tool::Unzoom => {
            path.moveToPoint(at(5.1, 2.1));
            path.lineToPoint(at(5.1, 5.1));
            path.lineToPoint(at(2.1, 5.1));
            path.moveToPoint(at(4.95, 4.95));
            path.lineToPoint(at(1.75, 1.75));
            path.moveToPoint(at(6.9, 9.9));
            path.lineToPoint(at(6.9, 6.9));
            path.lineToPoint(at(9.9, 6.9));
            path.moveToPoint(at(7.05, 7.05));
            path.lineToPoint(at(10.25, 10.25));
        }
        Tool::Close => {
            path.moveToPoint(at(3.4, 3.4));
            path.lineToPoint(at(8.6, 8.6));
            path.moveToPoint(at(8.6, 3.4));
            path.lineToPoint(at(3.4, 8.6));
        }
    }
    grid.stroke(&path, color, strength);
}
