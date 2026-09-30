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

/// Font face — a **typographic concept**, not an SGR flag.
///
/// Its four variants match `bt-core`'s `bold`/`italic` flags, but **for
/// different reasons**: there it is terminal semantics (SGR 1 / SGR 3), here
/// it is a CoreText trait. Merging the two into one type because "they look
/// the same" would add a `bt-core` edge to `bt-atlas`, and that edge would
/// pull `alacritty_terminal` into a pure-CoreText crate. The translation
/// lives in `bt-gpu`, the one layer that sees both.
// `repr(u8)`: see `RuleKind` — the key is on `slot()`'s hot path.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Face {
    #[default]
    Regular = 0,
    Bold = 1,
    Italic = 2,
    BoldItalic = 3,
}

/// Which **point-size class** a sprite is rasterized in.
///
/// An axis **orthogonal** to [`Face`] and deliberately separate: the face is
/// the text's style (the counterpart of SGR 1 / SGR 3), this is its size. Had
/// it been added to `Face` as a fifth variant, "bold small" would be
/// unrepresentable and [`Faces::effective`]'s ladder would line two separate
/// questions up in a single order.
///
/// The small class has **one** consumer, the dock's context line; that line
/// is the terminal's own footer and the shell's styling never reaches it, so
/// only the regular face is rasterized on the small side (`Atlas::slot`).
// `repr(u8)`: see `RuleKind` — the key is on `slot()`'s hot path.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum SizeClass {
    #[default]
    Normal = 0,
    Small = 1,
}

impl Face {
    /// The name used in the warning text.
    fn name(self) -> &'static str {
        match self {
            Face::Regular => "Regular",
            Face::Bold => "Bold",
            Face::Italic => "Italic",
            Face::BoldItalic => "BoldItalic",
        }
    }

    /// The face's CoreText trait mask.
    ///
    /// `Regular` is **never called**: the regular face is not derived, it
    /// comes from the chain. Had it returned an empty mask, `derive_face`
    /// would have to read that as a "not a real face" sentinel and `None`
    /// would carry two meanings.
    fn traits(self) -> CTFontSymbolicTraits {
        match self {
            // audit: unreachable, and what guards it is the **module
            // boundary**, not caller discipline: `derive_face` is private to
            // font.rs and its only caller iterates over
            // `[Bold, Italic, BoldItalic]`. Were it `pub(crate)`, someone in
            // the crate calling it with `Face::Regular` would land here
            // without a compiler warning, and the panic would take the frame
            // down on the main thread via `slot()`.
            Face::Regular => unreachable!("regular face is not derived, it comes from the chain"),
            Face::Bold => CTFontSymbolicTraits::TraitBold,
            Face::Italic => CTFontSymbolicTraits::TraitItalic,
            Face::BoldItalic => CTFontSymbolicTraits::TraitBold | CTFontSymbolicTraits::TraitItalic,
        }
    }
}

/// Four faces, in `Face` order. The regular face comes from the chain, the
/// others are derived from it.
pub(crate) struct Faces {
    fonts: [CFRetained<CTFont>; 4],
    /// The faces actually **acquired**; one that could not be acquired has
    /// collapsed onto the regular face.
    ///
    /// Without keeping this, the caller could not know which face it got and
    /// the slot waste that [`Faces::effective`] closes would silently stay
    /// open.
    acquired: [bool; 4],
}

