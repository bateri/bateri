//! Çizim yüzeyi: `CAMetalLayer`'ın sahibi bt-gpu'dur; bt-shell yalnız `&CALayer` alır.

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
// NSSize, CGSize'ın takma adı; ayrı bir core-foundation crate'i gerekmez.
use objc2_foundation::NSSize;
use objc2_metal::{MTLDevice, MTLPixelFormat};
use objc2_quartz_core::{CALayer, CAMetalLayer};

pub struct Surface {
    layer: Retained<CAMetalLayer>,
}

impl Surface {
    pub(crate) fn new(
        device: &ProtocolObject<dyn MTLDevice>,
        pixel_format: MTLPixelFormat,
    ) -> Self {
        let layer = CAMetalLayer::new();
        layer.setDevice(Some(device));
        layer.setPixelFormat(pixel_format);
        // Doku yalnız render hedefi; okuma/örnekleme yok. Metal bunu bilince
        // drawable'ı daha ucuz ayırır.
        layer.setFramebufferOnly(true);
        Self { layer }
    }

    /// bt-shell'in `NSView`'a takacağı şey.
    pub fn ca_layer(&self) -> &CALayer {
        &self.layer
    }

    /// Pencere boyutu ya da ölçeği değişince; boyut piksel cinsinden.
    pub fn set_size(&self, width_px: f64, height_px: f64, scale: f64) {
        self.layer.setContentsScale(scale);
        self.layer.setDrawableSize(NSSize::new(width_px, height_px));
    }

    pub(crate) fn layer(&self) -> &CAMetalLayer {
        &self.layer
    }
}
