//! bt-gpu — the renderer (wgpu), shaders, the frame loop, motion, overlays.
//!
//! It takes "what to draw" from `bt-core` and does not know "what it means":
//! a branch recognising an escape sequence does not come in here
//! (`CLAUDE.md` → pitfalls). **No platform library in its direct
//! dependencies or source** (040): the GPU is reached through wgpu, and the
//! two platform jobs left — the window's layer and the vsync rhythm — are
//! handed in by `bt-shell`. The layer comes as a pointer through one `unsafe`
//! entry ([`Surface::from_layer`]); the rhythm as a [`Pacer`] (the tick, its
//! switch, one delayed wakeup and the time base). The device is set up by
//! [`Renderer::system_default`]; `bt-shell` sees no wgpu type.
//!
//! This crate drives the frame too: [`DisplayLink`] reads from `Session`,
//! draws and stops when damage runs out; `bt-shell` only wires the window,
//! the [`Pacer`] and the [`Waker`]. The cell metric passes through here as
//! well ([`Renderer::cell_metrics`]): its source is `bt-atlas`'s font metric,
//! but `bt-shell` does not see that crate, it sees [`CellMetrics`] — the
//! layer table lives with one edge instead of two. The atlas's **textures**
//! are this crate's too: `bt-atlas` gives slot numbers and CPU bitmaps, this
//! crate writes them to the textures and draws them with the `cell` pipeline.
//!
//! **There are six pipelines** and three share a vertex: `cell_bg`
//! (backgrounds, block stripes, the dock's ground), `cell` (glyphs and rules;
//! it samples the atlas), `caret` (`cell_bg_vertex` + `caret_fragment`),
//! `emoji` (`cell_vertex` + `emoji_fragment`), `glyph_fx` (the dock's typing
//! effects, its own vertex) and `selection` (the mouse selection's
//! round-cornered shape; its own vertex reading `Instance` verbatim, because
//! the fragment must know its quad). `caret` is separate because the caret's
//! round corner, edge and halo want an SDF and there is no point charging
//! that to thousands of background quads per frame; `glyph_fx` is separate
//! because its quad grows by the effect's margin and its instance carries the
//! effect's parameters. The effects' **timing** is here too (the `glyph_fx`
//! module, pure): `bt-core` says which glyph arrived, this crate how long it
//! lasts. The shaders are WGSL (`shaders/*.wgsl`); the `.metal` twins and the
//! Metal renderer remain only as the test oracle until 040 phase-7.
//!
//! The frame path's **measurement book** is here too ([`Stats`]): whoever
//! produces the time collects the sample — the CPU spans from the tick, the
//! GPU delta from the renderer's completion poll. This crate **prints**
//! nothing; `bt-shell` sets the book up and reads it at shutdown. The
//! statistics' rule is here too: the smallest sample count at which a p95 is
//! meaningful ([`MIN_SAMPLES`]) is defined next to the book, `bt-shell`
//! **prints** it as `taban=` but does not choose its value.

mod blink;
mod error;
mod frame;
mod glyph_fx;
mod link;
mod motion;
mod renderer;
// Slot resolution and fan-out shared by the Metal and wgpu renderers (040 phase-3).
mod slots;
mod stats;
mod surface;
// The product renderer (wgpu) since 040 phase-5; `renderer` keeps the Metal
// oracle behind `cfg(test)` until phase-7.
mod wgpu_renderer;

pub use error::GpuError;
pub use frame::{DOCK_ROWS, context_cols, context_row_offset, dock_px};
pub use link::{DisplayLink, Layout, Origin, Pacer, TickTarget, Ticker, Waker};
pub use renderer::{CellMetrics, FontNotice, family_notice};
pub use stats::{MIN_SAMPLES, Samples, Stats};
pub use surface::Surface;
pub use wgpu_renderer::Renderer;

/// Ayar penceresinin Font listesi: eşaralıklı aileler, `bt-atlas`'ın
/// zincirinin uyarısız açtıkları. Yeniden ihraç, çünkü `bt-shell`
/// `bt-atlas`'ı görmüyor ([`FontNotice`] emsali).
pub use bt_atlas::monospaced_families;
