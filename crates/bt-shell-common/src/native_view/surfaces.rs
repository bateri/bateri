//! What a browser of a page shows. The engine draws in another process and its frames are the
//! shell's (textures of the renderer's device, which only the shell can make); the page's list
//! has a place for each browser ([`pencere_view::BrowserSlot`]) and the list's turn into the
//! renderer's lets the shell put the picture there, in the order the list holds it in: what the
//! page draws over the room (an overlay, a bar that scrolls) covers it as it does a picture.

use bt_gpu::{Renderer, UiList, UiRect};
use pencere_view::BrowserSlot;

/// The pictures of a page's browsers, held by the shell and read when the page's frame is drawn.
/// The frames come from another thread, so the holder is shared.
pub trait Surfaces: Send + Sync {
    /// Puts the picture of the browser `slot` names into `list`, in the slot's room and held to
    /// what shows of it, `origin` pixels from the surface's corner ([`BrowserSlot::content`] is the
    /// page's own pixels; see [`browser_room`]). Nothing where the browser has drawn nothing yet: the
    /// room's fill shows.
    fn paint(&self, renderer: &Renderer, list: &mut UiList, slot: &BrowserSlot, origin: (f32, f32));
}

/// Where a browser's picture is drawn, `origin` pixels from the surface's corner: the content box of
/// its element, as a replaced element's room is in a style sheet. The border and the padding the
/// element has are the page's to draw around it, and the browser's frame does not reach under them.
pub fn browser_room(slot: &BrowserSlot, origin: (f32, f32)) -> UiRect {
    UiRect::new(
        slot.content.x + origin.0,
        slot.content.y + origin.1,
        slot.content.w,
        slot.content.h,
    )
}

#[cfg(test)]
mod tests {
    use pencere_view::{Clip, Id, Rect};

    use super::*;

    #[test]
    fn a_browsers_picture_is_drawn_in_the_content_box_of_its_element() {
        // An element 100 by 60 with a border of 2 and a padding of 4: the room is what is left.
        let frame = Rect::new(10.0, 20.0, 100.0, 60.0);
        let slot = BrowserSlot {
            id: Id::new(7),
            rect: frame,
            padding_box: Rect::new(12.0, 22.0, 96.0, 56.0),
            content: Rect::new(16.0, 26.0, 88.0, 48.0),
            clip: Clip::new(frame),
            opacity: 1.0,
        };
        assert_eq!(
            browser_room(&slot, (3.0, 32.0)),
            UiRect::new(19.0, 58.0, 88.0, 48.0),
            "the content box, moved by the origin: not the border box"
        );
    }
}
