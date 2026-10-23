//! The grid a pane's pixels make: how many columns and rows of which cell fit a pane of a given
//! size, after the left gutter, the dock's share, the scroll bar's track and the top edge's
//! reserve — and how big the dock's share is at a given moment. Pure arithmetic every host of a
//! terminal pane asks the same way, so it stands here once rather than in a platform shell.

use bt_gpu::{CellMetrics, ContextStrip};

/// The grid derived from the window geometry + the cell size.
///
/// Its name is not `Metrics`: the owner of the cell metrics is now `bt-gpu`
/// ([`CellMetrics`]) and the two types are read side by side in this file.
/// Here "how many columns how many rows **and** with which cell", there only the cell.
///
/// Open to the `view` module too: mouse translation wants the same triple and
/// the derivation stays here once instead of being repeated at two call sites.
#[derive(Clone, Copy)]
pub struct Grid {
    pub cols: u16,
    pub rows: u16,
    /// The dock's width, columns: the strip between the dock's two margins
    /// (the ruler), with no scroll bar reserve taken off — the always-up
    /// bar's track stops at the dock's top, and the dock below it stays full
    /// width. Equal to `cols` while the grid's frame is the ruler and the
    /// track is no wider than the right margin. The link's dock readings and
    /// the mouse's dock hit come from here ([`split_into_grid`] is the owner).
    pub dock_cols: u16,
    /// The dock's context row: its column budget and the leftover pixels that
    /// go before its trailing zone. The budget counts the strip between the
    /// two side margins in the row's small step, so the zone ends flush with
    /// the right margin; [`split_into_grid`] is the only place either is
    /// computed — the link's budget, the frame's offset and the mouse's
    /// columns all read this value.
    pub context: ContextStrip,
    /// Not a tuple but `CellMetrics`: the metrics pass from here to
    /// `DisplayLink::resize` as they are. The value stored in `Grid` alone
    /// descends to a tuple when it enters `SessionOptions` — the tuple that
    /// `split_into_grid` splits is another value: the **incoming** metrics
    /// enter it, `Grid` is born after it.
    pub cell: CellMetrics,
}

