// WGSL twin of `glyph_fx.metal` (040 phase-4): the dock's typing effects —
// the arrival of a typed glyph and the ghost of an erased one. Today only the
// `cfg(test)` wgpu renderer reads it (`crate::wgpu_renderer`); Metal stays as
// the oracle and both backends' output is compared pixel by pixel over one
// scene list (discussion.md → Karar 4). The full rationale — every constant,
// every branch, the inverse transform — lives in the `.metal`; only what
// DIFFERS between the two languages is written here.
//
// **Every sample is `textureSampleLevel(.., 0.0)`, never `textureSample`.**
// WGSL allows implicit-derivative sampling only in uniform control flow, and
// `paint` is reached through data-dependent branches, early returns and loops
// with `continue`. The atlas has one mip level, so level 0 is exactly what
// Metal's `sample` reads.
//
// Rust counterpart: bt_gpu::frame::FxInstance, #[repr(C)]
// { pos: [f32; 2], uv0: [f32; 2], rgba: [f32; 4], fx: [f32; 4] }, stride 48.
// Like the other WGSL modules, instances come from an instance-stepped vertex
// buffer whose layout `crate::wgpu_renderer` builds from `FxInstance`'s
// `offset_of!` (pos@0, uv0@8, rgba@16, fx@32).
//
//   fx.x = progress t; fx.y = id | plane << 5 | half << 6 as an INTEGER held
//   in an f32 (not a bit pattern); fx.z = seed; fx.w = spare.

struct FxInstance {
    @location(0) pos: vec2<f32>,
    @location(1) uv0: vec2<f32>,
    @location(2) rgba: vec4<f32>,
    @location(3) fx: vec4<f32>,
}

// Metal's vertex uniforms (`viewport_px`, `cell_px`) and fragment uniforms
// (`cell_px`, `uv_size`, `heat`) become one `var<immediate>` block. Order
// follows alignment: heat@0 (vec4), viewport_px@16, cell_px@24, uv_size@32;
// 40 bytes round up to the struct's 16-byte alignment: size 48. The Rust twin
// is `crate::wgpu_renderer::FxImmediates`, pinned by `offset_of`/`size_of`
// asserts and an explicit trailing `pad`.
struct Immediates {
    heat: vec4<f32>,
    viewport_px: vec2<f32>,
    cell_px: vec2<f32>,
    uv_size: vec2<f32>,
}

var<immediate> imm: Immediates;

// Both planes are bound at once — the plane is read from the instance. When
// no colour texture exists yet, the renderer binds the mask to slot 1 too
// (Metal does the same); no colour-plane instance exists then, so it is never
// read as colour.
@group(0) @binding(0) var mask_tex: texture_2d<f32>;
@group(0) @binding(1) var color_tex: texture_2d<f32>;
// `near` is the `cell` pipeline's nearest/clamp sampler, `lin` the linear one
// for the scaling branches (`paint`'s doc in the `.metal`).
@group(0) @binding(2) var near: sampler;
@group(0) @binding(3) var lin: sampler;

// Effect ids — `glyph_fx::Effect::id`. Arrivals 1..16, ghosts 16..32.
const FX_FADE: u32 = 1u;
const FX_RISE: u32 = 2u;
const FX_POP: u32 = 3u;
const FX_EXTRUDE: u32 = 4u;
const FX_HEAT: u32 = 5u;
const FX_ECHO: u32 = 6u;
const FX_DROP: u32 = 7u;
const FX_INK: u32 = 8u;
const FX_SQUEEZE: u32 = 9u;
const FX_RECEDE: u32 = 16u;
const FX_IRIS: u32 = 17u;
const FX_UNDERTOW: u32 = 18u;
const FX_GHOST_ECHO: u32 = 19u;
const FX_BLEED: u32 = 20u;
const FX_UNRAVEL: u32 = 21u;
const FX_SUBLIME: u32 = 22u;
const FX_SHATTER: u32 = 23u;
const FX_GHOST_FIRST: u32 = 16u;

