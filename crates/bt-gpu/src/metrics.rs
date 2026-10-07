//! Grid geometry ([`CellMetrics`]) and font notices ([`FontNotice`]) —
//! the renderer's answers that `bt-shell` sees without seeing `bt-atlas`.

use bt_atlas::{FontIssue, Metrics};

/// The outcome of a font choice that is to be told to the user — the
/// counterpart of `bt_atlas::FontIssue` **in this crate**.
///
/// A separate type, because `bt-shell` does not see `bt-atlas` and must not
/// (the rationale of [`CellMetrics`]): a re-export would blur the
/// layer table. There is no text; the one that builds the subtitle's string is
/// `bt-shell`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FontNotice {
    /// The requested family is not on the machine; `using` is the name of the family opened.
    FamilyNotFound { requested: String, using: String },
    /// The family opened but is not monospaced; it was not rejected.
    NotMonospaced { family: String },
}

impl From<FontIssue> for FontNotice {
    fn from(issue: FontIssue) -> Self {
        match issue {
            FontIssue::FamilyNotFound { requested, using } => {
                FontNotice::FamilyNotFound { requested, using }
            }
            FontIssue::NotMonospaced { family } => FontNotice::NotMonospaced { family },
        }
    }
}

/// The settings window's question: what would be said if `family` were
/// opened — the status of a family that is not in the list
/// ([`bt_atlas::family_issue`]). Renderer-free, because the window can be open
/// even when there is no terminal window.
pub fn family_notice(family: &str) -> Option<FontNotice> {
    bt_atlas::family_issue(family).map(FontNotice::from)
}

#[derive(Clone, Copy, Debug)]
pub struct CellMetrics {
    cell_px: (u16, u16),
    context_cell_px: u16,
    gutter_px: u16,
    rule_px: u16,
    /// Physical pixels per point — the backing scale the atlas was opened at.
    ///
    /// Carried because the gutter is not the last design number in points:
    /// the scroll bar's sizes are points too ([`CellMetrics::pt_px`]), and
    /// the scale reaches them on **the gutter's road** — from the one place
    /// that knows it ([`crate::Renderer::cell_metrics`]) inside this type. A
    /// scale kept anywhere else could stay at @1x after the window moved to a
    /// Retina display while the cells grew.
    scale: f64,
}

/// A size in **points**, in whole physical pixels at `scale` — the **one**
/// point-to-pixel conversion: the gutter ([`CellMetrics::from_atlas`]) and
/// the scroll bar's sizes ([`CellMetrics::pt_px`]) both go through here, so
/// at a fractional scale the two cannot round apart.
///
/// Rounded, because an edge that does not land on the device grid would
/// spread over two pixels and fade; never negative, so a degenerate scale
/// (NaN, negative) gives zero-sized parts rather than inverted ones.
fn points_px(pt: f64, scale: f64) -> f64 {
    (pt * scale).round().max(0.0)
}

// By hand, because the scale is a float: equal bit for bit. The metrics are
// compared to tell "the geometry moved" from "the same geometry again", and a
// float compared by value would make a NaN scale never equal to itself — a
// metric that changes on every look.
impl PartialEq for CellMetrics {
    fn eq(&self, other: &Self) -> bool {
        self.cell_px == other.cell_px
            && self.context_cell_px == other.context_cell_px
            && self.gutter_px == other.gutter_px
            && self.rule_px == other.rule_px
            && self.scale.to_bits() == other.scale.to_bits()
    }
}

impl Eq for CellMetrics {}

impl CellMetrics {
    /// The width, in **points**, of the left gutter where the command block's
    /// stripe sits; [`crate::Renderer::cell_metrics`] converts it to physical
    /// pixels and that is the **only** place.
    ///
    /// A constant, not a setting: `command_gutter` is
    /// deliberately absent from this set, because a setting applied at save
    /// time forced three consumers to update in the same frame. Its value is a
    /// product decision — the stripe plus breathing room on both sides — and at
    /// a typical point size/scale it takes **at most one** column from `cols`;
    /// it is not a measured number.
    ///
    /// `private`: everyone who reads the gutter **carries** it with
    /// [`CellMetrics`], not from the constant. A second reader would bring back
    /// exactly the divergence the type prevents.
    const GUTTER_PT: f64 = 8.0;

