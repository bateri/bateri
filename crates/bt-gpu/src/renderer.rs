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
//! round trip, and it was measured: a
//! per-frame buffer visibly inflated `cpu_encode`.
//!
//! **Completion** is a submission index per frame and a
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

use bt_atlas::{Atlas, Metrics, Plane, Spacing, TOFU};
use bt_core::{Clusters, FontOptions, LinearRgba};

use crate::GpuError;
use crate::frame::{
    CursorBlock, FX_INSTANCE_OFFSETS, Frame, FxCell, FxInstance, GLYPH_INSTANCE_OFFSETS, GlyphCell,
    GlyphInstance, INSTANCE_OFFSETS, Instance, RuleCell, WaveDraw,
};
use crate::metrics::{CellMetrics, FontNotice};
use crate::slots::{self, SlotUpload};

/// Immediate data budget in bytes: the smallest `maxPushConstantsSize` Vulkan
/// **guarantees**. macOS offers 4096, but a layout
/// that does not fit the smallest Linux driver would fail there; the device is
/// requested with exactly this limit, so an oversized pipeline is rejected on
/// macOS too.
pub(crate) const IMMEDIATE_BUDGET: u32 = 128;

/// Target format — **the single source**. `Srgb`: the fragment's output
/// counts as **linear** and the hardware encodes it on write, so alpha
/// blending runs in linear space (the glyphs' one reason for it). Its
/// counterpart is `bt_core::color::linear_rgba`; the two change together —
/// move one off linear without the other and the palette washes out to grey.
///
/// A `const`, not a field: a second target (offscreen, screenshot) given a
/// plain `Bgra8Unorm` would get **silently wrong colour**, not an error, so
/// "linear palette + non-sRGB target" must not be representable.
pub(crate) const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;

/// The one wgpu backend of the target (see [`Gpu::new`]).
#[cfg(target_os = "macos")]
const BACKENDS: wgpu::Backends = wgpu::Backends::METAL;
#[cfg(not(target_os = "macos"))]
const BACKENDS: wgpu::Backends = wgpu::Backends::VULKAN;

/// The atlas's mask plane: one channel of coverage, sampled by shaders only.
pub(crate) const MASK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;

/// The atlas's colour plane: the same slot grid, four bytes per pixel.
///
/// **It must be sRGB.** The target is sRGB and the hardware treats fragment
/// output as linear; an emoji sampled from a plain `Rgba8Unorm` texture would
/// take **undecoded** sRGB values for linear and wash the palette out — the
/// same silent defect as the linear palette on a non-sRGB target ([`FORMAT`]).
pub(crate) const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// Field-for-field twin of `cell_bg.wgsl` → `Immediates`.
///
/// In WGSL `vec4` aligns to 16, `vec2` to 8, and a struct's size rounds up to
/// its largest alignment: core@0, shape@16, viewport_px@32, edge_px@40, size
/// 48. The trailing pad is WGSL's invisible 4 bytes — without it Rust would
/// send 44 bytes and the layout would silently come up short. Putting the
/// `vec4`s first is deliberate: with `viewport_px` first the padding would
/// land in the middle. The `selection` pipeline reads the same block with
/// another meaning: `core` is the highlight's colour, `shape[0]` its radius.
/// The `wave` pipeline reads it as geometry: `core[0]` is the line's centre y,
/// `core[1]` the pixels per point, `shape` the peaks, the phase and the ring's
/// front (the colour rides in the instance); `core[2]` = 1 picks the dust
/// scene's ramp instead, which reads the block as `cell_bg.wgsl` says.
/// The `dots` pipeline reads none of it but `viewport_px`.
///
/// `edge_px` is the content's top fade ([`Op::Edge`]). **No `Default`**: the
/// one block the encode reuses across draws is built field by field, so a
/// field added here is a compile error there, not a silent zero.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
struct Immediates {
    core: [f32; 4],
    shape: [f32; 4],
    viewport_px: [f32; 2],
    edge_px: f32,
    pad: f32,
}

const _: () = assert!(size_of::<Immediates>() == 48);
const _: () = assert!(std::mem::offset_of!(Immediates, shape) == 16);
const _: () = assert!(std::mem::offset_of!(Immediates, viewport_px) == 32);
const _: () = assert!(std::mem::offset_of!(Immediates, edge_px) == 40);
// Chosen **per struct** and by size: 48 ≤ 128, so this block stays
// in immediates; no uniform-buffer fallback was needed.
const _: () = assert!(size_of::<Immediates>() as u32 <= IMMEDIATE_BUDGET);

/// Field-for-field twin of `cell.wgsl` → `Immediates`: the vertex stage's
/// `viewport_px`, the slot's geometry ([`SlotQuad`]), the viewport's `lift`
/// and the fragment's [`CursorBlock`] in one block.
///
/// `CursorBlock` is embedded **as is** (rect@0, rgba@16, its own asserts in
/// `frame.rs`), so there is no second copy of the cursor's layout. Then the
/// `vec2`s: viewport_px@32, slot_px@40, uv_size@48, slot_offset@56, then the
/// `f32`s lift@64 and edge_px@68 (the content's top fade, [`Op::Edge`]); WGSL
/// rounds 72 up to the struct's 16-byte alignment, and the trailing 8 bytes
/// are the explicit `pad`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct GlyphImmediates {
    cursor: CursorBlock,
    viewport_px: [f32; 2],
    slot_px: [f32; 2],
    uv_size: [f32; 2],
    slot_offset: [f32; 2],
    lift: f32,
    edge_px: f32,
    pad: [f32; 2],
}

