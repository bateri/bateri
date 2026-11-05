//! Shaping a row of single-cell characters by the font's own rules: what a
//! font that masks a secret, or draws an alternate letter, turns characters
//! into before they are drawn.
//!
//! The grid draws one character per cell, by the font's `cmap`. A font with
//! rules (`GSUB`) says more than that: Obel Mono's mask lives in `ccmp`, an
//! alternate `a` behind `cv02`. This module asks the font's shaper what a
//! row of characters becomes and keeps **only the glyphs that differ** from
//! the `cmap`'s, so every other character is drawn exactly as it was and a
//! font without rules (Menlo) never reaches the shaper at all.
//!
//! The mapping from a shaped line back to cells holds where the shaper kept
//! the characters in order: one glyph per character, or a ligature the grid
//! can cut into cells. A font makes a ligature in one of two ways and both
//! are read — one glyph for several characters, or an empty spacer glyph per
//! cell and the picture on the last, its ink reaching back over the spacers
//! (JetBrains Mono's `->`). A shape that cannot be cut — the shaper
//! reordered the characters, its ink leaves the cells it stands for — is not
//! shaped here: its characters are drawn as themselves, which is what the
//! cell grid would do without this module.

use std::collections::HashMap;

use crate::features::has_default_rules;
use crate::raster;
use crate::rules::{Face, InkRect};
use crate::system::{Backend, Font, FontSystem, RunGlyph};
use crate::{Atlas, Shaped};

/// The most cells one shape may cover: a ligature longer than this is read
/// as separate characters.
const MOST_CELLS: usize = 8;

/// How far a shape's ink may stand outside the cells it covers, in the
/// font's own advances: an outline's rounding and a ligature's slightly
/// overshooting tip are not a different shape.
const SLACK: f64 = 0.05;

/// How many rows the cache keeps before it is emptied.
///
/// The cache is a speed-up, not state: the grid is rebuilt every frame and
/// each row is asked again, so a row that is asked twice must not be shaped
/// twice — but a window of changing text would grow it without a bound.
/// Emptying it whole is the atlas's own habit (`Atlas::recycle`): nothing
/// holds an entry across calls.
const CACHE_ROWS: usize = 4096;

/// Is the shaping path open for a font whose regular face is `regular` and
/// the user chose `features`?
///
/// It is when the font has a rule that runs without being asked (`ccmp`,
/// `calt`, `liga`, `clig`, `rlig`) or when the user chose anything at all.
/// Menlo has no `GSUB` table: no call is made for it and what it draws does
/// not move a pixel.
pub(crate) fn opens(regular: &Font, features: &[(String, bool)]) -> bool {
    !features.is_empty()
        || Backend::table(regular, *b"GSUB").is_some_and(|gsub| has_default_rules(&gsub))
}

