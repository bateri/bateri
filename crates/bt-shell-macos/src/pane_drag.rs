//! Carrying a pane: ⌥⌘ held, a pane is taken by its capsule or by any point of its body,
//! carried by a small card, and let go beside another pane, at the window's edge, in the
//! middle of another pane (a swap), on a tab's chip, between chips — in its own window or in
//! another — or over no window at all. The drop is the appliers' ([`TerminalWindow::move_pane`],
//! [`TerminalWindow::swap_panes`], and across windows [`AppDelegate::pane_to_other_tab`] and
//! its kin); nothing here changes a tab itself, only the selection while a chip is waited on.
//!
//! **The press only starts a session.** [`AppDelegate::pane_press`] keeps where
//! the press was and installs a local event monitor for the mouse drag, the
//! release and Esc; from there the monitor owns the gesture, and swallows
//! what it takes, so the view that got the press (a capsule that is about to
//! fade out, a pane that was never told) needs to know nothing. The pointer
//! must travel [`DRAG_SLOP`] before it is a carry: a click that slides a
//! point is still a click, and a ⌥⌘-click carries nothing. Once it is, the
//! keys may be let go — the capsules and the lift go with them, the carry
//! does not need them.
//!
//! **Two halves, one reading.** Inside its window the carry is the monitor's: the card hangs
//! from the pointer. When the pointer is no longer over the window (off it, or under another
//! of ours) the carry becomes a drag session ([`PaneDragSource`], the pane's id under
//! [`pane_type`]) — only a session shows a picture outside the window and goes on when the
//! pointer is over another — and the monitor goes. The session is the system's, so the
//! windows are **read from the outside**: no strip or pane container is a drag destination
//! (nothing of ours accepts the drop; the session always ends with no operation), the source
//! reports every move of the picture and the pointer's place on screen is read against the
//! window under it ([`AppDelegate::window_at`]). The same reading serves both halves
//! ([`Session::read`]), and the drop is carried out one turn after the session ends
//! ([`AppDelegate::pane_flight_ended`], [`Session::finish`]). A carry that comes back over its
//! own window goes on there with its regions (the picture stays the system's).
//!
//! **Nothing moves while a pane is carried.** What the pointer asks for is
//! read against the layout as it stands and answered by [`Tree::verdict`]
//! (the one answer the drop acts on, in the container's room): a region
//! painted over the target — its name, and exactly where the pane would
//! stand — or a refusal ("Too small", with the edges that would take it
//! drawn dashed as "Fits here"), or one warning when it fits nowhere. The
//! carried pane keeps its place under a veil with a dashed outline. No frame
//! is set, so no program is resized until the drop, where the panes settle in
//! one layout and slide to their places ([`SplitView::rearrange`]).
//!
//! **The strip is a place to let go too.** Over a tab bar the pane is over no pane: the
//! middle of a chip lights it and, waited on ([`SPRING_DELAY`]), opens its tab — in another
//! window it brings that window up — the regions
//! then answer in that tab, and a carry that ends without landing there puts the selection
//! back — while between chips room opens. Letting go on a chip is
//! [`TerminalWindow::pane_to_tab`] (the selection stays), between chips
//! [`TerminalWindow::pane_to_new_tab`], and in the panes of a tab opened that way
//! [`TerminalWindow::pane_to_tab_at`] with the landing the regions showed; in another
//! window the same three are [`AppDelegate::pane_to_other_tab`] and
//! [`AppDelegate::pane_to_other_new_tab`]. Let go over no window of ours, a pane becomes a
//! window there ([`AppDelegate::pane_to_window`]); a window's last pane goes to another window
//! and leaves its own empty, which closes.
//!
//! **A drop that does nothing flies back**: the card goes to where the pane
//! stands and fades out, in [`FLY_SECS`]; under Reduce Motion it just goes. (A carry
//! that has left its window has no card: its picture is the system's and just goes.)
//!
//! The drawing is AppKit's, a transparent view over the panes ([`Zones`])
//! and the card on the window's content view ([`Card`]); neither asks for a
//! GPU frame.

use std::cell::RefCell;
use std::ptr::NonNull;

use block2::RcBlock;
use bt_core::{Theme, contrast_ratio};
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAnimatablePropertyContainer, NSAutoresizingMaskOptions, NSBezierPath, NSColor, NSCursor,
    NSDragOperation, NSDraggingContext, NSDraggingSession, NSDraggingSource, NSEvent, NSEventMask,
    NSEventType, NSFont, NSFontAttributeName, NSFontWeightSemibold, NSForegroundColorAttributeName,
    NSGraphicsContext, NSImage, NSShadow, NSStringDrawing, NSView,
};
use objc2_foundation::{
    NSAttributedStringKey, NSDictionary, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize,
    NSString, ns_string,
};

use crate::app::{self, AppDelegate};
use crate::arrange;
use crate::card::corner_pt;
use crate::split::{Direction, Rect, Tree, Verdict};
use crate::tab_bar::{self, PaneTarget, Tint};
use crate::tabs::{DRAG_SLOP, SPRING_DELAY};
use crate::window::{Joins, TerminalWindow, pane_name};

/// The card's flight back to its pane, and how fast it fades when the drop
/// worked and the panes take over.
const FLY_SECS: f64 = 0.20;
const FADE_SECS: f64 = 0.12;
/// Where the card hangs from the pointer: its top-left corner, this far right
/// of and below it — the pane's place under the pointer stays in sight.
const CARD_OFFSET: f64 = 16.0;
const CARD_HEADER: f64 = 24.0;
const CARD_CORNER: f64 = 9.0;
/// How far inside a card its dashed outline runs (the carried pane's place,
/// a block's panes in their region), in points.
const OUTLINE_INSET: f64 = 4.0;
/// Keyboard code of Esc.
const ESCAPE: u16 = 53;

// ─── What the preview says (pure) ────────────────────────────────────────

/// The kinds of region the preview paints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Where the pane would stand: a solid line round a light fill.
    Lands,
    /// Where it was pointed and cannot go: hatched, dashed, in the error tone.
    Refused,
    /// An edge of the window that would take it: dashed, quiet.
    Suggests,
}

/// One region of the preview: the rectangle (the container's points, top-down)
/// and the word in its middle.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Mark {
    pub rect: Rect,
    pub kind: Kind,
    pub label: String,
}

/// What the preview paints for a verdict: the regions and, when the pane fits
/// nowhere, the one warning over the whole area.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Preview {
    pub marks: Vec<Mark>,
    pub warning: Option<String>,
    /// Where each pane of a block would stand once it has landed, outlined dashed inside
    /// the region (a pane carried alone is its region: none).
    pub inner: Vec<Rect>,
}

/// The warning for a carry that fits nowhere: how many panes are being carried.
pub(crate) fn no_room(panes: usize) -> String {
    if panes == 1 {
        "Not enough room for 1 split".to_owned()
    } else {
        format!("Not enough room for {panes} splits")
    }
}