/// Pixel geometry + grid metrics → grid.
///
/// It stands apart from `TerminalPane::sync_geometry` because this is the
/// only pure piece; the rest is window and layer, i.e. untestable. The
/// metrics are an **argument**: a constant hidden in this body would make `cell_metrics_come_from_outside` fail.
///
/// The scope is this much, no more: the line where `CELL_PX` really stood was
/// the `cell_metrics(scale)` call in `sync_geometry` and it is not tested
/// because it wants a window and a Metal device. `CellMetrics::new` is
/// deliberately `pub`, so a placeholder like `CellMetrics::new(9, 18, 7, 8,
/// 1, 1.0)` written there would revive and the two tests here would stay green.
///
/// **The grid's frame is subtracted on three sides** ([`CellMetrics::grid_inset`]):
/// the left from the columns, so the stripe does not overlap the text; the
/// right from the columns, so the last column stops where the dock's text
/// does instead of running to the pane's edge with whatever sub-cell
/// leftover the width gives (that leftover now comes on top of the right
/// side, inside the dock's line); and the foot from the rows, so the bottom
/// row keeps the dock's breathing room above the dock — or above the pane's
/// edge where a program reading the keyboard itself has the band step aside,
/// or where there is no dock. The frame is always reserved — the accepted
/// cost is that the left stays empty in a session without integration
/// (bash/fish, `shell.integration = false`, SSH); the alternative was a
/// SIGWINCH at the first prompt and three consumers being updated at once.
/// Beside a round bottom corner the sides are wider (the ruler,
/// [`CellMetrics::side_left_px`]), and the dock's width follows the same two
/// margins ([`Grid::dock_cols`]) — the dock's own vertical arithmetic (its
/// share, the top reserve) stays on the gutter.
///
/// **The dock share is subtracted from the rows** and, unlike the left
/// gutter, is **conditional**: the dock exists only in an integrated zsh
/// session and the decision is made while the session is born
/// (`TerminalPane::start`). Reserving the share unconditionally would take
/// two rows for no reason from a window without a dock — a cost not
/// comparable with the left gutter's eight points.
///
/// **The share varies during the run**: it drops to zero on the
/// alternate screen and returns to its birth value on exit (`dock_rows_for`,
/// `TerminalPane::alt_screen_did_change`). The cost of varying is one `TIOCSWINSZ` and that cost is paid **per
/// transition, not per command** — commands like `git log` that do not enter
/// the alternate screen never move the flag, so this function is not called again either.
///
/// **The scroll bar's reserve takes the grid's right side** (`reserve_px`,
/// [`ScrollbarMode::reserve_px`]) — the larger of the two, not their sum: in
/// the always-up form the track stands at the window's right edge down to the
/// dock's top, and a track wider than the right side is the room the text
/// keeps from the edge there. The dock does not give it up — the dock's
/// column count is a separate answer ([`Grid::dock_cols`]). The reserve is a function of the
/// form alone, never of the history or the alternate screen: a reserve that
/// came and went with them would resize the grid on the first line into
/// history, every clear and every full-screen program.
///
/// **The top edge's reserve is subtracted from the rows** (`top_px`,
/// [`bt_gpu::edge_reserve_px`]): with the content fading at the pane's top,
/// that much of the height is kept free above the grid **even when the height
/// divides into rows**, and the leftover goes **to the top** — the grid sits
/// its foot above the dock's share, a row at rest never enters the fade, and the drawing
/// side fades the whole leftover ([`bt_gpu::edge_drawn_px`]). Its source is the
/// left margin, so changing the margin moves the row count too. The cost is a
/// row less at the heights whose leftover is shorter than the reserve; zero
/// where the content is cut instead, every row the height allows as before.
/// Like the scroll bar's reserve it is a function of the mode and the cell
/// alone — never of the history or the alternate screen.
///
/// **The dock's context row keeps the right margin from the edge** ([`Grid::context`]):
/// the grid and the input row end where their whole columns end, the
/// sub-column leftover after them inside the right margin, but the context
/// row's ports, indicator and buttons are right-aligned, so they end on the
/// margin itself whatever the width — its own small-step leftover goes before
/// its trailing zone.
pub fn split_into_grid(
    width_px: f64,
    height_px: f64,
    cell: CellMetrics,
    dock_rows: u16,
    reserve_px: f32,
    top_px: f32,
) -> Grid {
    let (cell_w, cell_h) = cell.cell_px();
    // `as u16` saturates in f64 (NaN and negative → 0, large → 65535) and the
    // truncation is exactly the floor rounding we want; `Session::resize`
    // already ignores zero columns/rows (a minimized window). The divisor
    // cannot be zero and the type carries this: `CellMetrics`'s field is
    // private and its constructor (`CellMetrics::new`) rejects zero; its
    // source in production is `Renderer::cell_metrics`, and the guarantee of the ratio is `bt-atlas`'s ≥ 1 clamp.
    //
    // The subtraction is **in `f64`** and this is not a preference but a
    // requirement: in a window narrower than the margin the difference goes
    // negative, the division stays negative and `as u16` saturates it to zero
    // — i.e. the existing behavior (zero columns, `Session::resize` ignores
    // it) is preserved. Had the same subtraction been done in `u16` it would
    // **overflow** and produce a column count near 65535, a `TIOCSWINSZ` of
    // that size. No new lower bound is deliberately introduced: the end of the chain is already right.
    let inset = cell.grid_inset();
    // The right side is subtracted the same way and for the same reason: in a
    // window narrower than the frame the difference goes negative and
    // saturates to zero columns. The always-up track takes the right side's
    // place where it is wider. The width here is unrounded and the drawn
    // track's left edge is the texture's rounded width minus the reserve; the
    // text still ends at or before it, because where the text ends — the left
    // side plus whole cells — is a whole pixel not past `width − reserve`, so
    // not past its floor either.
    let right = f64::from(inset.right).max(f64::from(reserve_px));
    let grid_width = width_px - f64::from(inset.left) - right;
    // The dock spans the ruler, never the track: the bar stops at its top.
    let dock_width = width_px - f64::from(cell.side_left_px()) - f64::from(cell.side_right_px());
    // The dock share is also **in `f64`** and for the same reason: in a window
    // shorter than the dock the difference goes negative, the division stays
    // negative and `as u16` saturates it to zero — `Session::resize` already
    // ignores that size. Done in `u16` it would overflow and produce a
    // 65535-row `TIOCSWINSZ`. The formula is **bt-gpu's** ([`bt_gpu::dock_px`]):
    // the dock's share carries two breathing margins next to the rows and were
    // it rewritten here it would diverge for one frame on resize — the same
    // discipline as consuming `DOCK_ROWS`, no second copy is kept.
    //
    // The top edge's reserve and the grid's foot are subtracted the same way:
    // pixel heights like the dock's share, and the window that cannot hold
    // them gets no rows.
    let usable_height = height_px
        - f64::from(bt_gpu::dock_px(dock_rows, cell))
        - f64::from(top_px)
        - f64::from(inset.foot);
    Grid {
        cols: (grid_width / f64::from(cell_w)) as u16,
        rows: (usable_height / f64::from(cell_h)) as u16,
        dock_cols: (dock_width / f64::from(cell_w)) as u16,
        context: context_strip(width_px, cell),
        cell,
    }
}

