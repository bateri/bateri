//! How far the boxes of a page in a pane are scrolled: the pane's, not the page's
//! ([`pencere_view::Scrolls`]). The wheel and the trackpad move the box under the pointer at once,
//! on the page as it was last laid out, the way the system moves its own views (the deltas carry
//! the person's speed, direction and momentum; the [`Phase`] says which part of a gesture each
//! is); the bars are drawn as the pane's scroll bar is ([`Scrolling`]) and answer the pointer
//! before the page does; the page is told where its boxes went and asks for one to go somewhere.

use std::time::Instant;

use pencere_view::{Bars, Input, Laid, Phase, Point, ScrollTo, Scrolls, Wheeled};

/// How a pane's page scrolls on this system: how its bars are drawn (as the pane's own scroll
/// bar is: the setting, or the system's), whether a gesture pulls a box past its end, and what
/// a click in a bar's track does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scrolling {
    pub bars: Bars,
    pub elastic: bool,
    /// A click in the track takes the box to that place (`false`: a page toward it).
    pub track_jumps: bool,
}

impl Default for Scrolling {
    fn default() -> Self {
        Self {
            bars: Bars::Overlay,
            elastic: false,
            track_jumps: false,
        }
    }
}

/// The pane's scrolling of a page, on the clock its frames are built by.
#[derive(Debug)]
pub(crate) struct HostScrolls {
    pub(crate) scrolls: Scrolls,
    /// The time of the last frame, on the frames' clock, and when that was here: an input
    /// between frames is put on the same clock ([`HostScrolls::clock`]), so that a box let go
    /// long after the last frame springs back from then and not from that frame.
    frame: f64,
    at: Instant,
}

impl HostScrolls {
    pub(crate) fn new() -> Self {
        Self {
            scrolls: Scrolls::new(),
            frame: 0.0,
            at: Instant::now(),
        }
    }

    pub(crate) fn set(&mut self, scrolling: Scrolling) {
        self.scrolls.set_bars(scrolling.bars);
        self.scrolls.set_elastic(scrolling.elastic);
        self.scrolls.set_track_jumps(scrolling.track_jumps);
    }

    /// A frame is being built at `now`.
    pub(crate) fn frame(&mut self, now: f64) {
        self.frame = now;
        self.at = Instant::now();
    }

    /// Now, on the frames' clock.
    pub(crate) fn clock(&self) -> f64 {
        self.frame + self.at.elapsed().as_secs_f64()
    }

    /// The wheel over `at` (the page's points) on the page `laid`: whether a box took it, and
    /// what of it is the page's.
    pub(crate) fn wheel(
        &mut self,
        laid: &Laid,
        at: Point,
        dx: f32,
        dy: f32,
        phase: Phase,
    ) -> Wheeled {
        let now = self.clock();
        self.scrolls.wheel(laid, at, dx, dy, phase, now)
    }

    /// A pointer input (the page's points) offered to the bars first: whether a bar took it (it
    /// is not the page's), and whether a frame is worth drawing for what the bars did.
    pub(crate) fn pointer(&mut self, input: &Input) -> (bool, bool) {
        let now = self.clock();
        match *input {
            Input::Press { at, .. } => {
                let took = self.scrolls.press(at, now);
                (took, took)
            }
            Input::Move(at) => {
                let holding = self.scrolls.holding();
                let moved = self.scrolls.point(Some(at), now);
                (holding, moved)
            }
            Input::Release { .. } => {
                let took = self.scrolls.release(now);
                (took, took)
            }
            Input::Leave => {
                let holding = self.scrolls.holding();
                (holding, self.scrolls.point(None, now))
            }
            Input::Scroll { .. } | Input::Key { .. } | Input::KeyUp { .. } | Input::Mods(_) => {
                (false, false)
            }
        }
    }

    pub(crate) fn ask(&mut self, asks: impl IntoIterator<Item = ScrollTo>) {
        for ask in asks {
            self.scrolls.ask(ask);
        }
    }

    /// Whether anything of the scrolling is on its way (a box springing back or gliding, bars
    /// going): the pane asks for the next frame.
    pub(crate) fn moving(&self) -> bool {
        self.scrolls.moving(self.frame)
    }

    /// When the bars start to go, if nothing else moves.
    pub(crate) fn wake_at(&self) -> Option<f64> {
        self.scrolls.wake_at(self.frame)
    }
}