/// What `verdict` looks like: the same arithmetic the drop will act on, so what
/// is shown is what happens.
pub(crate) fn preview(verdict: &Verdict, carried: usize) -> Preview {
    let mark = |rect: Rect, kind: Kind, label: &str| Mark {
        rect,
        kind,
        label: label.to_owned(),
    };
    match verdict {
        Verdict::Nothing => Preview::default(),
        Verdict::Lands { zone, placement } => Preview {
            marks: vec![mark(
                placement.landing,
                Kind::Lands,
                zone.label().unwrap_or_default(),
            )],
            warning: None,
            inner: Vec::new(),
        },
        Verdict::Swaps { frame, fits, .. } => Preview {
            marks: vec![if *fits {
                mark(*frame, Kind::Lands, "Swap")
            } else {
                mark(*frame, Kind::Refused, "Too small")
            }],
            warning: None,
            inner: Vec::new(),
        },
        Verdict::TooSmall { region, edges, .. } => {
            let mut marks = vec![mark(*region, Kind::Refused, "Too small")];
            marks.extend(
                edges
                    .iter()
                    .map(|(_, landing)| mark(*landing, Kind::Suggests, "Fits here")),
            );
            Preview {
                marks,
                warning: None,
                inner: Vec::new(),
            }
        }
        Verdict::NoRoom => Preview {
            marks: Vec::new(),
            warning: Some(no_room(carried)),
            inner: Vec::new(),
        },
    }
}

/// What letting go at a verdict carries out, if anything.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Act {
    /// The panes take the places of the placement's tree.
    Move(crate::split::Tree),
    /// The carried pane and `target` trade places.
    Swap(u64),
    /// Nothing: the card flies back.
    Back,
}

/// The act `verdict` asks for.
pub(crate) fn act_of(verdict: &Verdict) -> Act {
    match verdict {
        Verdict::Lands { placement, .. } => Act::Move(placement.tree.clone()),
        Verdict::Swaps {
            target, fits: true, ..
        } => Act::Swap(*target),
        _ => Act::Back,
    }
}

/// What the strip is told the pointer is over, and the chip it waits on, for a pane of tab
/// `source` carried over `over`: a chip of the pane's own tab is not lit (letting go there is
/// taking the pane back), and a chip of a tab that is not on screen (`on_screen`) is waited on
/// — its tab opens when the wait is out, a tab already open needs no wait.
fn strip_reading(
    over: Option<PaneTarget>,
    source: u64,
    on_screen: impl Fn(u64) -> bool,
) -> (Option<PaneTarget>, Option<u64>) {
    let lit = match over {
        Some(PaneTarget::Tab(chip)) if chip == source => None,
        other => other,
    };
    let wait = match over {
        Some(PaneTarget::Tab(chip)) if !on_screen(chip) => Some(chip),
        _ => None,
    };
    (lit, wait)
}

/// Where letting go at `over` puts a pane of tab `source`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Lands {
    /// Into tab `0`, beside its focused pane.
    Tab(u64),
    /// Into a tab of its own at the strip's slot `0` (a tab's only pane moves its tab).
    NewTab(usize),
    /// Over its own tab's chip: nowhere, the card flies back.
    Back,
    /// Not over the strip: the panes of the tab on screen answer.
    Panes,
}

fn lands_at(over: Option<PaneTarget>, source: u64) -> Lands {
    match over {
        Some(PaneTarget::Tab(chip)) if chip == source => Lands::Back,
        Some(PaneTarget::Tab(chip)) => Lands::Tab(chip),
        Some(PaneTarget::Between(gap)) => Lands::NewTab(gap),
        None => Lands::Panes,
    }
}

/// The card's size for a pane of `pane` points: as wide as a quarter of the
/// pane within a range, with the pane's proportions below the header.
pub(crate) fn card_size(pane: NSSize) -> NSSize {
    let width = (pane.width * 0.25).clamp(150.0, 230.0);
    let ratio = if pane.width > 0.0 {
        pane.height / pane.width
    } else {
        0.6
    };
    let body = (width * ratio).clamp(54.0, 120.0);
    NSSize::new(width.round(), (CARD_HEADER + body).round())
}

/// Black or white ink for text on a fill of `rgb`: whichever contrasts more
/// ([`bt_core::contrast_ratio`], the measure the theme's own choices are made by).
pub(crate) fn ink_on(rgb: u32) -> u32 {
    if contrast_ratio(rgb, 0x000000) >= contrast_ratio(rgb, 0xffffff) {
        0x000000
    } else {
        0xffffff
    }
}

/// Whether the pointer has travelled far enough from the press to be a carry.
pub(crate) fn past_slop(from: NSPoint, to: NSPoint) -> bool {
    (to.x - from.x).hypot(to.y - from.y) >= DRAG_SLOP
}

fn rect_of(rect: Rect) -> NSRect {
    NSRect::new(
        NSPoint::new(rect.x, rect.y),
        NSSize::new(rect.width, rect.height),
    )
}

// ─── The colours ─────────────────────────────────────────────────────────

/// The preview's colours, from the theme's roles: the accent for where a pane
/// stands and where it may, the error tone for where it cannot, the
/// foreground and background for the card and the veil.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Colors {
    accent: u32,
    error: u32,
    ground: u32,
    ink: u32,
    dim: u32,
}

impl Colors {
    fn of(theme: &Theme) -> Self {
        Self {
            accent: theme.accent,
            error: theme.error,
            ground: theme.background,
            ink: theme.foreground,
            dim: theme.dim,
        }
    }
}

// ─── The regions ─────────────────────────────────────────────────────────

/// What [`Zones`] paints.
#[derive(Clone, Debug, Default)]
pub(crate) struct Picture {
    /// The carried pane's place, under a veil with a dashed outline.
    source: Option<NSRect>,
    preview: Preview,
}

pub(crate) struct ZonesIvars {
    colors: Colors,
    picture: RefCell<Picture>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; Zones implements no `Drop`
    // and is born with `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriCarryZones"]
    #[ivars = ZonesIvars]
    pub(crate) struct Zones;

    unsafe impl NSObjectProtocol for Zones {}

    impl Zones {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        /// A picture over the panes, never a target: every press goes
        /// through it.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let iv = self.ivars();
            let colors = iv.colors;
            let picture = iv.picture.borrow();
            if let Some(source) = picture.source {
                paint_source(source, colors);
            }
            for mark in &picture.preview.marks {
                paint_mark(mark, colors);
            }
            for rect in &picture.preview.inner {
                paint_inner(rect_of(*rect), colors);
            }
            if let Some(text) = &picture.preview.warning {
                paint_warning(self.bounds(), text, colors);
            }
        }
    }
);

impl Zones {
    /// The layer over a container of `frame`: with `source`, the carried pane's own place
    /// in it, veiled; without, a container the carried thing is not from.
    pub(crate) fn new(
        mtm: MainThreadMarker,
        frame: NSRect,
        theme: &Theme,
        source: Option<NSRect>,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ZonesIvars {
            colors: Colors::of(theme),
            picture: RefCell::new(Picture {
                source,
                preview: Preview::default(),
            }),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        this.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        this
    }

    /// Paints `preview` over the panes.
    pub(crate) fn show(&self, preview: Preview) {
        self.ivars().picture.borrow_mut().preview = preview;
        self.setNeedsDisplay(true);
    }
}

/// The carried pane's place: its card under a veil of the ground, a dashed
/// outline inside.
fn paint_source(rect: NSRect, colors: Colors) {
    tab_bar::rounded(rect, corner_pt(), Tint::of(colors.ground, 0.78), None);
    let path = outline(rect);
    dashed(&path);
    path.setLineWidth(1.2);
    Tint::of(colors.ink, 0.35).color().setStroke();
    path.stroke();
}

/// A card's place, rounded as the card is: a region is where a card will
/// stand, the veil is over one, and a different corner read as a second shape
/// laid on the card.
fn card_path(rect: NSRect) -> Retained<NSBezierPath> {
    NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect, corner_pt(), corner_pt())
}

