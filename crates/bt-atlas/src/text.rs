//! Proportional text for the UI kit: shaping a line, measuring it and drawing
//! one glyph at a time into a texture the renderer owns.
//!
//! The terminal's atlas (`crate::Atlas`) is a grid of equal slots and one
//! character per cell; a UI line is the opposite — glyphs of different
//! widths at fractional positions, in several sizes and weights — so it gets
//! an atlas of its own: [`TextAtlas`] shapes through the font system
//! (`FontSystem::shape_line`), draws a glyph's coverage into a **tight**
//! bitmap on first use and places it with a shelf packer ([`ShelfPacker`]).
//! The texture itself and its upload are `bt-gpu`'s, exactly as for the cell
//! atlas: what leaves here is a rectangle in the texture and the bytes to put
//! in it ([`TextUpload`]).
//!
//! Everything is in **pixels** (point size × display scale), like the rest of
//! this crate. A glyph is rasterised at one of [`SUBPIXELS`] horizontal
//! offsets, so a line's spacing follows the font's fractional advances
//! without blurring the glyphs.
//!
//! A backend without proportional text (`FontSystem::PROPORTIONAL` is false)
//! answers `None` from [`TextAtlas::open`]; callers fall back to something
//! plain. Nothing here is a fake.

use std::collections::HashMap;

use crate::system::{Backend, Font, FontSystem};

/// How many horizontal sub-pixel positions a glyph is rasterised at. A power
/// of two keeps [`subpixel`] exact; four is what the eye can tell apart at
/// text sizes.
pub const SUBPIXELS: u8 = 4;

/// The family a UI line is set in.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Family {
    /// The system's interface font (San Francisco on macOS).
    System,
    /// The terminal's own family: the named one, or the default chain's when
    /// `None` — so the kit's text can be set in the very font the grid uses.
    Terminal(Option<String>),
}

/// A font request: family, size in pixels, weight on the CSS scale
/// (100–900, 400 regular, 700 bold) and whether it is the italic.
#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    pub family: Family,
    pub size_px: f64,
    pub weight: u16,
    pub italic: bool,
}

/// An opened font of the [`TextAtlas`] that opened it. Numbered in the order
/// the registry met them; **not** meaningful across atlases.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FontId(u32);

/// One glyph of a shaped line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlacedGlyph {
    /// The run's own font: a line can mix fonts where the cascade stepped in.
    pub font: FontId,
    /// The font's glyph number.
    pub glyph: u32,
    /// The pen position from the line's start, pixels, fractional.
    pub x: f64,
}

/// A shaped line and its measurements.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapedText {
    pub glyphs: Vec<PlacedGlyph>,
    /// The typographic width: where the pen ends.
    pub width: f64,
    /// Baseline distance from the line's top, and from the baseline to the
    /// bottom, pixels. The cascade's fonts widen them.
    pub ascent: f64,
    pub descent: f64,
    /// The font's recommended extra line gap.
    pub leading: f64,
}

impl ShapedText {
    /// The line box's height: ascent + descent + leading.
    pub fn height(&self) -> f64 {
        self.ascent + self.descent + self.leading
    }
}

/// What a backend returns for a line, before the registry numbers its fonts.
#[cfg_attr(
    not(target_os = "macos"),
    allow(dead_code, reason = "only the CoreText backend makes one")
)]
pub(crate) struct ShapedLine<F> {
    pub runs: Vec<ShapedRun<F>>,
    pub width: f64,
    pub ascent: f64,
    pub descent: f64,
    pub leading: f64,
}

/// One run of a [`ShapedLine`]: glyph numbers and pen positions in one font.
#[cfg_attr(
    not(target_os = "macos"),
    allow(dead_code, reason = "only the CoreText backend makes one")
)]
pub(crate) struct ShapedRun<F> {
    pub font: F,
    pub glyphs: Vec<(u32, f64)>,
}

