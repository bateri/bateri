// The `mip` pipeline: one level of an image's mip chain, made from the level above it.
// Read by `crate::renderer::ui`.
//
// A single triangle covers the target level, and each of its pixels is the average of the 2×2
// block of the level above it stands over (a side of an odd length leaves its last row or
// column out, as the chain's sizes are halved and rounded down; a side of one stays one and its
// block is the same texel twice).
//
// The bytes are what a page's decoder made: sRGB, premultiplied by alpha, in an `Rgba8Unorm`
// texture. They are averaged as they are, the order a filter wants (a transparent texel
// contributes no colour) and the order `ui.wgsl` samples them in; the target is the same
// format and keeps all four channels, so the pipeline does not blend. The texels are read
// whole (`textureLoad`) and not through a filter: a tap that does not stand exactly on the
// corner of its block would weigh the texels unequally, and the error would grow with every
// level.
//
// The layout is the atlas planes' (texture @0, sampler @1); the sampler is not used.

@group(0) @binding(0) var source: texture_2d<f32>;

struct Out {
    @builtin(position) position: vec4<f32>,
}

@vertex
fn mip_vertex(@builtin(vertex_index) vid: u32) -> Out {
    // vid 0..2 → (0,0) (2,0) (0,2): one triangle that covers the unit square (and the target
    // with it); `corner` is the unit-square coordinate.
    let corner = vec2<f32>(f32((vid << 1u) & 2u), f32(vid & 2u));
    var o: Out;
    o.position = vec4<f32>(corner.x * 2.0 - 1.0, 1.0 - corner.y * 2.0, 0.0, 1.0);
    return o;
}

@fragment
fn mip_fragment(in: Out) -> @location(0) vec4<f32> {
    let last = vec2<i32>(textureDimensions(source, 0)) - vec2<i32>(1);
    let base = vec2<i32>(in.position.xy) * 2;
    let a = textureLoad(source, min(base, last), 0);
    let b = textureLoad(source, min(base + vec2<i32>(1, 0), last), 0);
    let c = textureLoad(source, min(base + vec2<i32>(0, 1), last), 0);
    let d = textureLoad(source, min(base + vec2<i32>(1, 1), last), 0);
    return (a + b + c + d) * 0.25;
}