// Design constants, in cell ratios — the same numbers as the `.metal`, where
// each one's reason is written.
const FX_PAD: f32 = 1.5;
const RISE_DISTANCE: f32 = 0.4;
const POP_START: f32 = 0.3;
const POP_BACK: f32 = 2.2;
const EXTRUDE_START: f32 = 0.05;
const ECHO_SCALE: f32 = 2.2;
const ECHO_ALPHA: f32 = 0.7;
const DROP_HEIGHT: f32 = 0.5;
const DROP_BACK: f32 = 1.7;
const INK_SOFTNESS: f32 = 0.35;
const SQUEEZE_X: f32 = 0.4;
const SQUEEZE_Y: f32 = 1.45;
const SQUEEZE_BACK: f32 = 1.5;
const RECEDE_SCALE: f32 = 0.3;
const IRIS_REACH: f32 = 0.7;
const UNDERTOW_X: f32 = 0.8;
const UNDERTOW_Y: f32 = 0.7;
const UNDERTOW_SCALE: f32 = 0.75;
const ECHO_OUT_SCALE: f32 = 1.8;
const BLEED_RADIUS: f32 = 0.3;
const UNRAVEL_STRIPS: f32 = 5.0;
const UNRAVEL_SHIFT: f32 = 0.6;
const UNRAVEL_STAGGER: f32 = 0.45;
const SUBLIME_RISE: f32 = 0.5;
const SUBLIME_OPEN_X: f32 = 1.4;
const SUBLIME_OPEN_Y: f32 = 1.1;
const SUBLIME_BLUR: f32 = 0.12;
const SHATTER_COLS: f32 = 2.0;
const SHATTER_COLS_WIDE: f32 = 3.0;
const SHATTER_ROWS: f32 = 2.0;
const SHATTER_SPREAD: f32 = 0.5;
const SHATTER_FALL: f32 = 1.2;
const SHATTER_SPIN: f32 = 0.8;
const SHATTER_JITTER: f32 = 0.7;
const BLUR_TAPS: i32 = 16;

struct FxOut {
    @builtin(position) position: vec4<f32>,
    // Pixels from the cell's top-left corner; interpolated.
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) uv0: vec2<f32>,
    @location(2) @interpolate(flat) rgba: vec4<f32>,
    @location(3) @interpolate(flat) fx: vec4<f32>,
}

@vertex
fn glyph_fx_vertex(@builtin(vertex_index) vid: u32, it: FxInstance) -> FxOut {
    let corner = vec2<f32>(f32(vid & 1u), f32(vid >> 1u));
    let pad = imm.cell_px * FX_PAD;
    let local = -pad + corner * (imm.cell_px + 2.0 * pad);
    let ndc = (it.pos + local) / imm.viewport_px * 2.0 - 1.0;
    var o: FxOut;
    o.position = vec4<f32>(ndc.x, -ndc.y, 0.0, 1.0);
    o.local = local;
    o.uv0 = it.uv0;
    o.rgba = it.rgba;
    o.fx = it.fx;
    return o;
}

fn ease_out(t: f32) -> f32 {
    let u = 1.0 - t;
    return 1.0 - u * u;
}

fn ease_out_back(t: f32, back: f32) -> f32 {
    let u = t - 1.0;
    return 1.0 + (back + 1.0) * u * u * u + back * u * u;
}

// The glyph's paint at a point; the slot's outside is never sampled. The
// `.metal`'s `paint`, with the textures and samplers as module globals.
fn paint(g: vec2<f32>, uv0: vec2<f32>, fg: vec3<f32>, colored: bool, smooth_: bool) -> vec4<f32> {
    let cell_px = imm.cell_px;
    if (any(g < vec2<f32>(0.0)) || any(g >= cell_px)) {
        return vec4<f32>(0.0);
    }
    let texel = imm.uv_size / cell_px;
    if (!smooth_) {
        let uv = uv0 + g * texel;
        if (colored) {
            return textureSampleLevel(color_tex, near, uv, 0.0);
        }
        return vec4<f32>(fg, textureSampleLevel(mask_tex, near, uv, 0.0).r);
    }
    let q = clamp(g, vec2<f32>(0.5), cell_px - 0.5);
    if (colored) {
        // Four texels by hand, blended premultiplied (the `.metal` says why
        // the hardware filter cannot be used on straight-alpha bytes).
        let p = q - 0.5;
        let i0 = floor(p);
        let f = p - i0;
        let i1 = min(i0 + 1.0, cell_px - 1.0);
        var a = textureSampleLevel(color_tex, near, uv0 + (vec2<f32>(i0.x, i0.y) + 0.5) * texel, 0.0);
        var b = textureSampleLevel(color_tex, near, uv0 + (vec2<f32>(i1.x, i0.y) + 0.5) * texel, 0.0);
        var c = textureSampleLevel(color_tex, near, uv0 + (vec2<f32>(i0.x, i1.y) + 0.5) * texel, 0.0);
        var d = textureSampleLevel(color_tex, near, uv0 + (vec2<f32>(i1.x, i1.y) + 0.5) * texel, 0.0);
        a = vec4<f32>(a.rgb * a.a, a.a);
        b = vec4<f32>(b.rgb * b.a, b.a);
        c = vec4<f32>(c.rgb * c.a, c.a);
        d = vec4<f32>(d.rgb * d.a, d.a);
        let m = mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
        if (m.a > 0.0) {
            return vec4<f32>(m.rgb / m.a, m.a);
        }
        return vec4<f32>(0.0);
    }
    return vec4<f32>(fg, textureSampleLevel(mask_tex, lin, uv0 + q * texel, 0.0).r);
}