const _: () = assert!(size_of::<GlyphImmediates>() == 80);
const _: () = assert!(std::mem::offset_of!(GlyphImmediates, viewport_px) == 32);
const _: () = assert!(std::mem::offset_of!(GlyphImmediates, slot_px) == 40);
const _: () = assert!(std::mem::offset_of!(GlyphImmediates, uv_size) == 48);
const _: () = assert!(std::mem::offset_of!(GlyphImmediates, slot_offset) == 56);
const _: () = assert!(std::mem::offset_of!(GlyphImmediates, lift) == 64);
const _: () = assert!(std::mem::offset_of!(GlyphImmediates, edge_px) == 68);
// 80 ≤ 128: this block stays in immediates too.
const _: () = assert!(size_of::<GlyphImmediates>() as u32 <= IMMEDIATE_BUDGET);

/// Field-for-field twin of `glyph_fx.wgsl` → `Immediates`: `heat` (the
/// `heat` effect's glowing colour, one per frame), the viewport, the grid
/// cell (the effects' amplitudes are its ratios) and the slot's geometry.
/// heat@0, viewport_px@16, cell_px@24, uv_size@32, slot_px@40,
/// slot_offset@48; WGSL rounds 56 up to 64 and the trailing 8 bytes are the
/// explicit `pad`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct FxImmediates {
    heat: [f32; 4],
    viewport_px: [f32; 2],
    cell_px: [f32; 2],
    uv_size: [f32; 2],
    slot_px: [f32; 2],
    slot_offset: [f32; 2],
    pad: [f32; 2],
}

const _: () = assert!(size_of::<FxImmediates>() == 64);
const _: () = assert!(std::mem::offset_of!(FxImmediates, viewport_px) == 16);
const _: () = assert!(std::mem::offset_of!(FxImmediates, cell_px) == 24);
const _: () = assert!(std::mem::offset_of!(FxImmediates, uv_size) == 32);
const _: () = assert!(std::mem::offset_of!(FxImmediates, slot_px) == 40);
const _: () = assert!(std::mem::offset_of!(FxImmediates, slot_offset) == 48);
// 64 ≤ 128: immediates.
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
/// No `pollster`: on native backends wgpu's futures are ready on
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
// asserted in `frame.rs`) followed by `f32` fields; WGSL's trailing padding is
// the explicit `pad`, size 80 is asserted.
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
    /// A glyph list's viewport ([`Renderer::glyph_draws`]): the surface's
    /// at `y`, raised by `lift` **and taller by it**, so the raised top does
    /// not pull the bottom edge up with it — a window without a dock would
    /// otherwise cut the ink of its bottom row. The list's `viewport_px`
    /// immediate carries the same taller height (the NDC scale's other half).
    Lifted { y: f32, lift: f32 },
    /// The content's top fade for the draws that follow, pixels from the
    /// window's top (`edge.wgsl`): every `cell_bg`, `caret`, `selection`,
    /// `cell` and `emoji` draw after it carries the value in its immediates
    /// until the next one. A **state**, like the viewport, because the
    /// fade's subject is a span of the plan — the grid and the fill band —
    /// not a pipeline: the scroll bar and the dock run the same pipelines
    /// without it ([`Renderer::plan`]).
    Edge(f32),
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
    /// A `wave_fragment` draw: one quad with its own `core`/`shape`
    /// ([`Plan::wave`]).
    Wave {
        range: Range<u32>,
        core: [f32; 4],
        shape: [f32; 4],
    },
    /// The `dots` pipeline over a range of the instance buffer: the dust's
    /// motes and the woven line's spark ([`Plan::dust`], [`Plan::dots`]).
    Dots(Range<u32>),
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
    /// band, [`Renderer::plan`] says why); `quad` is the atlas's slot
    /// geometry and `lift` how far the viewport was raised for this list
    /// ([`Renderer::glyph_draws`]).
    Glyphs {
        plane: Plane,
        range: Range<u32>,
        cursor: CursorBlock,
        quad: SlotQuad,
        lift: f32,
    },
    /// The `glyph_fx` pipeline over a range of the effect buffer: drawn in
    /// its own full-texture viewport (instances already carry `origin_y`),
    /// after which the dock's viewport at `origin_y` is restored
    /// ([`Renderer::fx_draw`] says why).
    Fx {
        range: Range<u32>,
        heat: [f32; 4],
        quad: SlotQuad,
        origin_y: f32,
    },
}

/// The atlas's slot geometry as the glyph shaders read it: the slot's
/// size, where the grid cell sits inside it and one slot's uv size. Taken
/// from the atlas's **slot** metric, not from `Frame::cell_px` — on the frame
/// between a scale change and the geometry event the two disagree and the uv
/// belongs to the atlas.
///
/// At `line_height, letter_spacing >= 1` the slot **is** the cell: the
/// offset is zero, the size is the grid cell and there is no overflow, i.e.
/// the quad is today's (`slot_quad_is_the_cell_at_or_above_one`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct SlotQuad {
    pub(crate) slot_px: [f32; 2],
    pub(crate) slot_offset: [f32; 2],
    pub(crate) uv_size: [f32; 2],
}

impl SlotQuad {
    pub(crate) fn of(atlas: &Atlas) -> Self {
        let (sw, sh) = atlas.slot_metrics().cell_px;
        let (ox, oy) = atlas.slot_offset();
        Self {
            slot_px: [f32::from(sw), f32::from(sh)],
            slot_offset: [f32::from(ox), f32::from(oy)],
            uv_size: uv_size(atlas),
        }
    }

    /// How far a glyph's quad reaches above its cell's top, in pixels — the
    /// lift a surface's glyph viewport needs so its top row is not clipped.
    pub(crate) fn overflow(self) -> f32 {
        self.slot_offset[1]
    }

