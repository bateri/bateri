//! What asks for a page's frames.
//!
//! A page shown over the terminal ([`super::DisplayLink::set_page`]) is drawn when *its own*
//! state asks for a frame — it was put up, the surface or the font changed, the pointer or a key
//! changed what it shows, an animation of its is still moving — and never because the terminal
//! under it printed something. The terminal's output reaches the link through the [`Waker`]'s
//! shared damage flag from the reader's thread; this flag is another, set on the main thread by
//! the page's own paths only, so the two cannot be mistaken for each other.
//!
//! A page at rest may also name a time it wants a frame at ([`super::UiPage::wake_at`]): the link
//! keeps it here and arms one delayed wakeup for it when it goes to sleep, and the tick that
//! comes at or after that time draws the page.
//!
//! [`Waker`]: super::Waker

use std::cell::Cell;

/// The page's request for its next frame.
#[derive(Debug, Default)]
pub(super) struct PageFrames {
    wanted: Cell<bool>,
    /// The time the page at rest asked to be drawn at, on the ticks' clock.
    due: Cell<Option<f64>>,
}

impl PageFrames {
    /// The page needs a frame: a path of its own asks (main thread).
    pub(super) fn ask(&self) {
        self.wanted.set(true);
    }

    /// Whether this tick, at `now`, draws the page, and the request is spent: someone asked since
    /// the last frame, the time the page asked for has come, or `theme_changed` (the theme is told
    /// to the terminal's session, not to the page).
    pub(super) fn take(&self, theme_changed: bool, now: f64) -> bool {
        let due = self.due.get().is_some_and(|at| now >= at);
        if due {
            self.due.set(None);
        }
        self.wanted.replace(false) || due || theme_changed
    }

    /// The time the page drawn last wants its next frame at, if it does (`None` forgets the one
    /// it asked for before).
    pub(super) fn wake_at(&self, at: Option<f64>) {
        self.due.set(at);
    }

    /// The time a wakeup is to be armed for when the link goes to sleep.
    pub(super) fn due(&self) -> Option<f64> {
        self.due.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_is_drawn_for_a_request_of_its_own_and_once() {
        let frames = PageFrames::default();
        assert!(!frames.take(false, 0.0), "nothing asked: no frame");
        frames.ask();
        assert!(frames.take(false, 0.0));
        assert!(
            !frames.take(false, 0.0),
            "the request is spent by the frame"
        );
    }

    #[test]
    fn a_new_theme_is_a_reason_without_a_request() {
        let frames = PageFrames::default();
        assert!(frames.take(true, 0.0));
    }

    #[test]
    fn requests_are_one_frame_however_many_came() {
        let frames = PageFrames::default();
        for _ in 0..50 {
            frames.ask();
        }
        assert!(frames.take(false, 0.0));
        assert!(!frames.take(false, 0.0));
    }

    #[test]
    fn a_page_that_names_a_time_is_drawn_once_when_it_comes() {
        let frames = PageFrames::default();
        frames.wake_at(Some(10.0));
        assert!(!frames.take(false, 9.5), "not yet");
        assert_eq!(frames.due(), Some(10.0), "kept for the wakeup");
        assert!(frames.take(false, 10.2));
        assert!(!frames.take(false, 11.0), "the time is spent by the frame");
        assert_eq!(frames.due(), None);
        frames.wake_at(Some(20.0));
        frames.wake_at(None);
        assert!(
            !frames.take(false, 30.0),
            "a page that asks for nothing more is not drawn"
        );
    }
}
