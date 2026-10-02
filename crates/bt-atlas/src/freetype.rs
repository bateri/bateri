//! The FreeType + fontconfig backend of [`FontSystem`] — the Linux font
//! system, and the only module of this crate that sees either library
//! (`.tasks/042-font-sistemi-linux/discussion.md` → Karar 5, 6).
//!
//! fontconfig **chooses** (the chain, the faces, the settings list, the
//! cascade) and FreeType **measures and draws**. A font's file is read into
//! memory once and every face of it — every size, every copy — is opened from
//! that one buffer (`new_memory_face`), and the shaper (`harfrust`) reads the
//! same buffer, so the glyph number a cluster shapes to is the drawing font's.
//!
//! Like CoreText, fontconfig **does not fail** on a missing family: it hands
//! back the closest font. The requested-family check stays platformless
//! (`rules::open_chain`), this backend only reports the family it got.
//!
//! **Colour glyphs are bitmaps** (`CBDT`, Noto Color Emoji): a bitmap-only
//! face is opened at its nearest strike and its one **scale factor**
//! (`FtFont::bitmap_scale`) is the single owner of the measured advance, the
//! measured ink and the drawn size — the gate and the drawing read the same
//! number, or `centre_shift`'s two consumers would disagree (Karar 6).

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::sync::OnceLock;

