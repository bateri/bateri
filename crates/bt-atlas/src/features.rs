//! What a font offers in OpenType features: the tags its `GSUB` table lists
//! and the names its `name` table gives them.
//!
//! The settings window and the palette list the choices **of the selected
//! font**, so the list comes from the font's own file, not from a table
//! kept per family. This module reads the two tables from bytes and touches
//! no platform: [`FontSystem::table`](crate::system::FontSystem::table) hands
//! the bytes over, the parsing exists once for every backend.
//!
//! The bytes are the font's, i.e. untrusted: every read checks its bounds,
//! nothing panics, and a broken table answers an empty list.

use std::collections::BTreeMap;

use crate::rules::same_family;
use crate::sfnt::u16_at;
use crate::system::{Backend, Font, FontSystem};

/// One OpenType feature a font lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontFeature {
    /// The four-character tag (`ss01`, `calt`).
    pub tag: String,
    /// The name the font gives it (`Mask secrets`); `None` when the font
    /// names nothing — only stylistic sets (`ss01`–`ss20`) and character
    /// variants (`cv01`–`cv99`) carry a name, and a font may leave even those
    /// out.
    pub name: Option<String>,
    /// The characters the feature draws differently, by code point: what the glyphs under its
    /// substitution lookups (`GSUB` types 1, 3, 6 and 7) are, read back through the font's `cmap`.
    /// Filled for the stylistic sets, the character variants and `zero`, the features the
    /// settings window shows letters for; empty for the rest, and for a feature whose changes
    /// no character stands for (a rule that works on a sequence of glyphs the `cmap` does not
    /// reach). A font's feature may change a hundred characters: the list is bounded.
    pub chars: Vec<char>,
}

/// The features a shaper switches on without being asked. A font with
/// rules under none of these shapes like a font without rules unless the
/// user turns something on.
const DEFAULT_ON: [&[u8; 4]; 5] = [b"ccmp", b"calt", b"liga", b"clig", b"rlig"];

/// The size a font is opened at only to read its tables: the tables do not
/// depend on it.
const PROBE_SIZE: f64 = 12.0;

/// The features the named family lists, sorted by tag; `None` is the default
/// chain's font. Empty for a family that is not installed, for a font
/// without a `GSUB` table (Menlo), and on a backend that cannot read tables.
pub fn font_features(family: Option<&str>) -> Vec<FontFeature> {
    let Some(font) = open_family(family) else {
        return Vec::new();
    };
    let Some(gsub) = Backend::table(&font, *b"GSUB") else {
        return Vec::new();
    };
    let names = Backend::table(&font, *b"name").unwrap_or_default();
    let cmap = Backend::table(&font, *b"cmap").unwrap_or_default();
    parse(&gsub, &names, &cmap)
}

/// The tags among `chosen` (tag, on) that the named family's font does not list — what a
/// choice in the settings file asks for that the font ignores without a word. `None` is the
/// default chain's font. Empty for a family that is not installed, and on a backend that cannot
/// read tables.
pub fn lacking_features(family: Option<&str>, chosen: &[(String, bool)]) -> Vec<String> {
    open_family(family).map_or_else(Vec::new, |font| lacking(&font, chosen))
}

/// The named family's font, opened to read its tables (`None`: the default chain's font);
/// `None` for a family that is not installed. The list a font offers and the tags the file asks
/// of it that it lacks are read from the same font, so the page cannot show a tile for a family
/// it then calls missing.
fn open_family(family: Option<&str>) -> Option<Font> {
    match family {
        None => Some(crate::app_fonts::open_default(PROBE_SIZE).0),
        Some(name) => {
            let (font, returned) = Backend::open(name, PROBE_SIZE);
            same_family(&returned, name).then_some(font)
        }
    }
}

/// Does the font have a rule that runs without the user asking for it
/// (`ccmp`, `calt`, `liga`, `clig`, `rlig`)? A font without any is not
/// shaped until the user selects a feature.
pub(crate) fn has_default_rules(gsub: &[u8]) -> bool {
    tags(gsub).any(|tag| DEFAULT_ON.iter().any(|on| **on == tag.tag))
}

