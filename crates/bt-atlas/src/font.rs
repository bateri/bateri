//! Font chain and cell metrics.
//!
//! The crux of the chain: `CTFontCreateWithName` **does not fail**. If the
//! requested family is missing, CoreText returns the closest font it has and
//! the caller notices nothing. So "the font opened" is not evidence; the
//! family name the opened font reports about itself is compared with the one
//! requested.

use std::ptr::{self, NonNull};

use objc2_core_foundation::{
    CFAttributedString, CFDictionary, CFIndex, CFNumber, CFRange, CFRetained, CFString, CFType,
    CGFloat, CGRect, CGSize,
};
use objc2_core_graphics::CGGlyph;
use objc2_core_text::{
    CTFont, CTFontDescriptor, CTFontOrientation, CTFontSymbolicTraits, CTLine, CTRun,
    kCTFontAttributeName, kCTFontFamilyNameAttribute, kCTFontSymbolicTrait, kCTFontTraitsAttribute,
};

/// Order of preference. A name that cannot be found is skipped **silently**:
/// SF Mono ships with Xcode, it is not on every machine, and its absence is
/// not a defect but a designed fallback. A warning only makes sense when the
/// base is substituted too.
const PREFERRED: [&str; 1] = ["SF Mono"];

/// The guaranteed base: installed on every macOS release. It is a separate
/// constant for a type-level guarantee — the chain cannot come back empty, so
/// an `Option`/`expect` path never arises.
const FALLBACK: &str = "Menlo";

use crate::rules::{self, InkRect, Metrics, RawMetrics, same_family};
use crate::rules::{Face, FontIssue};

/// The four faces over CoreText fonts.
pub(crate) type Faces = rules::Faces<CFRetained<CTFont>>;

/// A gate-accepted candidate over a CoreText font.
pub(crate) type Accepted = rules::Accepted<CFRetained<CTFont>>;

/// The face's CoreText trait mask.
///
/// `Regular` is **never called**: the regular face is not derived, it comes
/// from the chain. Had it returned an empty mask, `derive_face` would have to
/// read that as a "not a real face" sentinel and `None` would carry two
/// meanings.
fn face_traits(face: Face) -> CTFontSymbolicTraits {
    match face {
        // audit: unreachable, and what guards it is the **module boundary**,
        // not caller discipline: `derive_face` is private to font.rs and its
        // only caller iterates over `[Bold, Italic, BoldItalic]`. Were it
        // `pub(crate)`, someone in the crate calling it with `Face::Regular`
        // would land here without a compiler warning, and the panic would
        // take the frame down on the main thread via `slot()`.
        Face::Regular => unreachable!("regular face is not derived, it comes from the chain"),
        Face::Bold => CTFontSymbolicTraits::TraitBold,
        Face::Italic => CTFontSymbolicTraits::TraitItalic,
        Face::BoldItalic => CTFontSymbolicTraits::TraitBold | CTFontSymbolicTraits::TraitItalic,
    }
}

impl Faces {
    /// Opens from the chain ([`open_chain`]) and derives the three faces.
    pub(crate) fn from_chain(
        family: Option<&str>,
        point_size: CGFloat,
    ) -> (Self, Option<FontIssue>) {
        let (regular, issue) = open_chain(family, point_size);
        (Self::derive(regular), issue)
    }

    /// Derives from the given regular face ([`rules::Faces::derive_with`]
    /// with CoreText's [`derive_face`]).
    pub(crate) fn derive(regular: CFRetained<CTFont>) -> Self {
        Self::derive_with(regular, |regular, face| derive_face(regular, face))
    }
}

/// Derives `face` from `regular`; `None` if it cannot be acquired.
///
/// The check has **two gates and does no family-name comparison**. A family
/// comparison here would be a tautology: the API's contract already is "a new
/// font in the same family, or NULL", and the family of `Menlo-Bold` is
/// `Menlo`.
///
/// 1. **Is it `nil`** — in the type, it comes as an `Option`.
/// 2. **Did it really acquire the requested trait** — when CoreText cannot
///    find the requested face it **may hand back the regular face**, and
///    that silent substitution is exactly the failure `open_chain` lives
///    through with `CTFontCreateWithName`. The only difference: there the
///    family name is checked, here the trait mask.
fn derive_face(regular: &CTFont, face: Face) -> Option<CFRetained<CTFont>> {
    let wanted = face_traits(face);
    // SAFETY: `regular` is alive; a null `matrix` is valid. **Careful:** in
    // the copy family null does not mean "identity matrix", it means **the
    // source font's matrix is kept** — not to be confused with the
    // `CTFontCreateWithName` rationale in `open()`, where null really is the
    // identity matrix. This is what we want: the derived face should carry
    // the source's transform as is, otherwise the day a font with a matrix
    // joins the chain the slant is applied twice. `size` 0.0 → the source's
    // point size is kept.
    let font = unsafe { regular.copy_with_symbolic_traits(0.0, ptr::null(), wanted, wanted) }?;
    // SAFETY: `font` was just created and is alive in this scope.
    let returned = unsafe { font.symbolic_traits() };
    returned.contains(wanted).then_some(font)
}