/// A glyph's coverage, drawn tight around its ink (with a one-pixel margin
/// so a linear sampler never reads a neighbour).
#[cfg_attr(
    not(target_os = "macos"),
    allow(dead_code, reason = "only the CoreText backend makes one")
)]
pub(crate) struct TextBitmap {
    pub width: u16,
    pub height: u16,
    /// Where the bitmap's left edge sits relative to the glyph origin
    /// (pixels, to the right positive) and where its top edge sits above the
    /// baseline.
    pub left: i16,
    pub top: i16,
    /// `width * height` coverage bytes, top row first.
    pub data: Vec<u8>,
}

/// Where a glyph's bitmap sits in the texture, and how it hangs off the pen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlyphEntry {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
    /// The bitmap's left edge relative to the glyph origin, pixels.
    pub left: i16,
    /// The bitmap's top edge above the baseline, pixels.
    pub top: i16,
}

/// Bytes to write into the texture at `(x, y)`: `width * height` of `R8`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextUpload {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
    pub data: Vec<u8>,
}

/// Splits a pen position into the whole pixel the bitmap is placed at and the
/// sub-pixel bucket the glyph is rasterised at. The bucket rounds to nearest,
/// so a pen at 0.9 lands on the next pixel's bucket 0 rather than on bucket 3
/// of this one.
pub fn subpixel(pen: f64) -> (i32, u8) {
    let whole = pen.floor();
    let bucket = ((pen - whole) * f64::from(SUBPIXELS)).round() as i32;
    let whole = whole as i32;
    if bucket >= i32::from(SUBPIXELS) {
        (whole + 1, 0)
    } else {
        (whole, bucket as u8)
    }
}

/// A shelf packer for variable-size rectangles: rows ("shelves") as tall as
/// the first rectangle put on them, filled left to right. No removal — the
/// whole atlas is cleared at once when it fills, because a plan holds slot
/// positions until it is built again.
///
/// Every rectangle gets a one-pixel gutter right and below, so two glyphs
/// never share an edge texel.
#[derive(Debug)]
pub(crate) struct ShelfPacker {
    width: u16,
    height: u16,
    shelves: Vec<Shelf>,
    /// The first row below the last shelf.
    floor: u16,
}

#[derive(Debug)]
struct Shelf {
    y: u16,
    height: u16,
    /// The next free column.
    x: u16,
}

impl ShelfPacker {
    pub(crate) fn new(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            shelves: Vec::new(),
            floor: 0,
        }
    }

    pub(crate) fn clear(&mut self) {
        self.shelves.clear();
        self.floor = 0;
    }

    /// Reserves a `width × height` rectangle; its top-left corner.
    pub(crate) fn alloc(&mut self, width: u16, height: u16) -> Option<(u16, u16)> {
        let need_w = width.checked_add(1)?;
        let need_h = height.checked_add(1)?;
        if need_w > self.width || need_h > self.height {
            return None;
        }
        // The tightest existing shelf that is tall enough and has the room;
        // one more than twice the need is a waste, so such a shelf is passed
        // over and a new one started.
        let fit = self
            .shelves
            .iter_mut()
            .filter(|shelf| {
                shelf.height >= need_h
                    && u32::from(shelf.height) <= u32::from(need_h) * 2
                    && u32::from(shelf.x) + u32::from(need_w) <= u32::from(self.width)
            })
            .min_by_key(|shelf| shelf.height);
        if let Some(shelf) = fit {
            let at = (shelf.x, shelf.y);
            shelf.x += need_w;
            return Some(at);
        }
        if u32::from(self.floor) + u32::from(need_h) > u32::from(self.height) {
            return None;
        }
        let y = self.floor;
        self.shelves.push(Shelf {
            y,
            height: need_h,
            x: need_w,
        });
        self.floor += need_h;
        Some((0, y))
    }
}

/// The identity of a glyph bitmap: font, glyph and sub-pixel bucket.
type GlyphKey = (FontId, u32, u8);

