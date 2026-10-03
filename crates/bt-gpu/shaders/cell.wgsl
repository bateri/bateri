// The `cell` pipeline (glyphs and rules, sampling the atlas's mask plane) and
// `emoji_fragment` (the colour plane). Read by `crate::renderer`.
//
// **Emoji shares `cell_vertex` verbatim**; only the fragment differs.
// Both fragments sample with the same
// nearest/clamp sampler (`cell_fragment` says why).
//
// Rust counterpart: bt_gpu::frame::GlyphInstance, #[repr(C)]
// { pos: [f32; 2], uv0: [f32; 2], rgba: [f32; 4] }, stride 32. Like
// `cell_bg.wgsl`, instances come from an instance-stepped vertex buffer; the
// layout is written in `crate::renderer` from `GlyphInstance`'s `offset_of!`
// (pos@0, uv0@8, rgba@16).
//
// `size` and the uv size are NOT in the instance: every glyph is exactly one
// SLOT (fixed slot grid) and both are constant over the frame, so they come as
// immediates. A side effect is that the layout packs without padding; a
// `size` field in between would push `rgba` and grow the stride.
//
// **The quad is the slot, not the cell**: below `line_height` /
// `letter_spacing = 1` the slot (the glyph's own metric) is larger than the
// grid cell and the glyph spills into its neighbours. `pos` is still the
// CELL's corner; the slot's corner is `slot_offset` up and left of it
// (`bt_atlas::Atlas::slot_offset`). At `>= 1` the two are equal and the
// offset is zero, i.e. the quad is today's cell.

struct GlyphInstance {
    // The cell's top-left corner, pixels.
    @location(0) pos: vec2<f32>,
    // The slot's top-left corner in the atlas, normalised.
    @location(1) uv0: vec2<f32>,
    // Linear RGBA: the target is Bgra8UnormSrgb and the ROP does the encoding.
    // Adding a gamma correction here or in the fragment would encode the
    // palette TWICE.
    @location(2) rgba: vec4<f32>,
}

// The vertex sizes and the fragment's cursor block share one `var<immediate>`
// block (a module has one; both stages share it).
//
// **Order follows alignment**: the two `vec4`s first (the `CursorBlock`
// struct, rect@0 rgba@16), then the `vec2`s: viewport_px@32, slot_px@40,
// uv_size@48, slot_offset@56, then the `f32` lift@64. 68 bytes round up to the
// struct's 16-byte alignment: size 80. The Rust twin is
// `crate::renderer::GlyphImmediates`; it embeds `CursorBlock` as is and
// spells the trailing 12 bytes as an explicit `pad`, pinned by
// `offset_of`/`size_of` asserts.
//
//   slot_px     = the atlas's slot size (its slot metric), pixels.
//   slot_offset = where the grid cell sits inside the slot (x, y), pixels:
//                 the quad starts this much up and left of `pos`.
//   lift        = how many pixels the viewport was raised above the surface's
//                 origin so the overflow above the top row is not clipped
//                 (`crate::renderer::Renderer::plan`); added back here, so a
//                 glyph lands on the same window pixel. Zero at `>= 1`.
//
//   cursor_rect = the caret block's pixel rectangle (x0, y0, x1, y1), top-left
//                 origin, WINDOW space. Min/max, so the fragment test is two
//                 comparisons with no addition. An invisible cursor is a
//                 degenerate rectangle (all zero) — there is no separate flag,
//                 because a flag and a rectangle would be two truths that can
//                 disagree.
//   cursor_rgba = the colour of the text UNDER the block, linear; rgb comes
//                 from bt_core::Cursor::text. The ALPHA IS NOT A COLOUR but
//                 this frame's caret opacity (bt_gpu::motion::Motion::alpha):
//                 under Reduce Motion the caret fades in at its new cell and
//                 the block's alpha is written here too. Read below as the
//                 mix factor.
struct Immediates {
    cursor_rect: vec4<f32>,
    cursor_rgba: vec4<f32>,
    viewport_px: vec2<f32>,
    slot_px: vec2<f32>,
    uv_size: vec2<f32>,
    slot_offset: vec2<f32>,
    lift: f32,
}

var<immediate> imm: Immediates;

// One plane per bind group: the mask (`R8Unorm`: one channel of coverage —
// the atlas holds a mask per glyph, not an image; the colour comes from the
// instance) or the colour plane (`Rgba8UnormSrgb`, which the hardware decodes
// to linear when sampling).
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
    let at = it.pos - imm.slot_offset + corner * imm.slot_px + vec2<f32>(0.0, imm.lift);
    let ndc = at / imm.viewport_px * 2.0 - 1.0;
    var o: Out;
    // Pixel space starts at the top left, NDC at the bottom left: flip y. The
    // atlas's own y also starts at the top (its rows are uploaded top-down),
    // so the uv is NOT flipped — the two run in the same direction.
    o.position = vec4<f32>(ndc.x, -ndc.y, 0.0, 1.0);
    o.uv = it.uv0 + corner * imm.uv_size;
    o.rgba = it.rgba;
    return o;
}