/// The outcome of the requested family that has to be told to the user.
///
/// **Both cannot happen at once:** a family that is not found is replaced by
/// the chain, and both fonts of the chain are monospaced. That is why the
/// type is not a list.
///
/// No text: this crate builds no UI strings, it only reports the fact. The
/// type does not leave `bt-atlas` — `bt-gpu` translates it into its own
/// notice type (`bt-shell` does not see this crate, 003 R5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FontIssue {
    /// The family is not on this machine; `using` is the name of the family
    /// the chain actually opened.
    FamilyNotFound { requested: String, using: String },
    /// The family opened but CoreText does not consider it monospaced. **Not
    /// rejected**: the cell derives from the space's width and letters are
    /// drawn clipped to the cell, i.e. a broken but working screen. `family`
    /// is the name CoreText reports.
    NotMonospaced { family: String },
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

    /// Derives from the given regular face.
    ///
    /// A separate constructor, for testing: the base of the chain (Menlo)
    /// carries all four faces, so the fallback branch can only be fired with
    /// a real font by passing a **single-face** family.
    pub(crate) fn derive(regular: CFRetained<CTFont>) -> Self {
        let mut fonts = [regular.clone(), regular.clone(), regular.clone(), regular];
        let mut acquired = [true, false, false, false];
        let mut missing: Vec<&str> = Vec::new();
        for face in [Face::Bold, Face::Italic, Face::BoldItalic] {
            match derive_face(&fonts[Face::Regular as usize], face) {
                Some(font) => {
                    fonts[face as usize] = font;
                    acquired[face as usize] = true;
                }
                None => missing.push(face.name()),
            }
        }
        if !missing.is_empty() {
            // Once per atlas construction — `slot()` is on the drawing path
            // and a line printed there would repeat every frame. **Not "once
            // per lifetime":** `Atlas::ensure` rebuilds the atlas (and this)
            // when the family/size/scale changes, so the line shows up again
            // if the window moves between a Retina and an external display.
            // An accepted cost; silencing it would need persistent state
            // outside `Faces`. The prefix is the same as `open_chain`'s
            // (`bateri:`), for the same reason.
            eprintln!(
                "bateri: the font family has no {} face, using the regular face",
                missing.join(", ")
            );
        }
        Self { fonts, acquired }
    }

    pub(crate) fn get(&self, face: Face) -> &CTFont {
        &self.fonts[face as usize]
    }

    /// The face that goes into the slot key: **a face that could not be
    /// acquired collapses to `Regular`**.
    ///
    /// The key must carry the **drawn** face, not the requested one. In a
    /// single-face family (`Monaco`), `(Char, Bold)` and `(Char, Regular)`
    /// would hold byte-for-byte the same bitmap in two separate slots; with
    /// four faces the atlas fills four times as fast, surplus glyphs fall to
    /// tofu and the symptom is silent. The second face of the same fact as
    /// `Sprite::Rule` being lowered to `Regular`: the requested face and the
    /// drawn face need not be the same.
    pub(crate) fn effective(&self, face: Face) -> Face {
        // A ladder, not a straight drop: lowering `BoldItalic` directly to
        // `Regular` would drop the **weight** too. Families with no real
        // `Bold Italic` face but with a `Bold` one are common; there SGR 1;3
        // text would come out regular although the bold face is at hand.
        let ladder: &[Face] = match face {
            Face::BoldItalic => &[Face::BoldItalic, Face::Bold, Face::Italic],
            Face::Bold => &[Face::Bold],
            Face::Italic => &[Face::Italic],
            Face::Regular => &[],
        };
        ladder
            .iter()
            .copied()
            .find(|&f| self.acquired[f as usize])
            .unwrap_or(Face::Regular)
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
    let wanted = face.traits();
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

/// Cell measurements, in **physical pixels**.
///
/// `Atlas::new`'s `scale` parameter is multiplied with the point size and
/// goes into the font, so the display scale is inside these numbers. The
/// scale must be part of the key: a glyph rasterized at @1x blurs **without
/// error** at @2x and the symptom shows only on a two-display machine
/// (discussion.md → Muhakeme).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Metrics {
    /// (width, height).
    pub cell_px: (u16, u16),
    /// Pixels from the **top** of the cell to the baseline; the glyph sits
    /// there. Does not exceed the height of [`Metrics::cell_px`] —
    /// `metrics()` bounds it.
    pub baseline_px: u16,
    /// Underline: (position from the top of the cell, thickness).
    ///
    /// `position + thickness` **never** exceeds `cell_px.1` — [`rule_envelope`]
    /// bounds it. If it did, the line would appear at the top of the next row
    /// and the symptom would be silent.
    pub underline_px: (u16, u16),
    /// Strikeout: (position, thickness). The same guarantee.
    pub strikeout_px: (u16, u16),
}

impl Metrics {
    /// Byte count of a single slot (`R8`: one byte per pixel).
    ///
    /// This is the **single owner** of slot geometry: the atlas's buffer, the
    /// tofu drawing and the raster target all three read it. If the geometry
    /// changes (edge padding, alignment filler) there is one arithmetic point
    /// to fix; spread over three, forgetting one would make the buffers
    /// silently diverge.
    pub fn slot_bytes(self) -> usize {
        let (w, h) = self.cell_wh();
        w * h
    }