/// Opens the named family and returns it **together** with the family name
/// CoreText actually gave. If the two differ, the requested font is not on
/// this machine.
pub(crate) fn open(name: &str, point_size: CGFloat) -> (CFRetained<CTFont>, String) {
    let wanted = CFString::from_str(name);
    // SAFETY: null `matrix` → identity matrix; `CTFontCreateWithName`
    // explicitly supports it and the return is non-null.
    let font = unsafe { CTFont::with_name(&wanted, point_size, ptr::null()) };
    // SAFETY: `font` was just created and is alive in this scope.
    let returned = unsafe { font.family_name() };
    (font, returned.to_string())
}

/// Walks the chain: the requested family (if any), then [`PREFERRED`], then
/// [`FALLBACK`] — [`rules::open_chain`] with CoreText's calls. Anything to
/// tell the user is in the second value.
pub(crate) fn open_chain(
    family: Option<&str>,
    point_size: CGFloat,
) -> (CFRetained<CTFont>, Option<FontIssue>) {
    rules::open_chain(family, point_size, open, open_default, |font| {
        is_monospaced(font)
    })
}

/// Does CoreText consider the font monospaced — the **single place** of the
/// "monospaced" criterion: both the chain's warning ([`open_chain`]) and the
/// settings window's list ([`monospaced_families`]) ask this, so a family
/// picked from the list gets no warning.
fn is_monospaced(font: &CTFont) -> bool {
    // SAFETY: `font` is alive in the caller's hands.
    let traits = unsafe { font.symbolic_traits() };
    traits.contains(CTFontSymbolicTraits::TraitMonoSpace)
}

/// Names of the monospaced families on this machine, in case-insensitive
/// order — the settings window's Font list.
///
/// A family enters the list only if the chain would open it **without a
/// warning**: CoreText resolves the name to its own family ([`same_family`])
/// and the font it opens is monospaced ([`is_monospaced`]). System families
/// starting with a dot (`.AppleSystemUIFont`) are not shown to the user.
///
/// Candidates come from the descriptors CoreText matches on its monospace
/// bit, not from every family on the machine: opening and asking every family
/// means opening hundreds of fonts, and the window would wait for it while
/// opening. The match is only a pre-filter — the last word still belongs to
/// the two criteria above, so no family the criterion rejects can get into
/// the list.
pub fn monospaced_families() -> Vec<String> {
    // The `TraitMonoSpace` bit is `1 << 10`; it fits in an `i32` losslessly.
    let mono = CFNumber::new_i32(CTFontSymbolicTraits::TraitMonoSpace.bits() as i32);
    // SAFETY: both keys are constants CoreText exports, alive for the whole
    // program.
    let (traits_key, symbolic_key) = unsafe { (kCTFontTraitsAttribute, kCTFontSymbolicTrait) };
    let traits = CFDictionary::from_slices(&[symbolic_key], &[&*mono]);
    let attributes = CFDictionary::from_slices(&[traits_key], &[&*traits]);
    // SAFETY: the dictionary has the shape CoreText expects —
    // `kCTFontSymbolicTrait` → `CFNumber` under `kCTFontTraitsAttribute`.
    let wanted = unsafe { CTFontDescriptor::with_attributes(attributes.as_opaque()) };
    // SAFETY: `wanted` is alive; there is no mandatory key set.
    let Some(matches) = (unsafe { wanted.matching_font_descriptors(None) }) else {
        return Vec::new();
    };
    // SAFETY: per the function's documentation, the array's elements are
    // font descriptors.
    let matches = unsafe { matches.cast_unchecked::<CTFontDescriptor>() };
    // SAFETY: the key is a CoreText constant.
    let family_key = unsafe { kCTFontFamilyNameAttribute };
    let mut names: Vec<String> = matches
        .iter()
        // SAFETY: the descriptor is alive in the array's hands.
        .filter_map(|descriptor| unsafe { descriptor.attribute(family_key) })
        .filter_map(|name| name.downcast::<CFString>().ok())
        .map(|name| name.to_string())
        .filter(|name| !name.starts_with('.'))
        .collect();
    names.sort_by_cached_key(|name| name.to_lowercase());
    names.dedup();
    // The point size does not matter: family and monospacing are independent
    // of it.
    const PROBE_SIZE: CGFloat = 12.0;
    names.retain(|name| {
        let (font, returned) = open(name, PROBE_SIZE);
        same_family(&returned, name) && is_monospaced(&font)
    });
    names
}

