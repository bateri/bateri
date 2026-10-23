//! The UI kit's draw list and the renderer half that paints it.
//!
//! A pane that shows a UI page paints a [`UiList`] instead of the terminal
//! grid: boxes ([`UiQuad`]: fill, border, soft shadow, per-corner radii) and
//! glyphs of proportional text ([`UiGlyph`]), in painter's order. The list is
//! in **pixels** of the surface, top-left origin, and its colours are
//! **linear** straight RGBA — the rule of `renderer::FORMAT`, which every
//! pipeline here follows (`ui.wgsl`).
//!
//! The renderer is also what **measures** text. The object that measures has
//! to be the object that draws: it holds the font registry and the text atlas
//! (`bt_atlas::TextAtlas`), so a width asked here and a glyph drawn here come
//! from one font, one size and one scale. [`Renderer::ui_shape`] shapes and
//! measures a line, [`Renderer::ui_text`] puts it into a list.
//!
//! This module knows nothing of terminals and nothing of any engine that
//! decides what the list contains: a [`UiPage`] does, and the link calls it
//! whenever the surface needs a frame.
//!
//! The text atlas is **not** the cell atlas: no slot grid, no `Room` loop. It
//! is one `R8` texture filled glyph by glyph (a shelf packer) and, when it
//! fills, cleared as a whole — [`Renderer::ui_flushed`] tells whoever holds a
//! list built across the clear to build it again.

use std::ops::Range;

use bt_atlas::{FontId, ShapedText, TextAtlas, TextStyle, subpixel};
use bt_core::{LinearRgba, Theme};

use super::{GpuBytes, Pending, PlaneTexture, Renderer, Target, block_on, bytes_of, pipeline_over};
use crate::GpuError;

/// The text texture's edge, pixels: 4 MB of `R8`, room for several thousand
/// glyphs at text sizes. A device's texture ceiling is never below it.
const TEXT_EDGE: u16 = 2048;

/// A rectangle in surface pixels: the top-left corner, then the size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl UiRect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    /// A clip that clips nothing.
    pub const UNBOUNDED: Self = Self {
        x: -1.0e6,
        y: -1.0e6,
        w: 2.0e6,
        h: 2.0e6,
    };

    /// `x0, y0, x1, y1`: what the shader's clip test reads.
    const fn corners(self) -> [f32; 4] {
        [self.x, self.y, self.x + self.w, self.y + self.h]
    }
}

/// A colour for the draw list: a theme colour (already linear) with `alpha`.
pub fn tint(color: LinearRgba, alpha: f32) -> [f32; 4] {
    let [r, g, b, _] = color.to_array();
    [r, g, b, alpha]
}

/// A box: `ui.wgsl`'s `QuadIn` and its Rust twin. Built with the methods
/// below; a box with only a rectangle draws nothing (transparent, no border,
/// no shadow).
///
/// `#[repr(C)]`, seven `[f32; 4]`, stride 112, no padding.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiQuad {
    rect: [f32; 4],
    fill: [f32; 4],
    border_color: [f32; 4],
    shadow_color: [f32; 4],
    radii: [f32; 4],
    clip: [f32; 4],
    /// Border width, shadow blur, shadow dx, shadow dy.
    params: [f32; 4],
}

const _: () = assert!(size_of::<UiQuad>() == 112);
const _: () = assert!(std::mem::offset_of!(UiQuad, fill) == 16);
const _: () = assert!(std::mem::offset_of!(UiQuad, border_color) == 32);
const _: () = assert!(std::mem::offset_of!(UiQuad, shadow_color) == 48);
const _: () = assert!(std::mem::offset_of!(UiQuad, radii) == 64);
const _: () = assert!(std::mem::offset_of!(UiQuad, clip) == 80);
const _: () = assert!(std::mem::offset_of!(UiQuad, params) == 96);

// SAFETY: `repr(C)`, `f32` fields only; its size is the sum of its fields
// (the `size_of`/`offset_of` asserts above: 112, no gaps).
unsafe impl GpuBytes for UiQuad {}

