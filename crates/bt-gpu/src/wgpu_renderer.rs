//! The wgpu renderer — 040's parallel renderer, today **test-only**.
//!
//! The Metal renderer ([`crate::Renderer`]) stays in place as the oracle and
//! this module grows next to it one pipeline group at a time
//! (`.tasks/040-linux-kapisi-ve-wgpu/discussion.md` → Karar 3 and 4). Phase-2
//! brought `cell_bg` + caret (background quads, the caret's SDF and the dock
//! buttons); phase-3 brings `cell` + `emoji`: glyphs and rules from the
//! atlas's two planes, on all three surfaces (grid, fill band, dock), plus
//! the command marks drawn as sprites. Selection, search and effect lists do
//! not exist yet and [`WgpuRenderer::plan`] requires them to be **empty** — a
//! half-drawn scene must not silently diverge from the oracle.
//!
//! The module sits behind `cfg(test)` (wgpu is a dev-dependency): the product
//! binary's graph does not change and backing out is a single `git revert`.
//! Pipeline order, the viewport/scissor sequence and blending are **the same**
//! as `renderer.rs`'s `encode_pass` / `encode_fill` / `encode_dock` /
//! `encode_glyphs` / `pipeline`; the reasons live there and are not repeated
//! here. Slot resolution, the wide glyph's fan-out and uv baking are not
//! repeated either: both renderers call [`crate::slots`], and only the upload
//! target differs ([`WgpuUpload`]). The one difference is how commands are
//! recorded: Metal builds a buffer per list per frame, here every quad of the
//! frame goes into **one** instance buffer (and every glyph into one glyph
//! buffer) and draws read ranges of them ([`Plan`]). Those buffers are not
//! rebuilt per frame either: they live as long as the renderer, grow on demand
//! and are filled with `write_buffer` — creating a buffer in wgpu is a
//! validation and tracking round trip, and it was measured (phase-2 →
//! Uygulama Notları): a per-frame buffer visibly inflated `cpu_encode`.

use std::cell::RefCell;
use std::ops::Range;
use std::sync::OnceLock;
use std::task::{Context, Poll, Waker};

use bt_atlas::{Atlas, Metrics, Plane, TOFU};
use bt_core::{Clusters, FontOptions, LinearRgba};

use crate::GpuError;
use crate::frame::{
    CursorBlock, Frame, GLYPH_INSTANCE_OFFSETS, GlyphCell, GlyphInstance, INSTANCE_OFFSETS,
    Instance, RuleCell,
};
use crate::renderer::{CellMetrics, scissor_rect_below};
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
const MASK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;

/// The atlas's colour plane: **sRGB**, so sampling decodes to linear (Metal's
/// `RGBA8Unorm_sRGB`; why it must be sRGB is `new_color_texture`'s doc).
const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// Field-for-field twin of `cell_bg.wgsl` → `Immediates`.
///
/// In WGSL `vec4` aligns to 16, `vec2` to 8, and a struct's size rounds up to
/// its largest alignment: core@0, shape@16, viewport_px@32, size 48. The
/// trailing pad is WGSL's invisible 8 bytes — without it Rust would send 40
/// bytes and the layout would silently come up short. Putting the `vec4`s
/// first is deliberate: with `viewport_px` first the padding would land in
/// the middle.
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
// SAFETY: `repr(C)`, `f32` fields only; WGSL's trailing padding is an
// explicit field (`pad`), size 48 is asserted.
unsafe impl GpuBytes for Immediates {}
// SAFETY: `repr(C)`: a `CursorBlock` (`repr(C)`, two `[f32; 4]`, size 32
// asserted in `frame.rs`) followed by `f32` pairs; WGSL's trailing padding is
// the explicit `pad`, size 64 is asserted.
unsafe impl GpuBytes for GlyphImmediates {}

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
}

/// The frame's draw plan: two instance buffers and the steps reading ranges
/// of them. [`WgpuRenderer::plan`] builds it from a `Frame`,
/// [`WgpuRenderer::submit`] replays it into a pass.
#[derive(Default)]
struct Plan {
    instances: Vec<Instance>,
    glyphs: Vec<GlyphInstance>,
    ops: Vec<Op>,
    /// Scratch lists for [`slots::glyph_lists`]; each call's output is
    /// appended to `glyphs` as ranges.
    mask: Vec<GlyphInstance>,
    color: Vec<GlyphInstance>,
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