/// The style registry's key. `f64` has no `Hash`; the bits do.
type StyleKey = (Family, u64, u16, bool);

/// The UI text atlas: the font registry, the glyph cache and the packer,
/// without a texture. See the module documentation.
pub struct TextAtlas {
    fonts: Vec<Font>,
    opened: HashMap<StyleKey, Option<FontId>>,
    packer: ShelfPacker,
    entries: HashMap<GlyphKey, Option<GlyphEntry>>,
    uploads: Vec<TextUpload>,
    /// The packer filled up and was cleared since the last
    /// [`TextAtlas::take_flushed`]: every entry handed out before is void.
    flushed: bool,
    edge: u16,
}

impl TextAtlas {
    /// An atlas whose texture is `edge × edge` pixels.
    pub fn new(edge: u16) -> Self {
        Self {
            fonts: Vec::new(),
            opened: HashMap::new(),
            packer: ShelfPacker::new(edge, edge),
            entries: HashMap::new(),
            uploads: Vec::new(),
            flushed: false,
            edge,
        }
    }

    /// Does this platform shape and draw proportional text at all?
    pub fn supported() -> bool {
        Backend::PROPORTIONAL
    }

    /// The texture's edge, pixels.
    pub fn edge(&self) -> u16 {
        self.edge
    }

    /// Opens `style`; `None` when the platform has no proportional text.
    /// The same style opens the same [`FontId`].
    pub fn open(&mut self, style: &TextStyle) -> Option<FontId> {
        let key = (
            style.family.clone(),
            style.size_px.to_bits(),
            style.weight,
            style.italic,
        );
        if let Some(known) = self.opened.get(&key) {
            return *known;
        }
        let id = Backend::open_text(&style.family, style.size_px, style.weight, style.italic)
            .map(|font| self.intern(font));
        self.opened.insert(key, id);
        id
    }

    /// Shapes `text` as one line in `font` (a newline is not honoured: a line
    /// is the caller's unit).
    pub fn shape(&mut self, font: FontId, text: &str) -> Option<ShapedText> {
        let base = self.fonts.get(font.0 as usize)?;
        let line = Backend::shape_line(base, text)?;
        let mut glyphs = Vec::new();
        for run in line.runs {
            let id = self.intern(run.font);
            glyphs.extend(run.glyphs.into_iter().map(|(glyph, x)| PlacedGlyph {
                font: id,
                glyph,
                x,
            }));
        }
        Some(ShapedText {
            glyphs,
            width: line.width,
            ascent: line.ascent,
            descent: line.descent,
            leading: line.leading,
        })
    }

    /// Where a caret stands at each of the places `at` (byte offsets of
    /// `text`, in order) of `text` shaped as one line in `font`, pixels from
    /// the line's start. `None` where the backend has no proportional text,
    /// or `font` is not this atlas's.
    pub fn carets(&self, font: FontId, text: &str, at: &[usize]) -> Option<Vec<f64>> {
        let base = self.fonts.get(font.0 as usize)?;
        Backend::caret_offsets(base, text, at)
    }

    /// The glyph's place in the texture at sub-pixel bucket `sub`, drawing and
    /// placing it on first use. `None` for a glyph with no ink (a space) and
    /// for one that could not be drawn or does not fit the texture at all.
    pub fn glyph(&mut self, font: FontId, glyph: u32, sub: u8) -> Option<GlyphEntry> {
        let key = (font, glyph, sub.min(SUBPIXELS - 1));
        if let Some(known) = self.entries.get(&key) {
            return *known;
        }
        let bitmap = self.fonts.get(font.0 as usize).and_then(|face| {
            Backend::draw_text_glyph(face, glyph, f64::from(key.2) / f64::from(SUBPIXELS))
        });
        let entry = bitmap.and_then(|bitmap| self.place(bitmap));
        self.entries.insert(key, entry);
        entry
    }