impl UiQuad {
    /// A box at `rect`: transparent until given a fill, a border or a shadow.
    pub const fn new(rect: UiRect) -> Self {
        Self {
            rect: [rect.x, rect.y, rect.w, rect.h],
            fill: [0.0; 4],
            border_color: [0.0; 4],
            shadow_color: [0.0; 4],
            radii: [0.0; 4],
            clip: UiRect::UNBOUNDED.corners(),
            params: [0.0; 4],
        }
    }

    /// The background: linear straight RGBA ([`tint`]).
    #[must_use]
    pub const fn fill(mut self, color: [f32; 4]) -> Self {
        self.fill = color;
        self
    }

    /// The same radius on all four corners, pixels.
    #[must_use]
    pub const fn radius(mut self, radius: f32) -> Self {
        self.radii = [radius; 4];
        self
    }

    /// A radius per corner: top-left, top-right, bottom-right, bottom-left.
    #[must_use]
    pub const fn radii(mut self, radii: [f32; 4]) -> Self {
        self.radii = radii;
        self
    }

    /// A border of `width` pixels drawn inside the box, over the fill.
    #[must_use]
    pub const fn border(mut self, width: f32, color: [f32; 4]) -> Self {
        self.params[0] = width;
        self.border_color = color;
        self
    }

    /// A soft shadow: the box's silhouette moved by `(dx, dy)` and blurred
    /// over `blur` pixels (CSS's blur radius), never drawn under the box.
    #[must_use]
    pub const fn shadow(mut self, dx: f32, dy: f32, blur: f32, color: [f32; 4]) -> Self {
        self.params[1] = blur;
        self.params[2] = dx;
        self.params[3] = dy;
        self.shadow_color = color;
        self
    }

    /// Nothing is drawn outside `clip`.
    #[must_use]
    pub const fn clip(mut self, clip: UiRect) -> Self {
        self.clip = clip.corners();
        self
    }
}

/// One glyph of proportional text: `ui.wgsl`'s `GlyphIn` and its Rust twin.
/// Made by [`Renderer::ui_text`], never by hand — its source rectangle is a
/// place in the text atlas.
///
/// `#[repr(C)]`: three `[f32; 4]` and a `[f32; 2]` with its explicit pad,
/// stride 64.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiGlyph {
    /// x, y, width, height on the surface (whole pixels).
    rect: [f32; 4],
    color: [f32; 4],
    clip: [f32; 4],
    /// The bitmap's top-left corner in the atlas.
    src: [f32; 2],
    pad: [f32; 2],
}

const _: () = assert!(size_of::<UiGlyph>() == 64);
const _: () = assert!(std::mem::offset_of!(UiGlyph, color) == 16);
const _: () = assert!(std::mem::offset_of!(UiGlyph, clip) == 32);
const _: () = assert!(std::mem::offset_of!(UiGlyph, src) == 48);

// SAFETY: `repr(C)`, `f32` fields only; the trailing `pad` is explicit, size
// 64 is asserted.
unsafe impl GpuBytes for UiGlyph {}

/// One thing the list draws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UiItem {
    Quad(UiQuad),
    Glyph(UiGlyph),
}

/// A draw list in painter's order: later items cover earlier ones.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UiList {
    items: Vec<UiItem>,
}

impl UiList {
    pub fn new() -> Self {
        Self::default()
    }

    /// Draws `quad` over everything before it.
    pub fn quad(&mut self, quad: UiQuad) {
        self.items.push(UiItem::Quad(quad));
    }