/// What each character of `text` is drawn as: the glyph the shaper chose
/// where it differs from the character's own, `None` where the character
/// draws as itself.
///
/// `cmap` gives a character's own glyph and `measure` a glyph's advance and
/// ink. Three things are kept besides a plain alternate (one glyph for one
/// character, one cell):
///
/// - one glyph for several characters, when its advance is that many cells
///   and its ink stays inside them;
/// - spacer glyphs followed by a glyph whose ink reaches back over them: the
///   picture is one shape as wide as the group, and each cell draws its slice;
/// - nothing else — a glyph of another font (the cascade's), a procedural
///   character, a character the font lacks, a shape whose ink leaves its
///   cells, a spacer without its picture and any line the shaper reordered
///   are `None`.
fn per_char(
    text: &str,
    run: &[RunGlyph],
    cmap: impl Fn(char) -> Option<u32>,
    measure: impl Fn(u32) -> (f64, InkRect),
) -> Vec<Option<Shaped>> {
    let chars: Vec<char> = text.chars().collect();
    let mut result = vec![None; chars.len()];
    // The character each glyph begins at, from its UTF-16 index.
    let mut starts = Vec::with_capacity(run.len());
    let mut unit = 0;
    for (at, ch) in chars.iter().enumerate() {
        match run.get(starts.len()) {
            Some(glyph) if glyph.index == unit => starts.push(at),
            Some(glyph) if glyph.index < unit => return result,
            _ => {}
        }
        unit += ch.len_utf16();
    }
    if starts.len() != run.len() || starts.first().is_some_and(|&first| first != 0) {
        return result;
    }
    // The spacer glyphs waiting for their picture, by character.
    let mut spacers: Vec<usize> = Vec::new();
    for (k, glyph) in run.iter().enumerate() {
        let from = starts[k];
        let to = starts.get(k + 1).copied().unwrap_or(chars.len());
        let ordinary = glyph.base && chars[from..to].iter().all(|&ch| !raster::is_procedural(ch));
        let Some(own) = cmap(chars[from]).filter(|_| ordinary) else {
            spacers.clear();
            continue;
        };
        if to - from > 1 {
            spacers.clear();
            let (advance, ink) = measure(glyph.glyph);
            let cell = measure(own).0;
            let cells = to - from;
            let sized = (advance - cells as f64 * cell).abs() <= SLACK * cell;
            let inside = ink.x >= -SLACK * cell && ink.x + ink.width <= advance + SLACK * cell;
            if cells <= MOST_CELLS && sized && inside {
                for (slice, slot) in result[from..to].iter_mut().enumerate() {
                    *slot = Some(shape(glyph.glyph, slice, cells, 0));
                }
            }
            continue;
        }
        if own == glyph.glyph {
            spacers.clear();
            continue;
        }
        let (advance, ink) = measure(glyph.glyph);
        if ink.width <= 0.0 || ink.height <= 0.0 {
            spacers.push(from);
            continue;
        }
        if spacers.is_empty() {
            result[from] = Some(Shaped::single(glyph.glyph));
            continue;
        }
        // The picture of a spacer group: its ink must reach the group's first
        // cell, and not pass the group's start or the end of its last cell.
        let cells = spacers.len() + 1;
        let back = (cells - 1) as f64 * advance;
        let reaches = ink.x < -(back - advance);
        let inside =
            ink.x >= -back - SLACK * advance && ink.x + ink.width <= (1.0 + SLACK) * advance;
        if cells <= MOST_CELLS && reaches && inside {
            for (slice, &at) in spacers.iter().chain([&from]).enumerate() {
                result[at] = Some(shape(glyph.glyph, slice, cells, cells - 1));
            }
        }
        spacers.clear();
    }
    result
}

/// A shape's cell, from counts that [`MOST_CELLS`] keeps small.
fn shape(glyph: u32, slice: usize, cells: usize, lead: usize) -> Shaped {
    let small = |n: usize| u8::try_from(n).unwrap_or(u8::MAX);
    Shaped {
        glyph,
        slice: small(slice),
        cells: small(cells),
        lead: small(lead),
    }
}

/// One face's rows: the text → what each of its characters is drawn as.
type Rows = HashMap<Box<str>, Box<[Option<Shaped>]>>;

/// The rows already shaped, per face; the answer of a row is the same until
/// the atlas is rebuilt, and the cache dies with it.
#[derive(Default)]
pub(crate) struct Cache {
    rows: [Rows; 4],
    len: usize,
    /// How many times the shaper was asked: the proof a font without rules
    /// is never asked and a row seen twice is asked once.
    asked: usize,
}

impl Cache {
    fn get(&self, face: Face, text: &str) -> Option<&[Option<Shaped>]> {
        self.rows[face as usize].get(text).map(|row| &**row)
    }

    fn insert(&mut self, face: Face, text: &str, row: &[Option<Shaped>]) {
        if self.len >= CACHE_ROWS {
            self.rows.iter_mut().for_each(Rows::clear);
            self.len = 0;
        }
        self.rows[face as usize].insert(text.into(), row.into());
        self.len += 1;
    }
}

impl Atlas {
    /// Whether the font is shaped by its own rules at all ([`opens`]); when
    /// it is not, [`Atlas::shape_row`] answers every character as itself and
    /// a caller skips building the rows.
    pub fn shapes(&self) -> bool {
        self.faces.shaping()
    }

    /// The glyph each character of `text` is drawn as in `face` — one entry
    /// per character, in order, `None` where it draws as itself — into `out`
    /// (cleared first). A `Some` is drawn with [`Sprite::Glyph`]; a ligature
    /// answers one entry per cell it covers, each its own slice.
    ///
    /// `text` is a row of cells, one character per cell, a blank cell as a
    /// space. The answer is cached per face and text.
    pub fn shape_row(&mut self, face: Face, text: &str, out: &mut Vec<Option<Shaped>>) {
        out.clear();
        if !self.shapes() {
            out.resize(text.chars().count(), None);
            return;
        }
        let face = self.faces.effective(face);
        if let Some(row) = self.shaped.get(face, text) {
            out.extend_from_slice(row);
            return;
        }
        let font = self.faces.get(face);
        self.shaped.asked += 1;
        let row = match Backend::shape_run(font, text) {
            Some(run) => per_char(
                text,
                &run,
                |ch| Backend::glyph(font, ch),
                |glyph| (Backend::advance(font, glyph), Backend::ink(font, glyph)),
            ),
            None => vec![None; text.chars().count()],
        };
        out.extend_from_slice(&row);
        self.shaped.insert(face, text, &row);
    }