    /// The geometry if the cell size has no zero component, `None` otherwise.
    ///
    /// The fields are `private` but the constructor is `pub`: the guarantee is
    /// provided not by "nobody can construct it" but by **"whoever constructs
    /// it cannot pass a zero"**. The difference shows in testing — `bt-shell`'s
    /// grid arithmetic stays testable without building a Metal device, whereas
    /// a type only `Renderer::cell_metrics` could build would tie those tests
    /// to the GPU. The gutter being an **argument** is a continuation of the
    /// same rationale: a constant hidden in the body would tie the tests that
    /// probe the gutter to the GPU as well. The scale is an argument for the
    /// same reason and without a default: a metric built at @2x for a test
    /// must not draw its point-sized parts at @1x.
    pub fn new(
        width: u16,
        height: u16,
        context_width: u16,
        gutter: u16,
        rule: u16,
        scale: f64,
    ) -> Option<Self> {
        // The gate asks all three at once: the context width is a **divisor**
        // too (`bt-gpu`'s context column budget) and, had a zero passed, the
        // grid's would be caught while its own would slip through silently.
        (width > 0 && height > 0 && context_width > 0).then_some(Self {
            cell_px: (width, height),
            context_cell_px: context_width,
            gutter_px: gutter,
            rule_px: rule,
            scale,
        })
    }

    /// The grid geometry of an atlas at `scale`: cell size, context-row step,
    /// gutter and rule thickness — one copy for both renderers
    /// ([`crate::Renderer::cell_metrics`] and the wgpu renderer's twin).
    pub(crate) fn from_atlas(metrics: Metrics, context_w: u16, scale: f64) -> Self {
        let (w, h) = metrics.cell_px;
        // A NaN or negative scale gives a zero gutter (the grid starts at the
        // edge; `split_into_grid` and mouse mapping both stay correct), and
        // `as u16` stops a huge one at 65535. Rounding need not match the
        // cell's direction: the gutter is subtracted, not divided by, so a
        // one-pixel wobble moves the gutter, not the grid.
        let gutter = points_px(Self::GUTTER_PT, scale) as u16;
        // audit: `bt_atlas::Metrics.cell_px` is a bare `pub` field, so the
        // ≥ 1 guarantee lives one crate away (`rules::round_up` clamps to 1)
        // and the type does not carry it. Building with a struct literal
        // would leave that gap silent; `expect` turns it into a programming
        // error. Not a panic path: PTY reading and parsing never pass here,
        // this is the window-geometry path.
        Self::new(w, h, context_w, gutter, metrics.underline_px.1, scale)
            .expect("bt-atlas clamps the cell size to 1")
    }

    /// (width, height).
    ///
    /// The type is **carried** between `bt-gpu` and `bt-shell`; the tuple is
    /// opened only where the value has to leave the type: when turning into a
    /// number and entering a division (`split_into_grid`, `point_to_cell`, the
    /// wheel's line unit) and when crossing to `bt-core`
    /// (`SessionOptions.cell_px`, `Session::resize` — `bt-core` cannot see
    /// `bt-gpu`, which is the cost of the layer rule). Outside these the tuple
    /// does not circulate; `Frame::clear` takes **the type itself**, because it
    /// reads the origin from it too.
    pub fn cell_px(self) -> (u16, u16) {
        self.cell_px
    }

    /// The width of the gutter reserved on the left; the grid starts **after**
    /// it.
    ///
    /// It can be zero and that is not an error: a geometry whose gutter is zero
    /// means "the grid starts at the edge", and both the subtraction and the
    /// division work correctly with that value. In production zero only comes
    /// out at a degenerate scale ([`crate::Renderer::cell_metrics`]); tests
    /// deliberately give zero where the gutter is not the subject.
    pub fn gutter_px(self) -> u16 {
        self.gutter_px
    }

    /// The thickness of the rule line, in pixels — the font's **own**
    /// underline metric (the second component of
    /// `bt_atlas::Metrics::underline_px`).
    ///
    /// The width of the thin carets (underline, vertical bar) comes from here
    /// and no second design constant is invented — the chevron's thickness is
    /// from the same metric too. When the point size or font changes, the
    /// caret changes with it.
    pub fn rule_px(self) -> u16 {
        self.rule_px
    }

    /// The column step in the dock's context row, in pixels — the small face's
    /// advance width (`bt_atlas::Atlas::context_cell_w`).
    ///
    /// **Width only**: the small glyph is also rasterized into the large slot,
    /// on the large cell's baseline, so the row height and the baseline are
    /// shared. The band arithmetic ([`crate::dock_px`]) therefore does not
    /// change at all — the context row stays in its own band, only its letters
    /// are small and dense.
    ///
    /// Like `cell_px` it is ≥ 1, with the same structural reason: its source is
    /// `rules::round_up`, and on this side of the boundary the field is private.
    pub fn context_cell_px(self) -> u16 {
        self.context_cell_px
    }

    /// The backing scale these metrics were built at, physical pixels per
    /// point.
    pub fn scale(self) -> f64 {
        self.scale
    }

    /// A design size in **points**, in whole physical pixels at this scale —
    /// the gutter's conversion ([`points_px`]), for the sizes drawn outside
    /// the cell grid (the scroll bar).
    pub(crate) fn pt_px(self, pt: f32) -> f32 {
        points_px(f64::from(pt), self.scale) as f32
    }
}
