// The UI kit's two pipelines: `ui_quad_*` (a box: fill, border, soft shadow,
// per-corner radii) and `ui_glyph_*` (one glyph of the proportional text atlas).
// Read by `crate::renderer::ui`; `rounded_box_sdf` (`sdf.wgsl`) is appended at
// compile time, as to `cell_bg.wgsl`, so the corner curve has one copy.
//
// Everything is in PIXELS of the surface, top-left origin, and colours are
// LINEAR straight RGBA: the target is `Bgra8UnormSrgb` and the hardware
// encodes on write (`renderer::FORMAT`). Adding a gamma step here would
// encode the palette twice.
//
// There is one immediate block: the surface's size, for the NDC scale. Its
// Rust twin is `crate::renderer::ui::UiImmediates` (8 bytes, no padding).
//
// Both draws are instanced triangle strips over the unit square, 4 vertices
// per instance, like the grid's quads.

struct Immediates {
    viewport_px: vec2<f32>,
}

var<immediate> imm: Immediates;

// The text atlas: `R8Unorm` coverage. Read with `textureLoad` — every glyph's
// quad is its bitmap's size at a whole pixel, so a texel is a pixel and no
// sampler (and none of a sampler's blur) is involved. The layout still has
// the atlas planes' sampler @1; this module does not name it.
@group(0) @binding(0) var atlas: texture_2d<f32>;

// ---------------------------------------------------------------------------
// Boxes
// ---------------------------------------------------------------------------
//
// Rust twin: `UiQuad`, #[repr(C)], seven `[f32; 4]`, stride 112:
//   rect@0          x, y, width, height of the box
//   fill@16         the background; straight alpha
//   border_color@32 the border; straight alpha
//   shadow_color@48 the shadow; straight alpha, alpha 0 = none
//   radii@64        top-left, top-right, bottom-right, bottom-left
//   clip@80         x0, y0, x1, y1: nothing is drawn outside
//   params@96       border width, shadow blur, shadow dx, shadow dy
//
// The border is drawn INSIDE the rect, over the fill (the fill runs under it,
// as CSS's `background-clip: border-box`). The shadow is the box's silhouette
// offset by (dx, dy) and blurred; `blur` is the CSS blur radius, so the
// Gaussian's sigma is half of it. It is never drawn under the box itself.

struct QuadIn {
    @location(0) rect: vec4<f32>,
    @location(1) fill: vec4<f32>,
    @location(2) border_color: vec4<f32>,
    @location(3) shadow_color: vec4<f32>,
    @location(4) radii: vec4<f32>,
    @location(5) clip: vec4<f32>,
    @location(6) params: vec4<f32>,
}

struct QuadOut {
    @builtin(position) position: vec4<f32>,
    // The instance is constant across the quad: `flat` drops interpolation.
    @location(0) @interpolate(flat) rect: vec4<f32>,
    @location(1) @interpolate(flat) fill: vec4<f32>,
    @location(2) @interpolate(flat) border_color: vec4<f32>,
    @location(3) @interpolate(flat) shadow_color: vec4<f32>,
    @location(4) @interpolate(flat) radii: vec4<f32>,
    @location(5) @interpolate(flat) clip: vec4<f32>,
    @location(6) @interpolate(flat) params: vec4<f32>,
}

@vertex
fn ui_quad_vertex(@builtin(vertex_index) vid: u32, it: QuadIn) -> QuadOut {
    let corner = vec2<f32>(f32(vid & 1u), f32(vid >> 1u));
    // The quad is the box grown by what is drawn around it: a pixel for the
    // edge's antialiasing, and where a shadow is, its reach — three sigmas
    // (1.5 × the blur) past the box's offset silhouette.
    var lo = it.rect.xy;
    var hi = it.rect.xy + it.rect.zw;
    if it.shadow_color.a > 0.0 {
        let reach = it.params.y * 1.5 + 1.0;
        let off = it.params.zw;
        lo = min(lo, lo + off - reach);
        hi = max(hi, hi + off + reach);
    } else {
        lo = lo - 1.0;
        hi = hi + 1.0;
    }
    let at = mix(lo, hi, corner);
    let ndc = at / imm.viewport_px * 2.0 - 1.0;
    var o: QuadOut;
    // Pixel space starts at the top left, NDC at the bottom left: flip y.
    o.position = vec4<f32>(ndc.x, -ndc.y, 0.0, 1.0);
    o.rect = it.rect;
    o.fill = it.fill;
    o.border_color = it.border_color;
    o.shadow_color = it.shadow_color;
    o.radii = it.radii;
    o.clip = it.clip;
    o.params = it.params;
    return o;
}

