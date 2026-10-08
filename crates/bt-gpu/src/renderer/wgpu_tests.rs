use std::time::{Duration, Instant};

use bt_core::{
    Block, ButtonState, CaretShape, CaretStyle, Cell, DockButton, Erase, Keypress, SearchRun,
    SelectionRun, Theme, UnderlineStyle,
};

use super::*;
use crate::glyph_fx::{Effect, Fx, Kind};
use crate::renderer::tests::{
    ACCENT, BACKGROUND, MIDTONE, WHITE, bg_cell, cell_rows, grid, grid_with_gutter, pixel_at,
};
use crate::stats::{Samples, Stats};
use bt_atlas::fixture::{CLUSTER_SCALE, ONE_CELL_WIDE_CHAR, PAIR_CHAR, STROKE_PAIR_CHAR};

/// Fixed cell for the synthetic scenes: 8×16, eight columns and four rows
/// on a 64 texture.
const CELL: (u16, u16) = (8, 16);

#[test]
fn wgsl_pipelines_build() {
    // The shader canary (`make shader`): the WGSL passes naga and every pipeline builds on a device requested
    // with Vulkan's immediate floor. If the WGSL side of an `Immediates`
    // block outgrows its pipeline layout, creation fails and `shared`'s
    // `expect` names the failing pipeline.
    let _ = Gpu::shared();
}

// **Guards that need the wgpu internals** (a hand-built pass, the colour
// plane's texture, the completion poll, validation scopes). The pixel
// guards that only need a frame live in `tests`. Glyph tests draw at
// `SCALE`.

/// The atlas fixture's cluster scale (Retina): at 13pt@1x a flag cluster's
/// ink exceeds two cells and falls back to its base character
/// (`a_cluster_is_one_color_glyph_on_every_surface`), so the scene list could
/// not show a cluster at 1x. The measurement is the backend's, in
/// `bt_atlas::fixture`.
const SCALE: f64 = CLUSTER_SCALE;