    /// How many rows the shaper has been asked to shape: test support, not
    /// API.
    #[cfg(any(test, feature = "fixture"))]
    #[doc(hidden)]
    pub fn shape_calls(&self) -> usize {
        self.shaped.asked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(glyphs: &[(u32, usize, bool)]) -> Vec<RunGlyph> {
        glyphs
            .iter()
            .map(|&(glyph, index, base)| RunGlyph { glyph, index, base })
            .collect()
    }

    /// A font whose glyph for a character is its code point.
    fn own(ch: char) -> Option<u32> {
        Some(u32::from(ch))
    }

    /// The cell of the font in these tests, in units.
    const CELL: f64 = 10.0;

    /// A font of ten-unit cells. A glyph named in `table` — `(glyph,
    /// advance, ink x, ink width)` — measures as given (a width of zero is
    /// a spacer); every other glyph fills its cell.
    fn measure(table: &'static [(u32, f64, f64, f64)]) -> impl Fn(u32) -> (f64, InkRect) {
        move |glyph| {
            let (advance, x, width) = table
                .iter()
                .find(|entry| entry.0 == glyph)
                .map_or((CELL, 1.0, 8.0), |entry| (entry.1, entry.2, entry.3));
            let ink = InkRect {
                x,
                y: 0.0,
                width,
                height: if width > 0.0 { 8.0 } else { 0.0 },
            };
            (advance, ink)
        }
    }

    fn shaped(glyph: u32, slice: u8, cells: u8, lead: u8) -> Option<Shaped> {
        Some(Shaped {
            glyph,
            slice,
            cells,
            lead,
        })
    }

    #[test]
    fn only_the_glyphs_that_differ_from_the_cmap_are_kept() {
        let glyphs = run(&[
            (u32::from('a'), 0, true),
            (900, 1, true),
            (u32::from('c'), 2, true),
        ]);
        assert_eq!(
            per_char("abc", &glyphs, own, measure(&[])),
            [None, Some(Shaped::single(900)), None]
        );
    }

    #[test]
    fn a_line_the_shaper_reordered_is_not_shaped() {
        // The second glyph came from the first character's place.
        let moved = run(&[(900, 0, true), (901, 0, true)]);
        assert_eq!(per_char("ab", &moved, own, measure(&[])), [None, None]);
        // A glyph at an index no character starts at.
        let inside = run(&[(900, 0, true), (901, 1, true)]);
        assert_eq!(per_char("𝐚", &inside, own, measure(&[])), [None]);
        // The first character has no glyph.
        let late = run(&[(900, 1, true)]);
        assert_eq!(per_char("ab", &late, own, measure(&[])), [None, None]);
    }

    #[test]
    fn indices_are_counted_in_utf16_units() {
        // `𝐚` is two UTF-16 units; the character after it stands at 2.
        let glyphs = run(&[(900, 0, true), (901, 2, true)]);
        assert_eq!(
            per_char("𝐚b", &glyphs, own, measure(&[])),
            [Some(Shaped::single(900)), Some(Shaped::single(901))]
        );
    }

    #[test]
    fn another_fonts_glyph_a_missing_character_and_a_procedural_one_stay_themselves() {
        let glyphs = run(&[(900, 0, false), (901, 1, true), (902, 2, true)]);
        let cmap = |ch: char| (ch != 'b').then(|| u32::from(ch));
        assert_eq!(
            per_char("a\u{2588}b", &glyphs, cmap, measure(&[])),
            [None, None, None]
        );
    }

    #[test]
    fn spacers_and_their_picture_are_one_shape_cut_into_cells() {
        // `->` as JetBrains Mono draws it: nothing for `-`, the arrow for
        // `>` with its ink reaching back over the first cell.
        let two = run(&[(800, 0, true), (801, 1, true)]);
        let font = measure(&[(800, CELL, 0.0, 0.0), (801, CELL, -9.0, 19.0)]);
        assert_eq!(
            per_char("->", &two, own, font),
            [shaped(801, 0, 2, 1), shaped(801, 1, 2, 1)]
        );
        // Three cells: two spacers.
        let three = run(&[(800, 0, true), (800, 1, true), (802, 2, true)]);
        let font = measure(&[(800, CELL, 0.0, 0.0), (802, CELL, -19.0, 29.0)]);
        assert_eq!(
            per_char("===", &three, own, font),
            [
                shaped(802, 0, 3, 2),
                shaped(802, 1, 3, 2),
                shaped(802, 2, 3, 2)
            ]
        );
    }

    #[test]
    fn a_picture_that_does_not_fill_its_spacers_cells_is_not_a_ligature() {
        let two = run(&[(800, 0, true), (801, 1, true)]);
        for (x, width, why) in [
            (1.0, 8.0, "it does not reach back over the spacer"),
            (-14.0, 24.0, "it starts before the group"),
            (-9.0, 25.0, "it ends past the last cell"),
        ] {
            let table: &'static [(u32, f64, f64, f64)] =
                Box::leak(Box::new([(800, CELL, 0.0, 0.0), (801, CELL, x, width)]));
            assert_eq!(
                per_char("->", &two, own, measure(table)),
                [None, None],
                "{why}"
            );
        }
        // A spacer left alone at the end, or before an ordinary glyph.
        let font = measure(&[(800, CELL, 0.0, 0.0)]);
        let trailing = run(&[(u32::from('a'), 0, true), (800, 1, true)]);
        assert_eq!(per_char("a-", &trailing, own, font), [None, None]);
        let font = measure(&[(800, CELL, 0.0, 0.0)]);
        let before = run(&[(800, 0, true), (u32::from('b'), 1, true)]);
        assert_eq!(per_char("-b", &before, own, font), [None, None]);
    }

    #[test]
    fn one_glyph_for_several_characters_is_cut_when_it_is_that_many_cells_wide() {
        let joined = run(&[(900, 0, true)]);
        let font = measure(&[(900, 2.0 * CELL, 0.5, 19.0)]);
        assert_eq!(
            per_char("->", &joined, own, font),
            [shaped(900, 0, 2, 0), shaped(900, 1, 2, 0)]
        );
        // Not two cells wide: a proportional ligature stays two characters.
        let font = measure(&[(900, 1.5 * CELL, 0.5, 14.0)]);
        assert_eq!(per_char("fi", &joined, own, font), [None, None]);
        // Wide enough, but its ink leaves the two cells.
        let font = measure(&[(900, 2.0 * CELL, 0.5, 25.0)]);
        assert_eq!(per_char("->", &joined, own, font), [None, None]);
    }

    #[test]
    fn the_cache_empties_whole_at_its_bound() {
        let mut cache = Cache::default();
        for n in 0..CACHE_ROWS {
            cache.insert(Face::Regular, &n.to_string(), &[None]);
        }
        assert!(cache.get(Face::Regular, "0").is_some());
        cache.insert(Face::Regular, "next", &[None]);
        assert!(cache.get(Face::Regular, "0").is_none());
        assert!(cache.get(Face::Regular, "next").is_some());
        assert!(cache.get(Face::Bold, "next").is_none());
    }
}

#[cfg(all(test, target_os = "macos"))]
mod font_tests {
    use super::*;
    use crate::system::fixture;
    use crate::{Half, SizeClass, Spacing, Sprite};