// Integer hash — bit-exact on every device (u32 arithmetic wraps in WGSL as
// in MSL).
fn hash(v: u32) -> u32 {
    var x = v;
    x = x ^ (x >> 16u);
    x = x * 0x7feb352du;
    x = x ^ (x >> 15u);
    x = x * 0x846ca68bu;
    x = x ^ (x >> 16u);
    return x;
}

fn rand01(seed: u32, k: u32) -> f32 {
    return f32(hash(seed * 0x9e3779b9u + k) & 0xffffu) / 65535.0;
}

// Average paint over a Vogel disc of `radius`; premultiplied average,
// straight-alpha result.
fn spread(g: vec2<f32>, radius: f32, uv0: vec2<f32>, fg: vec3<f32>, colored: bool) -> vec4<f32> {
    var sum = vec4<f32>(0.0);
    for (var i: i32 = 0; i < BLUR_TAPS; i++) {
        let r = radius * sqrt((f32(i) + 0.5) / f32(BLUR_TAPS));
        let a = f32(i) * 2.39996323;
        let s = paint(g + r * vec2<f32>(cos(a), sin(a)), uv0, fg, colored, true);
        sum += vec4<f32>(s.rgb * s.a, s.a);
    }
    sum /= f32(BLUR_TAPS);
    if (sum.a > 0.0) {
        return vec4<f32>(sum.rgb / sum.a, sum.a);
    }
    return vec4<f32>(0.0);
}

// Premultiplied "top wins" composite; straight-alpha result.
fn over(top: vec4<f32>, under: vec4<f32>) -> vec4<f32> {
    let a = top.a + under.a * (1.0 - top.a);
    let rgb = top.rgb * top.a + under.rgb * under.a * (1.0 - top.a);
    if (a > 0.0) {
        return vec4<f32>(rgb / a, a);
    }
    return vec4<f32>(0.0);
}

// `ink`'s depth: the mean coverage of the point's 3×3 neighbourhood,
// neighbours clamped into the cell.
fn ink_depth(g: vec2<f32>, uv0: vec2<f32>) -> f32 {
    let cell_px = imm.cell_px;
    var sum = 0.0;
    for (var dy: i32 = -1; dy <= 1; dy++) {
        for (var dx: i32 = -1; dx <= 1; dx++) {
            let q = clamp(g + vec2<f32>(f32(dx), f32(dy)), vec2<f32>(0.5), cell_px - 0.5);
            sum += textureSampleLevel(mask_tex, near, uv0 + q / cell_px * imm.uv_size, 0.0).r;
        }
    }
    return sum / 9.0;
}