    /// A single slot of the colour plane (`RGBA8`: **four** bytes per pixel).
    ///
    /// Same slot geometry, different format — and the single-owner rule is
    /// not broken: both derive from [`Metrics::cell_wh`], so if edge padding
    /// or alignment filler ever comes in there is still one place to fix.
    /// The number could have been written as `slot_bytes() * 4`; a separate
    /// name forces whoever builds the buffer to **state** which plane it is
    /// for, and `raster::draw`'s assert catches the wrong plane.
    pub fn slot_bytes_rgba(self) -> usize {
        self.slot_bytes() * 4
    }

    /// Cell size as `usize` — for indexing and loop bounds.
    ///
    /// The same rationale as [`Metrics::slot_bytes`]: it keeps the widening
    /// with a single owner instead of spreading it over four places.
    pub(crate) fn cell_wh(self) -> (usize, usize) {
        (usize::from(self.cell_px.0), usize::from(self.cell_px.1))
    }
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
/// [`FALLBACK`]. Anything to tell the user is in the second value.
///
/// The requested family is checked **by the returned name** like the other
/// links ([`same_family`]): for a name that does not exist CoreText gives
/// Helvetica on this machine, so unchecked, every misspelled name would open
/// with a proportional font and the symptom would be a "not monospaced"
/// warning — one that names a side effect, not the actual error.
pub(crate) fn open_chain(
    family: Option<&str>,
    point_size: CGFloat,
) -> (CFRetained<CTFont>, Option<FontIssue>) {
    let Some(requested) = family else {
        return (open_default(point_size).0, None);
    };
    let (font, returned) = open(requested, point_size);
    if !same_family(&returned, requested) {
        let (font, using) = open_default(point_size);
        let issue = FontIssue::FamilyNotFound {
            requested: requested.to_owned(),
            using,
        };
        return (font, Some(issue));
    }
    let issue = (!is_monospaced(&font)).then_some(FontIssue::NotMonospaced { family: returned });
    (font, issue)
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

/// Is the family name CoreText reports the requested name —
/// **case-insensitively**.
///
/// CoreText finds the name case-insensitively (`"menlo"` → `Menlo`,
/// measured) but reports it in its own spelling; an exact comparison would
/// ignore the font it found. A PostScript name (`Menlo-Regular`) **does not
/// match**: CoreText opens that too, but the family name differs, and that
/// name names a single face of the family — the setting asks for a family
/// (`docs/AYARLAR.md`).
fn same_family(returned: &str, requested: &str) -> bool {
    returned.to_lowercase() == requested.to_lowercase()
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
pub(crate) fn glyph_index(font: &CTFont, ch: char) -> Option<CGGlyph> {
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
    (glyphs[0] != 0).then_some(glyphs[0])
}

/// Derives the cell size from the font's own metrics.
///
/// `line_height` is the user's line-spacing multiplier (`[font]
/// line_height`, base `1.0`). The surplus is distributed **equally below and
/// above** the glyph: half pushes the baseline down, the rest stays at the
/// bottom. Added to one side only, the text would shift up or down inside its
/// cell, and the shift would grow as the line spacing opens up.
///
/// Underline and strikeout follow **by themselves**: both are measured from
/// the baseline and the baseline has already moved. A separate correction
/// would tear the lines away from the letters as the multiplier grows.
pub(crate) fn metrics(font: &CTFont, line_height: f64) -> Metrics {
    // SAFETY: `font` is alive; all three are pure reads.
    let (ascent, descent, leading) = unsafe { (font.ascent(), font.descent(), font.leading()) };
    // The height is found by rounding the two parts **separately** and adding
    // them, not by `round_up(ascent + descent + leading)`. The difference was
    // a measurable clip: on this machine Menlo 13pt gives ascent 12.067,
    // descent 3.066, and rounding the sum up gives 16 — with the baseline at
    // 13 that leaves 3 pixels below, while the font asks for 3.066. What is
    // lost is the last coverage row under `g j p q y ,`; the symptom would be
    // "the text looks a bit off". The numbers depend on the font version and
    // may go stale, **the claim does not**: its guard is
    // `descender_fits_in_the_cell`, which reads that metric from the font
    // itself.
    let natural = round_up(ascent).saturating_add(round_up(descent + leading));
    // The multiplier is applied to the **cell**, not to the ascent: the
    // criterion is the distance between lines, and the font's definition of
    // that distance is `ascent + descent + leading`. At `1.0` the surplus is
    // zero, so by default this path is a no-op.
    let extra = round_up(f64::from(natural) * (line_height - 1.0));
    let above = extra / 2;
    let baseline = round_up(ascent).saturating_add(above);
    let cell_px = (
        round_up(space_advance(font)),
        // `saturating_add`: both parts can go up to `u16::MAX`.
        natural.saturating_add(extra),
    );
    // SAFETY: `font` is alive; all three are pure reads.
    let (u_pos, u_thick, x_h) = unsafe {
        (
            font.underline_position(),
            font.underline_thickness(),
            font.x_height(),
        )
    };
    // CoreText's `underline_position` is **negative**: it points below the
    // baseline. The sign is flipped when converting to a position measured
    // from the top of the cell.
    let thickness = round_up(u_thick);
    let underline_px = rule_envelope(
        baseline.saturating_add(round_up(-u_pos)),
        thickness,
        cell_px.1,
    );
    // Strikeout has **no** CoreText counterpart; half the x-height above the
    // baseline, the usual place in typography. `saturating_sub`: at small
    // point sizes the x-height can exceed the baseline.
    let strikeout_px = rule_envelope(
        baseline.saturating_sub(round_up(x_h / 2.0)),
        thickness,
        cell_px.1,
    );
    Metrics {
        cell_px,
        // The baseline stays inside the cell, and that is now a consequence,
        // not a wish: the bottom part is at least 1 because of `round_up`, so
        // `baseline_px < cell_px.1`. That is why `raster`'s
        // `cell_h - baseline` subtraction does not overflow.
        baseline_px: baseline,
        underline_px,
        strikeout_px,
    }
}

/// Fits a rule line **inside** the cell: (position from the top, thickness).
///
/// The return's invariant is `position + thickness <= cell_h`. In this
/// repository the bound is never stressed with today's font (Menlo) — at
/// 13pt the underline is 14+1, the cell 18 — but the clamp is a contract, not
/// a wish: `underline_position` is the font's own data and a font with a
/// narrow descent can push the line out of the cell. The symptom is silent:
/// one row's underline appears at the top of the row below. Its guard is
/// `envelope_stays_inside_cell`, and it tests with **synthetic** input,
/// because a real font never fires this branch.
pub(crate) fn rule_envelope(top: u16, thickness: u16, cell_h: u16) -> (u16, u16) {
    // No rule fits in zero height. It cannot be reached through `metrics()`
    // (`round_up` pins every measurement to >= 1), but the function's only
    // reason to exist is carrying the invariant: it should not both state it
    // and break it.
    if cell_h == 0 {
        return (0, 0);
    }
    // The thickness cannot exceed the cell; at least 1 — a line that is not
    // drawn is not a rule.
    let thickness = thickness.clamp(1, cell_h);
    (top.min(cell_h - thickness), thickness)
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
pub(crate) fn glyph_advance(font: &CTFont, glyph: CGGlyph) -> CGFloat {
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
pub(crate) fn glyph_ink(font: &CTFont, glyph: CGGlyph) -> CGRect {
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
    rect[0]
}

/// The glyph's horizontal shift inside the cell — **one formula, two
/// consumers**.
///
/// Drawing ([`crate::raster::draw`]) puts the glyph here, the gate
/// ([`fallback_font`]) measures the ink from here. Written separately, the
/// gate would test a placement that will not be drawn and the two would
/// silently diverge: an accepted candidate could paint outside the cell, or a
/// fitting candidate would be rejected.
///
/// The argument is the **box's** advance, not the cell's: for a single-cell
/// glyph the two are the same number, for a two-column character the box is
/// two cells ([`crate::Half`]). The rename from `cell_advance` to
/// `box_advance` in 023 was not a one-line rename: the same number goes to
/// both the gate and the drawing, so were it multiplied by the columns in
/// only one of them, the gate would test a placement that will not be drawn.
///
/// `max(0.0)` is the drawing's own rule: a glyph whose advance exceeds the
/// box sticks to the left, because clipping should happen on the right — the
/// rationale is in the body of [`crate::raster::draw`]. The gate must share
/// it **exactly**, otherwise it would assume a negative shift and believe the
/// candidate's left side to be inside the cell.
pub(crate) fn centre_shift(box_advance: CGFloat, advance: CGFloat) -> CGFloat {
    ((box_advance - advance) / 2.0).max(0.0)
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
/// in the cell and fits inside — [`Accepted::rise`]), so a second criterion
/// would be a rule written down without a witness.
/// The limit is written by name: a candidate that overflows vertically today
/// falls to **clipping**, not to the box.
fn ink_fits_box(font: &CTFont, glyph: CGGlyph, box_advance: CGFloat) -> bool {
    ink_fits_placed(
        box_advance,
        glyph_advance(font, glyph),
        glyph_ink(font, glyph),
    )
}

/// The font-free body of [`ink_fits_box`]: does a glyph with advance
/// `advance` and ink `ink` fit in the box at the place [`centre_shift`] puts
/// it.
///
/// It is separate because of the census (`census`): it asks **how much** a
/// candidate would have to shrink to fit, and gives the scaled measurements
/// without a font. The rule stays in one place, so what the census says
/// "fits" and what the gate accepts cannot diverge.
pub(crate) fn ink_fits_placed(box_advance: CGFloat, advance: CGFloat, ink: CGRect) -> bool {
    let left = ink.origin.x + centre_shift(box_advance, advance);
    // The left edge is tested too: a candidate carrying a negative `origin.x`
    // overflows the cell on the left and CG clips it **from the left**. In
    // Latin script a letter is recognised from its left side, so that clip
    // would be a silent corruption — the box is honest.
    left >= 0.0 && left + ink.size.width <= box_advance
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
///    if within the limit ([`SHRINK_LIMIT`]), otherwise box. The criterion
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
/// ([`Faces::derive`]'s written rule).
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

/// The ink gate: does the candidate's glyph fit first in one cell, then (if
/// declared two columns) in two cells — and if not, does it fit **shrunk**.
///
/// The **shared** gate of the single-glyph fallback ([`fallback_font`]) and
/// the grapheme cluster ([`shape_cluster`]) — the "box, full glyph or a glyph
/// shrunk just enough to fit" contract goes through the same order in both,
/// so a cluster's glyph cannot be accepted by a criterion different from a
/// single-code-point emoji's. It is also the census's (`census`) gate, hence
/// `pub(crate)`.
pub(crate) fn accept(
    candidate: CFRetained<CTFont>,
    glyph: CGGlyph,
    cell_advance: CGFloat,
    cols: u8,
) -> Option<Accepted> {
    // **The order is mandatory: one cell first.** A candidate that fits in
    // one cell fits today too and is drawn from a single slot; asked directly
    // with the two-cell box, `centre_shift` would move it to the middle of
    // two cells and a drawing *that works today* would move. Measured (023
    // `context.md`): 65 characters are declared wide but their ink fits in
    // one cell — 21 are Menlo's own glyphs, 44 are CJK punctuation and
    // fullwidth forms with slender ink from the cascade (`、 。 》 ！`). A side
    // benefit is capacity: those 65 do not spend a second slot.
    if ink_fits_box(&candidate, glyph, cell_advance) {
        return Some(Accepted {
            font: candidate,
            glyph,
            cols: 1,
            shrunk: false,
        });
    }
    // The second gate opens only for a character **declared two columns**.
    // Giving two cells to a single-column character would paint over its
    // neighbour: the grid reserves no spacer for it and that cell has its own
    // ink. That is why the criterion is `min(columns, ink)`.
    let box_advance = cell_advance * CGFloat::from(cols.max(1));
    if cols >= 2 && ink_fits_box(&candidate, glyph, box_advance) {
        return Some(Accepted {
            font: candidate,
            glyph,
            cols,
            shrunk: false,
        });
    }
    // **Third arm: shrinking** (041). The last arm, so a candidate that
    // passes either gate never gets here and its raster is bit-for-bit
    // today's (R3.3).
    shrink(&candidate, glyph, box_advance).map(|font| Accepted {
        font,
        glyph,
        cols: cols.max(1),
        shrunk: true,
    })
}

/// A copy of a candidate rejected by the gate, at the point size that fits
/// the box — if within the limit.
///
/// The box is the area the grid reserves for the character: one cell for a
/// single-column one, two cells for a two-column one. For a two-column
/// character the one-cell box is not tried separately: a narrower box needs
/// more shrinking, so a candidate that does not fit two cells within the
/// limit never fits one.
///
/// The factor comes from [`fit_ratio`], not from the ink/box ratio:
/// shrinking shrinks the advance too, and a glyph whose advance still
/// exceeds the box sticks to the left by [`centre_shift`]'s rule. `⧉` is the
/// example — the copy shrunk by the ratio (1.11) is rejected again on
/// re-test, 1.22 is needed (`.tasks/041-yedek-glyph-kucultme/phase-1.md` →
/// Uygulama Notları).
///
/// The copy is **the same font** at another point size
/// (`CTFontCreateCopyWithAttributes`): the glyph number, the colour trait
/// and hence the plane do not change, the drawing (`raster::draw_glyph` /
/// `draw_color_glyph`) and the centring ([`centre_shift`]) stay untouched.
/// The copy is tested **again** with [`ink_fits_box`] — the rule that the
/// gate measures the ink where the candidate will be drawn holds for the
/// small copy too; if it fails, box.
///
/// `.LastResort` is not accepted in this arm (R3.2): rationale in
/// [`is_last_resort`].
fn shrink(candidate: &CTFont, glyph: CGGlyph, box_advance: CGFloat) -> Option<CFRetained<CTFont>> {
    let fit = fit_ratio(
        box_advance,
        glyph_advance(candidate, glyph),
        glyph_ink(candidate, glyph),
    );
    if !(fit.is_finite() && fit <= SHRINK_LIMIT) || is_last_resort(candidate) {
        return None;
    }
    // SAFETY: `candidate` is alive; a pure read.
    let size = unsafe { candidate.size() };
    // SAFETY: `candidate` is alive; the matrix is `NULL` (the font's own
    // matrix) and there are no attributes, so the only thing that changes is
    // the point size.
    let at = |s: CGFloat| unsafe { candidate.copy_with_attributes(s, ptr::null(), None) };
    let fits = |f: &CTFont| ink_fits_box(f, glyph, box_advance);
    let first = at(size / fit);
    if fits(&first) {
        return Some(first);
    }
    // The factor misses on a font that does not scale linearly with the point
    // size: Apple Color Emoji's advance is rounded to an integer and at small
    // sizes wider than its proportion (at 16pt @2x the copy shrunk by 1.661
    // advances 23 px, the cell is 19.27). The copy's **own** measurement
    // finds the remaining margin: the largest point size that fits, by
    // bisection between `size / fit` and half of it.
    let (mut hi, mut lo) = (size / fit, size / fit / 2.0);
    let mut best = at(lo);
    if !fits(&best) {
        return None;
    }
    for _ in 0..SHRINK_STEPS {
        let mid = (lo + hi) / 2.0;
        let copy = at(mid);
        if fits(&copy) {
            lo = mid;
            best = copy;
        } else {
            hi = mid;
        }
    }
    Some(best)
}

/// The bisection steps of [`shrink`]'s second round: the interval is
/// `size / fit` and half of it, and ten steps bring it under a thousandth of
/// the point size (0.016 pt at 32 pt). Enough: in the census (`make tarama`,
/// four combinations) the number of candidates within the limit that are
/// rejected on re-test is **zero**.
const SHRINK_STEPS: usize = 10;

/// The upper bound of shrinking: a candidate whose [`fit_ratio`] is larger
/// than this stays a box.
///
/// A design constant (after `GUTTER_PT`), its value from the census
/// distribution (`make tarama`, Menlo 13/16 pt × @1x/@2x,
/// `.tasks/041-yedek-glyph-kucultme/` → phase-2 Uygulama Notları):
///
/// - **The largest that must stay inside** is Apple Color Emoji's
///   single-column `fit`: 1.661–1.681 at @2x, **2.124** at @1x (Karar 2, the
///   user preferred a small emoji to a box; a non-Retina display is a user
///   too). A two-column emoji fits in neither one nor two cells at @1x, and
///   its `fit` in the two-cell box is 1.062 — the same arm covers it too.
/// - **The smallest that stays outside** is 2.250 (a single glyph of Apple
///   Symbols); then STIX Two Math 2.307 / 2.583, Symbol 2.803, the user font
///   Inter Display 4.2. These would shrink by more than half and become a dot
///   in the cell.
///
/// 2.2 lies between the two: 3.6% above emoji, 2.2% below the outside one.
/// `.LastResort` (1.660) is **below** the limit and cannot be told apart by
/// geometry — what keeps it outside is [`is_last_resort`].
pub(crate) const SHRINK_LIMIT: f64 = 2.2;

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

/// The smallest `fit` the gate passes when the glyph is shrunk by a `1 / fit`
/// scale.
///
/// Bisection, not a closed form: the rule is [`ink_fits_placed`] itself and a
/// closed form would write it (including the stick-to-the-left arm) a second
/// time. As the scale goes to zero every glyph shrinks to the middle of the
/// box and fits, so the lower end always passes; `1` for a candidate that
/// passes the gate. Two consumers: the shrink factor ([`shrink`]) and the
/// census (`census`) — written separately, what the census calls "within the
/// limit" and what the gate shrinks could diverge.
///
/// The assumption that ink and advance scale **linearly** with the point
/// size is covered by the copy's re-test ([`shrink`]).
pub(crate) fn fit_ratio(box_advance: CGFloat, advance: CGFloat, ink: CGRect) -> f64 {
    let fits = |s: CGFloat| {
        let mut scaled = ink;
        scaled.origin.x *= s;
        scaled.size.width *= s;
        ink_fits_placed(box_advance, advance * s, scaled)
    };
    if fits(1.0) {
        return 1.0;
    }
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..60 {
        let mid = (lo + hi) / 2.0;
        if fits(mid) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    1.0 / lo
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
    accept(font, glyph, cell_advance, cols)
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

/// The accepted candidate and **how many cells** it fits in.
///
/// `cols` is not the number of columns the grid reserves but the box the gate
/// accepted: if a character declared two columns fits in one cell, this is
/// `1` and it is drawn from a single slot.
pub(crate) struct Accepted {
    pub(crate) font: CFRetained<CTFont>,
    /// The glyph the gate measured — also the one drawn. For a single code
    /// point it is [`glyph_index`]'s answer, for a cluster the one `CTLine`
    /// shaped.
    pub(crate) glyph: CGGlyph,
    pub(crate) cols: u8,
    /// Did the candidate come from the shrink arm ([`shrink`]). `false` for a
    /// candidate that passed either gate, and then the drawing is
    /// bit-for-bit today's (R3.3).
    pub(crate) shrunk: bool,
}

impl Accepted {
    /// The glyph's vertical shift from the baseline (px, up is positive) —
    /// **one formula**, both drawing recipes (`raster::draw_glyph`,
    /// `raster::draw_color_glyph`) read it from here.
    ///
    /// Zero for an unshrunk candidate: every glyph accepted today stays on
    /// its baseline. For a shrunk one the vertical middle of the ink comes to
    /// the middle of the **cell**, because the point size shrinks relative to
    /// the origin above the baseline and the glyph collapses towards the
    /// baseline — an emoji shrunk to half sat low next to the letters, its
    /// bottom at the level of `y`'s tail. The choice was made by eye: the
    /// baseline, the cell's middle and the middle of the x-height were drawn
    /// side by side, and emoji and `⧉` read together with the text in the
    /// second (`.tasks/041-yedek-glyph-kucultme/phase-2.md` → Uygulama
    /// Notları). It is rounded to an integer pixel: the baseline is already an
    /// integer and a fractional shift would change the AA phase and blur the
    /// edge.
    ///
    /// The horizontal gate is unaffected by this (its criterion is horizontal
    /// only, [`ink_fits_box`]); the cell's middle also reduces vertical
    /// overflow.
    pub(crate) fn rise(&self, m: Metrics) -> CGFloat {
        if !self.shrunk {
            return 0.0;
        }
        let ink = glyph_ink(&self.font, self.glyph);
        let baseline = CGFloat::from(m.cell_px.1 - m.baseline_px);
        (CGFloat::from(m.cell_px.1) / 2.0 - baseline - (ink.origin.y + ink.size.height / 2.0))
            .round()
    }
}

/// Rounds up and squeezes into a `u16`.
///
/// Lower bound 1: for a broken or missing font a metric can come back zero,
/// and a zero-width cell divides the grid by zero. The upper bound is the
/// type itself.
///
/// NaN is handled separately because `clamp` **lets it through** and
/// `NaN as u16` is 0: the lower bound is silently breached and the error
/// blows up at the division, not at its source.
pub(crate) fn round_up(v: CGFloat) -> u16 {
    if !v.is_finite() {
        return 1;
    }
    v.ceil().clamp(1.0, f64::from(u16::MAX)) as u16
}