/// The tags among `chosen` that the font behind `font` does not list in its
/// `GSUB` or `GPOS` table, in the order given. A font that has neither lists
/// nothing (Menlo), so every chosen tag is one it lacks; a backend that cannot read
/// tables at all (it has no `head` either) says nothing, because it cannot
/// tell a font that lacks the tag from a font it cannot read.
pub(crate) fn lacking(font: &Font, chosen: &[(String, bool)]) -> Vec<String> {
    if chosen.is_empty() || Backend::table(font, *b"head").is_none() {
        return Vec::new();
    }
    // `GPOS` has the same header and feature list as `GSUB`: a tag the font positions
    // with (`kern`) is a feature it has, though no rule of `GSUB` carries it.
    let listed: Vec<[u8; 4]> = [*b"GSUB", *b"GPOS"]
        .into_iter()
        .filter_map(|table| Backend::table(font, table))
        .flat_map(|table| tags(&table).map(|entry| entry.tag).collect::<Vec<_>>())
        .collect();
    chosen
        .iter()
        .filter(|(tag, _)| {
            <[u8; 4]>::try_from(tag.as_bytes()).map_or(true, |tag| !listed.contains(&tag))
        })
        .map(|(tag, _)| tag.clone())
        .collect()
}

/// The features of a `GSUB` table, named from a `name` table and given their characters from a
/// `cmap` table, sorted by tag and deduplicated (a tag listed under several scripts is one
/// feature; its first name wins and its characters are the union).
pub(crate) fn parse(gsub: &[u8], names: &[u8], cmap: &[u8]) -> Vec<FontFeature> {
    let names = Names::new(names);
    let mut inverse = None;
    let mut found: BTreeMap<[u8; 4], (Option<String>, Vec<u16>)> = BTreeMap::new();
    for entry in tags(gsub) {
        let (name, glyphs) = found.entry(entry.tag).or_default();
        if name.is_none()
            && let Some(id) = entry.name_id
        {
            *name = names.get(id);
        }
        if shows_letters(&entry.tag) && glyphs.len() < MAX_GLYPHS {
            glyphs.extend(glyphs_of(gsub, entry.feature));
        }
    }
    found
        .into_iter()
        .filter_map(|(tag, (name, mut glyphs))| {
            // A tag is four ASCII bytes by the specification; anything else
            // is a broken record and not a feature.
            let tag = std::str::from_utf8(&tag).ok()?.to_owned();
            if !tag.bytes().all(|b| b.is_ascii_alphanumeric()) {
                return None;
            }
            let mut chars = Vec::new();
            if !glyphs.is_empty() {
                glyphs.sort_unstable();
                glyphs.dedup();
                let inverse = inverse.get_or_insert_with(|| characters(cmap));
                chars = glyphs
                    .iter()
                    .filter_map(|glyph| inverse.get(glyph))
                    .copied()
                    .collect();
                chars.sort_unstable();
                chars.dedup();
                chars.truncate(MAX_CHARS);
            }
            Some(FontFeature { tag, name, chars })
        })
        .collect()
}

/// The features whose characters are read: the stylistic sets, the character variants and the
/// slashed zero.
fn shows_letters(tag: &[u8; 4]) -> bool {
    tag == b"zero"
        || (matches!(&tag[..2], b"ss" | b"cv") && tag[2..].iter().all(u8::is_ascii_digit))
}

/// One record of the `GSUB` feature list.
struct Entry {
    tag: [u8; 4],
    /// Where its feature table starts in the `GSUB` table.
    feature: usize,
    /// The `name` table id of its label, when the feature has parameters.
    name_id: Option<u16>,
}

/// The records of the `GSUB` feature list, in file order. Ends at the first
/// read that falls outside the table.
fn tags(gsub: &[u8]) -> impl Iterator<Item = Entry> + '_ {
    let list = u16_at(gsub, 6).map(usize::from).filter(|at| *at != 0);
    let count = list.and_then(|at| u16_at(gsub, at)).map_or(0, usize::from);
    (0..count).map_while(move |i| {
        let list = list?;
        let record = list.checked_add(2)?.checked_add(i.checked_mul(6)?)?;
        let tag: [u8; 4] = gsub.get(record..record.checked_add(4)?)?.try_into().ok()?;
        let feature = list.checked_add(usize::from(u16_at(gsub, record + 4)?))?;
        Some(Entry {
            tag,
            feature,
            name_id: label_id(gsub, feature, tag),
        })
    })
}