// `shatter`'s paint: the darkest shard landing on the point. WGSL has no
// comma operator, so the shard counter `n` advances in the loop body.
fn shatter_paint(p: vec2<f32>, t: f32, e: f32, seed: u32, center: vec2<f32>,
                 box_left: f32, box_w: f32, wide: bool,
                 uv0: vec2<f32>, fg: vec3<f32>, colored: bool) -> vec4<f32> {
    let cell_px = imm.cell_px;
    let cols = select(SHATTER_COLS, SHATTER_COLS_WIDE, wide);
    let tile = vec2<f32>(box_w / cols, cell_px.y / SHATTER_ROWS);
    var best = vec4<f32>(0.0);
    var n = 0u;
    for (var row = 0.0; row < SHATTER_ROWS; row += 1.0) {
        for (var col = 0.0; col < cols; col += 1.0) {
            let k = n;
            n += 1u;
            let lo = vec2<f32>(box_left, 0.0) + vec2<f32>(col, row) * tile;
            let mid = lo + tile * 0.5;
            let away = mid - center;
            let len = length(away);
            var dir = select(vec2<f32>(0.0, 1.0), away / len, len > 0.5);
            let turn = (rand01(seed, k * 4u) - 0.5) * SHATTER_JITTER;
            dir = vec2<f32>(dir.x * cos(turn) - dir.y * sin(turn),
                            dir.x * sin(turn) + dir.y * cos(turn));
            let speed = mix(0.6, 1.0, rand01(seed, k * 4u + 1u));
            let offset = dir * (SHATTER_SPREAD * cell_px.x * speed * t)
                       + vec2<f32>(0.0, SHATTER_FALL * cell_px.y * t * t);
            let angle = (rand01(seed, k * 4u + 2u) - 0.5) * 2.0 * SHATTER_SPIN * e;
            let v = p - mid - offset;
            let cs = cos(angle);
            let sn = sin(angle);
            let q = mid + vec2<f32>(v.x * cs + v.y * sn, -v.x * sn + v.y * cs);
            if (any(q < lo) || any(q >= lo + tile)) {
                continue;
            }
            let c = paint(q, uv0, fg, colored, true);
            if (c.a > best.a) {
                best = c;
            }
        }
    }
    return best;
}