    /// The `cell.wgsl` immediates of one glyph draw; `edge_px` is the
    /// plan's fade state at the draw ([`Op::Edge`]).
    fn glyph_immediates(
        self,
        cursor: CursorBlock,
        viewport_px: [f32; 2],
        lift: f32,
        edge_px: f32,
    ) -> GlyphImmediates {
        GlyphImmediates {
            cursor,
            viewport_px,
            slot_px: self.slot_px,
            uv_size: self.uv_size,
            slot_offset: self.slot_offset,
            lift,
            edge_px,
            pad: [0.0; 2],
        }
    }

    /// The `glyph_fx.wgsl` immediates of one effect draw.
    fn fx_immediates(
        self,
        heat: [f32; 4],
        viewport_px: [f32; 2],
        cell_px: [f32; 2],
    ) -> FxImmediates {
        FxImmediates {
            heat,
            viewport_px,
            cell_px,
            uv_size: self.uv_size,
            slot_px: self.slot_px,
            slot_offset: self.slot_offset,
            pad: [0.0; 2],
        }
    }
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
    /// The dust scene's motes and the woven line's spark for this plan, kept
    /// for their capacity ([`Renderer::plan`]).
    motes: Vec<Instance>,
    spark: Vec<Instance>,
}

/// The dust scene's motes once pushed, to be drawn twice — above the dock's
/// band under the grid's text, below it over the dock's ground
/// ([`Plan::dust`]).
struct DustField {
    motes: Range<u32>,
}

impl Plan {
    fn quads(&mut self, instances: &[Instance]) {
        if instances.is_empty() {
            return;
        }
        let range = self.push(instances);
        self.ops.push(Op::Quads(range));
    }

    fn wave(&mut self, draw: &WaveDraw) {
        let range = self.push(std::slice::from_ref(&draw.instance));
        self.ops.push(Op::Wave {
            range,
            core: draw.core,
            shape: draw.shape,
        });
    }

    /// A list of soft dots, one draw.
    fn dots(&mut self, instances: &[Instance]) {
        if instances.is_empty() {
            return;
        }
        let range = self.push(instances);
        self.ops.push(Op::Dots(range));
    }

    /// Pushes the dust scene's motes (the scratch list) once and returns them
    /// for [`Plan::dust`], or `None` when there is nothing to draw.
    fn dust_field(&mut self) -> Option<DustField> {
        let motes = std::mem::take(&mut self.motes);
        let range = (!motes.is_empty()).then(|| self.push(&motes));
        self.motes = motes;
        range.map(|motes| DustField { motes })
    }

    /// Draws the dust field inside `scissor`, in window space. The caller
    /// restores the viewport and the scissor.
    fn dust(&mut self, field: &DustField, scissor: [u32; 4]) {
        self.ops.push(Op::Scissor(scissor));
        self.ops.push(Op::Viewport(0.0));
        self.ops.push(Op::Dots(field.motes.clone()));
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

    /// Search highlights: two calls of the selection pipeline, one per
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
        self.motes.clear();
        self.spark.clear();
    }

    /// Moves the scratch list of `plane` into `glyphs` and records its draw.
    fn glyph_draw(&mut self, plane: Plane, cursor: CursorBlock, quad: SlotQuad, lift: f32) {
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
            quad,
            lift,
        });
    }
}

/// The texture's strip from `top_px` down to the bottom, as a scissor
/// (x, y, width, height) — the growing dock band. The scissor must stay
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
    /// `None` → not created yet, or `ensure` or `grow` dropped it. Created by
    /// the first frame that draws a glyph, with the resident tofu written once
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

    /// Drops both textures and the bind group made of them, for an atlas whose
    /// texture size changed ([`Atlas::ensure`], [`Atlas::grow`]): the next
    /// plan builds new ones at the new edge.
    fn drop_textures(&mut self) {
        self.mask = None;
        self.color = None;
        self.fx_bind = None;
    }
}

/// What the frame boundary does after planning a frame ([`room`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Room {
    /// The plan stands.
    Fits,
    /// Empty the atlas ([`Atlas::recycle`]) and plan the frame again.
    Recycle,
    /// Double the texture ([`Atlas::grow`]) and plan the frame again.
    Grow,
}

/// The frame boundary's decision: `overflowed` — the plan asked for a slot the
/// atlas no longer had ([`Atlas::take_overflow`]); `recycled` — this frame
/// already emptied the atlas once; `crowded` — the plan fills more than half
/// of it ([`Atlas::crowded`]).
///
/// A full atlas is emptied first: what filled it is mostly glyphs no longer
/// on screen, and the frame drawn again takes back only its own. Growing is
/// for the frame that does not fit an empty atlas, or fills more than half
/// of it — emptying alone would then come back every few frames, each time
/// rasterizing the whole screen again. A crowded atlas that never overflowed
/// is left alone: that is the slots of a long session, not this frame's.
fn room(overflowed: bool, recycled: bool, crowded: bool) -> Room {
    match (recycled, overflowed, crowded) {
        (false, false, _) | (true, false, false) => Room::Fits,
        (false, true, _) => Room::Recycle,
        (true, _, _) => Room::Grow,
    }
}