/// What the chain would say while opening `family` — the state of a family
/// **not** in the settings window's Font list (`— not found` / `— not
/// monospaced`, 029 Karar 3). The question is [`open_chain`] itself, so what
/// the window says and what the subtitle says cannot diverge.
pub fn family_issue(family: &str) -> Option<FontIssue> {
    // The point size does not matter: family and monospacing are independent
    // of it.
    const PROBE_SIZE: CGFloat = 12.0;
    open_chain(Some(family), PROBE_SIZE).1
}

/// The font opened when the setting asks for no family, and the name CoreText
/// reports for it.
pub(crate) fn open_default(point_size: CGFloat) -> (CFRetained<CTFont>, String) {
    for name in PREFERRED {
        let (font, returned) = open(name, point_size);
        if returned == name {
            return (font, returned);
        }
    }
    let (font, returned) = open(FALLBACK, point_size);
    if returned != FALLBACK {
        // Not expected to land here. If it does, the metrics and glyphs come
        // from an unknown font; kept silent, a wrong cell size would look
        // like "everything is fine". Process output, not a UI string, and its
        // prefix is the same as the repository's other stderr lines
        // (`bateri:`) — a separate prefix would mean a reader filtering on
        // `bateri` misses exactly this line.
        eprintln!("bateri: '{FALLBACK}' not found, CoreText substituted '{returned}'");
    }
    (font, returned)
}

/// The character's glyph number; `None` if the font does not know the
/// character.
pub(crate) fn glyph_index(font: &CTFont, ch: char) -> Option<u32> {
    let mut utf16 = [0u16; 2];
    let unit_count = ch.encode_utf16(&mut utf16).len();
    let mut glyphs = [0 as CGGlyph; 2];
    // The pointers are derived from the **slice**, not from `&array[0]`: for
    // a character outside the BMP `unit_count` is 2 and CoreText touches the
    // second element too (it reads the low surrogate, writes 0 for it). The
    // provenance of a pointer derived from a one-element reference does not
    // cover that second access — it works today, and is undefined under the
    // aliasing model.
    // SAFETY: both slices hold two elements and are alive in this scope;
    // `unit_count` ≤ 2, so the count is consistent with both.
    let _ = unsafe {
        font.glyphs_for_characters(
            NonNull::from(&mut utf16[..]).cast::<u16>(),
            NonNull::from(&mut glyphs[..]).cast::<CGGlyph>(),
            unit_count as isize,
        )
    };
    // The return value is **not the criterion**: for a surrogate pair no
    // glyph is produced for the second UTF-16 unit and the function returns
    // `false`, although the glyph is in the first unit and valid. The only
    // criterion is whether it is `.notdef` (0).
    (glyphs[0] != 0).then_some(u32::from(glyphs[0]))
}

/// Narrows the crate's glyph number back to CoreText's `CGGlyph` — the
/// **single** place of the narrowing.
///
/// Lossless by construction: every `u32` glyph in this crate originated as
/// a CoreText `CGGlyph` (`u16`) widened by [`glyph_index`] or
/// [`shape_cluster`]. The saturation is the type system's due, not a path
/// that is taken.
pub(crate) fn cg_glyph(glyph: u32) -> CGGlyph {
    CGGlyph::try_from(glyph).unwrap_or(CGGlyph::MAX)
}

/// Derives the cell size from the font's own metrics
/// ([`rules::cell_metrics`] with the font's raw measurements).
pub(crate) fn metrics(font: &CTFont, line_height: f64) -> Metrics {
    rules::cell_metrics(raw_metrics(font), space_advance(font), line_height)
}

