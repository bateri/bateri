//! The FreeType + fontconfig backend of [`FontSystem`] — the Linux font
//! system, and the only module of this crate that sees either library
//! (`.tasks/042-font-sistemi-linux/discussion.md` → Karar 5, 6).
//!
//! fontconfig **chooses** (the chain, the faces, the settings list, the
//! cascade) and FreeType **measures and draws**. A font's file is read into
//! memory once and every face of it — every size, every copy — is opened from
//! that one buffer (`new_memory_face`), so the bytes the shaper will read
//! (phase-5) are the bytes that draw.
//!
//! Like CoreText, fontconfig **does not fail** on a missing family: it hands
//! back the closest font. The requested-family check stays platformless
//! (`rules::open_chain`), this backend only reports the family it got.
//!
//! **Phase-4 stubs, by name:** [`FreeType::draw_color`] and
//! [`FreeType::shape`] are not written yet (phase-5). A colour candidate
//! passes the gate and then falls to the box (the drawing answers
//! `NoContext`), a grapheme cluster falls back to its base character.

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::sync::OnceLock;

use ::freetype::face::{LoadFlag, StyleFlag};
use ::freetype::{Library, RenderMode, ffi};
use fontconfig::{
    FC_CHARSET, FC_DUAL, FC_FAMILY, FC_MONO, FC_SLANT, FC_SLANT_ITALIC, FC_SPACING, FC_WEIGHT,
    FC_WEIGHT_BOLD, FontSet, Fontconfig, ObjectSet, Pattern, UnicodeCoverage,
};

use crate::raster::DrawResult;
use crate::rules::{Face, InkRect, Metrics, RawMetrics};
use crate::system::FontSystem;

/// fontconfig's alias for the monospaced font the user's configuration
/// chose — the default chain's only link (Karar 3.1, 6). An alias has no name
/// to check, so the returned family is reported as is.
const MONOSPACE: &str = "monospace";

/// FreeType and fontconfig behind [`FontSystem`].
pub(crate) struct FreeType;

/// An opened font at one size.
///
/// `face` is `None` only when fontconfig found no font at all or the file
/// could not be opened: a machine without fonts gets tofu, not a panic on the
/// main thread. Every primitive answers that case with a neutral value.
#[derive(Clone)]
pub(crate) struct FtFont {
    face: Option<::freetype::Face>,
    source: Rc<Source>,
    /// The pixel size **as FreeType applies it** (26.6), so the size
    /// [`FreeType::size`] reports is the one the measurements come from.
    size: f64,
    /// The cascade's sorted candidates — sorted once, on the first fallback,
    /// and shared by every face, size and copy of the chain it came from
    /// (Karar 6: `FcFontSort` once per atlas).
    fallbacks: Rc<OnceCell<Option<FontSet<'static>>>>,
}

/// Where a face comes from: the file's bytes (read once per file) and the
/// face's index in it.
struct Source {
    bytes: Rc<Vec<u8>>,
    index: isize,
    family: String,
}

thread_local! {
    /// One FreeType library per thread: a library is not thread-safe, and an
    /// atlas never leaves the thread it was built on (`Atlas` is not `Send`).
    /// Faces hold a reference to the library, so it outlives them.
    static LIBRARY: Option<Library> = Library::init().ok();

    /// Font files already read, by path — the "bytes once per font" rule.
    /// Held **weakly**: the faces own the bytes, so a file stays shared while
    /// any face of it lives and is freed with the last one — a strong cache
    /// would keep every probed family (the settings list opens all of them)
    /// and every cascade candidate in memory for the thread's lifetime.
    static BYTES: RefCell<HashMap<PathBuf, Weak<Vec<u8>>>> = RefCell::new(HashMap::new());
}

/// fontconfig's handle, initialised once per process. `None` if the library
/// cannot load its configuration; then every font is the empty font.
fn fc() -> Option<&'static Fontconfig> {
    static FC: OnceLock<Option<Fontconfig>> = OnceLock::new();
    FC.get_or_init(Fontconfig::new).as_ref()
}

/// A font fontconfig matched: file, face index and family.
struct Matched {
    path: PathBuf,
    index: isize,
    family: String,
}

impl Matched {
    fn from_pattern(pattern: &Pattern<'_>) -> Option<Self> {
        Some(Matched {
            path: PathBuf::from(pattern.filename().ok()?),
            index: pattern.face_index().unwrap_or(0) as isize,
            family: pattern.get_string(FC_FAMILY).ok()?.to_owned(),
        })
    }
}

