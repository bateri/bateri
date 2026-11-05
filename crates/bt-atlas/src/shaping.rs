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
//! The mapping from a shaped line back to cells holds only where the shaper
//! made one glyph per character, in order. A line that does not (a ligature
//! was formed, characters were reordered) is not shaped here: its characters
//! are drawn as themselves, which is what the cell grid would do without
//! this module.

use std::collections::HashMap;

use crate::Atlas;
use crate::features::has_default_rules;
use crate::raster;
use crate::rules::Face;
use crate::system::{Backend, Font, FontSystem, RunGlyph};

/// The features forced **off** while a cell holds one character: with them
/// on, the shaper would join `->` into one glyph and the two cells it
/// covers would draw half of it each. The one place the list is written.
const FORCED_OFF: [&str; 3] = ["calt", "liga", "clig"];

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

/// The features every face is given when the path is open: the user's, with
/// the forced-off list over them.
pub(crate) fn applied(features: &[(String, bool)]) -> Vec<(String, bool)> {
    let mut all: Vec<(String, bool)> = features
        .iter()
        .filter(|(tag, _)| !FORCED_OFF.contains(&tag.as_str()))
        .cloned()
        .collect();
    all.extend(FORCED_OFF.iter().map(|tag| ((*tag).to_owned(), false)));
    all
}

/// What each character of `text` is drawn as: the shaped glyph where it
/// differs from the character's own, `None` where the character draws as
/// itself.
///
/// `cmap` gives a character's own glyph. All `None` unless the shaper made
/// exactly one glyph per character, each at its character's place; a glyph
/// of another font (the cascade's), a character drawn procedurally, and a
/// character the font lacks are `None` too.
fn per_char(text: &str, run: &[RunGlyph], cmap: impl Fn(char) -> Option<u32>) -> Vec<Option<u32>> {
    let chars = text.chars().count();
    let mut result = vec![None; chars];
    if run.len() != chars {
        return result;
    }
    let mut unit = 0;
    for (ch, glyph) in text.chars().zip(run) {
        if glyph.index != unit {
            return vec![None; chars];
        }
        unit += ch.len_utf16();
    }
    for ((slot, ch), glyph) in result.iter_mut().zip(text.chars()).zip(run) {
        let own = cmap(ch);
        if glyph.base && !raster::is_procedural(ch) && own.is_some_and(|own| own != glyph.glyph) {
            *slot = Some(glyph.glyph);
        }
    }
    result
}

/// One face's rows: the text → what each of its characters is drawn as.
type Rows = HashMap<Box<str>, Box<[Option<u32>]>>;

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
    fn get(&self, face: Face, text: &str) -> Option<&[Option<u32>]> {
        self.rows[face as usize].get(text).map(|row| &**row)
    }

    fn insert(&mut self, face: Face, text: &str, row: &[Option<u32>]) {
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
    /// (cleared first). A `Some` is drawn with [`Sprite::Glyph`].
    ///
    /// `text` is a row of cells, one character per cell, a blank cell as a
    /// space. The answer is cached per face and text.
    pub fn shape_row(&mut self, face: Face, text: &str, out: &mut Vec<Option<u32>>) {
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
            Some(run) => per_char(text, &run, |ch| Backend::glyph(font, ch)),
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

    #[test]
    fn only_the_glyphs_that_differ_from_the_cmap_are_kept() {
        let shaped = run(&[
            (u32::from('a'), 0, true),
            (900, 1, true),
            (u32::from('c'), 2, true),
        ]);
        assert_eq!(per_char("abc", &shaped, own), [None, Some(900), None]);
    }

    #[test]
    fn a_line_that_is_not_one_glyph_per_character_is_not_shaped() {
        // A ligature: two characters, one glyph.
        let joined = run(&[(900, 0, true)]);
        assert_eq!(per_char("->", &joined, own), [None, None]);
        // Two glyphs but the second came from the first character's place.
        let moved = run(&[(900, 0, true), (901, 0, true)]);
        assert_eq!(per_char("ab", &moved, own), [None, None]);
    }

    #[test]
    fn indices_are_counted_in_utf16_units() {
        // `𝐚` is two UTF-16 units; the character after it stands at 2.
        let shaped = run(&[(900, 0, true), (901, 2, true)]);
        assert_eq!(per_char("𝐚b", &shaped, own), [Some(900), Some(901)]);
    }

    #[test]
    fn another_fonts_glyph_a_missing_character_and_a_procedural_one_stay_themselves() {
        let shaped = run(&[(900, 0, false), (901, 1, true), (902, 2, true)]);
        let cmap = |ch: char| (ch != 'b').then(|| u32::from(ch));
        assert_eq!(per_char("a\u{2588}b", &shaped, cmap), [None, None, None]);
    }

    #[test]
    fn the_forced_off_list_wins_over_the_users_and_leaves_the_rest() {
        let user = [("calt".to_owned(), true), ("ss01".to_owned(), true)];
        let all = applied(&user);
        assert!(all.contains(&("ss01".to_owned(), true)));
        for tag in FORCED_OFF {
            assert_eq!(
                all.iter()
                    .filter(|(held, _)| held == tag)
                    .collect::<Vec<_>>(),
                [&(tag.to_owned(), false)],
            );
        }
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

    fn row(atlas: &mut Atlas, face: Face, text: &str) -> Vec<Option<u32>> {
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
        let places = |row: Vec<Option<u32>>| row.iter().map(Option::is_some).collect::<Vec<_>>();
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
    fn a_ligature_is_not_formed_while_a_cell_holds_one_character() {
        // The font joins `->` and `!=` when asked plainly …
        fixture::register_test_fonts();
        let (plain, _) = Backend::open(fixture::JETBRAINS_MONO, 26.0);
        let joined = Backend::shape_run(&plain, "->!=").expect("a run");
        assert!(
            "->!="
                .chars()
                .zip(&joined)
                .any(|(ch, glyph)| Backend::glyph(&plain, ch) != Some(glyph.glyph)),
            "the font forms a ligature by itself, or this test says nothing"
        );
        // … and the atlas draws every character as itself.
        let mut atlas = atlas(Some(fixture::JETBRAINS_MONO), &[]);
        assert!(atlas.shapes());
        assert!(
            row(&mut atlas, Face::Regular, "a->b != c")
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
