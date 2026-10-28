// The UI kit's three pipelines: `ui_quad_*` (a box: fill, border, soft shadow,
// per-corner radii), `ui_glyph_*` (one glyph of the proportional text atlas) and
// `ui_image_*` (a page's image: a textured rounded rectangle).
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
// All three draws are instanced triangle strips over the unit square, 4 vertices
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
// Rust twin: `UiQuad`, #[repr(C)], ten `[f32; 4]`, stride 160:
//   rect@0            x, y, width, height of the box
//   fill@16           the background; straight alpha
//   border_color@32   the border; straight alpha
//   shadow_color@48   the shadow; straight alpha, alpha 0 = none
//   radii@64          top-left, top-right, bottom-right, bottom-left
//   clip@80           x0, y0, x1, y1: nothing is drawn outside
//   params@96         border width, shadow blur, shadow dx, shadow dy
//   gradient_start@112 where a fill from one colour to another starts; straight alpha
//   gradient_end@128  and where it ends
//   gradient@144      the way it runs (x, y: a unit vector, y down), and z 1 where there is one
//
// A fill from one colour to another is a style sheet's `linear-gradient`: the line runs through
// the middle of the box, as long as it takes for its ends to be the corners it points at, and the
// colours are mixed in sRGB (premultiplied), as a style sheet mixes them. It lies over the fill.
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
    @location(7) gradient_start: vec4<f32>,
    @location(8) gradient_end: vec4<f32>,
    @location(9) gradient: vec4<f32>,
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
    @location(7) @interpolate(flat) gradient_start: vec4<f32>,
    @location(8) @interpolate(flat) gradient_end: vec4<f32>,
    @location(9) @interpolate(flat) gradient: vec4<f32>,
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
    o.gradient_start = it.gradient_start;
    o.gradient_end = it.gradient_end;
    o.gradient = it.gradient;
    return o;
}

// sRGB's curve, each way, for the mixing of a gradient.
fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    return select(1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055, c * 12.92, c <= vec3<f32>(0.0031308));
}

fn from_srgb(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

// The colour of the gradient at `rel` (from the box's middle), straight alpha, linear.
fn gradient_at(rel: vec2<f32>, half_size: vec2<f32>, way: vec2<f32>, start: vec4<f32>, end: vec4<f32>) -> vec4<f32> {
    // Half the length of the line: the reach of the corner it points at along it.
    let reach = abs(half_size.x * way.x) + abs(half_size.y * way.y);
    let t = clamp(dot(rel, way) / max(reach, 0.0001) * 0.5 + 0.5, 0.0, 1.0);
    // Premultiplied, in sRGB.
    let a = mix(start.a, end.a, t);
    let rgb = mix(to_srgb(start.rgb) * start.a, to_srgb(end.rgb) * end.a, t);
    return vec4<f32>(from_srgb(rgb / max(a, 0.00001)), a);
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
    if in.gradient.z > 0.0 {
        let g = gradient_at(rel, half_size, in.gradient.xy, in.gradient_start, in.gradient_end);
        let g_a = g.a * outer;
        rgb = g.rgb * g_a + rgb * (1.0 - g_a);
        a = g_a + a * (1.0 - g_a);
    }
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

// ---------------------------------------------------------------------------
// Images
// ---------------------------------------------------------------------------
//
// Rust twin: `UiImage`, #[repr(C)], six `[f32; 4]`, stride 96:
//   rect@0   x, y, width, height of the rectangle the image is drawn in
//   uv@16    u0, v0, u1, v1: the part of the texture the rectangle shows,
//            shares of it (rows run top down, like the window's)
//   tint@32  linear straight RGBA; used where params.y is 1
//   radii@48 top-left, top-right, bottom-right, bottom-left
//   clip@64  x0, y0, x1, y1
//   params@80 opacity, tinted (0 or 1), turn, unused
//            the turn: radians clockwise about the middle of the rectangle (0 for an image that
//            does not spin); the clip stays where it is
//
// One draw per run of instances over ONE texture (each has its own bind group;
// group 0 is the same layout the atlas planes have: texture @0, sampler @1).
// The `atlas` of the glyphs sits at the same slot, and an entry point's
// resources are those it uses: the glyph draw never names `picture`, this one
// never names `atlas`.
//
// THE TEXTURE IS NOT sRGB-FORMATTED. Its bytes are `Rgba8Unorm`: sRGB,
// premultiplied by alpha (a decoder's output: `pencere-sandbox`'s bitmaps),
// that is the stored colour is `encoded(c) * a`. The hardware's sRGB decode
// would take `encoded(c) * a` for an encoded colour and darken every soft
// edge, so the sampler filters the premultiplied bytes as they are (the right
// order for a filter: a transparent texel contributes no colour), and this
// shader divides alpha out and decodes by hand. Then it is linear straight
// RGBA like every other colour here.
//
// A TINT paints the shape: every pixel is the tint's colour with the image's
// alpha at that pixel (times the tint's own and the opacity).
//
// The texture has its whole chain of levels (`mip.wgsl` makes them), and a pixel reads the
// level that suits the rectangle's size relative to the file, trilinearly.

struct ImageIn {
    @location(0) rect: vec4<f32>,
    @location(1) uv: vec4<f32>,
    @location(2) tint: vec4<f32>,
    @location(3) radii: vec4<f32>,
    @location(4) clip: vec4<f32>,
    @location(5) params: vec4<f32>,
}

struct ImageOut {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) rect: vec4<f32>,
    @location(1) @interpolate(flat) uv: vec4<f32>,
    @location(2) @interpolate(flat) tint: vec4<f32>,
    @location(3) @interpolate(flat) radii: vec4<f32>,
    @location(4) @interpolate(flat) clip: vec4<f32>,
    @location(5) @interpolate(flat) params: vec4<f32>,
}

@group(0) @binding(0) var picture: texture_2d<f32>;
@group(0) @binding(1) var picture_sampler: sampler;

@vertex
fn ui_image_vertex(@builtin(vertex_index) vid: u32, it: ImageIn) -> ImageOut {
    let corner = vec2<f32>(f32(vid & 1u), f32(vid >> 1u));
    // The rectangle grown by a pixel: the rounded edge's antialiasing. A turned one is drawn in
    // the square its corners can reach, whatever the turn.
    let half_size = it.rect.zw * 0.5;
    let center = it.rect.xy + half_size;
    var reach = half_size + 1.0;
    if it.params.z != 0.0 {
        reach = vec2<f32>(length(half_size) + 1.0);
    }
    let at = mix(center - reach, center + reach, corner);
    let ndc = at / imm.viewport_px * 2.0 - 1.0;
    var o: ImageOut;
    o.position = vec4<f32>(ndc.x, -ndc.y, 0.0, 1.0);
    o.rect = it.rect;
    o.uv = it.uv;
    o.tint = it.tint;
    o.radii = it.radii;
    o.clip = it.clip;
    o.params = it.params;
    return o;
}

// The sRGB transfer function's inverse: encoded 0..1 to linear light.
fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, c <= vec3<f32>(0.04045));
}