/// wgpu's [`SlotUpload`]: `Queue::write_texture` into the mask texture, or
/// into the colour texture, created here on the first colour slot (the
/// trait's doc says why at allocation time and not a frame earlier or later).
///
/// `write_texture` is staged by the queue and lands before the next `submit`'s
/// commands, i.e. before the frame that samples it — and after every frame
/// already submitted, because the queue keeps submission order. A recycled
/// atlas ([`Atlas::recycle`]) leans on that second half: it overwrites slots
/// that frames in flight still sample, and those frames read the old bytes.
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
    /// Blending is on and **has a customer**: backgrounds always have alpha
    /// `1.0` (`bt_core::LinearRgba`'s only constructor says so), so for them
    /// the result equals an opaque write, but the always-up scroll bar's
    /// track and hairline are translucent quads of this pipeline
    /// (`Frame::scrollbar_track`). Turning blending off would paint them as a
    /// solid bar of the foreground.
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
    /// The mouse selection and the search highlights: its own
    /// vertex reading [`Instance`] verbatim (`selection_vertex`) and a
    /// corner-masked fragment.
    ///
    /// Separate from `cell_bg` for the caret's reason (a round corner wants an
    /// SDF); its vertex is separate because the fragment must know its own
    /// quad — the caret takes its one quad from an immediate, the selection
    /// has one per instance. Blending softens the round corner's edge.
    selection: wgpu::RenderPipeline,
    /// The dock's wave line (`wave_fragment`): `cell_bg`'s vertex again, its
    /// own fragment — the curve's edge wants coverage computed per pixel,
    /// which a flat quad's fragment does not do. One quad per frame, and only
    /// while an arrival scene gives the top line a wave.
    wave: wgpu::RenderPipeline,
    /// The dust scene's soft dots (`dot_fragment`): its own vertex, since the
    /// quad is a little larger than the dot. At most 122 instances, only while
    /// a scene has dust.
    dots: wgpu::RenderPipeline,
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
    /// The dock's typing effects: its own vertex and its own instance
    /// ([`FxInstance`]).
    ///
    /// It cannot share `cell`'s vertex: the quad grows by the effect's margin
    /// and the fragment maps the point back into glyph space with the effect's
    /// inverse transform, so the instance must carry the effect's parameters
    /// — widening `GlyphInstance` would grow every glyph list's stride for a
    /// handful of animated glyphs. Emoji
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
    /// `unsupported`.
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

    #[cfg_attr(
        not(target_os = "macos"),
        expect(
            dead_code,
            reason = "the only surface entry, `Surface::from_layer`, is macOS-only until the winit set"
        )
    )]
    pub(crate) fn instance(&self) -> &wgpu::Instance {
        &self.instance
    }

    pub(crate) fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// A device on the platform's backend and six pipelines.
    ///
    /// The backend is **pinned per target**, not wgpu's `PRIMARY`: Metal on
    /// macOS (the product target; a wider mask would change which adapters
    /// are enumerated there), Vulkan on Linux — where `make linux` runs
    /// the pixel tests on lavapipe.
    pub(crate) fn new() -> Result<Self, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: BACKENDS,
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
        // (the asynchronous leg), not a panic in wgpu's default
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
            // The top edge's ramp is appended: one copy of the curve for both
            // modules, and naga's line numbers stay the host file's own.
            source: wgpu::ShaderSource::Wgsl(
                concat!(
                    include_str!("../shaders/cell_bg.wgsl"),
                    include_str!("../shaders/edge.wgsl")
                )
                .into(),
            ),
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
        let wave = pipeline(&device, quads, "cell_bg_vertex", "wave_fragment");
        let dots = pipeline(&device, quads, "dot_vertex", "dot_fragment");

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
            source: wgpu::ShaderSource::Wgsl(
                concat!(
                    include_str!("../shaders/cell.wgsl"),
                    include_str!("../shaders/edge.wgsl")
                )
                .into(),
            ),
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
            wave,
            dots,
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
    /// Frames the GPU finished **without error**; `make smoke`'s `frames=`.
    frames: Cell<u64>,
    /// The last **submitted** frame's background, glyph and rule counts
    /// (`cells=`, `glyphs=`, `rules=`); CPU counters.
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
    #[cfg_attr(
        not(target_os = "macos"),
        expect(
            dead_code,
            reason = "the only surface entry, `Surface::from_layer`, is macOS-only until the winit set"
        )
    )]
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
    /// scale), i.e. the cell geometry; drawing opens slots and may grow the
    /// texture ([`Renderer::plan_with_room`]) but never changes the cell. When
    /// `ensure` reports a rebuild, every texture drops in the
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
        let spacing = Spacing {
            line: font.line_height,
            letter: font.letter_spacing,
        };
        let entry = state.atlas.get_or_insert_with(|| WgpuAtlas {
            atlas: Atlas::new(family, font.size, scale, spacing),
            mask: None,
            color: None,
            fx_bind: None,
        });
        if entry.atlas.ensure(family, font.size, scale, spacing) {
            entry.drop_textures();
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
    /// atlas. `slots=`'s source.
    pub fn atlas_occupancy(&self) -> (usize, usize) {
        self.state
            .borrow()
            .atlas
            .as_ref()
            .map_or((0, 0), |a| a.atlas.occupancy())
    }

    /// The colour plane's slot occupancy; `slots2=`'s source.
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

    /// `false` → the GPU delta's token value is `unsupported`.
    pub fn gpu_timing_supported(&self) -> bool {
        self.gpu.timestamps
    }

    /// Whether a submitted frame is still waiting for [`Renderer::poll`]:
    /// a link going to sleep with one arms a single delayed poll ("the last
    /// frame before sleep is not lost"); an empty queue arms nothing — the
    /// stop condition.
    pub(crate) fn in_flight(&self) -> bool {
        !self.in_flight.borrow().is_empty()
    }

    /// Waits at most `timeout` for the newest submitted frame — the pending
    /// poll at shutdown, which must come before the report reads `frames=`.
    /// It only waits; counting is still [`Renderer::poll`]'s.
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
    /// The quad's position is the **frame's** cell (`GlyphCell::pos`, the
    /// background under it comes from the same cell), its size and uv size
    /// the **atlas's slot** ([`SlotQuad`]). They are born from the same
    /// scale; the only window where they differ is the one frame between a
    /// scale change and the geometry event, and there the glyph lands a
    /// little off, it does not break.
    ///
    /// **The overflow is not clipped**: below `1` a slot reaches
    /// [`SlotQuad::overflow`] pixels above its cell, and the surface's
    /// viewport starts at its top row — the top row's accents would be cut.
    /// So the lists are drawn in a viewport raised by `lift` (the overflow,
    /// at most `max_lift`: the dock may rise only to its band's top) above
    /// `origin_y` and taller by as much ([`Op::Lifted`]: the bottom edge stays
    /// put), the shader adds `lift` back and a glyph lands on the same window
    /// pixel. The viewport goes back to `origin_y` after the draw, so
    /// the caller's later lists are untouched. At `>= 1` the lift is zero and
    /// no viewport op is pushed: the plan is today's.
    #[allow(clippy::too_many_arguments)] // the surface's origin and lift cap are one viewport
    fn glyph_draws(
        &self,
        plan: &mut Plan,
        atlas: &mut Option<WgpuAtlas>,
        glyphs: &[GlyphCell],
        clusters: &Clusters,
        rules: &[RuleCell],
        cursor: CursorBlock,
        origin_y: f32,
        max_lift: f32,
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
        let quad = SlotQuad::of(atlas);
        let lift = quad.overflow().min(max_lift).max(0.0);
        // Nothing to draw (the colour list counts only with its texture),
        // nothing to lift.
        let drawn = !plan.mask.is_empty() || (color.is_some() && !plan.color.is_empty());
        let lifted = drawn && lift > 0.0;
        if lifted {
            plan.ops.push(Op::Lifted { y: origin_y, lift });
        }
        // The colour list can only be non-empty once its texture exists (the
        // upload created it); the check keeps a missing texture a skipped
        // draw rather than the mask texture read as colour.
        if color.is_some() {
            plan.glyph_draw(Plane::Color, cursor, quad, lift);
        }
        plan.glyph_draw(Plane::Mask, cursor, quad, lift);
        if lifted {
            plan.ops.push(Op::Viewport(origin_y));
        }
        Ok(())
    }

    /// The dock's typing effects: the `glyph_fx` pipeline, both textures
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
    /// **In its own viewport, not the dock's**: the dock's glyph viewport
    /// starts no higher than the band's top and clips above it (the static
    /// glyphs must stay inside their band), while `drop` falls from
    /// above the cell, `sublime` floats up and there is only a thin breathing
    /// margin above the input row — the first frames showed half-cut letters.
    /// The effect is drawn in window space (positions moved down by
    /// `origin_y`) and may spill over the hairline by its pad (`glyph_fx.wgsl`
    /// → `FX_PAD`); the dock's viewport at `origin_y` is restored after the
    /// draw. At `t = 1` an arrival equals its static glyph as long as the
    /// glyph's overflow fits the breathing margin above the input row.
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
            quad: SlotQuad::of(atlas),
            origin_y,
        });
        Ok(())
    }

    /// [`Renderer::plan`], again while the atlas has no room for the frame —
    /// **the frame boundary** where the atlas may be emptied or grown.
    ///
    /// The atlas never lets a slot go on its own, so a pane fills it over a
    /// long session (a multilingual one in hours) and from then on every new
    /// glyph would be a box. When a plan reports that ([`Atlas::take_overflow`])
    /// the atlas is emptied and the frame planned again: only the glyphs of
    /// this frame come back. A frame that does not fit an empty atlas, or
    /// fills more than half of it, grows the texture and is planned again
    /// ([`room`] has the rule). Here and not in the middle of a plan, because
    /// a plan's lists carry uvs baked as they resolve: a slot reused after an
    /// earlier list resolved would draw another glyph there. Nothing outside
    /// the plan holds a slot number, so a whole plan is the unit.
    ///
    /// Bounded: one recycle, then each grow at least doubles the edge up to
    /// the ceiling. **Known limit**: a frame that still overflows at the
    /// ceiling keeps its boxes and pays two plans each frame it stays on
    /// screen — only past the slot numbers' `u16` clamp with every cell a
    /// different glyph.
    fn plan_with_room(
        &self,
        frame: &Frame,
        viewport_px: [f32; 2],
        atlas: &mut Option<WgpuAtlas>,
        plan: &mut Plan,
    ) -> Result<(), GpuError> {
        self.plan(frame, viewport_px, atlas, plan)?;
        let mut recycled = false;
        while let Some(entry) = atlas.as_mut() {
            match room(entry.atlas.take_overflow(), recycled, entry.atlas.crowded()) {
                Room::Fits => break,
                Room::Recycle => {
                    entry.atlas.recycle();
                    recycled = true;
                }
                Room::Grow => {
                    if !entry.atlas.grow() {
                        break;
                    }
                    entry.drop_textures();
                }
            }
            self.plan(frame, viewport_px, atlas, plan)?;
        }
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
    /// `#[repr(C)]` layouts. The order is the draw order: command
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
    /// - **Search after the ground, before the selection**:
    ///   every match, the current match over them, the user's selection on top
    ///   — when Esc turns the current match into the selection it stays
    ///   visible. Text is over all of them in its own colour.
    /// - **Selection after the ground, before the caret and glyphs**:
    ///   text reads over the selection in its own colour and the caret stays
    ///   on top — reverse video's "the cursor wins" rule, as pixel order.
    /// - **Caret after the grounds, before the glyphs**, for the **solid**
    ///   caret: the block is opaque and the letter under it is drawn over it,
    ///   in the colour `cursor_block` inverts. For a hollow caret both halves
    ///   of the reason fall away (no fill, degenerate `CursorBlock`) and the
    ///   cost is recorded: a glyph with ink at the cell's edge is drawn over
    ///   the ring. This slot is filled
    ///   while the caret is in the grid; once it enters the dock band the list
    ///   is empty and the instance is in the dock's list (`Frame::push_caret`).
    ///
    /// **Fill band** — the third coordinate space, above the grid, with its
    /// own viewport: the twin of the dock's reason in the other direction. The
    /// band is not exempt from the offset but sits **on top of** it (origin
    /// `origin_px − fill_px`, [`Frame::fill_origin_px`]). Its rows are
    /// fill-local and which screen row they land on is known only here, **at
    /// encode time** — baked at push time, a motion frame (lists kept, only
    /// `origin_px` changes) would freeze the band in place. The
    /// origin **may go negative** and is left so: the band's oldest rows that
    /// do not fit spill over the top and are clipped (measured).
    /// A band of zero rows sets no viewport and the frame is bit-identical to
    /// the one without it; a window without a dock gets rows only while its
    /// grid is full — the slide's strip and the top fade, never its blank
    /// (`Session::fill_rows`, `Session::slide_fill_rows`). Search after the
    /// band's ground,
    /// before its letters; no selection in the band. Its
    /// inversion rectangle is **degenerate**: the band has no caret slot and
    /// in a settled frame the caret's screen row is always inside the
    /// content; passing the real one would paint the letter under a caret
    /// sliding over the band in the ground colour — an unreadable cell for a
    /// caret that is not drawn. **Interleaved with the grid**: the
    /// band and the grid are two parts of the same history and the seam
    /// between them must not cut a letter, so the order is grid ground →
    /// search → selection → caret → **band ground** → band search → **grid
    /// glyphs** → **band glyphs**. Below `1` a glyph's slot reaches past its
    /// cell, so the grid's top row's accent rises into the band and the
    /// band's bottom row's tail falls into the grid; each lands on the other
    /// surface's ground because every ground is drawn first. The
    /// offset-exempt caret is the one grid list that enters the band and it
    /// stays **under** the band's ground (`caret_stays_under_the_fill_band`).
    /// The glyph lists are drawn in viewports raised by the slot's overflow
    /// ([`Renderer::glyph_draws`]), or the top row's accent would be clipped
    /// at the surface's origin. The dock's opaque ground is still drawn last
    /// and covers whatever spills into it.
    ///
    /// **Top edge** — the grid's and the band's draws fade towards the clear
    /// colour inside [`Frame::edge_px`] (`edge.wgsl`) — the leftover the rows
    /// leave at the pane's top, which the content frame writes and the grid's
    /// origin already stands under (`Frame::origin_px`) — set once before the
    /// grid's first list ([`Op::Edge`]) and reset to zero after the band's
    /// last, **unconditionally**: the scroll bar belongs to the window's edge
    /// and the dock is its own panel, so neither fades — the dock climbing to
    /// the top of a short window, the thumb at the top of its travel. The
    /// state reaches all three immediates set-ups (the shared block, the
    /// selection's own, the glyphs'). At zero the ramp's factor is exactly
    /// `1.0` and the frame is bit for bit the one without the fade.
    ///
    /// **Scroll bar** — between the band and the dock, in a viewport at the
    /// window's origin: the thumb belongs to the window's edge and must not
    /// slide with the grid's offset or the band's. After every grid and band
    /// list, so text never covers it; before the dock, so the dock's opaque
    /// ground covers a track whose end has not yet followed a growing band.
    /// One rounded quad from `caret_fragment` (the dock buttons' road) over,
    /// in the wide form, two square ones for the track and its hairline
    /// (`cell_bg`), then its marks over the thumb — the matches', then the
    /// current match's, one `selection` draw per colour — and no op at all
    /// while the bar is hidden.
    ///
    /// **Dock** — the second coordinate space, **last**. Its own viewport is
    /// structural: the dock must be exempt from the offset and building the
    /// exemption arithmetically (`- origin_px`) **does not work** —
    /// `Frame::clear` resets the offset and `set_origin_rows` runs after the
    /// sink, so the value is unknown when dock cells are pushed. The origin
    /// sits at the texture's **bottom** (`height − dock`); the leftover strip
    /// (pixels not divisible by the cell) stays between dock and content where
    /// the content is cut at the top, and is the top fade above the grid where
    /// it fades — then the grid sits on the dock. Last because during a slide
    /// the grid's offset target
    /// is overshot and its bottom row spills over the dock; the dock's opaque
    /// ground covers it. The origin is **clamped at zero**: in a window
    /// shorter than the dock the right answer is degenerate (the dock covers
    /// the whole window); left negative, the dock would climb into the grid's
    /// area. A window without a dock sets no second viewport.
    ///
    /// **Two origins**: the ground and hairlines use the **drawn band's**
    /// viewport (`height − band`, the animation's current value); cells, caret
    /// and effects the **layout's** (`height − layout`). Cells are baked at
    /// push time and a motion frame does not re-push them; the layout is
    /// bottom-aligned, so while the band grows and shrinks the text stays put
    /// and only the band's top moves. At rest the two are the same number.
    /// The dock's glyph viewport may rise above the layout's top only up to
    /// the band's top: an accent spilling into the breathing
    /// margin shows, one spilling past the band is cut — the dock is its own
    /// panel, no scissor needed.
    /// Ground and separator first: the dock's own backgrounds (highlight
    /// ranges, caret) must come over them. **A growing band clips**: if the
    /// layout spills over the band's current top (the band has not risen yet),
    /// the spilling input rows would be drawn groundless over the grid's
    /// bottom rows. The scissor exists only in those frames: at rest it would
    /// cut the effects' margin that spills over the hairline.
    ///
    /// In the dock: the selection in the grid's order; the upload
    /// row's buttons over the ground and selection, under their
    /// labels. The caret's dock slot is after the opaque ground (or the ground
    /// would cover it) and before the glyphs (or it would paint over the
    /// letter); the instance is born in window space, the viewport is
    /// dock-local, so the difference is given back here. On hand-over frames
    /// the caret moves into the dock band and, this list being last, stays on
    /// top of everything. **The caret is outside the scissor**: at a hand-over
    /// or on a new input row a caret touching the band's top would be a
    /// half-cut block; a whole block showing for one frame above the band is
    /// better than a cut one. **Ghosts before the dock's glyphs**: the
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
        // **The dock's drawn top is decided first** (the one place it is
        // worked out), and with it the dust scene's field: that is drawn twice,
        // in two places of the plan, from one list. Both places are in window
        // space and the dock's band is where they meet.
        let dock_top = frame
            .dock()
            .is_some()
            .then(|| frame.band_top_px(viewport_px[1]) + frame.dock_rise_px());
        let dust = dock_top.and_then(|band_y| {
            let mut motes = std::mem::take(&mut plan.motes);
            frame.dock_dust(band_y, viewport_px[0], &mut motes);
            plan.motes = motes;
            plan.dust_field()
        });
        // Grid: the offset lives in one viewport. Command marks first (sprites,
        // degenerate inversion rectangle), then ground → search → selection →
        // caret; the grid's glyphs wait for the band's ground.
        let origin = frame.origin_px();
        let fill_origin = frame.fill_origin_px();
        let free = f32::INFINITY;
        // The padding first, in window space and unfaded
        // ([`Frame::padding`]): everything else is drawn over it.
        let padding = frame.padding(viewport_px);
        if !padding.is_empty() {
            plan.ops.push(Op::Viewport(0.0));
            plan.ops.push(Op::Edge(0.0));
            plan.quads(&padding);
        }
        plan.ops.push(Op::Viewport(origin));
        plan.ops.push(Op::Edge(frame.edge_px()));
        self.glyph_draws(
            plan,
            atlas,
            &[],
            frame.clusters(),
            frame.stripes(),
            CursorBlock::default(),
            origin,
            free,
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
        // Fill band: the third coordinate space, above the grid; search but no
        // selection, no caret slot, so the inversion rectangle is degenerate.
        // Its ground comes after the grid's caret (the caret stays under it)
        // and before both surfaces' glyphs (ink spilling across the seam stays
        // on top of the other surface's ground).
        let band = frame.fill_rows() != 0;
        if band {
            plan.ops.push(Op::Viewport(fill_origin));
            plan.quads(frame.fill_bg());
            plan.search(frame, true);
            plan.ops.push(Op::Viewport(origin));
        }
        // **The dust above the dock's band: over the grid's ground, under its
        // text** — a restored pane's old lines stay in front of the motes.
        // Its other half, inside the band, waits for the dock's ground. The
        // two scissors share the band's top, so each pixel is drawn once.
        if let (Some(field), Some(band_y)) = (&dust, dock_top) {
            let above = band_y.round().max(0.0) as u32;
            if above >= 1 {
                let width = viewport_px[0].max(0.0) as u32;
                plan.dust(field, [0, 0, width, above.min(viewport_px[1] as u32)]);
                plan.ops.push(Op::Scissor(scissor_below(0.0, viewport_px)));
                plan.ops.push(Op::Viewport(origin));
            }
        }
        self.glyph_draws(
            plan,
            atlas,
            frame.glyphs(),
            frame.clusters(),
            frame.rules(),
            *frame.cursor_block(),
            origin,
            free,
        )?;
        if band {
            plan.ops.push(Op::Viewport(fill_origin));
            self.glyph_draws(
                plan,
                atlas,
                frame.fill_glyphs(),
                frame.clusters(),
                frame.fill_rules(),
                CursorBlock::default(),
                fill_origin,
                free,
            )?;
        }
        // The content ends here: the bar and the dock do not fade. Pushed
        // unconditionally, so the dock's exemption does not hang on the bar
        // being shown.
        plan.ops.push(Op::Edge(0.0));
        // The scroll bar: window space, over the grid and the band and under
        // the dock — the dock's opaque ground, drawn next, covers a track
        // that has not caught up with a growing band.
        if let Some(thumb) = frame.scrollbar() {
            plan.ops.push(Op::Viewport(0.0));
            plan.quads(frame.scrollbar_track());
            plan.rounded(
                std::slice::from_ref(&thumb.instance),
                thumb.core,
                thumb.shape,
            );
            for (marks, color) in frame.scrollbar_block_marks() {
                plan.selection(marks, color, frame.scrollbar_mark_radius());
            }
            for (marks, color) in frame.scrollbar_marks() {
                plan.selection(marks, color, frame.scrollbar_mark_radius());
            }
            plan.ops.push(Op::Viewport(origin));
        }
        // Dock: last, with two origins.
        if let Some(band_y) = dock_top {
            // **The arrival scene's climb is a lever on the viewports**: the
            // band's, the cells' and the scissor all move by `rise`, and every
            // window-space number that is compared with a fragment position
            // (the caret's core) moves with them. The cells and rules need no
            // other change — they are dock-local. The caret's instance is
            // born in window space and converted with the layout's own origin;
            // the viewport it is drawn in is the lowered one.
            let rise = frame.dock_rise_px();
            let layout_y = (viewport_px[1] - frame.dock_layout_px()).max(0.0);
            let origin_y = layout_y + rise;
            plan.ops.push(Op::Viewport(band_y));
            plan.quads(&frame.dock_ground(viewport_px[0]));
            // **The wave over the ground, in window space**: it leaves its row
            // above the band's top, which the band's viewport would clip. The
            // dock's own top line is held back meanwhile (the scene gives it
            // no width) and comes back when the scene ends.
            if let Some(wave) = frame.dock_wave(band_y, viewport_px[0]) {
                plan.ops.push(Op::Viewport(0.0));
                plan.wave(&wave);
            }
            // **The dust inside the band, over the dock's ground** (its ground
            // is opaque from the arrival on, and the motes still on their way
            // to the line would vanish under it), then the lit tip of the line
            // they weave, over the ground and the line.
            if let Some(field) = &dust {
                plan.dust(field, scissor_below(band_y, viewport_px));
                plan.ops.push(Op::Scissor(scissor_below(0.0, viewport_px)));
            }
            let mut spark = std::mem::take(&mut plan.spark);
            let tip = frame.dock_weld_tip(band_y, viewport_px[0], &mut spark);
            if tip.is_some() || !spark.is_empty() {
                plan.ops.push(Op::Viewport(0.0));
                if let Some(tip) = &tip {
                    plan.wave(tip);
                }
                plan.dots(&spark);
            }
            plan.spark = spark;
            plan.ops.push(Op::Viewport(origin_y));
            let clipped = band_y > origin_y;
            // The dock's glyphs may rise above the layout's top only up to the
            // band's: the dock is a separate panel and its ink must
            // not leave it. In a clipped frame the band's top is below the
            // layout's and the scissor does the clipping.
            let dock_lift = (origin_y - band_y).max(0.0);
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
            let [x0, y0, x1, y1] = frame.caret_core();
            plan.rounded(
                frame.dock_caret(layout_y).as_slice(),
                [x0, y0 + rise, x1, y1 + rise],
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
                origin_y,
                dock_lift,
            )?;
            self.fx_draw(plan, atlas, arrivals, frame.dock_clusters(), heat, origin_y)?;
            self.glyph_draws(
                plan,
                atlas,
                &[],
                frame.dock_clusters(),
                late_rules,
                *frame.cursor_block(),
                origin_y,
                dock_lift,
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
        self.plan_with_room(frame, viewport_px, &mut state.atlas, plan)?;
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
            // Every field spelled out (no `..Default`): the block lives across
            // draws and carries the fade state, so a field added to it must be
            // decided here.
            let mut imm = Immediates {
                core: [0.0; 4],
                shape: [0.0; 4],
                viewport_px,
                edge_px: 0.0,
                pad: 0.0,
            };
            let cell_px = frame.cell_px();
            for op in &plan.ops {
                match op {
                    Op::Viewport(y) => {
                        pass.set_viewport(0.0, *y, viewport_px[0], viewport_px[1], 0.0, 1.0);
                    }
                    Op::Edge(px) => imm.edge_px = *px,
                    Op::Lifted { y, lift } => {
                        let h = viewport_px[1] + lift;
                        pass.set_viewport(0.0, y - lift, viewport_px[0], h, 0.0, 1.0);
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
                    Op::Dots(range) => {
                        let Some(buffer) = state.quads.as_ref() else {
                            continue;
                        };
                        pass.set_pipeline(&self.gpu.dots);
                        pass.set_vertex_buffer(0, buffer.slice(..));
                        pass.set_immediates(0, bytes_of(std::slice::from_ref(&imm)));
                        pass.draw(0..4, range.clone());
                    }
                    Op::Wave { range, core, shape } => {
                        let Some(buffer) = state.quads.as_ref() else {
                            continue;
                        };
                        imm.core = *core;
                        imm.shape = *shape;
                        pass.set_pipeline(&self.gpu.wave);
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
                            edge_px: imm.edge_px,
                            pad: 0.0,
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
                        quad,
                        lift,
                    } => {
                        let atlas = state.atlas.as_ref();
                        let (pipeline, texture) = match plane {
                            Plane::Mask => (&self.gpu.cell, atlas.and_then(|a| a.mask.as_ref())),
                            Plane::Color => (&self.gpu.emoji, atlas.and_then(|a| a.color.as_ref())),
                        };
                        let (Some(texture), Some(buffer)) = (texture, state.glyphs.as_ref()) else {
                            continue;
                        };
                        // A lifted list's viewport is taller by the lift
                        // (`Op::Lifted`); the NDC scale must match it.
                        let tall = [viewport_px[0], viewport_px[1] + lift];
                        let glyph_imm = quad.glyph_immediates(*cursor, tall, *lift, imm.edge_px);
                        pass.set_pipeline(pipeline);
                        pass.set_bind_group(0, &texture.bind, &[]);
                        pass.set_vertex_buffer(0, buffer.slice(..));
                        pass.set_immediates(0, bytes_of(std::slice::from_ref(&glyph_imm)));
                        pass.draw(0..4, range.clone());
                    }
                    Op::Fx {
                        range,
                        heat,
                        quad,
                        origin_y,
                    } => {
                        let (Some(bind), Some(buffer)) = (fx_bind.as_ref(), state.fx.as_ref())
                        else {
                            continue;
                        };
                        let fx_imm = quad.fx_immediates(*heat, viewport_px, cell_px);
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

    /// The frame path: encode and submit inside an error scope,
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
        // could not be submitted does not pollute `cells=`.
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

    /// The non-blocking poll at the start of a tick: hands every
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
    /// **`startup=`** is closed by the caller at the first `Ok` — the moment
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
            atlas.slot_metrics(),
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

/// The atlas's uv size of one slot — the **slot** metric's, which
/// below `1` is larger than the grid cell.
fn uv_size(atlas: &Atlas) -> [f32; 2] {
    let (cw, ch) = atlas.slot_metrics().cell_px;
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
/// The blend is **not a parameter**: all eight pipelines want it, each for its
/// own reason — `cell` makes alpha from the atlas's coverage, `caret` has a
/// translucent halo, `glyph_fx`'s effect is itself transparency, `selection`
/// softens its round corner, `wave` softens the curve's edge, `dots` the
/// round dot's. All eight output
/// **straight** alpha, emoji included: CoreGraphics writes colour glyphs premultiplied, but
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
