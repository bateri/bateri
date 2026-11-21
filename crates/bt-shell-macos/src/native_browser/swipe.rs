//! The two-finger sideways swipe over a browser room, the way Chrome on a Mac handles it ("Swipe
//! between pages"): the page has the gesture when it, or the box under the fingers, can scroll
//! that way; when it cannot (it is at its edge that way) the gesture is history's. An arrow shows
//! at the edge and fills as the fingers move; let go far enough and the browser goes back (or
//! forward), let go early and the arrow withdraws.
//!
//! The system's own tracking (`trackSwipeEventWithOptions:`) has to be started inside the scroll
//! event it is for, and whether the page scrolls is only known a moment later, from the helper
//! (`Command::CanScroll`): so the gesture is followed here. The events go to the engine until the
//! helper says the gesture is history's (a page that cannot scroll that way does nothing with
//! them), and are kept from it from then on, momentum included.
//!
//! Pure: the events go in, what to do comes out, so that it is tested without a window.

/// How far the fingers have to go, in points, for the arrow to be full and the swipe to count.
/// Chosen, not measured: a deliberate swipe is a hand's width, an overshoot at the end of a
/// scroll is less than half of this.
pub(super) const FULL: f64 = 150.0;
/// How much of a gesture is read before it is called sideways or not, in points.
const DECIDE: f64 = 6.0;
/// A gesture is sideways when it went this many times further across than up or down.
const SIDEWAYS: f64 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Way {
    Back,
    Forward,
}

impl Way {
    /// The engine's sign of a wheel (`Wheel::dx`): positive moves the content to the right, which
    /// is the fingers going to the right, which is back.
    pub(super) fn of(dx: f64) -> Self {
        if dx > 0.0 { Way::Back } else { Way::Forward }
    }

    fn sign(self) -> f64 {
        match self {
            Way::Back => 1.0,
            Way::Forward => -1.0,
        }
    }
}

/// Where a scroll event is in its gesture. The page's reading of `NSEvent.phase` has no
/// "cancelled" (the system took the fingers for something else): it is an end here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    /// No gesture: a wheel that clicks, or a trackpad's momentum after the fingers left.
    None,
    /// The fingers came down (and `Began` carries the first move too: the event is two).
    Began,
    Changed,
    Ended,
}

/// One scroll event as the swipe reads it. `dx` and `dy` are in points with the engine's sign
/// (`Wheel::dx`: positive moves the content to the right).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Scroll {
    pub dx: f64,
    pub dy: f64,
    pub phase: Phase,
    /// The event is the trackpad's momentum after the fingers left (`NSEvent.momentumPhase`).
    pub momentum: bool,
    /// Trackpad deltas, not a wheel's lines.
    pub precise: bool,
    /// Where in the room, in the view's points.
    pub at: (i32, i32),
}

/// What the helper is asked: whether a scroll at `at` with the sign of `dx` moves anything.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Ask {
    pub req: u32,
    pub at: (i32, i32),
    pub dx: i32,
}

/// What the arrow is doing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Arrow {
    /// Nothing shows.
    Hidden,
    /// Following the fingers: `fill` is 0 at the edge and 1 when the swipe would count.
    Pulled { way: Way, fill: f64 },
    /// The fingers left it full: the browser goes.
    Taken { way: Way },
    /// The fingers left it short: it withdraws from `fill`.
    Withdrawn { way: Way, fill: f64 },
}

/// What one event leads to.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub(super) struct Step {
    /// The engine gets the event.
    pub forward: bool,
    pub ask: Option<Ask>,
    /// The arrow changes to this.
    pub arrow: Option<Arrow>,
    /// The browser goes this way.
    pub go: Option<Way>,
}

impl Step {
    fn forwarded() -> Self {
        Step {
            forward: true,
            ..Step::default()
        }
    }

    fn kept() -> Self {
        Step::default()
    }
}

/// Which pages there are to go to (`State::can_back`, `State::can_forward`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct History {
    pub back: bool,
    pub forward: bool,
}