/// The font's raw vertical measurements, as CoreText reports them.
fn raw_metrics(font: &CTFont) -> RawMetrics {
    // SAFETY: `font` is alive; all six are pure reads.
    unsafe {
        RawMetrics {
            ascent: font.ascent(),
            descent: font.descent(),
            leading: font.leading(),
            underline_position: font.underline_position(),
            underline_thickness: font.underline_thickness(),
            x_height: font.x_height(),
        }
    }
}

/// The space's horizontal advance — the cell width, **fractional**.
///
/// The monospace assumption is in the chain itself (SF Mono / Menlo); if the
/// setting's family is not monospaced the cell still derives from the space,
/// wide letters get clipped and [`FontIssue::NotMonospaced`] says so. The
/// measured character is the space because every font has one; the choice is
/// fixed in the body, because calling it with another character would tie
/// the cell width to that letter of the font.
///
/// The return is **unrounded**, and that is why it is `pub(crate)`: the
/// grid's pitch is the rounded one ([`Metrics::cell_px`]) but two consumers
/// want the fractional one — [`fallback_font`]'s ink gate and
/// [`crate::raster::draw`]'s centring. Had both worked with the rounded
/// value, the base font's **own** glyph would look narrower than the cell
/// (7.827 < 8) and the centring would shift every glyph by under half a
/// pixel: the output would no longer be bit-for-bit the same. Its guard is
/// `the_cell_is_the_rounded_advance`.
pub(crate) fn space_advance(font: &CTFont) -> CGFloat {
    let Some(glyph) = glyph_index(font, ' ') else {
        return 0.0;
    };
    glyph_advance(font, glyph)
}

/// A glyph's horizontal advance, **fractional**.
///
/// It is separate from [`space_advance`] because of the second caller: the
/// fallback candidate's ink gate and `raster::draw`'s centring measure the
/// character's **own** glyph, not the space.
pub(crate) fn glyph_advance(font: &CTFont, glyph: u32) -> CGFloat {
    let glyph = cg_glyph(glyph);
    let mut advance = [CGSize::ZERO; 1];
    // SAFETY: one glyph, one measurement cell; the count is consistent with
    // both.
    unsafe {
        font.advances_for_glyphs(
            CTFontOrientation::Horizontal,
            NonNull::from(&glyph),
            advance.as_mut_ptr(),
            1,
        );
    }
    advance[0].width
}

/// A glyph's **ink** box: the bounds of the pixels that will actually be
/// painted, relative to the left of/above the baseline, and **fractional**.
///
/// A measurement separate from [`glyph_advance`], and the two diverging is
/// the gate's reason to exist: a symbol font's glyph can paint narrower than
/// its advance (`⏺` U+23FA, in STIX Two Math the advance is 1.046 times the
/// cell but the ink 0.914 — measured, this machine, Menlo 16pt). A gate
/// measuring the advance rejects it and the user sees a box in its place.
pub(crate) fn glyph_ink(font: &CTFont, glyph: u32) -> InkRect {
    let glyph = cg_glyph(glyph);
    let mut rect = [CGRect::ZERO; 1];
    // SAFETY: one glyph, one measurement cell; the count is consistent with
    // both.
    unsafe {
        font.bounding_rects_for_glyphs(
            CTFontOrientation::Horizontal,
            NonNull::from(&glyph),
            rect.as_mut_ptr(),
            1,
        );
    }
    // A copy of four `f64`s, no rounding: `CGFloat` is `f64` on macOS.
    let [rect] = rect;
    InkRect {
        x: rect.origin.x,
        y: rect.origin.y,
        width: rect.size.width,
        height: rect.size.height,
    }
}

/// Do the pixels the candidate paints stay inside the **box**.
///
/// The box is one cell or two cells (`box_advance`): a character declared
/// wide occupies two columns, so if its ink fits in two cells it should be
/// accepted. The order itself is in [`fallback_font`].
///
/// The criterion is horizontal and only horizontal. Testing the vertical too
/// **rejects no candidate** today (measured: every candidate that passes the
/// horizontal gate also fits the cell's baseline window; the only set that
/// overflows vertically is emoji, which at full size is rejected
/// horizontally in a single cell and whose shrunk copy is vertically centred
/// in the cell and fits inside — [`rules::Accepted::rise`]), so a second criterion
/// would be a rule written down without a witness.
/// The limit is written by name: a candidate that overflows vertically today
/// falls to **clipping**, not to the box.
fn ink_fits_box(font: &CTFont, glyph: u32, box_advance: CGFloat) -> bool {
    rules::ink_fits_placed(
        box_advance,
        glyph_advance(font, glyph),
        glyph_ink(font, glyph),
    )
}