/// The dashed outline [`OUTLINE_INSET`] inside the card at `rect`: concentric
/// with the card's corner, the corner less the inset.
fn outline(rect: NSRect) -> Retained<NSBezierPath> {
    let inner = NSRect::new(
        NSPoint::new(rect.origin.x + OUTLINE_INSET, rect.origin.y + OUTLINE_INSET),
        NSSize::new(
            (rect.size.width - 2.0 * OUTLINE_INSET).max(0.0),
            (rect.size.height - 2.0 * OUTLINE_INSET).max(0.0),
        ),
    );
    let corner = (corner_pt() - OUTLINE_INSET).max(0.0);
    NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(inner, corner, corner)
}

/// A dash pattern on `path`: five points on, four off.
fn dashed(path: &NSBezierPath) {
    let mut pattern = [5.0_f64, 4.0_f64];
    // SAFETY: `pattern` holds the two lengths the count says, alive for the call;
    // AppKit copies them.
    unsafe { path.setLineDash_count_phase(pattern.as_mut_ptr(), 2, 0.0) };
}

/// One region: its fill, its line and its word.
fn paint_mark(mark: &Mark, colors: Colors) {
    let rect = rect_of(mark.rect);
    let path = card_path(rect);
    let tone = match mark.kind {
        Kind::Lands | Kind::Suggests => colors.accent,
        Kind::Refused => colors.error,
    };
    match mark.kind {
        Kind::Lands => {
            Tint::of(tone, 0.16).color().setFill();
            path.fill();
        }
        Kind::Suggests => {
            Tint::of(tone, 0.06).color().setFill();
            path.fill();
            dashed(&path);
        }
        Kind::Refused => {
            Tint::of(tone, 0.04).color().setFill();
            path.fill();
            hatch(&path, rect, Tint::of(tone, 0.14));
            dashed(&path);
        }
    }
    path.setLineWidth(1.2);
    Tint::of(tone, 0.9).color().setStroke();
    path.stroke();
    pill(rect, &mark.label, tone);
}

/// One pane of a block that has landed, outlined dashed inside its region.
fn paint_inner(rect: NSRect, colors: Colors) {
    let path = outline(rect);
    dashed(&path);
    path.setLineWidth(1.0);
    Tint::of(colors.accent, 0.7).color().setStroke();
    path.stroke();
}

/// Diagonal stripes, five points wide every ten, inside `path`.
fn hatch(path: &NSBezierPath, rect: NSRect, ink: Tint) {
    NSGraphicsContext::saveGraphicsState_class();
    path.addClip();
    let stripes = NSBezierPath::bezierPath();
    let reach = rect.size.width + rect.size.height;
    let mut offset = -rect.size.height;
    while offset < reach {
        let x = rect.origin.x + offset;
        stripes.moveToPoint(NSPoint::new(x, rect.origin.y + rect.size.height));
        stripes.lineToPoint(NSPoint::new(x + rect.size.height, rect.origin.y));
        offset += 10.0;
    }
    stripes.setLineWidth(5.0);
    ink.color().setStroke();
    stripes.stroke();
    NSGraphicsContext::restoreGraphicsState_class();
}

/// The attributes of a pill's word: the small semibold system font in `ink`.
fn pill_attributes(ink: u32) -> Retained<NSDictionary<NSAttributedStringKey, AnyObject>> {
    // SAFETY: AppKit's constant font weight.
    let font = NSFont::systemFontOfSize_weight(11.0, unsafe { NSFontWeightSemibold });
    let color = Tint::of(ink, 1.0).color();
    let values: [&AnyObject; 2] = [font.as_ref(), color.as_ref()];
    // SAFETY: AppKit's font and colour attribute keys (extern statics) with the
    // `NSFont` and `NSColor` they document.
    unsafe {
        NSDictionary::<NSAttributedStringKey, AnyObject>::from_slices(
            &[NSFontAttributeName, NSForegroundColorAttributeName],
            &values,
        )
    }
}

/// A word on a rounded fill of `tone`, in the middle of `rect`.
fn pill(rect: NSRect, text: &str, tone: u32) {
    if text.is_empty() {
        return;
    }
    let attributes = pill_attributes(ink_on(tone));
    let word = NSString::from_str(text);
    // SAFETY: the dictionary holds a font and a colour under their keys.
    let size = unsafe { word.sizeWithAttributes(Some(&attributes)) };
    let (padding_x, padding_y) = (9.0, 3.0);
    let pill = NSRect::new(
        NSPoint::new(
            rect.origin.x + (rect.size.width - size.width) / 2.0 - padding_x,
            rect.origin.y + (rect.size.height - size.height) / 2.0 - padding_y,
        ),
        NSSize::new(size.width + 2.0 * padding_x, size.height + 2.0 * padding_y),
    );
    tab_bar::rounded(pill, pill.size.height / 2.0, Tint::of(tone, 0.95), None);
    // SAFETY: the same dictionary; drawing happens inside `drawRect:`.
    unsafe {
        word.drawAtPoint_withAttributes(
            NSPoint::new(pill.origin.x + padding_x, pill.origin.y + padding_y),
            Some(&attributes),
        );
    }
}

/// The one warning: the whole area hatched, the words in the middle.
fn paint_warning(bounds: NSRect, text: &str, colors: Colors) {
    let path = card_path(bounds);
    Tint::of(colors.error, 0.04).color().setFill();
    path.fill();
    hatch(&path, bounds, Tint::of(colors.error, 0.14));
    pill(bounds, text, colors.error);
}

// ─── The card ────────────────────────────────────────────────────────────

struct CardIvars {
    colors: Colors,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; Card implements no `Drop`
    // and is born with `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriCarryCard"]
    #[ivars = CardIvars]
    struct Card;

    unsafe impl NSObjectProtocol for Card {}

    impl Card {
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
            let colors = self.ivars().colors;
            let bounds = self.bounds();
            tab_bar::rounded(
                bounds,
                CARD_CORNER,
                Tint::of(colors.ground, 0.97),
                Some(Tint::of(colors.ink, 0.2)),
            );
            let line = NSBezierPath::bezierPath();
            line.moveToPoint(NSPoint::new(1.0, CARD_HEADER + 0.5));
            line.lineToPoint(NSPoint::new(bounds.size.width - 1.0, CARD_HEADER + 0.5));
            tab_bar::stroke(&line, 1.0, Tint::of(colors.ink, 0.08));
            arrange::draw_grip(4.0, CARD_HEADER / 2.0, Tint::of(colors.dim, 1.0));
            // The body: a few quiet lines, a terminal's look.
            let width = bounds.size.width - 20.0;
            let mut y = CARD_HEADER + 10.0;
            for share in [0.62, 0.9, 0.45, 0.74, 0.3] {
                if y + 4.0 > bounds.size.height - 6.0 {
                    break;
                }
                let bar = NSRect::new(NSPoint::new(10.0, y), NSSize::new(width * share, 3.0));
                tab_bar::rounded(bar, 1.5, Tint::of(colors.dim, 0.4), None);
                y += 10.0;
            }
        }
    }
);

impl Card {
    fn new(mtm: MainThreadMarker, frame: NSRect, theme: &Theme, name: &str) -> Retained<Self> {
        let colors = Colors::of(theme);
        let this = Self::alloc(mtm).set_ivars(CardIvars { colors });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        this.setWantsLayer(true);
        let shadow = NSShadow::new();
        shadow.setShadowBlurRadius(18.0);
        shadow.setShadowOffset(NSSize::new(0.0, -6.0));
        shadow.setShadowColor(Some(&NSColor::colorWithSRGBRed_green_blue_alpha(
            0.0, 0.0, 0.0, 0.5,
        )));
        this.setShadow(Some(&shadow));
        let title = arrange::label(mtm, name, 11.5, true, Tint::of(colors.ink, 1.0));
        let height = title.fittingSize().height.ceil();
        title.setFrame(NSRect::new(
            NSPoint::new(24.0, ((CARD_HEADER - height) / 2.0).floor()),
            NSSize::new((frame.size.width - 32.0).max(0.0), height),
        ));
        this.addSubview(&title);
        this
    }
}