/// The `name` id of the label a stylistic set or a character variant carries
/// in its feature parameters. Both parameter tables start with a version or
/// format word and the label's id.
fn label_id(gsub: &[u8], feature: usize, tag: [u8; 4]) -> Option<u16> {
    let named = matches!(&tag[..2], b"ss" | b"cv") && tag[2..].iter().all(u8::is_ascii_digit);
    if !named {
        return None;
    }
    let offset = usize::from(u16_at(gsub, feature)?);
    if offset == 0 {
        return None;
    }
    let params = feature.checked_add(offset)?;
    u16_at(gsub, params.checked_add(2)?).filter(|id| *id != 0)
}

/// The strings of a `name` table, by id.
struct Names<'a> {
    table: &'a [u8],
}

impl<'a> Names<'a> {
    fn new(table: &'a [u8]) -> Self {
        Self { table }
    }

    /// The string of `id`: the Windows English one when there is one, else
    /// any Windows or Unicode one, else a Macintosh Roman one. Empty strings
    /// are no name.
    fn get(&self, id: u16) -> Option<String> {
        let table = self.table;
        let count = usize::from(u16_at(table, 2)?);
        let storage = usize::from(u16_at(table, 4)?);
        let mut best: Option<(u8, String)> = None;
        for i in 0..count {
            let record = 6usize.checked_add(i.checked_mul(12)?)?;
            let Some(fields) = table.get(record..record.checked_add(12)?) else {
                break;
            };
            let word = |n: usize| u16::from_be_bytes([fields[n * 2], fields[n * 2 + 1]]);
            let (platform, _encoding, language, name_id, length, offset) =
                (word(0), word(1), word(2), word(3), word(4), word(5));
            if name_id != id {
                continue;
            }
            let rank = match (platform, language) {
                (3, 0x409) => 0,
                (3, _) => 1,
                (0, _) => 2,
                (1, _) => 3,
                _ => continue,
            };
            if best.as_ref().is_some_and(|(held, _)| *held <= rank) {
                continue;
            }
            let Some(start) = storage.checked_add(usize::from(offset)) else {
                continue;
            };
            let Some(bytes) = start
                .checked_add(usize::from(length))
                .and_then(|end| table.get(start..end))
            else {
                continue;
            };
            let text = if platform == 1 {
                bytes.iter().map(|b| char::from(*b)).collect::<String>()
            } else {
                let units = bytes
                    .chunks_exact(2)
                    .map(|pair| u16::from_be_bytes([pair[0], pair[1]]));
                char::decode_utf16(units)
                    .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
                    .collect()
            };
            let text = text.trim().to_owned();
            if !text.is_empty() {
                best = Some((rank, text));
            }
        }
        best.map(|(_, text)| text)
    }
}

/// The most glyphs read for one feature, the most characters kept for it, how many lookups and
/// subtables of a feature are followed, and how many characters of a `cmap` are walked. The
/// bytes are a font's, i.e. untrusted: each bound is far above what a real font needs and keeps
/// a table that lies about its counts from costing more than a blink.
const MAX_GLYPHS: usize = 4096;
const MAX_CHARS: usize = 256;
const MAX_LOOKUPS: usize = 256;
const MAX_SUBTABLES: usize = 64;
const MAX_CMAP: usize = 0x30000;

/// The glyphs the substitution lookups of the feature table at `feature` act on: the coverage of
/// each single (1) and alternate (3) substitution, of the input of each chained context (6), and
/// of what an extension (7) wraps. Not sorted, not deduplicated; at most [`MAX_GLYPHS`] and
/// empty on any read outside the table.
fn glyphs_of(gsub: &[u8], feature: usize) -> Vec<u16> {
    let mut out = Vec::new();
    let _ = collect_lookups(gsub, feature, &mut out);
    out
}

fn collect_lookups(gsub: &[u8], feature: usize, out: &mut Vec<u16>) -> Option<()> {
    let list = usize::from(u16_at(gsub, 8)?);
    if list == 0 {
        return None;
    }
    let lookups = usize::from(u16_at(gsub, list)?);
    let count = usize::from(u16_at(gsub, feature.checked_add(2)?)?);
    for i in 0..count.min(MAX_LOOKUPS) {
        let index = usize::from(u16_at(gsub, feature.checked_add(4)?.checked_add(i * 2)?)?);
        if index >= lookups {
            continue;
        }
        let lookup = list.checked_add(usize::from(u16_at(
            gsub,
            list.checked_add(2)?.checked_add(index * 2)?,
        )?))?;
        let kind = u16_at(gsub, lookup)?;
        let subtables = usize::from(u16_at(gsub, lookup.checked_add(4)?)?);
        for s in 0..subtables.min(MAX_SUBTABLES) {
            let offset = u16_at(gsub, lookup.checked_add(6)?.checked_add(s * 2)?)?;
            let _ = subtable_glyphs(gsub, kind, lookup.checked_add(usize::from(offset))?, out);
            if out.len() >= MAX_GLYPHS {
                return Some(());
            }
        }
    }
    Some(())
}

