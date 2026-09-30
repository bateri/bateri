//! The renderer: wgpu, one device per process ([`Gpu`]) and one [`Renderer`]
//! per pane (the atlas's key includes the pane's point size).
//!
//! The device is pinned to wgpu's Metal backend on macOS ([`Gpu::new`]). Six
//! pipelines draw on three surfaces (grid, fill band, dock): `cell_bg` and
//! `caret` (background quads; the caret's SDF, also used by the dock's
//! buttons), `cell` and `emoji` (glyphs and rules from the atlas's two
//! planes), `selection` (the mouse selection and the search highlights) and
//! `glyph_fx` (the dock's typing effects). Why each pipeline exists is
//! written on [`Gpu`]'s fields; why the draws come in the
//! order they do, per surface, is written in [`Renderer::plan`].
//!
//! Slot resolution, the wide glyph's fan-out, uv baking and the effects'
//! instance packing live in [`crate::slots`]; this module only supplies the
//! upload target ([`WgpuUpload`]). A frame is recorded as a [`Plan`]: every
//! quad of the frame goes into **one** instance buffer (every glyph into one
//! glyph buffer, every effect into one effect buffer) and the draws read
//! ranges of them. Those buffers are not rebuilt per frame either: they live
//! as long as the renderer, grow on demand and are filled with
//! `write_buffer` — creating a buffer in wgpu is a validation and tracking
//! round trip, and it was measured (040 phase-2 → Uygulama Notları): a
//! per-frame buffer visibly inflated `cpu_encode`.
//!
//! **Completion** (040 Karar 6) is a submission index per frame and a
//! non-blocking [`Renderer::poll`] at the start of a tick — no closure per
//! frame. Its jobs (counting finished frames, reporting a failed one, the GPU
//! delta, the first finished frame) are carried by name there and in
//! `crate::link`.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::ops::Range;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use bt_atlas::{Atlas, Metrics, Plane, TOFU};
use bt_core::{Clusters, FontOptions, LinearRgba};

use crate::GpuError;
use crate::frame::{
    CursorBlock, FX_INSTANCE_OFFSETS, Frame, FxCell, FxInstance, GLYPH_INSTANCE_OFFSETS, GlyphCell,
    GlyphInstance, INSTANCE_OFFSETS, Instance, RuleCell,
};
use crate::metrics::{CellMetrics, FontNotice};
use crate::slots::{self, SlotUpload};

/// Immediate data budget in bytes: the smallest `maxPushConstantsSize` Vulkan
/// **guarantees** (discussion.md → Karar 5). macOS offers 4096, but a layout
/// that does not fit the smallest Linux driver would fail there; the device is
/// requested with exactly this limit, so an oversized pipeline is rejected on
/// macOS too.
pub(crate) const IMMEDIATE_BUDGET: u32 = 128;

/// Target format — **the single source**. `Srgb`: the fragment's output
/// counts as **linear** and the hardware encodes it on write, so alpha
/// blending runs in linear space (the glyphs' one reason for it). Its
/// counterpart is `bt_core::color::linear_rgba`; the two change together —
/// move one off linear without the other and the palette washes out to grey
/// (`CLAUDE.md` → colour space).
///
/// A `const`, not a field: a second target (offscreen, screenshot) given a
/// plain `Bgra8Unorm` would get **silently wrong colour**, not an error, so
/// "linear palette + non-sRGB target" must not be representable.
pub(crate) const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;

/// The atlas's mask plane: one channel of coverage, sampled by shaders only.
pub(crate) const MASK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;

/// The atlas's colour plane: the same slot grid, four bytes per pixel.
///
/// **It must be sRGB.** The target is sRGB and the hardware treats fragment
/// output as linear; an emoji sampled from a plain `Rgba8Unorm` texture would
/// take **undecoded** sRGB values for linear and wash the palette out — the
/// same silent defect as `CLAUDE.md` → "Renk uzayı sınırı geçer".
pub(crate) const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// Field-for-field twin of `cell_bg.wgsl` → `Immediates`.
///
/// In WGSL `vec4` aligns to 16, `vec2` to 8, and a struct's size rounds up to
/// its largest alignment: core@0, shape@16, viewport_px@32, size 48. The
/// trailing pad is WGSL's invisible 8 bytes — without it Rust would send 40
/// bytes and the layout would silently come up short. Putting the `vec4`s
/// first is deliberate: with `viewport_px` first the padding would land in
/// the middle. The `selection` pipeline reads the same block with another
/// meaning: `core` is the highlight's colour, `shape[0]` its radius.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Immediates {
    core: [f32; 4],
    shape: [f32; 4],
    viewport_px: [f32; 2],
    pad: [f32; 2],
}

const _: () = assert!(size_of::<Immediates>() == 48);
const _: () = assert!(std::mem::offset_of!(Immediates, shape) == 16);
const _: () = assert!(std::mem::offset_of!(Immediates, viewport_px) == 32);
// Chosen **per struct** and by size (Karar 5): 48 ≤ 128, so this block stays
// in immediates; no uniform-buffer fallback was needed.
const _: () = assert!(size_of::<Immediates>() as u32 <= IMMEDIATE_BUDGET);

/// Field-for-field twin of `cell.wgsl` → `Immediates`: the vertex stage's
/// `viewport_px`, `cell_px` and `uv_size` and the fragment's [`CursorBlock`]
/// in one block.
///
/// `CursorBlock` is embedded **as is** (rect@0, rgba@16, its own asserts in
/// `frame.rs`), so there is no second copy of the cursor's layout. Then the
/// `vec2`s: viewport_px@32, cell_px@40, uv_size@48; WGSL rounds 56 up to the
/// struct's 16-byte alignment, and the trailing 8 bytes are the explicit
/// `pad`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct GlyphImmediates {
    cursor: CursorBlock,
    viewport_px: [f32; 2],
    cell_px: [f32; 2],
    uv_size: [f32; 2],
    pad: [f32; 2],
}

const _: () = assert!(size_of::<GlyphImmediates>() == 64);
const _: () = assert!(std::mem::offset_of!(GlyphImmediates, viewport_px) == 32);
const _: () = assert!(std::mem::offset_of!(GlyphImmediates, cell_px) == 40);
const _: () = assert!(std::mem::offset_of!(GlyphImmediates, uv_size) == 48);
// 64 ≤ 128: this block stays in immediates too (Karar 5).
const _: () = assert!(size_of::<GlyphImmediates>() as u32 <= IMMEDIATE_BUDGET);

/// Field-for-field twin of `glyph_fx.wgsl` → `Immediates`: `heat` (the
/// `heat` effect's glowing colour, one per frame) and the three sizes.
/// heat@0, viewport_px@16, cell_px@24, uv_size@32; WGSL rounds 40 up to 48 and
/// the trailing 8 bytes are the explicit `pad`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct FxImmediates {
    heat: [f32; 4],
    viewport_px: [f32; 2],
    cell_px: [f32; 2],
    uv_size: [f32; 2],
    pad: [f32; 2],
}

const _: () = assert!(size_of::<FxImmediates>() == 48);
const _: () = assert!(std::mem::offset_of!(FxImmediates, viewport_px) == 16);
const _: () = assert!(std::mem::offset_of!(FxImmediates, cell_px) == 24);
const _: () = assert!(std::mem::offset_of!(FxImmediates, uv_size) == 32);
// 48 ≤ 128: immediates (Karar 5).
const _: () = assert!(size_of::<FxImmediates>() as u32 <= IMMEDIATE_BUDGET);

/// `Instance`'s vertex buffer layout: the three `@location`s of
/// `cell_bg.wgsl` → `Instance`; offsets come from `frame.rs`'s `offset_of!`
/// (`INSTANCE_OFFSETS`), not from hand-written numbers.
const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 3] = [
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x2,
        offset: INSTANCE_OFFSETS[0],
        shader_location: 0,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x2,
        offset: INSTANCE_OFFSETS[1],
        shader_location: 1,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: INSTANCE_OFFSETS[2],
        shader_location: 2,
    },
];

/// `GlyphInstance`'s vertex buffer layout: `cell.wgsl` → `GlyphInstance`,
/// offsets from `frame.rs` (`GLYPH_INSTANCE_OFFSETS`).
const GLYPH_ATTRIBUTES: [wgpu::VertexAttribute; 3] = [
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x2,
        offset: GLYPH_INSTANCE_OFFSETS[0],
        shader_location: 0,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x2,
        offset: GLYPH_INSTANCE_OFFSETS[1],
        shader_location: 1,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: GLYPH_INSTANCE_OFFSETS[2],
        shader_location: 2,
    },
];

/// `FxInstance`'s vertex buffer layout: `glyph_fx.wgsl` → `FxInstance`,
/// offsets from `frame.rs` (`FX_INSTANCE_OFFSETS`).
const FX_ATTRIBUTES: [wgpu::VertexAttribute; 4] = [
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x2,
        offset: FX_INSTANCE_OFFSETS[0],
        shader_location: 0,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x2,
        offset: FX_INSTANCE_OFFSETS[1],
        shader_location: 1,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: FX_INSTANCE_OFFSETS[2],
        shader_location: 2,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: FX_INSTANCE_OFFSETS[3],
        shader_location: 3,
    },
];

/// Drives a future to completion on this thread.
///
/// No `pollster` (Karar 10): on native backends wgpu's futures are ready on
/// the first poll. A future that never became ready would **spin here
/// forever** — the test would hang rather than silently return a wrong
/// result.
pub(crate) fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut cx) {
            return value;
        }
        std::thread::yield_now();
    }
}

/// Types whose bytes may go to the GPU as they are: `#[repr(C)]`, `f32`
/// fields only, no padding — every byte is initialised.
///
/// # Safety
///
/// An implementor must have no hidden padding; `bytes_of` would read and
/// upload it. No `bytemuck` (that would be a new dependency); the contract is
/// this closed list.
unsafe trait GpuBytes: Copy {}

// SAFETY: `repr(C)`, `f32` fields only; its size is the sum of its fields
// (the `size_of`/`offset_of` asserts in `frame.rs`).
unsafe impl GpuBytes for Instance {}
// SAFETY: `repr(C)`, `f32` fields only; its size is the sum of its fields
// (the `size_of`/`offset_of` asserts in `frame.rs`).
unsafe impl GpuBytes for GlyphInstance {}
// SAFETY: `repr(C)`, `f32` fields only; its size is the sum of its fields
// (the `size_of`/`offset_of` asserts in `frame.rs`: 48, no gaps).
unsafe impl GpuBytes for FxInstance {}
// SAFETY: `repr(C)`, `f32` fields only; WGSL's trailing padding is an
// explicit field (`pad`), size 48 is asserted.
unsafe impl GpuBytes for Immediates {}
// SAFETY: `repr(C)`: a `CursorBlock` (`repr(C)`, two `[f32; 4]`, size 32
// asserted in `frame.rs`) followed by `f32` pairs; WGSL's trailing padding is
// the explicit `pad`, size 64 is asserted.
unsafe impl GpuBytes for GlyphImmediates {}
// SAFETY: `repr(C)`, `f32` fields only; WGSL's trailing padding is the
// explicit `pad`, size 48 is asserted.
unsafe impl GpuBytes for FxImmediates {}

