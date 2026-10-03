//! The fallback gate's **census** and a **guard** for the characters of real
//! tools (041 phase-1).
//!
//! The gate was calibrated twice by looking at a sample set, and both times
//! the user found the character outside the limit (`⏺`, `⎿`, then `⧉`). The
//! census is therefore an **inventory**, not a sample: it passes every code
//! point of the symbol and emoji blocks through the gate's own steps and puts
//! it in one of four groups. Its result depends on the fonts installed on the
//! machine, so it is not part of the gate — it is run by hand with
//! `make scan`.
//!
//! The module is compiled only for tests: the classification has no
//! consumer in production. It does not copy the gate, it calls its steps;
//! since phase-2 it also sees the shrink branch (`rules::accept`) the same
//! way.

use crate::coretext::LAST_RESORT;
use crate::system::{Backend, Font, FontSystem};
use crate::{Atlas, Face, raster, rules};

/// Where a character lands in the fallback gate.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Class {
    /// The base font draws it; the fallback path never runs.
    InBase,
    /// The cascade's candidate passed the gate.
    Fallback { font: String, ratio: f64 },
    /// The cascade's candidate gave `.notdef` too.
    NoFont,
    /// The candidate was turned back by both gates but its smaller-size copy
    /// was accepted (041).
    Shrunk { font: String, ratio: f64, fit: f64 },
    /// There is a candidate but the gate turned it back: a box on screen.
    /// `fit` is above the limit, the candidate is `.LastResort`, or the small
    /// copy failed the re-test.
    Rejected { font: String, ratio: f64, fit: f64 },
}

/// Passes `ch` through the gate's steps **verbatim**: [`FontSystem::glyph`]
/// (base) → [`FontSystem::cascade`] → [`FontSystem::glyph`] (candidate) →
/// [`rules::accept`]. There is no second gate; the classification only names
/// the steps' answers separately.
///
/// Two ratios, both divided by the **box** (`cell_advance × cols`):
///
/// - `ratio` — ink width / box. "How much wider the ink is than the cell":
///   `⧉` is ~1.11 in Menlo 16pt. Independent of placement, i.e. the shrink
///   that would be needed if the candidate's ink were centred in the box.
/// - `fit` — **by how much** the candidate must be shrunk for the gate to
///   pass with today's placement ([`rules::centre_shift`], including the
///   stick-to-the-left rule); it carries the larger of the left and right
///   overflow. It can be larger than `ratio` because shrinking also shrinks
///   the advance and a glyph whose advance still exceeds the box sticks to
///   the left: a candidate with no right bearing does not move to the centre
///   when shrunk. It is computed by [`rules::fit_ratio`], the same function as
///   the shrink branch's factor.
///
/// `Fallback` does not carry `fit`: a passing candidate raises no shrink
/// question.
pub(crate) fn classify(base: &Font, ch: char, cell_advance: f64, cols: u8) -> Class {
    if Backend::glyph(base, ch).is_some() {
        return Class::InBase;
    }
    let Some(candidate) = Backend::cascade(base, ch.encode_utf8(&mut [0u8; 4])) else {
        return Class::NoFont;
    };
    let Some(glyph) = Backend::glyph(&candidate, ch) else {
        return Class::NoFont;
    };
    // `.LastResort` is named in the report by the gate's own criterion
    // ([`FontSystem::is_last_resort`]), so the report and the gate single out the
    // same font.
    let family = if Backend::is_last_resort(&candidate) {
        LAST_RESORT.to_string()
    } else {
        crate::coretext::fixture::family_name(&candidate)
    };
    let advance = Backend::advance(&candidate, glyph);
    let ink = Backend::ink(&candidate, glyph);
    let box_advance = cell_advance * f64::from(cols);
    let ratio = ink.width / box_advance;
    let fit = rules::fit_ratio(box_advance, advance, ink);
    match rules::accept(candidate, glyph, cell_advance, cell_advance, cols) {
        Some(a) if a.shrunk => Class::Shrunk {
            font: family,
            ratio,
            fit,
        },
        Some(_) => Class::Fallback {
            font: family,
            ratio,
        },
        None => Class::Rejected {
            font: family,
            ratio,
            fit,
        },
    }
}