/// The pane size, in pixels, nearest to `width_px` × `height_px` at which
/// [`split_into_grid`] leaves nothing over — its inverse: whole columns
/// between the grid's two sides (the always-up track in the right side's
/// place where wider) and whole rows between the top edge's reserve, the
/// grid's foot and the dock's share. What a window resizing a whole cell at
/// a time asks of its lone pane: at the fitted size the text keeps exactly
/// the frame on every side and the top's strip its least height.
///
/// The nearest whole count, so a dragged edge stays within half a cell of the
/// pointer; never under one column or one row. The arguments are
/// [`split_into_grid`]'s, so the two cannot disagree about the frame.
pub fn fitted_size(
    width_px: f64,
    height_px: f64,
    cell: CellMetrics,
    dock_rows: u16,
    reserve_px: f32,
    top_px: f32,
) -> (f64, f64) {
    let (cell_w, cell_h) = cell.cell_px();
    let inset = cell.grid_inset();
    let sides = f64::from(inset.left) + f64::from(inset.right).max(f64::from(reserve_px));
    let ends =
        f64::from(bt_gpu::dock_px(dock_rows, cell)) + f64::from(top_px) + f64::from(inset.foot);
    // `f64::max` keeps the number over a NaN: a degenerate size fits one cell.
    let whole = |length: f64, fixed: f64, step: u16| {
        let step = f64::from(step);
        fixed + ((length - fixed) / step).round().max(1.0) * step
    };
    (
        whole(width_px, sides, cell_w),
        whole(height_px, ends, cell_h),
    )
}

/// The context row's strip in `width_px` wide pane: whole small-step columns
/// between the two side margins, and what is left over.
///
/// The width is **rounded** first, because the drawn surface is
/// (`Surface::set_size`): the zone's right edge is then exactly the surface's
/// width minus the right margin, not a fraction of a pixel off it. Everything
/// is `f64` for the reason `split_into_grid` gives: a window narrower than the
/// margins goes negative and saturates to zero columns, instead of
/// overflowing a `u16` subtraction. Unlike the grid's columns this strip does
/// **not** give the always-up scroll bar's track up — the dock below the bar
/// stays full width.
///
/// The leftover is below one column by construction, and zero where nothing
/// fits (a degenerate or saturated width).
fn context_strip(width_px: f64, cell: CellMetrics) -> ContextStrip {
    let strip = width_px.round() - f64::from(cell.side_left_px()) - f64::from(cell.side_right_px());
    let step = f64::from(cell.context_cell_px());
    // `as u16` saturates: a negative or NaN strip is zero columns.
    let cols = (strip / step) as u16;
    let offset = if cols > 0 && cols < u16::MAX {
        // audit: the remainder is below `step`, itself a `u16`.
        (strip - f64::from(cols) * step) as u16
    } else {
        0
    };
    ContextStrip {
        cols,
        offset_px: offset,
    }
}

/// This moment's dock share: **zero** on the alternate screen, otherwise the birth value
/// — except a **remote** session's alternate screen, where the dock stays as the
/// one-row status bar (`⇄ host`, the transfer line): vim on the server still
/// shows where it runs. One row is exactly the context band (`band_px(0) ==
/// dock_px(1)`), so the remote app's grid is not offset.
///
/// The birth value is a separate input and this is mandatory: in a session
/// without integration (`birth == 0`) leaving the alternate screen must not
/// **give birth** to a dock. Had a single `dock_rows` field been written over,
/// one would have to rebuild from the `DOCK_ROWS` constant, and that is exactly the way to conjure a dock that does not exist.
///
/// Pure: this is `bt-shell-macos`'s only half testable without AppKit.
pub fn dock_rows_for(alt_screen: bool, remote: bool, birth: u16) -> u16 {
    match (alt_screen, remote) {
        _ if birth == 0 => 0,
        (true, true) => 1,
        (true, false) => 0,
        (false, _) => birth,
    }
}

/// The rows of the dock's band that the **connection-lost veil** leaves clear: the
/// context row, alone, whatever the dock holds above it. The veil dims what is a picture
/// of a moment that has passed — the grid and, on a server's terminal that has the dock,
/// the line typed into its input row — and the context row at the bottom keeps saying what
/// is going on (`Reconnecting…`), at full clarity.
///
/// A count of rows the pane's PTY share is made of ([`bt_gpu::dock_px`]'s argument), so the
/// veil's frame and the dock's own band stay one measure: the context row, and none when the
/// pane has no dock (`dock_rows == 0`). A share that is already that one row (a remote
/// program on the alternate screen) is left as it is.
pub fn veil_clear_rows(dock_rows: u16) -> u16 {
    dock_rows.min(1)
}

#[cfg(test)]
mod tests {
    use bt_core::ContentEdge;
    use bt_gpu::{DOCK_ROWS, ScrollbarMode};

    use super::*;

    /// Grid metrics; the gutter is an **argument**, because `split_into_grid` is asked two
    /// separate things: the cell split (gutter zero) and the gutter's deduction from columns.
    fn metrics(w: u16, h: u16, gutter: u16) -> CellMetrics {
        CellMetrics::new(w, h, w, gutter, 1, 1.0).expect("non-zero cell")
    }

    /// A window without a dock: the state of an unintegrated session (and of the smoke recipe).
    /// Tests that query column and row arithmetic get this so the dock
    /// gutter does not mix into the numbers they expect; the gutter's own test
    /// is below and names `DOCK_ROWS` explicitly.
    const NO_DOCK: u16 = 0;