/// `card`'s frame with its top-left `CARD_OFFSET` right of and below the
/// pointer `at` (window coordinates), in `parent`'s coordinates.
fn card_frame(parent: &NSView, at: NSPoint, size: NSSize) -> NSRect {
    let point = parent.convertPoint_fromView(at, None);
    let y = if parent.isFlipped() {
        point.y + CARD_OFFSET
    } else {
        point.y - CARD_OFFSET - size.height
    };
    NSRect::new(NSPoint::new(point.x + CARD_OFFSET, y), size)
}

// ─── The session ─────────────────────────────────────────────────────────

/// Where the regions are painted now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    /// Over the carried pane's own tab, on its veil.
    Home,
    /// Over another tab: its window and its id.
    Far(u64, u64),
}

/// What a started carry holds.
struct Carry {
    /// The card hanging from the pointer while the carry is inside its window. A carry that
    /// has left it is a drag session's picture ([`PaneDragSource`]) and has none.
    card: Option<Retained<Card>>,
    /// The carried pane's place, veiled, in its own tab — which paints the regions too while
    /// the pointer is over that tab.
    home: Retained<Zones>,
    /// The regions of another tab (window, tab, layer) while the pointer is over it.
    far: Option<(u64, u64, Retained<Zones>)>,
    stage: Stage,
    /// The last verdict, to repaint only when it changed.
    verdict: Verdict,
    /// How many panes are carried (one, today).
    panes: usize,
    /// What the strip was last told the pointer is over (a chip of the pane's own tab is not
    /// lit: letting go there is taking the pane back).
    over: Option<PaneTarget>,
    /// The window whose strip the pointer is in, and so the one that is lit, holds room open
    /// or waits.
    strip: Option<u64>,
    /// The chip the pointer waits on to open its tab, and the generation of that wait: a fire
    /// of an older one finds it stale.
    dwell: Option<u64>,
    wait: u64,
    /// Where a wait opened a tab: the window and the tab that was open there before. Finishing
    /// without a landing in the tab on screen puts each selection back.
    opened: Vec<(u64, u64)>,
    /// A wait brought another window up: finishing without a landing returns the keyboard to
    /// the window the pane is from.
    raised: bool,
    /// Where the pointer was last, in screen points.
    at: NSPoint,
}

impl Carry {
    /// The strip the carry lit lets go: no chip lit, no room held open, no wait.
    fn release_strip(&mut self, app: &AppDelegate) {
        if let Some(window) = self.strip.take().and_then(|id| app.window(id)) {
            window.bar().show_pane_target(None);
            window.bar().dwell(None);
        }
        self.over = None;
        if self.dwell.take().is_some() {
            self.wait = self.wait.wrapping_add(1);
        }
    }

    /// What waits changed is put back when the carry is over: every window where a wait opened
    /// a tab gets the tab it had open (except the one the pane landed in by the regions of the
    /// tab on screen, `stays`), and a window a wait brought up gives the keyboard back to the
    /// pane's own (`home`) unless the pane crossed to another window (`crossed`), where the
    /// landing took it.
    fn put_back(&mut self, app: &AppDelegate, home: u64, stays: Option<u64>, crossed: bool) {
        for (opened, before) in restore_selections(&self.opened, stays) {
            if app.tab(before).is_some()
                && let Some(opened) = app.window(opened)
            {
                opened.select_tab(before);
            }
        }
        self.opened.clear();
        if self.raised && !crossed {
            if let Some(window) = app.window(home) {
                window.select();
            }
        }
        self.raised = false;
    }

    /// Takes everything the carry put on screen off it.
    fn take_down(&mut self) {
        self.home.removeFromSuperview();
        if let Some((_, _, far)) = self.far.take() {
            far.removeFromSuperview();
        }
        if let Some(card) = self.card.take() {
            card.removeFromSuperview();
            NSCursor::pop_class();
        }
    }
}

/// A press that may become a carry, and the carry once it has.
pub(crate) struct Session {
    window: u64,
    tab: u64,
    pane: u64,
    /// Where the press was, in the window's coordinates.
    origin: NSPoint,
    monitor: Option<Retained<AnyObject>>,
    /// The drag session the carry became when it left its window, whose callbacks drive it
    /// from there ([`PaneDragSource`]); kept here because AppKit's hold on it is not
    /// something to lean on.
    source: Option<Retained<PaneDragSource>>,
    carry: Option<Carry>,
}

/// What the session does after an event.
enum Step {
    Keep,
    End,
}

impl Session {
    /// A press on pane `pane` at `at` (window coordinates), if the pane has
    /// anywhere to go: another tab, another pane to be let go beside or
    /// another window.
    pub(crate) fn press(app: &AppDelegate, pane: u64, at: NSPoint) -> Option<Self> {
        let windows = app.windows();
        let (window, tab) = windows
            .iter()
            .find_map(|window| window.tab_holding(pane).map(|tab| (window.clone(), tab)))?;
        if window.tab_count() <= 1 && tab.panes().len() <= 1 && windows.len() <= 1 {
            return None;
        }
        Some(Self {
            window: window.id(),
            tab: tab.id(),
            pane,
            origin: at,
            monitor: watch(),
            source: None,
            carry: None,
        })
    }

    /// Whether the carry has left its window and is a drag session now.
    pub(crate) fn flying(&self) -> bool {
        self.source.is_some()
    }

    /// The window and the tab the pane is in, if they still stand.
    fn place(
        &self,
        app: &AppDelegate,
    ) -> Option<(Retained<TerminalWindow>, Retained<crate::tab::TerminalTab>)> {
        let window = app.window(self.window)?;
        let tab = app.tab(self.tab)?;
        tab.container().pane(self.pane)?;
        Some((window, tab))
    }