@fragment
fn cell_fragment(in: Out) -> @location(0) vec4<f32> {
    // The sampler is `nearest`, NOT `linear`. In the usual one-to-one case
    // they agree (fragment centres land on texel centres). They diverge on the
    // frame between a scale change and the next geometry event: if the quad
    // is wider than the slot, `linear`'s last column blends the neighbour
    // slot's first column. Slots have no padding between them and
    // `clamp_to_edge` clamps only at the texture's edge, not the slot's; the
    // neighbour is another glyph. `nearest` always stays inside the slot: on
    // that frame the glyph looks blocky, but it never reads stray coverage.
    let coverage = textureSample(atlas, atlas_sampler, in.uv).r;
    // Text under the caret block (glyph AND rule) takes its colour from the
    // immediates: the block is opaque and a letter in its own foreground under
    // it would be unreadable. The decision is bt-core's
    // (bt_core::Cursor::text); this only asks "is this fragment inside the
    // rectangle" — and because the question is asked per PIXEL, a block
    // standing between two cells overrides half a cell and leaves the other
    // half in its own colour.
    //
    // `@builtin(position)` is pixel-centred, top-left-origin, but it is NOT in
    // the same space as the instance positions: the viewport offsets the grid
    // so content sticks to the bottom, i.e. instance space is BEFORE the
    // transform, `position` AFTER, and the two differ by exactly the offset.
    // The cursor rectangle is therefore filled asymmetrically
    // (`Frame::grid_caret`: instance `pos - origin`, rectangle raw `pos`), and
    // a simplification that "fixes" the asymmetry moves the colour of the text
    // under the caret to another row. The subtraction happens at READ time,
    // because the offset can still change after the caret entered the sink;
    // the dock slot's twin does the same with `origin_y` (`Frame::dock_caret`).
    //
    // The bound is half-open: [x0, x1) — the neighbour cell's first column
    // does not belong to this block. `<=` gives the SAME result today and no
    // guard tests it: `position` is the fragment CENTRE (x + 0.5), so no
    // fragment lands exactly on the bound. It is written half-open for the
    // future — motion's intermediate positions will put the rectangle on half
    // pixels, and there the two forms diverge.
    let p = in.position.xy;
    let inside = all(p >= imm.cursor_rect.xy) && all(p < imm.cursor_rect.zw);
    // A BLEND, not an override, and its factor is the immediate's alpha: while
    // the block fades in (Reduce Motion) the letter must fade with it, or it
    // is painted in the colour of a block not yet visible — a
    // background-coloured letter on the background. Outside the fade the
    // alpha is 1.0, so the blend reduces to a full override.
    //
    // The blend is ONLY in RGB: the output alpha comes from coverage below,
    // and were the caret's opacity to leak into it the glyph's edge would be
    // silently thinned. The block's own transparency is drawn by `cell_bg`.
    // WGSL has no ternary: `select(if_false, if_true, cond)`.
    let rgb = mix(in.rgba.rgb, imm.cursor_rgba.rgb, select(0.0, imm.cursor_rgba.a, inside));
    // Straight alpha: the blend is src_alpha / one_minus_src_alpha.
    return vec4<f32>(rgb, in.rgba.a * coverage);
}

// **Emoji: the colour plane's sibling fragment.** It shares `cell_vertex`
// VERBATIM (as `caret_fragment` shares `cell_bg_vertex`): only the fragment
// differs, because the geometry is identical — a full one-slot quad, the same
// `slot_px` and `uv_size`, the same 32-byte `GlyphInstance`.
//
// The texture is `Rgba8UnormSrgb`: the hardware DECODES sRGB when sampling, so
// the value here is linear, in the space the target (`Bgra8UnormSrgb`)
// expects. A plain `Rgba8Unorm` texture would silently get this line wrong —
// the palette would wash out, and only a MIDTONE pixel would witness it
// (`0.0` and `1.0` are the transfer function's fixed points).
//
// The bytes are STRAIGHT ALPHA. CoreGraphics writes a colour glyph
// premultiplied, but bt-atlas undoes it before upload (`raster::unpremultiply`)
// and the reason is the SPACE: CG's context is sRGB, so the stored value is
// `encode(c)·a` — the encoded component times alpha. This texture's channel
// decode is independent of alpha and the sRGB decode is convex, so
// `decode(encode(c)·a) < decode(encode(c))·a`: every antialiased edge would
// shift dark (half-transparent white on black gives 0x80 instead of 0xBC).
// With the premultiplication undone this pipeline's blend is the SAME as the
// mask path's — RGB source factor `SrcAlpha`.
@fragment
fn emoji_fragment(in: Out) -> @location(0) vec4<f32> {
    // Same `nearest` sampler as the mask path, for the same reason (no padding
    // between slots; `linear`'s last column would blend the neighbour).
    //
    // `in.rgba` is NOT read and must not be: the colour comes from the
    // texture, not the instance. Multiplying by the foreground would paint the
    // emoji in the text colour — the exact opposite of the mask path, and the
    // reason this plane exists.
    //
    // The cursor block is NOT read either: `cell_fragment`'s `mix` turns the
    // letter under it into the caret's text colour and that is a PALETTE
    // decision (`bt_core::Cursor::text`). An emoji's colour does not come from
    // the palette, so "what colour is the emoji under the caret" has no
    // answer from the theme. Result: the block caret sits under the emoji's
    // ink and shows as a ring around it. Accepted behaviour.
    return textureSample(atlas, atlas_sampler, in.uv);
}