    /// No scroll bar reserve: the self-hiding forms' grid, and the one every
    /// test that is not about the reserve asks about.
    const NO_RESERVE: f32 = 0.0;

    /// No top edge reserve: the content cut at the top, every row the height
    /// allows — the row arithmetic every test that is not about the fade asks
    /// about.
    const NO_TOP: f32 = 0.0;

    /// The veil stops where the context row starts: for the dock's whole share (the input
    /// row and the context row) it leaves exactly the context row's height, and it
    /// leaves none where there is no dock to say anything.
    #[test]
    fn the_veil_leaves_the_context_row_clear_and_nothing_more() {
        let cell = metrics(9, 18, 6);
        let context_row = bt_gpu::dock_px(1, cell);
        assert!(context_row > 0.0);
        assert!(
            bt_gpu::dock_px(DOCK_ROWS, cell) > context_row,
            "the share is more than the context row, or this proves nothing"
        );
        // The dock's share, its alternate-screen one-row form and none.
        assert_eq!(
            bt_gpu::dock_px(veil_clear_rows(DOCK_ROWS), cell),
            context_row
        );
        assert_eq!(bt_gpu::dock_px(veil_clear_rows(1), cell), context_row);
        assert_eq!(bt_gpu::dock_px(veil_clear_rows(0), cell), 0.0);
        // Whatever `dock_rows_for` gives a remote pane, in or out of a full-screen program.
        for alt in [false, true] {
            let rows = dock_rows_for(alt, true, DOCK_ROWS);
            assert!(bt_gpu::dock_px(veil_clear_rows(rows), cell) <= context_row);
        }
    }

    #[test]
    fn cell_metrics_come_from_outside() {
        // The testable form of the placeholder being dead: same window, two
        // different cell sizes, two different grids. A constant leaking back into the body
        // would make the two equal and this test would fail.
        // Gutter zero: what is asked is that the cell size determines the grid, not the gutter's
        // effect. The gutter's own test is `the_gutter_costs_columns`.
        let narrow = split_into_grid(900.0, 600.0, metrics(9, 18, 0), NO_DOCK, NO_RESERVE, NO_TOP);
        let wide = split_into_grid(
            900.0,
            600.0,
            metrics(18, 36, 0),
            NO_DOCK,
            NO_RESERVE,
            NO_TOP,
        );
        assert_eq!((narrow.cols, narrow.rows), (100, 33));
        assert_eq!((wide.cols, wide.rows), (50, 16));
    }

    #[test]
    fn the_frame_costs_columns_on_both_sides_and_a_row_at_the_foot() {
        // The grid's frame is deducted on three sides: the left so the stripe
        // does not sit on top of the text, the right so the last column stops
        // where the dock's text does, the foot so the bottom row keeps the
        // dock's breathing room. 900 × 600 pixels, 9×18 cells → 100 columns and
        // 33 rows with no frame; an 8-pixel frame leaves `884 / 9 = 98`
        // columns and `592 / 18 = 32` rows.
        let plain = split_into_grid(900.0, 600.0, metrics(9, 18, 0), NO_DOCK, NO_RESERVE, NO_TOP);
        let framed = split_into_grid(900.0, 600.0, metrics(9, 18, 8), NO_DOCK, NO_RESERVE, NO_TOP);
        assert_eq!((plain.cols, plain.rows), (100, 33));
        assert_eq!(framed.cols, 98, "the two sides take two columns");
        assert_eq!(framed.rows, 32, "the foot takes a row");
        // The top edge's reserve, which in `Fade` is the gutter again, is its own
        // parameter (`top_px`, zero here) and its own test
        // (`the_fade_keeps_the_margin_free_at_the_top_and_takes_the_leftover`).
        // The frame travels with the metrics: the value that built the grid
        // gives it back and the draw origin and mouse mapping read the same value.
        assert_eq!(
            framed.cell.grid_inset(),
            bt_gpu::GridInset {
                left: 8,
                right: 8,
                foot: 8
            }
        );
    }

    #[test]
    fn the_grid_ends_inside_the_docks_right_margin_at_every_width() {
        // A 20.5 pt corner at @2x, 17 px cells: 25 px of margin each side. The
        // grid's last column never passes the dock's right margin, and what the
        // width leaves under a column stays inside it — the room after the
        // text is the margin plus less than a cell, never less than the margin.
        let cell = CellMetrics::new(17, 40, 13, 16, 1, 2.0)
            .expect("non-zero cell")
            .with_corners(20.5, 20.5);
        for width in 1700_u32..1800 {
            let g = split_into_grid(
                f64::from(width),
                1200.0,
                cell,
                DOCK_ROWS,
                NO_RESERVE,
                NO_TOP,
            );
            let text_end = 25 + u32::from(g.cols) * 17;
            assert!(text_end + 25 <= width, "{width}: past the margin");
            assert!(
                width - text_end - 25 < 17,
                "{width}: a whole column left out"
            );
            // The dock's input row counts the same strip.
            assert_eq!(g.dock_cols, g.cols, "{width}");
        }
        // The window the measure was taken in: 900 pt is 104 columns to the
        // edge but 102 inside the margin, 16 px of leftover after them.
        let g = split_into_grid(1800.0, 1200.0, cell, DOCK_ROWS, NO_RESERVE, NO_TOP);
        assert_eq!(g.cols, 102);
        assert_eq!(1800 - 25 - u32::from(g.cols) * 17, 25 + 16);
    }