/// The bytes of a slice; the element type is bounded by [`GpuBytes`].
fn bytes_of<T: GpuBytes>(values: &[T]) -> &[u8] {
    // SAFETY: `T: GpuBytes` has no padding and every byte is initialised; the
    // length is the slice's own byte size, the lifetime is the input's.
    unsafe { std::slice::from_raw_parts(values.as_ptr().cast::<u8>(), size_of_val(values)) }
}

/// One step of the frame's draw list, in draw order ([`Renderer::plan`]).
#[derive(Clone, Debug, PartialEq)]
enum Op {
    /// Viewport at the origin's y. The size is always the texture's: the
    /// viewport's height and the `viewport_px` immediate are the two halves of
    /// the NDC scale, and if they differed the surface would be squashed. An
    /// origin past the texture's edge is fine: fragments outside the viewport
    /// are clipped.
    Viewport(f32),
    /// Scissor: (x, y, width, height), inside the texture.
    Scissor([u32; 4]),
    /// The `cell_bg` pipeline over a range of the instance buffer.
    Quads(Range<u32>),
    /// A `caret_fragment` draw: one quad with its own `core`/`shape`.
    Rounded {
        range: Range<u32>,
        core: [f32; 4],
        shape: [f32; 4],
    },
    /// The `selection` pipeline over a range of the instance buffer: the
    /// selection, or one search role ([`Plan::selection`], [`Plan::search`]).
    Selection {
        range: Range<u32>,
        color: [f32; 4],
        radius: f32,
    },
    /// The `cell` (mask) or `emoji` (colour) pipeline over a range of the
    /// glyph buffer, with that plane's texture bound. `cursor` is the text
    /// inversion rectangle for this list (degenerate for stripes and the fill
    /// band, [`Renderer::plan`] says why); `uv_size` is the atlas's.
    Glyphs {
        plane: Plane,
        range: Range<u32>,
        cursor: CursorBlock,
        uv_size: [f32; 2],
    },
    /// The `glyph_fx` pipeline over a range of the effect buffer: drawn in
    /// its own full-texture viewport (instances already carry `origin_y`),
    /// after which the dock's viewport at `origin_y` is restored
    /// ([`Renderer::fx_draw`] says why).
    Fx {
        range: Range<u32>,
        heat: [f32; 4],
        uv_size: [f32; 2],
        origin_y: f32,
    },
}

/// The frame's draw plan: three instance buffers and the steps reading ranges
/// of them. [`Renderer::plan`] builds it from a `Frame`,
/// [`Renderer::submit`] replays it into a pass.
#[derive(Default)]
struct Plan {
    instances: Vec<Instance>,
    glyphs: Vec<GlyphInstance>,
    fx: Vec<FxInstance>,
    ops: Vec<Op>,
    /// Scratch lists for [`slots::glyph_lists`] / [`slots::fx_list`]; each
    /// call's output is appended to `glyphs` / `fx` as ranges.
    mask: Vec<GlyphInstance>,
    color: Vec<GlyphInstance>,
    fx_scratch: Vec<FxInstance>,
}

impl Plan {
    fn quads(&mut self, instances: &[Instance]) {
        if instances.is_empty() {
            return;
        }
        let range = self.push(instances);
        self.ops.push(Op::Quads(range));
    }

    fn rounded(&mut self, instances: &[Instance], core: [f32; 4], shape: [f32; 4]) {
        if instances.is_empty() {
            return;
        }
        // `core`/`shape` are per draw, not per instance: a second caret quad
        // would compute its SDF against the first one's rectangle and come out
        // empty or oddly clipped — a silent defect, so the contract is checked.
        debug_assert!(instances.len() == 1, "one quad per rounded draw");
        let range = self.push(instances);
        self.ops.push(Op::Rounded { range, core, shape });
    }

    /// A selection draw: the sixth pipeline with its colour and radius as
    /// immediates. One per call: a window has one selection and one colour,
    /// and search makes one call per role ([`Plan::search`]); the corner
    /// decision is in each instance's mask.
    fn selection(&mut self, instances: &[Instance], color: [f32; 4], radius: f32) {
        if instances.is_empty() {
            return;
        }
        let range = self.push(instances);
        self.ops.push(Op::Selection {
            range,
            color,
            radius,
        });
    }

    /// Search highlights (033): two calls of the selection pipeline, one per
    /// role — the colour is an immediate, so two roles are two draws. Order
    /// `search_match` → `search_current`: the current match sits over the
    /// others. `fill` picks the fill band's lists; the caller set the viewport.
    /// With search off both lists are empty and nothing is drawn.
    fn search(&mut self, frame: &Frame, fill: bool) {
        let (matched, current) = if fill {
            (
                frame.fill_search_match_instances(),
                frame.fill_search_current_instances(),
            )
        } else {
            (
                frame.search_match_instances(),
                frame.search_current_instances(),
            )
        };
        let radius = frame.selection_radius();
        self.selection(matched, frame.search_match_rgba(), radius);
        self.selection(current, frame.search_current_rgba(), radius);
    }

    fn push(&mut self, instances: &[Instance]) -> Range<u32> {
        let start = self.instances.len() as u32;
        self.instances.extend_from_slice(instances);
        start..self.instances.len() as u32
    }

    /// Empties every list, keeping the capacity.
    fn clear(&mut self) {
        self.instances.clear();
        self.glyphs.clear();
        self.fx.clear();
        self.ops.clear();
    }

    /// Moves the scratch list of `plane` into `glyphs` and records its draw.
    fn glyph_draw(&mut self, plane: Plane, cursor: CursorBlock, uv_size: [f32; 2]) {
        let list = match plane {
            Plane::Mask => &self.mask,
            Plane::Color => &self.color,
        };
        if list.is_empty() {
            return;
        }
        let start = self.glyphs.len() as u32;
        self.glyphs.extend_from_slice(list);
        let range = start..self.glyphs.len() as u32;
        self.ops.push(Op::Glyphs {
            plane,
            range,
            cursor,
            uv_size,
        });
    }
}

/// The texture's strip from `top_px` down to the bottom, as a scissor
/// (x, y, width, height) — the growing dock band (032). The scissor must stay
/// inside the texture (the backend validates it), so the bounds are clamped to
/// the texture's height and leave at least one row; `0.0` is the whole
/// texture.
fn scissor_below(top_px: f32, viewport_px: [f32; 2]) -> [u32; 4] {
    let width = viewport_px[0].max(0.0) as u32;
    let height = viewport_px[1].max(0.0) as u32;
    let y = (top_px.max(0.0).round() as u32).min(height.saturating_sub(1));
    [0, y, width, height - y]
}

/// A render target: texture and view together, so the view is not rebuilt per
/// frame (offscreen) or is built once per frame (the window's texture, which
/// changes every frame: [`Target::new`]).
pub(crate) struct Target {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

impl Target {
    /// A target over `texture` — the window path's surface texture.
    pub(crate) fn new(texture: wgpu::Texture) -> Self {
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self { texture, view }
    }
}

/// One atlas plane on the GPU: its texture, its view and the bind group that
/// samples it with the `cell` layout. Created together, so a texture is never
/// drawn without its bind group.
struct PlaneTexture {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    bind: wgpu::BindGroup,
}

/// The atlas and its textures — **in one place**.
///
/// Were they two separate fields, [`Atlas::ensure`]'s `true` ("I rebuilt the
/// atlas, rebuild the texture too") could be lost by dropping one line, and
/// the symptom would be silent: reading a stale slot from an atlas whose grid
/// geometry changed does not trip `slot_origin`'s defence as long as the slot
/// stays in range, and draws **another glyph**. Here the signal is not a
/// matter of remembering but one line of [`Renderer::cell_metrics`]: the
/// textures drop, the next frame builds new ones.
struct WgpuAtlas {
    atlas: Atlas,
    /// `None` → not created yet, or `ensure` dropped it. Created by the first
    /// frame that draws a glyph, with the resident tofu written once
    /// ([`mask_texture`]) — **by drawing**, not by `cell_metrics`: the metric
    /// path is on window resizing's hot path and must not be tied to a texture
    /// allocation.
    mask: Option<PlaneTexture>,
    /// `None` → no emoji seen yet. **Lazy**, created by the first colour
    /// upload ([`WgpuUpload`]): it has the mask texture's edge but four bytes
    /// per pixel (4 MiB instead of 1 MiB at the default cell), and a session
    /// without emoji never pays for it.
    color: Option<PlaneTexture>,
    /// The effects' bind group (both planes and both samplers), keyed by
    /// whether the colour texture existed when it was made: while it does
    /// not, the mask is bound to the colour slot too — every binding of the
    /// layout must be filled, and no colour-plane instance exists then, so the
    /// stand-in is never sampled.
    /// A key mismatch rebuilds it ([`WgpuAtlas::fx_bind`]); `ensure` drops it
    /// with the textures.
    fx_bind: Option<(bool, wgpu::BindGroup)>,
}

impl WgpuAtlas {
    /// The effects' bind group for today's textures; rebuilt when the colour
    /// texture appeared since. `None` → no mask texture yet (no glyph drawn).
    fn fx_bind(&mut self, gpu: &Gpu) -> Option<&wgpu::BindGroup> {
        let mask = self.mask.as_ref()?;
        let has_color = self.color.is_some();
        if self
            .fx_bind
            .as_ref()
            .is_none_or(|(key, _)| *key != has_color)
        {
            let color = self.color.as_ref().map_or(&mask.view, |plane| &plane.view);
            self.fx_bind = Some((has_color, gpu.fx_bind_group(&mask.view, color)));
        }
        self.fx_bind.as_ref().map(|(_, bind)| bind)
    }
}

/// wgpu's [`SlotUpload`]: `Queue::write_texture` into the mask texture, or
/// into the colour texture, created here on the first colour slot (the
/// trait's doc says why at allocation time and not a frame earlier or later).
///
/// `write_texture` is staged by the queue and lands before the next `submit`'s
/// commands, i.e. before the frame that samples it: a new slot is never
/// written into a texture a frame in flight is still reading.
struct WgpuUpload<'a> {
    gpu: &'a Gpu,
    mask: &'a wgpu::Texture,
    color: &'a mut Option<PlaneTexture>,
    edge: (u16, u16),
}