    /// An event of the monitor. `true` when the session took it (it goes no
    /// further); the step says whether the session lives on.
    fn handle(&mut self, app: &AppDelegate, event: &NSEvent) -> (bool, Step) {
        if self.flying() {
            // The drag session has the gesture; the monitor is about to go.
            return (false, Step::Keep);
        }
        let Some((window, tab)) = self.place(app) else {
            // The pane or its window went away under the gesture.
            self.abandon();
            return (false, Step::End);
        };
        match event.r#type() {
            NSEventType::KeyDown => {
                // Esc cancels a carry; before one begins it is the program's.
                if event.keyCode() != ESCAPE || self.carry.is_none() {
                    return (false, Step::Keep);
                }
                self.finish(app, &window, &tab, None);
                (true, Step::End)
            }
            NSEventType::LeftMouseDragged | NSEventType::LeftMouseUp => {
                let own = event
                    .window(app.mtm())
                    .is_some_and(|from| &*from == window.ns_window());
                if !own {
                    return (false, Step::Keep);
                }
                let at = event.locationInWindow();
                if event.r#type() == NSEventType::LeftMouseUp {
                    let screen = window.ns_window().convertPointToScreen(at);
                    self.finish(app, &window, &tab, Some(screen));
                    return (true, Step::End);
                }
                if self.carry.is_none() && past_slop(self.origin, at) {
                    self.begin(app, &window, &tab, at);
                }
                if self.carry.is_some() {
                    self.moved(app, &window, at, event);
                }
                (true, Step::Keep)
            }
            _ => (false, Step::Keep),
        }
    }

    /// The pointer went past the slop: the arrangement's lift is set down,
    /// the carried pane's place is veiled and the card hangs from the pointer.
    fn begin(
        &mut self,
        app: &AppDelegate,
        window: &TerminalWindow,
        tab: &crate::tab::TerminalTab,
        at: NSPoint,
    ) {
        let container = tab.container();
        let Some(pane) = container.pane(self.pane) else {
            return;
        };
        let Some(content) = window.ns_window().contentView() else {
            return;
        };
        let Some(theme) = pane.session().map(|session| session.theme()) else {
            return;
        };
        app.pane_drag_started();
        let mtm = app.mtm();
        let home = Zones::new(mtm, container.bounds(), &theme, Some(pane.frame()));
        container.addSubview(&home);
        let size = card_size(pane.frame().size);
        let card = Card::new(
            mtm,
            card_frame(&content, at, size),
            &theme,
            &pane_name(&pane),
        );
        content.addSubview(&card);
        NSCursor::closedHandCursor().push();
        self.carry = Some(Carry {
            card: Some(card),
            home,
            far: None,
            stage: Stage::Home,
            verdict: Verdict::Nothing,
            panes: 1,
            over: None,
            strip: None,
            dwell: None,
            wait: 0,
            opened: Vec::new(),
            raised: false,
            at: window.ns_window().convertPointToScreen(at),
        });
    }

    /// The pointer moved, in the window: the card follows, then the strip and the panes read
    /// where it is. A pointer that is no longer over the window — off it, or under another —
    /// turns the carry into a drag session ([`Self::fly`]).
    fn moved(&mut self, app: &AppDelegate, window: &TerminalWindow, at: NSPoint, event: &NSEvent) {
        let screen = window.ns_window().convertPointToScreen(at);
        let under = app.window_at(screen);
        let leaves = under.as_ref().is_none_or(|under| under.id() != self.window);
        if leaves && self.fly(app, window, event) {
            return;
        }
        let Some(carry) = self.carry.as_mut() else {
            return;
        };
        if let Some(card) = &carry.card {
            // SAFETY: reading the superview; we are on the main thread.
            if let Some(content) = unsafe { card.superview() } {
                card.setFrame(card_frame(&content, at, card.frame().size));
            }
        }
        self.read_in(app, screen, under);
    }

    /// The carry leaves its window: from here it is a drag session, because only a session
    /// finds the window under the pointer when that is another, and shows the pane's picture
    /// outside the window it is from. The card goes, its picture is the session's; the regions
    /// keep being read from where the pointer is ([`Self::read`], by the session's reports).
    /// `false` if it cannot begin (the carry stays where it is).
    fn fly(&mut self, app: &AppDelegate, window: &TerminalWindow, event: &NSEvent) -> bool {
        let Some(content) = window.ns_window().contentView() else {
            return false;
        };
        let Some(carry) = self.carry.as_mut() else {
            return false;
        };
        let Some(card) = carry.card.take() else {
            return false;
        };
        let image = crate::tab_drag::picture(&card);
        let size = card.frame().size;
        card.removeFromSuperview();
        NSCursor::pop_class();
        self.source = Some(begin_flight(
            &content,
            self.pane,
            event,
            image.as_deref(),
            size,
        ));
        if let Some(token) = self.monitor.take() {
            // SAFETY: the token is the one `addLocalMonitor…` gave.
            unsafe { NSEvent::removeMonitor(&token) };
        }
        self.read(app, NSEvent::mouseLocation());
        true
    }

    /// Reads the pointer's place on screen against the windows' strips and panes.
    ///
    /// **In a strip** the pane is not over any pane: the chip it is over the middle of is
    /// lit and, waited on, opens its tab (the line under it fills; [`SPRING_DELAY`]) and — in
    /// another window — brings the window up; between chips room opens, where it would
    /// become a tab. **Elsewhere** the tab on screen of the window under the pointer
    /// answers with its regions: the carried pane's own tab on its veil, any other (a chip
    /// waited on opened it, or it is another window's) with regions drawn in that tab. Over no
    /// window of ours nothing is painted.
    fn read(&mut self, app: &AppDelegate, screen: NSPoint) {
        self.read_in(app, screen, app.window_at(screen));
    }

    /// [`Self::read`] with the window under the pointer already found.
    fn read_in(
        &mut self,
        app: &AppDelegate,
        screen: NSPoint,
        target: Option<Retained<TerminalWindow>>,
    ) {
        let (pane, source) = (self.pane, self.tab);
        let Some(carry) = self.carry.as_mut() else {
            return;
        };
        carry.at = screen;
        let Some(moving) = app.tab(source).and_then(|tab| tab.container().pane(pane)) else {
            return;
        };
        let mut far_in = None;
        let (stage, verdict) = match &target {
            None => {
                carry.release_strip(app);
                (Stage::Home, Verdict::Nothing)
            }
            Some(window) => {
                let at = window.ns_window().convertPointFromScreen(screen);
                if carry.strip != Some(window.id()) {
                    carry.release_strip(app);
                    carry.strip = Some(window.id());
                }
                let bar = window.bar();
                let over = bar.pane_target(at);
                let (lit, wait) = strip_reading(over, source, |chip| window.is_selected(chip));
                if lit != carry.over {
                    bar.show_pane_target(lit);
                    carry.over = lit;
                }
                if wait != carry.dwell {
                    carry.dwell = wait;
                    carry.wait = carry.wait.wrapping_add(1);
                    bar.dwell(wait);
                    if let Some(chip) = wait {
                        let generation = carry.wait;
                        arrange::after(SPRING_DELAY, move |app| {
                            app.pane_drag_dwell(generation, chip);
                        });
                    }
                }
                match window.try_selected_tab() {
                    None => (Stage::Home, Verdict::Nothing),
                    Some(shown) => {
                        let verdict = if over.is_some() {
                            Verdict::Nothing
                        } else if shown.id() == source {
                            shown.container().verdict(pane, at)
                        } else {
                            shown.container().verdict_of(
                                &Tree::Leaf(pane),
                                std::slice::from_ref(&moving),
                                at,
                            )
                        };
                        if shown.id() == source {
                            (Stage::Home, verdict)
                        } else {
                            let stage = Stage::Far(window.id(), shown.id());
                            far_in = Some(shown);
                            (stage, verdict)
                        }
                    }
                }
            }
        };
        if stage != carry.stage {
            // The regions go where the pointer is.
            match carry.stage {
                Stage::Home => carry.home.show(Preview::default()),
                Stage::Far(..) => {
                    if let Some((_, _, far)) = carry.far.take() {
                        far.removeFromSuperview();
                    }
                }
            }
            if let (Stage::Far(window, tab), Some(shown)) = (stage, &far_in) {
                let Some(theme) = moving.session().map(|session| session.theme()) else {
                    return;
                };
                let far = Zones::new(app.mtm(), shown.container().bounds(), &theme, None);
                shown.container().addSubview(&far);
                carry.far = Some((window, tab, far));
            }
            carry.stage = stage;
            carry.verdict = Verdict::Nothing;
        }
        if verdict != carry.verdict {
            let layer = match carry.stage {
                Stage::Home => Some(&carry.home),
                Stage::Far(..) => carry.far.as_ref().map(|(_, _, far)| far),
            };
            if let Some(layer) = layer {
                layer.show(preview(&verdict, carry.panes));
            }
            carry.verdict = verdict;
        }
    }

    /// A chip has been waited on for [`SPRING_DELAY`]: its tab opens — and its window comes up
    /// if it is not the one in front — and the regions move to it.
    fn dwell_fired(&mut self, app: &AppDelegate, wait: u64, chip: u64) {
        let stale = self
            .carry
            .as_ref()
            .is_none_or(|carry| carry.wait != wait || carry.dwell != Some(chip));
        if stale {
            return;
        }
        let Some(carry) = self.carry.as_mut() else {
            return;
        };
        let Some(window) = carry.strip.and_then(|id| app.window(id)) else {
            return;
        };
        let before = window.try_selected_tab().map(|tab| tab.id());
        if !window.select_tab(chip) {
            // The tab will not open (the window holds a question): the wait is over.
            carry.dwell = None;
            window.bar().dwell(None);
            return;
        }
        if !window.ns_window().isKeyWindow() {
            window.select();
            carry.raised = true;
        }
        if let Some(before) = before
            && !carry
                .opened
                .iter()
                .any(|(opened, _)| *opened == window.id())
        {
            carry.opened.push((window.id(), before));
        }
        carry.dwell = None;
        window.bar().dwell(None);
        let at = carry.at;
        self.read(app, at);
    }

    /// The gesture ends: let go at `at` (a screen point), or `None` for Esc. The drop is the
    /// applier's; what does nothing flies back.
    ///
    /// Over a window's strip, over a chip's middle the pane joins that tab, between chips it
    /// becomes a tab (a tab's only pane moves its tab) — the selection stays where it is, so a
    /// tab a wait opened is left for the one it was. Over a window's panes the verdict of the
    /// tab on screen is the drop: a move in the pane's own tab, a join in another, which
    /// stays on screen. Over no window of ours the pane becomes a window there. In
    /// another window the tab comes up and the window with it.
    fn finish(
        &mut self,
        app: &AppDelegate,
        window: &TerminalWindow,
        tab: &crate::tab::TerminalTab,
        at: Option<NSPoint>,
    ) {
        let Some(mut carry) = self.carry.take() else {
            // A press that never travelled: nothing was carried.
            return;
        };
        let still = app.reduce_motion();
        let card = carry.card.take();
        // The window under the pointer, where in it, and what letting go there means.
        let here = at.and_then(|at| {
            let target = app.window_at(at)?;
            let point = target.ns_window().convertPointFromScreen(at);
            let lands = lands_at(target.bar().pane_target(point), self.tab);
            Some((target, point, lands))
        });
        // The strips: a gap a landing between chips fills is closed by the landing.
        let settled = match &here {
            Some((target, _, Lands::NewTab(_))) => {
                target.bar().settle_pane_target();
                Some(target.id())
            }
            _ => None,
        };
        if let Some(strip) = carry.strip.take().and_then(|id| app.window(id)) {
            if Some(strip.id()) != settled {
                strip.bar().show_pane_target(None);
            }
            strip.bar().dwell(None);
        }
        carry.take_down();
        if card.is_some() {
            NSCursor::pop_class();
        }
        let (mut done, mut stays, mut crossed) = (false, None, false);
        match (at, &here) {
            // Esc.
            (None, _) => {}
            // Over no window of ours: a window of its own, there.
            (Some(at), None) => {
                done = app.pane_to_window(window, self.pane, Some(at));
            }
            (Some(_), Some((target, point, lands))) if target.id() == window.id() => {
                match *lands {
                    Lands::Tab(chip) => {
                        done = window.pane_to_tab(self.pane, chip, Direction::Right);
                        if done && app.tab(self.tab).is_none() {
                            // A tab's only pane was the tab: it joined as a block and the
                            // tab is gone, so the screen goes to the tab it joined.
                            window.select_tab(chip);
                        }
                    }
                    Lands::NewTab(gap) => {
                        done = window.pane_to_new_tab(self.pane, gap);
                    }
                    Lands::Back => {}
                    Lands::Panes => {
                        if let Some(shown) = window.try_selected_tab() {
                            if shown.id() == self.tab {
                                done = match act_of(&shown.container().verdict(self.pane, *point)) {
                                    Act::Move(tree) => window.move_pane(self.tab, self.pane, tree),
                                    Act::Swap(target) => {
                                        window.swap_panes(self.tab, self.pane, target)
                                    }
                                    Act::Back => false,
                                };
                            } else if let Some(moving) = tab.container().pane(self.pane)
                                && let Act::Move(tree) = act_of(&shown.container().verdict_of(
                                    &Tree::Leaf(self.pane),
                                    &[moving],
                                    *point,
                                ))
                            {
                                done = window.pane_to_tab_at(self.pane, shown.id(), tree);
                                stays = done.then(|| window.id());
                            }
                        }
                    }
                }
            }
            // Over another window of ours: it takes the pane.
            (Some(_), Some((target, point, lands))) => {
                match *lands {
                    Lands::Tab(chip) => {
                        done = app.pane_to_other_tab(
                            window,
                            self.pane,
                            target,
                            chip,
                            Joins::Beside(Direction::Right),
                        );
                    }
                    Lands::NewTab(gap) => {
                        done = app.pane_to_other_new_tab(window, self.pane, target, gap);
                    }
                    Lands::Back => {}
                    Lands::Panes => {
                        if let Some(shown) = target.try_selected_tab()
                            && let Some(moving) = tab.container().pane(self.pane)
                            && let Act::Move(tree) = act_of(&shown.container().verdict_of(
                                &Tree::Leaf(self.pane),
                                &[moving],
                                *point,
                            ))
                        {
                            done = app.pane_to_other_tab(
                                window,
                                self.pane,
                                target,
                                shown.id(),
                                Joins::Planned(tree),
                            );
                            stays = done.then(|| target.id());
                        }
                    }
                }
                crossed = done;
            }
        }
        carry.put_back(app, self.window, stays, crossed);
        if !done {
            // Room a strip held open for a tab that did not come (the window the pane is
            // from, and the one it was let go in).
            window.bar().lay_out();
            if let Some((target, ..)) = &here {
                target.bar().lay_out();
            }
        }
        if let Some(card) = card {
            if done {
                fade_out(&card, still);
            } else {
                fly_back(&card, tab, self.pane, still);
            }
        }
    }

    /// The pane or the window is gone: the carry is taken apart without a drop.
    fn abandon(&mut self) {
        if let Some(mut carry) = self.carry.take() {
            carry.take_down();
            // The strip may still be lit or holding room open.
            if let Some(mtm) = MainThreadMarker::new()
                && let Some(app) = app::delegate(mtm)
            {
                carry.release_strip(&app);
                carry.put_back(&app, self.window, None, false);
            }
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // A carry cut short (the window stopped being key, a second press):
        // what it put on screen goes with it.
        self.abandon();
        if let Some(token) = self.monitor.take() {
            // SAFETY: the token is the one `addLocalMonitor…` gave.
            unsafe { NSEvent::removeMonitor(&token) };
        }
    }
}