    #[test]
    fn the_fitted_size_leaves_nothing_over_and_is_the_nearest() {
        // The measure's pane: a 20.5 pt corner at @2x, 17×40 cells, the fade's
        // 16 px reserve, the dock's share. At every size the fitted one cuts
        // into whole columns and rows with nothing left — the text keeps the
        // frame exactly and the fade its reserve — within half a cell of the
        // size asked for.
        let cell = CellMetrics::new(17, 40, 13, 16, 1, 2.0)
            .expect("non-zero cell")
            .with_corners(20.5, 20.5);
        let top = bt_gpu::edge_reserve_px(ContentEdge::Fade, cell);
        for (width, height) in [
            (1800.0, 1120.0),
            (1802.0, 1140.0),
            (1808.0, 1158.0),
            (1816.0, 1100.0),
        ] {
            let (w, h) = fitted_size(width, height, cell, DOCK_ROWS, NO_RESERVE, top);
            let g = split_into_grid(w, h, cell, DOCK_ROWS, NO_RESERVE, top);
            assert_eq!(
                w,
                25.0 + f64::from(g.cols) * 17.0 + 25.0,
                "{width}: a column's leftover"
            );
            let ends = f64::from(bt_gpu::dock_px(DOCK_ROWS, cell)) + f64::from(top) + 16.0;
            assert_eq!(
                h,
                ends + f64::from(g.rows) * 40.0,
                "{height}: a row's leftover"
            );
            assert!((w - width).abs() <= 8.5, "{width} → {w}: not the nearest");
            assert!(
                (h - height).abs() <= 20.0,
                "{height} → {h}: not the nearest"
            );
            // The fade is its reserve and nothing more.
            let fade = bt_gpu::edge_drawn_px(ContentEdge::Fade, h as f32, DOCK_ROWS, g.rows, cell);
            assert_eq!(fade, top, "{height}");
        }
        // The always-up track takes the right side's place where it is wider.
        let reserve = ScrollbarMode::Always.reserve_px(cell);
        let (w, _) = fitted_size(1800.0, 1120.0, cell, DOCK_ROWS, reserve, top);
        let g = split_into_grid(w, 1120.0, cell, DOCK_ROWS, reserve, top);
        assert_eq!(w, 25.0 + f64::from(g.cols) * 17.0 + f64::from(reserve));
        // Never under a cell, whatever is asked.
        let tiny = fitted_size(0.0, 0.0, cell, DOCK_ROWS, NO_RESERVE, top);
        assert_eq!(tiny.0, 25.0 + 17.0 + 25.0);
        assert!(
            fitted_size(f64::NAN, 1120.0, cell, DOCK_ROWS, NO_RESERVE, top)
                .0
                .is_finite()
        );
    }

    #[test]
    fn a_grid_frame_of_its_own_moves_the_grid_and_not_the_dock() {
        // The hook a dockless state would use: with no frame the grid runs to
        // the edges and keeps no foot, the dock's columns stay on the ruler.
        let ruled = CellMetrics::new(9, 18, 9, 8, 1, 1.0).expect("non-zero cell");
        let flush = ruled.with_grid_inset(bt_gpu::GridInset::ZERO);
        let (a, b) = (
            split_into_grid(900.0, 600.0, ruled, NO_DOCK, NO_RESERVE, NO_TOP),
            split_into_grid(900.0, 600.0, flush, NO_DOCK, NO_RESERVE, NO_TOP),
        );
        assert_eq!((a.cols, a.rows), (98, 32));
        assert_eq!((b.cols, b.rows), (100, 33));
        assert_eq!(b.dock_cols, a.dock_cols, "the dock left its ruler");
    }

    #[test]
    fn a_round_corner_costs_columns_beyond_the_gutter_and_only_sideways() {
        // A 20.5 pt corner at @2x: the margin is 25 px beside a 16 px gutter,
        // on both sides. At 1796 px of 18 px cells the gutter leaves exactly
        // 98 columns (1764 / 18) and the corner's margins only 97 of them.
        let flat = CellMetrics::new(18, 36, 18, 16, 1, 2.0).expect("non-zero cell");
        let round = flat.with_corners(20.5, 20.5);
        assert_eq!(round.side_left_px(), 25);
        let at = |cell| split_into_grid(1796.0, 1200.0, cell, DOCK_ROWS, NO_RESERVE, NO_TOP);
        let (flat_grid, round_grid) = (at(flat), at(round));
        assert_eq!(flat_grid.cols, 98, "(1796 - 2 * 16) / 18");
        assert_eq!(round_grid.cols, 97, "(1796 - 2 * 25) / 18");
        // The dock's width follows the same margin.
        assert_eq!(flat_grid.dock_cols, 98);
        assert_eq!(round_grid.dock_cols, 97);
        // Rows are the gutter's arithmetic — the dock's share and the grid's
        // foot: a corner does not change them.
        assert_eq!(round_grid.rows, flat_grid.rows);
        assert_eq!(
            bt_gpu::dock_px(DOCK_ROWS, round),
            bt_gpu::dock_px(DOCK_ROWS, flat)
        );
        // The metrics travel with the grid: the draw origin and the mouse
        // read the margin the columns were counted with.
        assert_eq!(round_grid.cell.side_left_px(), 25);
        assert_eq!(round_grid.cell.grid_inset().left, 25);
    }