impl SlotUpload for WgpuUpload<'_> {
    fn upload(&mut self, plane: Plane, origin: (u16, u16), metrics: Metrics, bytes: &[u8]) {
        let (gpu, edge) = (self.gpu, self.edge);
        let texture = match plane {
            Plane::Mask => self.mask,
            Plane::Color => {
                &self
                    .color
                    .get_or_insert_with(|| gpu.plane_texture(COLOR_FORMAT, edge))
                    .texture
            }
        };
        gpu.write_slot(texture, origin, metrics, bytes, plane);
    }
}

/// A renderer's own state: its three instance buffers and its atlas.
///
/// Per renderer, not per device: the queue is shared, and any `submit` on it
/// flushes every pending `write_buffer`/`write_texture` — which can only land
/// another renderer's write **earlier** than its own submit, never later, and
/// never into this renderer's buffers.
#[derive(Default)]
struct State {
    /// The frame's quads; grows, never shrinks.
    quads: Option<wgpu::Buffer>,
    /// The frame's glyph and rule instances; grows, never shrinks.
    glyphs: Option<wgpu::Buffer>,
    /// The frame's effect instances; grows, never shrinks.
    fx: Option<wgpu::Buffer>,
    /// `None` until [`Renderer::cell_metrics`] is asked: the scale half of the
    /// atlas's key comes **from the window** and the constructor does not see
    /// the window. Building it at a fixed 1.0 would break two things: on a
    /// Retina machine the font chain would run once more for nothing, and a
    /// path reading the atlas without ever asking the metric would draw
    /// **silently at @1x**. `None` makes that path loud: a frame with glyphs
    /// but no atlas fails with [`GpuError::NoAtlas`].
    atlas: Option<WgpuAtlas>,
    /// The frame's plan, kept between frames: `clear` keeps the capacity of
    /// its lists (and of the `slots` scratch lists), so the steady state does
    /// not allocate.
    plan: Plan,
}

/// A device fault reported **outside** a frame's error scope: wgpu's
/// uncaptured-error handler or the device-lost callback (both run on
/// whichever thread wgpu calls them from).
///
/// A generation, not a flag: each renderer records the generation when it
/// submits a frame and [`Renderer::poll`] fails every frame submitted
/// before a newer fault — without consuming it, so one renderer's poll does
/// not hide the fault from another sharing the device.
#[derive(Default)]
pub(crate) struct Fault {
    generation: AtomicU64,
    message: Mutex<String>,
}

impl Fault {
    /// Records a fault; the callbacks' single body.
    pub(crate) fn report(&self, message: String) {
        if let Ok(mut slot) = self.message.lock() {
            *slot = message;
        }
        self.generation.fetch_add(1, Ordering::AcqRel);
    }

    fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// The latest fault's text if one was reported after `seen`.
    fn since(&self, seen: u64) -> Option<String> {
        if self.generation() == seen {
            return None;
        }
        Some(
            self.message
                .lock()
                .map_or_else(|_| "device fault".to_owned(), |m| m.clone()),
        )
    }
}

/// What every renderer shares: the wgpu device, its queue, the six pipelines,
/// the bind group layouts and the samplers.
///
/// Split from [`Renderer`] because the atlas cannot be shared: `bt-atlas`
/// holds CoreText fonts, which are neither `Send` nor `Sync`, while a device
/// is worth creating once per process. The split is also the product's
/// shape: one device, a renderer per pane — the atlas's key includes the
/// pane's point size.
pub(crate) struct Gpu {
    /// Kept for the window surfaces ([`crate::Surface`]): a surface must come
    /// from the instance its device's adapter came from.
    instance: wgpu::Instance,
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// Cell backgrounds and the dock's ground; an instanced quad.
    ///
    /// Blending is on but has **no customer today**: backgrounds always have
    /// alpha `1.0` (`bt_core::LinearRgba`'s only constructor says so), so the
    /// result equals an opaque write. It stays on because the first consumer
    /// that wants alpha will come through this pipeline; turning it off would
    /// make that day a silent defect.
    cell_bg: wgpu::RenderPipeline,
    /// The caret: `cell_bg`'s vertex, its own fragment (`caret_fragment`).
    ///
    /// A separate pipeline because the shape language differs: the round
    /// corner, the edge and the halo want an SDF, and charging that to every
    /// background quad would cost thousands of fragments per frame. The
    /// vertex is shared, so there is no second corner path. Blending is
    /// **required** here — the halo is translucent by definition. Its second
    /// consumer is the upload row's buttons ([`Plan::rounded`]).
    caret: wgpu::RenderPipeline,
    /// The mouse selection and the search highlights (031, 033): its own
    /// vertex reading [`Instance`] verbatim (`selection_vertex`) and a
    /// corner-masked fragment.
    ///
    /// Separate from `cell_bg` for the caret's reason (a round corner wants an
    /// SDF); its vertex is separate because the fragment must know its own
    /// quad — the caret takes its one quad from an immediate, the selection
    /// has one per instance. Blending softens the round corner's edge.
    selection: wgpu::RenderPipeline,
    /// Glyphs and rules: the same quad, sampling the atlas's mask plane.
    /// Glyphs blend over the backgrounds: the atlas is a coverage mask and the
    /// colour comes from the instance.
    cell: wgpu::RenderPipeline,
    /// Colour emoji: **the same vertex** (`cell_vertex`), its own fragment,
    /// which takes the colour **from the texture**, not the instance, and
    /// never reads the cursor block. The blend is `cell`'s: the bytes are
    /// straight alpha (`raster::unpremultiply` undoes the premultiplication
    /// before upload).
    emoji: wgpu::RenderPipeline,
    /// The dock's typing effects (030): its own vertex and its own instance
    /// ([`FxInstance`]).
    ///
    /// It cannot share `cell`'s vertex: the quad grows by the effect's margin
    /// and the fragment maps the point back into glyph space with the effect's
    /// inverse transform, so the instance must carry the effect's parameters
    /// — widening `GlyphInstance` would grow every glyph list's stride for a
    /// handful of animated glyphs
    /// (`.tasks/030-dock-yazim-animasyonlari/discussion.md` → Karar 5). Emoji
    /// need no sibling: both textures are bound and the plane comes from the
    /// instance.
    glyph_fx: wgpu::RenderPipeline,
    /// The atlas planes' bind group layout: texture @0, sampler @1.
    plane_layout: wgpu::BindGroupLayout,
    /// The effects' layout: mask @0, colour @1, nearest @2, linear @3.
    fx_layout: wgpu::BindGroupLayout,
    /// Nearest, clamp-to-edge: the glyph pipelines' sampler (see where it is
    /// created for why not linear).
    sampler: wgpu::Sampler,
    /// Linear, clamp-to-edge: for the effects' scaling branches only
    /// (`glyph_fx.wgsl` clamps the point to texel centres, so it never reaches
    /// the neighbour slot).
    linear: wgpu::Sampler,
    /// `TIMESTAMP_QUERY` was granted: the GPU delta can be measured
    /// ([`Renderer::set_gpu_timing`]); otherwise its token is
    /// `unsupported` (Karar 6).
    timestamps: bool,
    fault: Arc<Fault>,
}