/// Asks fontconfig for the best match of `family` with the given weight and
/// slant (`None` leaves fontconfig's default: regular, roman).
fn find(family: &str, weight: Option<i32>, slant: Option<i32>) -> Option<Matched> {
    let fc = fc()?;
    let mut pattern = Pattern::new(fc).ok()?;
    let family = CString::new(family).ok()?;
    pattern.add_string(FC_FAMILY, &family).ok()?;
    if let Some(weight) = weight {
        pattern.add_integer(FC_WEIGHT, weight).ok()?;
    }
    if let Some(slant) = slant {
        pattern.add_integer(FC_SLANT, slant).ok()?;
    }
    let matched = pattern.font_match().ok()?;
    Matched::from_pattern(&matched)
}

/// The file's bytes, read on first use and shared afterwards.
fn bytes(path: &Path) -> Option<Rc<Vec<u8>>> {
    BYTES.with(|cache| {
        if let Some(bytes) = cache.borrow().get(path).and_then(Weak::upgrade) {
            return Some(bytes);
        }
        let bytes = Rc::new(std::fs::read(path).ok()?);
        let mut cache = cache.borrow_mut();
        // Drop the entries whose faces are all gone before adding one.
        cache.retain(|_, weak| weak.strong_count() > 0);
        cache.insert(path.to_owned(), Rc::downgrade(&bytes));
        Some(bytes)
    })
}

/// The pixel size FreeType will apply: 26.6 fixed point, at least 1/64.
fn quantise(size: f64) -> f64 {
    let size = if size.is_finite() { size } else { 0.0 };
    (size * 64.0).round().max(1.0) / 64.0
}

/// Opens a face of `source` at `size` pixels (72 dpi, so points are pixels).
fn open_face(source: &Source, size: f64) -> Option<::freetype::Face> {
    let face = LIBRARY.with(|library| {
        library
            .as_ref()?
            .new_memory_face(Rc::clone(&source.bytes), source.index)
            .ok()
    })?;
    face.set_char_size((size * 64.0) as isize, 0, 72, 72).ok()?;
    Some(face)
}

impl FtFont {
    /// Opens the matched font at `size`; the empty font if the file cannot
    /// be read or opened.
    fn open(
        matched: Option<Matched>,
        size: f64,
        fallbacks: Rc<OnceCell<Option<FontSet<'static>>>>,
    ) -> Self {
        let size = quantise(size);
        let source = matched.and_then(|m| {
            Some(Source {
                bytes: bytes(&m.path)?,
                index: m.index,
                family: m.family,
            })
        });
        let Some(source) = source else {
            return FtFont {
                face: None,
                source: Rc::new(Source {
                    bytes: Rc::new(Vec::new()),
                    index: 0,
                    family: String::new(),
                }),
                size,
                fallbacks,
            };
        };
        FtFont {
            face: open_face(&source, size),
            source: Rc::new(source),
            size,
            fallbacks,
        }
    }

    /// Design units to pixels at this font's size.
    fn scale(&self) -> f64 {
        match &self.face {
            Some(face) if face.em_size() > 0 => self.size / f64::from(face.em_size()),
            _ => 0.0,
        }
    }

    /// Loads `glyph` into the face's slot, unhinted: the outline if there is
    /// one, the embedded bitmap otherwise.
    fn load(&self, glyph: u32) -> Option<&::freetype::Face> {
        let face = self.face.as_ref()?;
        let outline = LoadFlag::NO_HINTING | LoadFlag::NO_BITMAP;
        if face.load_glyph(glyph, outline).is_ok() {
            return Some(face);
        }
        face.load_glyph(glyph, LoadFlag::NO_HINTING).ok()?;
        Some(face)
    }
}

/// The ink of a glyph that cannot be loaded: nothing painted.
const NO_INK: InkRect = InkRect {
    x: 0.0,
    y: 0.0,
    width: 0.0,
    height: 0.0,
};

