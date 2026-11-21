//! What a browser of a page shows. The engine draws in another process and its frames are the
//! shell's (textures of the renderer's device, which only the shell can make); the page's list
//! has a place for each browser ([`pencere_view::BrowserSlot`]) and the list's turn into the
//! renderer's lets the shell put the picture there, in the order the list holds it in: what the
//! page draws over the room (an overlay, a bar that scrolls) covers it as it does a picture.

use bt_gpu::{Renderer, UiList};
use pencere_view::BrowserSlot;

/// The pictures of a page's browsers, held by the shell and read when the page's frame is drawn.
/// The frames come from another thread, so the holder is shared.
pub trait Surfaces: Send + Sync {
    /// Puts the picture of the browser `slot` names into `list`, in the slot's room and held to
    /// what shows of it, `origin` pixels from the surface's corner ([`BrowserSlot::rect`] is the
    /// page's own pixels). Nothing where the browser has drawn nothing yet: the room's fill shows.
    fn paint(&self, renderer: &Renderer, list: &mut UiList, slot: &BrowserSlot, origin: (f32, f32));
}
