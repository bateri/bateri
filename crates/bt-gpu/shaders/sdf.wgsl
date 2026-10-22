// The signed distance to a rounded box: the one copy of the corner curve, read
// by the caret and the selection (`cell_bg.wgsl`) and by the UI kit's boxes
// (`ui.wgsl`). Appended at compile time to the end of each of those files
// (`crate::renderer`), like `edge.wgsl`: module scope does not depend on order
// in WGSL, and appending keeps naga's line numbers for the host file the
// file's own.

// Signed distance to a rounded box; negative = inside, zero = the edge.
// `p` is relative to the centre, `half_size` the half extent.
//
// The `min` is NOT a policy but a mathematical precondition: the SDF needs the
// radius not to exceed the half extent. The one side that decides the radius's
// VALUE is Rust (bt_gpu::frame::caret_radius_px) — it is the side that knows
// the cell size and the tests read from there. The clamp here only defends
// against a broken immediate.
//
// `max(q, 0.0)` must be spelled with a vector in WGSL: there is no scalar
// splat.
fn rounded_box_sdf(p: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    let r = min(radius, min(half_size.x, half_size.y));
    let q = abs(p) - half_size + r;
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}
