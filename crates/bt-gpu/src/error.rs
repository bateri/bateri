//! The GPU path's error type; every variant says what went wrong by name.

use std::fmt;

/// Errors of the GPU path. Each one names itself: `make smoke`'s red line and
/// `bateri` main's stderr line come from here, never a silent `None`.
#[derive(Debug)]
pub enum GpuError {
    /// A frame with glyphs reached a renderer whose atlas was never built.
    ///
    /// The scale half of the atlas's key comes from the window, and the only
    /// place that builds it is `Renderer::cell_metrics`. This error means
    /// "tried to draw glyphs without saying the scale"; the alternative was to
    /// invent a @1x atlas and draw at the wrong size **silently**.
    NoAtlas,
    /// wgpu reported an error: no adapter or device at start-up, a validation
    /// or out-of-memory error caught around a frame's submit (synchronous),
    /// or a device fault seen when the frame's completion is polled
    /// (asynchronous). The text is wgpu's own.
    Wgpu(String),
}

impl fmt::Display for GpuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoAtlas => write!(
                f,
                "atlas not built: cell_metrics(scale) must be called first"
            ),
            Self::Wgpu(message) => write!(f, "wgpu error: {message}"),
        }
    }
}

impl std::error::Error for GpuError {}
