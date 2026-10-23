//! Highlights that come and go: a row's hover and the choice of a row fade in and out instead
//! of switching.
//!
//! Every fade is a level between 0 and 1 that a page reads when it builds, kept by [`Id`] and
//! moved toward a target a fixed time at a time. **Each one stops**: a level that reaches its
//! target and is at 0 is forgotten, and [`Fades::advance`] says whether anything is still
//! moving, so the host asks for another frame exactly while one is. Under Reduce Motion a
//! change is a snap (the same rule the terminal's motion keeps: an accessibility setting never
//! adds an animation).

use std::collections::HashMap;

use crate::Id;

/// How long a highlight takes to come to full strength or to go, seconds. Short enough that a
/// pointer crossing rows leaves a trail and not a smear.
pub const FADE_SECONDS: f32 = 0.12;

/// The longest step one frame may take, seconds: a frame that comes after a long sleep (or the
/// first after the page woke) moves the fades a little, not to their end — the change is seen.
const STEP_MAX: f64 = 0.05;

/// The levels and the targets they move toward.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fades {
    levels: HashMap<Id, Fade>,
    /// When the last step was taken; `None` while nothing moves, so the first step after a
    /// rest is a short one.
    last: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Fade {
    level: f32,
    target: f32,
}

impl Fades {
    /// Sets where `id`'s level goes (`0.0..=1.0`). A level not known yet starts at 0 and comes
    /// up; a change of nothing changes nothing.
    pub fn to(&mut self, id: Id, target: f32) {
        let target = target.clamp(0.0, 1.0);
        match self.levels.get_mut(&id) {
            Some(fade) => fade.target = target,
            None if target > 0.0 => {
                self.levels.insert(id, Fade { level: 0.0, target });
            }
            None => {}
        }
    }

    /// Sets where `id`'s level goes and puts it there at once.
    pub fn snap(&mut self, id: Id, level: f32) {
        let level = level.clamp(0.0, 1.0);
        if level > 0.0 {
            self.levels.insert(
                id,
                Fade {
                    level,
                    target: level,
                },
            );
        } else {
            self.levels.remove(&id);
        }
    }

    /// `id`'s level now: 0 for one that is not known.
    pub fn level(&self, id: Id) -> f32 {
        self.levels.get(&id).map_or(0.0, |fade| fade.level)
    }

    /// Whether anything is on the way to its target.
    pub fn moving(&self) -> bool {
        self.levels.values().any(|fade| fade.level != fade.target)
    }

    /// Takes the step to `now` (seconds on the host's clock). `reduce_motion` puts every level
    /// at its target. Returns whether anything is still moving afterwards — the host draws
    /// another frame while it is.
    pub fn advance(&mut self, now: f64, reduce_motion: bool) -> bool {
        if !self.moving() {
            self.last = None;
            self.levels.retain(|_, fade| fade.level > 0.0);
            return false;
        }
        let dt = self
            .last
            .map_or(0.0, |last| (now - last).clamp(0.0, STEP_MAX));
        let step = if reduce_motion {
            1.0
        } else {
            dt as f32 / FADE_SECONDS
        };
        for fade in self.levels.values_mut() {
            if fade.level < fade.target {
                fade.level = (fade.level + step).min(fade.target);
            } else if fade.level > fade.target {
                fade.level = (fade.level - step).max(fade.target);
            }
        }
        let moving = self.moving();
        // A level that came to rest at 0 is not kept.
        self.levels
            .retain(|_, fade| fade.level > 0.0 || fade.target > 0.0);
        self.last = moving.then_some(now);
        // The first step after a rest only starts the clock: the change shows from the next frame.
        if reduce_motion { false } else { moving }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROW: Id = Id::new(7);

    #[test]
    fn a_highlight_comes_up_over_its_time_and_then_the_page_rests() {
        let mut fades = Fades::default();
        fades.to(ROW, 1.0);
        assert!(fades.moving());
        // The first frame after a rest moves nothing (it has no step to measure).
        assert!(fades.advance(10.0, false));
        assert_eq!(fades.level(ROW), 0.0);
        let mut now = 10.0;
        let mut frames = 0;
        while fades.advance(now + 1.0 / 60.0, false) {
            now += 1.0 / 60.0;
            frames += 1;
            assert!(frames < 30, "the fade never ends");
        }
        assert_eq!(fades.level(ROW), 1.0);
        assert!(!fades.moving());
        assert!(
            (6..=9).contains(&frames),
            "{frames} frames for {FADE_SECONDS} s"
        );
    }

    #[test]
    fn a_highlight_that_goes_is_forgotten_at_zero() {
        let mut fades = Fades::default();
        fades.snap(ROW, 1.0);
        fades.to(ROW, 0.0);
        let mut now = 0.0;
        while fades.advance(now, false) {
            now += 1.0 / 60.0;
        }
        assert_eq!(fades.level(ROW), 0.0);
        assert!(fades.levels.is_empty(), "nothing to keep");
    }

    #[test]
    fn reduce_motion_puts_the_level_at_its_target_in_one_step() {
        let mut fades = Fades::default();
        fades.to(ROW, 1.0);
        assert!(!fades.advance(1.0, true), "nothing is left moving");
        assert_eq!(fades.level(ROW), 1.0);
        fades.to(ROW, 0.0);
        assert!(!fades.advance(1.1, true));
        assert_eq!(fades.level(ROW), 0.0);
    }

    #[test]
    fn a_frame_after_a_long_sleep_does_not_jump_to_the_end() {
        let mut fades = Fades::default();
        fades.to(ROW, 1.0);
        fades.advance(100.0, false);
        // An hour later: one step is capped, the fade has not finished in it.
        assert!(fades.advance(3700.0, false));
        assert!(fades.level(ROW) < 1.0);
    }

    #[test]
    fn a_page_with_no_fades_asks_for_no_frames() {
        let mut fades = Fades::default();
        assert!(!fades.advance(1.0, false));
        assert!(!fades.moving());
    }
}
