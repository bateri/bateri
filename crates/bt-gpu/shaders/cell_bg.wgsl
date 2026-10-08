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
// to push constants on Vulkan). A module has one
// `var<immediate>` block and both stages share it.
//
// **The order follows alignment**: `vec4` aligns to 16, `vec2` to 8. With
// `viewport_px` first, `core` would be pushed to 16 and 8 invisible padding
// bytes would sit in between, which the Rust side would have to spell out.
// `vec4`s first, then the `vec2`, then the `f32`: core@0, shape@16,
// viewport_px@32, edge_px@40, size 48 (the struct aligns to 16, so 44 rounds
// up to 48). The Rust twin is `crate::renderer::Immediates`; its fields and
// trailing pad are pinned to these numbers by `offset_of`/`size_of` asserts.
//
//   core  = the caret's PAINTED rectangle (x0, y0, x1, y1), WINDOW space.
//           The fragment's `@builtin(position)` is the coordinate AFTER the
//           viewport transform and `core` is written in that space too
//           (bt_gpu::frame::Frame::caret_core); were the two in different
//           spaces, the glow would be drawn in the wrong place on a frame
//           with a content offset.
//   shape = (corner radius, stroke width, glow margin, glow peak alpha)
//   viewport_px = the target texture's size in pixels (the NDC scale's divisor).
//   edge_px     = the content's top fade, pixels from the window's top
//                 (`edge.wgsl`, appended to this file); zero for the draws
//                 outside the content — the scroll bar, the dock.
//
// `cell_bg` reads only `viewport_px` and `edge_px`; the whole block is still
// written on every draw because the pipelines share one layout. The wave's
// fragment reads `core.xy` and `shape` (below).
struct Immediates {
    core: vec4<f32>,
    shape: vec4<f32>,
    viewport_px: vec2<f32>,
    edge_px: f32,
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
    return vec4<f32>(in.rgba.rgb, in.rgba.a * edge_alpha(in.position.y, imm.edge_px));
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
    // fades with blink and no second path is written for it. So does the
    // content's top fade, the same way.
    let edge = edge_alpha(in.position.y, imm.edge_px);
    return vec4<f32>(in.rgba.rgb, in.rgba.a * max(body, halo) * edge);
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
    return vec4<f32>(color.rgb, color.a * coverage * edge_alpha(in.position.y, imm.edge_px));
}

// ---------------------------------------------------------------------------
// The dock's wave line (the arrival scene's `ripple`)
// ---------------------------------------------------------------------------
//
// One quad per frame through `cell_bg_vertex`, in WINDOW space: the line
// leaves its row on both sides, so it is drawn from a viewport at the window's
// top, not the dock's. The colour (alpha included) rides in the instance, like
// `cell_bg`'s; the block carries the geometry:
//
//   core  = (the line's centre y in window space, pixels per point, 0, 0)
//   shape = (the resting wave's peak, its phase, the ring's front x, the
//            ring's peak)
//
// The peaks and the front are PIXELS, the phase radians. The wavelengths and
// the ring's reach are DESIGN lengths — points — and the fragment scales them
// by `core.y`, so the wave is the same size on a 1x and a 2x screen.
//
//   height(x) = rest·sin(τx/λ_rest + phase)
//             + (x ≤ front) · ring·exp(−|x − front|/reach)·sin(τ(x − front)/λ_ring)
//
// The ring exists only behind its front: it spreads from the › and leaves a
// calm line ahead of it.
//
// **Zero peaks are today's line, exactly.** The coverage is a one-pixel tent
// round the curve: a line at a pixel centre covers that row fully and its
// neighbours not at all, so with both peaks at zero the pixels are the ones the
// dock's own hairline draws and the swap between them is invisible.

const TAU: f32 = 6.2831855;
// Design lengths, points.
const WAVE_REST_LENGTH: f32 = 140.0;
const WAVE_RING_LENGTH: f32 = 60.0;
const WAVE_RING_REACH: f32 = 90.0;