@fragment
fn ui_image_fragment(in: ImageOut) -> @location(0) vec4<f32> {
    let p = in.position.xy;
    if p.x < in.clip.x || p.y < in.clip.y || p.x > in.clip.z || p.y > in.clip.w {
        discard;
    }
    // A rectangle with no area shows nothing, and the shares below divide by its sides.
    if !(in.rect.z > 0.0 && in.rect.w > 0.0) {
        discard;
    }
    let half_size = in.rect.zw * 0.5;
    let center = in.rect.xy + half_size;
    // Where this pixel is in the rectangle as the file has it: a turned image is the file turned
    // clockwise about the rectangle's middle, so the pixel is turned back the same way round.
    var rel = p - center;
    if in.params.z != 0.0 {
        let c = cos(in.params.z);
        let s = sin(in.params.z);
        rel = vec2<f32>(c * rel.x + s * rel.y, c * rel.y - s * rel.x);
    }
    let d = rounded_box_sdf(rel, half_size, corner_radius(rel, in.radii));
    let inside = 1.0 - smoothstep(-0.5, 0.5, d);

    // The level of the chain this pixel is read from: the texels of the file one pixel of the
    // rectangle spans, as a power of two. At one texel a pixel (an image drawn at its own size)
    // or fewer it is the file itself; an image drawn smaller than its file reads the level that
    // is about as coarse as the rectangle, and does not shimmer as it moves. Computed from the
    // rectangle and not by the sampler's derivatives, which would be the neighbouring pixels'
    // (and the image's edge).
    let dims = vec2<f32>(textureDimensions(picture, 0));
    let spanned = abs(in.uv.zw - in.uv.xy) * dims / in.rect.zw;
    let lod = log2(max(max(spanned.x, spanned.y), 1.0));

    // Where in the rectangle this pixel is, held to it: the antialiasing pixel past the edge
    // must not read beyond the part of the texture the rectangle shows. And the filter does
    // not read beyond it either: the point it reads is held half a texel of the level it reads
    // inside the part the rectangle shows (a crop that cuts the file is the part's edge, and a
    // bilinear read there is half the texel on the other side of it). A rectangle that shows
    // the file at its own size reads texel centres and is not moved by this.
    let share = clamp((center + rel - in.rect.xy) / in.rect.zw, vec2<f32>(0.0), vec2<f32>(1.0));
    let near = min(in.uv.xy, in.uv.zw);
    let far = max(in.uv.xy, in.uv.zw);
    let middle = (near + far) * 0.5;
    let reach = 0.5 * exp2(ceil(lod)) / dims;
    let at = clamp(
        mix(in.uv.xy, in.uv.zw, share),
        min(near + reach, middle),
        max(far - reach, middle),
    );
    let texel = textureSampleLevel(picture, picture_sampler, at, lod);

    var rgb = in.tint.rgb;
    var alpha = texel.a * in.params.x * inside;
    if in.params.y > 0.5 {
        alpha = alpha * in.tint.a;
    } else {
        rgb = srgb_to_linear(texel.rgb / max(texel.a, 0.00001));
    }
    return vec4<f32>(rgb, alpha);
}
