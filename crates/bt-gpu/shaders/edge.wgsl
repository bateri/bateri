// The content's top edge: the ramp the grid's and the fill band's fragments
// multiply their output alpha by. Appended at compile time to the end of
// `cell_bg.wgsl` and `cell.wgsl` (`crate::renderer`), so there is one copy
// of the curve; module scope does not depend on order in WGSL, and appending
// keeps naga's line numbers for the host file the file's own.
//
// **Transparency, not paint.** The factor only thins what a fragment was
// going to draw: with the straight-alpha blend the content fades towards
// whatever is under it — the pass's clear, the window's ground — and nothing
// is painted where no content was. A gradient quad in the ground's colour
// would look the same today and paint a stripe over any texture laid under
// the content later.
//
// `y` is `@builtin(position).y`: window space, after the viewport transform,
// so a list drawn in a raised (`lift`) or negative-origin (fill band)
// viewport fades at the same window pixel as any other. `edge_px` is the
// fade's height from the window's top (`crate::frame::Frame::edge_px`).
//
// The curve is the design's own, piecewise linear through five stops from the
// top down — `0` at the top, `0.12` at a quarter, `0.42` at half, `0.80` at
// three quarters, `1` at the bottom — not a smoothstep approximating it.
// Written as four ramps, each the rise from one stop to the next over its own
// quarter, so the stops read as they are and no array is indexed.
//
// **Outside the fade the factor is exactly `1.0`**: `edge_px <= 0` (no fade)
// or a pixel at or below its bottom — returned as is, not the curve's end
// value, so a frame without a fade multiplies by one and comes out bit for bit
// the frame drawn before the fade existed. "No fade" is an early return: the
// value is an immediate, i.e. the same for the whole draw, so the branch is
// uniform and the draws outside the content (the dock, the scroll bar) and
// every frame without a fade skip the ramp. Inside a fade the per-pixel choice
// is a `select`, which evaluates both arms; the divisor is positive there.

const EDGE_QUARTER: f32 = 0.12;
const EDGE_HALF: f32 = 0.42;
const EDGE_THREE_QUARTERS: f32 = 0.80;

fn edge_alpha(y: f32, edge_px: f32) -> f32 {
    if (edge_px <= 0.0) {
        return 1.0;
    }
    let s = 4.0 * y / edge_px;
    let ramp = EDGE_QUARTER * saturate(s)
        + (EDGE_HALF - EDGE_QUARTER) * saturate(s - 1.0)
        + (EDGE_THREE_QUARTERS - EDGE_HALF) * saturate(s - 2.0)
        + (1.0 - EDGE_THREE_QUARTERS) * saturate(s - 3.0);
    // WGSL has no ternary: `select(if_false, if_true, cond)`.
    return select(ramp, 1.0, y >= edge_px);
}