/// Does the pattern's charset carry every code point of `text`.
fn covers(pattern: &Pattern<'_>, text: &str) -> bool {
    let mut charset = std::ptr::null_mut();
    // SAFETY: the pattern is alive for the call; `charset` is an
    // out-parameter that **borrows** the pattern's own charset (it is not
    // freed here) and is read only below, while the pattern still lives.
    unsafe {
        let found = fontconfig_sys::FcPatternGetCharSet(
            pattern.as_ptr().cast_mut(),
            FC_CHARSET.as_ptr(),
            0,
            &mut charset,
        );
        found == fontconfig_sys::FcResultMatch
            && !charset.is_null()
            && text
                .chars()
                .all(|ch| fontconfig_sys::FcCharSetHasChar(charset, u32::from(ch)) != 0)
    }
}

/// The ink box of the glyph loaded in `face`'s slot, pixels, `y` up.
fn slot_ink(face: &::freetype::Face) -> InkRect {
    let slot = face.glyph();
    if slot.outline().is_some() {
        let mut bbox = ffi::FT_BBox {
            xMin: 0,
            yMin: 0,
            xMax: 0,
            yMax: 0,
        };
        // SAFETY: the slot holds an outline (checked above) that lives until
        // the next load on this face; `bbox` is a valid out-parameter.
        let error = unsafe { ffi::FT_Outline_Get_BBox(&slot.raw().outline, &mut bbox) };
        if error != 0 {
            return NO_INK;
        }
        let px = |v: ffi::FT_Pos| v as f64 / 64.0;
        return InkRect {
            x: px(bbox.xMin),
            y: px(bbox.yMin),
            width: px(bbox.xMax - bbox.xMin),
            height: px(bbox.yMax - bbox.yMin),
        };
    }
    // A bitmap glyph: its pixel box is its ink box.
    let bitmap = slot.bitmap();
    let (width, rows) = (f64::from(bitmap.width()), f64::from(bitmap.rows()));
    let top = f64::from(slot.bitmap_top());
    InkRect {
        x: f64::from(slot.bitmap_left()),
        y: top - rows,
        width,
        height: rows,
    }
}

impl FontSystem for FreeType {
    type Font = FtFont;

    fn open(name: &str, size: f64) -> (Self::Font, String) {
        let matched = find(name, None, None);
        let family = matched
            .as_ref()
            .map(|m| m.family.clone())
            .unwrap_or_default();
        let font = FtFont::open(matched, size, Rc::default());
        (font, family)
    }

    /// The `monospace` alias: whatever the user's fontconfig configuration
    /// resolves it to, reported by the family it resolved to — an alias has
    /// no name of its own to compare, so there is no substitution warning
    /// here (Karar 3.1).
    fn open_default(size: f64) -> (Self::Font, String) {
        let (font, family) = Self::open(MONOSPACE, size);
        if font.face.is_none() {
            // Process output, not a UI string; the repository's stderr prefix.
            eprintln!("bateri: fontconfig found no '{MONOSPACE}' font, drawing boxes only");
        }
        (font, family)
    }

    /// fontconfig is asked for the same family at the face's weight and
    /// slant, and the answer is checked against the **font file's own**
    /// style bits, not the match pattern: fontconfig's synthetic rules
    /// (`90-synthetic.conf`) write `slant=oblique` into the match of a roman
    /// file when italic is requested, expecting the renderer to slant it. We
    /// do not synthesise, so that answer would be a silent substitution — the
    /// same failure CoreText's trait check guards against.
    fn derive(regular: &Self::Font, face: Face) -> Option<Self::Font> {
        let (bold, italic) = match face {
            // audit: unreachable, guarded by the module boundary — the only
            // caller, `rules::Faces::derive`, iterates over the three derived
            // faces (the same note as the CoreText backend's `face_traits`).
            Face::Regular => unreachable!("regular face is not derived, it comes from the chain"),
            Face::Bold => (true, false),
            Face::Italic => (false, true),
            Face::BoldItalic => (true, true),
        };
        let family = &regular.source.family;
        let matched = find(
            family,
            bold.then_some(FC_WEIGHT_BOLD),
            italic.then_some(FC_SLANT_ITALIC),
        )?;
        if &matched.family != family {
            return None;
        }
        let font = FtFont::open(Some(matched), regular.size, Rc::clone(&regular.fallbacks));
        let flags = font.face.as_ref()?.style_flags();
        let acquired = (!bold || flags.contains(StyleFlag::BOLD))
            && (!italic || flags.contains(StyleFlag::ITALIC));
        acquired.then_some(font)
    }

    /// The file's own fixed-width bit (`FT_IS_FIXED_WIDTH`) — CoreText's
    /// `TraitMonoSpace` counterpart (Karar 6).
    fn is_monospaced(font: &Self::Font) -> bool {
        font.face.as_ref().is_some_and(|face| face.is_fixed_width())
    }

