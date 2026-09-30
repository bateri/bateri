// The `cell_bg` pipeline (cell backgrounds and plain quads), the caret's SDF
// fragment and the selection's vertex and fragment. Read by `crate::renderer`.
//
// Rust counterpart: bt_gpu::frame::Instance, #[repr(C)]
// { pos: [f32; 2], size: [f32; 2], rgba: [f32; 4] }, stride 32.
//
// **Instances come from an instance-stepped vertex buffer, not a storage
// buffer**: indexing a storage array by `instance_index` would need a bind
// group rebuilt every frame. The vertex buffer's layout is written field by
// field in `crate::renderer`'s `VertexBufferLayout` (pos@0, size@8, rgba@16)
// and fed from `Instance`'s `offset_of!` in Rust. The layout must match field
// for field on both sides: a field added here shifts the stride away from 32,
// the Rust side cannot see it, and the GPU reads every instance from the
// second one on wrongly.

struct Instance {
    // Top-left corner, pixels.
    @location(0) pos: vec2<f32>,
    // Width/height, pixels.
    @location(1) size: vec2<f32>,
    // Linear RGBA: the target is Bgra8UnormSrgb and the ROP does the encoding.
    // Adding a gamma correction here or in the fragment would encode the
    // palette TWICE.
    @location(2) rgba: vec4<f32>,
}

// Small values are **immediates** (wgpu lowers them to `set*Bytes` on Metal and
// to push constants on Vulkan; discussion.md → Karar 5). A module has one
// `var<immediate>` block and both stages share it.
//
// **The order follows alignment**: `vec4` aligns to 16, `vec2` to 8. With
// `viewport_px` first, `core` would be pushed to 16 and 8 invisible padding
// bytes would sit in between, which the Rust side would have to spell out.
// `vec4`s first, `vec2` last: core@0, shape@16, viewport_px@32, size 48 (the
// struct aligns to 16, so 40 rounds up to 48). The Rust twin is
// `crate::renderer::Immediates`; its fields and trailing pad are pinned to
// these numbers by `offset_of`/`size_of` asserts.
//
//   core  = the caret's PAINTED rectangle (x0, y0, x1, y1), WINDOW space.
//           The fragment's `@builtin(position)` is the coordinate AFTER the
//           viewport transform and `core` is written in that space too
//           (bt_gpu::frame::Frame::caret_core); were the two in different
//           spaces, the glow would be drawn in the wrong place on a frame
//           with a content offset.
//   shape = (corner radius, stroke width, glow margin, glow peak alpha)
//   viewport_px = the target texture's size in pixels (the NDC scale's divisor).
//
// `cell_bg` reads only `viewport_px`; the whole block is still written on every
// draw because both pipelines share one layout.
struct Immediates {
    core: vec4<f32>,
    shape: vec4<f32>,
    viewport_px: vec2<f32>,
}

var<immediate> imm: Immediates;

struct Out {
    @builtin(position) position: vec4<f32>,
    // The colour is constant across the instance; `flat` drops the
    // rasterizer's per-fragment interpolation.
    @location(0) @interpolate(flat) rgba: vec4<f32>,
}

@vertex
fn cell_bg_vertex(@builtin(vertex_index) vid: u32, it: Instance) -> Out {
    // vid 0..3 → (0,0) (1,0) (0,1) (1,1); the triangle strip covers the unit
    // square. There is no corner data in any buffer: the quad is derived here.
    let corner = vec2<f32>(f32(vid & 1u), f32(vid >> 1u));
    let ndc = (it.pos + corner * it.size) / imm.viewport_px * 2.0 - 1.0;
    var o: Out;
    // Pixel space starts at the top left, NDC at the bottom left: flip y.
    o.position = vec4<f32>(ndc.x, -ndc.y, 0.0, 1.0);
    o.rgba = it.rgba;
    return o;
}

@fragment
fn cell_bg_fragment(in: Out) -> @location(0) vec4<f32> {
    return in.rgba;
}

// ---------------------------------------------------------------------------
// The caret's own fragment
// ---------------------------------------------------------------------------
//
// Its vertex is `cell_bg_vertex` ITSELF and `Instance` is used verbatim: the
// caret is one quad per frame, its corners again derived from the vertex
// index. Only the fragment differs, so this pipeline does not spawn a second
// vertex path. (The upload row's buttons are the same fragment's second
// consumer: fill + stroke, one quad per draw.)
//
// The quad arrives LARGER than `core` by the glow margin (`Caret::instance`
// in `frame.rs` inflates it); the glow lives exactly in that difference. The
// fragment does not recompute the inflation — its size is already `shape.z`.

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

