//! The themes as a gallery: each a small card in its own colours, its name under it, the one the
//! file chose ringed in the accent.
//!
//! A card is a terminal in miniature drawn from the theme's roles ([`Palette`]): its background,
//! a prompt line in its success and foreground, a line in its accent and dim, one in its error and
//! warning, and six of its ANSI colours. So the gallery shows what each theme looks like before it
//! is chosen, where a list of names said nothing. Like the pictured choice it keeps nothing of its
//! own: the window gives it the themes and the file's choice, and a click sends its action — the
//! window writes the line and comes back from the file.

use std::cell::{Cell, RefCell};

use objc2::rc::Retained;
use objc2::{ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSBezierPath, NSControl, NSEvent, NSFont, NSStringDrawing, NSView};
use objc2_foundation::{NSInteger, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};

use super::look::{self, Ink};
use super::pictures::{Palette, attributes};
use crate::tab_bar::{Tint, rounded};

/// How many cards a row holds.
const COLUMNS: usize = 4;
/// A card's picture, in points.
const CARD_WIDTH: f64 = 104.0;
const CARD_HEIGHT: f64 = 62.0;
const CARD_RADIUS: f64 = 8.0;
/// The room round a card for the ring that marks the chosen one.
const RING: f64 = 4.0;
/// One cell: the card, its ring and its name.
const CELL_WIDTH: f64 = CARD_WIDTH + 2.0 * RING;
const LABEL_HEIGHT: f64 = 16.0;
const CELL_HEIGHT: f64 = RING + CARD_HEIGHT + RING + LABEL_HEIGHT;
/// The room between two cells across and down.
const GAP_X: f64 = 8.0;
const GAP_Y: f64 = 8.0;
/// A section's title line, and the room above a section after another.
const TITLE_HEIGHT: f64 = 20.0;
const SECTION_GAP: f64 = 10.0;

/// One theme of the gallery: the name the file is written with, the title shown, its colours.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Card {
    pub(super) name: String,
    pub(super) title: String,
    pub(super) palette: Palette,
}

/// A run of cards under an optional title.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Section {
    pub(super) title: Option<String>,
    pub(super) cards: Vec<Card>,
}

/// Where the gallery's parts stand: each section's title line, and each card's cell with the
/// section and the place in it.
#[derive(Debug, PartialEq)]
struct Layout {
    titles: Vec<(usize, NSRect)>,
    cells: Vec<(usize, usize, NSRect)>,
    height: f64,
}

/// The gallery's width: its columns and the gaps between them.
const WIDTH: f64 = COLUMNS as f64 * CELL_WIDTH + (COLUMNS as f64 - 1.0) * GAP_X;

/// Lays out sections of the given sizes (whether each has a title, how many cards), top down in a
/// flipped view.
fn layout(sections: &[(bool, usize)]) -> Layout {
    let mut titles = Vec::new();
    let mut cells = Vec::new();
    let mut y = 0.0;
    for (at, &(titled, count)) in sections.iter().enumerate() {
        if count == 0 {
            continue;
        }
        if y > 0.0 {
            y += SECTION_GAP;
        }
        if titled {
            titles.push((
                at,
                NSRect::new(NSPoint::new(0.0, y), NSSize::new(WIDTH, TITLE_HEIGHT)),
            ));
            y += TITLE_HEIGHT;
        }
        for index in 0..count {
            let (row, column) = (index / COLUMNS, index % COLUMNS);
            let origin = NSPoint::new(
                column as f64 * (CELL_WIDTH + GAP_X),
                y + row as f64 * (CELL_HEIGHT + GAP_Y),
            );
            cells.push((
                at,
                index,
                NSRect::new(origin, NSSize::new(CELL_WIDTH, CELL_HEIGHT)),
            ));
        }
        let rows = count.div_ceil(COLUMNS) as f64;
        y += rows * CELL_HEIGHT + (rows - 1.0) * GAP_Y;
    }
    Layout {
        titles,
        cells,
        height: y,
    }
}

/// The card under `point`, as (section, place); the gaps and titles are no card's.
fn card_at(layout: &Layout, point: NSPoint) -> Option<(usize, usize)> {
    layout
        .cells
        .iter()
        .find(|(_, _, cell)| crate::tab_bar::contains(*cell, point))
        .map(|&(section, index, _)| (section, index))
}

pub(super) struct GalleryIvars {
    sections: RefCell<Vec<Section>>,
    /// The name of the theme the file chose (or the one just clicked, until the file says).
    selected: RefCell<Option<String>>,
    enabled: Cell<bool>,
    tag: Cell<NSInteger>,
}