    fn push(&mut self, instances: &[Instance]) -> Range<u32> {
        let start = self.instances.len() as u32;
        self.instances.extend_from_slice(instances);
        start..self.instances.len() as u32
    }

    /// Empties every list, keeping the capacity.
    fn clear(&mut self) {
        self.instances.clear();
        self.glyphs.clear();
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
/// frame.
pub(crate) struct Target {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

/// One atlas plane on the GPU: its texture and the bind group that samples it.
/// Created together, so a texture is never drawn without its bind group.
struct PlaneTexture {
    texture: wgpu::Texture,
    bind: wgpu::BindGroup,
}

/// The atlas and its two textures — **in one place**, like `renderer.rs`'s
/// `AtlasTexture`: when [`Atlas::ensure`] rebuilds the atlas both textures
/// are dropped in the same line ([`WgpuRenderer::cell_metrics`]).
struct WgpuAtlas {
    atlas: Atlas,
    /// `None` → not created yet, or `ensure` dropped it. Created by the first
    /// frame that draws a glyph, with the resident tofu written once.
    mask: Option<PlaneTexture>,
    /// `None` → no emoji seen yet. **Lazy**, created by the first colour
    /// upload ([`WgpuUpload`]); a session without emoji never pays for it.
    color: Option<PlaneTexture>,
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

/// A renderer's own state: its two instance buffers and its atlas.
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
    /// `None` until [`WgpuRenderer::cell_metrics`] is asked, like Metal's
    /// `Renderer::atlas`: the atlas key's scale comes from the window, and a
    /// frame with glyphs but no atlas fails with [`GpuError::NoAtlas`] rather
    /// than drawing @1x.
    atlas: Option<WgpuAtlas>,
    /// The frame's plan, kept between frames: `clear` keeps the capacity of
    /// its lists (and of `slots::glyph_lists`' scratch lists), so the steady
    /// state does not allocate — Metal's `AtlasTexture` keeps its lists the
    /// same way.
    plan: Plan,
}

/// What every renderer shares: the wgpu device, its queue, the four pipelines
/// of phases 2–3, the atlas planes' bind group layout and the sampler.
///
/// Split from [`WgpuRenderer`] because the atlas cannot be shared: `bt-atlas`
/// holds CoreText fonts, which are neither `Send` nor `Sync`, while a device
/// is worth creating once per process. The split is also the product's shape
/// (040 phase-5): one device, a renderer per pane — the atlas's key includes
/// the pane's point size.
pub(crate) struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    cell_bg: wgpu::RenderPipeline,
    caret: wgpu::RenderPipeline,
    cell: wgpu::RenderPipeline,
    emoji: wgpu::RenderPipeline,
    /// The atlas planes' bind group layout: texture @0, sampler @1.
    plane_layout: wgpu::BindGroupLayout,
    /// Nearest, clamp-to-edge: `cell.metal`'s `constexpr sampler`.
    sampler: wgpu::Sampler,
}

impl Gpu {
    /// The process-wide device: created once and shared by every renderer
    /// (the Metal side builds a new device per `Renderer`; creating a wgpu
    /// adapter and device is not a cost worth paying per test).
    pub(crate) fn shared() -> &'static Self {
        static SHARED: OnceLock<Gpu> = OnceLock::new();
        SHARED.get_or_init(|| Self::new().expect("wgpu device and pipelines"))
    }

    /// A device on the Metal backend and four pipelines.
    ///
    /// The backend is **pinned to Metal**: the same hardware path as the
    /// oracle is compared; the Vulkan branch is tested on Linux in the font
    /// set (plan.md → Kapsam Dışı).
    pub(crate) fn new() -> Result<Self, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .map_err(|e| format!("no adapter: {e}"))?;
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("bateri"),
            required_features: wgpu::Features::IMMEDIATES,
            required_limits: wgpu::Limits {
                max_immediate_size: IMMEDIATE_BUDGET,
                ..wgpu::Limits::default()
            },
            ..wgpu::DeviceDescriptor::default()
        }))
        .map_err(|e| format!("device request failed: {e}"))?;

        // Make a validation error a **value**, not a panic: an uncaptured
        // error kills the process in wgpu's default handler and the test's
        // message would not say which pipeline failed.
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

        let plane_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("atlas plane"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
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
        if let Some(error) = block_on(scope.pop()) {
            return Err(format!("pipeline creation failed: {error}"));
        }
        Ok(Self {
            device,
            queue,
            cell_bg,
            caret,
            cell,
            emoji,
            plane_layout,
            sampler,
        })
    }

    /// An atlas plane's texture (edge × edge, sampled and written by the
    /// queue) and its bind group.
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
        PlaneTexture { texture, bind }
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

