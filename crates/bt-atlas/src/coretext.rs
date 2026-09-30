//! The CoreText backend of [`FontSystem`] — the only module of this crate
//! that sees CoreText and CoreGraphics.
//!
//! The crux of the chain: `CTFontCreateWithName` **does not fail**. If the
//! requested family is missing, CoreText returns the closest font it has and
//! the caller notices nothing. So "the font opened" is not evidence; the
//! family name the opened font reports about itself is compared with the one
//! requested (`rules::open_chain`).

use std::ffi::c_void;
use std::ptr::{self, NonNull};

use objc2_core_foundation::{
    CFAttributedString, CFDictionary, CFIndex, CFNumber, CFRange, CFRetained, CFString, CFType,
    CGPoint, CGRect, CGSize,
};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGColorSpace, CGContext, CGGlyph, CGImageAlphaInfo, kCGColorSpaceSRGB,
};
use objc2_core_text::{
    CTFont, CTFontDescriptor, CTFontOrientation, CTFontSymbolicTraits, CTLine, CTRun,
    kCTFontAttributeName, kCTFontFamilyNameAttribute, kCTFontSymbolicTrait, kCTFontTraitsAttribute,
};

use crate::raster::DrawResult;
use crate::rules::{Face, InkRect, Metrics, RawMetrics};
use crate::system::FontSystem;

/// Order of preference. A name that cannot be found is skipped **silently**:
/// SF Mono ships with Xcode, it is not on every machine, and its absence is
/// not a defect but a designed fallback. A warning only makes sense when the
/// base is substituted too.
const PREFERRED: [&str; 1] = ["SF Mono"];

/// The guaranteed base: installed on every macOS release. It is a separate
/// constant for a type-level guarantee — the chain cannot come back empty, so
/// an `Option`/`expect` path never arises.
const FALLBACK: &str = "Menlo";

/// The PostScript name of `.LastResort`, the last link of CoreText's cascade.
pub(crate) const LAST_RESORT: &str = "LastResort";

/// CoreText and CoreGraphics behind [`FontSystem`].
pub(crate) struct CoreText;

/// The face's CoreText trait mask.
///
/// `Regular` is **never called**: the regular face is not derived, it comes
/// from the chain. Had it returned an empty mask, [`CoreText::derive`] would
/// have to read that as a "not a real face" sentinel and `None` would carry
/// two meanings.
fn face_traits(face: Face) -> CTFontSymbolicTraits {
    match face {
        // audit: unreachable, and what guards it is the **module boundary**,
        // not caller discipline: `FontSystem::derive`'s only caller is
        // `rules::Faces::derive`, which iterates over `[Bold, Italic,
        // BoldItalic]`. Someone in the crate calling it with `Face::Regular`
        // would land here without a compiler warning, and the panic would
        // take the frame down on the main thread via `slot()`.
        Face::Regular => unreachable!("regular face is not derived, it comes from the chain"),
        Face::Bold => CTFontSymbolicTraits::TraitBold,
        Face::Italic => CTFontSymbolicTraits::TraitItalic,
        Face::BoldItalic => CTFontSymbolicTraits::TraitBold | CTFontSymbolicTraits::TraitItalic,
    }
}

/// Narrows the crate's glyph number back to CoreText's `CGGlyph` — the
/// **single** place of the narrowing.
///
/// Lossless by construction: every `u32` glyph this backend hands out
/// originated as a `CGGlyph` (`u16`) widened by [`CoreText::glyph`] or
/// [`CoreText::shape`]. The saturation is the type system's due, not a path
/// that is taken.
fn cg_glyph(glyph: u32) -> CGGlyph {
    CGGlyph::try_from(glyph).unwrap_or(CGGlyph::MAX)
}

impl FontSystem for CoreText {
    type Font = CFRetained<CTFont>;

