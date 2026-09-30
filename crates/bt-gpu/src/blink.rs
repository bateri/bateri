//! Cursor blinking — **pure**, no ObjC, lock-free.
//!
//! Its precedent is [`crate::motion::Motion`] and `Gate`: the policy itself is
//! platform-independent, so it lives in a separate type and is tested without
//! a real window.
//!
//! **Blink is a motion frame, not a content frame.** The grid does not change,
//! only the caret's alpha; it fails the first of the three conditions in
//! `link.rs`'s module header ("the content will actually change"). But it
//! cannot be tied to the display rate either: a frame at refresh rate for a
//! 2 Hz change would undo the whole rationale of the set. The remaining path
//! is the second flavour of the clock — a wake-up that raises no damage
//! ([`crate::link::Waker::resume`]).
//!
//! **The phase holds an absolute deadline, it does not accumulate `dt`**, and
//! this is a requirement: because of the [`crate::motion::DT_MAX`] clamp, an
//! accumulation that counts a 500 ms sleep as 100 ms would flip the caret
//! once every ~5 wake-ups and draw four **identical** frames in between. The
//! symptom would be silent: the window wakes, draws, and no pixel changes.
//!
//! **The stop condition is named** (`CLAUDE.md`): the app or the user turns it
//! off, the caret is hidden, the window is occluded (`Gate` already drops to
//! `setPaused`), or [`IDLE_STOP`] has passed since the last content frame.
//! Stopping **leaves the phase lit**: stopping in the dark phase would make
//! the caret vanish until the next damage, and the user would read that as
//! "the caret disappeared".

/// The **default** half period of the blink — its value and rationale live
/// in `bt_core::CURSOR_BLINK_INTERVAL`.
///
/// Only a pointer here, no copy: a chosen number gets one owner, and a
/// rationale written in two places silently lies in one of them when it is
/// re-tuned and only the other is updated (`/code-review`). The period has
/// been a user setting since 016; the constant here is only the floor of
/// [`Blink::default`].
const HALF_PERIOD: f64 = bt_core::CURSOR_BLINK_INTERVAL;

/// How long after keyboard silence the blink stops, in seconds — **chosen,
/// not measured**; its source is kitty's `cursor_stop_blinking_after`
/// default (15 s).
///
/// This constant is what reconciles blink with this repo's central promise:
/// without it, an open blink would make the window **permanently** non-idle.
/// With it, the window truly returns to zero frames 15 seconds after typing
/// stops.
///
/// Its base is the **last content frame**, not the last drawn frame: blink
/// frames are drawn too, and a counter watching them would never fill up
/// (`link.rs`'s `last_frame_at` is written on the motion arm as well).
const IDLE_STOP: f64 = 15.0;

/// State of the cursor blink.
///
/// The time base is the display link's timestamp (`now` in [`crate::link`]),
/// not a clock read: the `quiet=` token and the animation's clock are already
/// read from there, and a second base would create two separate times.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Blink {
    /// Whether the user and the application together say "blink"
    /// (the answer of `bt_core::CursorBlink::resolve`).
    enabled: bool,
    /// Whether the phase is **lit** right now. Also `true` when disabled: stopping
    /// leaves the phase lit.
    lit: bool,
    /// **Absolute** time of the next phase change; `None` -> no pending tick.
    next_flip: Option<f64>,
    /// Timestamp of the last **content** frame; the base of [`IDLE_STOP`].
    last_content_at: Option<f64>,
    /// Half period, in seconds — comes from the setting
    /// (`[terminal] cursor_blink_interval`).
    ///
    /// A field, not a `const`: the user can change it at save time. The
    /// module's purity is not broken — `Blink` is still `Copy` and lives in a
    /// `Cell`, precedent `Motion::set_style`.
    half_period: f64,
}

impl Default for Blink {
    /// **`lit` starts as `true`**, not `derive`'s `false`: the field's
    /// invariant is "lit even when disabled" and `derive` would violate it the
    /// moment it is born. Today [`Blink::alpha`] short-circuits on `enabled`,
    /// so no symptom shows; a simplification the invariant licenses (tying
    /// `alpha` to `lit` alone) would spawn the window with an **invisible
    /// caret**.
    fn default() -> Self {
        Self {
            enabled: false,
            lit: true,
            next_flip: None,
            last_content_at: None,
            half_period: HALF_PERIOD,
        }
    }
}

impl Blink {
    /// Content frame: refreshes the setting and resets the inactivity
    /// counter.
    ///
    /// **It does not reset the phase**, deliberately: 013's live counter
    /// produces a content frame every second while a command runs, so if the
    /// phase were pulled to lit on every content frame the blink's rhythm
    /// would break while a command runs. The cost is named: a key pressed in
    /// the dark phase keeps the caret waiting for at most half a period.
    pub(crate) fn content_frame(&mut self, now: f64, enabled: bool) {
        self.last_content_at = Some(now);
        if self.enabled != enabled {
            self.enabled = enabled;
            // Turning off leaves the phase **lit** (R9.1); turning on starts
            // from the next half period.
            self.lit = true;
            self.next_flip = enabled.then_some(now + self.half_period);
        } else if enabled && self.next_flip.is_none() {
            // Return from inactivity: the counter was refreshed above, the
            // tick is being re-armed.
            self.next_flip = Some(now + self.half_period);
        }
    }

