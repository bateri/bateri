// The dock's typing effects: the arrival of a typed glyph and the ghost
// of an erased one. Read by `crate::renderer` (`Renderer::fx_draw`).
//
// **No geometry is generated; there is an inverse transform.** The quad is
// inflated from the cell by the effect margin (FX_PAD) and the fragment maps
// its own point into glyph space through the effect's INVERSE transform: a
// shrinking glyph's pixel is sampled from a growing point. If the result falls
// outside the slot there is NO sample — the neighbour slot is another glyph
// and slots have no padding between them.
//
// **Every sample is `textureSampleLevel(.., 0.0)`, never `textureSample`.**
// WGSL allows implicit-derivative sampling only in uniform control flow, and
// `paint` is reached through data-dependent branches, early returns and loops
// with `continue`. The atlas has one mip level, so level 0 is the whole
// texture.
//
// Rust counterpart: bt_gpu::frame::FxInstance, #[repr(C)]
// { pos: [f32; 2], uv0: [f32; 2], rgba: [f32; 4], fx: [f32; 4] }, stride 48.
// The first three fields sit where `GlyphInstance`'s do (`cell.wgsl`). Like
// the other WGSL modules, instances come from an instance-stepped vertex
// buffer whose layout `crate::renderer` builds from `FxInstance`'s
// `offset_of!` (pos@0, uv0@8, rgba@16, fx@32; no padding).
//
//   fx.x = progress t; fx.y = id | plane << 5 | half << 6 as an INTEGER held
//   in an f32 (not a bit pattern); fx.z = seed; fx.w = spare.

struct FxInstance {
    // The cell's top-left corner, pixels (dock-local).
    @location(0) pos: vec2<f32>,
    // The slot's top-left corner in the atlas, normalised.
    @location(1) uv0: vec2<f32>,
    // Linear foreground; `cell.wgsl`'s warning holds here too (no gamma
    // correction, or the palette is encoded twice).
    @location(2) rgba: vec4<f32>,
    @location(3) fx: vec4<f32>,
}

// The vertex and fragment sizes (`viewport_px`, `cell_px`, `uv_size`,
// `slot_px`, `slot_offset`) and the `heat` colour share one `var<immediate>`
// block. Order follows alignment: heat@0 (vec4), viewport_px@16, cell_px@24,
// uv_size@32, slot_px@40, slot_offset@48; 56 bytes round up to the struct's
// 16-byte alignment: size 64. The Rust twin is
// `crate::renderer::FxImmediates`, pinned by `offset_of`/`size_of` asserts and
// an explicit trailing `pad`.
//
// **Two boxes**: `cell_px` is the grid cell — the effects' amplitudes
// are its ratios and `local` is measured from its corner — and `slot_px` is
// the atlas's slot, which below `line_height` / `letter_spacing = 1` is larger
// than the cell. The glyph's ink lives in the slot, whose top-left corner is
// `slot_offset` up and left of the cell's: a cell-local point `g` is the slot
// point `g + slot_offset`. At `>= 1` the two boxes are equal and the offset is
// zero.
//
//   heat = the `heat` effect's glowing colour (the theme's `cursor`), one per
//          frame (`Frame::dock_fx_heat`).
struct Immediates {
    heat: vec4<f32>,
    viewport_px: vec2<f32>,
    cell_px: vec2<f32>,
    uv_size: vec2<f32>,
    slot_px: vec2<f32>,
    slot_offset: vec2<f32>,
}

var<immediate> imm: Immediates;

// Both planes are bound at once — the plane is read from the instance. When
// no colour texture exists yet, the renderer binds the mask to slot 1 too; no
// colour-plane instance exists then, so it is never read as colour.
@group(0) @binding(0) var mask_tex: texture_2d<f32>;
@group(0) @binding(1) var color_tex: texture_2d<f32>;
// `near` is the `cell` pipeline's nearest/clamp sampler, `lin` the linear one
// for the scaling branches (see `paint`).
@group(0) @binding(2) var near: sampler;
@group(0) @binding(3) var lin: sampler;

// Effect ids — the Rust side is `glyph_fx::Effect::id` (`Keypress` /
// `Erase`). Arrivals 1..16, ghosts 16..32: the kind of input is read from the
// id.
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