    /// The families fontconfig lists as `spacing = mono` **or** `dual` — a
    /// pre-filter; the last word is the platformless filter. `dual` is the
    /// spacing fontconfig gives a fixed-pitch font with full-width glyphs
    /// (the CJK monospaced families), so `mono` alone would drop them.
    fn families() -> Vec<String> {
        let Some(fc) = fc() else {
            return Vec::new();
        };
        let mut names = Vec::new();
        for spacing in [FC_MONO, FC_DUAL] {
            let (Ok(mut pattern), Ok(mut objects)) = (Pattern::new(fc), ObjectSet::new(fc)) else {
                continue;
            };
            if pattern.add_integer(FC_SPACING, spacing).is_err() || objects.add(FC_FAMILY).is_err()
            {
                continue;
            }
            let Ok(set) = fontconfig::list_fonts(&pattern, Some(&objects)) else {
                continue;
            };
            names.extend(
                set.iter()
                    .filter_map(|p| p.get_string(FC_FAMILY).ok().map(str::to_owned)),
            );
        }
        names
    }

    fn glyph(font: &Self::Font, ch: char) -> Option<u32> {
        font.face.as_ref()?.get_char_index(ch as usize)
    }

    /// The **linear** (unhinted) advance: design units times the size's
    /// scale, 16.16 — fractional like CoreText's, not rounded to a pixel.
    fn advance(font: &Self::Font, glyph: u32) -> f64 {
        let Some(face) = font.load(glyph) else {
            return 0.0;
        };
        let slot = face.glyph();
        if slot.outline().is_some() {
            slot.linear_hori_advance() as f64 / 65536.0
        } else {
            slot.advance().x as f64 / 64.0
        }
    }

    /// The outline's **exact** bounds (`FT_Outline_Get_BBox`), not its
    /// control box: the gate measures the pixels that will be painted.
    fn ink(font: &Self::Font, glyph: u32) -> InkRect {
        font.load(glyph).map(slot_ink).unwrap_or(NO_INK)
    }

    /// Scaled from design units (the `hhea` values FreeType reports), not
    /// from FreeType's rounded `size->metrics` — fractional like CoreText's
    /// (Karar 6). The x-height is the OS/2 table's, else the top of `x`.
    fn raw_metrics(font: &Self::Font) -> RawMetrics {
        let Some(face) = &font.face else {
            return RawMetrics {
                ascent: 0.0,
                descent: 0.0,
                leading: 0.0,
                underline_position: 0.0,
                underline_thickness: 0.0,
                x_height: 0.0,
            };
        };
        let scale = font.scale();
        let units = |v: i16| f64::from(v) * scale;
        let (ascender, descender) = (face.ascender(), face.descender());
        let line_gap = i32::from(face.height()) - (i32::from(ascender) - i32::from(descender));
        // SAFETY: the face is alive; the table pointer, if not null, points
        // into the face and is read before any other call on it.
        let os2_x_height = unsafe {
            let table =
                ffi::FT_Get_Sfnt_Table(face.raw() as *const _ as ffi::FT_Face, ffi::ft_sfnt_os2)
                    as *const ffi::TT_OS2;
            (!table.is_null() && (*table).version != 0xffff && (*table).version >= 2)
                .then(|| (*table).sxHeight)
                .filter(|&h| h > 0)
        };
        let x_height = match os2_x_height {
            Some(h) => units(h),
            None => Self::glyph(font, 'x')
                .map(|g| {
                    let ink = Self::ink(font, g);
                    ink.y + ink.height
                })
                .unwrap_or(0.0),
        };
        RawMetrics {
            ascent: units(ascender),
            descent: -units(descender),
            leading: f64::from(line_gap.max(0)) * scale,
            // FreeType keeps the sign the crate expects: negative, below the
            // baseline.
            underline_position: units(face.underline_position()),
            underline_thickness: units(face.underline_thickness()),
            x_height,
        }
    }