impl Gpu {
    /// The process-wide device: created once and shared by every renderer —
    /// every pane of every window (an adapter, a device and six pipelines are
    /// not worth paying per pane). A failure is kept too, so every pane reports the same error
    /// instead of retrying the adapter request.
    pub(crate) fn get() -> Result<&'static Self, GpuError> {
        static SHARED: OnceLock<Result<Gpu, String>> = OnceLock::new();
        SHARED
            .get_or_init(Self::new)
            .as_ref()
            .map_err(|e| GpuError::Wgpu(e.clone()))
    }

    /// [`Gpu::get`] for tests: a missing device is a failed test.
    #[cfg(test)]
    pub(crate) fn shared() -> &'static Self {
        Self::get().expect("wgpu device and pipelines")
    }

    pub(crate) fn instance(&self) -> &wgpu::Instance {
        &self.instance
    }

    pub(crate) fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// A device on the Metal backend and six pipelines.
    ///
    /// The backend is **pinned to Metal**: macOS is the only product target
    /// today; the Vulkan branch is opened and tested on Linux in the font set
    /// (`docs/YOL-HARITASI.md`).
    pub(crate) fn new() -> Result<Self, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .map_err(|e| format!("no adapter: {e}"))?;
        // The timestamp feature only if the adapter has it: its absence is a
        // measured `unsupported`, not a failed device.
        let timestamps = adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY);
        let mut required_features = wgpu::Features::IMMEDIATES;
        if timestamps {
            required_features |= wgpu::Features::TIMESTAMP_QUERY;
        }
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("bateri"),
            required_features,
            // The texture ceiling is the adapter's, not wgpu's portable
            // default (8192): the window surface is bound by it, and a backing
            // stretched across displays can exceed 8192 px where the hardware
            // allows 16384. The immediate budget stays exactly Vulkan's floor.
            required_limits: wgpu::Limits {
                max_immediate_size: IMMEDIATE_BUDGET,
                max_texture_dimension_2d: adapter.limits().max_texture_dimension_2d,
                ..wgpu::Limits::default()
            },
            ..wgpu::DeviceDescriptor::default()
        }))
        .map_err(|e| format!("device request failed: {e}"))?;
        // Faults outside a frame's error scope become the next poll's error
        // (Karar 6, the asynchronous leg), not a panic in wgpu's default
        // handler.
        let fault = Arc::new(Fault::default());
        {
            let fault = Arc::clone(&fault);
            device.on_uncaptured_error(Arc::new(move |error: wgpu::Error| {
                fault.report(error.to_string());
            }));
        }
        {
            let fault = Arc::clone(&fault);
            device.set_device_lost_callback(move |reason, message| {
                fault.report(format!("device lost ({reason:?}): {message}"));
            });
        }

        // Make a validation error a **value**, not a panic: the message must
        // say which pipeline failed.
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let quad_buffer = wgpu::VertexBufferLayout {
            array_stride: size_of::<Instance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &INSTANCE_ATTRIBUTES,
        };
        let glyph_buffer = wgpu::VertexBufferLayout {
            array_stride: size_of::<GlyphInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &GLYPH_ATTRIBUTES,
        };
        let fx_buffer = wgpu::VertexBufferLayout {
            array_stride: size_of::<FxInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &FX_ATTRIBUTES,
        };

        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cell_bg.wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/cell_bg.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cell_bg"),
            bind_group_layouts: &[],
            immediate_size: size_of::<Immediates>() as u32,
        });
        let quads = (&layout, &module, &quad_buffer);
        let cell_bg = pipeline(&device, quads, "cell_bg_vertex", "cell_bg_fragment");
        let caret = pipeline(&device, quads, "cell_bg_vertex", "caret_fragment");
        // Selection: its own vertex (the quad reaches the fragment), the same
        // module, layout and instance buffer.
        let selection = pipeline(&device, quads, "selection_vertex", "selection_fragment");

        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let sampler_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let plane_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("atlas plane"),
            entries: &[texture_entry(0), sampler_entry(1)],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cell.wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/cell.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cell"),
            bind_group_layouts: &[Some(&plane_layout)],
            immediate_size: size_of::<GlyphImmediates>() as u32,
        });
        let glyphs = (&layout, &module, &glyph_buffer);
        let cell = pipeline(&device, glyphs, "cell_vertex", "cell_fragment");
        // Emoji: `cell_vertex` shared verbatim, its own fragment; the blend is
        // the same (straight alpha, `raster::unpremultiply`).
        let emoji = pipeline(&device, glyphs, "cell_vertex", "emoji_fragment");

        let fx_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("typing effects"),
            entries: &[
                texture_entry(0),
                texture_entry(1),
                sampler_entry(2),
                sampler_entry(3),
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("glyph_fx.wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/glyph_fx.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("glyph_fx"),
            bind_group_layouts: &[Some(&fx_layout)],
            immediate_size: size_of::<FxImmediates>() as u32,
        });
        let glyph_fx = pipeline(
            &device,
            (&layout, &module, &fx_buffer),
            "glyph_fx_vertex",
            "glyph_fx_fragment",
        );
        // `nearest`, not `linear`: slots have no padding between them and
        // `linear`'s last column would blend the neighbour slot.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("atlas"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..wgpu::SamplerDescriptor::default()
        });
        let linear = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("atlas, linear"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..wgpu::SamplerDescriptor::default()
        });
        if let Some(error) = block_on(scope.pop()) {
            return Err(format!("pipeline creation failed: {error}"));
        }
        Ok(Self {
            instance,
            device,
            queue,
            cell_bg,
            caret,
            selection,
            cell,
            emoji,
            glyph_fx,
            plane_layout,
            fx_layout,
            sampler,
            linear,
            timestamps,
            fault,
        })
    }

    /// An atlas plane's texture (edge × edge, sampled and written by the
    /// queue), its view and its bind group.
    fn plane_texture(&self, format: wgpu::TextureFormat, edge: (u16, u16)) -> PlaneTexture {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(match format {
                COLOR_FORMAT => "atlas colour plane",
                _ => "atlas mask plane",
            }),
            size: wgpu::Extent3d {
                width: u32::from(edge.0),
                height: u32::from(edge.1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("atlas plane"),
            layout: &self.plane_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        PlaneTexture {
            texture,
            view,
            bind,
        }
    }

    /// The effects' bind group: both plane views and both samplers.
    fn fx_bind_group(
        &self,
        mask: &wgpu::TextureView,
        color: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("typing effects"),
            layout: &self.fx_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(mask),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(color),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.linear),
                },
            ],
        })
    }

    /// Writes one full slot.
    ///
    /// The length check is an `assert`, not a `debug_assert`: the width and
    /// height go to the copy from the metrics, the bytes from the slice, and if
    /// the two disagreed the GPU would read past a short buffer — silently.
    /// The expected length and row pitch come from [`slots::slot_layout`]
    /// (`bt-atlas` owns the slot geometry) and depend on the **plane**: mask
    /// `w*h`, colour `4*w*h` — one number for both would pass the wrong
    /// plane's buffer silently.
    fn write_slot(
        &self,
        texture: &wgpu::Texture,
        origin: (u16, u16),
        metrics: Metrics,
        bytes: &[u8],
        plane: Plane,
    ) {
        let (expected, row_bytes) = slots::slot_layout(metrics, plane);
        assert_eq!(bytes.len(), expected, "a full slot ({plane:?})");
        let (w, h) = metrics.cell_px;
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: u32::from(origin.0),
                    y: u32::from(origin.1),
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row_bytes as u32),
                rows_per_image: Some(u32::from(h)),
            },
            wgpu::Extent3d {
                width: u32::from(w),
                height: u32::from(h),
                depth_or_array_layers: 1,
            },
        );
    }

    /// Writes `bytes` into the buffer in `slot`, growing it (to a power of
    /// two, so a growing grid does not rebuild every frame) when too small.
    fn fill_buffer(&self, slot: &mut Option<wgpu::Buffer>, label: &'static str, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let len = bytes.len() as u64;
        if slot.as_ref().is_none_or(|buffer| buffer.size() < len) {
            *slot = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: len.next_power_of_two(),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        if let Some(buffer) = slot.as_ref() {
            self.queue.write_buffer(buffer, 0, bytes);
        }
    }

    /// An edge×edge offscreen target; renderable and copyable.
    #[cfg(test)]
    pub(crate) fn target(&self, edge: u32) -> Target {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen"),
            size: wgpu::Extent3d {
                width: edge,
                height: edge,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Target { texture, view }
    }

    /// Copies the texture into a buffer, waits and reads it. The queue is
    /// ordered: the copy is submitted after the draw, so it sees the drawn
    /// frame.
    ///
    /// `copy_texture_to_buffer` wants a row pitch that is a multiple of 256:
    /// edge 16 is 64 bytes per row, so the buffer is padded and rows are
    /// compacted while reading.
    #[cfg(test)]
    fn read_back(&self, target: &wgpu::Texture) -> Vec<u8> {
        let (width, height) = (target.width(), target.height());
        let row = width * 4;
        let padded =
            row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: u64::from(padded * height),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(height),
                },
            },
            target.size(),
        );
        self.queue.submit([encoder.finish()]);
        // The result is read from **this** call's channel: the device is
        // shared and another test's `poll` may run our callback; an error must
        // not blow up on that thread, and the buffer must not be read before
        // the mapping completes.
        let (sent, mapped_rx) = std::sync::mpsc::channel();
        buffer.map_async(wgpu::MapMode::Read, .., move |result| {
            let _ = sent.send(result);
        });
        let result = loop {
            self.device
                .poll(wgpu::PollType::wait_indefinitely())
                .expect("waiting for the GPU failed");
            if let Ok(result) = mapped_rx.try_recv() {
                break result;
            }
        };
        result.expect("mapping the readback buffer failed");
        let mapped = buffer.get_mapped_range(..).expect("mapped range");
        let pixels = mapped
            .chunks(padded as usize)
            .flat_map(|line| &line[..row as usize])
            .copied()
            .collect();
        drop(mapped);
        buffer.unmap();
        pixels
    }
}

/// A frame's GPU span, in seconds on the GPU's own clock — the arguments of
/// `Stats::record_gpu`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GpuSpan {
    pub(crate) start: f64,
    pub(crate) end: f64,
}

/// One frame's timestamp queries and their readback: the pass writes its
/// start and end, the frame's command buffer resolves and copies them, and
/// [`Renderer::poll`] maps the copy once the frame is done.
///
/// Pooled, not built per frame; the mapping needs a closure per timed frame
/// (wgpu reports a map only through its callback), which is why the whole
/// path exists only while measuring ([`Renderer::set_gpu_timing`]).
struct Timing {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
    /// `MAP_*` state of `readback`: written by the map callback.
    mapped: Arc<AtomicU8>,
    requested: bool,
}

const MAP_PENDING: u8 = 0;
const MAP_OK: u8 = 1;
const MAP_FAILED: u8 = 2;

/// The two timestamps' bytes.
const TIMESTAMP_BYTES: u64 = 2 * size_of::<u64>() as u64;

impl Timing {
    fn new(gpu: &Gpu) -> Self {
        let buffer = |label, usage| {
            gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: TIMESTAMP_BYTES,
                usage,
                mapped_at_creation: false,
            })
        };
        Self {
            queries: gpu.device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("frame timestamps"),
                ty: wgpu::QueryType::Timestamp,
                count: 2,
            }),
            resolve: buffer(
                "timestamp resolve",
                wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            ),
            readback: buffer(
                "timestamp readback",
                wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            ),
            mapped: Arc::new(AtomicU8::new(MAP_PENDING)),
            requested: false,
        }
    }

    /// Reads the span once the frame is done. `None` → the mapping has not
    /// completed yet (poll again next tick); `Some(None)` → no usable span
    /// (the mapping failed or the GPU gave no start), the frame still counts.
    fn read(&mut self, gpu: &Gpu) -> Option<Option<GpuSpan>> {
        if !self.requested {
            self.requested = true;
            let mapped = Arc::clone(&self.mapped);
            self.readback
                .map_async(wgpu::MapMode::Read, .., move |result| {
                    let state = if result.is_ok() { MAP_OK } else { MAP_FAILED };
                    mapped.store(state, Ordering::Release);
                });
            // The frame is done, so the copy is too: one non-blocking poll
            // resolves the mapping.
            let _ = gpu.device.poll(wgpu::PollType::Poll);
        }
        match self.mapped.load(Ordering::Acquire) {
            MAP_PENDING => None,
            MAP_OK => {
                let span = self.readback.get_mapped_range(..).ok().map(|bytes| {
                    let tick = |i: usize| {
                        let mut raw = [0u8; 8];
                        raw.copy_from_slice(&bytes[i * 8..i * 8 + 8]);
                        u64::from_le_bytes(raw)
                    };
                    let period = f64::from(gpu.queue.get_timestamp_period());
                    let seconds = |ticks: u64| ticks as f64 * period / 1e9;
                    GpuSpan {
                        start: seconds(tick(0)),
                        end: seconds(tick(1)),
                    }
                });
                self.readback.unmap();
                self.reset();
                Some(span)
            }
            _ => {
                self.reset();
                Some(None)
            }
        }
    }

    fn reset(&mut self) {
        self.requested = false;
        self.mapped.store(MAP_PENDING, Ordering::Release);
    }
}

/// A submitted frame waiting for [`Renderer::poll`].
struct Pending {
    index: wgpu::SubmissionIndex,
    /// [`Fault`]'s generation at submit: a newer one fails this frame.
    fault: u64,
    timing: Option<Timing>,
}