/// A wgpu renderer: the shared [`Gpu`] plus this renderer's atlas and
/// instance buffers — the twin of Metal's `Renderer`, which also owns its atlas
/// (and is built per test). `RefCell`: Metal's reason — `&self` methods that
/// mutate the atlas, and a renderer that never leaves its thread.
pub(crate) struct WgpuRenderer {
    gpu: &'static Gpu,
    state: RefCell<State>,
}

impl WgpuRenderer {
    /// A renderer on the shared device, with no atlas yet
    /// ([`WgpuRenderer::cell_metrics`] opens it).
    pub(crate) fn new() -> Self {
        Self {
            gpu: Gpu::shared(),
            state: RefCell::new(State::default()),
        }
    }

    pub(crate) fn device(&self) -> &wgpu::Device {
        &self.gpu.device
    }

    pub(crate) fn target(&self, edge: u32) -> Target {
        self.gpu.target(edge)
    }

    /// Brings the atlas to the default font at `scale` and returns the grid
    /// geometry — twin of `Renderer::cell_metrics` (same arithmetic,
    /// `CellMetrics::from_atlas`). When the key changes, both textures are
    /// dropped in the same line (`Renderer::sync_atlas`'s reason: a texture
    /// of the old size written with the new metrics would silently corrupt).
    pub(crate) fn cell_metrics(&self, scale: f64) -> CellMetrics {
        let font = FontOptions::default();
        let family = font.family.as_deref();
        let mut state = self.state.borrow_mut();
        let entry = state.atlas.get_or_insert_with(|| WgpuAtlas {
            atlas: Atlas::new(family, font.size, scale, font.line_height),
            mask: None,
            color: None,
        });
        if entry
            .atlas
            .ensure(family, font.size, scale, font.line_height)
        {
            entry.mask = None;
            entry.color = None;
        }
        CellMetrics::from_atlas(entry.atlas.metrics(), entry.atlas.context_cell_w(), scale)
    }

    /// The glyph and rule draws of one list — `encode_glyphs`' twin: emoji
    /// **before** the mask list (the order's reason is there), glyphs and
    /// rules in one mask list, rules last ([`slots::glyph_lists`]).
    #[allow(clippy::too_many_arguments)] // `encode_glyphs`' reason: the table is half of `glyphs`
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
        let WgpuAtlas { atlas, mask, color } = entry;
        let edge = atlas.texture_px();
        let metrics = atlas.metrics();
        let mask = mask.get_or_insert_with(|| {
            // The resident tofu is written once and never again:
            // `Atlas::slot` hands no bitmap when it falls back to tofu.
            let plane = self.gpu.plane_texture(MASK_FORMAT, edge);
            self.gpu.write_slot(
                &plane.texture,
                atlas.slot_origin(TOFU),
                metrics,
                atlas.tofu_bitmap(),
                Plane::Mask,
            );
            plane
        });
        let mut upload = WgpuUpload {
            gpu: self.gpu,
            mask: &mask.texture,
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
        let (cw, ch) = metrics.cell_px;
        let uv_size = [
            f32::from(cw) / f32::from(edge.0),
            f32::from(ch) / f32::from(edge.1),
        ];
        // The colour list can only be non-empty once its texture exists (the
        // upload created it); the check keeps a missing texture a skipped
        // draw, as on Metal.
        if color.is_some() {
            plan.glyph_draw(Plane::Color, cursor, uv_size);
        }
        plan.glyph_draw(Plane::Mask, cursor, uv_size);
        Ok(())
    }

