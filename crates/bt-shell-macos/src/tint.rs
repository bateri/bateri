//! A colour with its strength ([`Tint`]) and the pictures a layer is given ([`paint_in`]): the
//! small drawing words the window's modules and the pane's share.

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2_app_kit::{NSColor, NSImage};
use objc2_foundation::{NSRect, NSSize};
use objc2_quartz_core::{CALayer, CATransaction};

/// An sRGB colour with its strength.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Tint {
    pub(crate) rgb: u32,
    pub(crate) alpha: f64,
}

impl Tint {
    pub(crate) const fn of(rgb: u32, alpha: f64) -> Self {
        Self { rgb, alpha }
    }

    pub(crate) fn color(self) -> Retained<NSColor> {
        let byte = |shift: u32| f64::from((self.rgb >> shift) & 0xff) / 255.0;
        NSColor::colorWithSRGBRed_green_blue_alpha(byte(16), byte(8), byte(0), self.alpha)
    }
}

/// `layer`'s picture: what `draw` draws in a picture of `size` (it is given the rect to fill, a
/// flipped one), at `scale`, laid in without Core Animation's implicit fade.
pub(crate) fn paint_in(layer: &CALayer, size: NSSize, scale: f64, draw: impl Fn(NSRect) + 'static) {
    let block = RcBlock::new(move |rect: NSRect| -> Bool {
        draw(rect);
        Bool::YES
    });
    let image = NSImage::imageWithSize_flipped_drawingHandler(size, true, &block);
    let picture = image.layerContentsForContentsScale(scale);
    CATransaction::begin();
    CATransaction::setDisableActions(true);
    layer.setContentsScale(scale);
    // SAFETY: what `NSImage::layerContentsForContentsScale` returns is what `contents` takes.
    unsafe { layer.setContents(Some(&picture)) };
    CATransaction::commit();
}