/// A renderer: the shared [`Gpu`] plus this renderer's atlas, instance
/// buffers and in-flight frames — one per pane.
///
/// `RefCell`/`Cell`: [`Renderer::cell_metrics`] and [`Renderer::draw`] take
/// `&self` ([`Atlas::ensure`] and [`Atlas::slot`] take `&mut`) and `bt-shell`
/// holds the renderer in an `Rc` — shared ownership has no `&mut` path. Every
/// borrow is taken and released inside one call; none crosses a call
/// boundary. The renderer never leaves its thread: its frames are counted by
/// [`Renderer::poll`] on that same thread.
pub struct Renderer {
    gpu: &'static Gpu,
    state: RefCell<State>,
    /// The requested font: the family and size half of the atlas's key.
    ///
    /// **Stored**, not passed to [`Renderer::cell_metrics`]: as a parameter the
    /// setting would enter the call path on every resize and window geometry
    /// would have to know about fonts. It starts at `FontOptions::default()`,
    /// so a timed run (which never calls [`Renderer::set_font`]) and a user
    /// without a settings file see the same font, and the default size has no
    /// second owner.
    font: RefCell<FontOptions>,
    /// Frames the GPU finished **without error**; `make duman`'s `kare=`.
    frames: Cell<u64>,
    /// The last **submitted** frame's background, glyph and rule counts
    /// (`hucre=`, `glif=`, `kural=`); CPU counters.
    last_counts: Cell<[usize; 3]>,
    /// Submitted frames, oldest first.
    in_flight: RefCell<VecDeque<Pending>>,
    /// The GPU delta is measured (the measurement gate is open).
    timing: Cell<bool>,
    /// Idle [`Timing`]s, reused.
    spare: RefCell<Vec<Timing>>,
    /// Test hook: the next frame carries an invalid scissor, so its submit
    /// fails validation (the synchronous error leg).
    #[cfg(test)]
    poison: Cell<bool>,
}

impl Renderer {
    /// A renderer on the process's shared device ([`Gpu::get`]), with no
    /// atlas yet ([`Renderer::cell_metrics`] opens it). `bt-shell` calls only
    /// this and never sees wgpu.
    pub fn system_default() -> Result<Self, GpuError> {
        Ok(Self::on(Gpu::get()?))
    }

