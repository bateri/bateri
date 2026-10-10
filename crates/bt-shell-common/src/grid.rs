//! The grid a pane's pixels make: how many columns and rows of which cell fit a pane of a given
//! size, after the left gutter, the dock's share, the scroll bar's track and the top edge's
//! reserve — and how big the dock's share is at a given moment. Pure arithmetic every host of a
//! terminal pane asks the same way, so it stands here once rather than in a platform shell.

use bt_gpu::CellMetrics;

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
    /// The dock's width, columns: the window's, with no scroll bar reserve
    /// taken off — the always-up bar's track stops at the dock's top, and
    /// the dock below it stays full width. Equal to `cols` unless the grid
    /// reserves the track. The link's dock readings and the mouse's dock hit
    /// come from here ([`split_into_grid`] is the owner).
    pub dock_cols: u16,
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
/// **The left gutter is subtracted from the columns**: so the
/// stripe does not overlap the text. The gutter is always reserved — the
/// accepted cost is that it stays empty in a session without integration
/// (bash/fish, `shell.integration = false`, SSH); the alternative was a
/// SIGWINCH at the first prompt and three consumers being updated at once.
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
/// **The scroll bar's reserve is subtracted from the grid's columns only**
/// (`reserve_px`, [`ScrollbarMode::reserve_px`]): in the always-up form the
/// track takes the window's right edge down to the dock's top, so the grid
/// gives its width up and the dock does not — the dock's column count is a
/// separate answer ([`Grid::dock_cols`]). The reserve is a function of the
/// form alone, never of the history or the alternate screen: a reserve that
/// came and went with them would resize the grid on the first line into
/// history, every clear and every full-screen program.
///
/// **The top edge's reserve is subtracted from the rows** (`top_px`,
/// [`bt_gpu::edge_reserve_px`]): with the content fading at the pane's top,
/// that much of the height is kept free above the grid **even when the height
/// divides into rows**, and the leftover goes **to the top** — the grid sits
/// on the dock's share, a row at rest never enters the fade, and the drawing
/// side fades the whole leftover ([`bt_gpu::edge_drawn_px`]). Its source is the
/// left margin, so changing the margin moves the row count too. The cost is a
/// row less at the heights whose leftover is shorter than the reserve; zero
/// where the content is cut instead, every row the height allows as before.
/// Like the scroll bar's reserve it is a function of the mode and the cell
/// alone — never of the history or the alternate screen.
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
    // requirement: in a window narrower than the gutter the difference goes
    // negative, the division stays negative and `as u16` saturates it to zero
    // — i.e. the existing behavior (zero columns, `Session::resize` ignores
    // it) is preserved. Had the same subtraction been done in `u16` it would
    // **overflow** and produce a column count near 65535, a `TIOCSWINSZ` of
    // that size. No new lower bound is deliberately introduced: the end of the chain is already right.
    let usable_width = width_px - f64::from(cell.gutter_px());
    // The scroll bar's reserve is subtracted the same way and for the same
    // reason: in a window narrower than the gutter and the track the
    // difference goes negative and saturates to zero columns. The width here
    // is unrounded and the drawn track's left edge is the texture's rounded
    // width minus the same reserve; the text still ends at or before it,
    // because where the text ends — the gutter plus whole cells — is a whole
    // pixel not past `width − reserve`, so not past its floor either.
    let grid_width = usable_width - f64::from(reserve_px);
    // The dock share is also **in `f64`** and for the same reason: in a window
    // shorter than the dock the difference goes negative, the division stays
    // negative and `as u16` saturates it to zero — `Session::resize` already
    // ignores that size. Done in `u16` it would overflow and produce a
    // 65535-row `TIOCSWINSZ`. The formula is **bt-gpu's** ([`bt_gpu::dock_px`]):
    // the dock's share carries two breathing margins next to the rows and were
    // it rewritten here it would diverge for one frame on resize — the same
    // discipline as consuming `DOCK_ROWS`, no second copy is kept.
    //
    // The top edge's reserve is subtracted the same way: it is a pixel height
    // like the dock's share, and the window that cannot hold it gets no rows.
    let usable_height = height_px - f64::from(bt_gpu::dock_px(dock_rows, cell)) - f64::from(top_px);
    Grid {
        cols: (grid_width / f64::from(cell_w)) as u16,
        rows: (usable_height / f64::from(cell_h)) as u16,
        dock_cols: (usable_width / f64::from(cell_w)) as u16,
        cell,
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
    fn the_gutter_costs_columns() {
        // The left gutter is deducted from columns: so the stripe does not
        // sit on top of the text. 900 pixels, 9-pixel cells → 100 columns with no gutter; an 8-pixel
        // gutter takes one column, and so does 9 pixels (a full cell).
        let plain = split_into_grid(900.0, 600.0, metrics(9, 18, 0), NO_DOCK, NO_RESERVE, NO_TOP);
        let gutter = split_into_grid(900.0, 600.0, metrics(9, 18, 8), NO_DOCK, NO_RESERVE, NO_TOP);
        assert_eq!(plain.cols, 100);
        assert_eq!(gutter.cols, 99, "the gutter takes one column");
        // Rows **do not see** the gutter as a left margin: it is only on the left.
        // The top edge's reserve, which in `Fade` is the gutter again, is its own
        // parameter (`top_px`, zero here) and its own test
        // (`the_fade_keeps_the_margin_free_at_the_top_and_takes_the_leftover`).
        assert_eq!(gutter.rows, plain.rows);
        // The gutter travels with the metrics: the value that built the grid gives it back
        // and the draw origin and mouse mapping read the same value.
        assert_eq!(gutter.cell.gutter_px(), 8);
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
        // 600 / 18 = 33.3 → 33.
        assert_eq!(without.rows, 33);
        // The dock takes **two rows, two breathing gutters and one row gap**:
        // 2×18 + 2×8 + 16 = 68 px, i.e. 532 / 18 = 29.5 → 29. The row gap
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
        // (height, dock, rows cut, rows faded, the fade)
        // 600 with the dock: 532 / 18 = 29.5, a 10 px leftover — the same rows.
        // 596 with the dock: 528 / 18 = 29.3, a 6 px leftover — a row less.
        // 600 without: 600 / 18 = 33.3, a 6 px leftover — a row less.
        for (height, dock_rows, cut_rows, fade_rows, fade_px) in [
            (600.0, DOCK_ROWS, 29, 29, 10.0),
            (596.0, DOCK_ROWS, 29, 28, 24.0),
            (600.0, NO_DOCK, 33, 32, 24.0),
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
        // 900 px, 9 px cells, an 8 px gutter: 99 columns. The always-up
        // form's track is 16 pt — 16 px at @1x — so the grid ends at 892 px
        // − 16: 876 / 9 = 97 columns. The dock below the track keeps the
        // window's 99, and the rows do not see the track at all.
        let cell = metrics(9, 18, 8);
        let reserve = ScrollbarMode::Always.reserve_px(cell);
        let plain = split_into_grid(900.0, 600.0, cell, DOCK_ROWS, NO_RESERVE, NO_TOP);
        let always = split_into_grid(900.0, 600.0, cell, DOCK_ROWS, reserve, NO_TOP);
        assert_eq!((plain.cols, plain.dock_cols), (99, 99));
        assert_eq!(always.cols, 97, "the track's columns stayed in the grid");
        assert_eq!(always.dock_cols, 99, "the dock lost columns to the track");
        assert_eq!(always.rows, plain.rows, "the track took rows");
        // The text ends left of the track: nothing runs under the bar.
        let text_end = f64::from(cell.gutter_px()) + f64::from(always.cols) * 9.0;
        assert!(text_end <= 900.0 - f64::from(reserve), "{text_end}");
        // The self-hiding forms reserve nothing.
        for mode in [ScrollbarMode::Auto, ScrollbarMode::Never] {
            let grid =
                split_into_grid(900.0, 600.0, cell, DOCK_ROWS, mode.reserve_px(cell), NO_TOP);
            assert_eq!((grid.cols, grid.dock_cols), (99, 99), "{mode:?}");
        }
        // Narrower than the gutter and the track: no columns — the
        // subtraction is `f64` and saturates, it does not wrap to 65535.
        let narrow = split_into_grid(20.0, 600.0, cell, NO_DOCK, reserve, NO_TOP);
        assert_eq!((narrow.cols, narrow.dock_cols), (0, 1));
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
        assert_eq!(g.cols, 99);
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
        assert_eq!(g.rows, 33);
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