define_class!(
    /// The themes as cards in their own colours.
    // SAFETY: NSControl has no subclassing requirement beyond what `tag` and `isEnabled` say (the
    // base class keeps neither for a control without a cell — both are answered here); Drop is not
    // implemented.
    #[unsafe(super(NSControl))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriSettingsThemeGallery"]
    #[ivars = GalleryIvars]
    pub(super) struct ThemeGallery;

    unsafe impl NSObjectProtocol for ThemeGallery {}

    impl ThemeGallery {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(tag))]
        fn tag(&self) -> NSInteger {
            self.ivars().tag.get()
        }

        #[unsafe(method(setTag:))]
        fn set_tag(&self, tag: NSInteger) {
            self.ivars().tag.set(tag);
        }

        #[unsafe(method(isEnabled))]
        fn is_enabled(&self) -> bool {
            self.ivars().enabled.get()
        }

        #[unsafe(method(setEnabled:))]
        fn set_enabled(&self, enabled: bool) {
            if self.ivars().enabled.replace(enabled) != enabled {
                self.redraw();
            }
        }

        #[unsafe(method(intrinsicContentSize))]
        fn intrinsic_content_size(&self) -> NSSize {
            NSSize::new(WIDTH, self.layout().height)
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            if !self.ivars().enabled.get() {
                return;
            }
            let point = self.convertPoint_fromView(event.locationInWindow(), None);
            let Some((section, index)) = card_at(&self.layout(), point) else {
                return;
            };
            let name = self
                .ivars()
                .sections
                .borrow()
                .get(section)
                .and_then(|section| section.cards.get(index))
                .map(|card| card.name.clone());
            let Some(name) = name else {
                return;
            };
            if self.ivars().selected.replace(Some(name.clone())) == Some(name) {
                return;
            }
            self.redraw();
            // SAFETY: the action and target are the ones the window gave this control; the
            // target is a weak reference to an object that lives for the whole process.
            unsafe {
                let _ = self.sendAction_to(self.action(), self.target().as_deref());
            }
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            self.paint();
        }
    }
);

impl ThemeGallery {
    pub(super) fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(GalleryIvars {
            sections: RefCell::new(Vec::new()),
            selected: RefCell::new(None),
            enabled: Cell::new(true),
            tag: Cell::new(0),
        });
        // SAFETY: `init` is NSControl's designated initializer (zero frame); the size is the
        // intrinsic one.
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setTranslatesAutoresizingMaskIntoConstraints(false);
        this
    }

    /// Shows `sections`, with the theme named `selected` ringed.
    pub(super) fn show(&self, sections: Vec<Section>, selected: &str) {
        let ivars = self.ivars();
        let selected = Some(selected.to_owned());
        let same = *ivars.sections.borrow() == sections;
        if !same {
            ivars.sections.replace(sections);
            let view: &NSView = self.as_super().as_super();
            view.invalidateIntrinsicContentSize();
        }
        if ivars.selected.replace(selected.clone()) != selected || !same {
            self.redraw();
        }
    }

    /// The theme ringed: the file's, or the one just clicked.
    pub(super) fn chosen(&self) -> Option<String> {
        self.ivars().selected.borrow().clone()
    }

    fn layout(&self) -> Layout {
        let sizes: Vec<(bool, usize)> = self
            .ivars()
            .sections
            .borrow()
            .iter()
            .map(|section| (section.title.is_some(), section.cards.len()))
            .collect();
        layout(&sizes)
    }

    fn redraw(&self) {
        let view: &NSView = self.as_super().as_super();
        view.setNeedsDisplay(true);
    }

    fn paint(&self) {
        let ivars = self.ivars();
        let layout = self.layout();
        let sections = ivars.sections.borrow();
        let selected = ivars.selected.borrow();
        let enabled = ivars.enabled.get();
        let strength = if enabled { 1.0 } else { 0.4 };
        let font = NSFont::systemFontOfSize(11.0);
        let heading = NSFont::boldSystemFontOfSize(11.0);
        let window = look::palette();
        for (at, rect) in &layout.titles {
            let Some(title) = sections
                .get(*at)
                .and_then(|section| section.title.as_deref())
            else {
                continue;
            };
            let attributes = attributes(&heading, &look::faded(Ink::Dim, strength));
            // SAFETY: `attributes` holds the two keys `drawAtPoint:withAttributes:` reads, with
            // the value types they document; drawing happens inside `drawRect:`.
            unsafe {
                NSString::from_str(title).drawAtPoint_withAttributes(
                    NSPoint::new(rect.origin.x + RING, rect.origin.y + 2.0),
                    Some(&attributes),
                );
            }
        }
        for &(at, index, cell) in &layout.cells {
            let Some(card) = sections
                .get(at)
                .and_then(|section| section.cards.get(index))
            else {
                continue;
            };
            let frame = NSRect::new(
                NSPoint::new(cell.origin.x + RING, cell.origin.y + RING),
                NSSize::new(CARD_WIDTH, CARD_HEIGHT),
            );
            draw_card(frame, &card.palette, &window, strength);
            let chosen = selected.as_deref() == Some(card.name.as_str());
            if chosen {
                let ring = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                    NSRect::new(
                        NSPoint::new(frame.origin.x - 3.0, frame.origin.y - 3.0),
                        NSSize::new(CARD_WIDTH + 6.0, CARD_HEIGHT + 6.0),
                    ),
                    CARD_RADIUS + 3.0,
                    CARD_RADIUS + 3.0,
                );
                ring.setLineWidth(2.0);
                look::faded(Ink::Accent, strength).setStroke();
                ring.stroke();
            }
            let color = if chosen {
                look::faded(Ink::Text, strength)
            } else {
                look::faded(Ink::Dim, strength)
            };
            let attributes = attributes(&font, &color);
            let name = NSString::from_str(&card.title);
            // SAFETY: as above.
            unsafe {
                let width = name
                    .sizeWithAttributes(Some(&attributes))
                    .width
                    .min(CELL_WIDTH);
                name.drawAtPoint_withAttributes(
                    NSPoint::new(
                        cell.origin.x + (CELL_WIDTH - width) / 2.0,
                        cell.origin.y + RING + CARD_HEIGHT + RING + 1.0,
                    ),
                    Some(&attributes),
                );
            }
        }
    }
}