// The quad's inflation on every side, in slots (the quad is the slot's,
// grown by this much). The farthest points:
// horizontally `echo`'s copy (a wide glyph's two-cell box grown by ECHO_SCALE
// overflows each half's quad by 1.2 cells), vertically `shatter`'s falling
// shard (SHATTER_FALL + half a shard) and `undertow`. The margin is also where
// a wide glyph's other half lives: the left half's quad must cover the right
// half's cell too, so ink transformed around the box's centre can land there.
const FX_PAD: f32 = 1.5;

// **The amplitudes are design constants**, not measured, and all are in cell
// ratios — they grow with the font size. Picked by eye on offscreen frames.

// `rise`: where the glyph is born, this fraction of the cell height below.
// "A little below": a letter coming from under the baseline would read as
// arriving from the next row.
const RISE_DISTANCE: f32 = 0.4;
// `pop`: the starting scale and the spring's stiffness. Stiffness is the
// closed form's (`ease_out_back`) only constant; 2.2 gives a peak scale of
// ≈ 1.08 — "grows a little for a moment", not a bounce.
const POP_START: f32 = 0.3;
const POP_BACK: f32 = 2.2;
// `extrude`: the starting horizontal scale. Not zero, because the inverse
// transform is undefined at zero scale; a strip this thin is invisible on the
// first frame anyway.
const EXTRUDE_START: f32 = 0.05;
// `echo`: the scale the copy reaches and the opacity it starts at. The copy is
// "faint" — it must not compete with the glyph itself.
const ECHO_SCALE: f32 = 2.2;
const ECHO_ALPHA: f32 = 0.7;
// `drop`: the height the glyph falls from (fraction of the cell height) and
// the bounce's stiffness (`ease_out_back`; 1.7 overshoots the target by ~10%
// of the height — "bounces slightly").
const DROP_HEIGHT: f32 = 0.5;
const DROP_BACK: f32 = 1.7;
// `ink`: the softness of the ink's front, in "depth" units (0..1). A sharp
// threshold would staircase at the edge; a very soft one would be a plain
// `fade`.
const INK_SOFTNESS: f32 = 0.35;
// `squeeze`: the starting ratio (narrow horizontally, tall vertically) and the
// stretch's stiffness — the same closed form as `pop`, softer.
const SQUEEZE_X: f32 = 0.4;
const SQUEEZE_Y: f32 = 1.45;
const SQUEEZE_BACK: f32 = 1.5;
// `recede`: the scale it reaches. The glyph fades while shrinking to this
// ratio towards its centre; were it to reach zero, ink collapsing to a point
// in the last frames would read as "swallowed", not "withdrawn".
const RECEDE_SCALE: f32 = 0.3;

// The ghosts' amplitudes are cell ratios too, chosen large enough for the eye
// to catch within a quarter-second exit (user: "the animations are not
// noticeable at all"); the farthest point stays inside FX_PAD.

// `iris`: the open diaphragm's vertical diameter, as a fraction of the cell
// height — glyph ink does not reach the cell's top and bottom space.
const IRIS_REACH: f32 = 0.7;
// `undertow`: where the glyph is pulled — left (towards the caret; after
// Backspace the caret sits right at the ghost's left) and down, in cells.
const UNDERTOW_X: f32 = 0.8;
const UNDERTOW_Y: f32 = 0.7;
// The scale it reaches while pulled: the current carries the glyph away, and
// shrinking is the feel of that distance.
const UNDERTOW_SCALE: f32 = 0.75;
// `echo` (ghost): the scale the glyph reaches while dispersing. Smaller than
// the arrival's copy (ECHO_SCALE): here the glyph itself disperses, not a
// faint copy.
const ECHO_OUT_SCALE: f32 = 1.8;
// `bleed`: the radius the ink spreads to, as a fraction of the cell width. As
// the radius grows the stroke spreads and thins (a BLUR_TAPS-sample disc,
// `spread`).
const BLEED_RADIUS: f32 = 0.3;
// `unravel`: the number of strips in one cell height, one strip's sideways
// shift (fraction of the cell width) and the span the delays spread over — the
// last strip starts this late, so each strip's own run lasts
// `1 - UNRAVEL_STAGGER`.
const UNRAVEL_STRIPS: f32 = 5.0;
const UNRAVEL_SHIFT: f32 = 0.6;
const UNRAVEL_STAGGER: f32 = 0.45;
// `sublime`: the rise (fraction of the cell height) and the opening — more
// horizontal than vertical, vapour spreads sideways.
const SUBLIME_RISE: f32 = 0.5;
const SUBLIME_OPEN_X: f32 = 1.4;
const SUBLIME_OPEN_Y: f32 = 1.1;
// The vapour's dispersal: `bleed`'s spread, lighter (fraction of the cell
// width).
const SUBLIME_BLUR: f32 = 0.12;
// `shatter`: the shard grid (columns × rows per cell; a wide glyph's box has
// SHATTER_COLS_WIDE columns — odd, so the middle shard sits on the seam and
// the two halves break like one box), the spread (fraction of the cell width,
// linear in time), the fall (fraction of the cell height, quadratic in time —
// gravity), the maximum spin (radians) and the direction's jitter (radians).
const SHATTER_COLS: f32 = 2.0;
const SHATTER_COLS_WIDE: f32 = 3.0;
const SHATTER_ROWS: f32 = 2.0;
const SHATTER_SPREAD: f32 = 0.5;
const SHATTER_FALL: f32 = 1.2;
const SHATTER_SPIN: f32 = 0.8;
const SHATTER_JITTER: f32 = 0.7;
// `spread`'s sample count.
const BLUR_TAPS: i32 = 16;

