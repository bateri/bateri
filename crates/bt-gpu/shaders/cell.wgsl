// WGSL twin of `cell.metal` (040 phase-3): the `cell` pipeline (glyphs and
// rules, sampling the atlas's mask plane) and `emoji_fragment` (the colour
// plane). Today only the `cfg(test)` wgpu renderer reads it
// (`crate::wgpu_renderer`); Metal stays as the oracle and both backends'
// output is compared pixel by pixel over one scene list
// (discussion.md → Karar 4). The full rationale lives in the `.metal`; only
// what DIFFERS between the two languages is written here.
//
// **Emoji shares `cell_vertex` verbatim**; only the fragment differs (the
// `CLAUDE.md` pipeline contract). Both fragments sample with the same
// nearest/clamp sampler: slots have no padding between them and `linear`'s
// last column would blend the neighbour slot.
//
// Rust counterpart: bt_gpu::frame::GlyphInstance, #[repr(C)]
// { pos: [f32; 2], uv0: [f32; 2], rgba: [f32; 4] }, stride 32. Like
// `cell_bg.wgsl`, instances come from an instance-stepped vertex buffer; the
// layout is written in `crate::wgpu_renderer` from `GlyphInstance`'s
// `offset_of!` (pos@0, uv0@8, rgba@16).

struct GlyphInstance {
    // The cell's top-left corner, pixels.
    @location(0) pos: vec2<f32>,
    // The slot's top-left corner in the atlas, normalised.
    @location(1) uv0: vec2<f32>,
    // Linear RGBA: the target is Bgra8UnormSrgb and the ROP does the encoding.
    @location(2) rgba: vec4<f32>,
}

// Metal's three vertex uniforms and the fragment's `CursorBlock` become one
// `var<immediate>` block (a module has one; both stages share it).
//
// **Order follows alignment**: the two `vec4`s first (the `CursorBlock`
// struct, rect@0 rgba@16), then the `vec2`s: viewport_px@32, cell_px@40,
// uv_size@48. 56 bytes round up to the struct's 16-byte alignment: size 64.
// The Rust twin is `crate::wgpu_renderer::GlyphImmediates`; it embeds
// `CursorBlock` as is and spells the trailing 8 bytes as an explicit `pad`,
// pinned by `offset_of`/`size_of` asserts.
//
//   cursor_rect = the caret block's rectangle (x0, y0, x1, y1), WINDOW space;
//                 degenerate (all zero) when no text is inverted.
//   cursor_rgba = the text colour under the block; alpha is this frame's
//                 caret opacity, used as the mix factor.
struct Immediates {
    cursor_rect: vec4<f32>,
    cursor_rgba: vec4<f32>,
    viewport_px: vec2<f32>,
    cell_px: vec2<f32>,
    uv_size: vec2<f32>,
}

var<immediate> imm: Immediates;

// One plane per bind group: the mask (`R8Unorm`) or the colour plane
// (`Rgba8UnormSrgb`, which the hardware decodes to linear when sampling —
// the same contract as Metal's `RGBA8Unorm_sRGB`).
@group(0) @binding(0) var atlas: texture_2d<f32>;
@group(0) @binding(1) var atlas_sampler: sampler;

struct Out {
    @builtin(position) position: vec4<f32>,
    // The uv is interpolated: it sweeps the slot across the quad.
    @location(0) uv: vec2<f32>,
    // The colour is constant across the instance; `flat` drops interpolation.
    @location(1) @interpolate(flat) rgba: vec4<f32>,
}

@vertex
fn cell_vertex(@builtin(vertex_index) vid: u32, it: GlyphInstance) -> Out {
    let corner = vec2<f32>(f32(vid & 1u), f32(vid >> 1u));
    let ndc = (it.pos + corner * imm.cell_px) / imm.viewport_px * 2.0 - 1.0;
    var o: Out;
    // Pixel space starts at the top left, NDC at the bottom left: flip y. The
    // atlas's own y also starts at the top, so the uv is NOT flipped.
    o.position = vec4<f32>(ndc.x, -ndc.y, 0.0, 1.0);
    o.uv = it.uv0 + corner * imm.uv_size;
    o.rgba = it.rgba;
    return o;
}

@fragment
fn cell_fragment(in: Out) -> @location(0) vec4<f32> {
    let coverage = textureSample(atlas, atlas_sampler, in.uv).r;
    // Text under the caret block takes the block's text colour, asked per
    // PIXEL against a half-open rectangle [x0, x1) in window space
    // (`@builtin(position)` is pixel-centred, like Metal's `[[position]]`).
    let p = in.position.xy;
    let inside = all(p >= imm.cursor_rect.xy) && all(p < imm.cursor_rect.zw);
    // A blend, not an override, and only in RGB; WGSL has no ternary:
    // `select(if_false, if_true, cond)`.
    let rgb = mix(in.rgba.rgb, imm.cursor_rgba.rgb, select(0.0, imm.cursor_rgba.a, inside));
    // Straight alpha: the blend is src_alpha / one_minus_src_alpha.
    return vec4<f32>(rgb, in.rgba.a * coverage);
}

@fragment
fn emoji_fragment(in: Out) -> @location(0) vec4<f32> {
    // The colour comes from the TEXTURE, not the instance, and the cursor
    // block is not read: an emoji's colour is not a palette colour. The
    // bytes are straight alpha (`raster::unpremultiply`), so the blend is the
    // mask path's.
    return textureSample(atlas, atlas_sampler, in.uv);
}
