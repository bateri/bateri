//! The font system seam: what the atlas asks a platform's font stack.
//!
//! Every method is a **primitive** — it asks the font and applies no rule.
//! The rules (the ink gate, the shrink arm, the cell formula, the requested
//! family check, the face ladder) live on the platformless side (`rules`) and
//! reach the font only through this trait, so the arithmetic and its order
//! exist once for every backend.
//!
//! Static dispatch through a `cfg` alias ([`Backend`]), not a generic
//! `Atlas` and not `dyn`: a process has one font system, and `bt-gpu` holds a
//! plain `Atlas`.

use crate::raster::DrawResult;
use crate::rules::{Face, InkRect, Metrics, RawMetrics};

/// The primitives a platform's font stack answers.
///
/// Glyph numbers are a concrete `u32`: every backend's glyph index widens to
/// it losslessly (CoreText's `CGGlyph` is `u16`). Sizes are in **pixels**
/// (point size × display scale, 72 dpi); measurements are fractional pixels.
pub(crate) trait FontSystem {
    /// An opened font at one size. `Clone`, because the face ladder copies
    /// the regular face into the slots of the faces it cannot acquire.
    type Font: Clone;

    /// Opens the named family and returns it **together** with the family
    /// name the font system actually gave. Font systems do not fail here —
    /// they hand back the closest font — so the requested-family check is the
    /// caller's (`rules::open_chain`).
    fn open(name: &str, size: f64) -> (Self::Font, String);

    /// The backend's own default chain, used when no family is requested or
    /// the requested one is missing, with the name of the family it opened.
    fn open_default(size: f64) -> (Self::Font, String);

    /// Derives `face` from `regular`; `None` unless the returned font really
    /// carries the requested style (a silent substitution is not a face).
    /// Never called with [`Face::Regular`].
    fn derive(regular: &Self::Font, face: Face) -> Option<Self::Font>;

    /// Does the font system consider the font monospaced — the single
    /// criterion behind both the chain's warning and the settings list.
    fn is_monospaced(font: &Self::Font) -> bool;

    /// Candidate family names for the settings window's Font list — a
    /// pre-filter only; the last word is the platformless filter
    /// (`rules::monospaced_families`).
    fn families() -> Vec<String>;

    /// The character's glyph number; `None` for `.notdef`.
    fn glyph(font: &Self::Font, ch: char) -> Option<u32>;

    /// A glyph's horizontal advance, **fractional**.
    fn advance(font: &Self::Font, glyph: u32) -> f64;

    /// A glyph's **ink** box relative to the origin on the baseline, `y`
    /// growing upwards, **fractional**. Separate from [`Self::advance`]
    /// because the two diverge on symbol fonts and the gate measures the ink.
    fn ink(font: &Self::Font, glyph: u32) -> InkRect;

    /// The font's raw vertical measurements — the input of the cell formula.
    fn raw_metrics(font: &Self::Font) -> RawMetrics;

    /// The font the system's cascade suggests for `text`; `None` when no
    /// candidate exists at all.
    fn cascade(base: &Self::Font, text: &str) -> Option<Self::Font>;

    /// The same font at another size (the shrink arm's copy).
    fn at_size(font: &Self::Font, size: f64) -> Self::Font;

    /// The font's size, pixels.
    fn size(font: &Self::Font) -> f64;

    /// Is this the backend's last-resort font, whose glyph is a
    /// representative box rather than the character — kept out of the shrink
    /// arm, because shrunk it would still be a box.
    fn is_last_resort(font: &Self::Font) -> bool;

    /// Does the font draw **coloured** glyphs — the plane decision.
    fn has_color_glyphs(font: &Self::Font) -> bool;

    /// Shapes `text` (a grapheme cluster) from `base` through the cascade
    /// into a **single** glyph, and gives the font that actually produces it;
    /// `None` if it is not exactly one real glyph. The gate is the caller's.
    fn shape(base: &Self::Font, text: &str) -> Option<(Self::Font, u32)>;

    /// Draws the glyph's coverage into `target` (`R8`, one full slot).
    ///
    /// The position is **computed** by the caller (`raster::draw_glyph`): `x`
    /// is the glyph origin's distance from the slot's left edge, `baseline`
    /// the baseline's height above the slot's **bottom** edge. The backend
    /// only paints, so the centring formula cannot leak into it. The buffer is
    /// cleared only if drawing actually happens.
    fn draw_mask(
        font: &Self::Font,
        glyph: u32,
        m: Metrics,
        x: f64,
        baseline: f64,
        target: &mut [u8],
    ) -> DrawResult;

    /// Draws the glyph's colour pixels into `target` (`RGBA8`, sRGB,
    /// **premultiplied**, one full slot), positioned as [`Self::draw_mask`].
    /// Undoing the premultiplication is the caller's
    /// (`raster::draw_color_glyph`).
    fn draw_color(
        font: &Self::Font,
        glyph: u32,
        m: Metrics,
        x: f64,
        baseline: f64,
        target: &mut [u8],
    ) -> DrawResult;
}

/// The font system of the platform being built.
#[cfg(target_os = "macos")]
pub(crate) type Backend = crate::coretext::CoreText;

/// The font system of the platform being built.
#[cfg(target_os = "linux")]
pub(crate) type Backend = crate::freetype::FreeType;

/// An opened font of [`Backend`].
pub(crate) type Font = <Backend as FontSystem>::Font;

/// Backend-specific sample characters and family names for the
/// platformless tests.
#[cfg(all(any(test, feature = "fixture"), target_os = "macos"))]
pub use crate::coretext::fixture;

/// Backend-specific sample characters and family names for the
/// platformless tests.
#[cfg(all(any(test, feature = "fixture"), target_os = "linux"))]
pub use crate::freetype::fixture;
