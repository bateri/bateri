//! Bit-equality witness for the atlas (042, `discussion.md` → Karar 4).
//!
//! Prints one `key<TAB>digest` line for every observable output of the atlas:
//! metrics, the font diagnostic, every `slot()` answer (`Placed`) of a fixed
//! inventory together with its `Upload` bytes (left and right halves), the
//! tofu bitmap and the occupancy counters — across 13/16 pt × @1x/@2x × two
//! line heights and four family requests. A difference names the sprite it is
//! in.
//!
//! It reads **only the public API** of `bt_atlas`, so it stays valid while
//! the crate's internals are rewritten, and it is `#[ignore]`d because its
//! output depends on the fonts installed on this machine: it is not a gate,
//! it is a comparison.
//!
//! # Comparing against the parent commit
//!
//! Nothing is stored — a macOS update that changes the system fonts would
//! turn a stored digest into a false alarm. Run the parent and the working
//! tree on the same machine, at the same time, and diff:
//!
//! ```sh
//! git worktree add /tmp/bt-parent HEAD          # or the commit under test's parent
//! cp crates/bt-atlas/tests/raster_digest.rs /tmp/bt-parent/crates/bt-atlas/tests/
//! (cd /tmp/bt-parent && cargo test -p bt-atlas --release --test raster_digest \
//!     -- --ignored --nocapture) > /tmp/parent.txt
//! cargo test -p bt-atlas --release --test raster_digest -- --ignored --nocapture \
//!     > /tmp/tree.txt
//! diff <(grep -P '\t' /tmp/parent.txt) <(grep -P '\t' /tmp/tree.txt)   # must be empty
//! git worktree remove /tmp/bt-parent
//! ```
//!
//! The copy step is there because the parent may predate this file; the
//! witness itself must be the same on both sides. The digest is FNV-1a 64
//! written out below, not std's hasher, whose output is not stable across
//! Rust releases.
#![cfg(target_os = "macos")]

use bt_atlas::{Atlas, Face, Half, Placed, RuleKind, SizeClass, Spacing, Sprite, Upload};
use std::fmt::Write as _;

/// FNV-1a, 64 bit. Stable by construction: the constants are the algorithm.
struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn bytes(&mut self, data: &[u8]) -> &mut Self {
        for &b in data {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
        self
    }

    fn text(&mut self, s: &str) -> &mut Self {
        // Length prefix: `("ab", "c")` and `("a", "bc")` must not collide.
        self.bytes(&(s.len() as u64).to_le_bytes())
            .bytes(s.as_bytes())
    }
}

const FACES: [Face; 4] = [Face::Regular, Face::Bold, Face::Italic, Face::BoldItalic];

const RULES: [RuleKind; 7] = [
    RuleKind::Single,
    RuleKind::Double,
    RuleKind::Curl,
    RuleKind::Dotted,
    RuleKind::Dashed,
    RuleKind::Strike,
    RuleKind::Chevron,
];

/// Characters missing from the default monospaced font: they come from the
/// system cascade (`⏺` is the U+23FA case the ink gate was rebuilt for).
const FALLBACK: &[char] = &['⏺', '✓', '⚙', 'ℵ', '∮', '⌘', '★', '→', 'Ω', 'λ'];

/// Fallbacks accepted only after shrinking (041); `⧉` is the named case.
const SHRUNK: &[char] = &['⧉', '⟶', '⨁'];

/// Width-2 characters: CJK, fullwidth, and wide-declared-but-narrow ink
/// (`☕`, `！`) that the one-cell gate accepts first.
const WIDE: &[char] = &['中', '漢', 'あ', '한', '！', '☕', '⚡', '丨', '》'];

/// Colour glyphs: wide emoji and a one-column emoji that is shrunk.
const EMOJI: &[char] = &['😀', '🚀', '👍', '🥰', '☺', '❤'];

/// Multi-code-point sequences: flag, ZWJ, skin tone, VS16.
const CLUSTERS: &[&str] = &[
    "🇹🇷",
    "👨\u{200d}👩\u{200d}👧",
    "👍🏽",
    "❤\u{fe0f}",
    "e\u{301}",
];

/// No font has these: unassigned and supplementary private use.
const TOFU_CHARS: &[char] = &['\u{0378}', '\u{10fffd}'];

