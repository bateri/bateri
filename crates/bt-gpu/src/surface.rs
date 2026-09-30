//! The window surface: a wgpu surface over the `CAMetalLayer` that `bt-shell`
//! owns (040 → Karar 8).
//!
//! `bt-shell` creates the layer, hangs it on the view and sets its scale
//! (`contentsScale`); this crate opens a wgpu surface on it through **one**
//! `unsafe` entry ([`Surface::from_layer`]) and configures its pixel size.
//! `bt-shell` never sees a wgpu type. On Linux the same entry will take
//! winit's `raw-window-handle` (the winit set).

use std::cell::Cell;
use std::ffi::c_void;
use std::ptr::NonNull;

use crate::renderer::{FORMAT, Gpu, Target};
use crate::{GpuError, Renderer};

/// A window's surface: the swap chain the frame path draws into.
///
/// Main-thread only by use, like the layer under it: `bt-shell` resizes it
/// from the view's geometry, the tick acquires from it, both on the main
/// thread.
pub struct Surface {
    surface: wgpu::Surface<'static>,
    gpu: &'static Gpu,
    /// The configured size in pixels; `None` → not configured yet (no size
    /// known, or a degenerate one) and the tick does not draw.
    size: Cell<Option<(u32, u32)>>,
    /// The last acquisition failed (`Lost`/`Validation`): the next one
    /// configures again first. Without it the window would stay dead until a
    /// resize, because [`Surface::set_size`] skips an unchanged size.
    stale: Cell<bool>,
}

/// What [`Surface::acquire`] got — three classes, three answers (the tick
/// handles each in `crate::link`).
pub(crate) enum Acquired {
    /// A texture to draw into and present.
    Frame(wgpu::SurfaceTexture),
    /// Not a fault: the window is occluded or the drawable did not come in
    /// time. The frame is skipped quietly; the damage is kept for the next
    /// request.
    Skip,
    /// The surface is lost or failed validation: an error for the frame
    /// policy (`Retry::draw_failed`).
    Failed(GpuError),
}

impl Surface {
    /// Opens a surface on `layer`.
    ///
    /// # Safety
    ///
    /// `layer` must point to a live `CAMetalLayer`. wgpu retains it, so the
    /// caller need not keep it alive beyond this call — but the layer is the
    /// view's, and drawing into a layer no view shows is harmless, not unsafe.
    pub unsafe fn from_layer(
        renderer: &Renderer,
        layer: NonNull<c_void>,
    ) -> Result<Self, GpuError> {
        let gpu = renderer.gpu();
        // SAFETY: the caller guarantees a live `CAMetalLayer` (this function's
        // contract); wgpu-hal asserts the class and retains it.
        let surface = unsafe {
            gpu.instance()
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(
                    layer.as_ptr(),
                ))
        }
        .map_err(|e| GpuError::Wgpu(e.to_string()))?;
        Ok(Self {
            surface,
            gpu,
            size: Cell::new(None),
            stale: Cell::new(false),
        })
    }

    /// The view's backing size changed; size in **pixels**. The scale is
    /// the layer owner's (`bt-shell` sets `contentsScale`).
    ///
    /// Configures only when the size really changed and is not degenerate:
    /// wgpu's `configure` waits for the queue to drain (wgpu-core's
    /// `configure_surface`), and a minimised window reports zero.
    pub fn set_size(&self, width_px: f64, height_px: f64) {
        // `as u32` saturates: NaN and negatives give zero, which is skipped.
        let (width, height) = (width_px.round() as u32, height_px.round() as u32);
        if width == 0 || height == 0 || self.size.get() == Some((width, height)) {
            return;
        }
        self.configure(width, height);
        self.size.set(Some((width, height)));
    }

    fn configure(&self, width: u32, height: u32) {
        self.surface.configure(
            self.gpu.device(),
            &wgpu::SurfaceConfiguration {
                // Render target only: the counterpart of the old layer's
                // `framebufferOnly = true` (wgpu-hal sets it exactly when the
                // usage is the colour target alone).
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                // The single target format (`FORMAT`): the fragment writes
                // linear, the hardware encodes sRGB (`CLAUDE.md` → colour
                // space).
                format: FORMAT,
                // `Auto` resolves to sRGB for this format, which resets the
                // layer to its default colour space — what the old layer had.
                color_space: wgpu::SurfaceColorSpace::Auto,
                width,
                height,
                // Vsync-locked presentation: the old layer's
                // `displaySyncEnabled` default.
                present_mode: wgpu::PresentMode::Fifo,
                // Two frames in flight → three drawables, the old layer's
                // `maximumDrawableCount` default.
                desired_maximum_frame_latency: 2,
                // `PostMultiplied` leaves the layer non-opaque, as it was;
                // `Opaque` would flip `opaque` on the layer. Every pixel is
                // written with alpha 1 either way (the clear is opaque and the
                // alpha blend keeps it at 1), so the choice is about not
                // changing a layer property silently.
                alpha_mode: wgpu::CompositeAlphaMode::PostMultiplied,
                view_formats: Vec::new(),
            },
        );
    }

    fn reconfigure(&self) {
        if let Some((width, height)) = self.size.get() {
            self.configure(width, height);
        }
    }

    /// Whether a size was configured; the tick does not draw before that.
    pub(crate) fn is_configured(&self) -> bool {
        self.size.get().is_some()
    }

    /// This frame's texture. `Outdated`/`Suboptimal` reconfigure and try
    /// once more (the layer's size moved under the swap chain); a second
    /// miss is skipped, not failed.
    ///
    /// A `Lost`/`Validation` result marks the surface stale and the next call
    /// reconfigures before acquiring. (A lost surface proper would need a new
    /// surface from the layer; the Metal backend never reports one.)
    pub(crate) fn acquire(&self) -> Acquired {
        if self.stale.replace(false) {
            self.reconfigure();
        }
        for attempt in 0..2 {
            match self.surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(texture) => return Acquired::Frame(texture),
                wgpu::CurrentSurfaceTexture::Suboptimal(texture) if attempt == 1 => {
                    return Acquired::Frame(texture);
                }
                wgpu::CurrentSurfaceTexture::Suboptimal(texture) => {
                    // The texture must be gone before `configure`: wgpu
                    // rejects configuring while an output exists
                    // (`PreviousOutputExists`) and leaves the surface
                    // unconfigured.
                    drop(texture);
                    self.reconfigure();
                }
                wgpu::CurrentSurfaceTexture::Outdated => self.reconfigure(),
                wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                    return Acquired::Skip;
                }
                wgpu::CurrentSurfaceTexture::Lost => {
                    self.stale.set(true);
                    return Acquired::Failed(GpuError::Wgpu("surface lost".to_owned()));
                }
                wgpu::CurrentSurfaceTexture::Validation => {
                    self.stale.set(true);
                    return Acquired::Failed(GpuError::Wgpu(
                        "surface acquisition failed validation".to_owned(),
                    ));
                }
            }
        }
        Acquired::Skip
    }
}

/// The acquired texture as a render target.
pub(crate) fn target(frame: &wgpu::SurfaceTexture) -> Target {
    Target::new(frame.texture.clone())
}
