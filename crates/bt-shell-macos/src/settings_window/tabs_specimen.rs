//! The example under Tabs and Panes: a window in miniature — the tab bar on top or in a column,
//! three tabs of which one is selected and one runs a command, and two panes — drawn with the same
//! faces the bar and the cards are drawn with (the theme's band, ground and raised steps, the tab
//! tile and the shaped chip, the focus frame's own colour), so that each choice on the page shows
//! in the thing it changes.
//!
//! It is a still drawing, not the bar: the running line is shown at one moment of its turn (the
//! pictures beside the choice do the same) and no tab is a view with a state of its own.

use std::cell::Cell;

use bt_core::{SplitStyle, TabPosition, TabRunning};
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSBezierPath, NSFont, NSGraphicsContext, NSStringDrawing, NSView};
use objc2_foundation::{NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};

use super::pictures::{Palette, attributes, stroke_tail};
use crate::tab_bar::{Tint, inner_line, rounded, shaped, stroke};

/// The example's height, in points; its width is the page's.
pub(super) const HEIGHT: f64 = 150.0;
/// The window's corner.
const CORNER: f64 = 10.0;
/// The bar's height on top, and the column's width at the side.
const BAR: f64 = 30.0;
const COLUMN: f64 = 150.0;
/// The room between two cards.
const GAP: f64 = 6.0;
/// The card's corner.
const CARD: f64 = 9.0;
/// The program icon's tile, and the chip's corner.
const TILE: (f64, f64) = (22.0, 18.0);
const CHIP_CORNER: f64 = 12.0;
/// The window buttons' colours: close, minimise, zoom.
const LIGHTS: [u32; 3] = [0xff5f57, 0xfebc2e, 0x28c840];

/// What the example shows: the settings of the page and the theme's colours.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Look {
    pub(super) palette: Palette,
    pub(super) position: TabPosition,
    /// Program icons on the tabs; off leaves a dot.
    pub(super) icons: bool,
    /// The icons in their programs' own colours.
    pub(super) brand: bool,
    pub(super) running: TabRunning,
    /// A line under each tab's name: only in the column.
    pub(super) details: bool,
    pub(super) split: SplitStyle,
    pub(super) dim_others: bool,
    pub(super) focus_frame: bool,
    /// Reduce motion is on: a running tab has no line.
    pub(super) reduce_motion: bool,
}

struct Tab {
    title: &'static str,
    glyph: &'static str,
    brand: Option<u32>,
    running: bool,
    selected: bool,
    place: &'static str,
}

const TABS: [Tab; 3] = [
    Tab {
        title: "zsh bateri-term",
        glyph: "\u{276f}",
        brand: None,
        running: false,
        selected: true,
        place: "~/bateri-term \u{b7} main",
    },
    Tab {
        title: "cargo test",
        glyph: "\u{276f}",
        brand: None,
        running: true,
        selected: false,
        place: "~/bateri \u{b7} main",
    },
    Tab {
        title: "claude",
        glyph: "\u{273b}",
        brand: Some(0xd97757),
        running: false,
        selected: false,
        place: "asks: Bash npm test",
    },
];

define_class!(
    /// The tabs example: flipped, drawn from its [`Look`].
    // SAFETY: NSView has no subclassing requirement; Drop is not implemented.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriSettingsTabsSpecimen"]
    #[ivars = Cell<Option<Look>>]
    pub(super) struct TabsView;

    unsafe impl NSObjectProtocol for TabsView {}

    impl TabsView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            if let Some(look) = self.ivars().get() {
                paint(&look, self.bounds());
            }
        }
    }
);

impl TabsView {
    pub(super) fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Cell::new(None));
        // SAFETY: `init` is NSView's designated initializer (zero frame); the size comes from the
        // constraints the page puts on it.
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setTranslatesAutoresizingMaskIntoConstraints(false);
        this
    }

    /// Draws the example as `look` has it; nothing happens when it is the look on screen.
    pub(super) fn set_look(&self, look: Look) {
        if self.ivars().replace(Some(look)) != Some(look) {
            self.setNeedsDisplay(true);
        }
    }
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))
}

fn tint(rgb: u32, alpha: f64) -> Tint {
    Tint::of(rgb, alpha)
}

fn flat(area: NSRect, rgb: u32) {
    rounded(area, 0.0, tint(rgb, 1.0), None);
}

