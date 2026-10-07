//! bt-gpu — the renderer (wgpu), shaders, the frame loop, motion, overlays.
//!
//! It takes "what to draw" from `bt-core` and does not know "what it means":
//! a branch recognising an escape sequence does not come in here.
//! **No platform library in its direct
//! dependencies or source**: the GPU is reached through wgpu, and the
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
//! lasts. The **scroll bar** needs no pipeline of its own: its thumb is one
//! rounded quad from `caret_fragment` (the dock buttons' road) and its track
//! two square ones from `cell_bg`, drawn in a viewport at the window's origin;
//! its sizes, timing, forms and layout are the `scrollbar` module's, pure —
//! `bt-core` says where the window stands in the scrollback, this crate where
//! that lands on the window and when it shows, and the track's width is the
//! room `bt-shell`'s grid gives up in the always-up form
//! ([`ScrollbarMode::reserve_px`], the [`DOCK_ROWS`] discipline) and the
//! pointer's region ([`scrollbar_strip_px`], [`ScrollbarLayout::contains`]).
//! The shaders are WGSL (`shaders/*.wgsl`), embedded with `include_str!`;
//! there is no shader build step.
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
mod metrics;
mod motion;
mod renderer;
mod scrollbar;
mod slots;
mod stats;
mod surface;

pub use error::GpuError;
pub use frame::{DOCK_ROWS, context_cols, context_row_offset, dock_px};
pub use link::{DisplayLink, Layout, Origin, Pacer, TickTarget, Ticker, Waker};
pub use metrics::{CellMetrics, FontNotice, family_notice};
pub use renderer::Renderer;
pub use scrollbar::{Mode as ScrollbarMode, ScrollbarLayout, strip_px as scrollbar_strip_px};
pub use stats::{MIN_SAMPLES, Samples, Stats};
pub use surface::Surface;

/// The settings window's Font list: the monospaced families that `bt-atlas`'s
/// chain opens without a notice. Re-exported because `bt-shell` does not see
/// `bt-atlas` (the precedent is [`FontNotice`]).
pub use bt_atlas::monospaced_families;