    fn open(name: &str, size: f64) -> (Self::Font, String) {
        let wanted = CFString::from_str(name);
        // SAFETY: null `matrix` → identity matrix; `CTFontCreateWithName`
        // explicitly supports it and the return is non-null.
        let font = unsafe { CTFont::with_name(&wanted, size, ptr::null()) };
        // SAFETY: `font` was just created and is alive in this scope.
        let returned = unsafe { font.family_name() };
        (font, returned.to_string())
    }

    fn open_default(size: f64) -> (Self::Font, String) {
        for name in PREFERRED {
            let (font, returned) = Self::open(name, size);
            if returned == name {
                return (font, returned);
            }
        }
        let (font, returned) = Self::open(FALLBACK, size);
        if returned != FALLBACK {
            // Not expected to land here. If it does, the metrics and glyphs
            // come from an unknown font; kept silent, a wrong cell size would
            // look like "everything is fine". Process output, not a UI string,
            // and its prefix is the same as the repository's other stderr
            // lines (`bateri:`) — a separate prefix would mean a reader
            // filtering on `bateri` misses exactly this line.
            eprintln!("bateri: '{FALLBACK}' not found, CoreText substituted '{returned}'");
        }
        (font, returned)
    }

    /// The check has **two gates and does no family-name comparison**. A
    /// family comparison here would be a tautology: the API's contract
    /// already is "a new font in the same family, or NULL", and the family of
    /// `Menlo-Bold` is `Menlo`.
    ///
    /// 1. **Is it `nil`** — in the type, it comes as an `Option`.
    /// 2. **Did it really acquire the requested trait** — when CoreText
    ///    cannot find the requested face it **may hand back the regular
    ///    face**, and that silent substitution is exactly the failure
    ///    `rules::open_chain` lives through with `CTFontCreateWithName`. The
    ///    only difference: there the family name is checked, here the trait
    ///    mask.
    fn derive(regular: &Self::Font, face: Face) -> Option<Self::Font> {
        let wanted = face_traits(face);
        // SAFETY: `regular` is alive; a null `matrix` is valid. **Careful:**
        // in the copy family null does not mean "identity matrix", it means
        // **the source font's matrix is kept** — not to be confused with the
        // `CTFontCreateWithName` rationale in `open()`, where null really is
        // the identity matrix. This is what we want: the derived face should
        // carry the source's transform as is, otherwise the day a font with a
        // matrix joins the chain the slant is applied twice. `size` 0.0 → the
        // source's point size is kept.
        let font = unsafe { regular.copy_with_symbolic_traits(0.0, ptr::null(), wanted, wanted) }?;
        // SAFETY: `font` was just created and is alive in this scope.
        let returned = unsafe { font.symbolic_traits() };
        returned.contains(wanted).then_some(font)
    }

    fn is_monospaced(font: &Self::Font) -> bool {
        // SAFETY: `font` is alive in the caller's hands.
        let traits = unsafe { font.symbolic_traits() };
        traits.contains(CTFontSymbolicTraits::TraitMonoSpace)
    }

    /// Candidates come from the descriptors CoreText matches on its monospace
    /// bit, not from every family on the machine: opening and asking every
    /// family means opening hundreds of fonts, and the window would wait for
    /// it while opening. System families starting with a dot
    /// (`.AppleSystemUIFont`) are not shown to the user.
    fn families() -> Vec<String> {
        // The `TraitMonoSpace` bit is `1 << 10`; it fits in an `i32`
        // losslessly.
        let mono = CFNumber::new_i32(CTFontSymbolicTraits::TraitMonoSpace.bits() as i32);
        // SAFETY: both keys are constants CoreText exports, alive for the
        // whole program.
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
        matches
            .iter()
            // SAFETY: the descriptor is alive in the array's hands.
            .filter_map(|descriptor| unsafe { descriptor.attribute(family_key) })
            .filter_map(|name| name.downcast::<CFString>().ok())
            .map(|name| name.to_string())
            .filter(|name| !name.starts_with('.'))
            .collect()
    }