    /// The caret moved: pulls the phase to **lit** and restarts the counter.
    ///
    /// **The caret does not blink while typing**, and this came from a user
    /// report (2026-09-19): the caret blinking off and on while pressing keys
    /// was read as "typing and blinking at the same time", and rightly so —
    /// every editor and terminal keeps the caret steady while typing and goes
    /// back to blinking on a pause.
    ///
    /// **The trigger is the caret's movement**, not the keystroke itself, and
    /// this is deliberate: a key would need a separate signal from `bt-shell`
    /// to `bt-gpu`, whereas the movement is already in this module's hands.
    /// The distinction also stands in the right place — a running command's
    /// duration counter does **not** move the caret, so the blink carries on
    /// undisturbed throughout `sleep 5`; streaming output does move it, and
    /// keeping the caret steady there is what is wanted anyway.
    pub(crate) fn wake(&mut self, now: f64) {
        if !self.enabled {
            return;
        }
        self.lit = true;
        self.next_flip = Some(now + self.half_period);
    }

    /// Advances time; the return value is **whether the phase flipped in this
    /// frame**.
    ///
    /// One-shot, and consumed in `link.rs`'s sleep test **before**
    /// `motion.settled()`'s early return: if it were not asked, a wake-up that
    /// raises no damage would create a wake/sleep spin that produces no frames.
    ///
    /// On return from a long sleep the phase lands in the right place **in a
    /// single step**: the past ticks in between are skipped, not accumulated.
    pub(crate) fn advance(&mut self, now: f64) -> bool {
        if !self.enabled {
            return false;
        }
        // Inactivity: the stop condition. The phase is pulled to lit and the
        // tick is extinguished; `content_frame` re-arms both on the first
        // damage.
        if self.last_content_at.is_some_and(|at| now - at >= IDLE_STOP) {
            let was_dark = !self.lit;
            self.lit = true;
            self.next_flip = None;
            return was_dark;
        }
        let Some(due) = self.next_flip else {
            return false;
        };
        if now < due {
            return false;
        }
        self.lit = !self.lit;
        // **Absolute**, `now + HALF_PERIOD` and not `due + HALF_PERIOD`:
        // counting from a base left in the past after a long sleep would fire
        // several ticks back to back immediately.
        self.next_flip = Some(now + self.half_period);
        true
    }

    /// Changes the period and **re-arms** the pending tick.
    ///
    /// Re-arming is required because [`Blink::next_flip`] is an **absolute**
    /// deadline: merely writing the field would make the saved new rhythm a
    /// flip **late** — the user saves, nothing happens, and it suddenly
    /// changes at the next blink-off. With the same value nothing is done,
    /// otherwise every settings save would reset the phase.
    pub(crate) fn set_half_period(&mut self, now: f64, half_period: f64) {
        if self.half_period == half_period {
            return;
        }
        self.half_period = half_period;
        if self.next_flip.is_some() {
            self.next_flip = Some(now + half_period);
        }
    }

    /// The caret's opacity in this frame; **always `1.0`** while blink is off.
    pub(crate) fn alpha(self) -> f32 {
        if self.lit || !self.enabled { 1.0 } else { 0.0 }
    }

