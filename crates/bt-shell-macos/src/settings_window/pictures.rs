//! Pictured choices: a short list of options, each shown as a small picture of what it does with
//! its name under it.
//!
//! The pictures are drawn, not bitmaps, in the colours of the theme that is on screen
//! ([`Palette::of`]), so a light theme shows a light tab bar and the picture of a cursor is in the
//! theme's own cursor colour; the window hands the control a new palette whenever it refreshes.
//! The control keeps nothing of its own either: it shows the option the file chose, and a click
//! sends its action like a pop-up's — the window writes the line and comes back from the file.

use std::cell::{Cell, RefCell};

use bt_core::Theme;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSBezierPath, NSColor, NSControl, NSEvent, NSFont, NSFontAttributeName,
    NSForegroundColorAttributeName, NSStringDrawing, NSView,
};
use objc2_foundation::{
    NSAttributedStringKey, NSDictionary, NSInteger, NSObjectProtocol, NSPoint, NSRect, NSSize,
    NSString,
};

use crate::tab_bar::{Tint, rounded, stroke};

/// The picture's width and height, in points.
const PICTURE_WIDTH: f64 = 70.0;
const PICTURE_HEIGHT: f64 = 46.0;
/// The room round a picture for the ring that marks the chosen one.
const RING: f64 = 4.0;
/// One option's width (the picture, the ring and the room a name needs), the gap between two
/// options and the label's height under the picture.
const CELL_WIDTH: f64 = 88.0;
const CELL_GAP: f64 = 8.0;
const LABEL_HEIGHT: f64 = 16.0;
/// The picture's corner.
const PICTURE_RADIUS: f64 = 6.0;

/// What an option's picture shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Picture {
    TabsTop,
    TabsSide,
    Cards,
    Lines,
    EdgeFade,
    EdgeLine,
    EdgeCut,
    Block,
    Underline,
    Beam,
    Hollow,
    Solid,
    Tail,
    TailRing,
    Breathe,
}

/// The theme's colours the pictures are drawn in, as sRGB `0xRRGGBB`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Palette {
    pub(super) background: u32,
    pub(super) foreground: u32,
    pub(super) cursor: u32,
    pub(super) accent: u32,
    pub(super) dim: u32,
    /// The prompt's mark, and the line that frames the focused pane.
    pub(super) success: u32,
    pub(super) focus: u32,
    /// What the cards stand on.
    pub(super) ground: u32,
    /// The tab bar and the dock.
    pub(super) band: u32,
    pub(super) separator: u32,
    /// A tab chip two steps over the content.
    pub(super) raised: u32,
}

fn pack(rgb: [u8; 3]) -> u32 {
    u32::from(rgb[0]) << 16 | u32::from(rgb[1]) << 8 | u32::from(rgb[2])
}

impl Palette {
    /// The theme's own roles and the surfaces derived from them: the same ones the tab bar, the
    /// dock and the cards are drawn with, so a picture and the thing it pictures agree.
    pub(super) fn of(theme: &Theme) -> Palette {
        let raised = theme.raised_linear(2).to_srgb();
        let byte = |channel: f32| (channel.clamp(0.0, 1.0) * 255.0).round() as u8;
        Palette {
            background: theme.background,
            foreground: theme.foreground,
            cursor: theme.cursor,
            accent: theme.accent,
            dim: theme.dim,
            success: theme.success,
            focus: pack(theme.focus_frame_srgb()),
            ground: pack(theme.ground_srgb()),
            band: pack(theme.band_srgb()),
            separator: pack(theme.separator_srgb()),
            raised: pack([byte(raised[0]), byte(raised[1]), byte(raised[2])]),
        }
    }
}