    pub fn items(&self) -> &[UiItem] {
        &self.items
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// What a [`UiPage`] is built for.
pub struct UiContext<'a> {
    /// Measures and draws the text.
    pub renderer: &'a Renderer,
    /// The surface, pixels.
    pub size_px: (u32, u32),
    /// Physical pixels per point.
    pub scale: f64,
    /// The pane's palette.
    pub theme: &'a Theme,
    /// The frame's time, seconds on the pacer's clock: what a page that animates measures its
    /// steps by.
    pub now: f64,
    /// Reduce Motion is on: a page that animates snaps instead.
    pub reduce_motion: bool,
}

/// Something that fills a pane's surface with a [`UiList`] instead of the
/// terminal grid. The link calls [`UiPage::build`] for every frame it draws —
/// when the surface resized, the theme or the font changed, or the page asked
/// for a frame — and draws nothing in between.
pub trait UiPage {
    /// The list for this surface. May be called again for the same state (the
    /// text atlas filled up and was cleared while building).
    fn build(&mut self, cx: &UiContext<'_>) -> UiList;

    /// Whether the page is still moving after the frame [`UiPage::build`] just made: the link
    /// asks for the next frame itself until this says no, so every animation of a page has the
    /// stop condition of its own end and a page at rest costs no frames.
    fn animating(&self) -> bool {
        false
    }
}

/// `ui.wgsl`'s `Immediates`: the surface's size, for the NDC scale. 8 bytes,
/// no WGSL padding.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct UiImmediates {
    viewport_px: [f32; 2],
}

const _: () = assert!(size_of::<UiImmediates>() == 8);
const _: () = assert!(size_of::<UiImmediates>() as u32 <= super::IMMEDIATE_BUDGET);

// SAFETY: `repr(C)`, two `f32`s, size 8 asserted.
unsafe impl GpuBytes for UiImmediates {}

const QUAD_ATTRIBUTES: [wgpu::VertexAttribute; 7] = [
    attribute(0, 0),
    attribute(16, 1),
    attribute(32, 2),
    attribute(48, 3),
    attribute(64, 4),
    attribute(80, 5),
    attribute(96, 6),
];

/// The glyph's four attributes: rect@0, colour@16, clip@32, src@48 (read as a
/// `vec4`: the explicit pad is its tail).
const GLYPH_ATTRIBUTES: [wgpu::VertexAttribute; 4] = [
    attribute(0, 0),
    attribute(16, 1),
    attribute(32, 2),
    attribute(48, 3),
];

const fn attribute(offset: u64, location: u32) -> wgpu::VertexAttribute {
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset,
        shader_location: location,
    }
}

/// The two pipelines, built with the device ([`Gpu::new`](super::Gpu)).
pub(super) struct Pipelines {
    quad: wgpu::RenderPipeline,
    glyph: wgpu::RenderPipeline,
}

impl Pipelines {
    pub(super) fn new(device: &wgpu::Device, plane_layout: &wgpu::BindGroupLayout) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ui.wgsl"),
            // The corner curve is the grid's own, appended: one copy.
            source: wgpu::ShaderSource::Wgsl(
                concat!(
                    include_str!("../../shaders/ui.wgsl"),
                    include_str!("../../shaders/sdf.wgsl")
                )
                .into(),
            ),
        });
        let quads = wgpu::VertexBufferLayout {
            array_stride: size_of::<UiQuad>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &QUAD_ATTRIBUTES,
        };
        let glyphs = wgpu::VertexBufferLayout {
            array_stride: size_of::<UiGlyph>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &GLYPH_ATTRIBUTES,
        };
        let quad_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ui quad"),
            bind_group_layouts: &[],
            immediate_size: size_of::<UiImmediates>() as u32,
        });
        let glyph_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ui glyph"),
            bind_group_layouts: &[Some(plane_layout)],
            immediate_size: size_of::<UiImmediates>() as u32,
        });
        Self {
            quad: pipeline_over(
                device,
                &quad_layout,
                &module,
                &[Some(quads)],
                "ui_quad_vertex",
                "ui_quad_fragment",
            ),
            glyph: pipeline_over(
                device,
                &glyph_layout,
                &module,
                &[Some(glyphs)],
                "ui_glyph_vertex",
                "ui_glyph_fragment",
            ),
        }
    }
}

/// This renderer's UI half: the text atlas with its texture, and the two
/// instance buffers.
#[derive(Default)]
pub(super) struct UiState {
    /// Opened with the first font: a renderer that never paints a page never
    /// pays for the atlas or its 4 MB.
    text: Option<TextAtlas>,
    texture: Option<PlaneTexture>,
    quads: Option<wgpu::Buffer>,
    glyphs: Option<wgpu::Buffer>,
}

