//! The two drawings of Settings › Agents: the agent's tile beside its switch and, under the
//! explanation, an example of the tab row the switch adds a line to.
//!
//! Both are still drawings in the window's own colours ([`look::palette`]), read when they are
//! drawn, so a theme that changes needs only the view drawn again. The example is a tab row as
//! the list draws it — the tile, the title, where the agent stands, what it last said, where it
//! is — with the row's own measures ([`tabs`]) and English words whatever the system's language,
//! as a picture of a window is; the line the switch adds is marked with a wash of the accent, so
//! the page shows which line is new.

use std::cell::Cell;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSFont, NSFontWeightMedium, NSStringDrawing, NSStringDrawingOptions,
    NSStringNSExtendedStringDrawing, NSView,
};
use objc2_foundation::{
    NSAttributedStringKey, NSDictionary, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
};

use super::look;
use super::pictures::attributes;
use crate::icon;
use crate::program::identity::Kind;
use crate::tab_bar::{Tint, draw_mark, inner_line, rounded};
use crate::tabs;

/// The example's width, in points; its height follows from what it says.
pub(super) const SAMPLE_WIDTH: f64 = 276.0;
/// The room above the title and below the place, and at the row's right.
const PAD_TOP: f64 = 12.0;
const PAD_BOTTOM: f64 = 8.0;
/// The title's line, and the type of the lines under it: the status, the reply and the place.
const TITLE_ROW: f64 = 18.0;
const TITLE_TEXT: f64 = 13.0;
const LINE_TEXT: f64 = 11.0;
/// The wash behind the new line reaches this far past its text at the sides, and its text stands
/// this far in from its top.
const WASH_SIDE: f64 = 4.0;
const WASH_PAD: f64 = 1.0;
/// How much of the accent the wash is.
const WASH_STRENGTH: f64 = 0.15;
/// How much of the title's ink a reply is written in: the tab list's own ([`REPLY_INK`] there).
const REPLY_INK: f64 = 0.74;
/// The tile's corner and its mark's side, as a tab's tile has them.
const TILE_CORNER: f64 = 6.0;
const MARK: f64 = 12.0;

/// What the example is of: the words are fixed, so the picture is the same wherever it is read.
const TITLE: &str = "Add rate limiting to the API";
const STATUS: &str = "Idle \u{b7} 2m 10s";
const REPLY: &str = "Added a per-key limiter and 12 tests. The Redis fallback is still missing.";
const PLACE: &str = "api \u{b7} feat/rate-limit";

/// The tile of `kind` as the tab list wears it in the program's own colour, the mark on it white:
/// the page that names the program shows it whatever the tab icon settings are.
fn paint_tile(kind: Kind, area: NSRect) {
    let palette = look::palette();
    let icon = icon::of(kind);
    let (face, mark) = match icon.brand {
        Some(brand) => (brand, 0x00ff_ffff),
        None => (palette.raised, palette.foreground),
    };
    rounded(area, TILE_CORNER, Tint::of(face, 1.0), None);
    let side = MARK.min(area.size.height);
    let square = NSRect::new(
        NSPoint::new(
            area.origin.x + (area.size.width - side) / 2.0,
            area.origin.y + (area.size.height - side) / 2.0,
        ),
        NSSize::new(side, side),
    );
    draw_mark(kind, square, &Tint::of(mark, 1.0).color());
}

define_class!(
    /// The agent's tile: flipped, drawn in the program's colour.
    // SAFETY: NSView has no subclassing requirement; Drop is not implemented.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriSettingsAgentTile"]
    #[ivars = Cell<Option<Kind>>]
    pub(super) struct Tile;

    unsafe impl NSObjectProtocol for Tile {}

    impl Tile {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            if let Some(kind) = self.ivars().get() {
                paint_tile(kind, self.bounds());
            }
        }

        /// A picture beside a name that says it: nothing in it for VoiceOver to read.
        #[unsafe(method(isAccessibilityElement))]
        fn is_accessibility_element(&self) -> bool {
            false
        }
    }
);

impl Tile {
    /// The tile of `kind`, as large as a tab's.
    pub(super) fn new(mtm: MainThreadMarker, kind: Kind) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Cell::new(Some(kind)));
        // SAFETY: `init` is NSView's designated initializer (zero frame); the size is the
        // constraints' below.
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.widthAnchor()
            .constraintEqualToConstant(tabs::TILE_WIDTH)
            .setActive(true);
        this.heightAnchor()
            .constraintEqualToConstant(tabs::TILE_HEIGHT)
            .setActive(true);
        this
    }
}

/// The font and the attributes of a line of the example, `alpha` of the ink `rgb`.
fn line_attributes(
    size: f64,
    rgb: u32,
    alpha: f64,
) -> Retained<NSDictionary<NSAttributedStringKey, AnyObject>> {
    // SAFETY: AppKit's constant font weight.
    let weight = unsafe { NSFontWeightMedium };
    let font = NSFont::systemFontOfSize_weight(size, weight);
    attributes(&font, &Tint::of(rgb, alpha).color())
}

/// The width of `text` in one line of the example's type.
fn line_width(text: &str) -> f64 {
    let attributes = line_attributes(LINE_TEXT, 0, 1.0);
    // SAFETY: the dictionary holds a font and a colour under their keys.
    unsafe { NSString::from_str(text).sizeWithAttributes(Some(&attributes)) }.width
}

/// Where the text of a line starts, and how wide it may be: after the tile, as a row of the tab
/// list has them.
fn text_box() -> (f64, f64) {
    let at = tabs::ROW_LEAD + tabs::TILE_WIDTH + tabs::ICON_GAP + 2.0;
    (at, SAMPLE_WIDTH - at - tabs::ROW_END)
}