@fragment
fn glyph_fx_fragment(in: FxOut) -> @location(0) vec4<f32> {
    let cell_px = imm.cell_px;
    let packed = u32(round(in.fx.y));
    let id = packed & 31u;
    let colored = ((packed >> 5u) & 1u) != 0u;
    let half_ = (packed >> 6u) & 3u;
    let t = saturate(in.fx.x);
    let ghost = id >= FX_GHOST_FIRST;

    if (ghost && t >= 1.0) {
        return vec4<f32>(0.0);
    }

    // The transform's centre is the GLYPH's box, not the half's cell.
    let cx = select(select(0.0, cell_px.x, half_ == 1u), cell_px.x * 0.5, half_ == 0u);
    let center = vec2<f32>(cx, cell_px.y * 0.5);
    let box_left = select(0.0, -cell_px.x, half_ == 2u);
    let box_w = select(2.0 * cell_px.x, cell_px.x, half_ == 0u);

    var g = in.local;
    var alpha = 1.0;
    var fg = in.rgba.rgb;
    var smooth_ = false;
    var echo_g = vec2<f32>(0.0);
    var echo_alpha = 0.0;
    var ink_front = 0.0;
    var blur = 0.0;
    var iris = -1.0;
    var shatter = false;
    // The ends fall back to the static path's arithmetic (the `.metal` says
    // why): no inverse transform runs at an arrival's `t = 1` or a ghost's
    // `t = 0`.
    // Parenthesised: `a < b, c > d` would parse as a template list.
    let moving = select((t < 1.0), (t > 0.0), ghost);
    if (moving) {
        let e = ease_out(t);
        if (id == FX_FADE) {
            alpha = e;
        } else if (id == FX_RISE) {
            g.y -= RISE_DISTANCE * cell_px.y * (1.0 - e);
            alpha = e;
        } else if (id == FX_POP) {
            let s = mix(POP_START, 1.0, ease_out_back(t, POP_BACK));
            g = center + (in.local - center) / s;
            alpha = e;
            smooth_ = true;
        } else if (id == FX_EXTRUDE) {
            let s = mix(EXTRUDE_START, 1.0, e);
            g.x = box_left + (in.local.x - box_left) / s;
            alpha = e;
            smooth_ = true;
        } else if (id == FX_HEAT) {
            fg = mix(imm.heat.rgb, in.rgba.rgb, smoothstep(0.0, 1.0, t));
            if (colored) {
                alpha = e;
            }
        } else if (id == FX_ECHO) {
            alpha = e;
            let s = mix(1.0, ECHO_SCALE, e);
            echo_g = center + (in.local - center) / s;
            echo_alpha = ECHO_ALPHA * (1.0 - e) * (1.0 - e);
        } else if (id == FX_DROP) {
            g.y += DROP_HEIGHT * cell_px.y * (1.0 - ease_out_back(t, DROP_BACK));
            alpha = e;
        } else if (id == FX_INK) {
            if (colored) {
                alpha = e;
            } else {
                ink_front = 1.0 - t;
            }
        } else if (id == FX_SQUEEZE) {
            let b = ease_out_back(t, SQUEEZE_BACK);
            let s = vec2<f32>(mix(SQUEEZE_X, 1.0, b), mix(SQUEEZE_Y, 1.0, b));
            g = center + (in.local - center) / s;
            alpha = e;
            smooth_ = true;
        } else if (id == FX_RECEDE) {
            let s = mix(1.0, RECEDE_SCALE, e);
            g = center + (in.local - center) / s;
            alpha = 1.0 - e;
            smooth_ = true;
        } else if (id == FX_IRIS) {
            let reach = length(vec2<f32>(box_w, cell_px.y * IRIS_REACH) * 0.5) + 1.0;
            iris = reach * (1.0 - e);
        } else if (id == FX_UNDERTOW) {
            let pull = smoothstep(0.0, 1.0, t);
            let moved = in.local - vec2<f32>(-UNDERTOW_X * cell_px.x, UNDERTOW_Y * cell_px.y) * pull;
            g = center + (moved - center) / mix(1.0, UNDERTOW_SCALE, pull);
            alpha = 1.0 - smoothstep(0.25, 1.0, t);
            smooth_ = true;
        } else if (id == FX_GHOST_ECHO) {
            let s = mix(1.0, ECHO_OUT_SCALE, e);
            g = center + (in.local - center) / s;
            alpha = 1.0 - e;
            smooth_ = true;
        } else if (id == FX_BLEED) {
            blur = BLEED_RADIUS * cell_px.x * e;
            alpha = 1.0 - e;
            smooth_ = true;
        } else if (id == FX_UNRAVEL) {
            var strip = floor(in.local.y / (cell_px.y / UNRAVEL_STRIPS));
            strip = clamp(strip, 0.0, UNRAVEL_STRIPS - 1.0);
            let delay = strip / (UNRAVEL_STRIPS - 1.0) * UNRAVEL_STAGGER;
            let own = ease_out(saturate((t - delay) / (1.0 - UNRAVEL_STAGGER)));
            // WGSL's `%` on floats is the truncated remainder, Metal's `fmod`.
            let side = select(-1.0, 1.0, strip % 2.0 < 0.5);
            g.x -= side * UNRAVEL_SHIFT * cell_px.x * own;
            alpha = 1.0 - own;
        } else if (id == FX_SUBLIME) {
            let s = vec2<f32>(mix(1.0, SUBLIME_OPEN_X, e), mix(1.0, SUBLIME_OPEN_Y, e));
            let lifted = in.local + vec2<f32>(0.0, SUBLIME_RISE * cell_px.y * e);
            g = center + (lifted - center) / s;
            blur = SUBLIME_BLUR * cell_px.x * e;
            alpha = 1.0 - e;
            smooth_ = true;
        } else if (id == FX_SHATTER) {
            shatter = true;
            alpha = 1.0 - smoothstep(0.3, 1.0, t);
        }
    }

    var c: vec4<f32>;
    if (shatter) {
        let e = ease_out(t);
        let seed = u32(round(in.fx.z));
        c = shatter_paint(in.local, t, e, seed, center, box_left, box_w, half_ != 0u,
                          in.uv0, fg, colored);
    } else if (blur > 0.0) {
        c = spread(g, blur, in.uv0, fg, colored);
    } else {
        c = paint(g, in.uv0, fg, colored, smooth_);
    }
    if (ink_front > 0.0) {
        let depth = ink_depth(g, in.uv0);
        c.a *= smoothstep(ink_front - INK_SOFTNESS, ink_front, depth);
    }
    if (iris >= 0.0) {
        c.a *= saturate(iris - length(in.local - center) + 0.5);
    }
    c.a *= alpha;
    if (echo_alpha > 0.0) {
        var copy = paint(echo_g, in.uv0, fg, colored, true);
        copy.a *= echo_alpha;
        c = over(c, copy);
    }

    // No caret inversion: the effect is drawn over the caret in its own
    // colour. Mask plane: the sample's alpha times the instance's; colour
    // plane: the colour is the texture's.
    if (colored) {
        return c;
    }
    return vec4<f32>(c.rgb, in.rgba.a * c.a);
}