    /// `FcFontSort` on the chain's family, once (Karar 6), then the first
    /// font in that order whose charset carries every code point of `text`.
    /// `None` when no installed font covers it — fontconfig has no
    /// last-resort font, so "no candidate" arises here.
    fn cascade(base: &Self::Font, text: &str) -> Option<Self::Font> {
        let sorted = base.fallbacks.get_or_init(|| {
            let fc = fc()?;
            let mut pattern = Pattern::new(fc).ok()?;
            let family = CString::new(base.source.family.as_str()).ok()?;
            pattern.add_string(FC_FAMILY, &family).ok()?;
            pattern.sort_fonts(UnicodeCoverage::Trim).ok()
        });
        let matched = sorted
            .as_ref()?
            .iter()
            .filter(|pattern| covers(pattern, text))
            .find_map(|pattern| Matched::from_pattern(&pattern))?;
        Some(FtFont::open(
            Some(matched),
            base.size,
            Rc::clone(&base.fallbacks),
        ))
    }

    fn at_size(font: &Self::Font, size: f64) -> Self::Font {
        let size = quantise(size);
        FtFont {
            face: open_face(&font.source, size),
            source: Rc::clone(&font.source),
            size,
            fallbacks: Rc::clone(&font.fallbacks),
        }
    }

    fn size(font: &Self::Font) -> f64 {
        font.size
    }

    /// Always `false`: fontconfig has no last-resort font whose glyph is a
    /// representative box; a character nobody covers has **no** candidate
    /// ([`FreeType::cascade`]).
    fn is_last_resort(_font: &Self::Font) -> bool {
        false
    }

    fn has_color_glyphs(font: &Self::Font) -> bool {
        font.face.as_ref().is_some_and(|face| face.has_color())
    }

    /// **Stub (phase-5):** no shaping yet, so every cluster falls back to its
    /// base character.
    fn shape(_base: &Self::Font, _text: &str) -> Option<(Self::Font, u32)> {
        None
    }

    /// Unhinted outline, grey anti-aliasing, no LCD. The fractional part of
    /// the position is applied to the **outline** in 26.6
    /// (`FT_Outline_Translate`), the whole part to the blit — the cell
    /// advance is fractional and the centring gives a fractional `x`; a
    /// hinted or pixel-snapped glyph would stand somewhere other than where
    /// the gate measured it (Karar 6).
    fn draw_mask(
        font: &Self::Font,
        glyph: u32,
        m: Metrics,
        x: f64,
        baseline: f64,
        target: &mut [u8],
    ) -> DrawResult {
        assert_eq!(target.len(), m.slot_bytes(), "buffer must be a full slot");
        let Some(face) = font.face.as_ref() else {
            return DrawResult::NoContext;
        };
        if face
            .load_glyph(glyph, LoadFlag::NO_HINTING | LoadFlag::NO_BITMAP)
            .is_err()
        {
            return DrawResult::NoContext;
        }
        let slot = face.glyph();
        if slot.outline().is_none() {
            return DrawResult::NoContext;
        }
        let (w, h) = m.cell_wh();
        // `baseline` is measured from the slot's **bottom** (the trait's
        // convention); rows run from the top.
        let from_top = h as f64 - baseline;
        let (x0, y0) = (x.floor(), from_top.floor());
        let dx = ((x - x0) * 64.0).round() as ffi::FT_Pos;
        // FreeType's y grows upwards: a baseline lower than the whole row
        // moves the outline down.
        let dy = -((from_top - y0) * 64.0).round() as ffi::FT_Pos;
        // SAFETY: the slot holds an outline (checked above) owned by the
        // face's glyph slot; translating it in place is what the API is for.
        unsafe { ffi::FT_Outline_Translate(&slot.raw().outline, dx, dy) };
        if slot.render_glyph(RenderMode::Normal).is_err() {
            return DrawResult::NoContext;
        }
        target.fill(0);
        let bitmap = slot.bitmap();
        let pitch = bitmap.pitch().unsigned_abs() as usize;
        let buffer = bitmap.buffer();
        let (left, top) = (
            x0 as i64 + i64::from(slot.bitmap_left()),
            y0 as i64 - i64::from(slot.bitmap_top()),
        );
        for row in 0..bitmap.rows().max(0) as usize {
            let ty = top + row as i64;
            if ty < 0 || ty >= h as i64 {
                continue;
            }
            for col in 0..bitmap.width().max(0) as usize {
                let tx = left + col as i64;
                if tx < 0 || tx >= w as i64 {
                    continue;
                }
                target[ty as usize * w + tx as usize] = buffer[row * pitch + col];
            }
        }
        DrawResult::Drawn
    }

