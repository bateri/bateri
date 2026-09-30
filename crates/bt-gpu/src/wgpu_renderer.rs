//! The renderer: wgpu, one device per process ([`Gpu`]) and one [`Renderer`]
//! per pane (the atlas's key includes the pane's point size).
//!
//! Since 040 phase-5 this is the **product** renderer; the Metal renderer
//! (`crate::renderer::MetalRenderer`) stays behind `cfg(test)` as the oracle
//! (`.tasks/040-linux-kapisi-ve-wgpu/discussion.md` → Karar 3 and 4) until
//! phase-7 removes it. Every pipeline group is here: `cell_bg` + caret
//! (background quads, the caret's SDF, the dock buttons), `cell` + `emoji`
//! (glyphs and rules from the atlas's two planes), `selection` (the mouse
//! selection and the search highlights) and `glyph_fx` (the dock's typing
//! effects), on all three surfaces (grid, fill band, dock).
//!
//! Pipeline order, the viewport/scissor sequence and blending are **the same**
//! as the oracle's `encode_pass` / `encode_fill` / `encode_dock` /
//! `encode_fx` / `encode_glyphs` / `pipeline` in `renderer.rs`; the reasons
//! live there and are not repeated here. Slot resolution, the wide glyph's
//! fan-out, uv baking and the effects' instance packing are not repeated
//! either: both renderers call [`crate::slots`], and only the upload target
//! differs ([`WgpuUpload`]). The one difference is how commands are recorded:
//! Metal builds a buffer per list per frame, here every quad of the frame goes
//! into **one** instance buffer (every glyph into one glyph buffer, every
//! effect into one effect buffer) and draws read ranges of them ([`Plan`]).
//! Those buffers are not rebuilt per frame either: they live as long as the
//! renderer, grow on demand and are filled with `write_buffer` — creating a
//! buffer in wgpu is a validation and tracking round trip, and it was
//! measured (phase-2 → Uygulama Notları): a per-frame buffer visibly inflated
//! `cpu_encode`.
//!
//! **Completion** (Karar 6) is a submission index per frame and a
//! non-blocking [`Renderer::poll`] at the start of a tick — no closure per
//! frame. The four jobs of Metal's completion block are carried by name there
//! and in `crate::link`.

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
use crate::renderer::{CellMetrics, FontNotice, scissor_rect_below};
use crate::slots::{self, SlotUpload};

/// Immediate data budget in bytes: the smallest `maxPushConstantsSize` Vulkan
/// **guarantees** (discussion.md → Karar 5). Metal offers 4096, but a layout
/// that does not fit the smallest Linux driver would fail there; the device is
/// requested with exactly this limit, so an oversized pipeline is rejected on
/// macOS too.
pub(crate) const IMMEDIATE_BUDGET: u32 = 128;

/// Target format — Metal's `BGRA8Unorm_sRGB`: the fragment writes **linear**,
/// the hardware encodes to sRGB (`CLAUDE.md` → colour space).
pub(crate) const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;

/// The atlas's mask plane: one channel of coverage (Metal's `R8Unorm`).
pub(crate) const MASK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;

/// The atlas's colour plane: **sRGB**, so sampling decodes to linear (Metal's
/// `RGBA8Unorm_sRGB`; why it must be sRGB is `new_color_texture`'s doc).
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

/// Field-for-field twin of `cell.wgsl` → `Immediates`: Metal's three vertex
/// uniforms (`viewport_px`, `cell_px`, `uv_size`) and the fragment's
/// [`CursorBlock`] in one block.
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

/// One step of the frame's draw list, in Metal's encode order.
#[derive(Clone, Debug, PartialEq)]
enum Op {
    /// `setViewport`: the origin's y; the size is the texture's (`viewport_at`).
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
    /// selection, or one search role (`encode_selection`/`encode_search`).
    Selection {
        range: Range<u32>,
        color: [f32; 4],
        radius: f32,
    },
    /// The `cell` (mask) or `emoji` (colour) pipeline over a range of the
    /// glyph buffer, with that plane's texture bound. `cursor` is the text
    /// inversion rectangle for this list (degenerate for stripes and the fill
    /// band, `encode_glyphs`' callers say why); `uv_size` is the atlas's.
    Glyphs {
        plane: Plane,
        range: Range<u32>,
        cursor: CursorBlock,
        uv_size: [f32; 2],
    },
    /// The `glyph_fx` pipeline over a range of the effect buffer: drawn in
    /// its own full-texture viewport (instances already carry `origin_y`),
    /// after which the dock's viewport at `origin_y` is restored
    /// (`encode_fx`).
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
        // `core`/`shape` are per draw: two quads would compute their SDF
        // against the same rectangle (twin of `encode_caret`'s guard).
        debug_assert!(instances.len() == 1, "one quad per rounded draw");
        let range = self.push(instances);
        self.ops.push(Op::Rounded { range, core, shape });
    }

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

    /// Search highlights, `encode_search`'s order: every match, then the
    /// current match over them. `fill` picks the fill band's lists.
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