// How far the line is from its row at window x, pixels (down is positive).
fn wave_height(x: f32, scale: f32) -> f32 {
    let rest = imm.shape.x;
    let phase = imm.shape.y;
    let front = imm.shape.z;
    let ring = imm.shape.w;
    let behind = x - front;
    let spread = ring
        * exp(-abs(behind) / (WAVE_RING_REACH * scale))
        * sin(TAU * behind / (WAVE_RING_LENGTH * scale));
    return rest * sin(TAU * x / (WAVE_REST_LENGTH * scale) + phase)
        + select(0.0, spread, behind <= 0.0);
}

// The dust scene's gradient rides in the same fragment, picked by `core.z`
// (zero is the wave above):
//
//   1 = the lit tip of the woven line: the instance's alpha ramps from nothing
//       at `core.x` to whole at `core.w` (a row of pixels, left to right).
//
// It does not carry the content's top fade: it is drawn in window space like
// the wave.
const MODE_RAMP: f32 = 1.0;

@fragment
fn wave_fragment(in: Out) -> @location(0) vec4<f32> {
    let mode = imm.core.z;
    if (mode > MODE_RAMP - 0.5) {
        let across = clamp((in.position.x - imm.core.x) / max(imm.core.w - imm.core.x, 1.0), 0.0, 1.0);
        return vec4<f32>(in.rgba.rgb, in.rgba.a * across);
    }
    let scale = imm.core.y;
    let x = in.position.x;
    // The distance to the curve measured across it, not straight down: a
    // steep stretch would otherwise come out thinner than a flat one. The
    // slope is the height's change over one pixel.
    let slope = wave_height(x + 0.5, scale) - wave_height(x - 0.5, scale);
    let across = abs(in.position.y - (imm.core.x + wave_height(x, scale)))
        * inverseSqrt(1.0 + slope * slope);
    return vec4<f32>(in.rgba.rgb, in.rgba.a * clamp(1.0 - across, 0.0, 1.0));
}

// ---------------------------------------------------------------------------
// The dust scene's motes
// ---------------------------------------------------------------------------
//
// A soft round dot per instance: `pos` is its CENTRE, `size` is (diameter,
// blur) — both pixels — and `rgba` its colour. The blur is a Gaussian's
// spread; a dot as small as a mote is mostly edge, so a blurred one is also
// fainter, as a real blur would leave it. The quad is larger than the
// diameter by what the edge needs, so it has its own vertex (the selection's
// precedent: the quad reaches the fragment). No top fade: the dust is drawn in
// window space.

struct DotOut {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) rgba: vec4<f32>,
    // Pixels from the dot's centre.
    @location(1) local: vec2<f32>,
    // (radius, blur), pixels.
    @location(2) @interpolate(flat) shape: vec2<f32>,
}

// How far past the radius the edge reaches, in blurs, and the sharp edge's own
// half width, pixels.
const DOT_SOFT: f32 = 1.5;
const DOT_EDGE: f32 = 0.5;

@vertex
fn dot_vertex(@builtin(vertex_index) vid: u32, it: Instance) -> DotOut {
    let corner = vec2<f32>(f32(vid & 1u), f32(vid >> 1u));
    let radius = it.size.x * 0.5;
    let blur = it.size.y;
    let reach = radius + DOT_EDGE + DOT_SOFT * blur + 0.5;
    let local = (corner * 2.0 - 1.0) * reach;
    let ndc = (it.pos + local) / imm.viewport_px * 2.0 - 1.0;
    var o: DotOut;
    o.position = vec4<f32>(ndc.x, -ndc.y, 0.0, 1.0);
    o.rgba = it.rgba;
    o.local = local;
    o.shape = vec2<f32>(radius, blur);
    return o;
}

@fragment
fn dot_fragment(in: DotOut) -> @location(0) vec4<f32> {
    let radius = in.shape.x;
    let blur = in.shape.y;
    let soft = DOT_EDGE + DOT_SOFT * blur;
    let coverage = 1.0 - smoothstep(-soft, soft, length(in.local) - radius);
    // The peak a Gaussian blur leaves of a disc this size; one, for a sharp dot.
    let spread = max(blur, 0.001);
    let peak = select(1.0, 1.0 - exp(-(radius * radius) / (2.0 * spread * spread)), blur > 0.0);
    return vec4<f32>(in.rgba.rgb, in.rgba.a * coverage * peak);
}