use ::freetype::bitmap::PixelMode;
use ::freetype::face::{LoadFlag, StyleFlag};
use ::freetype::{Library, RenderMode, ffi};
use fontconfig::{
    FC_CHARSET, FC_COLOR, FC_DUAL, FC_FAMILY, FC_MONO, FC_SLANT, FC_SLANT_ITALIC, FC_SPACING,
    FC_WEIGHT, FC_WEIGHT_BOLD, FontSet, Fontconfig, ObjectSet, Pattern, UnicodeCoverage,
};
use harfrust::{BufferFlags, FontRef, ShapeOptions, ShaperData, UnicodeBuffer};

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
    /// The selected strike's pixels per em when the face is bitmap-only
    /// (`CBDT`); `None` for a scalable face. See [`FtFont::bitmap_scale`].
    strike: Option<f64>,
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
///
/// A bitmap-only face (`CBDT` colour emoji) has no size of its own to set —
/// `FT_Set_Char_Size` fails unless a strike matches exactly — so it is opened
/// at a **strike** and the strike's pixels per em come back with it; the
/// glyphs are scaled to `size` by [`FtFont::bitmap_scale`]. The strike is the
/// smallest one not smaller than `size` (the drawing then only shrinks), else
/// the largest.
fn open_face(source: &Source, size: f64) -> Option<(::freetype::Face, Option<f64>)> {
    let face = LIBRARY.with(|library| {
        library
            .as_ref()?
            .new_memory_face(Rc::clone(&source.bytes), source.index)
            .ok()
    })?;
    if face.is_scalable() {
        face.set_char_size((size * 64.0) as isize, 0, 72, 72).ok()?;
        return Some((face, None));
    }
    let raw = face.raw();
    let count = usize::try_from(raw.num_fixed_sizes).ok()?;
    if count == 0 || raw.available_sizes.is_null() {
        return None;
    }
    // SAFETY: FreeType owns `available_sizes`, an array of `num_fixed_sizes`
    // records (both checked above) that lives as long as the face; the slice
    // is read before any other call on it.
    let strikes = unsafe { std::slice::from_raw_parts(raw.available_sizes, count) };
    let ppem = |i: usize| strikes[i].y_ppem as f64 / 64.0;
    let index = (0..count)
        .filter(|&i| ppem(i) >= size)
        .min_by(|&a, &b| ppem(a).total_cmp(&ppem(b)))
        .or_else(|| (0..count).max_by(|&a, &b| ppem(a).total_cmp(&ppem(b))))?;
    face.select_size(i32::try_from(index).ok()?).ok()?;
    let strike = ppem(index);
    (strike > 0.0).then_some((face, Some(strike)))
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
                strike: None,
                fallbacks,
            };
        };
        let (face, strike) = open_face(&source, size).unzip();
        FtFont {
            face,
            source: Rc::new(source),
            size,
            strike: strike.flatten(),
            fallbacks,
        }
    }

    /// Strike pixels to this font's pixels: `size / strike` for a bitmap-only
    /// face, `1` for a scalable one. The **single owner** of a bitmap font's
    /// scale — [`FreeType::advance`], [`FreeType::ink`] and
    /// [`FreeType::draw_color`] all read it, so what the gate measures is
    /// what is drawn (Karar 6).
    fn bitmap_scale(&self) -> f64 {
        match self.strike {
            Some(strike) => self.size / strike,
            None => 1.0,
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
    /// one, the embedded bitmap otherwise — in colour, the same load
    /// [`FreeType::draw_color`] draws from.
    fn load(&self, glyph: u32) -> Option<&::freetype::Face> {
        let face = self.face.as_ref()?;
        let outline = LoadFlag::NO_HINTING | LoadFlag::NO_BITMAP;
        if face.load_glyph(glyph, outline).is_ok() {
            return Some(face);
        }
        face.load_glyph(glyph, LoadFlag::NO_HINTING | LoadFlag::COLOR)
            .ok()?;
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

/// Joiners and variation selectors: code points a font need not map for a
/// sequence to be its to draw. The shaper consumes them in a ligature or
/// removes them (`REMOVE_DEFAULT_IGNORABLES`), and Noto Color Emoji's charset
/// does not carry U+FE0F (measured), so demanding them would send `❤️` to a
/// text font that happens to map the selector.
fn is_joiner_or_selector(ch: char) -> bool {
    matches!(ch, '\u{200C}' | '\u{200D}' | '\u{FE00}'..='\u{FE0F}' | '\u{E0100}'..='\u{E01EF}')
}

/// Is the pattern a colour font (fontconfig's `color` property).
fn is_color(pattern: &Pattern<'_>) -> bool {
    let mut value: fontconfig_sys::FcBool = 0;
    // SAFETY: the pattern is alive for the call; `value` is a plain
    // out-parameter.
    let found = unsafe {
        fontconfig_sys::FcPatternGetBool(
            pattern.as_ptr().cast_mut(),
            FC_COLOR.as_ptr(),
            0,
            &mut value,
        )
    };
    found == fontconfig_sys::FcResultMatch && value != 0
}

/// Does the pattern's charset carry every code point of `text`, joiners and
/// variation selectors aside ([`is_joiner_or_selector`]).
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
                .filter(|&ch| !is_joiner_or_selector(ch))
                .all(|ch| fontconfig_sys::FcCharSetHasChar(charset, u32::from(ch)) != 0)
    }
}

/// The ink box of the glyph loaded in `face`'s slot, pixels, `y` up; a
/// bitmap glyph's box is scaled by `scale` ([`FtFont::bitmap_scale`]).
fn slot_ink(face: &::freetype::Face, scale: f64) -> InkRect {
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
    // A bitmap glyph: its pixel box is its ink box, in strike pixels.
    let bitmap = slot.bitmap();
    let (width, rows) = (f64::from(bitmap.width()), f64::from(bitmap.rows()));
    let top = f64::from(slot.bitmap_top());
    InkRect {
        x: f64::from(slot.bitmap_left()) * scale,
        y: (top - rows) * scale,
        width: width * scale,
        height: rows * scale,
    }
}

/// fontconfig's best match for `name`, opened at `size`, with the family it
/// resolved to (empty when nothing matched).
fn open_named(name: &str, size: f64) -> (FtFont, String) {
    let matched = find(name, None, None);
    let family = matched
        .as_ref()
        .map(|m| m.family.clone())
        .unwrap_or_default();
    (FtFont::open(matched, size, Rc::default()), family)
}

/// The cascade's walk ([`FreeType::cascade`]): with `colour_first`, the
/// colour fonts of the sorted list are tried before the rest.
fn cascade_from(base: &FtFont, text: &str, colour_first: bool) -> Option<FtFont> {
    let sorted = base.fallbacks.get_or_init(|| {
        let fc = fc()?;
        let mut pattern = Pattern::new(fc).ok()?;
        let family = CString::new(base.source.family.as_str()).ok()?;
        pattern.add_string(FC_FAMILY, &family).ok()?;
        // Not trimmed: `Trim` drops every font whose charset adds nothing to
        // the fonts before it, so a colour font whose code points a text
        // font earlier in the sort already covers (Symbola) would vanish
        // from the colour-first walk, and a font covering a whole cluster
        // would vanish when two earlier fonts cover its pieces between them.
        pattern.sort_fonts(UnicodeCoverage::NoTrim).ok()
    });
    let sorted = sorted.as_ref()?;
    let first = |colour_only: bool| {
        sorted
            .iter()
            .filter(|pattern| !colour_only || is_color(pattern))
            .filter(|pattern| covers(pattern, text))
            .find_map(|pattern| Matched::from_pattern(&pattern))
    };
    let matched = colour_first
        .then(|| first(true))
        .flatten()
        .or_else(|| first(false))?;
    Some(FtFont::open(
        Some(matched),
        base.size,
        Rc::clone(&base.fallbacks),
    ))
}

impl FontSystem for FreeType {
    type Font = FtFont;

    /// A bitmap-only **text** face (a PCF/BDF family such as `Fixed` or
    /// `Terminus`) answers with no family: the mask path draws outlines only
    /// ([`FreeType::draw_mask`]) and such a face has no `units_per_EM` for
    /// [`FreeType::raw_metrics`], so opening it would give a one-pixel-high
    /// cell full of boxes. Reported as "not found", the chain falls back to
    /// the default one and the settings list leaves it out.
    fn open(name: &str, size: f64) -> (Self::Font, String) {
        let (font, family) = open_named(name, size);
        let bitmap_text = font
            .face
            .as_ref()
            .is_some_and(|face| !face.is_scalable() && !face.has_color());
        if bitmap_text {
            return (font, String::new());
        }
        (font, family)
    }

    /// The `monospace` alias: whatever the user's fontconfig configuration
    /// resolves it to, reported by the family it resolved to — an alias has
    /// no name of its own to compare, so there is no substitution warning
    /// here (Karar 3.1).
    fn open_default(size: f64) -> (Self::Font, String) {
        let (font, family) = open_named(MONOSPACE, size);
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
            slot.advance().x as f64 / 64.0 * font.bitmap_scale()
        }
    }

    /// The outline's **exact** bounds (`FT_Outline_Get_BBox`), not its
    /// control box: the gate measures the pixels that will be painted.
    fn ink(font: &Self::Font, glyph: u32) -> InkRect {
        font.load(glyph)
            .map(|face| slot_ink(face, font.bitmap_scale()))
            .unwrap_or(NO_INK)
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
    /// font in that order whose charset carries every code point of `text`
    /// ([`covers`]). `None` when no installed font covers it — fontconfig has
    /// no last-resort font, so "no candidate" arises here.
    ///
    /// U+FE0F asks for **emoji presentation**, so a text carrying it tries
    /// the colour fonts first: the sort puts the text families ahead, and
    /// `❤️` would otherwise be drawn by the first of them that maps `❤` —
    /// in the text plane, the selector ignored. CoreText's cascade makes the
    /// same choice.
    fn cascade(base: &Self::Font, text: &str) -> Option<Self::Font> {
        cascade_from(base, text, text.contains('\u{FE0F}'))
    }

    fn at_size(font: &Self::Font, size: f64) -> Self::Font {
        let size = quantise(size);
        let (face, strike) = open_face(&font.source, size).unzip();
        FtFont {
            face,
            source: Rc::clone(&font.source),
            size,
            strike: strike.flatten(),
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

    /// `harfrust` over the **same bytes** the candidate's FreeType face was
    /// opened from, so the glyph number is the drawing font's (Karar 6). The
    /// candidate is the cascade's for the whole string; unlike `CTLine`,
    /// harfrust never substitutes another font, so the font that produces
    /// the glyph **is** the candidate — the CoreText backend's "the run's own
    /// font" rule holds by construction.
    ///
    /// Joiners and selectors that did not merge into a ligature are
    /// **removed**, not kept as zero-width glyphs: `CTLine` counts `❤️` as
    /// one glyph and so must this, or every emoji-presentation sequence
    /// would fall back to its base character.
    fn shape(base: &Self::Font, text: &str) -> Option<(Self::Font, u32)> {
        // Every cluster that reaches here is an emoji sequence (flag, ZWJ,
        // skin tone, VS16), so the colour fonts are tried first whether or
        // not the text carries U+FE0F: a text font that maps the pieces
        // (Symbola) would otherwise win the sort and never ligate them.
        let font = cascade_from(base, text, true)?;
        font.face.as_ref()?;
        // fontconfig's face index carries a named instance above bit 16; the
        // collection index is the low half.
        let index = u32::try_from(font.source.index).ok()? & 0xFFFF;
        let font_ref = FontRef::from_index(&font.source.bytes, index).ok()?;
        let data = ShaperData::new(&font_ref);
        let shaper = data.shaper(&font_ref).build();
        let mut buffer = UnicodeBuffer::new();
        buffer.push_str(text);
        buffer.guess_segment_properties();
        buffer.set_flags(BufferFlags::REMOVE_DEFAULT_IGNORABLES);
        let shaped = shaper.shape(buffer, ShapeOptions::new());
        let [info] = shaped.glyph_infos() else {
            return None;
        };
        // `.notdef` is the font's "I cannot draw this" answer, not a glyph.
        let glyph = info.glyph_id;
        (glyph != 0).then_some((font, glyph))
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

    /// FreeType's colour load (`FT_LOAD_COLOR`): a `CBDT` glyph arrives as
    /// the strike's premultiplied BGRA bitmap and is resampled to this
    /// font's size by [`FtFont::bitmap_scale`] — the factor the gate measured
    /// with — through [`resample`]. A colour **outline** (`COLR`) is rendered
    /// by FreeType at the font's size, positioned as [`FreeType::draw_mask`]
    /// positions it, and copied at scale 1. Anything that does not come out
    /// as BGRA is not drawn (`NoContext`: the caller's box).
    ///
    /// Only the channel order changes here: FreeType's BGRA is already
    /// premultiplied, and undoing that is the caller's.
    fn draw_color(
        font: &Self::Font,
        glyph: u32,
        m: Metrics,
        x: f64,
        baseline: f64,
        target: &mut [u8],
    ) -> DrawResult {
        // Not `debug_assert`: it is what catches a mask buffer passed for the
        // colour plane (the CoreText backend's reason).
        assert_eq!(
            target.len(),
            m.slot_bytes_rgba(),
            "buffer must be a full RGBA slot"
        );
        let Some(face) = font.face.as_ref() else {
            return DrawResult::NoContext;
        };
        if face
            .load_glyph(glyph, LoadFlag::NO_HINTING | LoadFlag::COLOR)
            .is_err()
        {
            return DrawResult::NoContext;
        }
        let slot = face.glyph();
        let (w, h) = m.cell_wh();
        let from_top = h as f64 - baseline;
        let (origin, scale) = if slot.outline().is_some() {
            // The outline path of `draw_mask`: fractions into the outline,
            // whole pixels into the placement.
            let (x0, y0) = (x.floor(), from_top.floor());
            let dx = ((x - x0) * 64.0).round() as ffi::FT_Pos;
            let dy = -((from_top - y0) * 64.0).round() as ffi::FT_Pos;
            // SAFETY: the slot holds an outline (checked above) owned by the
            // face's glyph slot; translating it in place is what the API is
            // for.
            unsafe { ffi::FT_Outline_Translate(&slot.raw().outline, dx, dy) };
            if slot.render_glyph(RenderMode::Normal).is_err() {
                return DrawResult::NoContext;
            }
            ((x0, y0), 1.0)
        } else {
            ((x, from_top), font.bitmap_scale())
        };
        let bitmap = slot.bitmap();
        if !matches!(bitmap.pixel_mode(), Ok(PixelMode::Bgra)) {
            return DrawResult::NoContext;
        }
        let source = Bgra {
            pixels: bitmap.buffer(),
            pitch: bitmap.pitch().unsigned_abs() as usize,
            width: bitmap.width().max(0) as usize,
            rows: bitmap.rows().max(0) as usize,
            left: f64::from(slot.bitmap_left()),
            top: f64::from(slot.bitmap_top()),
        };
        target.fill(0);
        resample(&source, origin, scale, (w, h), target);
        DrawResult::Drawn
    }
}

/// A premultiplied BGRA bitmap and where it sits: `left`/`top` are its
/// top-left corner relative to the glyph origin, source pixels, `y` up.
struct Bgra<'a> {
    pixels: &'a [u8],
    pitch: usize,
    width: usize,
    rows: usize,
    left: f64,
    top: f64,
}

/// For each target pixel along one axis, the source pixels that overlap it
/// and by how much: source pixel `i` covers `start + i·scale ..
/// start + (i+1)·scale` in target pixels. The weights of one target pixel
/// sum to its covered fraction (at most 1), so the average is an **area**
/// average and a pixel the bitmap covers half is half as opaque.
fn box_weights(start: f64, scale: f64, count: usize, extent: usize) -> Vec<Vec<(usize, f64)>> {
    (0..extent)
        .map(|t| {
            let (lo, hi) = (t as f64, t as f64 + 1.0);
            let first = ((lo - start) / scale).floor().max(0.0) as usize;
            let last = (((hi - start) / scale).ceil().max(0.0) as usize).min(count);
            (first..last)
                .filter_map(|i| {
                    let a = start + i as f64 * scale;
                    let overlap = hi.min(a + scale) - lo.max(a);
                    (overlap > 0.0).then_some((i, overlap))
                })
                .collect()
        })
        .collect()
}

/// Draws `source` into `target` (`RGBA8`, premultiplied, `w × h`) with its
/// glyph origin at `origin` (target pixels from the top-left, rows down) and
/// every source pixel `scale` target pixels wide.
///
/// A box filter in **premultiplied** space — the one resampler, no filter
/// choice (Karar 6). It is written for shrinking (Noto's strike is 109 px,
/// a cell is far smaller), and being an exact overlap integration it also
/// enlarges, blockily, for the size a strike does not reach, instead of
/// giving that glyph up to the box.
fn resample(
    source: &Bgra<'_>,
    origin: (f64, f64),
    scale: f64,
    (w, h): (usize, usize),
    target: &mut [u8],
) {
    let columns = box_weights(origin.0 + source.left * scale, scale, source.width, w);
    let rows = box_weights(origin.1 - source.top * scale, scale, source.rows, h);
    for (ty, row_weights) in rows.iter().enumerate() {
        for (tx, column_weights) in columns.iter().enumerate() {
            let mut sum = [0.0f64; 4];
            for &(sy, wy) in row_weights {
                for &(sx, wx) in column_weights {
                    let at = sy * source.pitch + sx * 4;
                    let Some(pixel) = source.pixels.get(at..at + 4) else {
                        continue;
                    };
                    let weight = wy * wx;
                    // BGRA in, RGBA out.
                    for (channel, &from) in [2, 1, 0, 3].iter().enumerate() {
                        sum[channel] += weight * f64::from(pixel[from]);
                    }
                }
            }
            let at = (ty * w + tx) * 4;
            for (channel, value) in sum.iter().enumerate() {
                target[at + channel] = value.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

/// Sample characters and family names for the platformless tests
/// (`.tasks/042-font-sistemi-linux/discussion.md` → Karar 7), measured on
/// the `make linux` image's fonts (`fonts-dejavu-core`).
#[cfg(any(test, feature = "fixture"))]
pub mod fixture {
    #[cfg(test)]
    use super::FtFont;

    /// The family `monospace` resolves to in the image.
    pub const DEFAULT_FAMILY: &str = "DejaVu Sans Mono";

    /// An installed family that is **not** monospaced.
    pub const PROPORTIONAL_FAMILY: &str = "DejaVu Sans";

    /// A second family, not the default one. The image has a single
    /// monospaced family, so this one is not monospaced; its one consumer
    /// (`ensure_rebuilds_when_family_changes`) asks only for the rebuild.
    pub const SECOND_FAMILY: &str = "DejaVu Serif";

    /// A character no installed font covers: the cascade has **no**
    /// candidate (there is no last-resort font), so the answer is tofu
    /// without the gate being asked.
    pub const UNKNOWN_CHAR: char = '\u{10FFFC}';

    /// A character rejected in one cell but accepted in **two** (a pair of
    /// slots for a wide request): `⁂` comes from DejaVu Sans with ink
    /// 0.05..1.61 cells (measured, 13pt@1x). Unlike the CoreText fixture's
    /// `.LastResort` box, a single-cell request for it is **shrunk**, not
    /// rejected: every glyph that fits two cells fits one at half the size,
    /// under `SHRINK_LIMIT`.
    pub const WIDE_CHAR: char = '\u{2042}';

    /// A character the fallback **accepts**: `⁊` is not in DejaVu Sans Mono
    /// and comes from DejaVu Sans, advancing 0.83 of the cell with ink
    /// 0.08..0.75 — narrower than the cell, so centring really moves it.
    pub const FALLBACK_CHAR: char = '\u{204A}';

    /// Advance wider than the cell, ink inside it: `∖` from DejaVu Sans
    /// advances 1.058 cells and paints 0.32..0.88 (measured).
    pub const INK_CHAR: char = '\u{2216}';

    /// Characters that exercise the gate's **rule**; none is in DejaVu Sans
    /// Mono. `⟹` paints 2.3 cells, beyond `SHRINK_LIMIT` — the box side of
    /// the experiment; `⁂` is shrunk; the private-use ones have no candidate.
    /// The expectation is derived from each candidate's own ink by the test.
    pub const GATE_PROBES: [char; 6] = [
        FALLBACK_CHAR,
        INK_CHAR,
        UNKNOWN_CHAR,
        '\u{27F9}',
        WIDE_CHAR,
        '\u{10FFFD}',
    ];

    /// Symbols beyond ASCII whose advance must equal the cell's: all of them,
    /// the two combining marks included, advance exactly one cell in DejaVu
    /// Sans Mono (measured).
    pub const BASE_SYMBOLS: &str = "─│┌┐└┘├┤┬┴┼✓⚠▶\u{0300}\u{0301}";

    /// Grapheme clusters that shape into a single colour glyph; the same
    /// sequences as the CoreText fixture, drawn from Noto Color Emoji
    /// (`fonts-noto-color-emoji`). The two selector sequences are the ones
    /// the cascade has to steer: Noto's charset has no U+FE0F and DejaVu
    /// Sans maps both `❤` and the selector (measured).
    pub const CLUSTERS: [&str; 5] = [
        "\u{1F1F9}\u{1F1F7}",
        "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}",
        "\u{1F44D}\u{1F3FD}",
        "\u{2764}\u{FE0F}",
        "\u{1F321}\u{FE0F}",
    ];

    /// An emoji whose doubled string does not shape into one glyph.
    pub const CLUSTER_BASE: char = '\u{1F44D}';

    /// The display scale of the cluster tests.
    pub const CLUSTER_SCALE: f64 = 2.0;

    /// The families the default chain may open: the image has one
    /// monospaced family.
    pub const CHAIN_FAMILIES: [&str; 1] = [DEFAULT_FAMILY];

    /// A wide character drawn as **two halves** with ink on both sides of
    /// the seam. The image has no CJK font, so it is [`WIDE_CHAR`]: `⁂`'s
    /// ink spans 1.56 cells and is centred in the two-cell box, the top
    /// asterisk on the seam. `bt-gpu`'s fan-out and typing-effect guards
    /// draw it.
    pub const PAIR_CHAR: char = WIDE_CHAR;

    /// A pair whose ink is horizontal strokes crossing the seam, for
    /// `bt-gpu`'s seam guard (no CJK `一` in the image): `⟺` from DejaVu
    /// Sans draws across both cells and its two shafts meet the seam with
    /// equal columns on either side (measured at 13pt@2x). `⁂` would not
    /// do: its asterisk sits on the seam and differs column to column.
    pub const STROKE_PAIR_CHAR: char = '\u{27FA}';

    /// Two columns in Unicode but its ink fits one cell: DejaVu Sans Mono's
    /// own `☕` (a wide request answers `Whole`).
    pub const ONE_CELL_WIDE_CHAR: char = '\u{2615}';

    /// The family name a font reports — diagnostics in test messages only.
    #[cfg(test)]
    pub(crate) fn family_name(font: &FtFont) -> String {
        font.source.family.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::fixture::{CLUSTER_BASE, CLUSTERS, DEFAULT_FAMILY};

    /// A 2×2 opaque-red BGRA bitmap whose top-left corner is the origin.
    fn red_square() -> Vec<u8> {
        [0u8, 0, 255, 255].repeat(4)
    }

    /// At scale 1 and a whole-pixel origin the resampler is a copy: every
    /// source pixel lands on exactly one target pixel, channels reordered.
    #[test]
    fn resampling_at_scale_one_copies() {
        let pixels = red_square();
        let source = Bgra {
            pixels: &pixels,
            pitch: 8,
            width: 2,
            rows: 2,
            left: 0.0,
            top: 2.0,
        };
        let mut target = vec![0u8; 4 * 4 * 4];
        resample(&source, (1.0, 3.0), 1.0, (4, 4), &mut target);
        for ty in 0..4 {
            for tx in 0..4 {
                let at = (ty * 4 + tx) * 4;
                let inside = (1..3).contains(&tx) && (1..3).contains(&ty);
                let expected = if inside { [255, 0, 0, 255] } else { [0; 4] };
                assert_eq!(target[at..at + 4], expected, "({tx}, {ty})");
            }
        }
    }

    /// Shrinking is an area average: the 2×2 square at half size covers one
    /// target pixel fully, and shifted by half a pixel it covers four pixels
    /// by a quarter each — the total opacity is the square's area either way.
    #[test]
    fn resampling_down_averages_the_area() {
        let pixels = red_square();
        let source = Bgra {
            pixels: &pixels,
            pitch: 8,
            width: 2,
            rows: 2,
            left: 0.0,
            top: 2.0,
        };
        let mut target = vec![0u8; 3 * 3 * 4];
        resample(&source, (1.0, 2.0), 0.5, (3, 3), &mut target);
        assert_eq!(target[(3 + 1) * 4..(3 + 1) * 4 + 4], [255, 0, 0, 255]);
        let alpha: u32 = target.chunks(4).map(|p| u32::from(p[3])).sum();
        assert_eq!(alpha, 255);

        target.fill(0);
        resample(&source, (1.5, 2.5), 0.5, (3, 3), &mut target);
        let alphas: Vec<u8> = target.chunks(4).map(|p| p[3]).collect();
        assert_eq!(alphas, [0, 0, 0, 0, 64, 64, 0, 64, 64]);
    }

    /// Every cluster really **shapes** into one colour glyph of its own, not
    /// its base character's: the atlas test that places them in two colour
    /// slots would pass on the base-character fallback too (a flag's base is
    /// a colour regional indicator), so the composition is checked here.
    #[test]
    fn every_cluster_shapes_into_one_colour_glyph() {
        let (base, _) = FreeType::open(DEFAULT_FAMILY, 26.0);
        for text in CLUSTERS {
            let (font, glyph) = FreeType::shape(&base, text)
                .unwrap_or_else(|| panic!("'{text}' did not shape into one glyph"));
            assert!(
                FreeType::has_color_glyphs(&font),
                "'{text}' shaped with {}, not a colour font",
                font.source.family
            );
            let first = text.chars().next().expect("a cluster is not empty");
            let single = FreeType::glyph(&font, first);
            // A selector sequence may legitimately shape into its base
            // character's glyph (the selector only picks the presentation);
            // the other sequences are ligatures.
            if !text.contains('\u{FE0F}') {
                assert_ne!(Some(glyph), single, "'{text}' is its base glyph");
            }
        }
    }

    /// A colour glyph draws at the size the gate measured: the drawn pixels
    /// stay inside the scaled ink box (one pixel of filter spill aside).
    #[test]
    fn a_colour_glyph_draws_inside_its_measured_ink() {
        let (base, _) = FreeType::open(DEFAULT_FAMILY, 26.0);
        let emoji = FreeType::cascade(&base, &CLUSTER_BASE.to_string()).expect("an emoji font");
        assert!(
            FreeType::has_color_glyphs(&emoji),
            "{CLUSTER_BASE} from a text font"
        );
        let glyph = FreeType::glyph(&emoji, CLUSTER_BASE).expect("the emoji is mapped");
        let m = crate::rules::metrics(&base, 1.0);
        let (w, h) = m.cell_wh();
        // Shrunk so it fits the slot, as the gate's shrink arm would.
        let ink = FreeType::ink(&emoji, glyph);
        let small = FreeType::at_size(
            &emoji,
            FreeType::size(&emoji) * (w as f64 / ink.width) * 0.9,
        );
        let ink = FreeType::ink(&small, glyph);
        let (x, baseline) = (-ink.x, f64::from(h as u32) / 2.0 - ink.y - ink.height / 2.0);
        let mut target = vec![0u8; m.slot_bytes_rgba()];
        let drawn = FreeType::draw_color(&small, glyph, m, x, baseline, &mut target);
        assert!(matches!(drawn, DrawResult::Drawn));
        let painted: Vec<(usize, usize)> = (0..h)
            .flat_map(|ty| (0..w).map(move |tx| (tx, ty)))
            .filter(|&(tx, ty)| target[(ty * w + tx) * 4 + 3] > 0)
            .collect();
        assert!(!painted.is_empty(), "nothing drawn");
        let right = painted.iter().map(|p| p.0).max().unwrap_or(0) as f64;
        assert!(
            right <= ink.width.ceil() + 1.0,
            "drawn to column {right}, measured ink {:.2} wide",
            ink.width
        );
    }
}