/// wgpu form of `renderer.rs` → `scissor_below`; the arithmetic has a single
/// copy (`scissor_rect_below`).
fn scissor_below(top_px: f32, viewport_px: [f32; 2]) -> [u32; 4] {
    scissor_rect_below(top_px, viewport_px).map(|v| v as u32)
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

/// The atlas and its two textures — **in one place**, like `renderer.rs`'s
/// `AtlasTexture`: when [`Atlas::ensure`] rebuilds the atlas both textures
/// are dropped in the same line ([`Renderer::cell_metrics`]).
struct WgpuAtlas {
    atlas: Atlas,
    /// `None` → not created yet, or `ensure` dropped it. Created by the first
    /// frame that draws a glyph, with the resident tofu written once.
    mask: Option<PlaneTexture>,
    /// `None` → no emoji seen yet. **Lazy**, created by the first colour
    /// upload ([`WgpuUpload`]); a session without emoji never pays for it.
    color: Option<PlaneTexture>,
    /// The effects' bind group (both planes and both samplers), keyed by
    /// whether the colour texture existed when it was made: while it does
    /// not, the mask is bound to the colour slot too (`encode_fx`'s reason).
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
/// `write_texture` lands before the next `submit`'s commands, i.e. before the
/// frame that samples it — and unlike Metal's `replaceRegion` it is staged
/// by the queue, so the known in-flight-overwrite limit of
/// `AtlasTexture::prepare` does not apply here.
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
    /// `None` until [`Renderer::cell_metrics`] is asked, like Metal's
    /// `MetalRenderer::atlas`: the atlas key's scale comes from the window, and a
    /// frame with glyphs but no atlas fails with [`GpuError::NoAtlas`] rather
    /// than drawing @1x.
    atlas: Option<WgpuAtlas>,
    /// The frame's plan, kept between frames: `clear` keeps the capacity of
    /// its lists (and of the `slots` scratch lists), so the steady state does
    /// not allocate — Metal's `AtlasTexture` keeps its lists the same way.
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
/// is worth creating once per process. The split is also the product's shape
/// (040 phase-5): one device, a renderer per pane — the atlas's key includes
/// the pane's point size.
pub(crate) struct Gpu {
    /// Kept for the window surfaces ([`crate::Surface`]): a surface must come
    /// from the instance its device's adapter came from.
    instance: wgpu::Instance,
    device: wgpu::Device,
    queue: wgpu::Queue,
    cell_bg: wgpu::RenderPipeline,
    caret: wgpu::RenderPipeline,
    selection: wgpu::RenderPipeline,
    cell: wgpu::RenderPipeline,
    emoji: wgpu::RenderPipeline,
    glyph_fx: wgpu::RenderPipeline,
    /// The atlas planes' bind group layout: texture @0, sampler @1.
    plane_layout: wgpu::BindGroupLayout,
    /// The effects' layout: mask @0, colour @1, nearest @2, linear @3.
    fx_layout: wgpu::BindGroupLayout,
    /// Nearest, clamp-to-edge: `cell.metal`'s `constexpr sampler`.
    sampler: wgpu::Sampler,
    /// Linear, clamp-to-edge: `glyph_fx.metal`'s `lin`, for the scaling
    /// branches (texel-centre clamping keeps it inside the slot).
    linear: wgpu::Sampler,
    /// `TIMESTAMP_QUERY` was granted: the GPU delta can be measured
    /// ([`Renderer::set_gpu_timing`]); otherwise its token is
    /// `unsupported` (Karar 6).
    timestamps: bool,
    fault: Arc<Fault>,
}

impl Gpu {
    /// The process-wide device: created once and shared by every renderer —
    /// every pane of every window (the Metal side built a new device per
    /// renderer; an adapter, a device and six pipelines are not worth paying
    /// per pane). A failure is kept too, so every pane reports the same error
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
    /// today and the oracle compares the same hardware path; the Vulkan branch
    /// is opened and tested on Linux in the font set (plan.md → Kapsam Dışı).
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
            required_limits: wgpu::Limits {
                max_immediate_size: IMMEDIATE_BUDGET,
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
        // `linear`'s last column would blend the neighbour slot (`cell.metal`).
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

    /// Writes one full slot — twin of `renderer.rs` → `upload_slot`, with the
    /// same length check before the copy (the byte length and row pitch come
    /// from [`slots::slot_layout`], shared with Metal).
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
/// `Stats::record_gpu` (Metal's `GPUStartTime`/`GPUEndTime`).
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
/// buffers and in-flight frames — one per pane, like the oracle
/// (`MetalRenderer`), which also owns its atlas. `RefCell`/`Cell`: Metal's reason — `&self` methods that
/// mutate the atlas, and a renderer that never leaves its thread (its frames
/// are counted by [`Renderer::poll`] on that thread, not on a driver
/// thread as Metal's completion block does).
pub struct Renderer {
    gpu: &'static Gpu,
    state: RefCell<State>,
    /// The requested font: the family and size half of the atlas's key
    /// (`MetalRenderer::font`'s reason).
    font: RefCell<FontOptions>,
    /// Frames the GPU finished **without error**; `make duman`'s `kare=`.
    frames: Cell<u64>,
    /// The last **submitted** frame's background, glyph and rule counts
    /// (`hucre=`, `glif=`, `kural=`); CPU counters, like Metal's.
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
    /// geometry — twin of `MetalRenderer::cell_metrics` (same arithmetic,
    /// `CellMetrics::from_atlas`). When the key changes, both textures are
    /// dropped in the same line (`MetalRenderer::sync_atlas`'s reason: a texture
    /// of the old size written with the new metrics would silently corrupt).
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

    /// Changes the requested font; `true` when it differs — twin of
    /// `MetalRenderer::set_font`: the atlas opens with it on the next
    /// [`Renderer::cell_metrics`].
    pub fn set_font(&self, font: &FontOptions) -> bool {
        let mut current = self.font.borrow_mut();
        if *current == *font {
            return false;
        }
        current.clone_from(font);
        true
    }

    /// What to tell the user about the open atlas's font — twin of
    /// `MetalRenderer::font_notice`.
    pub fn font_notice(&self) -> Option<FontNotice> {
        let state = self.state.borrow();
        let issue = state.atlas.as_ref()?.atlas.font_issue()?;
        Some(FontNotice::from(issue.clone()))
    }

    /// The mask plane's slot occupancy (used, total); `(0, 0)` without an
    /// atlas — twin of `MetalRenderer::atlas_occupancy`.
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

    /// The glyph and rule draws of one list — `encode_glyphs`' twin: emoji
    /// **before** the mask list (the order's reason is there), glyphs and
    /// rules in one mask list, rules last ([`slots::glyph_lists`]).
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
        // draw, as on Metal.
        if color.is_some() {
            plan.glyph_draw(Plane::Color, cursor, uv_size);
        }
        plan.glyph_draw(Plane::Mask, cursor, uv_size);
        Ok(())
    }

    /// The typing effects' draw — `encode_fx`'s twin: slot resolution through
    /// [`slots::fx_list`], colour-plane instances dropped while no colour
    /// texture exists, positions moved down by `origin_y` (the effect is
    /// drawn in window space, so it may spill over the hairline by its pad).
    #[allow(clippy::too_many_arguments)] // `encode_fx`'s reason: the table is half of `cells`
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

    /// The draw plan for a `Frame` — `encode_pass`'s order, every group.
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
        // caret → glyphs (`encode_pass`).
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
        // selection, no caret slot, so the inversion rectangle is degenerate
        // (`encode_fill`).
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
        // Dock: last, with two origins (see `encode_dock`'s doc).
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
            // The caret is drawn outside the scissor (`encode_dock`'s reason)
            // and the band scissor comes back after it.
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
            // the rules, so the call splits while one is in flight
            // (`encode_dock`).
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
    /// ground loaded with `clear` (Metal's `MTLLoadAction::Clear`), the plan on
    /// top.
    ///
    /// Does not wait and does not track completion (the offscreen tests and
    /// the measurement hook read or wait themselves); the frame path is
    /// [`Renderer::draw`]. The measurement hook times this whole call as
    /// `cpu_encode` — on the Metal side the span runs from creating the
    /// command buffer to `commit`, here from planning (slot resolution and
    /// uploads included, as in Metal's `encode_glyphs`) to `submit`.
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
                        // encodes the clear too (`MTLClearColor`'s semantics).
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
    /// **Asynchronous**, like Metal's `draw`: `Ok` only says "submitted". A
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
        // could not be submitted does not pollute `hucre=` (Metal's rule).
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
    /// offscreen guard shares, same byte order as Metal's (B, G, R, A).
    ///
    /// A validation error is caught by an error scope and becomes a **panic**:
    /// a test must never read an empty texture (the counterpart of the Metal
    /// side's `MTLCommandBufferStatus::Error` check). The scope stack is
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
/// once — `AtlasTexture::ensure_texture`'s twin, shared by glyphs and effects.
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

/// Twin of `renderer.rs` → `pipeline`: same blending (straight alpha, the alpha
/// channel's source factor is `One` — reason there), triangle strip, one
/// target. `parts` is the pipeline layout, the shader module and the instance
/// buffer's layout.
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
mod tests {
    use std::time::{Duration, Instant};

    use bt_core::{
        Block, ButtonState, CaretShape, CaretStyle, Cell, DockButton, Erase, Keypress, SearchRun,
        SelectionRun, Theme, UnderlineStyle,
    };
    use objc2_metal::MTLCommandBuffer;

    use super::*;
    use crate::glyph_fx::{Effect, Fx, Kind};
    use crate::renderer::MetalRenderer;
    use crate::renderer::tests::{
        ACCENT, BACKGROUND, MIDTONE, WHITE, bg_cell, cell_rows, commit_offscreen, grid,
        grid_with_gutter, metal_offscreen, pixel_at, target_texture,
    };
    use crate::stats::{Samples, Stats};

    /// Fixed cell for the synthetic scenes: 8×16, eight columns and four rows
    /// on a 64 texture.
    const CELL: (u16, u16) = (8, 16);

    #[test]
    fn wgsl_pipelines_build() {
        // wgpu successor of `metallib_is_embedded_and_valid` (Karar 9): the
        // WGSL passes naga and every pipeline builds on a device requested
        // with Vulkan's immediate floor. If the WGSL side of an `Immediates`
        // block outgrows its pipeline layout, creation fails and `shared`'s
        // `expect` names the failing pipeline.
        let _ = Gpu::shared();
    }

    // **Guards that need the wgpu internals** (a hand-built pass, the colour
    // plane's texture) or have no Metal original. Since phase-4 the other
    // guards live in `renderer.rs` and draw with this renderer; the phase-2/3
    // twins of those were merged into them. Glyph tests draw at `SCALE`.

    /// Retina: at 13pt@1x a flag cluster's ink exceeds two cells and falls back
    /// to its base character (`a_cluster_is_one_color_glyph_on_every_surface`),
    /// so the scene list could not show a cluster at 1x.
    const SCALE: f64 = 2.0;

    /// An inked cell with a white foreground (`renderer.rs`'s `glyph_cell`,
    /// with a row).
    fn glyph_cell(col: u16, row: u16, ch: char) -> Cell {
        Cell {
            col,
            row,
            ch: Some(ch),
            fg: WHITE,
            ..Cell::default()
        }
    }

    /// The atlas's own grid **without a gutter**, so column `n` starts at
    /// `n * cell width` and `cell_rows` reads it.
    fn flush_left(m: CellMetrics) -> CellMetrics {
        let (cw, ch) = m.cell_px();
        CellMetrics::new(cw, ch, m.context_cell_px(), 0, m.rule_px()).expect("non-zero metrics")
    }

    /// Draws one synthetic colour slot with the `emoji` pipeline over opaque
    /// black and returns a pixel inside it — twin of `emoji_round_trip`.
    ///
    /// Synthetic on purpose (a real emoji's bitmap is not bit-stable across
    /// macOS releases), and the instance's colour is **red**: whatever comes
    /// out must come from the texture.
    fn emoji_round_trip(rgb: (u8, u8, u8), alpha: u8) -> (u8, u8, u8) {
        const EDGE: u32 = 16;
        const SLOT: u16 = 8;
        let gpu = Gpu::shared();
        let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let color = gpu.plane_texture(COLOR_FORMAT, (EDGE as u16, EDGE as u16));
        let slot: Vec<u8> = (0..usize::from(SLOT) * usize::from(SLOT))
            .flat_map(|_| [rgb.0, rgb.1, rgb.2, alpha])
            .collect();
        gpu.queue.write_texture(
            color.texture.as_image_copy(),
            &slot,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(u32::from(SLOT) * 4),
                rows_per_image: Some(u32::from(SLOT)),
            },
            wgpu::Extent3d {
                width: u32::from(SLOT),
                height: u32::from(SLOT),
                depth_or_array_layers: 1,
            },
        );
        let instance = GlyphInstance {
            pos: [0.0, 0.0],
            uv0: [0.0, 0.0],
            rgba: [1.0, 0.0, 0.0, 1.0],
        };
        let mut buffer = None;
        gpu.fill_buffer(
            &mut buffer,
            "round trip",
            bytes_of(std::slice::from_ref(&instance)),
        );
        let buffer = buffer.expect("instance buffer");
        let target = gpu.target(EDGE);
        let imm = GlyphImmediates {
            viewport_px: [EDGE as f32; 2],
            cell_px: [f32::from(SLOT); 2],
            uv_size: [f32::from(SLOT) / EDGE as f32; 2],
            ..GlyphImmediates::default()
        };
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // Opaque black: the comparison base of the
                        // straight-alpha witness.
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..wgpu::RenderPassDescriptor::default()
            });
            pass.set_pipeline(&gpu.emoji);
            pass.set_bind_group(0, &color.bind, &[]);
            pass.set_vertex_buffer(0, buffer.slice(..));
            pass.set_immediates(0, bytes_of(std::slice::from_ref(&imm)));
            pass.draw(0..4, 0..1);
        }
        gpu.queue.submit([encoder.finish()]);
        let pixels = gpu.read_back(&target.texture);
        if let Some(error) = block_on(scope.pop()) {
            panic!("emoji round trip failed validation: {error}");
        }
        pixel_at(&pixels, EDGE as usize, 2, 2)
    }

    #[test]
    fn a_midtone_color_slot_survives_the_round_trip() {
        // A midtone (not a fixed point of the sRGB transfer, so a wrong
        // texture format could not pass) comes back as the same byte through
        // the sRGB colour plane — and not as the instance's red. Synthetic on
        // purpose: a real emoji's bitmap is not bit-stable across macOS
        // releases. Opaque, so straight and premultiplied blending agree here;
        // they part at the translucent edge below.
        const MID: (u8, u8, u8) = (0x80, 0x40, 0xc0);
        let seen = emoji_round_trip(MID, 0xff);
        assert!(
            seen.0.abs_diff(MID.0) <= 1
                && seen.1.abs_diff(MID.1) <= 1
                && seen.2.abs_diff(MID.2) <= 1,
            "colour plane round trip: {seen:02x?} ≠ {MID:02x?} \
             (red means the colour came from the instance)"
        );
    }

    #[test]
    fn a_translucent_edge_composites_in_linear_space() {
        // Half-alpha white over black is exactly half in linear space, 0xBC
        // once encoded; 0x80 — the signature of premultiplication left in
        // encoded space (`raster::unpremultiply`'s doc) — must not pass, so
        // the tolerance is ±2, far from 0x80.
        let seen = emoji_round_trip((0xff, 0xff, 0xff), 0x80);
        assert!(
            seen.0.abs_diff(0xbc) <= 2,
            "translucent edge did not composite linearly: {seen:02x?} ≠ ~0xbc"
        );
    }

    #[test]
    fn wide_glyph_halves_meet_without_a_seam() {
        // A wide glyph is two quads from two slots (`slots::fan`); the right
        // half is rasterised a whole number of pixels to the left, so its AA
        // phase is the left half's and a stroke crossing the boundary must
        // continue there pixel for pixel. `一` is one horizontal stroke across
        // nearly the full em: ink that crosses the boundary in any CJK font.
        // (The instance count is shared CPU code now and has its guard in
        // `a_wide_cell_becomes_two_quads`.)
        const EDGE: u32 = 64;
        let w = Renderer::new();
        let m = flush_left(w.cell_metrics(SCALE));
        let (cw, ch) = m.cell_px();
        let (cw, ch) = (usize::from(cw), usize::from(ch));
        assert!(
            2 * cw < EDGE as usize && ch < EDGE as usize,
            "two cells do not fit"
        );
        let mut frame = Frame::default();
        frame.clear(m, CaretStyle::default());
        frame.push(Cell {
            wide: true,
            ..glyph_cell(0, 0, '一')
        });
        let pixels = w.render_offscreen(EDGE, BACKGROUND, &frame);
        let edge = EDGE as usize;
        let px = |x, y| pixel_at(&pixels, edge, x, y);
        let clear = px(edge - 1, edge - 1);
        // It really is two cells wide, or the seam question is vacuous.
        let inked = |x| (0..ch).any(|y| px(x, y) != clear);
        assert!(
            inked(cw / 2) && inked(cw + cw / 2),
            "`一` did not draw across two cells"
        );
        let crossing: Vec<usize> = (0..ch).filter(|&y| px(cw - 1, y) != clear).collect();
        assert!(!crossing.is_empty(), "no ink at the boundary");
        for y in 0..ch {
            let (left, right) = (px(cw - 1, y), px(cw, y));
            assert!(
                left.0.abs_diff(right.0) <= 2
                    && left.1.abs_diff(right.1) <= 2
                    && left.2.abs_diff(right.2) <= 2,
                "seam at row {y}: {left:02x?} | {right:02x?}"
            );
        }
    }

    #[test]
    fn a_rule_is_drawn_over_its_glyph() {
        // Rules come after glyphs in the one mask list (`slots::glyph_lists`).
        // `█` is procedural and fills the whole cell (no font involved), so
        // the underline lies on it: drawn after the glyph it reads red, drawn
        // before it would vanish under the block's white.
        const EDGE: u32 = 64;
        let w = Renderer::new();
        let m = flush_left(w.cell_metrics(SCALE));
        let mut frame = Frame::default();
        frame.clear(m, CaretStyle::default());
        frame.push(Cell {
            underline: UnderlineStyle::Single,
            underline_color: Some(LinearRgba::from_srgb(0xff, 0x00, 0x00)),
            ..glyph_cell(0, 0, '█')
        });
        let pixels = w.render_offscreen(EDGE, BACKGROUND, &frame);
        let cell = cell_rows(&pixels, EDGE as usize, m.cell_px(), 0).concat();
        assert!(
            cell.contains(&(0xff, 0xff, 0xff)),
            "the block glyph was not drawn"
        );
        assert!(
            cell.contains(&(0xff, 0x00, 0x00)),
            "the underline is not on top of its glyph"
        );
    }

    // **Completion model** (Karar 6): the four jobs of Metal's completion
    // block, carried by the submission index and `poll`.

    /// A small frame with one coloured cell.
    fn one_cell_frame() -> Frame {
        let mut frame = Frame::default();
        frame.clear(grid(8, 8), CaretStyle::default());
        frame.push(bg_cell(0, 0, LinearRgba::from_srgb(0xff, 0x00, 0x00)));
        frame
    }

    /// Blocks until the device is idle **without** touching the renderer's
    /// bookkeeping: only `poll` may count a frame.
    fn wait_for_gpu(r: &Renderer) {
        r.device()
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("waiting for the GPU failed");
    }

    #[test]
    fn a_finished_frame_is_counted_by_one_poll() {
        // `kare=` counts frames the GPU finished without error — not
        // submitted ones — and the last frame before the link sleeps is not
        // lost: while it is in flight `in_flight` asks for one delayed poll,
        // that single poll counts it, and an empty queue arms nothing (the
        // stop condition). `acilis=` closes at the first `Ok` the poll hands
        // over, not at submit.
        let r = Renderer::new();
        assert_eq!(r.frames(), 0);
        let target = r.target(16);
        let stats = Stats::new(Instant::now(), 1);
        r.draw(&target, BACKGROUND, &one_cell_frame())
            .expect("the frame was submitted");
        assert_eq!(r.frames(), 0, "a submitted frame is not a finished one");
        assert!(r.in_flight(), "the submitted frame is not tracked");
        assert_eq!(stats.startup(), None);
        assert_eq!(r.last_bg_count(), 1, "`hucre=` of the submitted frame");
        assert_eq!((r.last_glyph_count(), r.last_rule_count()), (0, 0));
        // No atlas was asked for: an unopened atlas has no slots (Metal's
        // `(0, 0)` rule).
        assert_eq!(
            (r.atlas_occupancy(), r.color_atlas_occupancy()),
            ((0, 0), (0, 0))
        );
        wait_for_gpu(&r);
        assert_eq!(r.frames(), 0, "only `poll` counts");
        let mut seen = Vec::new();
        let still = r.poll(|result| {
            if result.is_ok() {
                stats.mark_startup();
            }
            seen.push(result.is_ok());
        });
        assert_eq!(seen, vec![true], "the finished frame was reported once");
        assert_eq!(r.frames(), 1, "a clean frame is counted");
        assert!(!still, "an empty queue must not ask for another poll");
        assert!(!r.in_flight());
        assert!(
            stats.startup().is_some(),
            "the first finished frame closes the startup time"
        );
        let mut again = 0;
        assert!(!r.poll(|_| again += 1));
        assert_eq!(again, 0, "a frame is reported once");
    }

    #[test]
    fn a_frame_failing_validation_is_not_counted() {
        // The synchronous leg: an error caught around the submit returns
        // `Err` (the caller sends it to `Retry::draw_failed`) and the frame is
        // never tracked, so it can never be counted — otherwise `make duman`
        // would pass a black window.
        let r = Renderer::new();
        let target = r.target(16);
        r.poison_next_frame();
        let result = r.draw(&target, BACKGROUND, &one_cell_frame());
        assert!(
            matches!(result, Err(GpuError::Wgpu(_))),
            "an invalid frame was submitted silently: {result:?}"
        );
        assert!(!r.in_flight(), "a failed frame is tracked");
        assert_eq!(r.last_bg_count(), 0, "a failed frame pollutes `hucre=`");
        wait_for_gpu(&r);
        assert!(!r.poll(|_| panic!("nothing to report")));
        assert_eq!(r.frames(), 0);
        // The next frame is clean again: the hook is one-shot.
        r.draw(&target, BACKGROUND, &one_cell_frame())
            .expect("a clean frame after a failed one");
    }

    #[test]
    fn a_device_fault_fails_the_frame_in_flight() {
        // The asynchronous leg: a fault reported outside the frame's scope
        // (uncaptured error, device lost) fails every frame submitted before
        // it, through the same callback as a success — one policy for both.
        // A device of its own, so the fault cannot leak into other tests.
        let gpu: &'static Gpu = Box::leak(Box::new(Gpu::new().expect("a second device")));
        let r = Renderer::on(gpu);
        let target = r.target(16);
        r.draw(&target, BACKGROUND, &one_cell_frame())
            .expect("the frame was submitted");
        gpu.fault.report("injected fault".to_owned());
        wait_for_gpu(&r);
        let mut seen = Vec::new();
        assert!(!r.poll(|result| seen.push(result.map(|_| ()).map_err(|e| e.to_string()))));
        assert_eq!(seen.len(), 1);
        assert!(
            matches!(&seen[0], Err(message) if message.contains("injected fault")),
            "the fault did not reach the frame: {seen:?}"
        );
        assert_eq!(r.frames(), 0, "a faulted frame is counted");
    }

    #[test]
    fn gpu_timestamps_reach_the_ledger_or_say_unsupported() {
        // `TIMESTAMP_QUERY` present: the frame's pass writes two timestamps
        // and the poll hands them over as a span, recorded **once** (as a
        // sample or rejected — what the hardware gives is its business, the
        // test pins the pipe, like the Metal guard it replaces). Absent: the
        // gate stays closed and the token's value is `unsupported`.
        let r = Renderer::new();
        r.set_gpu_timing(true);
        let target = r.target(16);
        let stats = Stats::new(Instant::now(), 1);
        r.draw(&target, BACKGROUND, &one_cell_frame())
            .expect("the frame was submitted");
        let mut spans = Vec::new();
        // The readback's mapping may need a second look after the frame is
        // done; each round waits for the GPU, it does not sleep.
        for _ in 0..8 {
            wait_for_gpu(&r);
            if !r.poll(|result| spans.push(result.expect("a clean frame"))) {
                break;
            }
        }
        assert_eq!(spans.len(), 1, "the frame was reported once");
        if !r.gpu_timing_supported() {
            assert_eq!(spans[0], None, "no timestamp feature, yet a span");
            return;
        }
        let span = spans[0].expect("timestamps are supported, yet no span");
        stats.record_gpu(span.start, span.end);
        let gpu = stats.gpu();
        assert_eq!(gpu.nanos.len() as u64 + gpu.rejected, 1);
        assert!(!r.in_flight());
    }

    // **Oracle scene list** (Karar 4). Each scene is a `Frame` drawn on both
    // backends. The list grows as groups are ported: phase-2's scenes need no
    // atlas; phase-3's draw glyphs at the atlas's own metrics (`SCALE`).

    /// A scene: its name, texture edge and frame.
    type Scene = (&'static str, u32, Frame);

    fn scene_midtone() -> Scene {
        let mut frame = Frame::default();
        frame.clear(grid(8, 8), CaretStyle::default());
        frame.push(bg_cell(0, 0, LinearRgba::from_srgb(0xff, 0x00, 0x00)));
        frame.push(bg_cell(1, 1, LinearRgba::from_srgb(0x00, 0xff, 0x00)));
        frame.push(bg_cell(0, 1, MIDTONE));
        ("midtone ground", 16, frame)
    }

    /// Dock ground and both hairlines (two input rows + the context row, with
    /// a gutter), the caret's dock slot and an offset grid above it — the dock
    /// must be exempt from the offset.
    fn scene_dock_ground() -> Scene {
        let mut frame = Frame::default();
        frame.clear(grid_with_gutter(CELL.0, CELL.1, 4), CaretStyle::default());
        frame.push(bg_cell(0, 0, MIDTONE));
        frame.push(bg_cell(3, 1, ACCENT));
        frame.set_dock_input_rows(2);
        frame.push_dock(bg_cell(1, 0, MIDTONE));
        frame.open_dock(
            LinearRgba::from_srgb(0x20, 0x22, 0x28),
            WHITE,
            LinearRgba::from_srgb(0x60, 0x60, 0x60),
        );
        frame.set_dock_band(128.0, 0.0);
        frame.set_origin_rows(0.5);
        // The caret is inside the band: the dock slot, drawn in the
        // dock-local viewport.
        frame.push_caret([2.0, 6.0], BACKGROUND, ACCENT, 1.0, CaretShape::Block, true);
        assert!(
            frame.dock_caret(0.0).is_some(),
            "the scene did not put the caret in the dock slot"
        );
        ("dock ground, hairlines and dock caret", 128, frame)
    }

    /// Growing band: the layout overshoots the band's top, the scissor clips.
    fn scene_growing_band() -> Scene {
        let mut frame = Frame::default();
        frame.clear(grid(8, 8), CaretStyle::default());
        frame.set_dock_rows(4);
        frame.push_dock(bg_cell(0, 0, MIDTONE));
        frame.push_dock(bg_cell(0, 2, MIDTONE));
        frame.push_dock(bg_cell(0, 3, ACCENT));
        frame.set_dock_band(48.0, 0.0);
        frame.open_dock(LinearRgba::from_srgb(0x00, 0x40, 0x00), WHITE, WHITE);
        ("growing dock band (scissor)", 48, frame)
    }

    fn caret_scene(name: &'static str, shape: CaretShape, focused: bool) -> Scene {
        let mut frame = Frame::default();
        frame.clear(grid_with_gutter(CELL.0, CELL.1, 8), CaretStyle::default());
        frame.push(bg_cell(2, 1, MIDTONE));
        frame.push_caret([1.0, 1.0], BACKGROUND, ACCENT, 1.0, shape, focused);
        (name, 64, frame)
    }

    /// Wide-gutter glow: the `the_caret_glow_*` guards' setup, at half alpha.
    fn scene_glow() -> Scene {
        let mut frame = Frame::default();
        frame.clear(
            grid_with_gutter(CELL.0, CELL.1, 16),
            CaretStyle {
                glow: 2.0,
                ..CaretStyle::default()
            },
        );
        frame.push_caret([1.0, 1.0], BACKGROUND, ACCENT, 0.5, CaretShape::Block, true);
        ("glow at half alpha", 64, frame)
    }

    /// The dock's upload button: the second consumer of the caret fragment.
    fn scene_dock_button() -> Scene {
        let mut frame = Frame::default();
        frame.clear(grid(CELL.0, CELL.1), CaretStyle::default());
        frame.set_dock_rows(1);
        frame.open_dock(BACKGROUND, BACKGROUND, BACKGROUND);
        frame.set_dock_buttons([
            None,
            Some(DockButton {
                start: 0,
                end: 3,
                color: LinearRgba::from_srgb(0xd0, 0x30, 0x30),
                state: ButtonState::Hover,
            }),
        ]);
        assert!(
            frame.dock_button_draws(0.0).next().is_some(),
            "no button in the scene"
        );
        ("dock upload button", 64, frame)
    }

    /// Fill band: the third viewport, whose origin goes **negative** with the
    /// offset (the band's top overflows the window) and slides with the grid.
    fn scene_fill_band() -> Scene {
        let mut frame = Frame::default();
        frame.clear(grid(8, 8), CaretStyle::default());
        frame.push(bg_cell(0, 0, ACCENT));
        frame.set_fill_rows(3);
        frame.push_fill(bg_cell(0, 0, MIDTONE));
        frame.push_fill(bg_cell(1, 2, LinearRgba::from_srgb(0x00, 0x80, 0xff)));
        frame.set_origin_rows(1.5);
        assert!(
            frame.fill_origin_px() < 0.0,
            "the band origin is not negative"
        );
        ("fill band, negative origin", 32, frame)
    }

    /// Growing band **and** a caret in the dock slot: the caret is drawn outside
    /// the scissor even if it pokes above the band's top.
    fn scene_growing_band_with_caret() -> Scene {
        let (_, edge, mut frame) = scene_growing_band();
        frame.push_caret([1.0, 5.0], BACKGROUND, WHITE, 1.0, CaretShape::Block, true);
        assert!(
            frame.dock_caret(0.0).is_some(),
            "the scene did not put the caret in the dock slot"
        );
        ("growing band with dock caret (scissor lifted)", edge, frame)
    }

    /// A frame on the atlas's grid (gutter included).
    fn glyph_frame(m: CellMetrics) -> Frame {
        let mut frame = Frame::default();
        frame.clear(m, CaretStyle::default());
        frame
    }

    /// The four faces, a descender and a glyph on its own ground.
    fn scene_faces(m: CellMetrics) -> Scene {
        let mut frame = glyph_frame(m);
        for (col, bold, italic) in [(0, false, false), (1, true, false), (2, false, true)] {
            frame.push(Cell {
                bold,
                italic,
                ..glyph_cell(col, 0, 'M')
            });
        }
        frame.push(Cell {
            bold: true,
            italic: true,
            ..glyph_cell(3, 0, 'M')
        });
        frame.push(Cell {
            italic: true,
            ..glyph_cell(4, 0, 'g')
        });
        frame.push(Cell {
            bg: Some(MIDTONE),
            ..glyph_cell(5, 1, 'a')
        });
        ("four faces", 128, frame)
    }

    /// Every underline family, the strikeout and an SGR 58 colour — rules are
    /// sprites in the mask list, after the glyphs.
    fn scene_rules(m: CellMetrics) -> Scene {
        let mut frame = glyph_frame(m);
        let styles = [
            UnderlineStyle::Single,
            UnderlineStyle::Double,
            UnderlineStyle::Curl,
            UnderlineStyle::Dotted,
            UnderlineStyle::Dashed,
        ];
        for (col, underline) in (0u16..).zip(styles) {
            frame.push(Cell {
                underline,
                ..glyph_cell(col, 0, 'x')
            });
        }
        frame.push(Cell {
            strikeout: true,
            ..glyph_cell(0, 1, 'x')
        });
        frame.push(Cell {
            underline: UnderlineStyle::Curl,
            underline_color: Some(LinearRgba::from_srgb(0xff, 0x40, 0x40)),
            ..glyph_cell(1, 1, 'y')
        });
        ("underline families, strikeout and SGR 58", 128, frame)
    }

    /// Command marks: the block stripe is the chevron sprite in column 0,
    /// drawn through the `cell` pipeline before the grounds (phase-2 → this
    /// phase's checklist).
    fn scene_block_stripe(m: CellMetrics) -> Scene {
        let mut frame = glyph_frame(m);
        frame.push_block(Block {
            row: 0,
            stripe: Theme::BATERI.success_linear(),
        });
        frame.push_block(Block {
            row: 2,
            stripe: Theme::BATERI.error_linear(),
        });
        frame.push(glyph_cell(2, 0, 'l'));
        frame.push(glyph_cell(3, 0, 's'));
        ("block stripes (chevron sprite)", 128, frame)
    }

    /// Procedural characters: blocks, a shade, box drawing, a rounded corner,
    /// Braille and the terminal graphic set — no font involved.
    fn scene_procedural(m: CellMetrics) -> Scene {
        let mut frame = glyph_frame(m);
        for (col, ch) in (0u16..).zip(['█', '▒', '╬', '╭', '⠋', '⎿', '─']) {
            frame.push(glyph_cell(col, 0, ch));
        }
        ("procedural block and line characters", 128, frame)
    }

    /// Wide glyphs: two CJK characters drawn as two halves, and one declared
    /// wide whose ink fits one cell (one quad).
    fn scene_wide(m: CellMetrics) -> Scene {
        let mut frame = glyph_frame(m);
        for (col, ch) in [(0, '漢'), (2, '一'), (4, '☕')] {
            frame.push(Cell {
                wide: true,
                ..glyph_cell(col, 0, ch)
            });
        }
        ("wide CJK glyph halves", 128, frame)
    }

    /// Colour emoji: a single one and two clusters (a flag and a ZWJ family),
    /// on the colour plane.
    fn scene_emoji(m: CellMetrics) -> Scene {
        let mut frame = glyph_frame(m);
        let mut clusters = frame.take_clusters();
        let flag = clusters.push("🇹🇷");
        let family = clusters.push("👨‍👩‍👧");
        frame.put_clusters(clusters);
        frame.push(Cell {
            wide: true,
            ..glyph_cell(0, 0, '🎉')
        });
        frame.push(Cell {
            wide: true,
            cluster: flag,
            ..glyph_cell(2, 0, '🇹')
        });
        frame.push(Cell {
            wide: true,
            cluster: family,
            ..glyph_cell(4, 0, '👨')
        });
        // Text on the same row: the mask list is drawn after the colour one.
        frame.push(Cell {
            underline: UnderlineStyle::Single,
            ..glyph_cell(0, 1, 'e')
        });
        ("colour emoji and clusters", 128, frame)
    }

    /// Reverse video (fg/bg swapped at the boundary) and a glyph under the
    /// block caret, which takes the caret's text colour.
    fn scene_inverse_and_caret(m: CellMetrics) -> Scene {
        let mut frame = glyph_frame(m);
        frame.push(Cell {
            fg: BACKGROUND,
            bg: Some(WHITE),
            ..glyph_cell(0, 0, 'R')
        });
        frame.push(Cell {
            underline: UnderlineStyle::Single,
            ..glyph_cell(2, 0, 'M')
        });
        frame.push_caret([2.0, 0.0], BACKGROUND, ACCENT, 1.0, CaretShape::Block, true);
        (
            "reverse video and a glyph under the block caret",
            128,
            frame,
        )
    }

    /// The fill band's glyphs, rules and command mark, with a negative band
    /// origin, above grid glyphs — the third viewport.
    fn scene_fill_glyphs(m: CellMetrics) -> Scene {
        let mut frame = glyph_frame(m);
        frame.push(glyph_cell(0, 0, 'A'));
        frame.push(Cell {
            underline: UnderlineStyle::Curl,
            ..glyph_cell(1, 0, 'B')
        });
        frame.set_fill_rows(2);
        frame.push_fill(glyph_cell(1, 0, 'f'));
        frame.push_fill(Cell {
            strikeout: true,
            ..glyph_cell(2, 1, 'x')
        });
        frame.push_fill_block(Block {
            row: 1,
            stripe: Theme::BATERI.error_linear(),
        });
        frame.set_origin_rows(1.5);
        assert!(
            frame.fill_origin_px() < 0.0,
            "the band origin is not negative"
        );
        ("fill band glyphs, negative origin", 128, frame)
    }

    /// The dock: the prompt chevron, input glyphs with a rule, the caret
    /// inverting a dock glyph, and the context row in the small size class.
    fn scene_dock_glyphs(m: CellMetrics) -> Scene {
        const EDGE: u32 = 192;
        let mut frame = glyph_frame(m);
        frame.push(glyph_cell(0, 0, 'g'));
        frame.set_dock_input_rows(1);
        frame.set_dock_band(EDGE as f32, 0.0);
        frame.push_dock_sigil(Theme::BATERI.success_linear());
        for (col, ch) in (2u16..).zip("ls -la".chars()) {
            if ch != ' ' {
                frame.push_dock(glyph_cell(col, 0, ch));
            }
        }
        frame.push_dock(Cell {
            underline: UnderlineStyle::Single,
            ..glyph_cell(8, 0, 'u')
        });
        for (col, ch) in (0u16..).zip("~/src | main".chars()) {
            if ch != ' ' {
                frame.push_dock(glyph_cell(col, 1, ch));
            }
        }
        frame.open_dock(
            LinearRgba::from_srgb(0x20, 0x22, 0x28),
            WHITE,
            LinearRgba::from_srgb(0x60, 0x60, 0x60),
        );
        // The caret on the input row's `l` (column 2), in window rows.
        let row = (EDGE as f32 - frame.dock_layout_px()) / frame.cell_px()[1];
        frame.push_caret([2.0, row], BACKGROUND, ACCENT, 1.0, CaretShape::Block, true);
        assert!(
            frame.dock_caret(0.0).is_some(),
            "the scene did not put the caret in the dock slot"
        );
        (
            "dock: chevron, input glyphs, caret, small context row",
            EDGE,
            frame,
        )
    }

    /// A growing band with glyphs: the scissor clips the dock's glyphs, is
    /// lifted for the caret and comes back for the glyphs after it.
    fn scene_growing_band_glyphs(m: CellMetrics) -> Scene {
        const EDGE: u32 = 192;
        let mut frame = glyph_frame(m);
        frame.set_dock_rows(4);
        for row in 0..4 {
            frame.push_dock(glyph_cell(1, row, 'W'));
            frame.push_dock(Cell {
                strikeout: true,
                ..glyph_cell(2, row, 'k')
            });
        }
        frame.set_dock_band(EDGE as f32, 0.0);
        frame.open_dock(LinearRgba::from_srgb(0x00, 0x40, 0x00), WHITE, WHITE);
        let band_y = EDGE as f32 - frame.dock_band_px();
        let origin_y = EDGE as f32 - frame.dock_layout_px();
        assert!(band_y > origin_y, "the band does not clip the layout");
        frame.push_caret(
            [1.0, origin_y / frame.cell_px()[1]],
            BACKGROUND,
            ACCENT,
            1.0,
            CaretShape::Block,
            true,
        );
        ("growing band with glyphs (scissor restored)", EDGE, frame)
    }

    // **Phase-4 scenes**: selection and search (no atlas; a large synthetic
    // cell so the corner radius is several pixels) and the typing effects
    // (the atlas's own metrics).

    fn selection_run(row: u16, first: u16, last: u16) -> SelectionRun {
        SelectionRun { row, first, last }
    }

    fn search_run(row: u16, first: u16, last: u16, current: bool, continues: bool) -> SearchRun {
        SearchRun {
            row,
            first,
            last,
            current,
            continues,
        }
    }

    /// Selection corners: a lone run (convex), a step (concave fill) and a
    /// wider row below (aligned edge) — every branch of `selection_fragment`.
    fn scene_selection_corners() -> Scene {
        let mut frame = Frame::default();
        frame.clear(grid(40, 80), CaretStyle::default());
        frame.push(bg_cell(3, 2, ACCENT));
        frame.push_selection(
            &[
                selection_run(0, 2, 3),
                selection_run(1, 0, 3),
                selection_run(2, 1, 1),
            ],
            MIDTONE,
        );
        ("selection corners: convex, concave, step", 256, frame)
    }

    /// The unfocused window's selection: the dimmed colour of the same shape
    /// (`SelectionRuns::color(false)`).
    fn scene_unfocused_selection() -> Scene {
        let mut frame = Frame::default();
        frame.clear(grid(40, 80), CaretStyle::default());
        frame.push_selection(
            &[selection_run(0, 1, 3), selection_run(1, 0, 2)],
            Theme::BATERI.selection_unfocused_linear(),
        );
        ("unfocused (dimmed) selection", 256, frame)
    }

    /// Search: matches, the current match, a wrapped match (one shape), the
    /// selection over a match, and a match in the fill band.
    fn scene_search() -> Scene {
        let mut frame = Frame::default();
        frame.clear(grid(40, 80), CaretStyle::default());
        frame.set_fill_rows(1);
        frame.push_search(
            &[
                search_run(0, 0, 2, false, false),
                search_run(1, 3, 5, true, false),
                search_run(2, 0, 1, false, true),
            ],
            Theme::BATERI.search_match_linear(),
            Theme::BATERI.search_current_linear(),
        );
        frame.push_fill_search(&[search_run(0, 1, 2, false, false)]);
        frame.push_selection(&[selection_run(0, 2, 3)], MIDTONE);
        frame.set_origin_rows(1.0);
        ("search matches, current match, fill band", 256, frame)
    }

    /// A dock frame carrying effects: `statics` are the dock's static cells,
    /// `fx` the effects over them.
    fn fx_frame(m: CellMetrics, statics: &[Cell], fx: &[Fx]) -> Frame {
        let mut frame = glyph_frame(m);
        for &cell in statics {
            frame.push_dock(cell);
        }
        frame.set_dock_fx(
            fx.iter().copied(),
            &Clusters::default(),
            LinearRgba::from_srgb(0xff, 0x80, 0x20),
        );
        frame.set_dock_rows(1);
        frame.open_dock(BACKGROUND, BACKGROUND, BACKGROUND);
        frame
    }

    fn fx(cell: Cell, kind: Kind, effect: u32, t: f32) -> Fx {
        Fx {
            cell,
            kind,
            effect,
            t,
            seed: 3.0,
        }
    }

    /// Every arrival and every ghost at mid-flight — each branch of
    /// `glyph_fx_fragment` once — plus one of each at `t = 1`. A single-cell
    /// glyph, a wide glyph's two halves and an underlined glyph (the rule
    /// splits around arrivals).
    fn scenes_fx(m: CellMetrics) -> Vec<Scene> {
        let letter = glyph_cell(2, 0, 'M');
        let wide = Cell {
            wide: true,
            ..glyph_cell(5, 0, '漢')
        };
        let ruled = Cell {
            underline: UnderlineStyle::Single,
            ..glyph_cell(8, 0, 'g')
        };
        let mut out = Vec::new();
        let arrivals = Keypress::effects()
            .into_iter()
            .map(|e| (Kind::Arrival, e.id().expect("drawing effect"), "arrival"));
        let ghosts = Erase::effects()
            .into_iter()
            .map(|e| (Kind::Ghost, e.id().expect("drawing effect"), "ghost"));
        for (kind, id, what) in arrivals.chain(ghosts) {
            for t in [0.5, 1.0] {
                // `t = 1` once per kind is enough: the equality to the static
                // glyph is `renderer.rs`'s own guard.
                if t == 1.0 && id != 1 && id != 16 {
                    continue;
                }
                let effects: Vec<Fx> = [letter, wide, ruled]
                    .into_iter()
                    .map(|cell| fx(cell, kind, id, t))
                    .collect();
                let statics: &[Cell] = if kind == Kind::Arrival {
                    &[letter, wide, ruled]
                } else {
                    &[]
                };
                let name: &'static str =
                    Box::leak(format!("{what} {id} at t = {t}").into_boxed_str());
                out.push((name, 192, fx_frame(m, statics, &effects)));
            }
        }
        out
    }

    /// Three viewports in one frame: grid glyphs, search and selection over
    /// an offset grid, the fill band with its match, and a dock with its
    /// selection, caret and an arrival in flight.
    fn scene_three_viewports(m: CellMetrics) -> Scene {
        const EDGE: u32 = 256;
        let mut frame = glyph_frame(m);
        for (col, ch) in (0u16..).zip("echo".chars()) {
            frame.push(glyph_cell(col, 0, ch));
        }
        frame.push(Cell {
            bg: Some(MIDTONE),
            ..glyph_cell(1, 1, 'x')
        });
        frame.set_fill_rows(1);
        frame.push_fill(glyph_cell(0, 0, 'f'));
        frame.push_search(
            &[search_run(0, 0, 1, true, false)],
            Theme::BATERI.search_match_linear(),
            Theme::BATERI.search_current_linear(),
        );
        frame.push_fill_search(&[search_run(0, 0, 0, false, false)]);
        frame.push_selection(&[selection_run(1, 0, 2)], Theme::BATERI.selection_linear());
        frame.set_dock_input_rows(1);
        frame.set_dock_band(EDGE as f32, 0.0);
        frame.push_dock_sigil(Theme::BATERI.success_linear());
        let typed = glyph_cell(3, 0, 'k');
        for (col, ch) in (2u16..).zip("ls".chars()) {
            frame.push_dock(glyph_cell(col, 0, ch));
        }
        frame.push_dock(typed);
        frame.push_dock_selection(&[selection_run(0, 2, 3)]);
        frame.set_dock_fx(
            [fx(
                typed,
                Kind::Arrival,
                Keypress::Pop.id().expect("drawing effect"),
                0.4,
            )],
            &Clusters::default(),
            LinearRgba::from_srgb(0xff, 0x80, 0x20),
        );
        frame.open_dock(
            LinearRgba::from_srgb(0x20, 0x22, 0x28),
            WHITE,
            LinearRgba::from_srgb(0x60, 0x60, 0x60),
        );
        let row = (EDGE as f32 - frame.dock_layout_px()) / frame.cell_px()[1];
        frame.push_caret([4.0, row], BACKGROUND, ACCENT, 1.0, CaretShape::Beam, true);
        frame.set_origin_rows(1.5);
        (
            "three viewports: grid, fill band, dock with effects",
            EDGE,
            frame,
        )
    }

    fn scenes(m: CellMetrics) -> Vec<Scene> {
        let mut scenes = vec![
            scene_midtone(),
            scene_dock_ground(),
            scene_growing_band(),
            scene_growing_band_with_caret(),
            scene_fill_band(),
            caret_scene("block caret", CaretShape::Block, true),
            caret_scene("underline caret", CaretShape::Underline, true),
            caret_scene("beam caret", CaretShape::Beam, true),
            caret_scene("unfocused hollow block", CaretShape::Block, false),
            scene_glow(),
            scene_dock_button(),
            scene_faces(m),
            scene_rules(m),
            scene_block_stripe(m),
            scene_procedural(m),
            scene_wide(m),
            scene_emoji(m),
            scene_inverse_and_caret(m),
            scene_fill_glyphs(m),
            scene_dock_glyphs(m),
            scene_growing_band_glyphs(m),
            scene_selection_corners(),
            scene_unfocused_selection(),
            scene_search(),
            scene_three_viewports(m),
        ];
        scenes.extend(scenes_fx(m));
        scenes
    }

    /// The worst pixel where wgpu diverges from the oracle: the failure
    /// message's body.
    struct Divergence {
        diff: u8,
        at: (usize, usize),
        oracle: [u8; 4],
        seen: [u8; 4],
        flat: bool,
    }

    #[test]
    fn wgpu_matches_the_metal_oracle_on_every_scene() {
        // Flat fills **exact**, AA/SDF edges ≤ 1/255 per channel (Karar 4).
        // The oracle's own output decides which pixels are flat: a pixel whose
        // eight neighbours equal it is inside a region, where two compilations
        // of the same maths must not differ; a pixel with a differing
        // neighbour is an edge, where one LSB of rounding is allowed.
        let metal = MetalRenderer::system_default().expect("Metal device and pipelines");
        let w = Renderer::new();
        let m = metal.cell_metrics(SCALE);
        assert_eq!(
            m,
            w.cell_metrics(SCALE),
            "the two atlases disagree on the grid"
        );
        let scenes = scenes(m);
        assert!(!scenes.is_empty());
        for (name, edge, frame) in &scenes {
            let n = *edge as usize;
            let oracle = metal_offscreen(&metal, n, BACKGROUND, frame);
            let seen = w.render_offscreen(*edge, BACKGROUND, frame);
            assert_eq!(oracle.len(), seen.len(), "{name}: size");
            let px = |buf: &[u8], x: usize, y: usize| {
                let i = (y * n + x) * 4;
                [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
            };
            let flat = |x: usize, y: usize| {
                let own = px(&oracle, x, y);
                (x.saturating_sub(1)..=(x + 1).min(n - 1)).all(|nx| {
                    (y.saturating_sub(1)..=(y + 1).min(n - 1)).all(|ny| px(&oracle, nx, ny) == own)
                })
            };
            let mut worst: Option<Divergence> = None;
            for y in 0..n {
                for x in 0..n {
                    let (oracle_px, seen_px) = (px(&oracle, x, y), px(&seen, x, y));
                    let diff = (0..4)
                        .map(|c| oracle_px[c].abs_diff(seen_px[c]))
                        .max()
                        .unwrap_or(0);
                    let is_flat = flat(x, y);
                    let bad = if is_flat { diff > 0 } else { diff > 1 };
                    if bad && worst.as_ref().is_none_or(|w| diff > w.diff) {
                        worst = Some(Divergence {
                            diff,
                            at: (x, y),
                            oracle: oracle_px,
                            seen: seen_px,
                            flat: is_flat,
                        });
                    }
                }
            }
            if let Some(w) = worst {
                panic!(
                    "{name}: wgpu diverged from the oracle — largest difference {}/255 \
                     at {:?} {}, oracle BGRA {:?}, wgpu {:?}",
                    w.diff,
                    w.at,
                    if w.flat {
                        "in a flat fill"
                    } else {
                        "on an edge"
                    },
                    w.oracle,
                    w.seen,
                );
            }
        }
    }

    // **Measurement hook** (R2.4). Not part of `make hepsi`; `/measure` runs it
    // and reads the line (`.claude/is-akisi/olcum.md` → Türler). The line's keys
    // (`arka_uc=`, `kare=`, …) are the project's token contract and stay as
    // they are.

    /// The hook's frame: a full grid (a ground in every cell), a caret and a
    /// two-row dock — the heaviest frame this phase's two pipelines can draw.
    fn loaded_frame(frame: &mut Frame, edge: u16) {
        let tints = [MIDTONE, ACCENT, BACKGROUND, WHITE];
        frame.clear(grid_with_gutter(CELL.0, CELL.1, 8), CaretStyle::default());
        let (cols, rows) = ((edge - 8) / CELL.0, edge / CELL.1 - 3);
        for row in 0..rows {
            for col in 0..cols {
                frame.push(bg_cell(col, row, tints[usize::from((col + row) % 4)]));
            }
        }
        frame.set_dock_input_rows(1);
        frame.push_dock(bg_cell(2, 0, MIDTONE));
        frame.open_dock(BACKGROUND, WHITE, WHITE);
        frame.push_caret([3.0, 2.0], BACKGROUND, ACCENT, 1.0, CaretShape::Block, true);
    }

    fn micros(d: Duration) -> String {
        format!("{:.1}us", d.as_secs_f64() * 1e6)
    }

    /// A column's `p95`/`max`, by `Samples::p95_and_worst`'s rule.
    fn span(name: &str, samples: Samples) -> String {
        match samples.p95_and_worst() {
            Some((p95, worst)) => {
                format!(" {name}_p95={} {name}_max={}", micros(p95), micros(worst))
            }
            None => format!(" {name}_p95=insufficient {name}_max=insufficient"),
        }
    }

    fn report(backend: &str, frames: usize, stats: &Stats, supported: bool) -> String {
        let profile = if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        };
        let cpu = stats.cpu_frame();
        let gpu = stats.gpu();
        let gpu_line = if backend == "wgpu" && !supported {
            // No `TIMESTAMP_QUERY`: the key stays, the value says so (Karar 6).
            " gpu_p95=unsupported gpu_max=unsupported".to_owned()
        } else {
            span("gpu", gpu)
        };
        format!(
            "arka_uc={backend} profil={profile} kare={frames} ornek={}{}{}{gpu_line}",
            cpu.nanos.len(),
            span("cpu_kare", cpu),
            span("cpu_encode", stats.cpu_encode()),
        )
    }

    #[test]
    #[ignore = "measurement hook: run by /measure (olcum.md → Türler)"]
    fn offscreen_frame_loop_on_both_backends() {
        // The same frame, the same number of times, on both backends; the two
        // CPU spans of `Stats`: `cpu_kare` builds the frame (identical code on
        // both backends — the run's noise witness), `cpu_encode` is encode +
        // submit (the deciding column). The GPU is waited for, but **outside**
        // the spans, so frames do not queue behind each other.
        //
        // The backends **alternate frame by frame** rather than one after the
        // other: run sequentially, the identical `cpu_kare` differed by a
        // quarter between the halves (warm-up, clock state), i.e. ordering
        // produced a difference attributed to a backend. The first `WARMUP`
        // rounds are not recorded.
        const EDGE: u16 = 1024;
        const FRAMES: usize = 1000;
        const WARMUP: usize = 50;
        let clear = BACKGROUND;
        let mut frame = Frame::default();

        let metal = MetalRenderer::system_default().expect("Metal device and pipelines");
        let texture = target_texture(&metal, usize::from(EDGE));
        let w = Renderer::new();
        w.set_gpu_timing(true);
        let target = w.target(u32::from(EDGE));
        let metal_stats = Stats::new(Instant::now(), 10);
        let wgpu_stats = Stats::new(Instant::now(), 10);
        for i in 0..WARMUP + FRAMES {
            let t0 = Instant::now();
            loaded_frame(&mut frame, EDGE);
            let t1 = Instant::now();
            let cmd = commit_offscreen(&metal, &texture, clear, &frame);
            let t2 = Instant::now();
            cmd.waitUntilCompleted();
            if i >= WARMUP {
                metal_stats.record_cpu(t1 - t0, t2 - t1);
                metal_stats.record_gpu(cmd.GPUStartTime(), cmd.GPUEndTime());
            }

            let t0 = Instant::now();
            loaded_frame(&mut frame, EDGE);
            let t1 = Instant::now();
            // The product path: error scope, submit, tracking (`draw`).
            w.draw(&target, clear, &frame)
                .expect("the hook frame draws no glyphs");
            let t2 = Instant::now();
            w.device()
                .poll(wgpu::PollType::wait_indefinitely())
                .expect("waiting for the GPU failed");
            let record = i >= WARMUP;
            while w.poll(|result| {
                if let (true, Ok(Some(span))) = (record, result) {
                    wgpu_stats.record_gpu(span.start, span.end);
                }
            }) {
                w.device()
                    .poll(wgpu::PollType::wait_indefinitely())
                    .expect("waiting for the GPU failed");
            }
            if record {
                wgpu_stats.record_cpu(t1 - t0, t2 - t1);
            }
        }
        println!("{}", report("metal", FRAMES, &metal_stats, true));
        println!(
            "{}",
            report("wgpu", FRAMES, &wgpu_stats, w.gpu_timing_supported())
        );
    }
}