/// The selections a carry's waits changed that finishing puts back: every window where a wait
/// opened a tab, to the tab that was open there before — except the window the pane landed
/// in by the regions of the tab on screen (`stays`), which is where the user's eyes are.
fn restore_selections(opened: &[(u64, u64)], stays: Option<u64>) -> Vec<(u64, u64)> {
    opened
        .iter()
        .copied()
        .filter(|(window, _)| Some(*window) != stays)
        .collect()
}

/// The card leaves the screen at once: the panes are sliding to their places.
fn fade_out(card: &Retained<Card>, still: bool) {
    if still {
        card.removeFromSuperview();
        return;
    }
    let fading = card.clone();
    let gone = card.clone();
    arrange::animate(
        FADE_SECS,
        move || fading.animator().setAlphaValue(0.0),
        move || gone.removeFromSuperview(),
    );
}

/// The card goes back to where its pane stands and fades there.
fn fly_back(card: &Retained<Card>, tab: &crate::tab::TerminalTab, pane: u64, still: bool) {
    let target = tab.container().pane(pane).and_then(|pane| {
        // SAFETY: reading the superview; we are on the main thread.
        let parent = unsafe { card.superview() }?;
        Some(parent.convertRect_fromView(pane.frame(), Some(tab.container())))
    });
    let Some(target) = target.filter(|_| !still) else {
        card.removeFromSuperview();
        return;
    };
    let moving = card.clone();
    let gone = card.clone();
    arrange::animate(
        FLY_SECS,
        move || {
            moving.animator().setFrame(target);
            moving.animator().setAlphaValue(0.0);
        },
        move || gone.removeFromSuperview(),
    );
}