    /// Absolute time of the next phase change — the blink half of the clock.
    pub(crate) fn next_flip(self) -> Option<f64> {
        self.enabled.then_some(self.next_flip).flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_disabled_blink_never_flips() {
        let mut blink = Blink::default();
        blink.content_frame(0.0, false);
        assert!(!blink.advance(10.0), "disabled blink changed phase");
        assert_eq!(blink.alpha(), 1.0);
        assert_eq!(blink.next_flip(), None, "disabled blink is arming a clock");
    }

    #[test]
    fn the_phase_is_an_absolute_deadline() {
        // **An implementation that accumulates `dt` would fail here.** On
        // return from a long sleep the phase flips in a single step and the
        // next tick is counted from `now` — not from a past base, otherwise
        // several ticks would fire back to back immediately.
        let mut blink = Blink::default();
        blink.content_frame(0.0, true);
        assert_eq!(blink.next_flip(), Some(HALF_PERIOD));
        assert!(blink.advance(5.0), "phase did not flip after a long sleep");
        assert_eq!(blink.alpha(), 0.0);
        assert_eq!(blink.next_flip(), Some(5.0 + HALF_PERIOD));
        // Asked a second time at the same instant it does not flip: the tick
        // is one-shot.
        assert!(!blink.advance(5.0), "phase flipped twice in the same frame");
    }

    #[test]
    fn a_lit_and_a_dark_phase_alternate() {
        let mut blink = Blink::default();
        blink.content_frame(0.0, true);
        assert_eq!(blink.alpha(), 1.0);
        assert!(blink.advance(HALF_PERIOD));
        assert_eq!(blink.alpha(), 0.0);
        assert!(blink.advance(2.0 * HALF_PERIOD));
        assert_eq!(blink.alpha(), 1.0);
    }

    #[test]
    fn a_new_interval_rebuilds_the_pending_tick() {
        // **Re-arming is required**: `next_flip` is an absolute deadline, so
        // merely writing the field would make the saved rhythm a flip **late**
        // — the user saves, nothing happens, and it suddenly changes at the
        // next blink-off.
        let mut blink = Blink::default();
        blink.content_frame(0.0, true);
        assert_eq!(blink.next_flip(), Some(0.5), "default half period");

        // At 2.0 the period shortens: the tick is re-armed **from that
        // moment**.
        blink.set_half_period(2.0, 0.1);
        assert_eq!(blink.next_flip(), Some(2.1));
        assert!(blink.advance(2.1), "did not flip at the new rhythm");
        assert_eq!(blink.next_flip(), Some(2.2), "new period is not sustained");

        // The same value **does nothing**: if every settings save reset the
        // phase, a saving user would keep pulling the caret to lit.
        let before = blink;
        blink.set_half_period(5.0, 0.1);
        assert_eq!(blink, before, "same period disturbed the phase");
    }

    #[test]
    fn an_interval_change_while_stopped_arms_nothing() {
        // With no pending tick (blink off, or stopped by inactivity) a period
        // change **must not spawn a tick**: if it did, a disabled blink would
        // arm a clock and the zero-frames-when-idle contract would break.
        let mut blink = Blink::default();
        blink.set_half_period(1.0, 0.2);
        assert_eq!(blink.next_flip(), None, "disabled blink armed a tick");
    }

    #[test]
    fn the_stop_condition_leaves_the_caret_lit() {
        // **R9.1.** Stopping in the dark phase would make the caret vanish
        // until the next damage; the stop pulls the phase to lit and asks for
        // one last frame (the returned `true`).
        let mut blink = Blink::default();
        blink.content_frame(0.0, true);
        assert!(blink.advance(HALF_PERIOD), "phase did not go dark");
        assert_eq!(blink.alpha(), 0.0);
        assert!(blink.advance(IDLE_STOP), "stop asked for no last frame");
        assert_eq!(blink.alpha(), 1.0, "caret stayed dark");
        assert_eq!(blink.next_flip(), None, "clock armed after stopping");
        // It asks for no more frames: the stop is one-shot.
        assert!(!blink.advance(IDLE_STOP + 10.0));
    }

    #[test]
    fn typing_keeps_the_caret_lit() {
        // When the caret moves the phase returns to lit and the counter
        // restarts, so a user who keeps typing sees the caret dark **never**.
        let mut blink = Blink::default();
        blink.content_frame(0.0, true);
        assert!(blink.advance(HALF_PERIOD), "phase did not go dark");
        assert_eq!(blink.alpha(), 0.0);

        blink.wake(HALF_PERIOD);
        assert_eq!(blink.alpha(), 1.0, "caret stayed dark while typing");
        assert_eq!(blink.next_flip(), Some(2.0 * HALF_PERIOD));
        // Typing again before the half period elapses pushes the counter
        // back once more.
        blink.wake(1.5 * HALF_PERIOD);
        assert!(!blink.advance(2.0 * HALF_PERIOD), "counter not pushed back");
        assert_eq!(blink.alpha(), 1.0);
    }

    #[test]
    fn a_disabled_blink_ignores_the_caret_moving() {
        let mut blink = Blink::default();
        blink.content_frame(0.0, false);
        blink.wake(1.0);
        assert_eq!(blink.next_flip(), None, "disabled blink armed a clock");
    }

    #[test]
    fn damage_brings_the_blink_back() {
        let mut blink = Blink::default();
        blink.content_frame(0.0, true);
        blink.advance(IDLE_STOP);
        assert_eq!(blink.next_flip(), None);
        blink.content_frame(IDLE_STOP, true);
        assert_eq!(
            blink.next_flip(),
            Some(IDLE_STOP + HALF_PERIOD),
            "damage did not bring the blink back"
        );
    }

    #[test]
    fn turning_it_off_mid_dark_phase_relights_the_caret() {
        let mut blink = Blink::default();
        blink.content_frame(0.0, true);
        blink.advance(HALF_PERIOD);
        assert_eq!(blink.alpha(), 0.0);
        blink.content_frame(0.6, false);
        assert_eq!(blink.alpha(), 1.0, "turned-off blink left the caret dark");
        assert_eq!(blink.next_flip(), None);
    }
}