/// A system font that can draw `ch` — **if it fits the cell**.
///
/// Three steps in one function, because all three answer one question: "can
/// we draw this character with an acceptable font". `None` does not mean "no
/// candidate found" but **"not accepted"**, and the caller does not have to
/// tell them apart — the answer to both is [`crate::TOFU`].
///
/// 1. **Candidate.** `CTFontCreateForString` walks the cascade for us, and
///    that is exactly what `CTFontGetGlyphsForCharacters`, which
///    [`glyph_index`] wraps, **does not do**: it only looks at the given font
///    and does not fall to the cascade. This difference is the set's reason
///    to exist (`⏵` U+23F5 is not in Menlo).
/// 2. **Glyph.** Can the candidate really draw it. The candidate may be
///    `base` itself, and then this step gives `None` — we only land here
///    after `base` gave `.notdef`, so no separate "is it the same font"
///    comparison is needed.
/// 3. **Ink gate** ([`accept`]). Does the candidate paint outside the cell at
///    the place it will be drawn — a candidate that does not fit is shrunk
///    if within the limit ([`rules::SHRINK_LIMIT`]), otherwise box. The criterion
///    is geometric: no family name and no trait bit, the only exception is
///    the shrink arm's `.LastResort` ([`is_last_resort`]; geometry cannot
///    tell it apart from emoji). The measured numbers are in
///    `.tasks/019-glyph-yedegi/phase-1.md` and
///    `.tasks/041-yedek-glyph-kucultme/`. The bound is the **fractional**
///    cell advance ([`space_advance`]), not the rounded cell width: the same
///    number also feeds `raster::draw`'s centring, and keeping two numbers
///    for two jobs would make them diverge.
///
/// The criterion was once the **advance** (`advance <= cell_advance`) and
/// its symptom was seen by the user: Claude Code's tool marker `⏺` (U+23FA)
/// came out as a box. The cause was measured — the candidate from STIX Two
/// Math **advances** 4.6% wider than the cell but **paints** 8.6% narrower,
/// so the gate measuring the advance rejected a glyph that fit comfortably
/// in the cell. 019's calibration samples (2.17× / 1.83× / 1.66×) had no
/// candidate near 1.0, and the gate had never been tested against symbol
/// fonts.
///
/// Changing the criterion also closes the gap in the other direction: a
/// candidate advancing narrow but painting wide is now a **box**, where it
/// used to be silently clipped from the right. "Box or full glyph" is for the
/// first time a contract, not a wish; since 041 "box, full glyph or a glyph
/// shrunk just enough to fit".
///
/// A candidate with no ink **passes** the gate (zero width fits any cell);
/// what gets drawn is an invisible glyph, not a box. Today this path does not
/// arise because combining marks never reach a grid cell as a separate
/// sprite.
///
/// **No log:** the function is on [`crate::Atlas::slot`]'s drawing path and a
/// line printed per glyph would land in the middle of the frame budget
/// ([`rules::Faces::derive_with`]'s written rule).
pub(crate) fn fallback_font(
    base: &CTFont,
    ch: char,
    cell_advance: CGFloat,
    cols: u8,
) -> Option<Accepted> {
    let candidate = cascade_candidate(base, ch);
    let glyph = glyph_index(&candidate, ch)?;
    accept(candidate, glyph, cell_advance, cols)
}

/// Step 1 of [`fallback_font`]: the font the cascade suggests for `ch`.
///
/// It is separate because of the census (`census`): it passes the character
/// through the **same** steps as the gate and reports each step's answer
/// separately. Were the step copied, the census would one day ask a question
/// the gate does not.
///
/// The return is never empty: for a character nobody can draw CoreText gives
/// `.LastResort`, and that returns a glyph too, so the "no candidate" answer
/// arises not here but in the next step ([`glyph_index`]).
pub(crate) fn cascade_candidate(base: &CTFont, ch: char) -> CFRetained<CTFont> {
    let mut utf8 = [0u8; 4];
    let text = CFString::from_str(ch.encode_utf8(&mut utf8));
    let range = CFRange {
        location: 0,
        // `CFString` counts UTF-16 units, not bytes: for a character outside
        // the BMP the range is two units, and passing `1` would ask for half
        // of the surrogate pair.
        length: ch.len_utf16() as CFIndex,
    };
    // SAFETY: `base` and `text` are alive in this scope; `range` is the whole
    // string.
    unsafe { base.for_string(&text, range) }
}