    /// **Stub (phase-5):** colour glyphs are not drawn yet; the answer puts
    /// the candidate in the box.
    fn draw_color(
        _font: &Self::Font,
        _glyph: u32,
        m: Metrics,
        _x: f64,
        _baseline: f64,
        target: &mut [u8],
    ) -> DrawResult {
        assert_eq!(
            target.len(),
            m.slot_bytes_rgba(),
            "buffer must be a full RGBA slot"
        );
        DrawResult::NoContext
    }
}

/// Sample characters and family names for the platformless tests
/// (`.tasks/042-font-sistemi-linux/discussion.md` → Karar 7), measured on
/// the `make linux` image's fonts (`fonts-dejavu-core`).
#[cfg(test)]
pub(crate) mod fixture {
    use super::FtFont;

    /// The family `monospace` resolves to in the image.
    pub(crate) const DEFAULT_FAMILY: &str = "DejaVu Sans Mono";

    /// An installed family that is **not** monospaced.
    pub(crate) const PROPORTIONAL_FAMILY: &str = "DejaVu Sans";

    /// A second family, not the default one. The image has a single
    /// monospaced family, so this one is not monospaced; its one consumer
    /// (`ensure_rebuilds_when_family_changes`) asks only for the rebuild.
    pub(crate) const SECOND_FAMILY: &str = "DejaVu Serif";

    /// A character no installed font covers: the cascade has **no**
    /// candidate (there is no last-resort font), so the answer is tofu
    /// without the gate being asked.
    pub(crate) const UNKNOWN_CHAR: char = '\u{10FFFC}';

    /// A character rejected in one cell but accepted in **two** (a pair of
    /// slots for a wide request): `⁂` comes from DejaVu Sans with ink
    /// 0.05..1.61 cells (measured, 13pt@1x). Unlike the CoreText fixture's
    /// `.LastResort` box, a single-cell request for it is **shrunk**, not
    /// rejected: every glyph that fits two cells fits one at half the size,
    /// under `SHRINK_LIMIT`.
    pub(crate) const WIDE_CHAR: char = '\u{2042}';

    /// A character the fallback **accepts**: `⁊` is not in DejaVu Sans Mono
    /// and comes from DejaVu Sans, advancing 0.83 of the cell with ink
    /// 0.08..0.75 — narrower than the cell, so centring really moves it.
    pub(crate) const FALLBACK_CHAR: char = '\u{204A}';

    /// Advance wider than the cell, ink inside it: `∖` from DejaVu Sans
    /// advances 1.058 cells and paints 0.32..0.88 (measured).
    pub(crate) const INK_CHAR: char = '\u{2216}';

    /// Characters that exercise the gate's **rule**; none is in DejaVu Sans
    /// Mono. `⟹` paints 2.3 cells, beyond `SHRINK_LIMIT` — the box side of
    /// the experiment; `⁂` is shrunk; the private-use ones have no candidate.
    /// The expectation is derived from each candidate's own ink by the test.
    pub(crate) const GATE_PROBES: [char; 7] = [
        FALLBACK_CHAR,
        INK_CHAR,
        UNKNOWN_CHAR,
        '\u{27F9}',
        WIDE_CHAR,
        '\u{10FFFD}',
        '⠋',
    ];

    /// Symbols beyond ASCII whose advance must equal the cell's: all of them,
    /// the two combining marks included, advance exactly one cell in DejaVu
    /// Sans Mono (measured).
    pub(crate) const BASE_SYMBOLS: &str = "─│┌┐└┘├┤┬┴┼✓⚠▶\u{0300}\u{0301}";

    /// Grapheme clusters that shape into a single colour glyph; the same
    /// sequences as the CoreText fixture. Their tests wait for phase-5 (no
    /// shaping, no colour font in the image yet).
    pub(crate) const CLUSTERS: [&str; 5] = [
        "\u{1F1F9}\u{1F1F7}",
        "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}",
        "\u{1F44D}\u{1F3FD}",
        "\u{2764}\u{FE0F}",
        "\u{1F321}\u{FE0F}",
    ];

    /// An emoji whose doubled string does not shape into one glyph.
    pub(crate) const CLUSTER_BASE: char = '\u{1F44D}';

    /// The display scale of the cluster tests.
    pub(crate) const CLUSTER_SCALE: f64 = 2.0;

    /// The family name a font reports — diagnostics in test messages only.
    pub(crate) fn family_name(font: &FtFont) -> String {
        font.source.family.clone()
    }
}