    #[test]
    fn the_dock_costs_rows_and_only_when_there_is_one() {
        // The dock gutter is deducted from **rows** and, unlike the left gutter, conditional:
        // not a single row should go **to the dock** from a window without one (an
        // unintegrated shell, the smoke recipe) — `smoke_shell`'s `cells=8 glyphs=6`
        // contract is measured in that window. The top edge's reserve is a separate
        // parameter (`top_px`, zero here): in `Fade` that window gives it a row at
        // some heights (`the_fade_keeps_the_margin_free_at_the_top_and_takes_the_leftover`).
        let without = split_into_grid(900.0, 600.0, metrics(9, 18, 8), NO_DOCK, NO_RESERVE, NO_TOP);
        let with = split_into_grid(
            900.0,
            600.0,
            metrics(9, 18, 8),
            DOCK_ROWS,
            NO_RESERVE,
            NO_TOP,
        );
        // The grid's foot alone: (600 − 8) / 18 = 32.9 → 32.
        assert_eq!(without.rows, 32);
        // The dock takes **two rows, two breathing gutters and one row gap**:
        // 2×18 + 2×8 + 16 = 68 px, and the foot 8 more, i.e. 524 / 18 = 29.1 →
        // 29. The row gap
        // (`dock_row_gap`) is **twice** the outer gutter, because a line
        // passes through its middle and one gutter falls on each side of the line; if left out of the sum
        // it would come to 52 px, which gives 30 rows and the difference becomes **visible**.
        // The number's source is `bt_gpu::dock_px`, not `DOCK_ROWS`; if the two
        // drift apart this goes red.
        assert_eq!(with.rows, 29, "dock gutter was not deducted from rows");
        // Columns **do not see** the dock: without the scroll bar's reserve the
        // dock uses the same columns as the grid and its gutter is vertical only.
        assert_eq!(with.cols, without.cols);
    }

    #[test]
    fn the_fade_keeps_the_margin_free_at_the_top_and_takes_the_leftover() {
        // In `Fade` the rows keep the top edge's reserve free — the left
        // margin, 8 px here: a leftover at or above it costs nothing, one under
        // it costs a row. The drawing side's fade over those rows is the whole
        // leftover, never under the reserve and never a cell of it.
        let cell = metrics(9, 18, 8);
        let top = bt_gpu::edge_reserve_px(ContentEdge::Fade, cell);
        assert_eq!(top, 8.0, "the reserve is not the left margin");
        let edge = |height: f64, dock_rows: u16, rows: u16| {
            bt_gpu::edge_drawn_px(ContentEdge::Fade, height as f32, dock_rows, rows, cell)
        };
        // (height, dock, rows cut, rows faded, the fade), the grid's 8 px foot
        // kept free in both modes:
        // 608 with the dock: 532 / 18 = 29.6, a 10 px leftover — the same rows.
        // 604 with the dock: 528 / 18 = 29.3, a 6 px leftover — a row less.
        // 590 without: 582 / 18 = 32.3, a 6 px leftover — a row less.
        for (height, dock_rows, cut_rows, fade_rows, fade_px) in [
            (608.0, DOCK_ROWS, 29, 29, 10.0),
            (604.0, DOCK_ROWS, 29, 28, 24.0),
            (590.0, NO_DOCK, 32, 31, 24.0),
        ] {
            let cut = split_into_grid(900.0, height, cell, dock_rows, NO_RESERVE, NO_TOP);
            let fade = split_into_grid(900.0, height, cell, dock_rows, NO_RESERVE, top);
            assert_eq!((cut.rows, fade.rows), (cut_rows, fade_rows), "{height}");
            assert_eq!(fade.cols, cut.cols, "{height}: the fade took columns");
            assert_eq!(edge(height, dock_rows, fade.rows), fade_px, "{height}");
        }
        // `Cut` and `Line` keep nothing free: today's rows to the row.
        for mode in [ContentEdge::Cut, ContentEdge::Line] {
            assert_eq!(bt_gpu::edge_reserve_px(mode, cell), 0.0, "{mode:?}");
        }
        // A window that cannot hold the reserve gets no rows: the subtraction
        // saturates in `f64`, it does not wrap.
        let g = split_into_grid(900.0, 4.0, cell, NO_DOCK, NO_RESERVE, top);
        assert_eq!(g.rows, 0);
    }