/// The mouse monitor of a session: the drag, the release and a key press, for
/// as long as the session lives.
fn watch() -> Option<Retained<AnyObject>> {
    let block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
        // audit: a local monitor runs on the main thread, before `sendEvent:`.
        let mtm = MainThreadMarker::new().expect("a local event monitor runs on the main thread");
        // SAFETY: AppKit gives the monitor a valid event.
        let taken =
            app::delegate(mtm).is_some_and(|app| app.pane_drag_event(unsafe { event.as_ref() }));
        if taken {
            std::ptr::null_mut()
        } else {
            event.as_ptr()
        }
    });
    // SAFETY: the block returns either the valid event it was given or null,
    // which the monitor API takes as "swallowed".
    unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(
            NSEventMask::LeftMouseDragged | NSEventMask::LeftMouseUp | NSEventMask::KeyDown,
            &block,
        )
    }
}

// ─── The drag session ────────────────────────────────────────────────────

/// The pasteboard type a carried pane is written as.
pub(crate) fn pane_type() -> &'static NSString {
    ns_string!("dev.bateri.pane")
}

pub(crate) struct SourceIvars {
    /// The pane being carried.
    pane: u64,
}

define_class!(
    // SAFETY: NSObject subclassing has no requirements; `PaneDragSource` implements no `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriPaneDragSource"]
    #[ivars = SourceIvars]
    pub(crate) struct PaneDragSource;

    unsafe impl NSObjectProtocol for PaneDragSource {}

    unsafe impl NSDraggingSource for PaneDragSource {
        /// A move inside the application, nothing outside it: no other application takes a
        /// pane, and the cursor over one says so.
        #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
        fn source_operation_mask(
            &self,
            _session: &NSDraggingSession,
            context: NSDraggingContext,
        ) -> NSDragOperation {
            if context == NSDraggingContext::WithinApplication {
                NSDragOperation::Move
            } else {
                NSDragOperation::None
            }
        }

        /// The picture moved: the application reads what the pane is over, at the pointer.
        #[unsafe(method(draggingSession:movedToPoint:))]
        fn session_moved(&self, _session: &NSDraggingSession, _at: NSPoint) {
            if let Some(app) = app::delegate(self.mtm()) {
                app.pane_flight_moved();
            }
        }

        /// ⌘ or ⌥ held at the drop would turn a move into a copy or a link; a pane is only
        /// moved (and the keys are the carry's own).
        #[unsafe(method(ignoreModifierKeysForDraggingSession:))]
        fn ignore_modifier_keys(&self, _session: &NSDraggingSession) -> bool {
            true
        }

        /// The drag is over — let go anywhere (no window of ours takes a drop: the pane is
        /// read from the outside, so the operation is `None` whatever happened) or taken back
        /// with Esc, which also reports `None` ([`crate::tab_drag::taken_back`]).
        #[unsafe(method(draggingSession:endedAtPoint:operation:))]
        fn session_ended(
            &self,
            _session: &NSDraggingSession,
            _at: NSPoint,
            _operation: NSDragOperation,
        ) {
            // Where the pointer let go, not where the picture was: the regions that were
            // read from the pointer are the ones the drop acts on.
            let at = NSEvent::mouseLocation();
            if let Some(app) = app::delegate(self.mtm()) {
                app.pane_flight_ended(
                    self.ivars().pane,
                    at,
                    crate::tab_drag::taken_back(self.mtm()),
                );
            }
        }
    }
);

impl PaneDragSource {
    fn new(mtm: MainThreadMarker, pane: u64) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(SourceIvars { pane });
        // SAFETY: NSObject's init takes no arguments and the ivars are set.
        unsafe { msg_send![super(this), init] }
    }
}

/// Starts the drag session a carry becomes at the window's edge: pane `pane` drawn as `image`
/// (the card, `size` points) under the pointer `event` reports, in `view`'s window. The
/// string on the pasteboard is the pane's id, meaningful in this process only (ids count from
/// zero in every process).
fn begin_flight(
    view: &NSView,
    pane: u64,
    event: &NSEvent,
    image: Option<&NSImage>,
    size: NSSize,
) -> Retained<PaneDragSource> {
    let source = PaneDragSource::new(view.mtm(), pane);
    crate::tab_drag::start(
        view,
        pane_type(),
        pane,
        event,
        image,
        size,
        ProtocolObject::from_ref(&*source),
    );
    source
}

impl AppDelegate {
    /// A chip a carried pane waits on has been waited on long enough (`wait` is the
    /// generation of the wait): its tab opens. Found stale if the pointer left it, or the
    /// carry ended, meanwhile.
    pub(crate) fn pane_drag_dwell(&self, wait: u64, chip: u64) {
        let Some(mut session) = self.pane_drag_cell().take() else {
            return;
        };
        session.dwell_fired(self, wait, chip);
        self.pane_drag_cell().replace(Some(session));
    }

    /// Hands one event to the session; `true` if it took it. The session is
    /// out of its cell while it works — a drop changes tabs and windows, and
    /// whatever that touches must find no carry in progress.
    pub(crate) fn pane_drag_event(&self, event: &NSEvent) -> bool {
        let Some(mut session) = self.pane_drag_cell().take() else {
            return false;
        };
        let (taken, step) = session.handle(self, event);
        match step {
            Step::Keep => {
                self.pane_drag_cell().replace(Some(session));
            }
            Step::End => {
                drop(session);
                self.pane_drag_ended();
            }
        }
        taken
    }

    /// The picture of a carry that has left its window moved: the windows' strips and panes
    /// read where the pointer is. Out of its cell while it reads, like an event.
    pub(crate) fn pane_flight_moved(&self) {
        let Some(mut session) = self.pane_drag_cell().take() else {
            return;
        };
        if session.flying() {
            session.read(self, NSEvent::mouseLocation());
        }
        self.pane_drag_cell().replace(Some(session));
    }