    /// The bitmaps drawn since the last call, to write into the texture.
    pub fn take_uploads(&mut self) -> Vec<TextUpload> {
        std::mem::take(&mut self.uploads)
    }

    /// Whether the atlas filled and was cleared since the last call. Entries
    /// handed out before the clear point at texels the next glyphs overwrite:
    /// whoever holds a draw list built across the clear builds it again.
    pub fn take_flushed(&mut self) -> bool {
        std::mem::take(&mut self.flushed)
    }

    /// Entries cached (including the glyphs with no ink).
    pub fn cached(&self) -> usize {
        self.entries.len()
    }

    fn intern(&mut self, font: Font) -> FontId {
        let at = self
            .fonts
            .iter()
            .position(|known| Backend::same_font(known, &font))
            .unwrap_or_else(|| {
                self.fonts.push(font);
                self.fonts.len() - 1
            });
        FontId(u32::try_from(at).unwrap_or(u32::MAX))
    }

    fn place(&mut self, bitmap: TextBitmap) -> Option<GlyphEntry> {
        let (x, y) = match self.packer.alloc(bitmap.width, bitmap.height) {
            Some(at) => at,
            None => {
                // Full: start over. Every cached entry and every pending
                // upload belongs to the texture's old contents.
                self.entries.clear();
                self.uploads.clear();
                self.packer.clear();
                self.flushed = true;
                self.packer.alloc(bitmap.width, bitmap.height)?
            }
        };
        let entry = GlyphEntry {
            x,
            y,
            width: bitmap.width,
            height: bitmap.height,
            left: bitmap.left,
            top: bitmap.top,
        };
        self.uploads.push(TextUpload {
            x,
            y,
            width: bitmap.width,
            height: bitmap.height,
            data: bitmap.data,
        });
        Some(entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pen_position_rounds_to_the_nearest_subpixel() {
        assert_eq!(subpixel(10.0), (10, 0));
        assert_eq!(subpixel(10.24), (10, 1));
        assert_eq!(subpixel(10.5), (10, 2));
        assert_eq!(subpixel(10.74), (10, 3));
        // 0.9 is nearer the next pixel than bucket 3, so it carries over.
        assert_eq!(subpixel(10.9), (11, 0));
        assert_eq!(subpixel(-0.1), (0, 0));
        assert_eq!(subpixel(-0.3), (-1, 3));
    }

    #[test]
    fn the_packer_fills_a_shelf_before_opening_the_next() {
        let mut packer = ShelfPacker::new(32, 32);
        assert_eq!(packer.alloc(9, 9), Some((0, 0)));
        // 10 px each with the gutter: three fit a 32 px row.
        assert_eq!(packer.alloc(9, 9), Some((10, 0)));
        assert_eq!(packer.alloc(9, 9), Some((20, 0)));
        // The fourth does not: a second shelf, below the first.
        assert_eq!(packer.alloc(9, 9), Some((0, 10)));
    }

    #[test]
    fn the_packer_reuses_a_shelf_of_about_the_right_height() {
        let mut packer = ShelfPacker::new(64, 64);
        assert_eq!(packer.alloc(5, 19), Some((0, 0)));
        // 12 high needs 13; the 20 px shelf is under twice that, so it hosts
        // it instead of a new shelf being opened.
        assert_eq!(packer.alloc(5, 12), Some((6, 0)));
        // 4 high needs 5: the 20 px shelf is over twice that.
        assert_eq!(packer.alloc(5, 4), Some((0, 20)));
    }

    #[test]
    fn the_packer_refuses_what_cannot_fit_and_recovers_when_cleared() {
        let mut packer = ShelfPacker::new(16, 16);
        assert_eq!(
            packer.alloc(16, 4),
            None,
            "the gutter does not fit the edge"
        );
        assert_eq!(packer.alloc(15, 15), Some((0, 0)));
        assert_eq!(packer.alloc(2, 2), None, "no shelf and no floor left");
        packer.clear();
        assert_eq!(packer.alloc(2, 2), Some((0, 0)));
    }

    #[test]
    fn two_rectangles_never_overlap() {
        let mut packer = ShelfPacker::new(128, 128);
        let mut placed: Vec<(u16, u16, u16, u16)> = Vec::new();
        for i in 0..200u16 {
            let (w, h) = (3 + i % 13, 5 + (i * 7) % 17);
            let Some((x, y)) = packer.alloc(w, h) else {
                break;
            };
            assert!(x + w < 128 && y + h < 128, "inside the texture");
            for &(ox, oy, ow, oh) in &placed {
                let apart = x + w < ox || ox + ow < x || y + h < oy || oy + oh < y;
                assert!(apart, "({x},{y},{w},{h}) overlaps ({ox},{oy},{ow},{oh})");
            }
            placed.push((x, y, w, h));
        }
        assert!(placed.len() > 20, "the texture took a fair number");
    }

    /// The proportional tests need a backend that has proportional text; on
    /// one that does not they say so and stop (the packer's tests above run
    /// everywhere).
    fn proportional() -> bool {
        if !TextAtlas::supported() {
            eprintln!("skipped: this backend has no proportional text");
        }
        TextAtlas::supported()
    }

    fn style(family: Family, size_px: f64, weight: u16) -> TextStyle {
        TextStyle {
            family,
            size_px,
            weight,
            italic: false,
        }
    }

    #[test]
    fn a_backend_without_proportional_text_opens_nothing() {
        let mut atlas = TextAtlas::new(256);
        let opened = atlas.open(&style(Family::System, 16.0, 400));
        assert_eq!(opened.is_some(), TextAtlas::supported());
    }

    #[test]
    fn a_line_is_wider_than_a_shorter_one_and_the_pen_advances() {
        if !proportional() {
            return;
        }
        let mut atlas = TextAtlas::new(256);
        let font = atlas
            .open(&style(Family::System, 32.0, 400))
            .expect("opens");
        let short = atlas.shape(font, "ill").expect("shapes");
        let long = atlas.shape(font, "illWWW").expect("shapes");
        assert!(long.width > short.width);
        assert_eq!(long.glyphs.len(), 6);
        assert!(long.glyphs.windows(2).all(|pair| pair[1].x > pair[0].x));
        // The proportional part: an `i` advances less than a `W`.
        let narrow = long.glyphs[3].x - long.glyphs[2].x;
        let wide = long.glyphs[4].x - long.glyphs[3].x;
        assert!(
            narrow < wide,
            "'l' ({narrow}) advances less than 'W' ({wide})"
        );
        assert!(long.ascent > 0.0 && long.descent > 0.0);
        assert!(long.height() >= long.ascent + long.descent);
    }

    #[test]
    fn the_same_style_is_the_same_font_and_a_heavier_weight_is_wider() {
        if !proportional() {
            return;
        }
        let mut atlas = TextAtlas::new(256);
        let regular = atlas
            .open(&style(Family::System, 24.0, 400))
            .expect("opens");
        let again = atlas
            .open(&style(Family::System, 24.0, 400))
            .expect("opens");
        let bold = atlas
            .open(&style(Family::System, 24.0, 800))
            .expect("opens");
        assert_eq!(regular, again);
        assert_ne!(regular, bold);
        let text = "Hamburgefonstiv";
        let regular = atlas.shape(regular, text).expect("shapes").width;
        let bold = atlas.shape(bold, text).expect("shapes").width;
        assert!(
            bold > regular,
            "bold {bold} is wider than regular {regular}"
        );
    }

    #[test]
    fn a_heavy_weight_of_the_terminal_family_is_a_heavier_face_too() {
        if !proportional() {
            return;
        }
        let mut atlas = TextAtlas::new(256);
        let regular = atlas
            .open(&style(Family::Terminal(None), 20.0, 400))
            .expect("opens");
        for weight in [600, 650, 700] {
            let heavy = atlas
                .open(&style(Family::Terminal(None), 20.0, weight))
                .expect("opens");
            assert_ne!(
                regular, heavy,
                "weight {weight} of the terminal's face is not the regular one"
            );
        }
    }

    #[test]
    fn the_terminal_family_is_one_fixed_advance() {
        if !proportional() {
            return;
        }
        let mut atlas = TextAtlas::new(256);
        let font = atlas
            .open(&style(Family::Terminal(None), 20.0, 400))
            .expect("opens");
        let line = atlas.shape(font, "illWWW").expect("shapes");
        let first = line.glyphs[1].x - line.glyphs[0].x;
        for pair in line.glyphs.windows(2) {
            assert!((pair[1].x - pair[0].x - first).abs() < 0.01);
        }
    }

    #[test]
    fn a_glyph_is_drawn_once_and_a_space_has_no_ink() {
        if !proportional() {
            return;
        }
        let mut atlas = TextAtlas::new(256);
        let font = atlas
            .open(&style(Family::System, 24.0, 400))
            .expect("opens");
        let line = atlas.shape(font, "H H").expect("shapes");
        let h = line.glyphs[0];
        let space = line.glyphs[1];
        let entry = atlas.glyph(h.font, h.glyph, 0).expect("H has ink");
        assert!(entry.width > 4 && entry.height > 8);
        assert!(entry.top > 0, "an H stands above the baseline");
        assert_eq!(atlas.glyph(space.font, space.glyph, 0), None);
        let uploads = atlas.take_uploads();
        assert_eq!(uploads.len(), 1, "the second ask is the cache's");
        assert_eq!(atlas.glyph(h.font, h.glyph, 0), Some(entry));
        assert!(atlas.take_uploads().is_empty());
        let upload = &uploads[0];
        assert_eq!(
            upload.data.len(),
            usize::from(upload.width) * usize::from(upload.height)
        );
        assert!(upload.data.iter().any(|&b| b > 200), "an H has solid ink");
        assert!(!atlas.take_flushed());
    }

    #[test]
    fn a_sub_pixel_bucket_moves_the_ink() {
        if !proportional() {
            return;
        }
        let mut atlas = TextAtlas::new(512);
        let font = atlas
            .open(&style(Family::System, 24.0, 400))
            .expect("opens");
        let line = atlas.shape(font, "l").expect("shapes");
        let l = line.glyphs[0];
        let mut centre = |sub| {
            let entry = atlas.glyph(l.font, l.glyph, sub).expect("ink");
            let upload = atlas.take_uploads().pop().expect("uploaded");
            let (mut weight, mut sum) = (0.0f64, 0.0f64);
            for (i, &b) in upload.data.iter().enumerate() {
                let col = (i % usize::from(entry.width)) as f64 + f64::from(entry.left);
                weight += f64::from(b);
                sum += f64::from(b) * col;
            }
            sum / weight
        };
        let at_0 = centre(0);
        let at_2 = centre(2);
        assert!(
            (at_2 - at_0 - 0.5).abs() < 0.15,
            "half a pixel right: {at_0} → {at_2}"
        );
    }

    #[test]
    fn a_full_texture_starts_over_and_says_so() {
        if !proportional() {
            return;
        }
        // 64 px holds a handful of 40 px glyphs; the loop outruns it.
        let mut atlas = TextAtlas::new(64);
        let font = atlas
            .open(&style(Family::System, 40.0, 400))
            .expect("opens");
        let line = atlas.shape(font, "HEMWNBDPRKUAOQGXYZ").expect("shapes");
        for placed in &line.glyphs {
            let _ = atlas.glyph(placed.font, placed.glyph, 0);
        }
        assert!(atlas.take_flushed(), "the texture filled and was cleared");
        assert!(!atlas.take_flushed(), "the flag is taken");
    }
}
