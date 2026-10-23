//! What asks for a page's frames.
//!
//! A page shown over the terminal ([`super::DisplayLink::set_page`]) is drawn when *its own*
//! state asks for a frame — it was put up, the surface or the font changed, the pointer or a key
//! changed what it shows, an animation of its is still moving — and never because the terminal
//! under it printed something. The terminal's output reaches the link through the [`Waker`]'s
//! shared damage flag from the reader's thread; this flag is another, set on the main thread by
//! the page's own paths only, so the two cannot be mistaken for each other.
//!
//! [`Waker`]: super::Waker

use std::cell::Cell;

/// The page's request for its next frame.
#[derive(Debug, Default)]
pub(super) struct PageFrames {
    wanted: Cell<bool>,
}

impl PageFrames {
    /// The page needs a frame: a path of its own asks (main thread).
    pub(super) fn ask(&self) {
        self.wanted.set(true);
    }

    /// Whether this tick draws the page, and the request is spent: someone asked since the last
    /// frame, or `theme_changed` (the theme is told to the terminal's session, not to the page).
    pub(super) fn take(&self, theme_changed: bool) -> bool {
        self.wanted.replace(false) || theme_changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_is_drawn_for_a_request_of_its_own_and_once() {
        let frames = PageFrames::default();
        assert!(!frames.take(false), "nothing asked: no frame");
        frames.ask();
        assert!(frames.take(false));
        assert!(!frames.take(false), "the request is spent by the frame");
    }

    #[test]
    fn a_new_theme_is_a_reason_without_a_request() {
        let frames = PageFrames::default();
        assert!(frames.take(true));
    }

    #[test]
    fn requests_are_one_frame_however_many_came() {
        let frames = PageFrames::default();
        for _ in 0..50 {
            frames.ask();
        }
        assert!(frames.take(false));
        assert!(!frames.take(false));
    }
}