    /// The drag session of pane `pane` ended at the screen point `at`, `taken_back` if the
    /// user ended it with Esc. Carried out one main-queue turn later, outside AppKit's
    /// teardown of the session, as for a tab: what the carry came to is read where the pointer
    /// let go ([`Session::finish`]).
    pub(crate) fn pane_flight_ended(&self, pane: u64, at: NSPoint, taken_back: bool) {
        DispatchQueue::main().exec_async(move || {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            let Some(app) = app::delegate(mtm) else {
                return;
            };
            let Some(mut session) = app.pane_drag_cell().take() else {
                return;
            };
            if session.pane == pane {
                match session.place(&app) {
                    Some((window, tab)) => {
                        session.finish(&app, &window, &tab, (!taken_back).then_some(at));
                    }
                    None => session.abandon(),
                }
            }
            // The session is over; its source may go.
            drop(session);
            app.pane_drag_ended();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::split::{Direction, Placement, Tree, Zone};

    fn landing() -> Placement {
        Placement {
            tree: Tree::Leaf(1),
            landing: Rect::new(10.0, 20.0, 300.0, 200.0),
            made_room: false,
        }
    }

    #[test]
    fn a_landing_is_painted_where_the_pane_would_stand_with_its_zones_name() {
        let verdict = Verdict::Lands {
            zone: Zone::Beside {
                target: 2,
                side: Direction::Up,
            },
            placement: landing(),
        };
        assert_eq!(
            preview(&verdict, 1),
            Preview {
                marks: vec![Mark {
                    rect: Rect::new(10.0, 20.0, 300.0, 200.0),
                    kind: Kind::Lands,
                    label: "Above".to_owned(),
                }],
                warning: None,
                inner: Vec::new(),
            }
        );
    }

    #[test]
    fn a_swap_names_itself_and_a_swap_that_does_not_fit_is_refused() {
        let frame = Rect::new(0.0, 0.0, 100.0, 80.0);
        let swap = |fits| Verdict::Swaps {
            target: 2,
            frame,
            fits,
        };
        let shown = preview(&swap(true), 1);
        assert_eq!(shown.marks[0].kind, Kind::Lands);
        assert_eq!(shown.marks[0].label, "Swap");
        let shown = preview(&swap(false), 1);
        assert_eq!(shown.marks[0].kind, Kind::Refused);
        assert_eq!(shown.marks[0].label, "Too small");
        assert_eq!(act_of(&swap(true)), Act::Swap(2));
        assert_eq!(act_of(&swap(false)), Act::Back);
    }

    #[test]
    fn a_refusal_shows_where_it_would_fit() {
        let verdict = Verdict::TooSmall {
            zone: Zone::Beside {
                target: 2,
                side: Direction::Down,
            },
            region: Rect::new(0.0, 100.0, 200.0, 50.0),
            edges: vec![
                (Direction::Left, Rect::new(0.0, 0.0, 80.0, 300.0)),
                (Direction::Right, Rect::new(120.0, 0.0, 80.0, 300.0)),
            ],
        };
        let shown = preview(&verdict, 1);
        let kinds: Vec<Kind> = shown.marks.iter().map(|mark| mark.kind).collect();
        assert_eq!(kinds, [Kind::Refused, Kind::Suggests, Kind::Suggests]);
        assert_eq!(shown.marks[0].label, "Too small");
        assert_eq!(shown.marks[1].label, "Fits here");
        assert_eq!(act_of(&verdict), Act::Back);
    }

    #[test]
    fn a_pane_that_fits_nowhere_gets_the_one_warning() {
        let shown = preview(&Verdict::NoRoom, 1);
        assert!(shown.marks.is_empty());
        assert_eq!(
            shown.warning.as_deref(),
            Some("Not enough room for 1 split")
        );
        assert_eq!(no_room(3), "Not enough room for 3 splits");
        assert_eq!(act_of(&Verdict::NoRoom), Act::Back);
    }

    #[test]
    fn nothing_under_the_pointer_paints_nothing_and_flies_back() {
        assert_eq!(preview(&Verdict::Nothing, 1), Preview::default());
        assert_eq!(act_of(&Verdict::Nothing), Act::Back);
    }

    #[test]
    fn a_landing_is_carried_out_as_its_tree() {
        let verdict = Verdict::Lands {
            zone: Zone::WindowEdge(Direction::Left),
            placement: landing(),
        };
        assert_eq!(act_of(&verdict), Act::Move(Tree::Leaf(1)));
    }

    #[test]
    fn the_card_keeps_the_panes_proportions_within_a_range() {
        let wide = card_size(NSSize::new(800.0, 400.0));
        assert_eq!(wide, NSSize::new(200.0, 124.0));
        // Never narrower than a title, never taller than a quarter of a screen.
        let tiny = card_size(NSSize::new(100.0, 40.0));
        assert_eq!(tiny.width, 150.0);
        assert!(tiny.height >= CARD_HEADER + 54.0);
        let tall = card_size(NSSize::new(400.0, 2000.0));
        assert_eq!(tall.height, CARD_HEADER + 120.0);
    }

    #[test]
    fn text_on_a_region_is_black_on_light_and_white_on_dark() {
        assert_eq!(ink_on(0xa9c1df), 0x000000);
        assert_eq!(ink_on(0x7a9cc6), 0x000000);
        assert_eq!(ink_on(0x203040), 0xffffff);
    }

    #[test]
    fn a_chip_of_the_panes_own_tab_is_not_lit_but_is_waited_on_when_off_screen() {
        // Tab 1 is the pane's, tab 2 is on screen, tab 3 is not.
        let on_screen = |chip| chip == 2;
        let reading = |over| strip_reading(over, 1, on_screen);
        assert_eq!(reading(None), (None, None));
        // Its own chip, off screen (a wait opened another tab): back to it by waiting.
        assert_eq!(
            reading(Some(PaneTarget::Tab(1))),
            (None, Some(1)),
            "unlit, but waited on"
        );
        // Another tab's chip: lit, and waited on unless it is the one on screen.
        assert_eq!(
            reading(Some(PaneTarget::Tab(3))),
            (Some(PaneTarget::Tab(3)), Some(3))
        );
        assert_eq!(
            reading(Some(PaneTarget::Tab(2))),
            (Some(PaneTarget::Tab(2)), None)
        );
        // Between chips there is no tab to open: room, and no wait.
        assert_eq!(
            reading(Some(PaneTarget::Between(2))),
            (Some(PaneTarget::Between(2)), None)
        );
    }

    #[test]
    fn letting_go_on_the_strip_says_where_the_pane_goes() {
        assert_eq!(lands_at(Some(PaneTarget::Tab(7)), 1), Lands::Tab(7));
        assert_eq!(lands_at(Some(PaneTarget::Tab(1)), 1), Lands::Back);
        assert_eq!(lands_at(Some(PaneTarget::Between(0)), 1), Lands::NewTab(0));
        assert_eq!(lands_at(None, 1), Lands::Panes);
    }

    #[test]
    fn a_press_is_a_carry_only_past_the_slop() {
        let origin = NSPoint::new(100.0, 100.0);
        assert!(!past_slop(origin, NSPoint::new(102.0, 101.0)));
        assert!(!past_slop(origin, NSPoint::new(100.0, 100.0)));
        assert!(past_slop(origin, NSPoint::new(104.0, 100.0)));
        assert!(past_slop(origin, NSPoint::new(103.0, 103.0)));
    }

    #[test]
    fn another_windows_chips_are_never_the_panes_own() {
        // Tab ids are one namespace for the whole process: a chip of another window's strip is
        // lit and landed on like any other tab's, and waited on unless it is the one showing
        // there (a window that is not the pane's own has no chip with the pane's tab id).
        let showing = |chip| chip == 20;
        assert_eq!(
            strip_reading(Some(PaneTarget::Tab(20)), 1, showing),
            (Some(PaneTarget::Tab(20)), None)
        );
        assert_eq!(
            strip_reading(Some(PaneTarget::Tab(21)), 1, showing),
            (Some(PaneTarget::Tab(21)), Some(21))
        );
        assert_eq!(lands_at(Some(PaneTarget::Tab(20)), 1), Lands::Tab(20));
        assert_eq!(lands_at(Some(PaneTarget::Between(1)), 1), Lands::NewTab(1));
    }

    #[test]
    fn a_wait_that_opened_a_tab_is_undone_except_where_the_pane_landed() {
        // Window 10 had tab 1 open and a wait opened another; window 20 had tab 5 open.
        let opened = [(10, 1), (20, 5)];
        assert_eq!(restore_selections(&opened, None), vec![(10, 1), (20, 5)]);
        // Landed by the regions of the tab on screen in window 20: that tab stays on screen.
        assert_eq!(restore_selections(&opened, Some(20)), vec![(10, 1)]);
        assert_eq!(restore_selections(&[], Some(20)), Vec::new());
    }
}