/// The ink gate ([`rules::accept`]) with the candidate's own measurements
/// and CoreText's shrink arm ([`shrink`]).
///
/// The **shared** gate of the single-glyph fallback ([`fallback_font`]) and
/// the grapheme cluster ([`shape_cluster`]), and the census's (`census`),
/// hence `pub(crate)`.
pub(crate) fn accept(
    candidate: CFRetained<CTFont>,
    glyph: u32,
    cell_advance: CGFloat,
    cols: u8,
) -> Option<Accepted> {
    let advance = glyph_advance(&candidate, glyph);
    let ink = glyph_ink(&candidate, glyph);
    rules::accept(
        candidate,
        glyph,
        cell_advance,
        cols,
        advance,
        ink,
        |candidate, box_advance| shrink(candidate, glyph, box_advance),
    )
}

/// The shrink arm ([`rules::shrink`]) with CoreText's calls: the copy at
/// another point size is `CTFontCreateCopyWithAttributes` and is re-tested
/// with [`ink_fits_box`] at its own measurements.
///
/// `.LastResort` is not accepted in this arm (R3.2): rationale in
/// [`is_last_resort`].
fn shrink(candidate: &CTFont, glyph: u32, box_advance: CGFloat) -> Option<CFRetained<CTFont>> {
    // SAFETY: `candidate` is alive; a pure read.
    let size = unsafe { candidate.size() };
    rules::shrink(
        box_advance,
        glyph_advance(candidate, glyph),
        glyph_ink(candidate, glyph),
        size,
        is_last_resort(candidate),
        // SAFETY: `candidate` is alive; the matrix is `NULL` (the font's own
        // matrix) and there are no attributes, so the only thing that changes
        // is the point size.
        |s: CGFloat| unsafe { candidate.copy_with_attributes(s, ptr::null(), None) },
        |f: &CFRetained<CTFont>| ink_fits_box(f, glyph, box_advance),
    )
}

/// The PostScript name of `.LastResort`, the last link of CoreText's cascade.
pub(crate) const LAST_RESORT: &str = "LastResort";

/// Is the candidate the cascade's last resort — the shrink arm's **only**
/// name criterion (R3.2).
///
/// `.LastResort` is the answer "no installed font can draw this character",
/// and its glyph is not the character itself but its block's **representative
/// box**: even shrunk, the user still sees a box, and on top of that a box
/// different from our [`crate::TOFU`]. The ink gate used to reject it by
/// geometry, but in the shrink arm geometry **cannot tell it apart**: its
/// `fit` is 1.660 for every glyph, below the single-column emoji's
/// (1.661–2.124), so any limit that covers emoji covers it too.
///
/// No structural signal was found: the cascade always gives a font and
/// `.LastResort`'s cmap **covers** the character (in the census the "in no
/// font" group is zero, 7189 code points come back with `.LastResort`'s real
/// glyph), so the "no candidate" question cannot see it. The comparison is
/// therefore by name, and by PostScript name, because that is the font's
/// unique identity. Its scope is this arm only: the two gates reject it by
/// geometry as today (or fit it in two cells for a wide character), so the
/// name changes no drawing accepted today.
pub(crate) fn is_last_resort(font: &CTFont) -> bool {
    // SAFETY: `font` is alive; a pure read.
    unsafe { font.post_script_name() }.to_string() == LAST_RESORT
}