/// The glyphs of one substitution subtable of type `kind`, at `at`.
fn subtable_glyphs(gsub: &[u8], kind: u16, at: usize, out: &mut Vec<u16>) -> Option<()> {
    match kind {
        // Single and alternate substitution: the coverage is the word after the format in both
        // of the first's formats and in the second's only one.
        1 | 3 => coverage(
            gsub,
            at.checked_add(usize::from(u16_at(gsub, at.checked_add(2)?)?))?,
            out,
        ),
        6 => match u16_at(gsub, at)? {
            // Formats 1 and 2 name the coverage of the first input glyph.
            1 | 2 => coverage(
                gsub,
                at.checked_add(usize::from(u16_at(gsub, at.checked_add(2)?)?))?,
                out,
            ),
            // Format 3 lists a coverage for the backtrack, the input and the lookahead; the
            // input's is the one whose glyphs the rule can substitute.
            3 => {
                let backtrack = usize::from(u16_at(gsub, at.checked_add(2)?)?);
                let input_at = at.checked_add(4)?.checked_add(backtrack * 2)?;
                let input = usize::from(u16_at(gsub, input_at)?);
                for i in 0..input.min(MAX_SUBTABLES) {
                    let offset = u16_at(gsub, input_at.checked_add(2)?.checked_add(i * 2)?)?;
                    let _ = coverage(gsub, at.checked_add(usize::from(offset))?, out);
                }
                Some(())
            }
            _ => Some(()),
        },
        // An extension names the real subtable by a 32-bit offset from itself; one that wraps
        // another extension is a loop and is left alone.
        7 if u16_at(gsub, at)? == 1 => {
            let inner = u16_at(gsub, at.checked_add(2)?)?;
            let offset = u32::from_be_bytes(
                gsub.get(at.checked_add(4)?..at.checked_add(8)?)?
                    .try_into()
                    .ok()?,
            );
            if inner == 7 {
                return Some(());
            }
            subtable_glyphs(
                gsub,
                inner,
                at.checked_add(usize::try_from(offset).ok()?)?,
                out,
            )
        }
        _ => Some(()),
    }
}

/// The glyphs of the coverage table at `at`: a list (format 1) or ranges (format 2).
fn coverage(gsub: &[u8], at: usize, out: &mut Vec<u16>) -> Option<()> {
    let count = usize::from(u16_at(gsub, at.checked_add(2)?)?);
    match u16_at(gsub, at)? {
        1 => {
            for i in 0..count {
                if out.len() >= MAX_GLYPHS {
                    break;
                }
                out.push(u16_at(gsub, at.checked_add(4)?.checked_add(i * 2)?)?);
            }
        }
        2 => {
            for i in 0..count {
                let record = at.checked_add(4)?.checked_add(i * 6)?;
                let (start, end) = (u16_at(gsub, record)?, u16_at(gsub, record.checked_add(2)?)?);
                for glyph in start..=end.max(start) {
                    if out.len() >= MAX_GLYPHS || glyph > end {
                        break;
                    }
                    out.push(glyph);
                }
            }
        }
        _ => {}
    }
    Some(())
}

/// The glyph → character inverse of a `cmap` table: for each glyph the lowest printable code
/// point that maps to it. The Unicode subtable of format 12 or 4 is read; any other, or a table
/// that does not hold together, gives an empty inverse.
fn characters(cmap: &[u8]) -> BTreeMap<u16, char> {
    let mut inverse = BTreeMap::new();
    let _ = read_cmap(cmap, &mut inverse);
    inverse
}