    /// [`Renderer::system_default`] for tests.
    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self::on(Gpu::shared())
    }

    /// A renderer on `gpu` — also a test with a device of its own (a fault
    /// must not leak into other tests' frames).
    pub(crate) fn on(gpu: &'static Gpu) -> Self {
        Self {
            gpu,
            state: RefCell::new(State::default()),
            font: RefCell::new(FontOptions::default()),
            frames: Cell::new(0),
            last_counts: Cell::new([0; 3]),
            in_flight: RefCell::new(VecDeque::new()),
            timing: Cell::new(false),
            spare: RefCell::new(Vec::new()),
            #[cfg(test)]
            poison: Cell::new(false),
        }
    }

    #[cfg(test)]
    pub(crate) fn device(&self) -> &wgpu::Device {
        &self.gpu.device
    }

    /// The shared device; the window surface is created and configured on it.
    pub(crate) fn gpu(&self) -> &'static Gpu {
        self.gpu
    }

    #[cfg(test)]
    pub(crate) fn target(&self, edge: u32) -> Target {
        self.gpu.target(edge)
    }

    /// Brings the atlas to the requested font at `scale` and returns the grid
    /// geometry: cell size and gutter ([`CellMetrics::from_atlas`]).
    ///
    /// `scale` is a parameter because the screen's scale can change at run
    /// time (`windowDidChangeBackingProperties:`, an external display) and the
    /// atlas carries it as part of its cache key: the same renderer gives two
    /// metrics at two scales. A glyph rasterised at @1x blurs at @2x without an
    /// error, and the symptom shows only on a two-display machine. The gutter
    /// comes from the **same call** so the two cannot drift for a frame.
    ///
    /// **This is the only place that changes the atlas's key** (family, size and
    /// scale), i.e. the grid geometry; drawing opens slots but never changes
    /// the grid. When `ensure` reports a rebuild, every texture drops in the
    /// same line: the slot map and the texture edge may have changed (the edge
    /// derives from `SLOT_TARGET` and the cell size), and writing a texture of
    /// the old size with the new metrics would corrupt silently — the colour
    /// texture too, which Cmd+ or moving the window to another display
    /// triggers with an emoji on screen. The metrics and the context width are
    /// read inside the same borrow, so no `ensure` can slip between them.
    pub fn cell_metrics(&self, scale: f64) -> CellMetrics {
        let font = self.font.borrow();
        let family = font.family.as_deref();
        let mut state = self.state.borrow_mut();
        let entry = state.atlas.get_or_insert_with(|| WgpuAtlas {
            atlas: Atlas::new(family, font.size, scale, font.line_height),
            mask: None,
            color: None,
            fx_bind: None,
        });
        if entry
            .atlas
            .ensure(family, font.size, scale, font.line_height)
        {
            entry.mask = None;
            entry.color = None;
            entry.fx_bind = None;
        }
        CellMetrics::from_atlas(entry.atlas.metrics(), entry.atlas.context_cell_w(), scale)
    }

    /// Changes the requested font; `true` when it differs. The atlas opens
    /// with it on the next [`Renderer::cell_metrics`].
    pub fn set_font(&self, font: &FontOptions) -> bool {
        let mut current = self.font.borrow_mut();
        if *current == *font {
            return false;
        }
        current.clone_from(font);
        true
    }

    /// What to tell the user about the open atlas's font; `None` without an
    /// atlas or when the font opened as asked.
    pub fn font_notice(&self) -> Option<FontNotice> {
        let state = self.state.borrow();
        let issue = state.atlas.as_ref()?.atlas.font_issue()?;
        Some(FontNotice::from(issue.clone()))
    }

    /// The mask plane's slot occupancy (used, total); `(0, 0)` without an
    /// atlas. `yuva=`'s source.
    pub fn atlas_occupancy(&self) -> (usize, usize) {
        self.state
            .borrow()
            .atlas
            .as_ref()
            .map_or((0, 0), |a| a.atlas.occupancy())
    }

    /// The colour plane's slot occupancy; `yuva2=`'s source.
    pub fn color_atlas_occupancy(&self) -> (usize, usize) {
        self.state
            .borrow()
            .atlas
            .as_ref()
            .map_or((0, 0), |a| a.atlas.color_occupancy())
    }

    /// Frames the GPU finished without error, as counted by
    /// [`Renderer::poll`].
    pub fn frames(&self) -> u64 {
        self.frames.get()
    }

    pub fn last_bg_count(&self) -> usize {
        self.last_counts.get()[0]
    }

    pub fn last_glyph_count(&self) -> usize {
        self.last_counts.get()[1]
    }

    pub fn last_rule_count(&self) -> usize {
        self.last_counts.get()[2]
    }

    /// Whether this renderer's atlas has its (mask, colour) textures — the
    /// "rebuilding drops both" guard's window.
    #[cfg(test)]
    pub(crate) fn plane_textures(&self) -> (bool, bool) {
        self.state
            .borrow()
            .atlas
            .as_ref()
            .map_or((false, false), |a| (a.mask.is_some(), a.color.is_some()))
    }

    /// Makes the next frame fail validation (test hook).
    #[cfg(test)]
    pub(crate) fn poison_next_frame(&self) {
        self.poison.set(true);
    }

    /// Opens or closes the GPU-delta measurement. Without `TIMESTAMP_QUERY`
    /// it stays closed and [`Renderer::gpu_timing_supported`] says so.
    pub(crate) fn set_gpu_timing(&self, on: bool) {
        self.timing.set(on && self.gpu.timestamps);
    }

    /// `false` → the GPU delta's token value is `unsupported` (Karar 6).
    pub fn gpu_timing_supported(&self) -> bool {
        self.gpu.timestamps
    }

    /// Whether a submitted frame is still waiting for [`Renderer::poll`]:
    /// a link going to sleep with one arms a single delayed poll (Karar 6,
    /// "the last frame before sleep is not lost"); an empty queue arms
    /// nothing — the stop condition.
    pub(crate) fn in_flight(&self) -> bool {
        !self.in_flight.borrow().is_empty()
    }

    /// Waits at most `timeout` for the newest submitted frame — the pending
    /// poll at shutdown, which must come before the report reads `kare=`
    /// (Karar 6). It only waits; counting is still [`Renderer::poll`]'s.
    pub(crate) fn wait_in_flight(&self, timeout: Duration) {
        let newest = self.in_flight.borrow().back().map(|p| p.index.clone());
        if let Some(index) = newest {
            // A timeout or a fault here is not this call's to report: the
            // next `poll` sees the frame as unfinished or failed.
            let _ = self.gpu.device.poll(wgpu::PollType::Wait {
                submission_index: Some(index),
                timeout: Some(timeout),
            });
        }
    }

    /// The glyph and rule draws of one list: slot resolution, uploading the
    /// missing slots, then the emoji draw and the mask draw. Glyphs and rules
    /// share one mask list, rules last ([`slots::glyph_lists`]): both are
    /// cell-sized coverage masks drawn by the same pipeline.
    ///
    /// **Emoji before the mask list, after the caret.** Emoji must cover the
    /// background (so after the grounds) and rule lines must stay on top of
    /// emoji too (so before the mask list — a struck-through emoji must look
    /// struck through). After the caret is a choice: emoji are opaque, so the
    /// caret under the ink is covered and shows as a ring around it; drawn
    /// before the caret, a block caret would hide the emoji **entirely**. The
    /// same order is won on every surface at once, because this runs for each
    /// list (stripes, grid, fill band, dock).
    ///
    /// The atlas borrow **is born and dies inside this call**. Hoisting slot
    /// resolution into `Session::frame`'s sink is the natural reflex (the sink
    /// already runs per cell), but `link.rs` holds the `Frame`'s borrow for the
    /// whole draw: the same shape copied for the atlas would be a
    /// `BorrowMutError` on the first frame with glyphs — a panic on the draw
    /// path, where `Retry` was designed for `GpuError`, not for unwinding.
    ///
    /// "Ask the metric first": the atlas's key comes from the window and
    /// [`Renderer::cell_metrics`] is the only place that builds it. Arriving
    /// here with `None` means "drawing glyphs without ever saying the scale";
    /// rather than invent a @1x atlas the frame fails ([`GpuError::NoAtlas`]).
    ///
    /// The quad's size is the **frame's** cell (`Frame::clear`), the uv size
    /// the **atlas's**. They are born from the same scale; the only window
    /// where they differ is the one frame between a scale change and the
    /// geometry event, and there the glyph stretches, it does not break. The
    /// quad's size cannot be taken from the atlas: the position
    /// (`GlyphCell::pos`) and the background under it come from the frame's
    /// cell, and tying only the size to the atlas would shift the glyph out of
    /// its cell.
    fn glyph_draws(
        &self,
        plan: &mut Plan,
        atlas: &mut Option<WgpuAtlas>,
        glyphs: &[GlyphCell],
        clusters: &Clusters,
        rules: &[RuleCell],
        cursor: CursorBlock,
    ) -> Result<(), GpuError> {
        if glyphs.is_empty() && rules.is_empty() {
            return Ok(());
        }
        let entry = atlas.as_mut().ok_or(GpuError::NoAtlas)?;
        let WgpuAtlas {
            atlas, mask, color, ..
        } = entry;
        let mask = mask_texture(self.gpu, atlas, mask);
        let edge = atlas.texture_px();
        let mut upload = WgpuUpload {
            gpu: self.gpu,
            mask,
            color: &mut *color,
            edge,
        };
        slots::glyph_lists(
            atlas,
            &mut upload,
            glyphs,
            clusters,
            rules,
            &mut plan.mask,
            &mut plan.color,
        );
        let uv_size = uv_size(atlas);
        // The colour list can only be non-empty once its texture exists (the
        // upload created it); the check keeps a missing texture a skipped
        // draw rather than the mask texture read as colour.
        if color.is_some() {
            plan.glyph_draw(Plane::Color, cursor, uv_size);
        }
        plan.glyph_draw(Plane::Mask, cursor, uv_size);
        Ok(())
    }

    /// The dock's typing effects (030): the `glyph_fx` pipeline, both textures
    /// and `heat`'s glowing colour ([`Frame::dock_fx_heat`]). Slot resolution
    /// and fan-out are [`slots::fx_list`], inside the same atlas borrow as
    /// [`Renderer::glyph_draws`]. A colour-plane instance whose texture does
    /// not exist is **not drawn**: reading the mask texture as colour would be
    /// random pixels.
    ///
    /// **No [`CursorBlock`]**: an effect is drawn over the caret in its own
    /// colour and takes no part in the block's inversion. Measured (the user
    /// said "the animations do not show at all"): an arrival's first moment is
    /// inside the not-yet-moved caret, and on Backspace the caret lands on the
    /// ghost's column, so the ghost played entirely inside the block — an
    /// inverted effect read as part of the caret. The cost: had an effect
    /// ended while the caret sat on an arrival, the letter's colour would jump
    /// to its inverted form on the hand-over frame; in insert mode the caret
    /// leaves the typed letter long before the effect ends.
    ///
    /// **In its own viewport, not the dock's**: the dock's viewport starts at
    /// the band's top and clips above it, while `drop` falls from above the
    /// cell, `sublime` floats up and there is only a thin breathing margin
    /// above the input row — the first frames showed half-cut letters. The
    /// effect is drawn in window space (positions moved down by `origin_y`)
    /// and may spill over the hairline by its pad (`glyph_fx.wgsl` →
    /// `FX_PAD`); the dock's viewport is restored after the draw.
    #[allow(clippy::too_many_arguments)] // the cluster table is half of `cells`
    fn fx_draw(
        &self,
        plan: &mut Plan,
        atlas: &mut Option<WgpuAtlas>,
        cells: &[FxCell],
        clusters: &Clusters,
        heat: [f32; 4],
        origin_y: f32,
    ) -> Result<(), GpuError> {
        if cells.is_empty() {
            return Ok(());
        }
        let entry = atlas.as_mut().ok_or(GpuError::NoAtlas)?;
        let WgpuAtlas {
            atlas, mask, color, ..
        } = entry;
        let mask = mask_texture(self.gpu, atlas, mask);
        let edge = atlas.texture_px();
        let mut upload = WgpuUpload {
            gpu: self.gpu,
            mask,
            color: &mut *color,
            edge,
        };
        slots::fx_list(atlas, &mut upload, cells, clusters, &mut plan.fx_scratch);
        if color.is_none() {
            plan.fx_scratch
                .retain(|instance| !slots::fx_is_color(instance));
        }
        if plan.fx_scratch.is_empty() {
            return Ok(());
        }
        let start = plan.fx.len() as u32;
        plan.fx
            .extend(plan.fx_scratch.iter().map(|&instance| FxInstance {
                pos: [instance.pos[0], instance.pos[1] + origin_y],
                ..instance
            }));
        let range = start..plan.fx.len() as u32;
        plan.ops.push(Op::Fx {
            range,
            heat,
            uv_size: uv_size(atlas),
            origin_y,
        });
        Ok(())
    }

    /// The draw plan for a `Frame`: one render pass, three coordinate spaces
    /// (grid, fill band, dock), each set by its own viewport.
    ///
    /// The same body serves the window's drawable and an offscreen texture:
    /// the tests read what the GPU really painted from here, so the proven
    /// part of drawing does not depend on a window.
    ///
    /// **Grid.** The vertical offset lives in the viewport, one op for every
    /// list: `Frame` does not bake it into the lists (`Frame::origin_px`'s
    /// reason: the sink bakes a cell at push time, the occupancy count is
    /// known only after the loop), and the viewport transform applies it from
    /// NDC to window coordinates, so grounds, glyphs, rules and stripes all
    /// move by the same amount without touching the shaders or the
    /// `#[repr(C)]` layouts. The order is the draw order (003 → R4.1): command
    /// marks, then grounds, search, selection and the caret, then glyphs, rules
    /// last. The reverse would let the caret cover the letter under it; a rule
    /// over the caret stays visible for free from the same order.
    ///
    /// - Command marks first, as sprites (the dock's chevron shape and colour
    ///   dictionary). Being first is a layer decision, not an overlap one: the
    ///   gutter is left of the grid and no cell lands there
    ///   (`Frame::push_block`). Their inversion rectangle is **degenerate**:
    ///   the caret never goes to the gutter, and passing the real one would
    ///   widen the claim to "a caret over the gutter turns the mark's colour".
    /// - **Search after the ground, before the selection** (033 Karar 7):
    ///   every match, the current match over them, the user's selection on top
    ///   — when Esc turns the current match into the selection it stays
    ///   visible. Text is over all of them in its own colour.
    /// - **Selection after the ground, before the caret and glyphs** (031):
    ///   text reads over the selection in its own colour and the caret stays
    ///   on top — reverse video's "the cursor wins" rule, as pixel order.
    /// - **Caret after the grounds, before the glyphs**, for the **solid**
    ///   caret: the block is opaque and the letter under it is drawn over it,
    ///   in the colour `cursor_block` inverts. For a hollow caret both halves
    ///   of the reason fall away (no fill, degenerate `CursorBlock`) and the
    ///   cost is recorded: a glyph with ink at the cell's edge is drawn over
    ///   the ring (`.tasks/015-imlec-cilasi/phase-3.md`). This slot is filled
    ///   while the caret is in the grid; once it enters the dock band the list
    ///   is empty and the instance is in the dock's list (`Frame::push_caret`).
    ///
    /// **Fill band** — the third coordinate space, above the grid, with its
    /// own viewport: the twin of the dock's reason in the other direction. The
    /// band is not exempt from the offset but sits **on top of** it (origin
    /// `origin_px − fill_px`, [`Frame::fill_origin_px`]). Its rows are
    /// fill-local and which screen row they land on is known only here, **at
    /// encode time** — baked at push time, a motion frame (lists kept, only
    /// `origin_px` changes) would freeze the band in place (017 R3.1). The
    /// origin **may go negative** and is left so: the band's oldest rows that
    /// do not fit spill over the top and are clipped (measured, 017 phase-0).
    /// A band of zero rows sets no viewport and the frame is bit-identical to
    /// the one without it; a window without a dock never gets rows
    /// (`Session::fill_rows` returns zero). Search after the band's ground,
    /// before its letters (033 Karar 8); no selection in the band. Its
    /// inversion rectangle is **degenerate**: the band has no caret slot and
    /// in a settled frame the caret's screen row is always inside the
    /// content; passing the real one would paint the letter under a caret
    /// sliding over the band in the ground colour — an unreadable cell for a
    /// caret that is not drawn. **After the grid, before the dock**: the
    /// grid's lists never enter the band (all `y ≥ origin_px`), the only thing
    /// that does is the offset-exempt caret, so the band is drawn after it;
    /// and the dock's opaque ground must be drawn last.
    ///
    /// **Dock** — the second coordinate space, **last**. Its own viewport is
    /// structural: the dock must be exempt from the offset and building the
    /// exemption arithmetically (`- origin_px`) **does not work** —
    /// `Frame::clear` resets the offset and `set_origin_rows` runs after the
    /// sink, so the value is unknown when dock cells are pushed. The origin
    /// sits at the texture's **bottom** (`height − dock`), and the leftover
    /// strip under the grid (pixels not divisible by the cell) stays between
    /// dock and content. Last because during a slide the grid's offset target
    /// is overshot and its bottom row spills over the dock; the dock's opaque
    /// ground covers it. The origin is **clamped at zero**: in a window
    /// shorter than the dock the right answer is degenerate (the dock covers
    /// the whole window); left negative, the dock would climb into the grid's
    /// area. A window without a dock sets no second viewport.
    ///
    /// **Two origins** (032): the ground and hairlines use the **drawn band's**
    /// viewport (`height − band`, the animation's current value); cells, caret
    /// and effects the **layout's** (`height − layout`). Cells are baked at
    /// push time and a motion frame does not re-push them; the layout is
    /// bottom-aligned, so while the band grows and shrinks the text stays put
    /// and only the band's top moves. At rest the two are the same number.
    /// Ground and separator first: the dock's own backgrounds (highlight
    /// ranges, caret) must come over them. **A growing band clips**: if the
    /// layout spills over the band's current top (the band has not risen yet),
    /// the spilling input rows would be drawn groundless over the grid's
    /// bottom rows. The scissor exists only in those frames: at rest it would
    /// cut the effects' margin that spills over the hairline.
    ///
    /// In the dock: the selection in the grid's order (031 R3.2); the upload
    /// row's buttons (037 phase-6) over the ground and selection, under their
    /// labels. The caret's dock slot is after the opaque ground (or the ground
    /// would cover it) and before the glyphs (or it would paint over the
    /// letter); the instance is born in window space, the viewport is
    /// dock-local, so the difference is given back here. On hand-over frames
    /// the caret moves into the dock band and, this list being last, stays on
    /// top of everything. **The caret is outside the scissor**: at a hand-over
    /// or on a new input row a caret touching the band's top would be a
    /// half-cut block; a whole block showing for one frame above the band is
    /// better than a cut one. **Ghosts before the dock's glyphs** (030): the
    /// letter sliding into a deleted letter's place must sit over the ghost —
    /// the text flows at once, the ghost fades under it. **Arrivals after the
    /// glyphs, before the rules**, so the call splits while one is in flight:
    /// an underlined letter arriving must have its line **over** it, or the
    /// line would jump from under to over the letter on the frame the effect
    /// returns to the static glyph (pixel equality at `t = 1`). With no arrival
    /// in flight it is one call. The caret's rectangle is compared with the
    /// fragment's position after the viewport transform, i.e. window space —
    /// the **same** rectangle as the grid's: one caret, one inversion. The
    /// scissor is reset at the end so no later list starts clipped.
    fn plan(
        &self,
        frame: &Frame,
        viewport_px: [f32; 2],
        atlas: &mut Option<WgpuAtlas>,
        plan: &mut Plan,
    ) -> Result<(), GpuError> {
        plan.clear();
        // Grid: the offset lives in one viewport. Command marks first (sprites,
        // degenerate inversion rectangle), then ground → search → selection →
        // caret → glyphs.
        plan.ops.push(Op::Viewport(frame.origin_px()));
        self.glyph_draws(
            plan,
            atlas,
            &[],
            frame.clusters(),
            frame.stripes(),
            CursorBlock::default(),
        )?;
        plan.quads(frame.bg_instances());
        plan.search(frame, false);
        plan.selection(
            frame.selection_instances(),
            frame.selection_rgba(),
            frame.selection_radius(),
        );
        plan.rounded(
            frame.grid_caret().as_slice(),
            frame.caret_core(),
            frame.caret_sdf(),
        );
        self.glyph_draws(
            plan,
            atlas,
            frame.glyphs(),
            frame.clusters(),
            frame.rules(),
            *frame.cursor_block(),
        )?;
        // Fill band: the third coordinate space, above the grid; search but no
        // selection, no caret slot, so the inversion rectangle is degenerate.
        if frame.fill_rows() != 0 {
            plan.ops.push(Op::Viewport(frame.fill_origin_px()));
            plan.quads(frame.fill_bg());
            plan.search(frame, true);
            self.glyph_draws(
                plan,
                atlas,
                frame.fill_glyphs(),
                frame.clusters(),
                frame.fill_rules(),
                CursorBlock::default(),
            )?;
        }
        // Dock: last, with two origins.
        if frame.dock().is_some() {
            let band_y = (viewport_px[1] - frame.dock_band_px()).max(0.0);
            let origin_y = (viewport_px[1] - frame.dock_layout_px()).max(0.0);
            plan.ops.push(Op::Viewport(band_y));
            plan.quads(&frame.dock_ground(viewport_px[0]));
            plan.ops.push(Op::Viewport(origin_y));
            let clipped = band_y > origin_y;
            let band = Op::Scissor(scissor_below(band_y, viewport_px));
            let open = Op::Scissor(scissor_below(0.0, viewport_px));
            if clipped {
                plan.ops.push(band.clone());
            }
            plan.quads(frame.dock_bg());
            plan.selection(
                frame.dock_selection_instances(),
                frame.selection_rgba(),
                frame.selection_radius(),
            );
            for draw in frame.dock_button_draws(origin_y) {
                plan.rounded(std::slice::from_ref(&draw.instance), draw.core, draw.shape);
            }
            // The caret is drawn outside the scissor and the band scissor
            // comes back after it.
            if clipped {
                plan.ops.push(open.clone());
            }
            plan.rounded(
                frame.dock_caret(origin_y).as_slice(),
                frame.caret_core(),
                frame.caret_sdf(),
            );
            if clipped {
                plan.ops.push(band);
            }
            let heat = *frame.dock_fx_heat();
            // Ghosts before the dock's glyphs; arrivals between the glyphs and
            // the rules, so the call splits while one is in flight.
            self.fx_draw(
                plan,
                atlas,
                frame.dock_ghosts(),
                frame.fx_clusters(),
                heat,
                origin_y,
            )?;
            let arrivals = frame.dock_arrivals();
            let (glyph_rules, late_rules) = if arrivals.is_empty() {
                (frame.dock_rules(), &[][..])
            } else {
                (&[][..], frame.dock_rules())
            };
            self.glyph_draws(
                plan,
                atlas,
                frame.dock_glyphs(),
                frame.dock_clusters(),
                glyph_rules,
                *frame.cursor_block(),
            )?;
            self.fx_draw(plan, atlas, arrivals, frame.dock_clusters(), heat, origin_y)?;
            self.glyph_draws(
                plan,
                atlas,
                &[],
                frame.dock_clusters(),
                late_rules,
                *frame.cursor_block(),
            )?;
            if clipped {
                plan.ops.push(open);
            }
        }
        Ok(())
    }

    /// Draws the frame into `target` and **submits** it: one render pass, the
    /// ground painted by the pass's clear load (no full-screen quad, no draw
    /// call), the plan on top.
    ///
    /// Does not wait and does not track completion (the offscreen tests and
    /// the measurement hook read or wait themselves); the frame path is
    /// [`Renderer::draw`]. The measurement hook times this whole call as
    /// `cpu_encode`: from planning (slot resolution and uploads included) to
    /// `submit`.
    #[cfg(test)]
    pub(crate) fn submit(
        &self,
        target: &Target,
        clear: LinearRgba,
        frame: &Frame,
    ) -> Result<wgpu::SubmissionIndex, GpuError> {
        self.encode(target, clear, frame, None)
    }

    fn encode(
        &self,
        target: &Target,
        clear: LinearRgba,
        frame: &Frame,
        timing: Option<&Timing>,
    ) -> Result<wgpu::SubmissionIndex, GpuError> {
        let viewport_px = [
            target.texture.width() as f32,
            target.texture.height() as f32,
        ];
        let mut state = self.state.borrow_mut();
        let state = &mut *state;
        let plan = &mut state.plan;
        self.plan(frame, viewport_px, &mut state.atlas, plan)?;
        #[cfg(test)]
        if self.poison.take() {
            // Outside the target: validation rejects the pass.
            plan.ops
                .push(Op::Scissor([0, 0, u32::MAX / 2, u32::MAX / 2]));
        }
        self.gpu
            .fill_buffer(&mut state.quads, "instances", bytes_of(&plan.instances));
        self.gpu
            .fill_buffer(&mut state.glyphs, "glyphs", bytes_of(&plan.glyphs));
        self.gpu
            .fill_buffer(&mut state.fx, "effects", bytes_of(&plan.fx));
        // The effects' bind group is resolved **after** planning: a later list
        // of the same frame may have created the colour texture.
        let fx_bind = if plan.fx.is_empty() {
            None
        } else {
            state
                .atlas
                .as_mut()
                .and_then(|a| a.fx_bind(self.gpu))
                .cloned()
        };
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let [r, g, b, a] = clear.to_array();
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("frame"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // Linear, unconverted: on an sRGB target the hardware
                        // encodes the clear colour too.
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(r),
                            g: f64::from(g),
                            b: f64::from(b),
                            a: f64::from(a),
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                timestamp_writes: timing.map(|t| wgpu::RenderPassTimestampWrites {
                    query_set: &t.queries,
                    beginning_of_pass_write_index: Some(0),
                    end_of_pass_write_index: Some(1),
                }),
                ..wgpu::RenderPassDescriptor::default()
            });
            let mut imm = Immediates {
                viewport_px,
                ..Immediates::default()
            };
            let cell_px = frame.cell_px();
            for op in &plan.ops {
                match op {
                    Op::Viewport(y) => {
                        pass.set_viewport(0.0, *y, viewport_px[0], viewport_px[1], 0.0, 1.0);
                    }
                    Op::Scissor([x, y, w, h]) => pass.set_scissor_rect(*x, *y, *w, *h),
                    Op::Quads(range) => {
                        let Some(buffer) = state.quads.as_ref() else {
                            continue;
                        };
                        pass.set_pipeline(&self.gpu.cell_bg);
                        pass.set_vertex_buffer(0, buffer.slice(..));
                        pass.set_immediates(0, bytes_of(std::slice::from_ref(&imm)));
                        pass.draw(0..4, range.clone());
                    }
                    Op::Rounded { range, core, shape } => {
                        let Some(buffer) = state.quads.as_ref() else {
                            continue;
                        };
                        imm.core = *core;
                        imm.shape = *shape;
                        pass.set_pipeline(&self.gpu.caret);
                        pass.set_vertex_buffer(0, buffer.slice(..));
                        pass.set_immediates(0, bytes_of(std::slice::from_ref(&imm)));
                        pass.draw(0..4, range.clone());
                    }
                    Op::Selection {
                        range,
                        color,
                        radius,
                    } => {
                        let Some(buffer) = state.quads.as_ref() else {
                            continue;
                        };
                        let selection = Immediates {
                            core: *color,
                            shape: [*radius, 0.0, 0.0, 0.0],
                            viewport_px,
                            pad: [0.0; 2],
                        };
                        pass.set_pipeline(&self.gpu.selection);
                        pass.set_vertex_buffer(0, buffer.slice(..));
                        pass.set_immediates(0, bytes_of(std::slice::from_ref(&selection)));
                        pass.draw(0..4, range.clone());
                    }
                    Op::Glyphs {
                        plane,
                        range,
                        cursor,
                        uv_size,
                    } => {
                        let atlas = state.atlas.as_ref();
                        let (pipeline, texture) = match plane {
                            Plane::Mask => (&self.gpu.cell, atlas.and_then(|a| a.mask.as_ref())),
                            Plane::Color => (&self.gpu.emoji, atlas.and_then(|a| a.color.as_ref())),
                        };
                        let (Some(texture), Some(buffer)) = (texture, state.glyphs.as_ref()) else {
                            continue;
                        };
                        let glyph_imm = GlyphImmediates {
                            cursor: *cursor,
                            viewport_px,
                            cell_px,
                            uv_size: *uv_size,
                            pad: [0.0; 2],
                        };
                        pass.set_pipeline(pipeline);
                        pass.set_bind_group(0, &texture.bind, &[]);
                        pass.set_vertex_buffer(0, buffer.slice(..));
                        pass.set_immediates(0, bytes_of(std::slice::from_ref(&glyph_imm)));
                        pass.draw(0..4, range.clone());
                    }
                    Op::Fx {
                        range,
                        heat,
                        uv_size,
                        origin_y,
                    } => {
                        let (Some(bind), Some(buffer)) = (fx_bind.as_ref(), state.fx.as_ref())
                        else {
                            continue;
                        };
                        let fx_imm = FxImmediates {
                            heat: *heat,
                            viewport_px,
                            cell_px,
                            uv_size: *uv_size,
                            pad: [0.0; 2],
                        };
                        pass.set_viewport(0.0, 0.0, viewport_px[0], viewport_px[1], 0.0, 1.0);
                        pass.set_pipeline(&self.gpu.glyph_fx);
                        pass.set_bind_group(0, bind, &[]);
                        pass.set_vertex_buffer(0, buffer.slice(..));
                        pass.set_immediates(0, bytes_of(std::slice::from_ref(&fx_imm)));
                        pass.draw(0..4, range.clone());
                        pass.set_viewport(0.0, *origin_y, viewport_px[0], viewport_px[1], 0.0, 1.0);
                    }
                }
            }
        }
        if let Some(t) = timing {
            encoder.resolve_query_set(&t.queries, 0..2, &t.resolve, 0);
            encoder.copy_buffer_to_buffer(&t.resolve, 0, &t.readback, 0, TIMESTAMP_BYTES);
        }
        Ok(self.gpu.queue.submit([encoder.finish()]))
    }

    /// The frame path (Karar 6): encode and submit inside an error scope,
    /// then track the submission for [`Renderer::poll`].
    ///
    /// **Asynchronous**: `Ok` only says "submitted". A
    /// synchronous error (planning, validation, out of memory) returns `Err`
    /// and the frame is **not** tracked, so it is never counted; the caller
    /// sends it to the same policy as the asynchronous error from `poll`
    /// (`Retry::draw_failed`). The scope stack is thread-local
    /// (`Device::push_error_scope`'s doc), so a parallel renderer's error
    /// cannot land in this frame's scope.
    pub(crate) fn draw(
        &self,
        target: &Target,
        clear: LinearRgba,
        frame: &Frame,
    ) -> Result<(), GpuError> {
        let timing = self.timing.get().then(|| {
            self.spare
                .borrow_mut()
                .pop()
                .unwrap_or_else(|| Timing::new(self.gpu))
        });
        let fault = self.gpu.fault.generation();
        let memory = self
            .gpu
            .device
            .push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let validation = self
            .gpu
            .device
            .push_error_scope(wgpu::ErrorFilter::Validation);
        let submitted = self.encode(target, clear, frame, timing.as_ref());
        // Popped in reverse order; on native both futures are ready at once.
        let errors = [block_on(validation.pop()), block_on(memory.pop())];
        let index = submitted?;
        if let Some(error) = errors.into_iter().flatten().next() {
            if let Some(timing) = timing {
                self.spare.borrow_mut().push(timing);
            }
            return Err(GpuError::Wgpu(error.to_string()));
        }
        // The frame is on its way: the counters change here, so a frame that
        // could not be submitted does not pollute `hucre=`.
        self.last_counts
            .set([frame.bg_count(), frame.glyph_count(), frame.rule_count()]);
        self.in_flight.borrow_mut().push_back(Pending {
            index,
            fault,
            timing,
        });
        Ok(())
    }

    /// Queues a drawn window texture for presentation — after
    /// [`Renderer::draw`] returned `Ok` for it.
    pub(crate) fn present(&self, texture: wgpu::SurfaceTexture) {
        self.gpu.queue.present(texture);
    }

    /// The non-blocking poll at the start of a tick (Karar 6): hands every
    /// submitted frame the GPU has finished, oldest first, to `on_complete` —
    /// `Ok(span)` for a frame finished without error (counted in
    /// [`Renderer::frames`]; `span` is the GPU delta when measured),
    /// `Err` for one that failed on the GPU (not counted). Returns whether a
    /// frame is still in flight.
    ///
    /// Each check is a zero-timeout wait on that frame's index: wgpu's plain
    /// `Poll` only says "queue empty or not", and the device may be shared.
    /// `on_complete` must not call back into this renderer.
    ///
    /// **`acilis=`** is closed by the caller at the first `Ok` — the moment
    /// this poll **observes** the finish, which trails the finish itself by at
    /// most one tick (or one delayed poll); native wgpu has no earlier
    /// observable moment without a thread of its own.
    pub(crate) fn poll(
        &self,
        mut on_complete: impl FnMut(Result<Option<GpuSpan>, GpuError>),
    ) -> bool {
        let mut in_flight = self.in_flight.borrow_mut();
        while let Some(front) = in_flight.front_mut() {
            let done = self.gpu.device.poll(wgpu::PollType::Wait {
                submission_index: Some(front.index.clone()),
                timeout: Some(Duration::ZERO),
            });
            let failed = match done {
                Ok(_) => self.gpu.fault.since(front.fault),
                Err(wgpu::PollError::Timeout) => break,
                Err(e) => Some(e.to_string()),
            };
            let result = match failed {
                Some(message) => Err(GpuError::Wgpu(message)),
                None => match front.timing.as_mut() {
                    None => Ok(None),
                    Some(timing) => match timing.read(self.gpu) {
                        // The readback is not mapped yet: next tick.
                        None => break,
                        Some(span) => Ok(span),
                    },
                },
            };
            // A failed frame's `Timing` is dropped, not pooled: its readback
            // may still be mid-`map_async`, and a mapped buffer reused as a
            // copy target would fail every later timed frame's validation.
            if let Some(pending) = in_flight.pop_front()
                && let Some(timing) = pending.timing
                && result.is_ok()
            {
                self.spare.borrow_mut().push(timing);
            }
            if result.is_ok() {
                self.frames.set(self.frames.get() + 1);
            }
            on_complete(result);
        }
        !in_flight.is_empty()
    }

    /// Draws the frame offscreen and reads the pixels back — the body every
    /// offscreen guard shares, byte order B, G, R, A (the target's format).
    ///
    /// A validation error is caught by an error scope and becomes a **panic**:
    /// a test must never read an empty texture. The scope stack is
    /// **thread-local** with wgpu's `std` feature (`Device::push_error_scope`'s
    /// doc) and errors are raised on the calling thread, so on the shared
    /// device a parallel test's error cannot land in this scope's `pop`.
    #[cfg(test)]
    pub(crate) fn render_offscreen(&self, edge: u32, clear: LinearRgba, frame: &Frame) -> Vec<u8> {
        let scope = self
            .gpu
            .device
            .push_error_scope(wgpu::ErrorFilter::Validation);
        let target = self.target(edge);
        self.submit(&target, clear, frame)
            .expect("the frame could not be planned");
        let pixels = self.gpu.read_back(&target.texture);
        if let Some(error) = block_on(scope.pop()) {
            panic!("wgpu frame failed validation: {error}");
        }
        pixels
    }

    /// The same, returning the planning error instead of panicking — the
    /// "no atlas refuses glyphs" guard.
    #[cfg(test)]
    pub(crate) fn try_submit_offscreen(
        &self,
        edge: u32,
        clear: LinearRgba,
        frame: &Frame,
    ) -> Result<(), GpuError> {
        self.submit(&self.target(edge), clear, frame).map(|_| ())
    }
}