/// Shapes a grapheme cluster (`🇹🇷`, `👨‍👩‍👧`, `👍🏽`, `❤️`) into a **single
/// glyph** and passes it through [`fallback_font`]'s gate; `None` means "not
/// a single glyph, or rejected by the gate" and the caller falls back to the
/// base character (035 R1.1).
///
/// Shaping comes from `CTLine`, because the cluster's glyph is not any code
/// point's glyph: the flag's two RIs, the ZWJ family and the skin tone merge
/// into a **single** glyph in the font's ligature/`morx` table, and the only
/// API that asks for that is line layout. `CTFontGetGlyphsForCharacters`,
/// which [`glyph_index`] wraps, looks per code point and would give `🇹🇷` as
/// two separate letters.
///
/// The font is asked **twice**, and the two are separate questions: the
/// candidate comes from the cascade for the whole string
/// (`CTFontCreateForString`, step 1 of [`fallback_font`]) and is given to
/// the line; the font measured and drawn, however, is **the run's own** font.
/// If the candidate cannot draw part of the cluster, `CTLine` may substitute
/// again during shaping, and the advance, ink and plane have to be the
/// measurements of the font that actually produces the glyph — drawing
/// another font's glyph number with the candidate would draw an entirely
/// different letter.
///
/// **No log**, same rationale as [`fallback_font`]: on the drawing path.
pub(crate) fn shape_cluster(
    base: &CTFont,
    text: &str,
    cell_advance: CGFloat,
    cols: u8,
) -> Option<Accepted> {
    let string = CFString::from_str(text);
    let range = CFRange {
        location: 0,
        // UTF-16 units, not bytes ([`fallback_font`]'s same trap): the ZWJ
        // family is five code points but eight units.
        length: text.encode_utf16().count() as CFIndex,
    };
    // SAFETY: `base` and `string` are alive in this scope; `range` is the
    // whole string.
    let candidate = unsafe { base.for_string(&string, range) };
    // SAFETY: the key is a constant CoreText exports, alive for the whole
    // program.
    let font_key = unsafe { kCTFontAttributeName };
    let attributes = CFDictionary::from_slices(&[font_key], &[&*candidate]);
    // SAFETY: the allocator is the default (`None`), the string and the
    // dictionary are alive; the dictionary has the shape CoreText expects —
    // `kCTFontAttributeName` → `CTFont`.
    let attributed =
        unsafe { CFAttributedString::new(None, Some(&string), Some(attributes.as_opaque())) }?;
    // SAFETY: `attributed` is alive; the line copies it.
    let line = unsafe { CTLine::with_attributed_string(&attributed) };
    // SAFETY: `line` is alive; a pure read.
    if unsafe { line.glyph_count() } != 1 {
        return None;
    }
    // SAFETY: `line` is alive; per the documentation the array's elements are
    // `CTRun`.
    let runs = unsafe { line.glyph_runs() };
    // SAFETY: the function's documentation gives the element type as `CTRun`.
    let runs = unsafe { runs.cast_unchecked::<CTRun>() };
    // One glyph means one run; a second run cannot have a glyph.
    let run = runs.get(0)?;
    let mut glyph: CGGlyph = 0;
    // SAFETY: the run carries a single glyph (the line's count is 1), the
    // range is `0..1` and the buffer is one element.
    unsafe {
        run.glyphs(
            CFRange {
                location: 0,
                length: 1,
            },
            NonNull::from(&mut glyph),
        )
    };
    // `.notdef` is not a glyph: it is the font's "I cannot draw this" answer.
    if glyph == 0 {
        return None;
    }
    // SAFETY: `run` is alive; the attribute dictionary is the line's as it
    // falls to the run, its keys are `CFString`.
    let attributes = unsafe { run.attributes() };
    // SAFETY: the keys of CoreText's attribute dictionary are `CFString`; the
    // value's type is checked below with `downcast`.
    let attributes = unsafe { attributes.cast_unchecked::<CFString, CFType>() };
    let font = attributes
        .get(font_key)
        .and_then(|font| font.downcast::<CTFont>().ok())
        .unwrap_or(candidate);
    accept(font, u32::from(glyph), cell_advance, cols)
}

/// Are the font's glyphs **coloured**.
///
/// The criterion is the font's own trait bit (`kCTFontTraitColorGlyphs`),
/// **not** the family name: the rule `CLAUDE.md` writes for the ink gate
/// ("no family-name comparison, trait bit or magic string") is about that
/// gate's criterion, and the subject here is a different one — the answer to
/// "which plane is this glyph rasterized into" is a real property of the
/// font. Looking up Apple Color Emoji by name would silently drop another
/// colour font doing the same job (a Nerd Font emoji set the user installed)
/// to the mask plane.
pub(crate) fn has_color_glyphs(font: &CTFont) -> bool {
    // SAFETY: `font` is alive for the call; the return is a bit set.
    let traits = unsafe { font.symbolic_traits() };
    traits.contains(CTFontSymbolicTraits::TraitColorGlyphs)
}
