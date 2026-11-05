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
    /// A feature the file chose for the font that the font does not list. The
    /// font's engine ignores it without a word, so this is the one place it
    /// shows. `family` is what the file asked for; empty is the default
    /// chain's font.
    NoFeature { family: String, tag: String },
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

/// The frame the grid's text keeps inside its pane, in pixels: how far its
/// first column starts from the left edge, how far its last column may come
/// to the right edge, and how much clear room its bottom row keeps above
/// whatever is under it — the dock's top, or the pane's bottom edge where no
/// dock is drawn.
///
/// **The grid's own value, apart from the dock's margins** (the ruler,
/// [`CellMetrics::side_left_px`]): today [`CellMetrics::with_corners`] builds
/// it from the ruler — the grid's sides are the dock's sides and its foot is
/// the dock's breathing room, so the two surfaces line up on both sides and
/// mirror each other across the dock's top — but a state that wants the text
/// elsewhere (edge to edge, say) changes this one value
/// ([`CellMetrics::with_grid_inset`]) and every grid reader follows. Only in
/// a state **without a dock**: with one, the grid leaving the ruler would
/// stand its text off the dock's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GridInset {
    /// From the pane's left edge to the grid's first column.
    pub left: u16,
    /// The least room between the grid's last column and the pane's right
    /// edge; the columns' sub-cell leftover comes on top of it.
    pub right: u16,
    /// The clear room between the grid's bottom row and what is below it.
    pub foot: u16,
}

impl GridInset {
    /// No frame: the grid starts at the pane's left edge and runs to its
    /// right and bottom edges.
    pub const ZERO: Self = Self {
        left: 0,
        right: 0,
        foot: 0,
    };
}