    #[test]
    fn the_dock_breathing_room_scales_with_the_gutter() {
        // The breathing gutter is **derived**, not chosen: its source is the left
        // gutter itself. With a fixed pixel count the gutter would stay the same while the font
        // grows with Cmd +/− and the ratio would break; this test holds exactly that link.
        let tight = split_into_grid(
            900.0,
            600.0,
            metrics(9, 18, 0),
            DOCK_ROWS,
            NO_RESERVE,
            NO_TOP,
        );
        let loose = split_into_grid(
            900.0,
            600.0,
            metrics(9, 18, 8),
            DOCK_ROWS,
            NO_RESERVE,
            NO_TOP,
        );
        // A dock without gutters takes only its rows: 600 − 36 = 564 → 31.
        assert_eq!(tight.rows, 31);
        assert!(
            loose.rows < tight.rows,
            "the gutter grew but the dock covered the same space: {} / {}",
            loose.rows,
            tight.rows
        );
    }

    #[test]
    fn the_always_up_scroll_bar_costs_grid_columns_and_not_dock_columns() {
        // 900 px, 9 px cells, an 8 px frame: 98 columns. The always-up
        // form's track is 16 pt — 16 px at @1x — wider than the 8 px right
        // side, so it takes the side's place: `(900 − 8 − 16) / 9 = 97`
        // columns. The dock below the track keeps its 98, and the rows do not
        // see the track at all.
        let cell = metrics(9, 18, 8);
        let reserve = ScrollbarMode::Always.reserve_px(cell);
        let plain = split_into_grid(900.0, 600.0, cell, DOCK_ROWS, NO_RESERVE, NO_TOP);
        let always = split_into_grid(900.0, 600.0, cell, DOCK_ROWS, reserve, NO_TOP);
        assert_eq!((plain.cols, plain.dock_cols), (98, 98));
        assert_eq!(always.cols, 97, "the track's columns stayed in the grid");
        assert_eq!(always.dock_cols, 98, "the dock lost columns to the track");
        assert_eq!(always.rows, plain.rows, "the track took rows");
        // The text ends left of the track: nothing runs under the bar.
        let text_end = f64::from(cell.grid_inset().left) + f64::from(always.cols) * 9.0;
        assert!(text_end <= 900.0 - f64::from(reserve), "{text_end}");
        // The self-hiding forms reserve nothing.
        for mode in [ScrollbarMode::Auto, ScrollbarMode::Never] {
            let grid =
                split_into_grid(900.0, 600.0, cell, DOCK_ROWS, mode.reserve_px(cell), NO_TOP);
            assert_eq!((grid.cols, grid.dock_cols), (98, 98), "{mode:?}");
        }
        // Narrower than the frame and the track: no columns — the
        // subtraction is `f64` and saturates, it does not wrap to 65535.
        let narrow = split_into_grid(20.0, 600.0, cell, NO_DOCK, reserve, NO_TOP);
        assert_eq!((narrow.cols, narrow.dock_cols), (0, 0));
    }

    #[test]
    fn the_alternate_screen_takes_the_dock_and_gives_it_back() {
        // On the alternate screen the gutter is zero, on exit the **birth value** comes back.
        assert_eq!(dock_rows_for(true, false, DOCK_ROWS), 0);
        assert_eq!(dock_rows_for(false, false, DOCK_ROWS), DOCK_ROWS);
        // **This line is why the birth value is a separate input:**
        // leaving the alternate screen in a window that never had a dock (an unintegrated shell, the smoke
        // recipe) must **not** give birth to a dock. Were it written over a single
        // field, the value to restore would be built from the `DOCK_ROWS`
        // constant and exactly this window would gain a dock.
        assert_eq!(dock_rows_for(true, false, NO_DOCK), 0);
        assert_eq!(dock_rows_for(false, false, NO_DOCK), 0);
        // A remote session's alternate screen keeps the one-row status bar;
        // a window without a dock never gets one.
        assert_eq!(dock_rows_for(true, true, DOCK_ROWS), 1);
        assert_eq!(dock_rows_for(false, true, DOCK_ROWS), DOCK_ROWS);
        assert_eq!(dock_rows_for(true, true, NO_DOCK), 0);
    }

    #[test]
    fn a_window_shorter_than_the_dock_yields_no_rows() {
        // The vertical twin of `a_window_narrower_than_the_gutter_yields_no_columns`
        // and a guard for the same breakage: the subtraction goes negative in `f64` and
        // `as u16` saturates to zero. Done in `u16` it would overflow and
        // produce a 65535-row `TIOCSWINSZ`. `Session::resize` already
        // ignores a zero-row size.
        let g = split_into_grid(
            900.0,
            20.0,
            metrics(9, 18, 8),
            DOCK_ROWS,
            NO_RESERVE,
            NO_TOP,
        );
        assert_eq!(g.rows, 0);
        // Columns stand: a short window eliminates only rows.
        assert_eq!(g.cols, 98);
    }