pub(super) struct ChoiceIvars {
    /// The options: the name under the picture and what the picture shows.
    items: Vec<(&'static str, Picture)>,
    selected: Cell<Option<usize>>,
    palette: Cell<Palette>,
    enabled: Cell<bool>,
    tag: Cell<NSInteger>,
    /// The font the label is drawn in; made once.
    label: RefCell<Option<Retained<NSFont>>>,
}

define_class!(
    /// The pictured choice: its options side by side, one of them ringed.
    // SAFETY: NSControl has no subclassing requirement beyond what `tag` and `isEnabled` say (the
    // base class keeps neither for a control without a cell — both are answered here); Drop is not
    // implemented.
    #[unsafe(super(NSControl))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriSettingsPictureChoice"]
    #[ivars = ChoiceIvars]
    pub(super) struct PictureChoice;

    unsafe impl NSObjectProtocol for PictureChoice {}

    impl PictureChoice {
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
            size_of_choice(self.ivars().items.len())
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
            let Some(index) = cell_at(point.x, self.ivars().items.len()) else {
                return;
            };
            if self.ivars().selected.replace(Some(index)) == Some(index) {
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

impl PictureChoice {
    /// Draws the options again.
    fn redraw(&self) {
        let view: &NSView = self.as_super().as_super();
        view.setNeedsDisplay(true);
    }

    pub(super) fn new(
        mtm: MainThreadMarker,
        items: Vec<(&'static str, Picture)>,
        palette: Palette,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ChoiceIvars {
            items,
            selected: Cell::new(None),
            palette: Cell::new(palette),
            enabled: Cell::new(true),
            tag: Cell::new(0),
            label: RefCell::new(None),
        });
        // SAFETY: `init` is NSControl's designated initializer (zero frame); the size is the
        // intrinsic one.
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setTranslatesAutoresizingMaskIntoConstraints(false);
        this
    }

    /// The chosen option's place in the list, if the file's value is one of the options.
    pub(super) fn selected(&self) -> Option<usize> {
        self.ivars().selected.get()
    }

    pub(super) fn select(&self, index: Option<usize>) {
        if self.ivars().selected.replace(index) != index {
            self.redraw();
        }
    }

    /// Draws the options in another theme's colours.
    pub(super) fn set_palette(&self, palette: Palette) {
        if self.ivars().palette.replace(palette) != palette {
            self.redraw();
        }
    }

    fn paint(&self) {
        let ivars = self.ivars();
        let palette = ivars.palette.get();
        let enabled = ivars.enabled.get();
        let font = ivars
            .label
            .borrow_mut()
            .get_or_insert_with(|| NSFont::systemFontOfSize(11.0))
            .clone();
        for (index, (title, picture)) in ivars.items.iter().enumerate() {
            let left = index as f64 * (CELL_WIDTH + CELL_GAP);
            let origin = NSPoint::new(left + (CELL_WIDTH - PICTURE_WIDTH) / 2.0, RING);
            draw_picture(*picture, origin, &palette, if enabled { 1.0 } else { 0.4 });
            let chosen = ivars.selected.get() == Some(index);
            if chosen {
                let ring = NSRect::new(
                    NSPoint::new(origin.x - 3.0, origin.y - 3.0),
                    NSSize::new(PICTURE_WIDTH + 6.0, PICTURE_HEIGHT + 6.0),
                );
                let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                    ring,
                    PICTURE_RADIUS + 3.0,
                    PICTURE_RADIUS + 3.0,
                );
                path.setLineWidth(2.0);
                let accent = if enabled {
                    NSColor::controlAccentColor()
                } else {
                    NSColor::disabledControlTextColor()
                };
                accent.setStroke();
                path.stroke();
            }
            let color = if !enabled {
                NSColor::disabledControlTextColor()
            } else if chosen {
                NSColor::labelColor()
            } else {
                NSColor::secondaryLabelColor()
            };
            let attributes = attributes(&font, &color);
            let name = NSString::from_str(title);
            // SAFETY: `attributes` holds the two keys `sizeWithAttributes:` and
            // `drawAtPoint:withAttributes:` read, with the value types they document; drawing
            // happens inside `drawRect:`.
            unsafe {
                let width = name.sizeWithAttributes(Some(&attributes)).width;
                name.drawAtPoint_withAttributes(
                    NSPoint::new(
                        left + (CELL_WIDTH - width) / 2.0,
                        RING + PICTURE_HEIGHT + RING + 2.0,
                    ),
                    Some(&attributes),
                );
            }
        }
    }
}

/// The control's size for `count` options.
fn size_of_choice(count: usize) -> NSSize {
    let count = count as f64;
    NSSize::new(
        count * CELL_WIDTH + (count - 1.0).max(0.0) * CELL_GAP,
        RING + PICTURE_HEIGHT + RING + LABEL_HEIGHT,
    )
}

/// The option under `x`, counting from the control's left edge; the gap between two options and
/// the room beyond the last belong to none.
fn cell_at(x: f64, count: usize) -> Option<usize> {
    if x < 0.0 {
        return None;
    }
    let stride = CELL_WIDTH + CELL_GAP;
    let index = (x / stride).floor() as usize;
    (index < count && x - index as f64 * stride <= CELL_WIDTH).then_some(index)
}

pub(super) fn attributes(
    font: &NSFont,
    color: &NSColor,
) -> Retained<NSDictionary<NSAttributedStringKey, AnyObject>> {
    let values: [&AnyObject; 2] = [color.as_ref(), font.as_ref()];
    // SAFETY: AppKit's two attribute keys (extern statics), each with the value type it
    // documents — an `NSColor` and an `NSFont`.
    unsafe {
        NSDictionary::<NSAttributedStringKey, AnyObject>::from_slices(
            &[NSForegroundColorAttributeName, NSFontAttributeName],
            &values,
        )
    }
}

/// Draws `picture` with its top-left corner at `at`, every colour at `strength` of its own.
fn draw_picture(picture: Picture, at: NSPoint, palette: &Palette, strength: f64) {
    let tint = |rgb: u32, alpha: f64| Tint::of(rgb, alpha * strength);
    let rect = |x: f64, y: f64, width: f64, height: f64| {
        NSRect::new(NSPoint::new(at.x + x, at.y + y), NSSize::new(width, height))
    };
    let face = |x: f64, y: f64, width: f64, height: f64, radius: f64, fill: u32| {
        rounded(rect(x, y, width, height), radius, tint(fill, 1.0), None);
    };
    let outline = |x: f64, y: f64, width: f64, height: f64, radius: f64, rgb: u32, line: f64| {
        let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
            rect(x, y, width, height),
            radius,
            radius,
        );
        stroke(&path, line, tint(rgb, 1.0));
    };
    let ground = match picture {
        Picture::TabsTop | Picture::TabsSide | Picture::Cards => palette.ground,
        Picture::Tail | Picture::TailRing | Picture::Breathe => palette.band,
        _ => palette.background,
    };
    rounded(
        rect(0.0, 0.0, PICTURE_WIDTH, PICTURE_HEIGHT),
        PICTURE_RADIUS,
        tint(ground, 1.0),
        Some(tint(palette.separator, 0.8)),
    );
    let text = |text: &str, x: f64, y: f64, size: f64, rgb: u32| {
        let font = NSFont::fontWithName_size(&NSString::from_str("Menlo"), size)
            .unwrap_or_else(|| NSFont::monospacedSystemFontOfSize_weight(size, 0.0));
        let attributes = attributes(&font, &tint(rgb, 1.0).color());
        // SAFETY: as in `paint`.
        unsafe {
            NSString::from_str(text)
                .drawAtPoint_withAttributes(NSPoint::new(at.x + x, at.y + y), Some(&attributes));
        }
    };
    let chip = |x: f64, y: f64, width: f64, height: f64, selected: bool| {
        if selected {
            face(x, y, width, height, height / 2.0, palette.raised);
            outline(x, y, width, height, height / 2.0, palette.dim, 0.6);
        } else {
            outline(x, y, width, height, height / 2.0, palette.separator, 0.6);
        }
    };
    match picture {
        Picture::TabsTop => {
            rounded(
                rect(0.5, 0.5, PICTURE_WIDTH - 1.0, 11.5),
                PICTURE_RADIUS - 0.5,
                tint(palette.band, 1.0),
                None,
            );
            chip(5.0, 3.0, 18.0, 6.0, true);
            chip(26.0, 3.0, 16.0, 6.0, false);
            face(5.0, 16.0, 60.0, 26.0, 4.0, palette.background);
        }
        Picture::TabsSide => {
            rounded(
                rect(0.5, 0.5, 19.5, PICTURE_HEIGHT - 1.0),
                PICTURE_RADIUS - 0.5,
                tint(palette.band, 1.0),
                None,
            );
            chip(3.0, 5.0, 14.0, 5.0, true);
            chip(3.0, 13.0, 14.0, 5.0, false);
            face(24.0, 4.0, 42.0, 38.0, 4.0, palette.background);
        }
        Picture::Cards => {
            face(4.0, 4.0, 29.0, 38.0, 5.0, palette.background);
            face(37.0, 4.0, 29.0, 38.0, 5.0, palette.background);
        }
        Picture::Lines => {
            let path = NSBezierPath::bezierPath();
            path.moveToPoint(NSPoint::new(at.x + 35.0, at.y + 1.0));
            path.lineToPoint(NSPoint::new(at.x + 35.0, at.y + PICTURE_HEIGHT - 1.0));
            stroke(&path, 1.0, tint(palette.separator, 1.0));
        }
        Picture::EdgeFade | Picture::EdgeLine | Picture::EdgeCut => {
            const WIDTHS: [f64; 5] = [44.0, 52.0, 30.0, 48.0, 38.0];
            for (line, width) in WIDTHS.iter().enumerate() {
                let faded = picture == Picture::EdgeFade && line == 0;
                rounded(
                    rect(6.0, 14.0 + 6.0 * line as f64, *width, 3.0),
                    1.5,
                    tint(palette.foreground, if faded { 0.25 } else { 1.0 }),
                    None,
                );
            }
            match picture {
                Picture::EdgeLine => {
                    let path = NSBezierPath::bezierPath();
                    path.moveToPoint(NSPoint::new(at.x + 1.0, at.y + 11.5));
                    path.lineToPoint(NSPoint::new(at.x + PICTURE_WIDTH - 1.0, at.y + 11.5));
                    stroke(&path, 1.0, tint(palette.separator, 1.0));
                }
                Picture::EdgeCut => {
                    rounded(
                        rect(6.0, 9.0, 40.0, 3.0),
                        0.0,
                        tint(palette.foreground, 1.0),
                        None,
                    );
                }
                _ => {}
            }
            rounded(
                rect(0.5, 0.5, PICTURE_WIDTH - 1.0, 10.5),
                PICTURE_RADIUS - 0.5,
                tint(palette.band, 1.0),
                None,
            );
        }
        Picture::Block | Picture::Underline | Picture::Beam | Picture::Hollow | Picture::Solid => {
            text("ls", 14.0, 14.0, 13.0, palette.foreground);
            match picture {
                Picture::Block | Picture::Solid => {
                    face(38.0, 15.0, 11.0, 17.0, 1.6, palette.cursor);
                }
                Picture::Underline => face(38.0, 30.0, 11.0, 2.4, 1.0, palette.cursor),
                Picture::Beam => face(38.0, 15.0, 2.2, 17.0, 1.0, palette.cursor),
                _ => outline(38.5, 15.5, 10.0, 16.0, 1.6, palette.cursor, 1.2),
            }
        }
        Picture::Tail | Picture::TailRing | Picture::Breathe => {
            face(24.0, 14.0, 22.0, 18.0, 6.0, palette.raised);
            text("\u{276f}", 31.0, 16.0, 10.0, palette.dim);
            let ring = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                rect(21.5, 11.5, 27.0, 23.0),
                8.5,
                8.5,
            );
            match picture {
                Picture::Breathe => stroke(&ring, 1.6, tint(palette.accent, 0.55)),
                Picture::TailRing => {
                    stroke(&ring, 1.6, tint(palette.accent, 0.22));
                    stroke_tail(&ring, tint(palette.accent, 1.0));
                }
                _ => stroke_tail(&ring, tint(palette.accent, 1.0)),
            }
        }
    }
}

/// The running line's tail, as it is at one moment: about a third of the ring from its start.
pub(super) fn stroke_tail(ring: &NSBezierPath, color: Tint) {
    // The ring is 2 * (27 + 23) minus the corners' share long, about 90 points.
    let mut pattern: [f64; 2] = [29.0, 61.0];
    // SAFETY: `pattern` outlives the call (AppKit copies it) and `count` is its length.
    unsafe { ring.setLineDash_count_phase(pattern.as_mut_ptr(), 2, 0.0) };
    stroke(ring, 1.6, color);
    // SAFETY: as above; an empty pattern is a solid line again, so the next stroke of this path
    // object is not dashed.
    unsafe { ring.setLineDash_count_phase(std::ptr::null_mut(), 0, 0.0) };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A click lands on the option under it; the gaps and the room beyond the last land on none.
    #[test]
    fn a_click_lands_on_the_option_under_it() {
        assert_eq!(cell_at(-1.0, 3), None);
        assert_eq!(cell_at(0.0, 3), Some(0));
        assert_eq!(cell_at(CELL_WIDTH - 1.0, 3), Some(0));
        assert_eq!(cell_at(CELL_WIDTH + CELL_GAP / 2.0, 3), None);
        assert_eq!(cell_at(CELL_WIDTH + CELL_GAP + 1.0, 3), Some(1));
        let last = 2.0 * (CELL_WIDTH + CELL_GAP);
        assert_eq!(cell_at(last + 5.0, 3), Some(2));
        assert_eq!(cell_at(last + CELL_WIDTH + 20.0, 3), None);
        assert_eq!(cell_at(size_of_choice(3).width + 1.0, 3), None);
    }

    /// The control is as wide as its options and gaps, and as tall as a picture, its ring and a
    /// name.
    #[test]
    fn the_control_is_as_wide_as_its_options() {
        let size = size_of_choice(3);
        assert_eq!(size.width, 3.0 * CELL_WIDTH + 2.0 * CELL_GAP);
        assert_eq!(size.height, 2.0 * RING + PICTURE_HEIGHT + LABEL_HEIGHT);
        assert_eq!(size_of_choice(0).width, 0.0);
    }

    /// The palette is the theme's own roles and surfaces, the ones the bar and the dock draw with.
    #[test]
    fn the_palette_is_the_themes_own() {
        for theme in [Theme::BATERI, Theme::ULTRAVIOLET] {
            let palette = Palette::of(&theme);
            assert_eq!(palette.background, theme.background);
            assert_eq!(palette.cursor, theme.cursor);
            assert_eq!(palette.accent, theme.accent);
            assert_eq!(palette.band, pack(theme.band_srgb()));
            assert_eq!(palette.separator, pack(theme.separator_srgb()));
            assert_ne!(palette.raised, palette.background);
        }
    }
}