/// The mask texture, created on first use with the resident tofu written
/// once and never touched again — [`Atlas::slot`] hands out no bitmap when it
/// falls back to tofu, because the data is already here. Shared by glyphs and
/// effects.
fn mask_texture<'a>(
    gpu: &Gpu,
    atlas: &Atlas,
    mask: &'a mut Option<PlaneTexture>,
) -> &'a wgpu::Texture {
    let plane = mask.get_or_insert_with(|| {
        // `Atlas::slot` hands no bitmap when it falls back to tofu.
        let plane = gpu.plane_texture(MASK_FORMAT, atlas.texture_px());
        gpu.write_slot(
            &plane.texture,
            atlas.slot_origin(TOFU),
            atlas.metrics(),
            atlas.tofu_bitmap(),
            Plane::Mask,
        );
        plane
    });
    &plane.texture
}

/// The format of the texture this renderer creates for `plane` — the colour
/// plane's sRGB contract, asked directly (`the_color_plane_is_an_srgb_texture`).
#[cfg(test)]
pub(crate) fn plane_format(plane: Plane) -> wgpu::TextureFormat {
    let format = match plane {
        Plane::Mask => MASK_FORMAT,
        Plane::Color => COLOR_FORMAT,
    };
    Gpu::shared().plane_texture(format, (8, 8)).texture.format()
}