    fn glyph(font: &Self::Font, ch: char) -> Option<u32> {
        let mut utf16 = [0u16; 2];
        let unit_count = ch.encode_utf16(&mut utf16).len();
        let mut glyphs = [0 as CGGlyph; 2];
        // The pointers are derived from the **slice**, not from `&array[0]`:
        // for a character outside the BMP `unit_count` is 2 and CoreText
        // touches the second element too (it reads the low surrogate, writes
        // 0 for it). The provenance of a pointer derived from a one-element
        // reference does not cover that second access — it works today, and
        // is undefined under the aliasing model.
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
        // glyph is produced for the second UTF-16 unit and the function
        // returns `false`, although the glyph is in the first unit and valid.
        // The only criterion is whether it is `.notdef` (0).
        (glyphs[0] != 0).then_some(u32::from(glyphs[0]))
    }

    fn advance(font: &Self::Font, glyph: u32) -> f64 {
        let glyph = cg_glyph(glyph);
        let mut advance = [CGSize::ZERO; 1];
        // SAFETY: one glyph, one measurement cell; the count is consistent
        // with both.
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

    /// A symbol font's glyph can paint narrower than its advance (`⏺`
    /// U+23FA, in STIX Two Math the advance is 1.046 times the cell but the
    /// ink 0.914 — measured, this machine, Menlo 16pt). A gate measuring the
    /// advance rejects it and the user sees a box in its place.
    fn ink(font: &Self::Font, glyph: u32) -> InkRect {
        let glyph = cg_glyph(glyph);
        let mut rect = [CGRect::ZERO; 1];
        // SAFETY: one glyph, one measurement cell; the count is consistent
        // with both.
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

    fn raw_metrics(font: &Self::Font) -> RawMetrics {
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

    /// `CTFontCreateForString` walks the cascade for us, and that is exactly
    /// what `CTFontGetGlyphsForCharacters`, which [`CoreText::glyph`] wraps,
    /// **does not do**: it only looks at the given font. This difference is
    /// 019's reason to exist (`⏵` U+23F5 is not in Menlo).
    ///
    /// Never `None`: for a character nobody can draw CoreText gives
    /// `.LastResort`, and that returns a glyph too, so the "no candidate"
    /// answer arises not here but in the next step ([`CoreText::glyph`]).
    fn cascade(base: &Self::Font, text: &str) -> Option<Self::Font> {
        let string = CFString::from_str(text);
        let range = CFRange {
            location: 0,
            // `CFString` counts UTF-16 units, not bytes: for a character
            // outside the BMP the range is two units, and passing `1` would
            // ask for half of the surrogate pair; the ZWJ family is five code
            // points but eight units.
            length: text.encode_utf16().count() as CFIndex,
        };
        // SAFETY: `base` and `string` are alive in this scope; `range` is the
        // whole string.
        Some(unsafe { base.for_string(&string, range) })
    }

    fn at_size(font: &Self::Font, size: f64) -> Self::Font {
        // SAFETY: `font` is alive; the matrix is `NULL` (the font's own
        // matrix) and there are no attributes, so the only thing that changes
        // is the point size.
        unsafe { font.copy_with_attributes(size, ptr::null(), None) }
    }

    fn size(font: &Self::Font) -> f64 {
        // SAFETY: `font` is alive; a pure read.
        unsafe { font.size() }
    }

    /// `.LastResort` is the answer "no installed font can draw this
    /// character", and its glyph is not the character itself but its block's
    /// **representative box**: even shrunk, the user still sees a box, and on
    /// top of that a box different from our [`crate::TOFU`]. The ink gate
    /// rejects it by geometry, but in the shrink arm geometry **cannot tell
    /// it apart**: its `fit` is 1.660 for every glyph, below the
    /// single-column emoji's (1.661–2.124), so any limit that covers emoji
    /// covers it too.
    ///
    /// No structural signal was found: the cascade always gives a font and
    /// `.LastResort`'s cmap **covers** the character (in the census the "in
    /// no font" group is zero, 7189 code points come back with `.LastResort`'s
    /// real glyph), so the "no candidate" question cannot see it. The
    /// comparison is therefore by name, and by PostScript name, because that
    /// is the font's unique identity. Its scope is the shrink arm only (041
    /// R3.2): the two gates reject it by geometry, so the name changes no
    /// drawing they accept.
    fn is_last_resort(font: &Self::Font) -> bool {
        // SAFETY: `font` is alive; a pure read.
        unsafe { font.post_script_name() }.to_string() == LAST_RESORT
    }

    /// The criterion is the font's own trait bit (`kCTFontTraitColorGlyphs`),
    /// **not** the family name: the answer to "which plane is this glyph
    /// rasterized into" is a real property of the font. Looking up Apple
    /// Color Emoji by name would silently drop another colour font doing the
    /// same job (a Nerd Font emoji set the user installed) to the mask plane.
    fn has_color_glyphs(font: &Self::Font) -> bool {
        // SAFETY: `font` is alive for the call; the return is a bit set.
        let traits = unsafe { font.symbolic_traits() };
        traits.contains(CTFontSymbolicTraits::TraitColorGlyphs)
    }

    /// Shaping comes from `CTLine`, because the cluster's glyph is not any
    /// code point's glyph: the flag's two RIs, the ZWJ family and the skin
    /// tone merge into a **single** glyph in the font's ligature/`morx` table,
    /// and the only API that asks for that is line layout.
    /// `CTFontGetGlyphsForCharacters`, which [`CoreText::glyph`] wraps, looks
    /// per code point and would give `🇹🇷` as two separate letters.
    ///
    /// The font is asked **twice**, and the two are separate questions: the
    /// candidate comes from the cascade for the whole string
    /// ([`CoreText::cascade`]) and is given to the line; the font returned,
    /// however, is **the run's own** font. If the candidate cannot draw part
    /// of the cluster, `CTLine` may substitute again during shaping, and the
    /// advance, ink and plane have to be the measurements of the font that
    /// actually produces the glyph — drawing another font's glyph number with
    /// the candidate would draw an entirely different letter.
    fn shape(base: &Self::Font, text: &str) -> Option<(Self::Font, u32)> {
        let candidate = Self::cascade(base, text)?;
        let string = CFString::from_str(text);
        // SAFETY: the key is a constant CoreText exports, alive for the whole
        // program.
        let font_key = unsafe { kCTFontAttributeName };
        let attributes = CFDictionary::from_slices(&[font_key], &[&*candidate]);
        // SAFETY: the allocator is the default (`None`), the string and the
        // dictionary are alive; the dictionary has the shape CoreText expects
        // — `kCTFontAttributeName` → `CTFont`.
        let attributed =
            unsafe { CFAttributedString::new(None, Some(&string), Some(attributes.as_opaque())) }?;
        // SAFETY: `attributed` is alive; the line copies it.
        let line = unsafe { CTLine::with_attributed_string(&attributed) };
        // SAFETY: `line` is alive; a pure read.
        if unsafe { line.glyph_count() } != 1 {
            return None;
        }
        // SAFETY: `line` is alive; per the documentation the array's elements
        // are `CTRun`.
        let runs = unsafe { line.glyph_runs() };
        // SAFETY: the function's documentation gives the element type as
        // `CTRun`.
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
        // `.notdef` is not a glyph: it is the font's "I cannot draw this"
        // answer.
        if glyph == 0 {
            return None;
        }
        // SAFETY: `run` is alive; the attribute dictionary is the line's as it
        // falls to the run, its keys are `CFString`.
        let attributes = unsafe { run.attributes() };
        // SAFETY: the keys of CoreText's attribute dictionary are `CFString`;
        // the value's type is checked below with `downcast`.
        let attributes = unsafe { attributes.cast_unchecked::<CFString, CFType>() };
        let font = attributes
            .get(font_key)
            .and_then(|font| font.downcast::<CTFont>().ok())
            .unwrap_or(candidate);
        Some((font, u32::from(glyph)))
    }

    fn draw_mask(
        font: &Self::Font,
        glyph: u32,
        m: Metrics,
        x: f64,
        baseline: f64,
        target: &mut [u8],
    ) -> DrawResult {
        // Not `debug_assert`: this line is the precondition of the `unsafe`
        // block below. CG gets `width`/`height` from `m` and the pointer from
        // `target`; if the two diverge, CG writes past the short buffer and
        // nothing notices in a release build — the `make hepsi` tests run in
        // debug.
        assert_eq!(target.len(), m.slot_bytes(), "buffer must be a full slot");

        let (w, h) = m.cell_wh();
        // Alpha-only context: **no** colour space (`space: None`), 8 bits per
        // component, row stride the full cell width. The coverage of a glyph
        // drawn in white becomes the alpha byte directly; no separate channel
        // extraction step arises and the buffer is already in the atlas's
        // `R8Unorm` layout.
        // SAFETY: `target` is w*h bytes and alive while the context lives
        // (until the end of this function); the dimensions match the buffer.
        // After the context is dropped `target` is accessed only from Rust.
        let ctx = unsafe {
            CGBitmapContextCreate(
                target.as_mut_ptr().cast::<c_void>(),
                w,
                h,
                8,
                w,
                None,
                CGImageAlphaInfo::Only.0,
            )
        };
        let Some(ctx) = ctx else {
            return DrawResult::NoContext;
        };
        // Clearing comes **after** the context is created: on both failing
        // branches the caller never looks at the buffer (tofu is resident and
        // in the texture), so a memset there would be pure waste.
        target.fill(0);

        CGContext::set_should_antialias(Some(&ctx), true);
        // Subpixel AA is off: the atlas is single-channel and the system
        // itself dropped subpixel AA in macOS 10.14 (discussion.md → karar
        // 3a). Both calls are needed separately: `allows_font_smoothing` turns
        // off the context's permission, `should` the preference for this
        // drawing.
        CGContext::set_allows_font_smoothing(Some(&ctx), false);
        CGContext::set_should_smooth_fonts(Some(&ctx), false);
        // In an alpha-only context the grey component is ignored; alpha is
        // what matters.
        CGContext::set_gray_fill_color(Some(&ctx), 1.0, 1.0);

        // CG's origin is bottom left, and `baseline` is already measured from
        // the slot's bottom, so the position goes in as is.
        let position = CGPoint::new(x, baseline);
        let glyph = cg_glyph(glyph);
        // SAFETY: one glyph, one position, the count matches both; context
        // alive.
        unsafe { font.draw_glyphs(NonNull::from(&glyph), NonNull::from(&position), 1, &ctx) };
        DrawResult::Drawn
    }

    /// A sibling of [`CoreText::draw_mask`] and a **separate** body, not a
    /// parameterised branch of it: what differs is the context itself — this
    /// one has a colour space (`sRGB`), four components per pixel and
    /// premultiplied alpha; the mask's is alpha-only and **cannot produce**
    /// colour (`space: None`, [`CGImageAlphaInfo::Only`]).
    ///
    /// **The colour space must be sRGB** and so must the texture side
    /// (`RGBA8Unorm_sRGB`): the target is `BGRA8Unorm_sRGB`, the hardware
    /// treats fragment output as linear, and an emoji sampled from a non-sRGB
    /// texture **washes out** the palette. The symptom is the same silent
    /// defect as in `CLAUDE.md` → "Renk uzayı sınırı geçer", so its witness
    /// is of the same kind: a mid-tone pixel (`0.0` and `1.0` are fixed points
    /// of the transfer function).
    ///
    /// **Premultiplication is CG's decision**, not ours: CoreGraphics delivers
    /// colour glyphs `PremultipliedLast` and offers no straight alpha at 8
    /// bits; `rules::unpremultiply` undoes it on the caller's side.
    fn draw_color(
        font: &Self::Font,
        glyph: u32,
        m: Metrics,
        x: f64,
        baseline: f64,
        target: &mut [u8],
    ) -> DrawResult {
        // Not `debug_assert`: it is the precondition of the `unsafe` block
        // below and the thing that **catches a mask buffer**. If the wrong
        // plane's buffer is passed, CG writes past the short buffer and the
        // symptom is silent.
        assert_eq!(
            target.len(),
            m.slot_bytes_rgba(),
            "buffer must be a full RGBA slot"
        );

        let (w, h) = m.cell_wh();
        // SAFETY: a named system constant; the return is non-null.
        let space = unsafe { CGColorSpace::with_name(Some(kCGColorSpaceSRGB)) };
        let Some(space) = space else {
            return DrawResult::NoContext;
        };
        // SAFETY: `target` is 4*w*h bytes and alive while the context lives;
        // the dimensions match the buffer. After the context is dropped
        // `target` is accessed only from Rust.
        let ctx = unsafe {
            CGBitmapContextCreate(
                target.as_mut_ptr().cast::<c_void>(),
                w,
                h,
                8,
                w * 4,
                Some(&space),
                CGImageAlphaInfo::PremultipliedLast.0,
            )
        };
        let Some(ctx) = ctx else {
            return DrawResult::NoContext;
        };
        target.fill(0);
        CGContext::set_should_antialias(Some(&ctx), true);
        // Subpixel still off: the same reason as in the mask drawing (the
        // system dropped it too), and it is moot for a colour glyph anyway.
        CGContext::set_allows_font_smoothing(Some(&ctx), false);
        CGContext::set_should_smooth_fonts(Some(&ctx), false);

        let position = CGPoint::new(x, baseline);
        let glyph = cg_glyph(glyph);
        // SAFETY: one glyph, one position, the count matches both; context
        // alive. For a colour font `draw_glyphs` draws the `sbix`/`CBDT` table
        // itself; the separate "is it colour" branch is inside CoreText.
        unsafe { font.draw_glyphs(NonNull::from(&glyph), NonNull::from(&position), 1, &ctx) };
        // The context is dropped before returning, so the caller's pass over
        // `target` cannot touch a buffer CG could still write to.
        drop(ctx);
        DrawResult::Drawn
    }
}

/// Sample characters and family names for the platformless tests
/// (`.tasks/042-font-sistemi-linux/discussion.md` → Karar 7): a test body
/// names no font and assumes no character of the base font; what it needs
/// comes from here, measured on this backend's fonts.
#[cfg(test)]
pub(crate) mod fixture {
    use super::{CoreText, FontSystem};

    /// The default chain's guaranteed base family.
    pub(crate) const DEFAULT_FAMILY: &str = super::FALLBACK;

    /// A family that is installed everywhere and is **not** monospaced.
    pub(crate) const PROPORTIONAL_FAMILY: &str = "Helvetica";

    /// A second monospaced family, not the default one.
    pub(crate) const SECOND_FAMILY: &str = "Monaco";

    /// A character the **one-cell** gate rejects: on the sixteenth plane's
    /// private use area, no installed font covers it and the cascade gives
    /// `.LastResort`, whose ink is 1.494 cells (measured, `make tarama`, the
    /// same in all four combinations) and which the shrink arm keeps out by
    /// name. Tests reading "tofu" from it assume the gate works; the gate's
    /// own guard is `the_gate_decides_by_ink_alone`.
    pub(crate) const UNKNOWN_CHAR: char = '\u{10FFFC}';

    /// A character rejected in one cell but accepted in **two** (a pair of
    /// slots for a wide request): `.LastResort`'s box fits two cells.
    pub(crate) const WIDE_CHAR: char = UNKNOWN_CHAR;

    /// A character the fallback **accepts**, 019's reason to exist: `⏵` is
    /// not in Menlo and comes from STIX Two Math (measured, macOS 26.4.1),
    /// advancing 0.84 of the cell with ink 0.69 — the ratio is scale-free, so
    /// it passes the gate in both size classes. Its side bearings are
    /// asymmetric (1.04 left, 0.13 right), so centring really moves it.
    pub(crate) const FALLBACK_CHAR: char = '⏵';

    /// The character that tests the gate's **criterion**: its advance exceeds
    /// the cell but its ink fits. `⏺` is Claude Code's tool marker and used
    /// to come out as a box; from STIX Two Math it advances 1.046 cells but
    /// paints 0.914 (measured, Menlo 16pt). [`FALLBACK_CHAR`] passes both
    /// criteria and [`UNKNOWN_CHAR`] fails both, so only this one would see
    /// the criterion reverted.
    pub(crate) const INK_CHAR: char = '⏺';

    /// Characters that exercise the gate's **rule**; none is in Menlo, so
    /// they take the fallback path. The expectation is not written here —
    /// whether a character becomes a box depends on the installed fonts
    /// (`U+E0B0` falls to `.LastResort` here but draws with a Nerd Font) — it
    /// is derived from the candidate's own ink by the test.
    pub(crate) const GATE_PROBES: [char; 9] = [
        FALLBACK_CHAR,
        INK_CHAR,
        '𝔸',
        UNKNOWN_CHAR,
        '\u{E0B0}',
        '\u{10FFFD}',
        '🎉',
        '\u{F8FF}',
        // Braille comes from Apple Braille. The large class never reaches
        // the gate (it is drawn procedurally); it stays for the small class,
        // where the procedural gate is closed. Its answer changes with the
        // criterion: advance 1.135 cells, ink 2.62..8.34 inside a 9.633 cell
        // (measured, Menlo 16pt).
        '⠋',
    ];

    /// Symbols beyond printable ASCII whose advance must equal the cell's in
    /// the base font: box drawing, the base font's own symbols and two
    /// combining marks (a zero-advance glyph would rasterize in the middle of
    /// the cell; in Menlo U+0301 advances a full cell — measured).
    pub(crate) const BASE_SYMBOLS: &str = "─│┌┐└┘├┤┬┴┼✓⚠▶\u{0300}\u{0301}";

    /// Grapheme clusters that shape into a single colour glyph: a flag (two
    /// RIs), ZWJ, a skin tone and two VS16 — `❤` and `🌡` alone are
    /// single-column, VS16 makes them two.
    pub(crate) const CLUSTERS: [&str; 5] = [
        "\u{1F1F9}\u{1F1F7}",                          // 🇹🇷
        "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}", // 👨‍👩‍👧
        "\u{1F44D}\u{1F3FD}",                          // 👍🏽
        "\u{2764}\u{FE0F}",                            // ❤️
        "\u{1F321}\u{FE0F}",                           // 🌡️
    ];

    /// An emoji whose doubled string does not shape into one glyph, so the
    /// cluster falls back to it.
    pub(crate) const CLUSTER_BASE: char = '\u{1F44D}'; // 👍

    /// The display scale of the cluster tests: **Retina**. At 13pt@1x even
    /// the single-code-point `👍` is rejected by the two-cell gate (measured
    /// on the flag's glyph: ink 16.25 pt, two cells 15.65 pt), so there a
    /// cluster's rejection would say nothing about shaping and the fallback
    /// to the base character would stay green against tofu.
    pub(crate) const CLUSTER_SCALE: f64 = 2.0;

    /// The family name a font reports — diagnostics in test messages only.
    pub(crate) fn family_name(font: &<CoreText as FontSystem>::Font) -> String {
        // SAFETY: `font` is alive; a pure read.
        unsafe { font.family_name() }.to_string()
    }
}