/// A run of one kind of item in the list, drawn with one call.
struct Run {
    glyphs: bool,
    range: Range<u32>,
}

impl Renderer {
    /// Does this platform shape and draw proportional text at all? Where it
    /// does not, [`Renderer::ui_font`] answers `None` and a page draws its
    /// boxes without words.
    pub fn ui_text_supported() -> bool {
        TextAtlas::supported()
    }

    /// Opens `style` (sizes in pixels); `None` where there is no proportional
    /// text or the font could not be opened.
    pub fn ui_font(&self, style: &TextStyle) -> Option<FontId> {
        self.ui
            .borrow_mut()
            .text
            .get_or_insert_with(|| TextAtlas::new(TEXT_EDGE))
            .open(style)
    }

    /// Shapes `text` as one line in `font` and measures it: width, ascent,
    /// descent and leading, pixels ([`ShapedText`]).
    pub fn ui_shape(&self, font: FontId, text: &str) -> Option<ShapedText> {
        self.ui.borrow_mut().text.as_mut()?.shape(font, text)
    }

    /// Puts a shaped line into `list`: its line box starts at `at` (the top
    /// left), glyphs are placed at their fractional pen positions in the
    /// atlas's sub-pixel buckets and the baseline sits on a whole pixel.
    /// Returns the line's width.
    pub fn ui_text(
        &self,
        list: &mut UiList,
        line: &ShapedText,
        at: [f32; 2],
        color: [f32; 4],
        clip: UiRect,
    ) -> f32 {
        let mut ui = self.ui.borrow_mut();
        let Some(text) = ui.text.as_mut() else {
            return 0.0;
        };
        // The leading is split above and below the line, as the grid's cell
        // splits the line-height excess.
        let baseline = (f64::from(at[1]) + line.leading / 2.0 + line.ascent).round();
        for placed in &line.glyphs {
            let (whole, bucket) = subpixel(f64::from(at[0]) + placed.x);
            let Some(entry) = text.glyph(placed.font, placed.glyph, bucket) else {
                continue;
            };
            list.items.push(UiItem::Glyph(UiGlyph {
                rect: [
                    (whole + i32::from(entry.left)) as f32,
                    (baseline - f64::from(entry.top)) as f32,
                    f32::from(entry.width),
                    f32::from(entry.height),
                ],
                color,
                clip: clip.corners(),
                src: [f32::from(entry.x), f32::from(entry.y)],
                pad: [0.0; 2],
            }));
        }
        self.write_text(&mut ui);
        line.width as f32
    }

    /// Whether the text atlas filled up and was cleared since the last call:
    /// a list built across the clear points at texels the later glyphs
    /// overwrote, so it is built again.
    pub fn ui_flushed(&self) -> bool {
        self.ui
            .borrow_mut()
            .text
            .as_mut()
            .is_some_and(TextAtlas::take_flushed)
    }