// The corner radius of the quadrant `p` (relative to the centre) is in.
// Window space has y pointing down, so `p.y < 0` is the top half; radii are
// top-left, top-right, bottom-right, bottom-left.
fn corner_radius(p: vec2<f32>, radii: vec4<f32>) -> f32 {
    return select(
        select(radii.z, radii.w, p.x < 0.0),
        select(radii.y, radii.x, p.x < 0.0),
        p.y < 0.0,
    );
}

@fragment
fn ui_quad_fragment(in: QuadOut) -> @location(0) vec4<f32> {
    let p = in.position.xy;
    if p.x < in.clip.x || p.y < in.clip.y || p.x > in.clip.z || p.y > in.clip.w {
        discard;
    }
    let half_size = in.rect.zw * 0.5;
    let rel = p - (in.rect.xy + half_size);
    let radius = corner_radius(rel, in.radii);
    let d = rounded_box_sdf(rel, half_size, radius);

    // Coverage of the outer edge, and of the inside of the border.
    let outer = 1.0 - smoothstep(-0.5, 0.5, d);
    let width = in.params.x;
    var inner = outer;
    if width > 0.0 {
        inner = 1.0 - smoothstep(-0.5, 0.5, d + width);
    }

    // Premultiplied layers, top over bottom: the border ring over the fill
    // over the shadow.
    let fill_a = in.fill.a * outer;
    var rgb = in.fill.rgb * fill_a;
    var a = fill_a;
    let ring_a = in.border_color.a * max(outer - inner, 0.0);
    rgb = in.border_color.rgb * ring_a + rgb * (1.0 - ring_a);
    a = ring_a + a * (1.0 - ring_a);

    if in.shadow_color.a > 0.0 {
        // The silhouette's distance is read at the point shifted against the
        // offset. A Gaussian-blurred edge is the normal CDF of the distance;
        // the logistic curve 1 / (1 + e^(1.702 x)) is its usual closed form.
        let sigma = max(in.params.y * 0.5, 0.5);
        let shifted = rounded_box_sdf(rel - in.params.zw, half_size, radius);
        let x = clamp(1.702 * shifted / sigma, -30.0, 30.0);
        // Not under the box: the shadow is what the box does NOT cover.
        let outside = smoothstep(-0.5, 0.5, d);
        let shadow_a = in.shadow_color.a * outside / (1.0 + exp(x));
        rgb = rgb + in.shadow_color.rgb * shadow_a * (1.0 - a);
        a = a + shadow_a * (1.0 - a);
    }

    // Back to straight alpha for the pipeline's `SrcAlpha` blend.
    return vec4<f32>(rgb / max(a, 0.00001), a);
}

// ---------------------------------------------------------------------------
// Glyphs
// ---------------------------------------------------------------------------
//
// Rust twin: `UiGlyph`, #[repr(C)], three `[f32; 4]` and a `[f32; 2]` with its
// explicit pad, stride 64:
//   rect@0   x, y, width, height of the quad on the surface (whole pixels)
//   color@16 linear straight RGBA of the text
//   clip@32  x0, y0, x1, y1
//   src@48   the bitmap's top-left corner in the atlas, texels

struct GlyphIn {
    @location(0) rect: vec4<f32>,
    @location(1) color: vec4<f32>,
    @location(2) clip: vec4<f32>,
    @location(3) src: vec4<f32>,
}

struct GlyphOut {
    @builtin(position) position: vec4<f32>,
    @location(0) texel: vec2<f32>,
    @location(1) @interpolate(flat) color: vec4<f32>,
    @location(2) @interpolate(flat) clip: vec4<f32>,
}

@vertex
fn ui_glyph_vertex(@builtin(vertex_index) vid: u32, it: GlyphIn) -> GlyphOut {
    let corner = vec2<f32>(f32(vid & 1u), f32(vid >> 1u));
    let at = it.rect.xy + corner * it.rect.zw;
    let ndc = at / imm.viewport_px * 2.0 - 1.0;
    var o: GlyphOut;
    o.position = vec4<f32>(ndc.x, -ndc.y, 0.0, 1.0);
    // The atlas's own y starts at the top too: the texel runs the quad's way.
    o.texel = it.src.xy + corner * it.rect.zw;
    o.color = it.color;
    o.clip = it.clip;
    return o;
}

@fragment
fn ui_glyph_fragment(in: GlyphOut) -> @location(0) vec4<f32> {
    let p = in.position.xy;
    if p.x < in.clip.x || p.y < in.clip.y || p.x > in.clip.z || p.y > in.clip.w {
        discard;
    }
    let coverage = textureLoad(atlas, vec2<i32>(floor(in.texel)), 0).r;
    return vec4<f32>(in.color.rgb, in.color.a * coverage);
}