    /// The draw plan for a `Frame` — `encode_pass`'s order, restricted to the
    /// groups ported so far (`cell_bg`, caret, `cell`, `emoji`).
    ///
    /// Lists not yet ported **must be empty**, and that is an `assert`: if a
    /// frame with a selection entered the scene list, wgpu would draw it
    /// without it and the comparison must fail with "this frame cannot be
    /// drawn yet", not "diverged from the oracle".
    fn plan(
        &self,
        frame: &Frame,
        viewport_px: [f32; 2],
        atlas: &mut Option<WgpuAtlas>,
        plan: &mut Plan,
    ) -> Result<(), GpuError> {
        assert!(
            frame.selection_instances().is_empty()
                && frame.search_match_instances().is_empty()
                && frame.search_current_instances().is_empty()
                && frame.fill_search_match_instances().is_empty()
                && frame.fill_search_current_instances().is_empty()
                && frame.dock_selection_instances().is_empty()
                && frame.dock_ghosts().is_empty()
                && frame.dock_arrivals().is_empty(),
            "the wgpu renderer draws cell_bg, caret, cell and emoji today (040 phase-3); \
             a frame carrying selection/search/effect lists cannot be drawn yet"
        );
        plan.clear();
        // Grid: the offset lives in one viewport. Command marks first (sprites,
        // degenerate inversion rectangle), then ground → caret → glyphs.
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
        // Fill band: the third coordinate space, above the grid; no caret
        // slot, so the inversion rectangle is degenerate (`encode_fill`).
        if frame.fill_rows() != 0 {
            plan.ops.push(Op::Viewport(frame.fill_origin_px()));
            plan.quads(frame.fill_bg());
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
            for draw in frame.dock_button_draws(origin_y) {
                plan.rounded(std::slice::from_ref(&draw.instance), draw.core, draw.shape);
            }
            // The caret is drawn outside the scissor (`encode_dock`'s reason)
            // and the band scissor comes back for the glyphs.
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
            // No arrivals in flight (asserted above), so dock rules go with
            // the glyphs in one list, as in `encode_dock`'s no-arrival branch.
            self.glyph_draws(
                plan,
                atlas,
                frame.dock_glyphs(),
                frame.dock_clusters(),
                frame.dock_rules(),
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
    /// Does not wait. The measurement hook times this whole call as
    /// `cpu_encode` — on the Metal side the span runs from creating the command
    /// buffer to `commit`, here from planning (slot resolution and uploads
    /// included, as in Metal's `encode_glyphs`) to `submit`.
    pub(crate) fn submit(
        &self,
        target: &Target,
        clear: LinearRgba,
        frame: &Frame,
    ) -> Result<wgpu::SubmissionIndex, GpuError> {
        let viewport_px = [
            target.texture.width() as f32,
            target.texture.height() as f32,
        ];
        let mut state = self.state.borrow_mut();
        let state = &mut *state;
        let plan = &mut state.plan;
        self.plan(frame, viewport_px, &mut state.atlas, plan)?;
        self.gpu
            .fill_buffer(&mut state.quads, "instances", bytes_of(&plan.instances));
        self.gpu
            .fill_buffer(&mut state.glyphs, "glyphs", bytes_of(&plan.glyphs));
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
                }
            }
        }
        Ok(self.gpu.queue.submit([encoder.finish()]))
    }