    /// Writes the glyph bitmaps drawn since the last time into the texture,
    /// creating it first.
    fn write_text(&self, ui: &mut UiState) {
        let UiState { text, texture, .. } = ui;
        let Some(text) = text.as_mut() else {
            return;
        };
        let uploads = text.take_uploads();
        if uploads.is_empty() && texture.is_some() {
            return;
        }
        let edge = text.edge();
        let plane =
            texture.get_or_insert_with(|| self.gpu.plane_texture(super::MASK_FORMAT, (edge, edge)));
        for upload in uploads {
            self.gpu.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &plane.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: u32::from(upload.x),
                        y: u32::from(upload.y),
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &upload.data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(u32::from(upload.width)),
                    rows_per_image: Some(u32::from(upload.height)),
                },
                wgpu::Extent3d {
                    width: u32::from(upload.width),
                    height: u32::from(upload.height),
                    depth_or_array_layers: 1,
                },
            );
        }
    }

    /// The frame path for a page: encode and submit inside an error scope,
    /// then track the submission for [`Renderer::poll`] — [`Renderer::draw`]'s
    /// contract, for a list.
    pub(crate) fn draw_ui(
        &self,
        target: &Target,
        clear: LinearRgba,
        list: &UiList,
    ) -> Result<(), GpuError> {
        let fault = self.gpu.fault.generation();
        let memory = self
            .gpu
            .device
            .push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let validation = self
            .gpu
            .device
            .push_error_scope(wgpu::ErrorFilter::Validation);
        let submitted = self.encode_ui(target, clear, list);
        let errors = [block_on(validation.pop()), block_on(memory.pop())];
        let index = submitted?;
        if let Some(error) = errors.into_iter().flatten().next() {
            return Err(GpuError::Wgpu(error.to_string()));
        }
        self.in_flight.borrow_mut().push_back(Pending {
            index,
            fault,
            timing: None,
        });
        Ok(())
    }

    fn encode_ui(
        &self,
        target: &Target,
        clear: LinearRgba,
        list: &UiList,
    ) -> Result<wgpu::SubmissionIndex, GpuError> {
        let mut ui = self.ui.borrow_mut();
        self.write_text(&mut ui);

        let mut quads: Vec<UiQuad> = Vec::new();
        let mut glyphs: Vec<UiGlyph> = Vec::new();
        let mut runs: Vec<Run> = Vec::new();
        for item in &list.items {
            let (is_glyph, count) = match item {
                UiItem::Quad(quad) => {
                    quads.push(*quad);
                    (false, quads.len())
                }
                UiItem::Glyph(glyph) => {
                    glyphs.push(*glyph);
                    (true, glyphs.len())
                }
            };
            let end = u32::try_from(count).unwrap_or(u32::MAX);
            match runs.last_mut() {
                Some(run) if run.glyphs == is_glyph => run.range.end = end,
                _ => runs.push(Run {
                    glyphs: is_glyph,
                    range: end - 1..end,
                }),
            }
        }
        self.gpu
            .fill_buffer(&mut ui.quads, "ui quads", bytes_of(&quads));
        self.gpu
            .fill_buffer(&mut ui.glyphs, "ui glyphs", bytes_of(&glyphs));

        let imm = UiImmediates {
            viewport_px: [
                target.texture.width() as f32,
                target.texture.height() as f32,
            ],
        };
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let [r, g, b, a] = clear.to_array();
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ui"),
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
                ..wgpu::RenderPassDescriptor::default()
            });
            for run in &runs {
                if run.glyphs {
                    // A glyph exists only after the atlas did.
                    let (Some(buffer), Some(plane)) = (ui.glyphs.as_ref(), ui.texture.as_ref())
                    else {
                        continue;
                    };
                    pass.set_pipeline(&self.gpu.ui.glyph);
                    pass.set_bind_group(0, &plane.bind, &[]);
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    pass.set_immediates(0, bytes_of(std::slice::from_ref(&imm)));
                    pass.draw(0..4, run.range.clone());
                } else {
                    let Some(buffer) = ui.quads.as_ref() else {
                        continue;
                    };
                    pass.set_pipeline(&self.gpu.ui.quad);
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    pass.set_immediates(0, bytes_of(std::slice::from_ref(&imm)));
                    pass.draw(0..4, run.range.clone());
                }
            }
        }
        Ok(self.gpu.queue.submit([encoder.finish()]))
    }

    /// Draws a list offscreen and reads the pixels back, byte order B, G, R,
    /// A — [`Renderer::render_offscreen`]'s twin for a list.
    #[cfg(test)]
    pub(crate) fn render_ui_offscreen(
        &self,
        edge: u32,
        clear: LinearRgba,
        list: &UiList,
    ) -> Vec<u8> {
        let scope = self
            .gpu
            .device
            .push_error_scope(wgpu::ErrorFilter::Validation);
        let target = self.gpu.target(edge);
        self.encode_ui(&target, clear, list)
            .expect("the list could not be encoded");
        let pixels = self.gpu.read_back(&target.texture);
        if let Some(error) = block_on(scope.pop()) {
            panic!("wgpu ui frame failed validation: {error}");
        }
        pixels
    }
}