/// Procedural ranges (021): box drawing, blocks, Braille, terminal graphics.
fn procedural() -> impl Iterator<Item = char> {
    [
        0x2500..=0x257f_u32,
        0x2580..=0x259f,
        0x2800..=0x28ff,
        0x23b8..=0x23bf,
    ]
    .into_iter()
    .flatten()
    .filter_map(char::from_u32)
}

fn ascii() -> impl Iterator<Item = char> {
    (0x20..=0x7e_u32).filter_map(char::from_u32)
}

fn digest_answer(placed: Placed, upload: Option<&Upload<'_>>) -> String {
    let mut h = Fnv::new();
    h.text(&format!("{placed:?}"));
    match upload {
        None => {
            h.text("cached");
        }
        Some(u) => {
            h.text(&format!("{:?} {:?} {:?}", u.origin, u.right, u.plane))
                .bytes(u.bytes);
            if u.right.is_some() {
                h.bytes(u.right_bytes);
            }
        }
    }
    format!("{placed:?}\t{:016x}", h.0)
}

/// Output buffer plus the configuration prefix every line carries.
struct Sink {
    out: String,
    prefix: String,
}

impl Sink {
    fn line(&mut self, key: &str, value: &str) {
        let _ = writeln!(self.out, "{} {key}\t{value}", self.prefix);
    }

    /// Asks for one sprite and, when the gate returned the left half of a
    /// pair, for the right half too — the way `bt-gpu` does.
    fn probe(
        &mut self,
        atlas: &mut Atlas,
        label: &str,
        sprite: Sprite,
        face: Face,
        size: SizeClass,
        want: Half,
    ) {
        let (placed, upload) = atlas.slot(sprite, face, size, want);
        let value = digest_answer(placed, upload.as_ref());
        self.line(&format!("{label} {face:?} {size:?} {want:?}"), &value);
        if want == Half::Left && placed.half == Half::Left {
            let (placed, upload) = atlas.slot(sprite, face, size, Half::Right);
            let value = digest_answer(placed, upload.as_ref());
            self.line(&format!("{label} {face:?} {size:?} Right"), &value);
        }
    }

    fn summary(&mut self, atlas: &Atlas, group: &str) {
        let mut h = Fnv::new();
        h.bytes(atlas.tofu_bitmap());
        let value = format!(
            "texture={:?} occupancy={:?} color={:?} tofu={:016x}",
            atlas.texture_px(),
            atlas.occupancy(),
            atlas.color_occupancy(),
            h.0
        );
        self.line(&format!("{group} summary"), &value);
    }
}

fn char_label(ch: char) -> String {
    format!("U+{:04X}", u32::from(ch))
}