/// Characters real tools print to the screen. Each one that comes out as a
/// box is a defect the user will see; when the user finds a new one it is
/// added here, so the same defect does not silently come back a second time.
const TOOL_CHARS: [char; 26] = [
    // Claude Code: tool marker, result tree, artifact link, spinner stars,
    // separator, mode indicator, pause, interrupt.
    '⏺', '⎿', '⧉', '✻', '✢', '✳', '✶', '·', '⏵', '⏸', '↯',
    // Spinners: Braille (procedural) and quarter circles.
    '⠋', '⠙', '◐', '◓', '⣾', '⣽',
    // git / starship / p10k: Nerd Font's branch and powerline glyphs (PUA),
    // status marks, ahead/behind, prompt characters, dot.
    '\u{E0A0}', '\u{E0B0}', '✔', '✘', '⇡', '⇣', '❯', '❮', '●',
];

/// The ones from [`TOOL_CHARS`] that come out as a **box** today (Menlo 16pt
/// @2x, this machine).
///
/// The list only shrinks: if a character is now drawn, the guard goes red
/// and asks for it to be removed from here ([`tofu_drift`]). That way a fixed
/// character does not silently stay an "expected box", and if the fix is
/// reverted the guard sees it again.
///
/// `U+E0A0`/`U+E0B0` are in no installed font, the cascade gives
/// `.LastResort`. R3.2 forbids shrinking it ([`FontSystem::is_last_resort`]), so
/// shrinking does not empty this list; what empties it is a machine with a
/// Nerd Font installed. `⧉` left the list in 041 phase-2: it is drawn shrunk.
const EXPECTED_TOFU: [char; 2] = ['\u{E0A0}', '\u{E0B0}'];

/// Compares the observation (character, is it a box) with the expected box
/// list and returns every drift as one line; an empty return is green.
///
/// Both ways: a box not on the list is a defect, and a character on the list
/// that is now drawn says the list must be updated. A character on the list
/// that is not observed is drift too: the list cannot claim anything outside
/// the guard's characters.
fn tofu_drift(observed: &[(char, bool)], expected: &[char]) -> Vec<String> {
    let mut drift = Vec::new();
    for &(ch, tofu) in observed {
        let listed = expected.contains(&ch);
        if tofu && !listed {
            drift.push(format!("'{ch}' (U+{:04X}) renders tofu", u32::from(ch)));
        }
        if !tofu && listed {
            drift.push(format!(
                "'{ch}' (U+{:04X}) is drawn now — drop from EXPECTED_TOFU",
                u32::from(ch)
            ));
        }
    }
    for &ch in expected {
        if !observed.iter().any(|&(c, _)| c == ch) {
            drift.push(format!(
                "'{ch}' (U+{:04X}) is in EXPECTED_TOFU but not in the guard's list",
                u32::from(ch)
            ));
        }
    }
    drift
}