struct FxOut {
    @builtin(position) position: vec4<f32>,
    // Pixels from the cell's top-left corner; interpolated. No `uv` travels:
    // the sample point is only known after the inverse transform.
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) uv0: vec2<f32>,
    @location(2) @interpolate(flat) rgba: vec4<f32>,
    @location(3) @interpolate(flat) fx: vec4<f32>,
}

@vertex
fn glyph_fx_vertex(@builtin(vertex_index) vid: u32, it: FxInstance) -> FxOut {
    let corner = vec2<f32>(f32(vid & 1u), f32(vid >> 1u));
    let pad = imm.slot_px * FX_PAD;
    let local = -imm.slot_offset - pad + corner * (imm.slot_px + 2.0 * pad);
    let ndc = (it.pos + local) / imm.viewport_px * 2.0 - 1.0;
    var o: FxOut;
    o.position = vec4<f32>(ndc.x, -ndc.y, 0.0, 1.0);
    o.local = local;
    o.uv0 = it.uv0;
    o.rgba = it.rgba;
    o.fx = it.fx;
    return o;
}

// The ease-out curve — the effects' single curve, **quadratic**. A cubic piled
// the change at the start (half of it done in a fifth of the duration) and the
// visible part was squeezed into a few frames: the user said "the animations
// are not noticeable at all". Quadratic leaves the half to a third of the
// duration.
fn ease_out(t: f32) -> f32 {
    let u = 1.0 - t;
    return 1.0 - u * u;
}

// A curve that overshoots the target for a moment and comes back — the
// "closed form" of `pop`, `drop` and `squeeze`. 0 at 0, 1 at 1;
// `back` is the overshoot's stiffness, the peak 1 + 4·back³ / (27·(back + 1)²).
fn ease_out_back(t: f32, back: f32) -> f32 {
    let u = t - 1.0;
    return 1.0 + (back + 1.0) * u * u * u + back * u * u;
}