fn read_cmap(cmap: &[u8], inverse: &mut BTreeMap<u16, char>) -> Option<()> {
    let tables = usize::from(u16_at(cmap, 2)?);
    // The best subtable: Windows full repertoire, Unicode full, then the basic planes.
    let mut best: Option<(u8, usize)> = None;
    for i in 0..tables.min(64) {
        let record = 4usize.checked_add(i * 8)?;
        let (platform, encoding) = (u16_at(cmap, record)?, u16_at(cmap, record.checked_add(2)?)?);
        let offset = u32::from_be_bytes(
            cmap.get(record.checked_add(4)?..record.checked_add(8)?)?
                .try_into()
                .ok()?,
        );
        let rank = match (platform, encoding) {
            (3, 10) => 0,
            (0, 4 | 6) => 1,
            (3, 1) => 2,
            (0, _) => 3,
            _ => continue,
        };
        if best.is_none_or(|(held, _)| rank < held) {
            best = Some((rank, usize::try_from(offset).ok()?));
        }
    }
    let at = best?.1;
    let mut budget = MAX_CMAP;
    let mut note = |glyph: u16, code: u32| {
        if glyph == 0 {
            return;
        }
        if let Some(c) = char::from_u32(code).filter(|c| !c.is_control()) {
            let held = inverse.entry(glyph).or_insert(c);
            *held = (*held).min(c);
        }
    };
    match u16_at(cmap, at)? {
        12 => {
            let groups = usize::try_from(u32::from_be_bytes(
                cmap.get(at.checked_add(12)?..at.checked_add(16)?)?
                    .try_into()
                    .ok()?,
            ))
            .ok()?;
            for i in 0..groups {
                let group = at.checked_add(16)?.checked_add(i.checked_mul(12)?)?;
                let word = |n: usize| -> Option<u32> {
                    Some(u32::from_be_bytes(
                        cmap.get(group.checked_add(n)?..group.checked_add(n + 4)?)?
                            .try_into()
                            .ok()?,
                    ))
                };
                let (start, end, glyph) = (word(0)?, word(4)?, word(8)?);
                for code in start..=end.max(start) {
                    if budget == 0 || code > end {
                        return Some(());
                    }
                    budget -= 1;
                    if let Ok(glyph) = u16::try_from(glyph.checked_add(code - start)?) {
                        note(glyph, code);
                    }
                }
            }
        }
        4 => {
            let segments = usize::from(u16_at(cmap, at.checked_add(6)?)? / 2);
            let ends = at.checked_add(14)?;
            let starts = ends.checked_add(segments * 2)?.checked_add(2)?;
            let deltas = starts.checked_add(segments * 2)?;
            let ranges = deltas.checked_add(segments * 2)?;
            for i in 0..segments {
                let end = u16_at(cmap, ends.checked_add(i * 2)?)?;
                let start = u16_at(cmap, starts.checked_add(i * 2)?)?;
                let delta = u16_at(cmap, deltas.checked_add(i * 2)?)?;
                let range_at = ranges.checked_add(i * 2)?;
                let range = usize::from(u16_at(cmap, range_at)?);
                for code in start..=end.max(start) {
                    if budget == 0 || code > end {
                        return Some(());
                    }
                    budget -= 1;
                    if code == 0xffff {
                        break;
                    }
                    let glyph = if range == 0 {
                        code.wrapping_add(delta)
                    } else {
                        let slot = range_at
                            .checked_add(range)?
                            .checked_add(usize::from(code - start) * 2)?;
                        match u16_at(cmap, slot) {
                            Some(0) | None => continue,
                            Some(glyph) => glyph.wrapping_add(delta),
                        }
                    };
                    note(glyph, u32::from(code));
                }
            }
        }
        _ => {}
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A font's table out of its `sfnt` directory, for the fonts the tests
    /// read straight from the files.
    fn table_of<'a>(font: &'a [u8], tag: &[u8; 4]) -> &'a [u8] {
        let count = usize::from(u16_at(font, 4).expect("an sfnt header"));
        for i in 0..count {
            let record = 12 + i * 16;
            if &font[record..record + 4] == tag {
                let at = u32::from_be_bytes(font[record + 8..record + 12].try_into().unwrap());
                let len = u32::from_be_bytes(font[record + 12..record + 16].try_into().unwrap());
                return &font[at as usize..(at + len) as usize];
            }
        }
        panic!("no {} table", String::from_utf8_lossy(tag));
    }

    fn file(name: &str) -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/fonts")
            .join(name);
        std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    fn features_of(file_name: &str) -> (Vec<FontFeature>, Vec<u8>, Vec<u8>) {
        let font = file(file_name);
        let gsub = table_of(&font, b"GSUB").to_vec();
        let names = table_of(&font, b"name").to_vec();
        let cmap = table_of(&font, b"cmap").to_vec();
        (parse(&gsub, &names, &cmap), gsub, names)
    }

    fn cmap_of(file_name: &str) -> Vec<u8> {
        table_of(&file(file_name), b"cmap").to_vec()
    }

    fn named<'a>(features: &'a [FontFeature], tag: &str) -> Option<&'a FontFeature> {
        features.iter().find(|f| f.tag == tag)
    }

    #[test]
    fn obel_mono_names_its_mask_and_its_variants() {
        let (features, gsub, _) = features_of("ObelMono/ObelMono[wght].ttf");
        assert_eq!(
            named(&features, "ss01").and_then(|f| f.name.as_deref()),
            Some("Mask secrets")
        );
        assert_eq!(
            named(&features, "cv02").and_then(|f| f.name.as_deref()),
            Some("Single-story a")
        );
        // The rules live in `ccmp`, which a shaper runs unasked: the font is
        // shaped even before the user selects anything.
        assert!(has_default_rules(&gsub));
        // A feature without parameters is listed, unnamed.
        assert_eq!(named(&features, "ccmp").map(|f| f.name.clone()), Some(None));
        // Sorted by tag, one entry per tag.
        let tags: Vec<&str> = features.iter().map(|f| f.tag.as_str()).collect();
        let mut sorted = tags.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(tags, sorted);
    }

    #[test]
    fn jetbrains_mono_has_its_ligatures_and_a_broken_equals_set() {
        let (features, gsub, _) = features_of("JetBrainsMono/JetBrainsMono[wght].ttf");
        assert!(named(&features, "calt").is_some());
        assert!(named(&features, "zero").is_some());
        assert_eq!(
            named(&features, "ss19").and_then(|f| f.name.as_deref()),
            Some("Broken equals ligatures")
        );
        // A character variant the font leaves unnamed shows up under its tag
        // alone.
        assert_eq!(named(&features, "cv01").map(|f| f.name.clone()), Some(None));
        assert!(has_default_rules(&gsub));
    }

    fn chars_of(features: &[FontFeature], tag: &str) -> String {
        named(features, tag)
            .map(|f| f.chars.iter().collect())
            .unwrap_or_default()
    }

    /// A set or a variant lists the characters it draws, read back through the `cmap`: the
    /// letters the settings window shows on its tile.
    #[test]
    fn a_stylistic_set_knows_the_characters_it_draws() {
        let (jetbrains, _, _) = features_of("JetBrainsMono/JetBrainsMono[wght].ttf");
        let w = chars_of(&jetbrains, "cv07");
        assert!(w.contains('W') && w.contains('w'), "cv07 changes {w:?}");
        assert_eq!(chars_of(&jetbrains, "zero"), "0");
        let (obel, _, _) = features_of("ObelMono/ObelMono[wght].ttf");
        assert!(chars_of(&obel, "cv02").contains('a'));
        assert_eq!(chars_of(&obel, "cv01"), "0");
        // The mask covers the whole alphabet: it is not a letter to show.
        assert!(chars_of(&obel, "ss01").len() > 100);
        let (fira, _, _) = features_of("FiraCode/FiraCode-VF.ttf");
        assert!(chars_of(&fira, "ss01").contains('r'));
        let (geist, _, _) = features_of("GeistMono/GeistMono[wght].ttf");
        assert!(chars_of(&geist, "ss11").contains('='));
        // A feature that is not a set, a variant or the zero is listed without characters, and
        // so is one that only works on sequences the `cmap` does not reach.
        assert_eq!(chars_of(&jetbrains, "calt"), "");
        for features in [&jetbrains, &obel, &fira, &geist] {
            for feature in features {
                assert!(feature.chars.len() <= MAX_CHARS);
                assert!(feature.chars.windows(2).all(|pair| pair[0] < pair[1]));
            }
        }
    }

    /// A `cmap` that claims four billion groups of four billion characters is read for a
    /// bounded number of them, not to its end.
    #[test]
    fn a_cmap_that_lies_about_its_size_is_read_within_a_bound() {
        let mut cmap = Vec::new();
        cmap.extend([0, 0, 0, 1]);
        // One record: Windows, full repertoire, the subtable at byte 12.
        cmap.extend([0, 3, 0, 10, 0, 0, 0, 12]);
        // Format 12, claiming 2^32 - 1 groups, the first of them every code point.
        cmap.extend([0, 12, 0, 0]);
        cmap.extend(u32::MAX.to_be_bytes());
        cmap.extend(0u32.to_be_bytes());
        cmap.extend(u32::MAX.to_be_bytes());
        cmap.extend(0u32.to_be_bytes());
        cmap.extend(u32::MAX.to_be_bytes());
        cmap.extend(1u32.to_be_bytes());
        let inverse = characters(&cmap);
        assert!(inverse.len() <= MAX_CMAP);
        assert!(!characters(&[]).contains_key(&1));
    }

    #[test]
    fn a_truncated_or_damaged_table_answers_without_panicking() {
        let (_, gsub, names) = features_of("ObelMono/ObelMono[wght].ttf");
        let cmap = cmap_of("ObelMono/ObelMono[wght].ttf");
        // The `GSUB` and `name` walks run without a `cmap` (there is nothing to read back
        // through): a byte-by-byte truncation would otherwise read the real one tens of
        // thousands of times.
        // Every length near the header, then a stride: a parse now walks the lookups too.
        let lengths = |total: usize| (0..=total).filter(move |len| *len < 512 || len % 29 == 0);
        for len in lengths(gsub.len()) {
            let _ = parse(&gsub[..len], &names, &[]);
            let _ = has_default_rules(&gsub[..len]);
        }
        for len in lengths(names.len()) {
            let _ = parse(&gsub, &names[..len], &[]);
        }
        for len in (0..=cmap.len()).step_by(7) {
            let _ = characters(&cmap[..len]);
        }
        // Damage: every word of the header and of the first records set to
        // all ones, then a deterministic scatter.
        for at in 0..64usize {
            let mut damaged = gsub.clone();
            for byte in damaged.iter_mut().skip(at).take(2) {
                *byte = 0xff;
            }
            let _ = parse(&damaged, &names, &[]);
            let _ = parse(&damaged, &names, &cmap);
        }
        let mut state = 0x2545_f491_4f6c_dd1du64;
        for round in 0..400 {
            let mut damaged = gsub.clone();
            let mut damaged_names = names.clone();
            let mut damaged_cmap = cmap.clone();
            for _ in 0..8 {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let at = (state >> 33) as usize;
                let len = damaged.len();
                damaged[at % len] = (state >> 8) as u8;
                let len = damaged_names.len();
                damaged_names[(at >> 3) % len] = (state >> 16) as u8;
                let len = damaged_cmap.len();
                damaged_cmap[(at >> 5) % len] = (state >> 24) as u8;
            }
            let _ = parse(&damaged, &damaged_names, &[]);
            let _ = characters(&damaged_cmap);
            if round % 50 == 0 {
                // Both damaged at once, through the whole of `parse`.
                let _ = parse(&damaged, &damaged_names, &damaged_cmap);
            }
        }
        assert!(parse(&[], &[], &[]).is_empty());
        assert!(!has_default_rules(&[]));
    }

    #[test]
    fn a_table_that_is_not_a_table_lists_nothing() {
        assert!(parse(&[0xff; 64], &[0xff; 64], &[0xff; 64]).is_empty());
        assert!(parse(&[0; 64], &[0; 64], &[0; 64]).is_empty());
    }

    #[cfg(target_os = "macos")]
    mod backend {
        use super::*;
        use crate::rules::{Face, Faces};
        use crate::system::fixture;

        #[test]
        fn a_registered_font_is_listed_by_its_family() {
            fixture::register_test_fonts();
            let features = font_features(Some(fixture::OBEL_MONO));
            assert_eq!(
                named(&features, "ss01").and_then(|f| f.name.as_deref()),
                Some("Mask secrets")
            );
            // Menlo has no `GSUB`; a family that is not installed has no
            // font to ask.
            assert!(font_features(Some(fixture::DEFAULT_FAMILY)).is_empty());
            assert!(font_features(Some("No Such Family 1234")).is_empty());
        }

        /// The last glyph of a masked key, shaped in `face`, against what the
        /// character maps to without any rule.
        fn masked(faces: &Faces, face: Face, text: &str) -> bool {
            let font = faces.get(face);
            let shaped = Backend::shape_line(font, text).expect("a shaped line");
            let (glyph, _) = *shaped
                .runs
                .last()
                .and_then(|run| run.glyphs.last())
                .expect("a glyph");
            let last = text.chars().last().expect("a character");
            Backend::glyph(font, last) != Some(glyph)
        }

        #[test]
        fn features_reach_the_derived_faces() {
            fixture::register_test_fonts();
            let key = "OBEL_TOKEN=masked-when-on";
            let on = [("ss01".to_owned(), true)];
            // CoreText does not flag Obel Mono monospaced (its symbolic
            // traits carry no such bit although every advance is equal), so
            // the chain's issue is not asserted here.
            let (faces, _) = Faces::from_chain(Some(fixture::OBEL_MONO), 26.0, &on);
            // The bold face is the one a derived copy would have stripped:
            // the witness is that it was derived first and given the
            // feature after.
            assert_eq!(
                faces.effective(Face::Bold),
                Face::Bold,
                "bold was not derived"
            );
            for face in [Face::Regular, Face::Bold, Face::Italic, Face::BoldItalic] {
                assert!(masked(&faces, face, key), "{face:?} draws the key open");
            }
            let (plain, _) = Faces::from_chain(Some(fixture::OBEL_MONO), 26.0, &[]);
            for face in [Face::Regular, Face::Bold] {
                assert!(!masked(&plain, face, key), "{face:?} masks without asking");
            }
        }

        #[test]
        fn a_tag_the_font_lacks_is_reported_and_one_it_has_is_not() {
            use crate::{Atlas, Spacing};
            fixture::register_test_fonts();
            let spacing = Spacing::default();
            let tags = |family: Option<&str>, chosen: &[(&str, bool)]| {
                let chosen: Vec<(String, bool)> = chosen
                    .iter()
                    .map(|&(tag, on)| (tag.to_owned(), on))
                    .collect();
                Atlas::with_features(family, 13.0, 1.0, spacing, &chosen, false)
                    .missing_features()
                    .to_vec()
            };
            let obel = Some(fixture::OBEL_MONO);
            assert!(tags(obel, &[("ss01", true), ("cv02", false)]).is_empty());
            assert_eq!(
                tags(obel, &[("ss99", true), ("ss01", true), ("zzzz", false)]),
                ["ss99", "zzzz"]
            );
            assert!(tags(obel, &[]).is_empty());
            // A font with no `GSUB` lists nothing, so any choice is one it
            // lacks; the family that is not installed is the chain's font
            // opened instead and the choice was not made for it.
            assert_eq!(
                tags(Some(fixture::DEFAULT_FAMILY), &[("calt", true)]),
                ["calt"]
            );
            assert!(tags(Some("No Such Family 1234"), &[("calt", true)]).is_empty());
            let atlas = Atlas::with_features(
                obel,
                13.0,
                1.0,
                spacing,
                &[("ss99".to_owned(), true)],
                false,
            );
            assert_eq!(atlas.family(), obel);
        }

        #[test]
        fn choices_made_for_a_family_that_was_not_found_do_not_reach_the_font_that_replaced_it() {
            // Every feature of the default font, written for a family that is not
            // installed: the chain's font opens as if nothing were chosen.
            let all: Vec<(String, bool)> = font_features(None)
                .into_iter()
                .map(|feature| (feature.tag, true))
                .collect();
            let (asked, issue) = Faces::from_chain(Some("No Such Family 1234"), 26.0, &all);
            assert!(matches!(
                issue,
                Some(crate::FontIssue::FamilyNotFound { .. })
            ));
            let (plain, _) = Faces::from_chain(None, 26.0, &[]);
            assert_eq!(asked.shaping(), plain.shaping());
            assert!(asked.lacking().is_empty());
        }

        #[test]
        fn the_atlas_is_rebuilt_when_the_features_change() {
            use crate::{Atlas, Spacing};
            let spacing = Spacing::default();
            let on = [("ss01".to_owned(), true)];
            let mut atlas = Atlas::new(None, 13.0, 1.0, spacing);
            // The same request, with or without the empty list, is the same
            // key: the atlas is not rebuilt for nothing.
            assert!(!atlas.ensure(None, 13.0, 1.0, spacing));
            assert!(!atlas.ensure_with(None, 13.0, 1.0, spacing, &[], false));
            assert!(atlas.ensure_with(None, 13.0, 1.0, spacing, &on, false));
            assert!(!atlas.ensure_with(None, 13.0, 1.0, spacing, &on, false));
            assert!(atlas.ensure_with(None, 13.0, 1.0, spacing, &[], false));
        }
    }
}