fn mono(size: f64) -> Retained<NSFont> {
    NSFont::fontWithName_size(&NSString::from_str("Menlo"), size)
        .unwrap_or_else(|| NSFont::monospacedSystemFontOfSize_weight(size, 0.0))
}

fn text_width(text: &str, font: &NSFont) -> f64 {
    let attributes = attributes(font, &tint(0, 1.0).color());
    // SAFETY: `attributes` holds the two keys `sizeWithAttributes:` reads, with the value types
    // it documents.
    unsafe {
        NSString::from_str(text)
            .sizeWithAttributes(Some(&attributes))
            .width
    }
}

fn write(text: &str, at: NSPoint, font: &NSFont, rgb: u32) {
    let attributes = attributes(font, &tint(rgb, 1.0).color());
    // SAFETY: as in `text_width`; drawing happens inside `drawRect:`.
    unsafe {
        NSString::from_str(text).drawAtPoint_withAttributes(at, Some(&attributes));
    }
}

/// Draws the whole example into `bounds`.
fn paint(look: &Look, bounds: NSRect) {
    let p = &look.palette;
    let (width, height) = (bounds.size.width, bounds.size.height);
    let side = look.position == TabPosition::Side;
    NSGraphicsContext::saveGraphicsState_class();
    NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(bounds, CORNER, CORNER).addClip();
    flat(bounds, p.ground);
    let (bar, content) = if side {
        (
            rect(0.0, 0.0, COLUMN, height),
            rect(COLUMN, 0.0, width - COLUMN, height),
        )
    } else {
        (
            rect(0.0, 0.0, width, BAR),
            rect(0.0, BAR, width, height - BAR),
        )
    };
    flat(bar, p.band);
    // The window's buttons: on the bar's left, over the tabs in the column.
    let lights = if side { 16.0 } else { BAR / 2.0 };
    for (i, light) in LIGHTS.iter().enumerate() {
        let x = 14.0 + i as f64 * 13.0;
        rounded(
            rect(x - 4.0, lights - 4.0, 8.0, 8.0),
            4.0,
            tint(*light, 1.0),
            None,
        );
    }
    tabs(look, side);
    panes(look, content);
    NSGraphicsContext::restoreGraphicsState_class();
    inner_line(bounds, CORNER, tint(p.separator, 0.8));
}

/// The three tabs: in a row after the buttons, or one under the other in the column.
fn tabs(look: &Look, side: bool) {
    let name = NSFont::systemFontOfSize(11.5);
    let small = NSFont::systemFontOfSize(10.0);
    let lead = if look.icons { TILE.0 } else { 14.0 };
    let mut x = 54.0;
    for (i, tab) in TABS.iter().enumerate() {
        let two_lines = look.details && side;
        let height = if two_lines { 34.0 } else { 24.0 };
        let text = text_width(tab.title, &name);
        let width = if side {
            COLUMN - 10.0
        } else {
            3.0 + lead + 5.0 + text + 8.0
        };
        let (left, top) = if side {
            (5.0, 32.0 + i as f64 * (height + 2.0))
        } else {
            (x, (BAR - height) / 2.0)
        };
        x += width + 3.0;
        chip(look, tab, rect(left, top, width, height), (&name, &small));
    }
}