/// An inked cell with a white foreground (`tests`'s `glyph_cell`, with a
/// row).
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
    CellMetrics::new(cw, ch, m.context_cell_px(), 0, m.rule_px(), m.scale())
        .expect("non-zero metrics")
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
        slot_px: [f32::from(SLOT); 2],
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
        seen.0.abs_diff(MID.0) <= 1 && seen.1.abs_diff(MID.1) <= 1 && seen.2.abs_diff(MID.2) <= 1,
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
    // continue there pixel for pixel. The fixture's `STROKE_PAIR_CHAR` has
    // ink crossing the boundary (`一`, one horizontal stroke across nearly
    // the full em, where a CJK font is installed).
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
        ..glyph_cell(0, 0, STROKE_PAIR_CHAR)
    });
    let pixels = w.render_offscreen(EDGE, BACKGROUND, &frame);
    let edge = EDGE as usize;
    let px = |x, y| pixel_at(&pixels, edge, x, y);
    let clear = px(edge - 1, edge - 1);
    // It really is two cells wide, or the seam question is vacuous.
    let inked = |x| (0..ch).any(|y| px(x, y) != clear);
    assert!(
        inked(cw / 2) && inked(cw + cw / 2),
        "`{STROKE_PAIR_CHAR}` did not draw across two cells"
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

#[test]
fn the_climbing_dock_is_drawn_lower_by_its_rise() {
    // The scene's climb is a lever on the dock's viewports (`Renderer::plan`):
    // the ground and the top line move down together by `rise`, the window
    // above shows the clear colour where the dock was.
    const EDGE: u32 = 128;
    const RISE: f32 = 8.0;
    let w = Renderer::new();
    let m = flush_left(w.cell_metrics(SCALE));
    let ground = LinearRgba::from_srgb(0x00, 0x80, 0x00);
    let top_line = LinearRgba::from_srgb(0xff, 0xff, 0xff);
    let build = |scene: Option<crate::arrival::Scene>, rise: f32| {
        let mut frame = glyph_frame(m);
        frame.set_dock_input_rows(Some(1));
        frame.set_dock_band(EDGE as f32, 0.0);
        frame.open_dock(ground, top_line, ground);
        frame.set_dock_scene(scene, rise);
        frame.set_dock_fx(std::iter::empty(), &Clusters::default(), WHITE);
        frame
    };
    let rested = build(None, 0.0);
    let top = rested.band_top_px(EDGE as f32) as usize;
    let at_rest = w.render_offscreen(EDGE, BACKGROUND, &rested);
    let x = 40;
    assert_eq!(
        pixel_at(&at_rest, EDGE as usize, x, top),
        (0xff, 0xff, 0xff)
    );
    // Scene time 0.3 s: the ground is whole and the first line is drawn
    // across, so the only difference is where the dock stands.
    let lowered = build(Some(arrival_scene(0.3, 0, false)), RISE);
    assert_eq!(lowered.dock_rise_px(), RISE);
    let climbing = w.render_offscreen(EDGE, BACKGROUND, &lowered);
    let row = top + RISE as usize;
    assert_eq!(
        pixel_at(&climbing, EDGE as usize, x, row),
        (0xff, 0xff, 0xff),
        "the line is {RISE}px lower"
    );
    let clear = pixel_at(&at_rest, EDGE as usize, x, top - 2);
    assert_eq!(
        pixel_at(&climbing, EDGE as usize, x, top),
        clear,
        "the dock left its place"
    );
    let (_, g, _) = pixel_at(&climbing, EDGE as usize, x, row + 6);
    assert!(g > 0x70, "the ground moved with the line: {g:#x}");
}

#[test]
fn the_chevron_is_held_back_then_drawn_as_an_effect_then_static() {
    // The sigil is a rule sprite: while the scene owns it the static rule is
    // out of the list and the effect pipeline draws the same sprite
    // (`slots::fx_list`). Three moments, one cell: nothing, in flight, there.
    const EDGE: u32 = 128;
    let w = Renderer::new();
    let m = w.cell_metrics(SCALE);
    let cell_w = usize::from(m.cell_px().0);
    let ink = |scene: Option<crate::arrival::Scene>| {
        let frame = arriving_dock_frame(m, EDGE, scene, 0.0);
        let pixels = w.render_offscreen(EDGE, BACKGROUND, &frame);
        // The input row's first cell, everywhere below the dock's top.
        let top = frame.band_top_px(EDGE as f32) as usize;
        white_in(
            &pixels,
            EDGE,
            top..top + usize::from(m.cell_px().1) * 2,
            0..cell_w * 2,
        )
    };
    assert_eq!(
        ink(Some(arrival_scene(0.5, 0, true))),
        0,
        "the waiting dock shows a chevron"
    );
    let flying = ink(Some(arrival_scene(0.2, 12, false)));
    assert!(flying > 0, "the chevron in flight left no ink");
    let static_ink = ink(None);
    assert!(static_ink > 0, "the static chevron left no ink");
}

// **Completion model**: four jobs carried by the submission index
// and `poll` — `frames=` counts only finished frames, a failed frame goes to
// `Retry`, `startup=` closes on the first finished frame, and a frame in
// flight is polled before the link sleeps.

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
    // `frames=` counts frames the GPU finished without error — not
    // submitted ones — and the last frame before the link sleeps is not
    // lost: while it is in flight `in_flight` asks for one delayed poll,
    // that single poll counts it, and an empty queue arms nothing (the
    // stop condition). `startup=` closes at the first `Ok` the poll hands
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
    assert_eq!(r.last_bg_count(), 1, "`cells=` of the submitted frame");
    assert_eq!((r.last_glyph_count(), r.last_rule_count()), (0, 0));
    // No atlas was asked for: an unopened atlas has no slots (`(0, 0)`).
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
    // never tracked, so it can never be counted — otherwise `make smoke`
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
    assert_eq!(r.last_bg_count(), 0, "a failed frame pollutes `cells=`");
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
    // test pins the pipe). Absent: the
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

// **Scene list**: composed frames — every pipeline of a frame together, on
// the three surfaces — drawn by `every_scene_draws_all_its_pipelines_together`.
// Scenes without glyphs need no atlas; the others draw at the atlas's own
// metrics (`SCALE`).

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
    frame.set_dock_input_rows(Some(2));
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
/// drawn through the `cell` pipeline before the grounds.
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

/// Wide glyphs: two characters drawn as two halves (CJK where a CJK font is
/// installed; the fixture's pair characters), and one declared
/// wide whose ink fits one cell (one quad).
fn scene_wide(m: CellMetrics) -> Scene {
    let mut frame = glyph_frame(m);
    for (col, ch) in [
        (0, PAIR_CHAR),
        (2, STROKE_PAIR_CHAR),
        (4, ONE_CELL_WIDE_CHAR),
    ] {
        frame.push(Cell {
            wide: true,
            ..glyph_cell(col, 0, ch)
        });
    }
    ("wide glyph halves", 128, frame)
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
    frame.set_dock_input_rows(Some(1));
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
        ..glyph_cell(5, 0, PAIR_CHAR)
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
            // glyph is `tests`'s own guard.
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
            let name: &'static str = Box::leak(format!("{what} {id} at t = {t}").into_boxed_str());
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
    frame.set_dock_input_rows(Some(1));
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

/// The scroll bar over an offset grid and a fill band, under a dock: its
/// viewport at the window's origin between the band's and the dock's.
fn scene_scroll_bar() -> Scene {
    const EDGE: u32 = 128;
    let cell = grid(CELL.0, CELL.1);
    let mut frame = Frame::default();
    frame.clear(cell, CaretStyle::default());
    frame.push(bg_cell(0, 0, MIDTONE));
    frame.set_fill_rows(1);
    frame.push_fill(bg_cell(1, 0, ACCENT));
    frame.set_dock_input_rows(Some(1));
    frame.set_dock_band(EDGE as f32, 0.0);
    frame.open_dock(
        LinearRgba::from_srgb(0x20, 0x22, 0x28),
        WHITE,
        LinearRgba::from_srgb(0x60, 0x60, 0x60),
    );
    frame.set_origin_rows(1.5);
    let position = bt_core::ScrollPosition {
        room: 40,
        top: 12.5,
        visible: 4,
    };
    let floor = frame.band_top_px(EDGE as f32);
    let layout = crate::scrollbar::ScrollbarLayout::new(Some(position), EDGE as f32, floor, cell);
    // A match far up the history and the current one on the window's rows:
    // the marks' two `selection` draws over the thumb.
    let marks = [
        bt_core::TrackMark {
            position: 3.0,
            current: false,
        },
        bt_core::TrackMark {
            position: 14.0,
            current: true,
        },
    ];
    frame.set_scrollbar(
        layout,
        crate::scrollbar::Look::auto(0.8, 0.0, crate::scrollbar::THUMB_ALPHA),
        Theme::BATERI.foreground_linear(),
        &marks,
        [
            Theme::BATERI.search_mark_linear(),
            Theme::BATERI.search_current_mark_linear(),
        ],
        &[],
        [Theme::BATERI.foreground_linear(); 3],
    );
    assert!(frame.scrollbar().is_some(), "no bar in the scene");
    (
        "scroll bar and its marks between the band and the dock",
        EDGE,
        frame,
    )
}

/// The always-up scroll bar: its track and hairline (`cell_bg`) under the
/// wide thumb, over a grid that ends where the track starts.
fn scene_scroll_bar_always() -> Scene {
    const EDGE: u32 = 128;
    let cell = grid(CELL.0, CELL.1);
    let mut frame = Frame::default();
    frame.clear(cell, CaretStyle::default());
    frame.push(bg_cell(0, 0, MIDTONE));
    let position = bt_core::ScrollPosition {
        room: 40,
        top: 40.0,
        visible: 4,
    };
    let layout =
        crate::scrollbar::ScrollbarLayout::new(Some(position), EDGE as f32, EDGE as f32, cell);
    frame.set_scrollbar(
        layout,
        crate::scrollbar::Look::ALWAYS,
        Theme::BATERI.foreground_linear(),
        &[bt_core::TrackMark {
            position: 20.0,
            current: false,
        }],
        [
            Theme::BATERI.search_mark_linear(),
            Theme::BATERI.search_current_mark_linear(),
        ],
        // A block mark in its lane: the wide form's third `selection` draw.
        &[bt_core::TrackBlock {
            position: 30.0,
            color: 2,
            handle: bt_core::BlockHandle::default(),
        }],
        [
            Theme::BATERI.success_linear(),
            Theme::BATERI.accent_linear(),
            Theme::BATERI.error_linear(),
        ],
    );
    assert!(
        frame.scrollbar().is_some() && !frame.scrollbar_track().is_empty(),
        "no track in the scene"
    );
    (
        "always-up scroll bar over its track, marks in their lanes",
        EDGE,
        frame,
    )
}

/// The dock's arrival scene `since` seconds after a prompt that came with
/// `columns` of context text (`waiting` → before any prompt).
fn arrival_scene(since: f64, columns: u16, waiting: bool) -> crate::arrival::Scene {
    use crate::arrival::Arrival;
    let mut arrival = Arrival::new(bt_core::DockArrival::Type, 10.0, false);
    if !waiting {
        arrival.advance(11.2, true);
        arrival.arrive(11.2, true);
        arrival.note_letters(columns);
    }
    arrival.advance(11.2 + since, true);
    arrival.scene().expect("the scene is still armed")
}

/// A dock of one input row and the context row `~/src | main`, with the
/// prompt's ›, on a `EDGE`-high texture, under `scene` raised `rise` pixels.
fn arriving_dock_frame(
    m: CellMetrics,
    edge: u32,
    scene: Option<crate::arrival::Scene>,
    rise: f32,
) -> Frame {
    let mut frame = glyph_frame(m);
    frame.set_dock_input_rows(Some(1));
    frame.set_dock_band(edge as f32, 0.0);
    frame.push_dock_sigil(WHITE);
    for (col, ch) in (0u16..).zip("~/src | main".chars()) {
        if ch != ' ' {
            frame.push_dock(glyph_cell(col, 1, ch));
        }
    }
    // The lines share the ground's colour: what the pixels can then show is
    // ink, not hairlines.
    let ground = LinearRgba::from_srgb(0x20, 0x22, 0x28);
    frame.open_dock(ground, ground, ground);
    frame.set_dock_scene(scene, rise);
    frame.set_dock_fx(std::iter::empty(), &Clusters::default(), WHITE);
    frame
}

/// The ripple's scene: `since` seconds after a prompt at 1.2 s (`waiting` →
/// that long after birth, before any prompt).
fn ripple_scene(since: f64, waiting: bool) -> crate::arrival::Scene {
    use crate::arrival::Arrival;
    let mut arrival = Arrival::new(bt_core::DockArrival::Ripple, 10.0, false);
    if waiting {
        arrival.advance(10.0 + since, true);
    } else {
        arrival.advance(11.2, true);
        arrival.arrive(11.2, true);
        arrival.advance(11.2 + since, true);
    }
    arrival.scene().expect("the scene is still armed")
}

/// A scene whose wave is `wave`, whatever time says.
fn waved(mut scene: crate::arrival::Scene, wave: crate::arrival::Wave) -> crate::arrival::Scene {
    scene.wave = Some(wave);
    scene
}

/// A bare dock of one input row on an `EDGE`-high texture: opaque green
/// ground, a white top line, red for the wave's quiet tone and no letters
/// or › — what the pixels can show is the line.
fn waving_dock_frame(m: CellMetrics, edge: u32, scene: Option<crate::arrival::Scene>) -> Frame {
    let mut frame = glyph_frame(m);
    frame.set_dock_input_rows(Some(1));
    frame.set_dock_band(edge as f32, 0.0);
    let ground = LinearRgba::from_srgb(0x00, 0x80, 0x00);
    frame.open_dock(ground, WHITE, ground);
    frame.set_dock_quiet(LinearRgba::from_srgb(0xff, 0x00, 0x00));
    frame.set_dock_scene(scene, 0.0);
    frame.set_dock_fx(std::iter::empty(), &Clusters::default(), WHITE);
    frame
}

/// Pixels in `rows` (all columns, or `cols`) that are not the black clear
/// colour.
fn inked_in(
    pixels: &[u8],
    edge: u32,
    rows: std::ops::Range<usize>,
    cols: std::ops::Range<usize>,
) -> usize {
    rows.flat_map(|y| cols.clone().map(move |x| (x, y)))
        .filter(|&(x, y)| pixel_at(pixels, edge as usize, x, y) != (0, 0, 0))
        .count()
}

#[test]
fn a_flat_wave_is_the_docks_own_line_pixel_for_pixel() {
    // The swap between the wave and the dock's line is invisible only if a
    // wave with no height lands on the same row in the same colour, edge
    // pixels included. Whole frames compared: the line, the ground, the rest.
    const EDGE: u32 = 128;
    let w = Renderer::new();
    let m = flush_left(w.cell_metrics(SCALE));
    let flat = crate::arrival::Wave {
        alpha: 1.0,
        amp: 0.0,
        phase: 0.0,
        travel: 0.0,
        kick: 0.0,
        tone: 1.0,
    };
    let rested = w.render_offscreen(EDGE, BACKGROUND, &waving_dock_frame(m, EDGE, None));
    // Partway through the arrival: the ground is whole, the dock's own top line
    // is held back, the wave draws it.
    let scene = waved(ripple_scene(0.3, false), flat);
    let frame = waving_dock_frame(m, EDGE, Some(scene));
    assert!(frame.dock_wave(100.0, EDGE as f32).is_some());
    let wave = w.render_offscreen(EDGE, BACKGROUND, &frame);
    let top = frame.band_top_px(EDGE as f32) as usize;
    assert_eq!(
        pixel_at(&wave, EDGE as usize, 40, top),
        (0xff, 0xff, 0xff),
        "the line is on the dock's top row, in its colour"
    );
    assert_eq!(wave, rested, "a flat wave is today's line");
}

#[test]
fn a_wave_with_height_paints_beyond_its_row_and_the_ring_only_behind_its_front() {
    const EDGE: u32 = 128;
    let w = Renderer::new();
    let m = flush_left(w.cell_metrics(SCALE));
    let flat = crate::arrival::Wave {
        alpha: 1.0,
        amp: 0.0,
        phase: 0.0,
        travel: 0.0,
        kick: 0.0,
        tone: 1.0,
    };
    let top = waving_dock_frame(m, EDGE, None).band_top_px(EDGE as f32) as usize;
    let above = top - 10..top;
    let render = |wave| {
        let scene = waved(ripple_scene(0.3, false), wave);
        w.render_offscreen(EDGE, BACKGROUND, &waving_dock_frame(m, EDGE, Some(scene)))
    };
    let all = 0..EDGE as usize;
    // No height: nothing above the dock's top row.
    assert_eq!(inked_in(&render(flat), EDGE, above.clone(), all.clone()), 0);
    // A resting wave a quarter-cycle out of phase: crests and troughs both,
    // so the line climbs over the dock's top somewhere along the window.
    let resting = render(crate::arrival::Wave {
        amp: 1.8,
        phase: std::f32::consts::FRAC_PI_2,
        ..flat
    });
    assert!(
        inked_in(&resting, EDGE, above.clone(), all.clone()) > 0,
        "the wave stayed on its row"
    );
    // The ring is behind its front: ahead of it the line is flat.
    let (cell_w, _) = m.cell_px();
    let front = 64.0;
    let ring = render(crate::arrival::Wave {
        kick: 2.4,
        travel: (front - f32::from(cell_w) * 0.5) / m.scale() as f32,
        ..flat
    });
    let behind = inked_in(&ring, EDGE, above.clone(), 0..front as usize);
    let ahead = inked_in(&ring, EDGE, above, front as usize + 2..EDGE as usize);
    assert!(behind > 0, "the ring left no trace behind its front");
    assert_eq!(ahead, 0, "the line is disturbed ahead of the ring");
}

#[test]
fn a_waiting_ripple_draws_its_line_and_nothing_else() {
    // The waiting scene holds the ground and the lines back: what is on the
    // pixels is the wave, in its quiet tone, within its reach of the dock's
    // top row.
    const EDGE: u32 = 128;
    let w = Renderer::new();
    let m = flush_left(w.cell_metrics(SCALE));
    let frame = waving_dock_frame(m, EDGE, Some(ripple_scene(1.0, true)));
    let top = frame.band_top_px(EDGE as f32) as usize;
    let pixels = w.render_offscreen(EDGE, BACKGROUND, &frame);
    let reach = (crate::arrival::WAVE_REACH_PT * m.scale() as f32).ceil() as usize + 2;
    let all = 0..EDGE as usize;
    assert!(
        inked_in(&pixels, EDGE, top - reach..top + reach + 1, all.clone()) > 0,
        "no line"
    );
    assert_eq!(
        inked_in(&pixels, EDGE, 0..top - reach, all.clone()),
        0,
        "ink above the wave's reach"
    );
    assert_eq!(
        inked_in(&pixels, EDGE, top + reach + 1..EDGE as usize, all),
        0,
        "the held-back ground was drawn"
    );
    let (r, g, b) = (0..EDGE as usize)
        .flat_map(|x| (top - reach..top + reach + 1).map(move |y| (x, y)))
        .map(|(x, y)| pixel_at(&pixels, EDGE as usize, x, y))
        .max_by_key(|&(r, ..)| r)
        .expect("pixels");
    assert!(
        r > 0x40 && g == 0 && b == 0,
        "not the quiet tone: {r} {g} {b}"
    );
}

/// The dust scene: `since` seconds after a prompt at 1.2 s (`waiting` → that
/// long after birth, before any prompt).
fn dust_scene(since: f64, waiting: bool) -> crate::arrival::Scene {
    use crate::arrival::Arrival;
    let mut arrival = Arrival::new(bt_core::DockArrival::Dust, 10.0, false);
    if waiting {
        arrival.advance(10.0 + since, true);
    } else {
        arrival.advance(11.2, true);
        arrival.arrive(11.2, true);
        arrival.advance(11.2 + since, true);
    }
    arrival.scene().expect("the scene is still armed")
}

/// How much of the dock's ground the scene shows.
fn scene_band(scene: crate::arrival::Scene) -> f32 {
    scene.band
}

/// `scene` with its dust changed by `change`.
fn dust_changed(
    mut scene: crate::arrival::Scene,
    change: impl FnOnce(crate::arrival::dust::Dust) -> crate::arrival::dust::Dust,
) -> crate::arrival::Scene {
    scene.dust = Some(change(scene.dust.expect("the scene has dust")));
    scene
}

/// [`waving_dock_frame`] with the dust's colours: white motes, the accent.
fn dusting_dock_frame(m: CellMetrics, edge: u32, scene: Option<crate::arrival::Scene>) -> Frame {
    let mut frame = waving_dock_frame(m, edge, scene);
    frame.set_dock_dust_tones(WHITE, ACCENT, BACKGROUND);
    frame
}

/// The same pixels, window by window: how many of `a`'s and `b`'s differ in
/// `rows` (all columns).
fn differing_in(a: &[u8], b: &[u8], edge: u32, rows: std::ops::Range<usize>) -> usize {
    rows.flat_map(|y| (0..edge as usize).map(move |x| (x, y)))
        .filter(|&(x, y)| pixel_at(a, edge as usize, x, y) != pixel_at(b, edge as usize, x, y))
        .count()
}

#[test]
fn waiting_dust_paints_inside_its_zone_and_nowhere_else() {
    // The ground is held back while the shell starts, so what is on the pixels
    // is the dust: motes in the zone, from `ABOVE_PT` above the dock to its
    // floor, and nothing else — no light drawn behind them reaching higher.
    const EDGE: u32 = 256;
    let w = Renderer::new();
    let m = flush_left(w.cell_metrics(1.0));
    let scene = dust_scene(1.5, true);
    let frame = dusting_dock_frame(m, EDGE, Some(scene));
    let top = frame.band_top_px(EDGE as f32) as usize;
    let floor = (frame.dock_band_px() - 6.0).max(8.0) as usize;
    let above = crate::arrival::dust::ABOVE_PT as usize;
    assert!(top > above + 16, "the test window is too short: {top}");
    let pixels = w.render_offscreen(EDGE, BACKGROUND, &frame);
    let all = 0..EDGE as usize;
    assert!(
        inked_in(&pixels, EDGE, top - above..top + floor, all.clone()) > 20,
        "no dust in its zone"
    );
    assert_eq!(
        inked_in(&pixels, EDGE, 0..top - above - 8, all),
        0,
        "dust above its zone"
    );
}

#[test]
fn dust_still_on_its_way_shows_over_the_docks_ground() {
    // The dock's ground is opaque from the arrival's first moment; the motes
    // that are inside the band are drawn after it, or they would vanish.
    const EDGE: u32 = 256;
    let w = Renderer::new();
    let m = flush_left(w.cell_metrics(1.0));
    let with = dust_scene(0.1, false);
    let without = dust_changed(with, |dust| crate::arrival::dust::Dust {
        appear: 0.0,
        ..dust
    });
    let frame = dusting_dock_frame(m, EDGE, Some(with));
    let top = frame.band_top_px(EDGE as f32) as usize;
    assert_eq!(scene_band(with), 1.0, "the dock's ground is whole");
    let a = w.render_offscreen(EDGE, BACKGROUND, &frame);
    let b = w.render_offscreen(
        EDGE,
        BACKGROUND,
        &dusting_dock_frame(m, EDGE, Some(without)),
    );
    assert!(
        differing_in(&a, &b, EDGE, top + 1..EDGE as usize) > 0,
        "the dock's ground hid the dust"
    );
}

#[test]
fn dust_is_drawn_over_the_grids_ground() {
    // The grid's ground is under the dust (its text is over it): over a field
    // of ground cells the motes still show.
    const EDGE: u32 = 256;
    let w = Renderer::new();
    let m = flush_left(w.cell_metrics(1.0));
    let (cw, ch) = m.cell_px();
    let with = dust_scene(1.5, true);
    let without = dust_changed(with, |dust| crate::arrival::dust::Dust {
        appear: 0.0,
        ..dust
    });
    let render = |scene| {
        let mut frame = dusting_dock_frame(m, EDGE, Some(scene));
        let top = frame.band_top_px(EDGE as f32) as u16;
        for row in 0..top / ch {
            for col in 0..EDGE as u16 / cw {
                frame.push(bg_cell(col, row, MIDTONE));
            }
        }
        (frame, top as usize)
    };
    let (frame, top) = render(with);
    let (control, _) = render(without);
    let a = w.render_offscreen(EDGE, BACKGROUND, &frame);
    let b = w.render_offscreen(EDGE, BACKGROUND, &control);
    // The field is drawn: the control's pixels there are the ground.
    assert_eq!(
        pixel_at(&b, EDGE as usize, 40, 40),
        pixel_at(&b, EDGE as usize, 41, 41)
    );
    assert!(
        differing_in(&a, &b, EDGE, top.saturating_sub(120)..top) > 20,
        "the dust did not show over the grid's ground"
    );
}

#[test]
fn the_woven_line_ends_in_a_lit_tip_and_nothing_is_woven_ahead_of_it() {
    // Motes out of the way (zero opacity), a prompt 300 ms ago: the
    // line is drawn to the front, the accent's ramp trails it and the ground
    // is bare ahead of it.
    const EDGE: u32 = 256;
    let w = Renderer::new();
    let m = flush_left(w.cell_metrics(1.0));
    let scene = dust_changed(dust_scene(0.3, false), |dust| crate::arrival::dust::Dust {
        appear: 0.0,
        ..dust
    });
    let frame = dusting_dock_frame(m, EDGE, Some(scene));
    let top = frame.band_top_px(EDGE as f32) as usize;
    let front = (crate::arrival::dust::front_at(0.3) * EDGE as f32) as usize;
    assert!((20..EDGE as usize - 40).contains(&front), "{front}");
    let pixels = w.render_offscreen(EDGE, BACKGROUND, &frame);
    let at = |x: usize| pixel_at(&pixels, EDGE as usize, x, top);
    assert_eq!(at(front + 20), (0x00, 0x80, 0x00), "bare ground ahead");
    let tip = at(front - 3);
    assert!(
        tip.0 < 0xc0 && tip.2 > 0x90,
        "{tip:?}: the accent, not the white line"
    );
    assert!(at(2).0 > 0xe0, "the ramp is faint at its start");
    // Once the tip has faded only the woven line is left.
    let late = dust_changed(dust_scene(0.71, false), |dust| crate::arrival::dust::Dust {
        appear: 0.0,
        landing: Some(0.719),
        ..dust
    });
    let after = w.render_offscreen(EDGE, BACKGROUND, &dusting_dock_frame(m, EDGE, Some(late)));
    assert_eq!(
        pixel_at(&after, EDGE as usize, EDGE as usize - 3, top),
        (0xff, 0xff, 0xff),
        "the line is whole and white"
    );
}

/// Pixels in `rows` × `cols` (window pixels) that are close to white.
fn white_in(
    pixels: &[u8],
    edge: u32,
    rows: std::ops::Range<usize>,
    cols: std::ops::Range<usize>,
) -> usize {
    rows.flat_map(|y| cols.clone().map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let (r, g, b) = pixel_at(pixels, edge as usize, x, y);
            r > 0xa0 && g > 0xa0 && b > 0xa0
        })
        .count()
}

/// The dock under an arrival scene: the chevron as a rule effect over its
/// hidden static rule, the context letters typing out, the dock lowered by
/// its climb.
fn scene_dock_arrival(m: CellMetrics) -> Scene {
    const EDGE: u32 = 192;
    let frame = arriving_dock_frame(m, EDGE, Some(arrival_scene(0.2, 12, false)), 6.0);
    assert!(
        frame.dock_arrivals().iter().any(|fx| fx.rule.is_some()),
        "the scene did not put the chevron in flight"
    );
    assert!(
        frame.dock_arrivals().iter().any(|fx| fx.rule.is_none()),
        "the scene did not put a letter in flight"
    );
    (
        "dock arrival: chevron and letters in flight, climbing",
        EDGE,
        frame,
    )
}

/// The dock under the ripple: the line a wave with the ring spreading from
/// the ›, the › and the letters coming in.
fn scene_dock_wave(m: CellMetrics) -> Scene {
    const EDGE: u32 = 192;
    let scene = ripple_scene(0.15, false);
    assert!(scene.wave.is_some_and(|wave| wave.kick > 0.0));
    let mut frame = arriving_dock_frame(m, EDGE, Some(scene), 0.0);
    frame.set_dock_quiet(Theme::BATERI.quiet_linear());
    frame.set_dock_fx(std::iter::empty(), &Clusters::default(), WHITE);
    assert!(frame.dock_wave(100.0, EDGE as f32).is_some());
    (
        "dock arrival: the top line as a wave with the ring spreading",
        EDGE,
        frame,
    )
}

/// The dock under the dust: motes on their way to the top line, which is
/// being woven.
fn scene_dock_dust(m: CellMetrics) -> Scene {
    const EDGE: u32 = 256;
    let scene = dust_scene(0.2, false);
    assert!(scene.dust.is_some_and(|dust| dust.landing.is_some()));
    let mut frame = arriving_dock_frame(m, EDGE, Some(scene), 0.0);
    frame.set_dock_dust_tones(WHITE, ACCENT, BACKGROUND);
    frame.set_dock_fx(std::iter::empty(), &Clusters::default(), WHITE);
    (
        "dock arrival: dust pulled onto the line being woven",
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
        scene_dock_arrival(m),
        scene_dock_wave(m),
        scene_dock_dust(m),
        scene_growing_band_glyphs(m),
        scene_selection_corners(),
        scene_unfocused_selection(),
        scene_search(),
        scene_three_viewports(m),
        scene_scroll_bar(),
        scene_scroll_bar_always(),
    ];
    scenes.extend(scenes_fx(m));
    scenes
}

#[test]
fn every_scene_draws_all_its_pipelines_together() {
    // The scene list is the composed frames — three viewports, a band with
    // its caret, effects over the dock — that no single-pipeline guard in
    // `tests` draws. Each must go through the whole encode (validation
    // included: `render_offscreen` fails on any wgpu error) and leave ink
    // that is not the clear colour: a pass that silently drew nothing would
    // pass every other assertion here.
    let w = Renderer::new();
    let scenes = scenes(w.cell_metrics(SCALE));
    assert!(!scenes.is_empty());
    let clear = [0x00, 0xff, 0x00, 0xff];
    let clear_color = LinearRgba::from_srgb(clear[2], clear[1], clear[0]);
    for (name, edge, frame) in &scenes {
        let pixels = w.render_offscreen(*edge, clear_color, frame);
        assert_eq!(pixels.len(), (*edge as usize).pow(2) * 4, "{name}: size");
        assert!(
            pixels.chunks_exact(4).any(|px| px != clear),
            "{name}: the frame left only the clear colour"
        );
    }
}

// **Measurement hook**. Not part of `make check`; it is run on demand and its
// line is read. The line's keys
// (`backend=`, `frames=`, …) follow the project's token contract: never
// delete a key, only add. They were renamed once from Turkish (2026-10-01).

/// The hook's frame: a full grid (a ground in every cell), a caret and a
/// two-row dock — `cell_bg` and the caret only, so the numbers stay
/// comparable with the recorded rows.
fn loaded_frame(frame: &mut Frame, edge: u16) {
    let tints = [MIDTONE, ACCENT, BACKGROUND, WHITE];
    frame.clear(grid_with_gutter(CELL.0, CELL.1, 8), CaretStyle::default());
    let (cols, rows) = ((edge - 8) / CELL.0, edge / CELL.1 - 3);
    for row in 0..rows {
        for col in 0..cols {
            frame.push(bg_cell(col, row, tints[usize::from((col + row) % 4)]));
        }
    }
    frame.set_dock_input_rows(Some(1));
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

fn report(frames: usize, stats: &Stats, supported: bool) -> String {
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    let cpu = stats.cpu_frame();
    let gpu = stats.gpu();
    let gpu_line = if !supported {
        // No `TIMESTAMP_QUERY`: the key stays, the value says so.
        " gpu_p95=unsupported gpu_max=unsupported".to_owned()
    } else {
        span("gpu", gpu)
    };
    format!(
        "backend=wgpu profile={profile} frames={frames} samples={}{}{}{gpu_line}",
        cpu.nanos.len(),
        span("cpu_frame", cpu),
        span("cpu_encode", stats.cpu_encode()),
    )
}

#[test]
#[ignore = "measurement hook: run on demand"]
fn offscreen_frame_loop() {
    // The same frame, `FRAMES` times; the two CPU spans of `Stats`:
    // `cpu_frame` builds the frame (the run's noise witness), `cpu_encode` is
    // encode + submit. The GPU is waited for, but **outside** the spans, so
    // frames do not queue behind each other. The first `WARMUP` frames are
    // not recorded (warm-up, clock state).
    const EDGE: u16 = 1024;
    const FRAMES: usize = 1000;
    const WARMUP: usize = 50;
    let clear = BACKGROUND;
    let mut frame = Frame::default();

    let w = Renderer::new();
    w.set_gpu_timing(true);
    let target = w.target(u32::from(EDGE));
    let wgpu_stats = Stats::new(Instant::now(), 10);
    for i in 0..WARMUP + FRAMES {
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
    println!("{}", report(FRAMES, &wgpu_stats, w.gpu_timing_supported()));
}