/// A theme's card in `frame`: its background framed by the window's line, three lines of a
/// terminal in its roles and six of its ANSI colours.
fn draw_card(frame: NSRect, theme: &Palette, window: &Palette, strength: f64) {
    rounded(
        frame,
        CARD_RADIUS,
        Tint::of(theme.background, strength),
        Some(Tint::of(window.separator, strength)),
    );
    let (left, top) = (frame.origin.x + 10.0, frame.origin.y + 11.0);
    let bar = |x: f64, line: f64, width: f64, rgb: u32| {
        rounded(
            NSRect::new(
                NSPoint::new(left + x, top + line * 9.0),
                NSSize::new(width, 4.0),
            ),
            2.0,
            Tint::of(rgb, strength),
            None,
        );
    };
    // The prompt and a command, a line of output, a line of trouble.
    bar(0.0, 0.0, 6.0, theme.success);
    bar(10.0, 0.0, 44.0, theme.foreground);
    bar(0.0, 1.0, 30.0, theme.accent);
    bar(34.0, 1.0, 26.0, theme.dim);
    bar(0.0, 2.0, 18.0, theme.error);
    bar(22.0, 2.0, 30.0, theme.warning);
    for (i, rgb) in theme.ansi.iter().enumerate() {
        let x = left + i as f64 * 11.0;
        rounded(
            NSRect::new(
                NSPoint::new(x, frame.origin.y + CARD_HEIGHT - 16.0),
                NSSize::new(7.0, 7.0),
            ),
            3.5,
            Tint::of(*rgb, strength),
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cards run four to a row under their section's title; a click lands on the card under it,
    /// and a gap or a title lands on none.
    #[test]
    fn a_click_lands_on_the_card_under_it() {
        let laid = layout(&[(true, 5), (false, 0), (true, 2)]);
        assert_eq!(laid.titles.len(), 2, "an empty section has no title");
        assert_eq!(laid.cells.len(), 7);
        let (_, _, first) = laid.cells[0];
        let inside = |rect: NSRect| NSPoint::new(rect.origin.x + 5.0, rect.origin.y + 5.0);
        assert_eq!(card_at(&laid, inside(first)), Some((0, 0)));
        // The fifth card starts the second row, under the first.
        let (_, _, fifth) = laid.cells[4];
        assert_eq!(fifth.origin.x, first.origin.x);
        assert!(fifth.origin.y > first.origin.y + CELL_HEIGHT);
        assert_eq!(card_at(&laid, inside(fifth)), Some((0, 4)));
        // The last section's cards are its own.
        let (_, _, last) = laid.cells[6];
        assert_eq!(card_at(&laid, inside(last)), Some((2, 1)));
        // A gap between two cells, and a title line, are no card's.
        let gap = NSPoint::new(CELL_WIDTH + GAP_X / 2.0, first.origin.y + 5.0);
        assert_eq!(card_at(&laid, gap), None);
        assert_eq!(card_at(&laid, NSPoint::new(5.0, 5.0)), None);
        // As tall as its rows and titles.
        let rows = 2.0 + 1.0;
        let expected = 2.0 * TITLE_HEIGHT + rows * CELL_HEIGHT + (2.0 - 1.0) * GAP_Y + SECTION_GAP;
        assert!((laid.height - expected).abs() < 1e-9);
    }
}