/// One tab: its face when selected, the program's icon (or a dot), its name and — under it —
/// where it is.
fn chip(look: &Look, tab: &Tab, area: NSRect, fonts: (&NSFont, &NSFont)) {
    let p = &look.palette;
    let (left, top, height) = (area.origin.x, area.origin.y, area.size.height);
    if tab.selected {
        tint(p.raised, 1.0).color().setFill();
        shaped(area, CHIP_CORNER).fill();
    }
    let ink = if tab.selected { p.foreground } else { p.dim };
    let tile = rect(left + 3.0, top + (height - TILE.1) / 2.0, TILE.0, TILE.1);
    let running = tab.running && !look.reduce_motion;
    if look.icons {
        tint(p.raised, 1.0).color().setFill();
        shaped(tile, 6.0).fill();
        let glyph = mono(10.0);
        let color = match tab.brand {
            Some(brand) if look.brand => brand,
            _ => p.dim,
        };
        let glyph_width = text_width(tab.glyph, &glyph);
        write(
            tab.glyph,
            NSPoint::new(
                tile.origin.x + (TILE.0 - glyph_width) / 2.0,
                tile.origin.y + 2.0,
            ),
            &glyph,
            color,
        );
        if running {
            let ring = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                rect(
                    tile.origin.x - 2.5,
                    tile.origin.y - 2.5,
                    TILE.0 + 5.0,
                    TILE.1 + 5.0,
                ),
                8.5,
                8.5,
            );
            match look.running {
                TabRunning::Breathe => stroke(&ring, 1.6, tint(p.accent, 0.55)),
                TabRunning::TailRing => {
                    stroke(&ring, 1.6, tint(p.accent, 0.22));
                    stroke_tail(&ring, tint(p.accent, 1.0));
                }
                TabRunning::Tail => stroke_tail(&ring, tint(p.accent, 1.0)),
            }
        }
    } else if running {
        rounded(
            rect(tile.origin.x + 8.0, tile.origin.y + 6.0, 6.0, 6.0),
            3.0,
            tint(p.accent, 1.0),
            None,
        );
    }
    let lead = if look.icons { TILE.0 } else { 14.0 };
    let at = left + 3.0 + lead + 5.0;
    if look.details {
        write(tab.title, NSPoint::new(at, top + 4.0), fonts.0, ink);
        write(tab.place, NSPoint::new(at, top + 18.0), fonts.1, p.dim);
    } else {
        write(
            tab.title,
            NSPoint::new(at, top + (height - 14.0) / 2.0),
            fonts.0,
            ink,
        );
    }
}

/// The two panes: cards on the ground, or one surface with a line between them; the first is the
/// focused one.
fn panes(look: &Look, content: NSRect) {
    let p = &look.palette;
    let cards = look.split == SplitStyle::Cards;
    let inset = if cards { GAP } else { 0.0 };
    let area = rect(
        content.origin.x + inset,
        content.origin.y + inset,
        content.size.width - 2.0 * inset,
        content.size.height - 2.0 * inset,
    );
    if !cards {
        flat(content, p.background);
    }
    let gap = if cards { GAP } else { 1.0 };
    let half = ((area.size.width - gap) / 2.0).floor();
    let font = mono(11.0);
    for (i, lines) in [
        ("cargo test", "running 24 tests"),
        ("git log", "a272cf9 Offer the font\u{2026}"),
    ]
    .iter()
    .enumerate()
    {
        let focused = i == 0;
        let at = rect(
            area.origin.x + i as f64 * (half + gap),
            area.origin.y,
            half,
            area.size.height,
        );
        let radius = if cards { CARD } else { 0.0 };
        NSGraphicsContext::saveGraphicsState_class();
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(at, radius, radius).addClip();
        if cards {
            rounded(at, radius, tint(p.background, 1.0), None);
        }
        write(
            "\u{276f} ",
            NSPoint::new(at.origin.x + 10.0, at.origin.y + 8.0),
            &font,
            p.success,
        );
        write(
            lines.0,
            NSPoint::new(
                at.origin.x + 10.0 + text_width("\u{276f} ", &font),
                at.origin.y + 8.0,
            ),
            &font,
            p.foreground,
        );
        write(
            lines.1,
            NSPoint::new(at.origin.x + 10.0, at.origin.y + 8.0 + 16.0),
            &font,
            p.dim,
        );
        if !focused && look.dim_others {
            // The veil over what the pane shows: the theme's background at part strength.
            rounded(at, radius, tint(p.background, 0.45), None);
        }
        NSGraphicsContext::restoreGraphicsState_class();
        if cards {
            inner_line(at, radius, tint(p.separator, 0.8));
        }
        if focused && look.focus_frame {
            let outline = if cards {
                rect(
                    at.origin.x - 1.0,
                    at.origin.y - 1.0,
                    half + 2.0,
                    at.size.height + 2.0,
                )
            } else {
                rect(
                    at.origin.x + 1.0,
                    at.origin.y + 1.0,
                    half - 2.0,
                    at.size.height - 2.0,
                )
            };
            let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                outline,
                radius + 1.0,
                radius + 1.0,
            );
            stroke(&path, 2.0, tint(p.focus, 1.0));
        }
    }
    if !cards {
        let line = NSBezierPath::bezierPath();
        let x = area.origin.x + half + 0.5;
        line.moveToPoint(NSPoint::new(x, area.origin.y));
        line.lineToPoint(NSPoint::new(x, area.origin.y + area.size.height));
        stroke(&line, 1.0, tint(p.separator, 1.0));
    }
}