    /// A secret the test font masks under `ss01`: it recognises the shape of
    /// the key, so some of its characters change and the others do not.
    const KEY: &str = "sk-ant-abc123";

    fn atlas(family: Option<&str>, features: &[(&str, bool)]) -> Atlas {
        fixture::register_test_fonts();
        let features: Vec<(String, bool)> = features
            .iter()
            .map(|&(tag, on)| (tag.to_owned(), on))
            .collect();
        Atlas::with_features(family, 13.0, 2.0, Spacing::default(), &features)
    }

    fn row(atlas: &mut Atlas, face: Face, text: &str) -> Vec<Option<Shaped>> {
        let mut out = Vec::new();
        atlas.shape_row(face, text, &mut out);
        out
    }

    #[test]
    fn the_mask_changes_a_secrets_glyphs_when_it_is_on_and_only_then() {
        let mut on = atlas(Some(fixture::OBEL_MONO), &[("ss01", true)]);
        let masked = row(&mut on, Face::Regular, KEY);
        assert!(masked.iter().any(Option::is_some), "the key is masked");
        assert!(masked.iter().any(Option::is_none), "and only part of it");
        // The font's mask rule is in `ccmp`, which runs unasked: with the
        // switch off, or no choice made, the characters are themselves.
        let mut off = atlas(Some(fixture::OBEL_MONO), &[("ss01", false)]);
        assert!(
            row(&mut off, Face::Regular, KEY)
                .iter()
                .all(Option::is_none)
        );
        let mut unchosen = atlas(Some(fixture::OBEL_MONO), &[]);
        assert!(unchosen.shapes(), "the font's own rules open the path");
        assert!(
            row(&mut unchosen, Face::Regular, KEY)
                .iter()
                .all(Option::is_none)
        );
    }

