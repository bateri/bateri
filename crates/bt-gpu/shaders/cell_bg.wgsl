// WGSL twin of `cell_bg.metal` (040 phase-2): the `cell_bg` pipeline and the
// caret's SDF fragment. Today only the `cfg(test)` wgpu renderer reads it
// (`crate::wgpu_renderer`); Metal stays as the oracle and both backends'
// output is compared pixel by pixel over one scene list
// (discussion.md → Karar 4). The full rationale lives in the `.metal`; only
// what DIFFERS between the two languages is written here.
//
// Rust counterpart: bt_gpu::frame::Instance, #[repr(C)]
// { pos: [f32; 2], size: [f32; 2], rgba: [f32; 4] }, stride 32.
//
// **Instances come from an instance-stepped vertex buffer, not a storage
// buffer**: the Metal side indexes `device const Instance*` by `instance_id`;
// in WGSL that would need a bind group, rebuilt every frame. The vertex
// buffer's layout is written field by field in `crate::wgpu_renderer`'s
// `VertexBufferLayout` (pos@0, size@8, rgba@16) and fed from `Instance`'s
// `offset_of!` in Rust.

struct Instance {
    @location(0) pos: vec2<f32>,
    @location(1) size: vec2<f32>,
    // Linear RGBA: the target is Bgra8UnormSrgb and the ROP does the encoding.
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
// `crate::wgpu_renderer::Immediates`; its fields and trailing pad are pinned
// to these numbers by `offset_of`/`size_of` asserts.
//
//   core  = the caret's PAINTED rectangle (x0, y0, x1, y1), WINDOW space.
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
    // The colour is constant across the instance; `flat` drops interpolation.
    @location(0) @interpolate(flat) rgba: vec4<f32>,
}

@vertex
fn cell_bg_vertex(@builtin(vertex_index) vid: u32, it: Instance) -> Out {
    // vid 0..3 → (0,0) (1,0) (0,1) (1,1); the triangle strip covers the unit
    // square.
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

// Signed distance to a rounded box; `rounded_box_sdf` in the `.metal`.
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
    // coordinate, like Metal's `[[position]]` — `core` is written in that space.
    let d = rounded_box_sdf(in.position.xy - center, half_size, radius);

    // In the degenerate branch the edge is a HARD step (bit for bit the old
    // plain quad). WGSL has no ternary; `select(if_false, if_true, cond)` —
    // the argument order reads backwards.
    let degenerate = radius == 0.0 && glow == 0.0;
    var body = select(1.0 - smoothstep(-0.5, 0.5, d), step(d, 0.0), degenerate);
    if (stroke > 0.0) {
        let inner = select(
            1.0 - smoothstep(-stroke - 0.5, -stroke + 0.5, d),
            step(d, -stroke),
            degenerate,
        );
        body = body - inner;
    }

    // The glow lives only outside and reaches zero at the margin's end.
    var halo = 0.0;
    if (glow > 0.0) {
        halo = (1.0 - smoothstep(0.0, glow, max(d, 0.0))) * glow_alpha * step(0.0, d);
    }

    // The caret's own alpha multiplies everything: the glow fades with blink.
    return vec4<f32>(in.rgba.rgb, in.rgba.a * max(body, halo));
}