/// The atlas's uv size of one slot.
fn uv_size(atlas: &Atlas) -> [f32; 2] {
    let (cw, ch) = atlas.metrics().cell_px;
    let (tw, th) = atlas.texture_px();
    [f32::from(cw) / f32::from(tw), f32::from(ch) / f32::from(th)]
}

/// A render pipeline from a vertex/fragment pair: triangle strip, one target,
/// **straight-alpha blending**. `parts` is the pipeline layout, the shader
/// module and the instance buffer's layout.
///
/// The names are separate parameters, not derived as `{name}_vertex`: the
/// failing pipeline is reported by name ([`Gpu::new`]'s error scope).
///
/// The blend is **not a parameter**: all six pipelines want it, each for its
/// own reason — `cell` makes alpha from the atlas's coverage, `caret` has a
/// translucent halo, `glyph_fx`'s effect is itself transparency, `selection`
/// softens its round corner. All six output **straight** alpha, emoji
/// included: CoreGraphics writes colour glyphs premultiplied, but
/// `raster::draw_color` undoes it before upload (`raster::unpremultiply`'s doc:
/// premultiplying in sRGB-encoded space darkened them).
///
/// The alpha channel's **source factor is `One`**, not `SrcAlpha`. With a
/// straight fragment (`rgb`, `a = coverage`) the right colour is
/// `sa·src + (1-sa)·dst`, but the same factor on alpha gives
/// `sa² + (1-sa)·dst_a`, and on a half-covered edge the target's alpha drops
/// from 1 to 0.75. The layer is not opaque, so the compositor honours that
/// hole and the window's backdrop leaks through the letters' edges. With
/// `One` it is `sa + (1-sa)·dst_a`, which stays 1 when `dst_a = 1`.
fn pipeline(
    device: &wgpu::Device,
    parts: (
        &wgpu::PipelineLayout,
        &wgpu::ShaderModule,
        &wgpu::VertexBufferLayout<'_>,
    ),
    vs_name: &'static str,
    fs_name: &'static str,
) -> wgpu::RenderPipeline {
    let (layout, module, buffer) = parts;
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(fs_name),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some(vs_name),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Some(buffer.clone())],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            ..wgpu::PrimitiveState::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(fs_name),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: FORMAT,
                blend: Some(wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::SrcAlpha,
                        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                        operation: wgpu::BlendOperation::Add,
                    },
                    alpha: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::One,
                        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                        operation: wgpu::BlendOperation::Add,
                    },
                }),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
pub(crate) mod tests;
#[cfg(test)]
mod wgpu_tests;