/// Whether a single character comes out as a box today — in the atlas's
/// order for the regular face: the procedural family is drawn before the
/// font, the rest is the gate's classification.
fn is_tofu(a: &Atlas, ch: char) -> bool {
    if raster::is_procedural(ch) {
        return false;
    }
    matches!(
        classify(a.faces.get(Face::Regular), ch, a.cell_advance, 1),
        Class::NoFont | Class::Rejected { .. }
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fmt::Write as _;

    use super::*;
    use crate::Spacing;

    /// The characters of real tools do not come out as boxes — or the ones
    /// that do are named in [`EXPECTED_TOFU`].
    ///
    /// **Known limit:** the expectation depends on the fonts installed on the
    /// machine. On a machine with a Nerd Font installed `U+E0A0` lands on a
    /// real glyph and the guard goes red with "remove it from the list" —
    /// while the code is right. `the_gate_decides_by_ink_alone` therefore
    /// derives its expectation from the candidate; this test deliberately
    /// pins the **fact**, because its question is not "is the rule right" but
    /// "does the user see a box". The base family is independent of settings,
    /// the chain's default.
    #[test]
    fn tool_chars_are_not_tofu() {
        let a = Atlas::new(None, 16.0, 2.0, Spacing::default());
        let observed: Vec<(char, bool)> =
            TOOL_CHARS.iter().map(|&ch| (ch, is_tofu(&a, ch))).collect();
        let drift = tofu_drift(&observed, &EXPECTED_TOFU);
        assert!(drift.is_empty(), "guard drift:\n{}", drift.join("\n"));
    }

    /// The guard's comparison goes red in both directions: a box not on the
    /// list and a character on the list that is now drawn. The directions
    /// that cannot be set up with a real font are tested with a synthetic
    /// observation.
    #[test]
    fn tofu_drift_flags_both_directions() {
        assert!(tofu_drift(&[('a', false), ('⧉', true)], &['⧉']).is_empty());
        let fixed = tofu_drift(&[('⧉', false)], &['⧉']);
        assert_eq!(fixed.len(), 1, "fixed character not seen: {fixed:?}");
        assert!(fixed[0].contains("drop from EXPECTED_TOFU"), "{fixed:?}");
        let broken = tofu_drift(&[('a', true)], &[]);
        assert_eq!(broken.len(), 1, "new box not seen: {broken:?}");
        let stray = tofu_drift(&[], &['⧉']);
        assert_eq!(stray.len(), 1, "stray list entry not seen: {stray:?}");
    }

    /// Draws the accepted candidate onto a 3×3-cell canvas that leaves **one
    /// cell of space on all four sides** of the cell and returns the coverage
    /// (alpha) map; the cell is in the middle of the canvas.
    ///
    /// The slot's own buffer cannot see an overflow — CG clips at the slot's
    /// edge — so the "is it inside the cell" question can only be asked in a
    /// context wider than the slot. The glyph moves to the middle column with
    /// `x_offset = -w` and to the middle row by moving the baseline down one
    /// cell height; the box, centring and `rise` are the same as the atlas's
    /// drawing (`rise` from the real cell's metrics).
    fn draw_wide(alt: &rules::Accepted, m: crate::Metrics, cell: f64) -> Vec<u8> {
        let wide = crate::Metrics {
            cell_px: (m.cell_px.0 * 3, m.cell_px.1 * 3),
            baseline_px: m.baseline_px + m.cell_px.1,
            ..m
        };
        let box_advance = cell * f64::from(alt.cols);
        let offset = -f64::from(m.cell_px.0);
        let rise = alt.rise(m);
        if Backend::has_color_glyphs(&alt.font) {
            let mut rgba = vec![0u8; wide.slot_bytes_rgba()];
            raster::draw_color_glyph(
                &alt.font,
                alt.glyph,
                wide,
                box_advance,
                offset,
                rise,
                &mut rgba,
            );
            rgba.chunks(4).map(|p| p[3]).collect()
        } else {
            let mut mask = vec![0u8; wide.slot_bytes()];
            raster::draw_glyph(
                &alt.font,
                alt.glyph,
                wide,
                box_advance,
                offset,
                rise,
                &mut mask,
            );
            mask
        }
    }

    /// A shrunk glyph stays inside the cell: `⧉` (Apple Symbols, mask) and
    /// the single-column emoji `🌡` (Apple Color Emoji, colour plane) are
    /// accepted at two scales, come from the shrink branch and have not a
    /// single coverage pixel outside the middle cell — not left, right, above
    /// or below (vertical centring, `rules::Accepted::rise`, brings the emoji
    /// inside the cell too). The colour copy keeps its trait, so the plane and
    /// the drawing recipe are the same.
    #[test]
    fn shrunk_glyph_stays_inside_the_cell() {
        for (pt, scale) in [(16.0, 2.0), (13.0, 1.0)] {
            let a = Atlas::new(None, pt, scale, Spacing::default());
            let base = a.faces.get(Face::Regular);
            let m = a.metrics;
            for (ch, color) in [('⧉', false), ('🌡', true)] {
                let alt = rules::fallback_font(base, ch, a.cell_advance, a.cell_advance, 1)
                    .unwrap_or_else(|| panic!("{ch} {pt}pt@{scale}x: came out as a box"));
                assert!(
                    alt.shrunk,
                    "{ch} {pt}pt@{scale}x was accepted without shrinking"
                );
                assert_eq!(alt.cols, 1);
                assert_eq!(Backend::has_color_glyphs(&alt.font), color, "{ch}: plane");
                let cov = draw_wide(&alt, m, a.cell_advance);
                let (w, h) = m.cell_wh();
                let mut ink = 0usize;
                for (i, &c) in cov.iter().enumerate() {
                    if c == 0 {
                        continue;
                    }
                    ink += 1;
                    let (x, y) = (i % (3 * w), i / (3 * w));
                    assert!(
                        (w..2 * w).contains(&x) && (h..2 * h).contains(&y),
                        "{ch} {pt}pt@{scale}x: ({x}, {y}) outside the \
                         cell ({w}..{}, {h}..{})",
                        2 * w,
                        2 * h
                    );
                }
                assert!(ink > 0, "{ch} {pt}pt@{scale}x: no ink at all");
            }
        }
    }

    /// **A shrunk single-cell acceptance is not the answer to a two-cell
    /// request** (041): `漢` shrinks into a single cell and then, on the grid's
    /// `Left` request, comes at full size as a **pair** — had a shortcut put
    /// the small copy on the left of the wide cell, the right half would stay
    /// empty and the result would depend on the order of requests. The face
    /// ladder's alias (the bold face falls back to regular) carries the bit
    /// too.
    #[test]
    fn a_shrunk_single_cell_does_not_answer_the_wide_request() {
        let mut a = Atlas::new(None, 13.0, 1.0, Spacing::default());
        for face in [Face::Regular, Face::Bold] {
            let ask = |a: &mut Atlas, half| {
                let (placed, upload) = a.slot(
                    crate::Sprite::Char('漢'),
                    face,
                    crate::SizeClass::Normal,
                    half,
                );
                (placed, upload.is_some())
            };
            let (whole, _) = ask(&mut a, crate::Half::Whole);
            assert_ne!(
                whole.slot,
                crate::TOFU,
                "{face:?}: '漢' should have shrunk to one cell"
            );
            assert_eq!(whole.half, crate::Half::Whole);
            let (left, _) = ask(&mut a, crate::Half::Left);
            assert_eq!(
                left.half,
                crate::Half::Left,
                "{face:?}: the small copy answered the pair"
            );
            assert_ne!(
                left.slot, whole.slot,
                "{face:?}: pair in the single cell's slot"
            );
        }
    }

    /// A two-column emoji does not fit two cells either at @1x (`fit` 1.062
    /// in the two-cell box) and the same branch shrinks it into the
    /// **two-cell** box: it does not go down to one cell, so it fills the two
    /// columns the grid set aside.
    #[test]
    fn wide_emoji_shrinks_into_two_cells_at_1x() {
        let a = Atlas::new(None, 13.0, 1.0, Spacing::default());
        let base = a.faces.get(Face::Regular);
        let alt = rules::fallback_font(base, '😀', a.cell_advance, a.cell_advance, 2)
            .expect("😀 @1x: tofu");
        assert!(alt.shrunk, "did not fit two cells at @1x; should be shrunk");
        assert_eq!(alt.cols, 2, "the shrink must target the two-cell box");
    }

    /// A candidate **just above** the limit stays a box: `🝇` (Apple Symbols,
    /// the smallest `fit` above the limit in the census apart from
    /// `.LastResort`, 2.250). The classification tests the expectation
    /// against the candidate's own `fit`, so if the limit moves the test says
    /// which side it fell on.
    #[test]
    fn just_above_the_limit_stays_tofu() {
        let a = Atlas::new(None, 16.0, 2.0, Spacing::default());
        let base = a.faces.get(Face::Regular);
        match classify(base, '🝇', a.cell_advance, 1) {
            Class::Rejected { fit, .. } => assert!(
                fit > rules::SHRINK_LIMIT && fit < rules::SHRINK_LIMIT * 1.05,
                "🝇 fit {fit:.3}: should be just above the limit"
            ),
            other => panic!("🝇 should stay a box: {other:?}"),
        }
    }

    /// `.LastResort` is within the limit (`fit` 1.660) but is not shrunk
    /// (R3.2): the candidate meets the shrink branch's geometric condition and
    /// is still a box.
    #[test]
    fn last_resort_is_not_shrunk() {
        let a = Atlas::new(None, 16.0, 2.0, Spacing::default());
        let base = a.faces.get(Face::Regular);
        let ch = '\u{E0A0}';
        let candidate = Backend::cascade(base, ch.encode_utf8(&mut [0u8; 4]))
            .expect("CoreText's cascade always answers");
        assert!(
            Backend::is_last_resort(&candidate),
            "U+E0A0 came from another font"
        );
        let glyph = Backend::glyph(&candidate, ch).expect(".LastResort gave no glyph");
        let fit = rules::fit_ratio(
            a.cell_advance,
            Backend::advance(&candidate, glyph),
            Backend::ink(&candidate, glyph),
        );
        assert!(
            fit <= rules::SHRINK_LIMIT,
            "fit {fit:.3} must be within the limit"
        );
        assert!(rules::accept(candidate, glyph, a.cell_advance, a.cell_advance, 1).is_none());
    }

    /// Every candidate that passes the gate today is drawn **bit-for-bit the
    /// same** (R3.3): shrinking is the last branch, so a candidate passing
    /// either gate comes back with the same font (same object, same size),
    /// `shrunk = false` and zero `rise`. All scanned blocks, 16pt @2x; for `⏺`
    /// the raster is additionally compared byte for byte with the pre-041
    /// path (`raster::draw`, the wrapper without `rise`).
    #[test]
    fn gate_accepted_candidates_are_unchanged() {
        let a = Atlas::new(None, 16.0, 2.0, Spacing::default());
        let base = a.faces.get(Face::Regular);
        let (cell, m) = (a.cell_advance, a.metrics);
        let mut passed = 0usize;
        for (_, first, last) in BLOCKS {
            for ch in (first..=last).filter_map(char::from_u32) {
                if raster::is_procedural(ch) || Backend::glyph(base, ch).is_some() {
                    continue;
                }
                let candidate = Backend::cascade(base, ch.encode_utf8(&mut [0u8; 4]))
                    .expect("CoreText's cascade always answers");
                let Some(glyph) = Backend::glyph(&candidate, ch) else {
                    continue;
                };
                let advance = Backend::advance(&candidate, glyph);
                let ink = Backend::ink(&candidate, glyph);
                if !rules::ink_fits_placed(cell, advance, ink) {
                    continue;
                }
                let ptr: *const _ = &*candidate;
                let alt =
                    rules::accept(candidate, glyph, cell, cell, 1).expect("gate-passing rejected");
                assert!(!alt.shrunk, "{ch}: a gate-passing candidate was shrunk");
                assert!(std::ptr::eq(ptr, &*alt.font), "{ch}: font changed");
                assert_eq!(alt.rise(m), 0.0, "{ch}: vertical shift");
                passed += 1;
            }
        }
        assert!(passed > 300, "too few gate-passing candidates: {passed}");

        let alt = rules::fallback_font(base, '⏺', cell, cell, 1).expect("⏺ came out as a box");
        let mut before = vec![0u8; m.slot_bytes()];
        let mut after = vec![0u8; m.slot_bytes()];
        raster::draw(&alt.font, '⏺', m, cell, 0.0, &mut before);
        raster::draw_glyph(&alt.font, alt.glyph, m, cell, 0.0, alt.rise(m), &mut after);
        assert_eq!(before, after, "⏺ raster changed");
    }

    /// Scanned blocks: (name, first, last).
    const BLOCKS: [(&str, u32, u32); 13] = [
        ("Arrows", 0x2190, 0x21FF),
        ("Mathematical Operators", 0x2200, 0x22FF),
        ("Misc Technical", 0x2300, 0x23FF),
        ("Geometric Shapes", 0x25A0, 0x25FF),
        ("Misc Symbols", 0x2600, 0x26FF),
        ("Dingbats", 0x2700, 0x27BF),
        ("Misc Math Symbols-A", 0x27C0, 0x27EF),
        ("Supplemental Arrows-A", 0x27F0, 0x27FF),
        ("Supplemental Arrows-B", 0x2900, 0x297F),
        ("Misc Math Symbols-B", 0x2980, 0x29FF),
        ("Misc Symbols and Arrows", 0x2B00, 0x2BFF),
        ("Emoji (1F300–1FAFF)", 0x1F300, 0x1FAFF),
        ("Private Use Area", 0xE000, 0xF8FF),
    ];
    /// (size, scale) — the four combinations of R1.1.
    const COMBOS: [(f64, f64); 4] = [(13.0, 1.0), (13.0, 2.0), (16.0, 1.0), (16.0, 2.0)];
    /// The histogram's bucket edges: <1.2 / 1.2–1.5 / 1.5–1.7 / 1.7+. The
    /// first bucket also holds values below 1.0: a candidate that overflows
    /// on the left, or sticks to the left and overflows on the right, is
    /// turned back even if its ink is narrower than the cell.
    const EDGES: [f64; 3] = [1.2, 1.5, 1.7];
    fn bucket(v: f64) -> usize {
        EDGES.iter().take_while(|&&e| v >= e).count()
    }

    fn histogram(values: &[f64]) -> String {
        let mut counts = [0usize; 4];
        for &v in values {
            counts[bucket(v)] += 1;
        }
        format!(
            "<1.2: {} · 1.2–1.5: {} · 1.5–1.7: {} · 1.7+: {}",
            counts[0], counts[1], counts[2], counts[3]
        )
    }

    /// By code point if the character is not visible on screen (PUA).
    fn show(ch: char) -> String {
        if ('\u{E000}'..='\u{F8FF}').contains(&ch) {
            format!("U+{:04X}", u32::from(ch))
        } else {
            ch.to_string()
        }
    }

    /// Collects consecutive code points into `first..last` ranges.
    fn spans(cps: &[u32]) -> String {
        let mut out: Vec<String> = Vec::new();
        let mut i = 0;
        while i < cps.len() {
            let mut j = i;
            while j + 1 < cps.len() && cps[j + 1] == cps[j] + 1 {
                j += 1;
            }
            out.push(if i == j {
                format!("{:04X}", cps[i])
            } else {
                format!("{:04X}..{:04X}", cps[i], cps[j])
            });
            i = j + 1;
        }
        out.join(" ")
    }

    fn range(values: &[f64]) -> String {
        let min = values.iter().copied().fold(f64::INFINITY, f64::min);
        let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        format!("{min:.3}..{max:.3}")
    }

    /// A candidate summary: what the per-font breakdown keeps.
    #[derive(Default)]
    struct FontRow {
        accepted: usize,
        shrunk: usize,
        fit2: Vec<f64>,
        rejected: Vec<(char, f64, f64)>,
        wide: usize,
    }

    /// Passes the symbol and emoji blocks through the gate and prints the
    /// group counts, the ratio histogram of rejected candidates and the
    /// per-font breakdown.
    ///
    /// Every character is asked as **single-column** (`cols = 1`): `bt-atlas`
    /// does not see `unicode-width` and the sole authority on width is
    /// `bt-core`'s table. What happens to the rejected and shrunk ones when
    /// asked as two columns is asked separately (`2h`, "fit in a 2-cell
    /// box"): a character declared wide is drawn there, one declared
    /// single-column stays a box or shrinks into one cell. Which is which is
    /// the grid's question.
    ///
    /// Two witnesses are printed for shrinking: the `fit` distribution of the
    /// shrunk ones, and those within the limit whose small copy failed the
    /// re-test (must be zero; otherwise [`rules::SHRINK_LIMIT`]'s derivation is
    /// stale).
    ///
    /// The procedural ranges are skipped: they are drawn without asking a
    /// font. The base family comes from `BT_SCAN_FONT` (else the chain's
    /// default).
    #[test]
    #[ignore = "an inventory that depends on the machine's fonts; `make scan`"]
    fn census() {
        let family = std::env::var("BT_SCAN_FONT").ok();
        let mut out = String::new();
        for (pt, scale) in COMBOS {
            let a = Atlas::new(family.as_deref(), pt, scale, Spacing::default());
            let base = a.faces.get(Face::Regular);
            let cell = a.cell_advance;
            let base_name = crate::coretext::fixture::family_name(base);
            let _ = writeln!(
                out,
                "\n=== {pt}pt @{scale}x — base {base_name}, cell {cell:.3} px ==="
            );
            let _ = writeln!(
                out,
                "{:<26} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6}",
                "block", "base", "fallb", "shrunk", "none", "reject", "(2h)", "(LR)"
            );
            let mut fonts: BTreeMap<String, FontRow> = BTreeMap::new();
            let mut totals = [0usize; 7];
            let mut shrunk_fits: Vec<f64> = Vec::new();
            let mut shrink_failed: Vec<String> = Vec::new();
            // Code points that land on `.LastResort`, excluding PUA (almost
            // all of it lands there): which characters it answers is
            // phase-2's R3.2 question.
            let mut last_resort: Vec<u32> = Vec::new();
            for (name, first, last) in BLOCKS {
                let mut row = [0usize; 7];
                for cp in first..=last {
                    let Some(ch) = char::from_u32(cp) else {
                        continue;
                    };
                    if raster::is_procedural(ch) {
                        continue;
                    }
                    match classify(base, ch, cell, 1) {
                        Class::InBase => row[0] += 1,
                        Class::Fallback { font, .. } => {
                            row[1] += 1;
                            fonts.entry(font).or_default().accepted += 1;
                        }
                        Class::Shrunk { font, fit, .. } => {
                            row[2] += 1;
                            shrunk_fits.push(fit);
                            let entry = fonts.entry(font).or_default();
                            entry.shrunk += 1;
                            if let Class::Rejected { fit, .. } | Class::Shrunk { fit, .. } =
                                classify(base, ch, cell, 2)
                            {
                                entry.fit2.push(fit);
                            }
                        }
                        Class::NoFont => row[3] += 1,
                        Class::Rejected { font, ratio, fit } => {
                            row[4] += 1;
                            let wide_class = classify(base, ch, cell, 2);
                            let wide =
                                !matches!(wide_class, Class::Rejected { .. } | Class::NoFont);
                            if let Class::Rejected { fit, .. } | Class::Shrunk { fit, .. } =
                                wide_class
                            {
                                fonts.entry(font.clone()).or_default().fit2.push(fit);
                            }
                            if wide {
                                row[5] += 1;
                            }
                            if font != LAST_RESORT && fit <= rules::SHRINK_LIMIT {
                                shrink_failed.push(format!("{}({fit:.3})", show(ch)));
                            }
                            if font == LAST_RESORT {
                                row[6] += 1;
                                if first != 0xE000 {
                                    last_resort.push(cp);
                                }
                            }
                            let entry = fonts.entry(font).or_default();
                            entry.rejected.push((ch, ratio, fit));
                            entry.wide += usize::from(wide);
                        }
                    }
                }
                for (t, r) in totals.iter_mut().zip(row) {
                    *t += r;
                }
                let _ = writeln!(
                    out,
                    "{:<26} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6}",
                    name, row[0], row[1], row[2], row[3], row[4], row[5], row[6]
                );
            }
            let _ = writeln!(
                out,
                "{:<26} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6}",
                "TOTAL",
                totals[0],
                totals[1],
                totals[2],
                totals[3],
                totals[4],
                totals[5],
                totals[6]
            );
            let _ = writeln!(
                out,
                "shrunk: fit {} ({}) · within the limit but failed the re-test: {} {}",
                range(&shrunk_fits),
                histogram(&shrunk_fits),
                shrink_failed.len(),
                shrink_failed.join(" ")
            );

            let _ = writeln!(out, ".LastResort (excluding PUA): {}", spans(&last_resort));
            let real: Vec<&(char, f64, f64)> = fonts
                .iter()
                .filter(|(f, _)| f.as_str() != LAST_RESORT)
                .flat_map(|(_, r)| &r.rejected)
                .collect();
            let ratios: Vec<f64> = real.iter().map(|r| r.1).collect();
            let fits: Vec<f64> = real.iter().map(|r| r.2).collect();
            let listed: String = real
                .iter()
                .map(|r| format!("{}({:.3}) ", show(r.0), r.2))
                .collect();
            let _ = writeln!(
                out,
                "rejected (excluding .LastResort, {} candidates): {listed}",
                real.len()
            );
            let _ = writeln!(out, "  ratio {}", histogram(&ratios));
            let _ = writeln!(out, "  fit   {}", histogram(&fits));
            let _ = writeln!(
                out,
                "per font (accepted / shrunk / rejected, 2h = rejected drawn as two columns):"
            );
            for (font, row) in &fonts {
                if row.rejected.is_empty() {
                    let _ = writeln!(
                        out,
                        "  {font}: {} accepted / {} shrunk — fit in a 2-cell box {}",
                        row.accepted,
                        row.shrunk,
                        range(&row.fit2)
                    );
                    continue;
                }
                let r: Vec<f64> = row.rejected.iter().map(|x| x.1).collect();
                let f: Vec<f64> = row.rejected.iter().map(|x| x.2).collect();
                let _ = writeln!(
                    out,
                    "  {font}: {} accepted / {} shrunk / {} rejected (2h {}) — ratio {} · fit {} · \
                     fit in a 2-cell box {}",
                    row.accepted,
                    row.shrunk,
                    row.rejected.len(),
                    row.wide,
                    range(&r),
                    range(&f),
                    range(&row.fit2)
                );
                // The region where the limit is found: the characters with fit < 1.5.
                let near: String = row
                    .rejected
                    .iter()
                    .filter(|x| x.2 < EDGES[1])
                    .map(|x| format!("{}({:.2}/{:.2}) ", show(x.0), x.1, x.2))
                    .collect();
                if !near.is_empty() && font != LAST_RESORT {
                    let _ = writeln!(out, "    fit<1.5: {near}");
                }
            }
        }
        println!("{out}");
    }
}
