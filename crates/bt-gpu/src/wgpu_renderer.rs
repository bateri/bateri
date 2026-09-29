//! The wgpu renderer — 040's parallel renderer, today **test-only**.
//!
//! The Metal renderer ([`crate::Renderer`]) stays in place as the oracle and
//! this module grows next to it one pipeline group at a time
//! (`.tasks/040-linux-kapisi-ve-wgpu/discussion.md` → Karar 3 and 4). Phase-2
//! covers `cell_bg` + caret: background quads, the caret's SDF and the dock
//! buttons, the second consumer of the same fragment. Glyph, rule, selection
//! and effect lists do not exist yet and [`WgpuRenderer::plan`] requires them
//! to be **empty** — a half-drawn scene must not silently diverge from the
//! oracle.
//!
//! The module sits behind `cfg(test)` (wgpu is a dev-dependency): the product
//! binary's graph does not change and backing out is a single `git revert`.
//! Pipeline order, the viewport/scissor sequence and blending are **the same**
//! as `renderer.rs`'s `encode_pass` / `encode_fill` / `encode_dock` /
//! `pipeline`; the reasons live there and are not repeated here. The one
//! difference is how commands are recorded: Metal builds a buffer per list per
//! frame, here every quad of the frame goes into **one** instance buffer and
//! draws read ranges of it ([`Plan`]). That buffer is not rebuilt per frame
//! either: it lives as long as the renderer, grows on demand and is filled
//! with `write_buffer` — creating a buffer in wgpu is a validation and
//! tracking round trip, and it was measured (phase-2 → Uygulama Notları): a
//! per-frame buffer visibly inflated `cpu_encode`.

use std::ops::Range;
use std::sync::{Mutex, OnceLock, PoisonError};
use std::task::{Context, Poll, Waker};

use crate::frame::{Frame, INSTANCE_OFFSETS, Instance};
use crate::renderer::scissor_rect_below;
use bt_core::LinearRgba;

/// Immediate data budget in bytes: the smallest `maxPushConstantsSize` Vulkan
/// **guarantees** (discussion.md → Karar 5). Metal offers 4096, but a layout
/// that does not fit the smallest Linux driver would fail there; the device is
/// requested with exactly this limit, so an oversized pipeline is rejected on
/// macOS too.
pub(crate) const IMMEDIATE_BUDGET: u32 = 128;

/// Target format — Metal's `BGRA8Unorm_sRGB`: the fragment writes **linear**,
/// the hardware encodes to sRGB (`CLAUDE.md` → colour space).
pub(crate) const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;

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
// SAFETY: `repr(C)`, `f32` fields only; WGSL's trailing padding is an
// explicit field (`pad`), size 48 is asserted.
unsafe impl GpuBytes for Immediates {}

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
}

/// The frame's draw plan: one instance buffer and the steps reading ranges of
/// it. [`WgpuRenderer::plan`] builds it from a `Frame`,
/// [`WgpuRenderer::submit`] replays it into a pass.
#[derive(Default)]
struct Plan {
    instances: Vec<Instance>,
    ops: Vec<Op>,
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

/// The wgpu device, its queue, this phase's two pipelines and the instance
/// buffer.
pub(crate) struct WgpuRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    cell_bg: wgpu::RenderPipeline,
    caret: wgpu::RenderPipeline,
    /// The frame's quads; grows, never shrinks. **The lock is held from the
    /// write to the submit**: `write_buffer` lands at the next `submit`, so if
    /// two callers (parallel tests) interleaved, A's frame would be drawn with
    /// B's quads.
    instances: Mutex<Option<wgpu::Buffer>>,
}

