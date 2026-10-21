//! The picture a tab's card shows of a pane that has a program's view over it.
//!
//! A pane's picture for the card is the terminal's own, drawn on the GPU from the
//! session (`TerminalPane::snapshot`). While a view covers the pane that picture is
//! the terminal **under** the view, which is not what the pane shows, so the card
//! takes the web view's own (`takeSnapshotWithConfiguration:`). That is not a frame
//! of the terminal and asks nothing of it: the terminal's still path stays as it is
//! (`bt_gpu::DisplayLink::snapshot`; a still never counts as a frame) and is not
//! called for a pane that shows a view.
//!
//! **Asked and kept, not waited for.** The web view answers on the main queue, later, and
//! the card asks every half second (`tabs::MAP_REFRESH`): a call asks for a new picture and
//! gives back the last one that came. The first call of a view has none, so a card that
//! opens on it shows an empty tile until its next turn. A view that cannot be shot (a
//! web process that is gone) leaves the last picture where it was; a request that never
//! comes back is asked again after [`PATIENCE`].
//!
//! **The tile is filled the way the terminal's is**: the picture covers it in both directions
//! and what spills over is cropped, keeping the left edge and the bottom
//! ([`bt_gpu::tile_crop`] — the one rule, so the card does not crop some panes at the
//! bottom and others in the middle). The web view makes the crop and the size itself
//! (`rect` and `snapshotWidth` of its configuration), so what crosses is the tile's pixels.
//! The view's area is the pane's less the strip, a little shorter than the tile's shape;
//! the strip is not in the picture.

use std::cell::RefCell;
use std::ptr::NonNull;
use std::rc::Rc;
use std::time::{Duration, Instant};

use objc2_app_kit::NSImage;
use objc2_core_foundation::CFRetained;
use objc2_core_graphics::CGImage;

/// How long a request for a picture may stay unanswered before the next call asks again. A
/// policy value, not a measurement: a snapshot takes milliseconds, and one that has not
/// come in two seconds is not coming.
pub(super) const PATIENCE: Duration = Duration::from_secs(2);

/// What the card has of the view: the last picture that came and whether one is on its way.
#[derive(Default)]
pub(super) struct Still {
    /// The last picture that came back.
    image: Option<CFRetained<CGImage>>,
    /// When the request that has not come back was made.
    asked: Option<Instant>,
    /// Counts the times the pictures were let go: an answer to an older request than the
    /// last release is not kept.
    generation: u64,
}

/// What the host and the request's completion share; the completion is a block the web view
/// holds, so it owns a copy of this and never the host.
pub(super) type Shared = Rc<RefCell<Still>>;

impl Still {
    /// The picture to give for this call, and whether to ask for a new one now.
    pub(super) fn call(
        shared: &Shared,
        now: Instant,
    ) -> (Option<CFRetained<CGImage>>, Option<u64>) {
        let mut still = shared.borrow_mut();
        let due = still
            .asked
            .is_none_or(|asked| now.saturating_duration_since(asked) >= PATIENCE);
        if due {
            still.asked = Some(now);
        }
        (still.image.clone(), due.then_some(still.generation))
    }

    /// A request that was made at `generation` came back with `image`, or with nothing.
    pub(super) fn answered(shared: &Shared, generation: u64, image: Option<CFRetained<CGImage>>) {
        let mut still = shared.borrow_mut();
        if still.generation != generation {
            return;
        }
        still.asked = None;
        if image.is_some() {
            still.image = image;
        }
    }

    /// The card is done with its pictures: the last one goes, and a request on its way will
    /// find that it was made before.
    pub(super) fn release(shared: &Shared) {
        let mut still = shared.borrow_mut();
        still.image = None;
        still.asked = None;
        still.generation += 1;
    }
}

/// What to ask the web view for, in points: the part of its page that fills the tile and
/// the width the picture comes back at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Capture {
    /// x, y, width and height in the page's coordinates (origin at the top left, as a web
    /// view's are).
    pub rect: [f64; 4],
    /// The width of the picture, in points: the tile's pixels over the pixels to the point.
    pub width: f64,
}

/// What to ask a web view whose page is `view` points across for a tile of `tile` pixels at
/// `scale` pixels to the point, or `None` for a view or a tile with nothing in it. The part
/// that fills the tile is the terminal's rule ([`bt_gpu::tile_crop`]) over the page's size in
/// pixels.
pub(super) fn capture(view: (f64, f64), scale: f64, tile: (u32, u32)) -> Option<Capture> {
    if !(view.0 >= 1.0 && view.1 >= 1.0 && scale > 0.0) || tile.0 == 0 || tile.1 == 0 {
        return None;
    }
    let pixels = |points: f64| (points * scale).round().max(1.0) as u32;
    let [u0, v0, u1, v1] = bt_gpu::tile_crop((pixels(view.0), pixels(view.1)), tile);
    let (u0, v0, u1, v1) = (f64::from(u0), f64::from(v0), f64::from(u1), f64::from(v1));
    Some(Capture {
        rect: [
            u0 * view.0,
            v0 * view.1,
            (u1 - u0) * view.0,
            (v1 - v0) * view.1,
        ],
        width: f64::from(tile.0) / scale,
    })
}

/// The `CGImage` of what the web view gave back.
pub(super) fn cg_image(image: &NSImage) -> Option<CFRetained<CGImage>> {
    // SAFETY: no proposed rectangle, no context and no hints: the image's own best
    // representation, which is what `proposedDestRect: nil` asks for.
    let image =
        unsafe { image.CGImageForProposedRect_context_hints(std::ptr::null_mut(), None, None) }?;
    // SAFETY: the pointer is the live image's; the new owner takes its own count on it and
    // the `Retained` gives back the one it had.
    Some(unsafe { CFRetained::retain(NonNull::from(&*image)) })
}