    #[test]
    fn the_bold_face_is_masked_where_the_regular_one_is() {
        let mut atlas = atlas(Some(fixture::OBEL_MONO), &[("ss01", true)]);
        let places = |row: Vec<Option<Shaped>>| row.iter().map(Option::is_some).collect::<Vec<_>>();
        let regular = places(row(&mut atlas, Face::Regular, KEY));
        assert_eq!(places(row(&mut atlas, Face::Bold, KEY)), regular);
        assert!(regular.contains(&true));
    }

    #[test]
    fn an_alternate_letter_changes_that_letter_alone() {
        let mut atlas = atlas(Some(fixture::OBEL_MONO), &[("cv02", true)]);
        let shaped = row(&mut atlas, Face::Regular, "abca");
        assert!(shaped[0].is_some() && shaped[3].is_some());
        assert_eq!(shaped[0], shaped[3], "one alternate, one glyph");
        assert_eq!((shaped[1], shaped[2]), (None, None));
    }

    #[test]
    fn a_ligature_drawn_with_spacers_is_cut_into_the_cells_it_covers() {
        // No choice made: the font's own rule joins the pairs.
        let mut atlas = atlas(Some(fixture::JETBRAINS_MONO), &[]);
        assert!(atlas.shapes());
        for (text, cells) in [("->", 2), ("!=", 2), ("===", 3), ("<!--", 4)] {
            let slices = row(&mut atlas, Face::Regular, text);
            let first = slices[0].unwrap_or_else(|| panic!("{text} was not joined"));
            for (at, slice) in slices.iter().enumerate() {
                let slice = slice.unwrap_or_else(|| panic!("{text}: cell {at} has no slice"));
                assert_eq!(
                    (
                        slice.glyph,
                        usize::from(slice.slice),
                        usize::from(slice.cells)
                    ),
                    (first.glyph, at, cells),
                    "{text}"
                );
                assert_eq!(usize::from(slice.lead), cells - 1, "{text}");
            }
        }
        // The letters around a ligature and a word the font leaves alone stay
        // themselves.
        let line = row(&mut atlas, Face::Regular, "a->b www");
        let joined: Vec<bool> = line.iter().map(Option::is_some).collect();
        assert_eq!(
            joined,
            [false, true, true, false, false, false, false, false]
        );
    }