    /// Draws the frame offscreen and reads the pixels back — twin of Metal's
    /// `render_offscreen`, same byte order (B, G, R, A).
    ///
    /// A validation error is caught by an error scope and becomes a **panic**:
    /// a test must never read an empty texture (the counterpart of the Metal
    /// side's `MTLCommandBufferStatus::Error` check). The scope stack is
    /// **thread-local** with wgpu's `std` feature (`Device::push_error_scope`'s
    /// doc) and errors are raised on the calling thread, so on the shared
    /// device a parallel test's error cannot land in this scope's `pop`.
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
        Block, ButtonState, CaretShape, CaretStyle, Cell, DockButton, Theme, UnderlineStyle,
    };
    use objc2_metal::MTLCommandBuffer;

    use super::*;
    use crate::Renderer;
    use crate::renderer::tests::{
        ACCENT, BACKGROUND, MIDTONE, MIDTONE_SRGB, WHITE, bg_cell, brightness, cell_rows,
        commit_offscreen, cursor_at, grid, grid_with_gutter, pixel_at, push_settled,
        render_offscreen as metal_offscreen, target_texture,
    };
    use crate::stats::{Samples, Stats};

    // **Guard twins.** The six tests below are wgpu twins of the Metal guards
    // in `renderer.rs` and assert the same things; the reasons are there. The
    // cell size is fixed rather than taken from the atlas: these tests are not
    // about glyphs, and `fitting_cell_px`'s only job was a cell that fits the
    // texture. In phase-4 `render_offscreen` moves to wgpu, these merge into
    // the real guards and the twins go away (discussion.md → Karar 4).

    /// Fixed cell: 8×16, eight columns and four rows on a 64 texture.
    const CELL: (u16, u16) = (8, 16);

    fn render(edge: u32, clear: LinearRgba, frame: &Frame) -> Vec<u8> {
        WgpuRenderer::new().render_offscreen(edge, clear, frame)
    }

    #[test]
    fn wgsl_pipelines_build() {
        // wgpu successor of `metallib_is_embedded_and_valid` (Karar 9): the
        // WGSL passes naga and every pipeline builds on a device requested
        // with Vulkan's immediate floor. If the WGSL side of an `Immediates`
        // block outgrows its pipeline layout, creation fails and `shared`'s
        // `expect` names the failing pipeline.
        let _ = Gpu::shared();
    }

    #[test]
    fn wgpu_cell_bg_paints_pixels_on_the_gpu() {
        // Twin of `cell_bg_paints_pixels_on_the_gpu`: two instances (stride),
        // the y flip and the sRGB round trip through `MIDTONE`.
        const EDGE: u32 = 16;
        let mut frame = Frame::default();
        frame.clear(grid(8, 8), CaretStyle::default());
        frame.push(bg_cell(0, 0, LinearRgba::from_srgb(0xff, 0x00, 0x00)));
        frame.push(bg_cell(1, 1, LinearRgba::from_srgb(0x00, 0xff, 0x00)));
        frame.push(bg_cell(0, 1, MIDTONE));
        let pixels = render(EDGE, ACCENT, &frame);
        let pixel = |x: usize, y: usize| pixel_at(&pixels, EDGE as usize, x, y);
        assert_eq!(pixel(2, 2), (255, 0, 0), "first cell red at top left");
        assert_eq!(
            pixel(12, 12),
            (0, 255, 0),
            "second cell green at bottom right"
        );
        let srgb = |hex: u32| ((hex >> 16) as u8, (hex >> 8) as u8, hex as u8);
        let close_to = |seen: (u8, u8, u8), expected: (u8, u8, u8), what: &str| {
            assert!(
                seen.0.abs_diff(expected.0) <= 1
                    && seen.1.abs_diff(expected.1) <= 1
                    && seen.2.abs_diff(expected.2) <= 1,
                "{what}: {seen:02x?} ≠ {expected:02x?}"
            );
        };
        close_to(pixel(2, 12), srgb(MIDTONE_SRGB), "cell midtone");
        close_to(
            pixel(12, 2),
            srgb(Theme::BATERI.accent),
            "empty quadrant has the clear colour",
        );
    }

    #[test]
    fn wgpu_degenerate_caret_is_the_old_rectangle_bit_for_bit() {
        // Twin of `a_degenerate_caret_shape_paints_the_old_rectangle`.
        // Equality is exact **within** the backend (Karar 3): the degenerate
        // branch uses `step`, not `smoothstep` — a leak shows at the edges.
        const EDGE: u32 = 64;
        let mut frame = Frame::default();
        frame.clear(
            grid(CELL.0, CELL.1),
            CaretStyle {
                radius_ratio: 0.0,
                glow: 0.0,
                ..CaretStyle::default()
            },
        );
        push_settled(&mut frame, cursor_at(0, BACKGROUND), ACCENT);
        frame.push(bg_cell(1, 0, ACCENT));
        let pixels = render(EDGE, BACKGROUND, &frame);
        let cell = |col| cell_rows(&pixels, EDGE as usize, CELL, col).concat();
        assert_eq!(
            cell(0),
            cell(1),
            "degenerate caret differs from the plain quad: the fallback path is broken"
        );
    }

    #[test]
    fn wgpu_caret_corner_is_rounded() {
        // Twin of `the_caret_corner_is_rounded`: the reference is outside the
        // caret.
        const EDGE: u32 = 64;
        let mut frame = Frame::default();
        frame.clear(
            grid(CELL.0, CELL.1),
            CaretStyle {
                radius_ratio: 0.5,
                glow: 0.0,
                ..CaretStyle::default()
            },
        );
        push_settled(&mut frame, cursor_at(0, BACKGROUND), ACCENT);
        let pixels = render(EDGE, BACKGROUND, &frame);
        let edge = EDGE as usize;
        let sum = |x, y| brightness(&pixels, edge, x, y);
        let clear = sum(edge - 1, edge - 1);
        let middle = sum(usize::from(CELL.0) / 2, usize::from(CELL.1) / 2);
        assert!(middle > clear, "caret centre is not painted");
        assert_eq!(
            sum(0, 0),
            clear,
            "corner is painted: the shader does not round"
        );
    }

    #[test]
    fn wgpu_hollow_caret_paints_only_its_edge() {
        // Twin of `a_hollow_caret_paints_only_its_edge`: the stroke branch.
        const EDGE: u32 = 64;
        let mut frame = Frame::default();
        frame.clear(grid(CELL.0, CELL.1), CaretStyle::default());
        push_settled(&mut frame, cursor_at(0, BACKGROUND), ACCENT);
        let stroke = (f32::from(CELL.0) / 4.0).max(1.0);
        frame.force_caret_sdf([0.0, stroke, 0.0, 0.0]);
        let pixels = render(EDGE, BACKGROUND, &frame);
        let edge = EDGE as usize;
        let sum = |x, y| brightness(&pixels, edge, x, y);
        let clear = sum(edge - 1, edge - 1);
        let rim = sum(0, usize::from(CELL.1) / 2);
        let middle = sum(usize::from(CELL.0) / 2, usize::from(CELL.1) / 2);
        assert!(rim > clear, "hollow caret edge was not drawn");
        assert_eq!(middle, clear, "hollow caret centre is painted");
    }

    #[test]
    fn wgpu_caret_glow_spills_but_stops() {
        // Twin of `the_caret_glow_spills_but_stops`: the glow is outside and
        // within its margin.
        const EDGE: u32 = 64;
        const GUTTER: u16 = 16;
        let mut frame = Frame::default();
        frame.clear(
            grid_with_gutter(CELL.0, CELL.1, GUTTER),
            CaretStyle::default(),
        );
        push_settled(&mut frame, cursor_at(0, BACKGROUND), ACCENT);
        let pixels = render(EDGE, BACKGROUND, &frame);
        let edge = EDGE as usize;
        let right = usize::from(GUTTER) + usize::from(CELL.0);
        let pad = (f32::from(GUTTER) * crate::frame::CARET_GLOW_RATIO) as usize;
        let y = usize::from(CELL.1) / 2;
        assert!(
            right + pad + 2 < edge,
            "sample points do not fit the texture"
        );
        let clear = pixel_at(&pixels, edge, edge - 1, edge - 1);
        assert_ne!(
            pixel_at(&pixels, edge, right + pad / 2, y),
            clear,
            "no glow outside the rectangle"
        );
        assert_eq!(
            pixel_at(&pixels, edge, right + pad + 2, y),
            clear,
            "glow paints beyond its margin: unbounded"
        );
    }

    #[test]
    fn wgpu_caret_glow_fades_with_the_caret() {
        // Twin of `the_caret_glow_fades_with_the_caret`: glow × caret alpha.
        const EDGE: u32 = 64;
        const GUTTER: u16 = 16;
        let edge = EDGE as usize;
        let y = usize::from(CELL.1) / 2;
        let pad = (f32::from(GUTTER) * crate::frame::CARET_GLOW_RATIO) as usize;
        let at = usize::from(GUTTER) + usize::from(CELL.0) + pad / 2;
        assert!(at < edge, "sample point does not fit the texture");
        let sample = |alpha: f32| {
            let mut frame = Frame::default();
            frame.clear(
                grid_with_gutter(CELL.0, CELL.1, GUTTER),
                CaretStyle::default(),
            );
            frame.push_caret(
                [0.0, 0.0],
                BACKGROUND,
                ACCENT,
                alpha,
                CaretShape::Block,
                true,
            );
            brightness(&render(EDGE, BACKGROUND, &frame), edge, at, y)
        };
        let (dark, half, full) = (sample(0.0), sample(0.5), sample(1.0));
        assert!(
            dark < half && half < full,
            "glow does not follow the caret's alpha: {dark} / {half} / {full}"
        );
    }

    // **Guard twins of the `cell` + `emoji` group** (phase-3). Glyph tests draw
    // at `SCALE`; the reasons are the Metal guards' named in each test.

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
    fn wgpu_emoji_round_trip(rgb: (u8, u8, u8), alpha: u8) -> (u8, u8, u8) {
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
    fn wgpu_a_color_glyph_takes_its_color_from_the_texture() {
        // Twin of `a_midtone_color_slot_survives_the_round_trip`: a midtone
        // (not a fixed point of the sRGB transfer) comes back as the same byte
        // through the sRGB colour plane — and not as the instance's red.
        const MID: (u8, u8, u8) = (0x80, 0x40, 0xc0);
        let seen = wgpu_emoji_round_trip(MID, 0xff);
        assert!(
            seen.0.abs_diff(MID.0) <= 1
                && seen.1.abs_diff(MID.1) <= 1
                && seen.2.abs_diff(MID.2) <= 1,
            "colour plane round trip: {seen:02x?} ≠ {MID:02x?} \
             (red means the colour came from the instance)"
        );
    }

    #[test]
    fn wgpu_a_translucent_color_edge_composites_in_linear_space() {
        // Twin of `a_translucent_edge_composites_in_linear_space`: half-alpha
        // white over black is exactly half in linear space, 0xBC once encoded;
        // 0x80 would mean straight alpha was blended in encoded space.
        let seen = wgpu_emoji_round_trip((0xff, 0xff, 0xff), 0x80);
        assert!(
            seen.0.abs_diff(0xbc) <= 2,
            "translucent edge did not composite linearly: {seen:02x?} ≠ ~0xbc"
        );
    }

    #[test]
    fn wgpu_wide_glyph_halves_meet_without_a_seam() {
        // A wide glyph is two quads from two slots (`slots::fan`); the right
        // half is rasterised a whole number of pixels to the left, so its AA
        // phase is the left half's and a stroke crossing the boundary must
        // continue there pixel for pixel. `一` is one horizontal stroke across
        // nearly the full em: ink that crosses the boundary in any CJK font.
        // (The instance count is shared CPU code now and has its guard in
        // `a_wide_cell_becomes_two_quads`.)
        const EDGE: u32 = 64;
        let w = WgpuRenderer::new();
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
    fn wgpu_a_rule_is_drawn_over_its_glyph() {
        // Rules come after glyphs in the one mask list (`slots::glyph_lists`).
        // `█` is procedural and fills the whole cell (no font involved), so
        // the underline lies on it: drawn after the glyph it reads red, drawn
        // before it would vanish under the block's white.
        const EDGE: u32 = 64;
        let w = WgpuRenderer::new();
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

    fn scenes(m: CellMetrics) -> Vec<Scene> {
        vec![
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
        ]
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
        let metal = Renderer::system_default().expect("Metal device and pipelines");
        let w = WgpuRenderer::new();
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

    fn report(backend: &str, frames: usize, stats: &Stats) -> String {
        let profile = if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        };
        let cpu = stats.cpu_frame();
        let gpu = stats.gpu();
        let gpu_line = if backend == "wgpu" {
            // wgpu's GPU timestamp needs `TIMESTAMP_QUERY`; that is phase-4.
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

        let metal = Renderer::system_default().expect("Metal device and pipelines");
        let texture = target_texture(&metal, usize::from(EDGE));
        let w = WgpuRenderer::new();
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
            w.submit(&target, clear, &frame)
                .expect("the hook frame draws no glyphs");
            let t2 = Instant::now();
            w.device()
                .poll(wgpu::PollType::wait_indefinitely())
                .expect("waiting for the GPU failed");
            if i >= WARMUP {
                wgpu_stats.record_cpu(t1 - t0, t2 - t1);
            }
        }
        println!("{}", report("metal", FRAMES, &metal_stats));
        println!("{}", report("wgpu", FRAMES, &wgpu_stats));
    }
}