    #[test]
    fn the_context_strip_ends_flush_with_the_right_margin() {
        // A 20.5 pt corner at @2x: 25 px of margin each side, the context row
        // in 13 px steps. Whatever the width, whole columns plus the leftover
        // plus the two margins make the surface: the zone's last letter ends at
        // the width minus the right margin, a remainder or none.
        let cell = CellMetrics::new(18, 36, 13, 16, 1, 2.0)
            .expect("non-zero cell")
            .with_corners(20.5, 20.5);
        let (left, right) = (
            u32::from(cell.side_left_px()),
            u32::from(cell.side_right_px()),
        );
        assert_eq!((left, right), (25, 25));
        let mut seen = [false; 2];
        for width in [900_u32, 901, 905, 912, 913, 925, 1000, 1779, 1780] {
            let g = split_into_grid(
                f64::from(width),
                1200.0,
                cell,
                DOCK_ROWS,
                NO_RESERVE,
                NO_TOP,
            );
            let ContextStrip { cols, offset_px } = g.context;
            assert!(offset_px < 13, "{width}: below one column");
            assert_eq!(
                left + u32::from(cols) * 13 + u32::from(offset_px) + right,
                width,
                "{width}: the strip fills the width"
            );
            seen[usize::from(offset_px > 0)] = true;
        }
        assert_eq!(seen, [true, true], "widths with and without a leftover");
        // The surface is the rounded width.
        let at =
            |width| split_into_grid(width, 1200.0, cell, DOCK_ROWS, NO_RESERVE, NO_TOP).context;
        assert_eq!(at(900.4), at(900.0));
        assert_eq!(at(900.5), at(901.0));
        // A square corner on the right asks for the gutter only there.
        let square = cell.with_corners(20.5, 0.0);
        let g = split_into_grid(900.0, 1200.0, square, DOCK_ROWS, NO_RESERVE, NO_TOP);
        assert_eq!(
            25 + u32::from(g.context.cols) * 13 + u32::from(g.context.offset_px) + 16,
            900
        );
    }

    #[test]
    fn the_context_strip_keeps_the_track_columns_the_grid_gives_up() {
        // The always-up bar's track narrows the grid; the dock below it, the
        // context row included, stays the window's width.
        let cell = metrics(9, 18, 8);
        let reserve = ScrollbarMode::Always.reserve_px(cell);
        let (open, barred) = (
            split_into_grid(900.0, 600.0, cell, DOCK_ROWS, NO_RESERVE, NO_TOP),
            split_into_grid(900.0, 600.0, cell, DOCK_ROWS, reserve, NO_TOP),
        );
        assert!(barred.cols < open.cols);
        assert_eq!(barred.context, open.context);
        // The context row's budget counts the same strip in smaller steps.
        let small = CellMetrics::new(9, 18, 7, 8, 1, 1.0).expect("cell");
        let g = split_into_grid(900.0, 600.0, small, DOCK_ROWS, NO_RESERVE, NO_TOP);
        assert_eq!(
            u32::from(g.context.cols) * 7 + u32::from(g.context.offset_px),
            900 - 16
        );
        assert!(g.context.cols > g.dock_cols);
    }

    #[test]
    fn a_window_narrower_than_the_margins_yields_no_context_columns() {
        // The strip goes negative in `f64` and saturates, like the grid's
        // columns: no columns and no leftover, never an overflowed `u16`.
        let cell = metrics(9, 18, 8);
        for width in [-5.0, 0.0, 4.0, 16.0] {
            let g = split_into_grid(width, 600.0, cell, DOCK_ROWS, NO_RESERVE, NO_TOP);
            assert_eq!(g.context, ContextStrip::default(), "{width}");
        }
        let g = split_into_grid(f64::NAN, 600.0, cell, DOCK_ROWS, NO_RESERVE, NO_TOP);
        assert_eq!(g.context, ContextStrip::default());
    }

    #[test]
    fn a_window_narrower_than_the_gutter_yields_no_columns() {
        // Accepted: no new lower bound is **introduced**, the existing chain gives the
        // right answer. The subtraction goes negative in `f64`, the division stays
        // negative and `as u16` saturates to zero; `Session::resize` already
        // ignores a zero-column size. Done in `u16` the same subtraction would
        // **overflow** and produce a `TIOCSWINSZ` with a column count near
        // 65535 — that is the breakage this test guards.
        let g = split_into_grid(4.0, 600.0, metrics(9, 18, 8), NO_DOCK, NO_RESERVE, NO_TOP);
        assert_eq!(g.cols, 0);
        // Rows stand: a narrow window eliminates only columns.
        assert_eq!(g.rows, 32);
    }

    #[test]
    fn zero_window_does_not_panic() {
        // A minimized window gives 0×0 bounds; `Session::resize` ignores a
        // zero grid but the path leading here must not panic —
        // not the split, the `as u16` saturation carries it.
        let g = split_into_grid(0.0, 0.0, metrics(9, 18, 8), NO_DOCK, NO_RESERVE, NO_TOP);
        assert_eq!((g.cols, g.rows), (0, 0));
    }
}