    /// The slices of a ligature are the one-piece raster of the whole shape
    /// cut at the cells' edges: nothing is missing at a seam and no pixel is
    /// painted by two cells. Below `1` the slot is wider than its cell and the
    /// cut matters; above it the slot is the cell.
    #[test]
    fn the_slices_of_a_ligature_are_the_whole_shape_cut_at_the_cell_edges() {
        fixture::register_test_fonts();
        for (scale, letter) in [
            (1.0, 1.0),
            (2.0, 1.0),
            (1.0, 1.2),
            (2.0, 1.2),
            (1.0, 0.9),
            (2.0, 0.9),
            (1.0, 0.7),
        ] {
            let spacing = Spacing { line: 1.0, letter };
            let mut atlas =
                Atlas::with_features(Some(fixture::JETBRAINS_MONO), 13.0, scale, spacing, &[]);
            for text in ["->", "===", "<!--"] {
                let slices: Vec<Shaped> = row(&mut atlas, Face::Regular, text)
                    .into_iter()
                    .map(|slice| slice.expect("a joined cell"))
                    .collect();
                let n = slices.len();
                let (cell, slot) = (atlas.metrics(), atlas.slot_metrics());
                let (bx, _) = atlas.glyph_box(SizeClass::Normal);
                let (cw, sw, sh) = (
                    usize::from(cell.cell_px.0),
                    usize::from(slot.cell_px.0),
                    usize::from(slot.cell_px.1),
                );
                // The one-piece raster: the slots side by side.
                let ww = (n - 1) * cw + sw;
                let wide = crate::rules::Metrics {
                    cell_px: (u16::try_from(ww).expect("a short row"), slot.cell_px.1),
                    ..slot
                };
                let mut whole = vec![0u8; wide.slot_bytes()];
                let cells = u8::try_from(n).expect("a few cells");
                raster::draw_slice(
                    atlas.faces.get(Face::Regular),
                    slices[0],
                    wide,
                    bx.cols(cells),
                    0.0,
                    &mut whole,
                );
                assert!(
                    whole.iter().any(|&b| b > 0),
                    "{text}@{scale}x/{letter}: no ink"
                );
                let edge = usize::from(atlas.slot_offset().0);
                // The slice that painted each column of the one-piece raster.
                let mut owner: Vec<Option<usize>> = vec![None; ww];
                for (at, slice) in slices.iter().enumerate() {
                    let (placed, upload) = atlas.slot(
                        Sprite::Glyph(*slice),
                        Face::Regular,
                        SizeClass::Normal,
                        Half::Whole,
                    );
                    assert_ne!(placed.slot, crate::TOFU);
                    let bytes = upload.expect("a new slot").bytes.to_vec();
                    // The pad outside the shape's two ends is the shape's own;
                    // the cut is at the cell's edge wherever a neighbour is.
                    let lo = if at > 0 { edge } else { 0 };
                    let hi = if at + 1 < n { edge + cw } else { sw };
                    for y in 0..sh {
                        for x in 0..sw {
                            let want = if (lo..hi).contains(&x) {
                                whole[y * ww + at * cw + x]
                            } else {
                                0
                            };
                            assert_eq!(
                                bytes[y * sw + x],
                                want,
                                "{text}@{scale}x/{letter}: slice {at}, ({x}, {y})"
                            );
                            if bytes[y * sw + x] > 0 {
                                let held = owner[at * cw + x].replace(at);
                                assert!(
                                    held.is_none_or(|other| other == at),
                                    "{text}@{scale}x/{letter}: column {} is painted by slices {held:?} and {at}",
                                    at * cw + x
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_ligature_is_not_formed_when_the_user_turned_it_off() {
        let mut atlas = atlas(Some(fixture::JETBRAINS_MONO), &[("calt", false)]);
        assert!(atlas.shapes(), "a choice opens the path");
        assert!(
            row(&mut atlas, Face::Regular, "a->b != c <!--")
                .iter()
                .all(Option::is_none)
        );
    }

    #[test]
    fn a_font_without_rules_is_never_asked_and_a_row_is_asked_once() {
        let mut menlo = atlas(None, &[]);
        assert!(!menlo.shapes());
        assert_eq!(row(&mut menlo, Face::Regular, KEY), vec![None; KEY.len()]);
        assert_eq!(menlo.shape_calls(), 0);

        let mut obel = atlas(Some(fixture::OBEL_MONO), &[("ss01", true)]);
        let first = row(&mut obel, Face::Regular, KEY);
        assert_eq!(obel.shape_calls(), 1);
        assert_eq!(row(&mut obel, Face::Regular, KEY), first);
        assert_eq!(obel.shape_calls(), 1, "the second ask is the cache's");
        let _ = row(&mut obel, Face::Bold, KEY);
        assert_eq!(obel.shape_calls(), 2, "a face is a key of its own");
    }

    #[test]
    fn a_glyph_sprite_takes_its_own_slot_and_draws_ink() {
        let mut atlas = atlas(Some(fixture::OBEL_MONO), &[("ss01", true)]);
        let masked = row(&mut atlas, Face::Regular, KEY)
            .into_iter()
            .flatten()
            .next()
            .expect("a masked glyph");
        let ask = |atlas: &mut Atlas, sprite| {
            let (placed, upload) =
                atlas.slot(sprite, Face::Regular, SizeClass::Normal, Half::Whole);
            (placed, upload.map(|upload| upload.bytes.to_vec()))
        };
        let (first, bytes) = ask(&mut atlas, Sprite::Glyph(masked));
        assert_ne!(first.slot, crate::TOFU);
        assert!(
            bytes.expect("an upload").iter().any(|&b| b > 0),
            "it has ink"
        );
        let (again, upload) = ask(&mut atlas, Sprite::Glyph(masked));
        assert_eq!((again.slot, upload), (first.slot, None));
        let (letter, _) = ask(&mut atlas, Sprite::Char('2'));
        assert_ne!(letter.slot, first.slot);
    }
}