impl History {
    fn has(self, way: Way) -> bool {
        match way {
            Way::Back => self.back,
            Way::Forward => self.forward,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Stage {
    Idle,
    /// A gesture began: not yet known which way it is going.
    Reading {
        dx: f64,
        dy: f64,
    },
    /// Sideways, with a page to go to: the helper is asked, and the engine keeps the events.
    Asking {
        req: u32,
        way: Way,
        pulled: f64,
    },
    /// The page's, to its end.
    Page,
    /// History's: the engine no longer gets the events.
    Pulling {
        way: Way,
        pulled: f64,
    },
    /// History's, and the fingers have left: the momentum that follows is not the page's either.
    Coasting,
}

#[derive(Debug)]
pub(super) struct Swipe {
    stage: Stage,
    next_req: u32,
}

impl Default for Swipe {
    fn default() -> Self {
        Swipe {
            stage: Stage::Idle,
            next_req: 1,
        }
    }
}

impl Swipe {
    /// `enabled` is the system's "Swipe between pages"; `history` what there is to go to now.
    pub(super) fn feed(&mut self, event: Scroll, enabled: bool, history: History) -> Step {
        // A wheel that clicks is no gesture.
        if !event.precise {
            return Step::forwarded();
        }
        if event.phase == Phase::Began {
            // A new gesture ends whatever was left of the one before.
            let lost = self.hide();
            self.stage = Stage::Reading { dx: 0.0, dy: 0.0 };
            let mut step = self.read(event, enabled, history);
            step.arrow = step.arrow.or(lost);
            return step;
        }
        if event.momentum {
            return match self.stage {
                Stage::Coasting | Stage::Pulling { .. } => Step::kept(),
                _ => Step::forwarded(),
            };
        }
        match self.stage {
            Stage::Idle | Stage::Coasting => Step::forwarded(),
            Stage::Reading { .. } => self.read(event, enabled, history),
            Stage::Asking { req, way, pulled } => {
                if ended(event.phase) {
                    self.stage = Stage::Idle;
                    return Step::forwarded();
                }
                self.stage = Stage::Asking {
                    req,
                    way,
                    pulled: pulled + event.dx * way.sign(),
                };
                Step::forwarded()
            }
            Stage::Page => {
                if ended(event.phase) {
                    self.stage = Stage::Idle;
                }
                Step::forwarded()
            }
            Stage::Pulling { way, pulled } => self.pull(way, pulled, event),
        }
    }

    /// The helper's answer: `can` is that something there scrolls that way.
    pub(super) fn answer(&mut self, req: u32, can: bool, history: History) -> Step {
        let Stage::Asking {
            req: asked,
            way,
            pulled,
        } = self.stage
        else {
            return Step::kept();
        };
        if asked != req {
            return Step::kept();
        }
        if can || !history.has(way) {
            self.stage = Stage::Page;
            return Step::kept();
        }
        let pulled = pulled.max(0.0);
        self.stage = Stage::Pulling { way, pulled };
        Step {
            arrow: Some(Arrow::Pulled {
                way,
                fill: fill(pulled),
            }),
            ..Step::kept()
        }
    }

    /// The room went away, or the page it showed did: nothing of a gesture is left to follow.
    pub(super) fn reset(&mut self) -> Option<Arrow> {
        let lost = self.hide();
        self.stage = Stage::Idle;
        lost
    }

    /// Whether the engine is kept from the events now (the arrow is following the fingers).
    pub(super) fn is_pulling(&self) -> bool {
        matches!(self.stage, Stage::Pulling { .. })
    }

    fn hide(&self) -> Option<Arrow> {
        matches!(self.stage, Stage::Pulling { .. }).then_some(Arrow::Hidden)
    }

    fn read(&mut self, event: Scroll, enabled: bool, history: History) -> Step {
        let Stage::Reading { dx, dy } = self.stage else {
            return Step::forwarded();
        };
        let (dx, dy) = (dx + event.dx, dy + event.dy);
        if ended(event.phase) {
            self.stage = Stage::Idle;
            return Step::forwarded();
        }
        if dx.abs() + dy.abs() < DECIDE {
            self.stage = Stage::Reading { dx, dy };
            return Step::forwarded();
        }
        let way = Way::of(dx);
        if !enabled || dx.abs() < SIDEWAYS * dy.abs() || !history.has(way) {
            self.stage = Stage::Page;
            return Step::forwarded();
        }
        let req = self.next_req;
        self.next_req = self.next_req.wrapping_add(1).max(1);
        self.stage = Stage::Asking {
            req,
            way,
            pulled: dx * way.sign(),
        };
        Step {
            forward: true,
            ask: Some(Ask {
                req,
                at: event.at,
                dx: way.sign() as i32,
            }),
            ..Step::default()
        }
    }

    fn pull(&mut self, way: Way, pulled: f64, event: Scroll) -> Step {
        let pulled = (pulled + event.dx * way.sign()).max(0.0);
        match event.phase {
            Phase::Ended => {
                self.stage = Stage::Coasting;
                if fill(pulled) >= 1.0 {
                    Step {
                        arrow: Some(Arrow::Taken { way }),
                        go: Some(way),
                        ..Step::kept()
                    }
                } else {
                    Step {
                        arrow: Some(Arrow::Withdrawn {
                            way,
                            fill: fill(pulled),
                        }),
                        ..Step::kept()
                    }
                }
            }
            _ => {
                self.stage = Stage::Pulling { way, pulled };
                Step {
                    arrow: Some(Arrow::Pulled {
                        way,
                        fill: fill(pulled),
                    }),
                    ..Step::kept()
                }
            }
        }
    }
}

fn ended(phase: Phase) -> bool {
    phase == Phase::Ended
}

/// How full the arrow is for a pull of `pulled` points: 1 and over counts.
pub(super) fn fill(pulled: f64) -> f64 {
    (pulled / FULL).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOTH: History = History {
        back: true,
        forward: true,
    };

    fn scroll(dx: f64, dy: f64, phase: Phase) -> Scroll {
        Scroll {
            dx,
            dy,
            phase,
            momentum: false,
            precise: true,
            at: (40, 60),
        }
    }

    /// The fingers begin sideways to the right and the helper says nothing there scrolls: the swipe
    /// is history's, and the arrow follows.
    fn pulling_back(swipe: &mut Swipe) {
        let step = swipe.feed(scroll(10.0, 1.0, Phase::Began), true, BOTH);
        assert!(step.forward);
        let ask = step.ask.expect("a sideways gesture asks the helper");
        assert_eq!((ask.at, ask.dx), ((40, 60), 1));
        let step = swipe.answer(ask.req, false, BOTH);
        assert!(matches!(
            step.arrow,
            Some(Arrow::Pulled { way: Way::Back, .. })
        ));
        assert!(swipe.is_pulling());
    }

    #[test]
    fn a_swipe_at_the_edge_goes_back_when_the_fingers_went_far_enough() {
        let mut swipe = Swipe::default();
        pulling_back(&mut swipe);
        // The engine no longer gets the events, and the arrow fills with the pull.
        let step = swipe.feed(scroll(60.0, 0.0, Phase::Changed), true, BOTH);
        assert!(!step.forward);
        let Some(Arrow::Pulled { way, fill }) = step.arrow else {
            panic!("{step:?}")
        };
        assert_eq!(way, Way::Back);
        assert!((fill - 70.0 / FULL).abs() < 1e-9, "{fill}");
        swipe.feed(scroll(90.0, 0.0, Phase::Changed), true, BOTH);
        let step = swipe.feed(scroll(0.0, 0.0, Phase::Ended), true, BOTH);
        assert_eq!(step.go, Some(Way::Back));
        assert_eq!(step.arrow, Some(Arrow::Taken { way: Way::Back }));
        assert!(!step.forward);
        // The momentum that follows is not the page's either.
        let mut momentum = scroll(30.0, 0.0, Phase::None);
        momentum.momentum = true;
        assert!(!swipe.feed(momentum, true, BOTH).forward);
    }

    #[test]
    fn too_short_a_pull_withdraws_the_arrow_and_goes_nowhere() {
        let mut swipe = Swipe::default();
        pulling_back(&mut swipe);
        swipe.feed(scroll(40.0, 0.0, Phase::Changed), true, BOTH);
        let step = swipe.feed(scroll(0.0, 0.0, Phase::Ended), true, BOTH);
        assert_eq!(step.go, None);
        let Some(Arrow::Withdrawn { way, fill }) = step.arrow else {
            panic!("{step:?}")
        };
        assert_eq!(way, Way::Back);
        assert!(fill > 0.0 && fill < 1.0, "{fill}");
        // A pull that turned round and came back to nothing fills nothing.
        let mut swipe = Swipe::default();
        pulling_back(&mut swipe);
        swipe.feed(scroll(-200.0, 0.0, Phase::Changed), true, BOTH);
        let step = swipe.feed(scroll(0.0, 0.0, Phase::Ended), true, BOTH);
        assert_eq!(step.go, None);
    }

    #[test]
    fn something_that_scrolls_under_the_fingers_keeps_the_gesture_for_the_page() {
        let mut swipe = Swipe::default();
        let step = swipe.feed(scroll(10.0, 0.0, Phase::Began), true, BOTH);
        let ask = step.ask.expect("asked");
        // The engine had the events while the helper was asked, and has them after "it scrolls".
        let step = swipe.feed(scroll(30.0, 0.0, Phase::Changed), true, BOTH);
        assert!(step.forward);
        let step = swipe.answer(ask.req, true, BOTH);
        assert_eq!(step, Step::default());
        assert!(!swipe.is_pulling());
        for _ in 0..10 {
            let step = swipe.feed(scroll(80.0, 0.0, Phase::Changed), true, BOTH);
            assert!(step.forward && step.go.is_none() && step.arrow.is_none());
        }
        let step = swipe.feed(scroll(0.0, 0.0, Phase::Ended), true, BOTH);
        assert!(step.forward && step.go.is_none());
        // The page's momentum is its own.
        let mut momentum = scroll(30.0, 0.0, Phase::None);
        momentum.momentum = true;
        assert!(swipe.feed(momentum, true, BOTH).forward);
    }

    #[test]
    fn a_swipe_the_other_way_is_forward() {
        let mut swipe = Swipe::default();
        let step = swipe.feed(scroll(-10.0, 0.0, Phase::Began), true, BOTH);
        let ask = step.ask.expect("asked");
        assert_eq!(ask.dx, -1);
        swipe.answer(ask.req, false, BOTH);
        swipe.feed(scroll(-170.0, 0.0, Phase::Changed), true, BOTH);
        let step = swipe.feed(scroll(0.0, 0.0, Phase::Ended), true, BOTH);
        assert_eq!(step.go, Some(Way::Forward));
    }

    #[test]
    fn no_page_to_go_to_means_no_question_and_no_arrow() {
        let none = History {
            back: false,
            forward: true,
        };
        let mut swipe = Swipe::default();
        let step = swipe.feed(scroll(40.0, 0.0, Phase::Began), true, none);
        assert!(step.forward && step.ask.is_none() && step.arrow.is_none());
        let step = swipe.feed(scroll(100.0, 0.0, Phase::Changed), true, none);
        assert!(step.forward && step.arrow.is_none());
        // The page that was the last to go to is gone by the time the answer comes.
        let mut swipe = Swipe::default();
        let ask = swipe
            .feed(scroll(40.0, 0.0, Phase::Began), true, BOTH)
            .ask
            .expect("asked");
        let step = swipe.answer(ask.req, false, none);
        assert!(step.arrow.is_none());
        assert!(!swipe.is_pulling());
    }

    #[test]
    fn with_the_system_setting_off_the_gesture_is_the_pages() {
        let mut swipe = Swipe::default();
        let step = swipe.feed(scroll(40.0, 0.0, Phase::Began), false, BOTH);
        assert!(step.forward && step.ask.is_none());
        let step = swipe.feed(scroll(100.0, 0.0, Phase::Changed), false, BOTH);
        assert!(step.forward && step.arrow.is_none());
    }

    #[test]
    fn a_gesture_that_goes_up_and_down_is_not_a_swipe() {
        let mut swipe = Swipe::default();
        let step = swipe.feed(scroll(6.0, 9.0, Phase::Began), true, BOTH);
        assert!(step.forward && step.ask.is_none());
        let step = swipe.feed(scroll(40.0, 3.0, Phase::Changed), true, BOTH);
        assert!(step.forward && step.ask.is_none());
    }

    #[test]
    fn the_first_points_are_read_before_the_gesture_is_called() {
        let mut swipe = Swipe::default();
        // Too little to say which way it goes: the page has it, and nothing is asked yet.
        let step = swipe.feed(scroll(2.0, 0.0, Phase::Began), true, BOTH);
        assert!(step.forward && step.ask.is_none());
        let step = swipe.feed(scroll(5.0, 0.0, Phase::Changed), true, BOTH);
        assert!(step.forward);
        assert!(step.ask.is_some());
    }

    #[test]
    fn a_wheel_that_clicks_is_never_a_swipe() {
        let mut swipe = Swipe::default();
        let mut wheel = scroll(100.0, 0.0, Phase::None);
        wheel.precise = false;
        let step = swipe.feed(wheel, true, BOTH);
        assert!(step.forward && step.ask.is_none() && step.arrow.is_none());
    }

    #[test]
    fn an_answer_for_a_gesture_that_is_over_is_ignored() {
        let mut swipe = Swipe::default();
        let ask = swipe
            .feed(scroll(40.0, 0.0, Phase::Began), true, BOTH)
            .ask
            .expect("asked");
        swipe.feed(scroll(0.0, 0.0, Phase::Ended), true, BOTH);
        let step = swipe.answer(ask.req, false, BOTH);
        assert_eq!(step, Step::default());
        assert!(!swipe.is_pulling());
        // And one for another question is not this gesture's.
        let ask = swipe
            .feed(scroll(40.0, 0.0, Phase::Began), true, BOTH)
            .ask
            .expect("asked");
        assert_ne!(ask.req, 0);
        assert_eq!(swipe.answer(ask.req + 7, false, BOTH), Step::default());
        assert!(!swipe.is_pulling());
    }

    #[test]
    fn a_new_gesture_takes_over_from_the_one_the_arrow_still_follows() {
        let mut swipe = Swipe::default();
        pulling_back(&mut swipe);
        let step = swipe.feed(scroll(4.0, 0.0, Phase::Began), true, BOTH);
        assert_eq!(step.arrow, Some(Arrow::Hidden));
        assert!(!swipe.is_pulling());
        // Losing the room hides it too.
        let mut swipe = Swipe::default();
        pulling_back(&mut swipe);
        assert_eq!(swipe.reset(), Some(Arrow::Hidden));
        assert_eq!(swipe.reset(), None);
    }
}