@fragment
fn caret_fragment(in: Out) -> @location(0) vec4<f32> {
    let radius = imm.shape.x;
    let stroke = imm.shape.y;
    let glow = imm.shape.z;
    let glow_alpha = imm.shape.w;

    let center = (imm.core.xy + imm.core.zw) * 0.5;
    let half_size = (imm.core.zw - imm.core.xy) * 0.5;
    // In the fragment stage `@builtin(position)` is a pixel-centred window
    // coordinate — `core` is written in that space.
    let d = rounded_box_sdf(in.position.xy - center, half_size, radius);

    // **In the degenerate branch the edge is a HARD step.** With radius and
    // glow both zero the output must be bit for bit the old plain quad: were
    // `smoothstep` to run in that branch too, the edge pixels would get half
    // alpha and the pixel tests would stop being proof of that parity.
    // "Radius 0, glow 0" is the supported rollback path.
    // WGSL has no ternary; `select(if_false, if_true, cond)` — the argument
    // order reads backwards.
    let degenerate = radius == 0.0 && glow == 0.0;
    var body = select(1.0 - smoothstep(-0.5, 0.5, d), step(d, 0.0), degenerate);
    if (stroke > 0.0) {
        // Hollow caret (unfocused): only the edge band. The band's inner
        // boundary is -stroke, i.e. `stroke` pixels inside the rectangle.
        // `stroke == 0` means solid.
        let inner = select(
            1.0 - smoothstep(-stroke - 0.5, -stroke + 0.5, d),
            step(d, -stroke),
            degenerate,
        );
        body = body - inner;
    }

    // The glow lives only OUTSIDE: inside, the body is already opaque and
    // adding the two would make the edge brighter than it is. Peak alpha at
    // the edge, zero at the end of the glow margin.
    var halo = 0.0;
    if (glow > 0.0) {
        halo = (1.0 - smoothstep(0.0, glow, max(d, 0.0))) * glow_alpha * step(0.0, d);
    }

    // The caret's own alpha (motion × blink) multiplies EVERYTHING: the glow
    // fades with blink and no second path is written for it.
    return vec4<f32>(in.rgba.rgb, in.rgba.a * max(body, halo));
}

// ---------------------------------------------------------------------------
// The selection's own vertex and fragment. Search highlights are drawn with
// the same pipeline, one draw per role.
// ---------------------------------------------------------------------------
//
// `Instance` is read VERBATIM (same vertex buffer layout as `cell_bg`); only
// the MEANING of `rgba` changes: here it is not a colour but a corner mask
// (`bt_gpu::frame::Frame::push_selection`), in the order TL, TR, BR, BL:
//
//   run   →  per corner, 1 = convex rounded, 0 = square
//   fill  →  an r×r piece; the corner that is the circle's centre is -1,
//            the rest 0
//
// **The immediates block is shared with the caret** (one module, one
// `var<immediate>`): for this pipeline `core` is the highlight's linear RGBA
// and `shape.x` its corner radius in pixels; `shape.yzw` are unused. Colour
// and radius are one value per draw. One layout is cheaper than a second
// pipeline layout with a 32-byte block of its own.
//
// The vertex is separate because the fragment must know its own quad and
// `cell_bg_vertex`'s output does not carry it. The caret solves this with a
// `core` immediate written in window space in Rust — enough for a single
// instance; the selection has one quad per instance, so the quad itself
// travels as varyings.

struct SelectionOut {
    @builtin(position) position: vec4<f32>,
    // Pixels relative to the quad's centre. Linear interpolation gives an
    // affine coordinate derived from the corners EXACTLY at the fragment
    // centre; with `flat` it would stay constant across the quad.
    // Viewport-independent: the offset (`origin_px`) never reaches it.
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) half_size: vec2<f32>,
    @location(2) @interpolate(flat) mask: vec4<f32>,
}

@vertex
fn selection_vertex(@builtin(vertex_index) vid: u32, it: Instance) -> SelectionOut {
    let corner = vec2<f32>(f32(vid & 1u), f32(vid >> 1u));
    let ndc = (it.pos + corner * it.size) / imm.viewport_px * 2.0 - 1.0;
    var o: SelectionOut;
    o.position = vec4<f32>(ndc.x, -ndc.y, 0.0, 1.0);
    o.local = (corner - 0.5) * it.size;
    o.half_size = it.size * 0.5;
    o.mask = it.rgba;
    return o;
}

@fragment
fn selection_fragment(in: SelectionOut) -> @location(0) vec4<f32> {
    let color = imm.core;
    let radius = imm.shape.x;
    let p = in.local;
    let m = in.mask;
    var coverage: f32;
    if (any(m < vec4<f32>(0.0))) {
        // Concave fill: paints OUTSIDE the circle whose centre is the corner
        // marked -1. The piece is r×r, so the circle's arc rounds the step's
        // inner corner and the piece's two outer edges stay inside the circle.
        // The corner is picked by nested `select`s (first match wins: TR, BR,
        // BL, else TL).
        let unit = select(
            select(
                select(vec2<f32>(0.0, 0.0), vec2<f32>(0.0, 1.0), m.w < 0.0),
                vec2<f32>(1.0, 1.0),
                m.z < 0.0,
            ),
            vec2<f32>(1.0, 0.0),
            m.y < 0.0,
        );
        let centre = (unit - 0.5) * in.half_size * 2.0;
        coverage = smoothstep(-0.5, 0.5, length(p - centre) - radius);
    } else {
        // Per-corner radius: the code of the quadrant the pixel is in. Window
        // space has y pointing down, so `p.y < 0` is the top half.
        // Top half: TL (x < 0) or TR; bottom half: BL (x < 0) or BR.
        // At a square corner the radius is 0 and the SDF is the plain box
        // itself; the edges sit on the pixel grid, so the ±0.5 smoothing gives
        // exactly 0/1 there — two runs stacked vertically join seamlessly.
        let code = select(select(m.z, m.w, p.x < 0.0), select(m.y, m.x, p.x < 0.0), p.y < 0.0);
        let d = rounded_box_sdf(p, in.half_size, code * radius);
        coverage = 1.0 - smoothstep(-0.5, 0.5, d);
    }
    return vec4<f32>(color.rgb, color.a * coverage);
}