#[derive(Clone, Copy, Debug)]
pub struct CellMetrics {
    cell_px: (u16, u16),
    context_cell_px: u16,
    gutter_px: u16,
    /// The dock's left and right margins, in pixels — the **horizontal**
    /// ruler, kept apart from the gutter ([`CellMetrics::side_left_px`]). Both
    /// equal the gutter until [`CellMetrics::with_corners`] says the pane's
    /// bottom corners are round.
    side_left_px: u16,
    side_right_px: u16,
    /// The grid's frame ([`GridInset`]): the ruler and the gutter today.
    grid_inset: GridInset,
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

/// The largest corner radius the margin follows, in points: a window's
/// corner is a few tens of points at most, and a host that passes something
/// absurd must not eat the width (it would leave no columns). A cap, not a
/// measurement.
const MAX_CORNER_PT: f64 = 64.0;

/// The side margin for a corner of `corner_pt` points: `max(gutter, corner −
/// gutter)`, in pixels ([`CellMetrics::with_corners`] is the rule's doc). A
/// corner that is not a radius (NaN, infinite, negative) is square and leaves
/// the gutter; one past [`MAX_CORNER_PT`] is capped there. The **one** place
/// that decides either, so the pane and the C host can pass what they have.
fn side_px(gutter_px: u16, corner_pt: f64, scale: f64) -> u16 {
    let gutter = f64::from(gutter_px);
    let corner = if corner_pt.is_finite() {
        corner_pt.clamp(0.0, MAX_CORNER_PT)
    } else {
        0.0
    };
    // audit: at most 64 pt · scale, far inside `u16` at any real scale; the
    // cast saturates at 65535 for a degenerate one.
    (points_px(corner, scale) - gutter).max(gutter) as u16
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
            && self.side_left_px == other.side_left_px
            && self.side_right_px == other.side_right_px
            && self.grid_inset == other.grid_inset
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
            side_left_px: gutter,
            side_right_px: gutter,
            grid_inset: GridInset {
                left: gutter,
                right: gutter,
                foot: gutter,
            },
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

    /// These metrics for a pane whose bottom corners have the given radii, in
    /// **points** (left and right): the horizontal margin follows the corner
    /// instead of the gutter alone.
    ///
    /// The rule is concentric, and this is its **one** owner: the margin is
    /// `max(gutter, corner − gutter)`. The dock's text sits `gutter` above the
    /// pane's bottom edge, so a text edge that keeps the same `gutter` clear
    /// of a concentric curve of radius `corner` stands `corner − gutter` from
    /// the side. A 20.5 pt window corner gives 12.5 pt: the first letter is
    /// `gutter` away from the curve, as far as it is from the straight edge.
    /// A square corner (zero, or a value that is not a radius: NaN, infinite,
    /// negative) and a corner no wider than twice the gutter leave the gutter
    /// itself; a radius past [`MAX_CORNER_PT`] is taken as that.
    ///
    /// The vertical arithmetic is **not** here: the dock's breathing room, the
    /// row gap and the top fade stay on the gutter ([`CellMetrics::gutter_px`]).
    /// Pixels come from the gutter's own conversion ([`points_px`]), so at a
    /// fractional scale the margin and the gutter cannot round apart.
    ///
    /// **The grid's frame is rebuilt from the ruler here** ([`GridInset`]):
    /// its sides are the two margins and its foot the gutter, so the grid's
    /// text keeps the dock's distance from the curve too — its bottom row
    /// stands `gutter` above the pane's bottom edge where no dock is drawn,
    /// which is the height the concentric rule assumes. A state that wants
    /// another frame sets it after this ([`CellMetrics::with_grid_inset`]).
    pub fn with_corners(self, left_pt: f64, right_pt: f64) -> Self {
        let side_left_px = side_px(self.gutter_px, left_pt, self.scale);
        let side_right_px = side_px(self.gutter_px, right_pt, self.scale);
        Self {
            side_left_px,
            side_right_px,
            grid_inset: GridInset {
                left: side_left_px,
                right: side_right_px,
                foot: self.gutter_px,
            },
            ..self
        }
    }

    /// These metrics with the grid's frame replaced: every grid reader — the
    /// columns, the rows, the draw origin, the mouse, the padding's paint —
    /// follows; the dock keeps the ruler. The doc of [`GridInset`] says when
    /// a frame other than the ruler's is sound.
    pub fn with_grid_inset(self, grid_inset: GridInset) -> Self {
        Self { grid_inset, ..self }
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

    /// The dock's left margin — the ruler — in pixels: the gutter on a square
    /// corner, more where the corner is round ([`CellMetrics::with_corners`]).
    /// Every horizontal reader of the dock — its cells, its columns, the
    /// context row, the mouse on it — reads it from here; the grid reads its
    /// own frame ([`CellMetrics::grid_inset`]), which is this ruler today.
    pub fn side_left_px(self) -> u16 {
        self.side_left_px
    }

    /// The dock's right margin, the same rule as [`CellMetrics::side_left_px`]
    /// with the bottom right corner: the input row's columns and the context
    /// row's trailing zone end inside it.
    pub fn side_right_px(self) -> u16 {
        self.side_right_px
    }

    /// The grid's frame ([`GridInset`]): where its first column starts, the
    /// least room after its last one and the room under its bottom row. Every
    /// grid reader — the column and row arithmetic, the draw origin, the
    /// mouse, the padding's paint, the top fade's height — reads it from here.
    pub fn grid_inset(self) -> GridInset {
        self.grid_inset
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The metrics a pane has at `scale` before the corners are known: the
    /// production gutter (8 pt) in pixels, both side margins equal to it.
    fn flat(scale: f64) -> CellMetrics {
        let gutter = points_px(CellMetrics::GUTTER_PT, scale) as u16;
        CellMetrics::new(9, 18, 9, gutter, 1, scale).expect("a cell with no zero component")
    }

    #[test]
    fn the_side_margin_is_the_gutter_until_a_corner_is_told() {
        for scale in [1.0, 2.0] {
            let metrics = flat(scale);
            assert_eq!(metrics.side_left_px(), metrics.gutter_px());
            assert_eq!(metrics.side_right_px(), metrics.gutter_px());
        }
    }

    #[test]
    fn a_corner_gives_the_margin_concentric_with_it() {
        // The macOS 26 window corner: 20.5 − 8 = 12.5 pt, so the first letter
        // stands as far from the curve as from the straight edge (8 pt).
        let retina = flat(2.0).with_corners(20.5, 20.5);
        assert_eq!(retina.gutter_px(), 16);
        assert_eq!(retina.side_left_px(), 25, "12.5 pt at @2x");
        assert_eq!(retina.side_right_px(), 25);
        // At @1x 12.5 pt rounds half up, the way the gutter's own pixels do.
        let plain = flat(1.0).with_corners(20.5, 20.5);
        assert_eq!(plain.gutter_px(), 8);
        assert_eq!(plain.side_left_px(), 13);
        // A card's corner (window less the gap) is 12.5 pt as well.
        assert_eq!(flat(2.0).with_corners(12.5, 12.5).side_left_px(), 16);
    }

    #[test]
    fn a_corner_no_wider_than_twice_the_gutter_leaves_the_gutter() {
        // 12.5 pt − 8 = 4.5 pt is under the gutter; so is the 10 pt corner of
        // the releases before macOS 26 (10 − 8 = 2), and a square one.
        for corner in [12.5, 10.0, 16.0, 0.0] {
            for scale in [1.0, 2.0] {
                let metrics = flat(scale).with_corners(corner, corner);
                assert_eq!(
                    metrics.side_left_px(),
                    metrics.gutter_px(),
                    "{corner} @{scale}"
                );
                assert_eq!(
                    metrics.side_right_px(),
                    metrics.gutter_px(),
                    "{corner} @{scale}"
                );
            }
        }
    }

    #[test]
    fn a_value_that_is_not_a_radius_is_a_square_corner() {
        for corner in [f64::NAN, -20.5, f64::NEG_INFINITY] {
            let metrics = flat(2.0).with_corners(corner, corner);
            assert_eq!(metrics.side_left_px(), 16, "{corner}");
            assert_eq!(metrics.side_right_px(), 16, "{corner}");
        }
        // Infinity is not a radius either.
        assert_eq!(
            flat(2.0).with_corners(f64::INFINITY, 0.0).side_left_px(),
            16
        );
        // A huge finite one is capped (64 pt − 8 pt = 56 pt = 112 px at @2x)
        // instead of eating the width.
        let huge = flat(2.0).with_corners(1.0e6, 1.0e6);
        assert_eq!(huge.side_left_px(), 112);
        assert_eq!(huge.side_right_px(), 112);
        assert_eq!(flat(2.0).with_corners(64.0, 0.0).side_left_px(), 112);
    }

    #[test]
    fn metrics_with_other_margins_are_not_the_same_geometry() {
        // The equality tells "the geometry moved" from "the same again": the
        // margins are geometry.
        let square = flat(2.0);
        assert_ne!(square, square.with_corners(20.5, 0.0));
        assert_ne!(square, square.with_corners(0.0, 20.5));
        assert_eq!(square, square.with_corners(0.0, 0.0));
        assert_eq!(
            square.with_corners(20.5, 20.5),
            flat(2.0).with_corners(20.5, 20.5)
        );
    }

    #[test]
    fn the_two_sides_follow_their_own_corner_and_the_gutter_stays() {
        let metrics = flat(2.0).with_corners(20.5, 0.0);
        assert_eq!(metrics.side_left_px(), 25);
        assert_eq!(metrics.side_right_px(), 16, "a square corner on the right");
        // The vertical readers keep the gutter: the dock's breathing room does
        // not grow with the corner.
        assert_eq!(metrics.gutter_px(), 16);
        assert_eq!(metrics.cell_px(), (9, 18));
    }

    #[test]
    fn the_grid_frame_is_the_ruler_and_the_gutter() {
        // Before the corners: the gutter on every side and under the grid.
        assert_eq!(
            flat(2.0).grid_inset(),
            GridInset {
                left: 16,
                right: 16,
                foot: 16
            }
        );
        // Beside the corners: the dock's margins on the sides, the dock's
        // breathing room under the grid — the two surfaces line up.
        let round = flat(2.0).with_corners(20.5, 0.0);
        let inset = round.grid_inset();
        assert_eq!((inset.left, inset.right), (25, 16));
        assert_eq!(
            (inset.left, inset.right),
            (round.side_left_px(), round.side_right_px())
        );
        assert_eq!(inset.foot, round.gutter_px());
    }

    #[test]
    fn a_grid_frame_of_its_own_leaves_the_ruler() {
        // The hook a dockless state would use: the grid runs edge to edge,
        // the dock's margins stay where the corners put them, and the change
        // is geometry.
        let round = flat(2.0).with_corners(20.5, 20.5);
        let flush = round.with_grid_inset(GridInset::ZERO);
        assert_eq!(flush.grid_inset(), GridInset::ZERO);
        assert_eq!((flush.side_left_px(), flush.side_right_px()), (25, 25));
        assert_eq!(flush.gutter_px(), 16);
        assert_ne!(flush, round);
        // Corners told again rebuild the frame from the ruler.
        assert_eq!(flush.with_corners(20.5, 20.5), round);
    }
}