/// How many lines the reply takes: the tab list's rule ([`tabs::ask_lines`]).
fn reply_lines() -> usize {
    tabs::ask_lines(Some(line_width(REPLY)), text_box().1)
}

/// The example's height: the title, the new line with the wash round it, the status, the place.
fn height() -> f64 {
    PAD_TOP
        + TITLE_ROW
        + tabs::ASK_ROW
        + 2.0 * WASH_PAD
        + reply_lines() as f64 * tabs::ASK_ROW
        + tabs::PLACE_ROW
        + PAD_BOTTOM
}

/// `text` in `rect`, in the example's type at `size` and `alpha` of the ink `rgb`: wrapped in the
/// box, the last line that fits ending in an ellipsis where the rest is cut, as the list draws a
/// reply.
fn write(text: &str, rect: NSRect, size: f64, rgb: u32, alpha: f64) {
    let attributes = line_attributes(size, rgb, alpha);
    // SAFETY: as in `line_width`; drawing happens inside `drawRect:`, in a flipped view, so the
    // rectangle's origin is the text's top left.
    unsafe {
        NSString::from_str(text).drawWithRect_options_attributes_context(
            rect,
            NSStringDrawingOptions::UsesLineFragmentOrigin
                | NSStringDrawingOptions::TruncatesLastVisibleLine,
            Some(&attributes),
            None,
        );
    }
}

/// Draws the example into `bounds`.
fn paint_sample(bounds: NSRect, kind: Kind) {
    let palette = look::palette();
    rounded(
        bounds,
        tabs::ROW_RADIUS,
        Tint::of(palette.raised, 1.0),
        None,
    );
    inner_line(bounds, tabs::ROW_RADIUS, Tint::of(palette.separator, 0.8));
    paint_tile(
        kind,
        NSRect::new(
            NSPoint::new(tabs::ROW_LEAD, PAD_TOP),
            NSSize::new(tabs::TILE_WIDTH, tabs::TILE_HEIGHT),
        ),
    );
    let (x, room) = text_box();
    let row = |top: f64, rows: f64| {
        NSRect::new(
            NSPoint::new(x, top),
            NSSize::new(room, rows * tabs::ASK_ROW),
        )
    };
    let mut at = PAD_TOP;
    write(
        TITLE,
        NSRect::new(NSPoint::new(x, at), NSSize::new(room, TITLE_ROW)),
        TITLE_TEXT,
        palette.foreground,
        1.0,
    );
    at += TITLE_ROW;
    // The new line, on a wash of the accent that reaches past its text.
    let lines = reply_lines() as f64;
    rounded(
        NSRect::new(
            NSPoint::new(x - WASH_SIDE, at),
            NSSize::new(
                room + 2.0 * WASH_SIDE,
                lines * tabs::ASK_ROW + 2.0 * WASH_PAD,
            ),
        ),
        WASH_SIDE,
        Tint::of(palette.accent, WASH_STRENGTH),
        None,
    );
    at += WASH_PAD;
    write(
        REPLY,
        row(at, lines),
        LINE_TEXT,
        palette.foreground,
        REPLY_INK,
    );
    at += lines * tabs::ASK_ROW + WASH_PAD;
    write(STATUS, row(at, 1.0), LINE_TEXT, palette.dim, 1.0);
    at += tabs::ASK_ROW;
    write(
        PLACE,
        NSRect::new(NSPoint::new(x, at), NSSize::new(room, tabs::PLACE_ROW)),
        LINE_TEXT,
        palette.dim,
        1.0,
    );
}

define_class!(
    /// The example of a tab row with the agent's last reply in it: flipped, still.
    // SAFETY: NSView has no subclassing requirement; Drop is not implemented.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriSettingsAgentSample"]
    #[ivars = Cell<Option<Kind>>]
    pub(super) struct Sample;

    unsafe impl NSObjectProtocol for Sample {}

    impl Sample {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            if let Some(kind) = self.ivars().get() {
                paint_sample(self.bounds(), kind);
            }
        }

        /// A picture of a row, not a row: nothing in it for VoiceOver to read.
        #[unsafe(method(isAccessibilityElement))]
        fn is_accessibility_element(&self) -> bool {
            false
        }
    }
);

impl Sample {
    /// The example of `kind`'s row.
    pub(super) fn new(mtm: MainThreadMarker, kind: Kind) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Cell::new(Some(kind)));
        // SAFETY: as `Tile::new`.
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.widthAnchor()
            .constraintEqualToConstant(SAMPLE_WIDTH)
            .setActive(true);
        this.heightAnchor()
            .constraintEqualToConstant(height())
            .setActive(true);
        this
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::program::identity::Agent;

    /// The picture is as tall as the lines the tab list's rule gives its reply, and the text
    /// stands in the row.
    #[test]
    fn the_example_is_tall_for_the_lines_its_reply_takes() {
        let lines = reply_lines();
        assert!((1..=tabs::ASK_LINES).contains(&lines), "{lines}");
        let expected = PAD_TOP
            + TITLE_ROW
            + tabs::ASK_ROW
            + 2.0 * WASH_PAD
            + lines as f64 * tabs::ASK_ROW
            + tabs::PLACE_ROW
            + PAD_BOTTOM;
        assert_eq!(height(), expected);
        // The text starts after the tile and leaves the row's end.
        let (at, room) = text_box();
        assert!(at > tabs::ROW_LEAD + tabs::TILE_WIDTH && room > 100.0);
        assert!(at + room < SAMPLE_WIDTH);
        for text in [TITLE, STATUS, REPLY, PLACE] {
            assert!(!text.is_empty());
        }
        // The example is of Claude Code, which has a colour of its own to wear.
        assert!(icon::of(Kind::Agent(Agent::ClaudeCode)).brand.is_some());
    }
}