// The glyph's paint at a point: on the mask plane the foreground colour and
// coverage, on the colour plane the texture itself. **The slot's outside is
// never sampled** and the rule lives in one place: every sample (the main
// glyph, `echo`'s copy, `spread`'s disc, `shatter`'s shards) goes through
// here. `g` is cell-local; the bound is the SLOT's (`g + slot_offset` inside
// `[0, slot_px)`). Half-open, like the static path's fragment centres: when
// the point is inside the slot it lands on a texel centre.
//
// **`smooth_` is for the scaling and rotating branches** (`pop`, `squeeze`,
// `recede`, `sublime`, `shatter`…): on a growing or shrinking glyph `nearest`
// skips different texels every frame and the edge flickered jaggedly.
// Linear sampling's four texels **stay inside the slot**: the point is clamped
// to texel centres, so the footprint never touches the neighbour slot —
// another glyph (`clamp_to_edge` clamps the texture, not the slot). Only the
// sliding branches (`rise`, `drop`, `undertow`, `unravel`) and the ends
// (`t = 0`, `t = 1`) use `nearest`: the letter stays sharp and bit for bit
// equal to the static path.
fn paint(g: vec2<f32>, uv0: vec2<f32>, fg: vec3<f32>, colored: bool, smooth_: bool) -> vec4<f32> {
    let slot_px = imm.slot_px;
    let s = g + imm.slot_offset;
    if (any(s < vec2<f32>(0.0)) || any(s >= slot_px)) {
        return vec4<f32>(0.0);
    }
    let texel = imm.uv_size / slot_px;
    if (!smooth_) {
        let uv = uv0 + s * texel;
        if (colored) {
            // Colour plane: the same as `emoji_fragment` — colour from the
            // texture.
            return textureSampleLevel(color_tex, near, uv, 0.0);
        }
        return vec4<f32>(fg, textureSampleLevel(mask_tex, near, uv, 0.0).r);
    }
    let q = clamp(s, vec2<f32>(0.5), slot_px - 0.5);
    if (colored) {
        // The hardware filter cannot be used on the colour plane: the bytes
        // are straight alpha (`raster::unpremultiply`) and a transparent
        // texel's colour would enter the blend and darken the edge. Four
        // texels by hand, blended premultiplied.
        let p = q - 0.5;
        let i0 = floor(p);
        let f = p - i0;
        let i1 = min(i0 + 1.0, slot_px - 1.0);
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

// Integer hash — `shatter`'s randomness. Not `fract(sin)`: the GPU's `sin`
// loses precision on large arguments and the pattern would drift per device;
// this hash is bit-exact everywhere (u32 arithmetic wraps in WGSL).
fn hash(v: u32) -> u32 {
    var x = v;
    x = x ^ (x >> 16u);
    x = x * 0x7feb352du;
    x = x ^ (x >> 15u);
    x = x * 0x846ca68bu;
    x = x ^ (x >> 16u);
    return x;
}

// The `k`th random of `seed`, in `[0, 1]`. The seed is fixed per input, so
// the shards do not flicker from frame to frame.
fn rand01(seed: u32, k: u32) -> f32 {
    return f32(hash(seed * 0x9e3779b9u + k) & 0xffffu) / 65535.0;
}

// The point's average paint over a disc of `radius` — `bleed`'s spreading ink,
// `sublime`'s vapour. The samples form a Vogel disc (golden angle), not a
// grid: a 3×3 grid at a large radius showed as nine separate copies of the
// glyph. Premultiplied average, straight-alpha result.
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

// Premultiplied "top wins" composite of two paints; straight-alpha result.
fn over(top: vec4<f32>, under: vec4<f32>) -> vec4<f32> {
    let a = top.a + under.a * (1.0 - top.a);
    let rgb = top.rgb * top.a + under.rgb * under.a * (1.0 - top.a);
    if (a > 0.0) {
        return vec4<f32>(rgb / a, a);
    }
    return vec4<f32>(0.0);
}

// `ink`'s "depth": the mean coverage of the point's 3×3 neighbourhood. The
// stroke's core (ink on every side) is near 1, its edge near 0 — a single
// texel's coverage is partial on most pixels of a thin font, so it could not
// tell the core from the edge. Neighbours are **clamped into the slot**: the
// slot's outside is not sampled here either.
fn ink_depth(g: vec2<f32>, uv0: vec2<f32>) -> f32 {
    let slot_px = imm.slot_px;
    let s = g + imm.slot_offset;
    var sum = 0.0;
    for (var dy: i32 = -1; dy <= 1; dy++) {
        for (var dx: i32 = -1; dx <= 1; dx++) {
            let q = clamp(s + vec2<f32>(f32(dx), f32(dy)), vec2<f32>(0.5), slot_px - 0.5);
            sum += textureSampleLevel(mask_tex, near, uv0 + q / slot_px * imm.uv_size, 0.0).r;
        }
    }
    return sum / 9.0;
}

// `shatter`'s paint: the darkest of the shards landing on the point. Each
// shard turns around its own centre and slides; the fragment walks the shards
// and maps the point back into each one's rest frame; if it falls inside that
// shard's rectangle it is a hit. **Clipped at the shard's bound**: outside the
// rectangle is not that shard's — a neighbour shard's ink must not leave with
// the one breaking away. Where shards overlap the darkest wins, not "first
// hit": a shard with an empty corner landing on a full one must not cover it.
//
// The shard grid is on the glyph's BOX (`box_left`, width `box_w`; the slot
// box, from `-slot_offset.y` and `slot_px.y` tall), not the half's cell: a
// wide glyph's two halves break like one box and each half paints only the
// hits that land in its own slot (`paint`).
//
// WGSL has no comma operator, so the shard counter `n` advances in the loop
// body.
fn shatter_paint(p: vec2<f32>, t: f32, e: f32, seed: u32, center: vec2<f32>,
                 box_left: f32, box_w: f32, wide: bool,
                 uv0: vec2<f32>, fg: vec3<f32>, colored: bool) -> vec4<f32> {
    let cell_px = imm.cell_px;
    let cols = select(SHATTER_COLS, SHATTER_COLS_WIDE, wide);
    let tile = vec2<f32>(box_w / cols, imm.slot_px.y / SHATTER_ROWS);
    var best = vec4<f32>(0.0);
    var n = 0u;
    for (var row = 0.0; row < SHATTER_ROWS; row += 1.0) {
        for (var col = 0.0; col < cols; col += 1.0) {
            let k = n;
            n += 1u;
            let lo = vec2<f32>(box_left, -imm.slot_offset.y) + vec2<f32>(col, row) * tile;
            let mid = lo + tile * 0.5;
            // Direction: outward from the box's centre, with a seeded jitter;
            // the middle shard (on the centre) goes down.
            let away = mid - center;
            let len = length(away);
            var dir = select(vec2<f32>(0.0, 1.0), away / len, len > 0.5);
            let turn = (rand01(seed, k * 4u) - 0.5) * SHATTER_JITTER;
            dir = vec2<f32>(dir.x * cos(turn) - dir.y * sin(turn),
                            dir.x * sin(turn) + dir.y * cos(turn));
            let speed = mix(0.6, 1.0, rand01(seed, k * 4u + 1u));
            // Ballistic: spread linear in time (initial velocity), fall
            // quadratic (gravity) — a shard flung upwards peaks and falls.
            let offset = dir * (SHATTER_SPREAD * cell_px.x * speed * t)
                       + vec2<f32>(0.0, SHATTER_FALL * cell_px.y * t * t);
            let angle = (rand01(seed, k * 4u + 2u) - 0.5) * 2.0 * SHATTER_SPIN * e;
            // Inverse transform: undo the slide first, then the rotation.
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

    // A ghost is fully transparent at `t = 1`: after the last effect frame the
    // background is plain.
    if (ghost && t >= 1.0) {
        return vec4<f32>(0.0);
    }

    // The transform's centre is the GLYPH's box, not the half's cell: a wide
    // glyph split across two slots must still transform as one box, or
    // `recede` would split an emoji down the middle. 0 = single cell, 1 = left
    // half (box extends right), 2 = right half (box extends left). `extrude`'s
    // anchor is also the box's left edge, not the half's.
    //
    // The centre stays on the CELL box (the glyph is centred on its cells);
    // the box `extrude`, `iris` and `shatter` read is the SLOT box: the
    // ink can spill past the cells and their bound must cover it — one slot
    // wide for a single glyph, `cell + slot` for a wide one, `slot_offset.x`
    // left of the cells. At `>= 1` the added terms are exactly zero.
    let cx = select(select(0.0, cell_px.x, half_ == 1u), cell_px.x * 0.5, half_ == 0u);
    let center = vec2<f32>(cx, cell_px.y * 0.5);
    let box_left = select(0.0, -cell_px.x, half_ == 2u) - imm.slot_offset.x;
    let box_w = select(2.0 * cell_px.x, cell_px.x, half_ == 0u) + (imm.slot_px.x - cell_px.x);

    var g = in.local;
    var alpha = 1.0;
    // The foreground colour on the mask plane; `heat` changes it.
    var fg = in.rgba.rgb;
    // The scaling and rotating branches sample linearly (see `paint`).
    var smooth_ = false;
    // `echo`'s copy: the second sample point and its opacity (0 = none).
    var echo_g = vec2<f32>(0.0);
    var echo_alpha = 0.0;
    // `ink`'s threshold; at 0 all the ink is visible.
    var ink_front = 0.0;
    // `bleed`'s radius, pixels; at 0 a single sample.
    var blur = 0.0;
    // `iris`'s diaphragm radius, pixels; negative means no diaphragm.
    var iris = -1.0;
    var shatter = false;
    // **The ends fall back to the static path's arithmetic**: when an arrival
    // settles (`t = 1`) and when a ghost is erased (`t = 0`) the inverse
    // transform does not run at all, so `g` is exactly the static path's
    // interpolation and no pixel jumps at the handover. `center + (g - center)
    // / 1` is not bit-equal to `g`; `nearest` hid that, linear sampling would
    // not. Every branch also approaches identity at the ends: the last effect
    // frame differs from the static glyph only by what is left of the curve.
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
            // It fades in too: the first frame's thin strip showed as a
            // broken line.
            alpha = e;
            smooth_ = true;
        } else if (id == FX_HEAT) {
            // Cooling uses `smoothstep`, not the shared ease-out curve: an ease-out
            // cubic cooled the colour mostly in the first quarter and the hot
            // colour lasted a single frame. `smoothstep` is slow at the start
            // — the colour stays visible — and slow at the end, so it reaches
            // its own colour without a kink.
            fg = mix(imm.heat.rgb, in.rgba.rgb, smoothstep(0.0, 1.0, t));
            // On the colour plane `fg` is not read (colour comes from the
            // texture, `emoji_fragment`'s rule): the emoji is not tinted, a
            // plain fade-in as in `ink`.
            if (colored) {
                alpha = e;
            }
        } else if (id == FX_ECHO) {
            alpha = e;
            let s = mix(1.0, ECHO_SCALE, e);
            echo_g = center + (in.local - center) / s;
            // The copy fades fast as it grows (the square of what is left): a
            // 0.7 copy fading along the curve read as a big bold letter in
            // the first frames, not as a ring.
            echo_alpha = ECHO_ALPHA * (1.0 - e) * (1.0 - e);
        } else if (id == FX_DROP) {
            g.y += DROP_HEIGHT * cell_px.y * (1.0 - ease_out_back(t, DROP_BACK));
            alpha = e;
        } else if (id == FX_INK) {
            // No threshold on the colour plane (an emoji's "depth" cannot be
            // read from coverage; its edge is often fully opaque): a plain
            // fade-in.
            if (colored) {
                alpha = e;
            } else {
                // The front is linear in time: with the ease-out curve the
                // spread finished in the first frame and the "filling" was
                // never seen.
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
            // The diaphragm closes to the centre from roughly the ink's bound
            // (the box's width, IRIS_REACH of its height); the glyph does not
            // fade, it is covered. Starting from the box's corner, the first
            // third of the duration would close over an empty cell.
            let reach = length(vec2<f32>(box_w, imm.slot_px.y * IRIS_REACH) * 0.5) + 1.0;
            iris = reach * (1.0 - e);
        } else if (id == FX_UNDERTOW) {
            // The pull starts slow and speeds up (`smoothstep`): the current
            // grips the glyph first, then carries it off. The fade starts
            // late — let the glyph travel while it is visible.
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
            // The strip comes from the point's OWN row: the shift is only
            // horizontal, so the row the point samples and the strip it
            // belongs to are the same and there is no seam between strips.
            // Strips start in turn from top to bottom and neighbours slide in
            // opposite directions — unravelling like thread.
            var strip = floor(in.local.y / (cell_px.y / UNRAVEL_STRIPS));
            strip = clamp(strip, 0.0, UNRAVEL_STRIPS - 1.0);
            let delay = strip / (UNRAVEL_STRIPS - 1.0) * UNRAVEL_STAGGER;
            let own = ease_out(saturate((t - delay) / (1.0 - UNRAVEL_STAGGER)));
            // WGSL's `%` on floats is the truncated remainder (C's `fmod`).
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
            // Let the shards travel while visible: the fade starts late.
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
        // A one-pixel soft edge: the diaphragm is sharp but not staircased.
        c.a *= saturate(iris - length(in.local - center) + 0.5);
    }
    c.a *= alpha;
    if (echo_alpha > 0.0) {
        // The copy is UNDER the glyph: a ring "dispersing off it" must not
        // cover it. Only the copy grows, so linear sampling is the copy's; the
        // glyph itself stays in place and `nearest`.
        var copy = paint(echo_g, in.uv0, fg, colored, true);
        copy.a *= echo_alpha;
        c = over(c, copy);
    }

    // **No caret inversion** (`Renderer::fx_draw`): the effect is drawn over
    // the caret in its own colour, because after Backspace the caret sits
    // right on the ghost. Mask plane: the sample's alpha times the instance's
    // (`cell_fragment`'s rule); colour plane: the colour is the texture's.
    if (colored) {
        return c;
    }
    return vec4<f32>(c.rgb, in.rgba.a * c.a);
}