impl WgpuRenderer {
    /// The process-wide renderer: the device is created once and tests share
    /// it (the Metal side builds a new `Renderer` per test; creating a wgpu
    /// adapter and device is not a cost worth paying per test).
    pub(crate) fn shared() -> &'static Self {
        static SHARED: OnceLock<WgpuRenderer> = OnceLock::new();
        SHARED.get_or_init(|| Self::new().expect("wgpu device and pipelines"))
    }

    /// A device on the Metal backend and two pipelines.
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
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cell_bg.wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/cell_bg.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cell_bg"),
            bind_group_layouts: &[],
            immediate_size: size_of::<Immediates>() as u32,
        });
        let cell_bg = pipeline(
            &device,
            &layout,
            &module,
            "cell_bg_vertex",
            "cell_bg_fragment",
        );
        let caret = pipeline(
            &device,
            &layout,
            &module,
            "cell_bg_vertex",
            "caret_fragment",
        );
        if let Some(error) = block_on(scope.pop()) {
            return Err(format!("pipeline creation failed: {error}"));
        }
        Ok(Self {
            device,
            queue,
            cell_bg,
            caret,
            instances: Mutex::new(None),
        })
    }

    pub(crate) fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// The draw plan for a `Frame` — `encode_pass`'s order, restricted to
    /// `cell_bg` + caret.
    ///
    /// Lists not yet ported **must be empty**, and that is an `assert`: if a
    /// frame with glyphs entered the scene list, wgpu would draw it without
    /// letters and the comparison must fail with "this frame cannot be drawn
    /// yet", not "diverged from the oracle".
    fn plan(frame: &Frame, viewport_px: [f32; 2]) -> Plan {
        assert!(
            frame.stripes().is_empty()
                && frame.glyphs().is_empty()
                && frame.rules().is_empty()
                && frame.selection_instances().is_empty()
                && frame.search_match_instances().is_empty()
                && frame.search_current_instances().is_empty()
                && frame.fill_glyphs().is_empty()
                && frame.fill_rules().is_empty()
                && frame.fill_search_match_instances().is_empty()
                && frame.fill_search_current_instances().is_empty()
                && frame.dock_selection_instances().is_empty()
                && frame.dock_glyphs().is_empty()
                && frame.dock_rules().is_empty()
                && frame.dock_ghosts().is_empty()
                && frame.dock_arrivals().is_empty(),
            "the wgpu renderer draws only cell_bg + caret today (040 phase-2); \
             a frame carrying glyph/rule/selection/effect lists cannot be drawn yet"
        );
        let mut plan = Plan::default();
        // Grid: the offset lives in one viewport, ground → caret.
        plan.ops.push(Op::Viewport(frame.origin_px()));
        plan.quads(frame.bg_instances());
        plan.rounded(
            frame.grid_caret().as_slice(),
            frame.caret_core(),
            frame.caret_sdf(),
        );
        // Fill band: the third coordinate space, above the grid.
        if frame.fill_rows() != 0 {
            plan.ops.push(Op::Viewport(frame.fill_origin_px()));
            plan.quads(frame.fill_bg());
        }
        // Dock: last, with two origins (see `encode_dock`'s doc).
        if frame.dock().is_some() {
            let band_y = (viewport_px[1] - frame.dock_band_px()).max(0.0);
            let origin_y = (viewport_px[1] - frame.dock_layout_px()).max(0.0);
            plan.ops.push(Op::Viewport(band_y));
            plan.quads(&frame.dock_ground(viewport_px[0]));
            plan.ops.push(Op::Viewport(origin_y));
            let clipped = band_y > origin_y;
            if clipped {
                plan.ops
                    .push(Op::Scissor(scissor_below(band_y, viewport_px)));
            }
            plan.quads(frame.dock_bg());
            for draw in frame.dock_button_draws(origin_y) {
                plan.rounded(std::slice::from_ref(&draw.instance), draw.core, draw.shape);
            }
            // The caret is drawn outside the scissor (`encode_dock`'s reason).
            if clipped {
                plan.ops.push(Op::Scissor(scissor_below(0.0, viewport_px)));
            }
            // After the caret Metal restores the band scissor for effects and
            // glyphs and finally opens it to the whole texture; neither exists
            // in this phase, so the open scissor is already the final state.
            plan.rounded(
                frame.dock_caret(origin_y).as_slice(),
                frame.caret_core(),
                frame.caret_sdf(),
            );
        }
        plan
    }

    /// Draws the frame into `target` and **submits** it: one render pass, the
    /// ground loaded with `clear` (Metal's `MTLLoadAction::Clear`), the plan on
    /// top.
    ///
    /// Does not wait. The measurement hook times this whole call as
    /// `cpu_encode` — on the Metal side the span runs from creating the command
    /// buffer to `commit`, here from `write_buffer` to `submit`.
    pub(crate) fn submit(
        &self,
        target: &Target,
        clear: LinearRgba,
        frame: &Frame,
    ) -> wgpu::SubmissionIndex {
        let viewport_px = [
            target.texture.width() as f32,
            target.texture.height() as f32,
        ];
        let plan = Self::plan(frame, viewport_px);
        let bytes = bytes_of(&plan.instances);
        let mut slot = self
            .instances
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if !bytes.is_empty() {
            let len = bytes.len() as u64;
            if slot.as_ref().is_none_or(|buffer| buffer.size() < len) {
                *slot = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("instances"),
                    // Power of two: a growing grid should not force a
                    // rebuild every frame.
                    size: len.next_power_of_two(),
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
            }
            if let Some(buffer) = slot.as_ref() {
                self.queue.write_buffer(buffer, 0, bytes);
            }
        }
        let mut encoder = self
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
            if let (false, Some(buffer)) = (bytes.is_empty(), slot.as_ref()) {
                pass.set_vertex_buffer(0, buffer.slice(..));
            }
            let mut imm = Immediates {
                viewport_px,
                ..Immediates::default()
            };
            for op in &plan.ops {
                match op {
                    Op::Viewport(y) => {
                        pass.set_viewport(0.0, *y, viewport_px[0], viewport_px[1], 0.0, 1.0);
                    }
                    Op::Scissor([x, y, w, h]) => pass.set_scissor_rect(*x, *y, *w, *h),
                    Op::Quads(range) => {
                        pass.set_pipeline(&self.cell_bg);
                        pass.set_immediates(0, bytes_of(std::slice::from_ref(&imm)));
                        pass.draw(0..4, range.clone());
                    }
                    Op::Rounded { range, core, shape } => {
                        imm.core = *core;
                        imm.shape = *shape;
                        pass.set_pipeline(&self.caret);
                        pass.set_immediates(0, bytes_of(std::slice::from_ref(&imm)));
                        pass.draw(0..4, range.clone());
                    }
                }
            }
        }
        let index = self.queue.submit([encoder.finish()]);
        drop(slot);
        index
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

    /// Draws the frame offscreen and reads the pixels back — twin of Metal's
    /// `render_offscreen`, same byte order (B, G, R, A).
    ///
    /// A validation error is caught by an error scope and becomes a **panic**:
    /// a test must never read an empty texture (the counterpart of the Metal
    /// side's `MTLCommandBufferStatus::Error` check).
    pub(crate) fn render_offscreen(&self, edge: u32, clear: LinearRgba, frame: &Frame) -> Vec<u8> {
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let target = self.target(edge);
        self.submit(&target, clear, frame);
        let pixels = self.read_back(&target.texture);
        if let Some(error) = block_on(scope.pop()) {
            panic!("wgpu frame failed validation: {error}");
        }
        pixels
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

/// Twin of `renderer.rs` → `pipeline`: same blending (straight alpha, the alpha
/// channel's source factor is `One` — reason there), triangle strip, one
/// target.
fn pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    module: &wgpu::ShaderModule,
    vs_name: &'static str,
    fs_name: &'static str,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(fs_name),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some(vs_name),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: size_of::<Instance>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &INSTANCE_ATTRIBUTES,
            })],
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

    use bt_core::{ButtonState, CaretShape, CaretStyle, DockButton, Theme};
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
        WgpuRenderer::shared().render_offscreen(edge, clear, frame)
    }

    #[test]
    fn wgsl_pipelines_build() {
        // wgpu successor of `metallib_is_embedded_and_valid` (Karar 9): the
        // WGSL passes naga and both pipelines build on a device requested with
        // Vulkan's immediate floor. If the WGSL side of `Immediates` outgrows
        // the pipeline layout, creation fails and `shared`'s `expect` names
        // the failing pipeline.
        let _ = WgpuRenderer::shared();
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

    // **Oracle scene list** (Karar 4). Each scene is a `Frame` drawn on both
    // backends. The list grows as groups are ported; the block stripe is a
    // sprite today (`cell` pipeline) and is phase-3's scene.

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

    fn scenes() -> Vec<Scene> {
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
        let scenes = scenes();
        assert!(!scenes.is_empty());
        for (name, edge, frame) in &scenes {
            let n = *edge as usize;
            let oracle = metal_offscreen(&metal, n, BACKGROUND, frame);
            let seen = render(*edge, BACKGROUND, frame);
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
        let w = WgpuRenderer::shared();
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
            w.submit(&target, clear, &frame);
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