/// One configuration of the atlas. Each inventory group gets a **fresh**
/// atlas so that a capacity limit in one group cannot shift slot numbers in
/// another: a difference then stays in the group that caused it.
fn configuration(out: &mut String, family: Option<&str>, pt: f64, scale: f64, lh: f64) {
    let prefix = format!("{pt}pt@{scale}x lh{lh:.1} family={}", family.unwrap_or("-"));
    let mut sink = Sink {
        out: String::new(),
        prefix,
    };
    let new = || {
        Atlas::new(
            family,
            pt,
            scale,
            Spacing {
                line: lh,
                ..Spacing::default()
            },
        )
    };
    let n = SizeClass::Normal;
    let small = SizeClass::Small;

    let atlas = new();
    sink.line("metrics", &format!("{:?}", atlas.metrics()));
    sink.line("context_cell_w", &atlas.context_cell_w().to_string());
    sink.line("font_issue", &format!("{:?}", atlas.font_issue()));
    for slot in [0, 1, 2, 63, 64, 1000] {
        sink.line(
            &format!("slot_origin {slot}"),
            &format!("{:?}", atlas.slot_origin(slot)),
        );
    }

    // ASCII in four faces, then the small class.
    let mut atlas = new();
    for face in FACES {
        for ch in ascii() {
            sink.probe(
                &mut atlas,
                &char_label(ch),
                Sprite::Char(ch),
                face,
                n,
                Half::Whole,
            );
        }
    }
    for ch in ascii() {
        let s = Sprite::Char(ch);
        sink.probe(
            &mut atlas,
            &char_label(ch),
            s,
            Face::Regular,
            small,
            Half::Whole,
        );
    }
    sink.summary(&atlas, "ascii");

    // Fallback, shrunk fallback, wide, emoji, tofu — four faces and small.
    let mut atlas = new();
    for face in FACES {
        for &ch in FALLBACK.iter().chain(SHRUNK).chain(TOFU_CHARS) {
            sink.probe(
                &mut atlas,
                &char_label(ch),
                Sprite::Char(ch),
                face,
                n,
                Half::Whole,
            );
        }
        for &ch in WIDE.iter().chain(EMOJI) {
            let s = Sprite::Char(ch);
            sink.probe(&mut atlas, &char_label(ch), s, face, n, Half::Left);
            sink.probe(&mut atlas, &char_label(ch), s, face, n, Half::Whole);
        }
    }
    for &ch in FALLBACK.iter().chain(SHRUNK).chain(WIDE).chain(EMOJI) {
        let s = Sprite::Char(ch);
        sink.probe(
            &mut atlas,
            &char_label(ch),
            s,
            Face::Regular,
            small,
            Half::Left,
        );
    }
    sink.summary(&atlas, "fallback");

    // Clusters, interned the way the grid does it.
    let mut atlas = new();
    for text in CLUSTERS {
        let sprite = atlas.intern(text);
        let label = text.chars().map(char_label).collect::<Vec<_>>().join("+");
        sink.line(&format!("intern {label}"), &format!("{sprite:?}"));
        for size in [n, small] {
            sink.probe(&mut atlas, &label, sprite, Face::Regular, size, Half::Left);
        }
        sink.probe(&mut atlas, &label, sprite, Face::Bold, n, Half::Left);
    }
    sink.summary(&atlas, "cluster");

    // Procedural family (face-independent, closed in the small class).
    let mut atlas = new();
    for ch in procedural() {
        let s = Sprite::Char(ch);
        sink.probe(
            &mut atlas,
            &char_label(ch),
            s,
            Face::Regular,
            n,
            Half::Whole,
        );
    }
    for ch in ['─', '█', '⠋', '⎿'] {
        let s = Sprite::Char(ch);
        sink.probe(&mut atlas, &char_label(ch), s, Face::Bold, n, Half::Whole);
        sink.probe(
            &mut atlas,
            &char_label(ch),
            s,
            Face::Regular,
            small,
            Half::Whole,
        );
    }
    sink.summary(&atlas, "procedural");

    // Rules (face- and size-independent, always one cell).
    let mut atlas = new();
    for rule in RULES {
        let label = format!("{rule:?}");
        let s = Sprite::Rule(rule);
        sink.probe(&mut atlas, &label, s, Face::Regular, n, Half::Whole);
        sink.probe(&mut atlas, &label, s, Face::Bold, small, Half::Left);
    }
    sink.summary(&atlas, "rules");

    out.push_str(&sink.out);
}

/// The whole witness as one string.
fn witness() -> String {
    let mut out = String::new();
    // Free functions of the public API first.
    let families = bt_atlas::monospaced_families();
    let mut h = Fnv::new();
    for f in &families {
        h.text(f);
    }
    let _ = writeln!(
        out,
        "monospaced_families\tcount={} {:016x}",
        families.len(),
        h.0
    );
    for family in ["Menlo", "Helvetica", "NoSuchFamilyBateri"] {
        let _ = writeln!(
            out,
            "family_issue {family}\t{:?}",
            bt_atlas::family_issue(family)
        );
    }

    // Default chain, an explicit monospaced family, a proportional family and
    // a missing one (falls back to the chain and reports it).
    let families: [Option<&str>; 4] = [
        None,
        Some("Menlo"),
        Some("Helvetica"),
        Some("NoSuchFamilyBateri"),
    ];
    for family in families {
        for pt in [13.0, 16.0] {
            for scale in [1.0, 2.0] {
                for lh in [1.0, 1.2] {
                    configuration(&mut out, family, pt, scale, lh);
                }
            }
        }
    }
    out
}

#[test]
#[ignore = "depends on installed fonts; compared against the parent commit, see the header"]
fn raster_digest() {
    let first = witness();
    print!("{first}");
    // Determinism inside one process: two fresh passes must agree, or the
    // parent-vs-tree diff would be noise.
    let second = witness();
    assert!(
        first == second,
        "two consecutive witness passes differed: the witness is not deterministic"
    );
}
