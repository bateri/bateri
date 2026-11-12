//! What a pane wears while the program it was opened for is on its way.
//!
//! A pane opened for a program — a file that was ⌘-clicked, which the chosen program opens — is
//! born before the program has drawn anything. The program is started first (on a server it is
//! asked for over the connection, a few round trips away), and one that shows a view asks the
//! terminal about it before it opens one. Until then the terminal under the pane is an empty grid
//! with a cursor on it, which reads as a shell that has died and can be typed to.
//!
//! So the pane wears a cover for that stretch ([`ProgramWait`]): the theme's ground over the
//! whole pane, and in its middle, after a hold, the line that runs round a tab's mark while a
//! command runs — the same tail on the same faint ring, at the same pace (`tabs::ring_turn`,
//! `tabs::tail_strength`), larger. A tab's ring already says "at work, nothing to show yet";
//! here it says it where the person is looking.
//!
//! **Nothing to read and nothing to press.** The header over the pane names it; the stretch is
//! too short for words, and a wait that is long is the same wait.
//!
//! **A hold before it shows** ([`HOLD`]): a program on this computer is there before the hold is
//! over, and the ring never flashes for it — the cover alone, which is the ground the view comes
//! in on, is all that was seen.
//!
//! **Core Animation's, all of it.** The hold is the fade's own begin time and the turn is handed
//! to the window server: the pane is asked for no frame and the application sets no clock, so a
//! pane that waits costs what a tab's ring costs. Under Reduce Motion the ring does not turn; it
//! stands with its head at the top.
//!
//! **It goes once** ([`ProgramWait::end`]), when there is something to see — the program's view,
//! or anything the terminal has drawn (a program that shows no view, the line that says why the
//! terminal could not be had) — and fades as it goes. A pane that closes takes it along.

use std::cell::Cell;
use std::time::{Duration, Instant};

use block2::RcBlock;
use bt_core::{TabRunning, Theme};
use bt_shell_common::tabs;
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message as _, define_class, msg_send};
use objc2_app_kit::{
    NSAnimatablePropertyContainer as _, NSAutoresizingMaskOptions, NSBezierPath, NSColor, NSImage,
    NSView,
};
use objc2_foundation::{NSNumber, NSObjectProtocol, NSPoint, NSRect, NSSize, ns_string};
use objc2_quartz_core::{
    CABasicAnimation, CACurrentMediaTime, CALayer, CAMediaTiming as _, CAMediaTimingFunction,
    CATransaction, kCAFillModeBackwards,
};

/// How long the cover stands bare before the ring starts to show — **chosen, not measured**: a
/// program that is there sooner was not worth a sign, and a ring that came and went inside a
/// quarter of a second would be a flicker.
pub(crate) const HOLD: Duration = Duration::from_millis(300);

/// How long the ring takes to come in, seconds: the application's base for something new on the
/// screen.
const FADE_IN: f64 = 0.24;

/// How long the cover takes to go, seconds, once there is something under it to see: no longer
/// than the ring took to come.
const FADE_OUT: f64 = 0.18;
const _: () = assert!(FADE_OUT <= FADE_IN);

/// The ring's side and its line, points: a tab's small ring is `tabs::INDICATOR_SIDE` with a
/// line of 1.6; this one stands alone in a pane and is twice that.
const RING_SIDE: f64 = 24.0;
const RING_LINE: f64 = 2.0;

/// The room the ring's picture is drawn in: the ring and half a line clear on every side, so the
/// line's outer edge is not cut where the picture ends.
const RING_ROOM: f64 = RING_SIDE + RING_LINE;

/// The keys the ring's two animations are held under.
fn turn_key() -> &'static objc2_foundation::NSString {
    ns_string!("bateri.wait.turn")
}

fn show_key() -> &'static objc2_foundation::NSString {
    ns_string!("bateri.wait.show")
}

pub(crate) struct WaitIvars {
    ring: Retained<CALayer>,
    /// When the pane was born: the hold is counted from here, so a pane shown again before it is
    /// over (a tab brought forward) does not start the hold anew.
    born: Instant,
    /// Reduce Motion: the ring stands still.
    still: bool,
    /// What the ring's picture was last drawn from: the accent and the screen's scale.
    ink: Cell<Option<(u32, u64)>>,
    accent: Cell<u32>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; ProgramWait implements no `Drop` and is born
    // with `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriProgramWait"]
    #[ivars = WaitIvars]
    pub(crate) struct ProgramWait;

    unsafe impl NSObjectProtocol for ProgramWait {}

    impl ProgramWait {
        /// The ring's head is at the top and it turns clockwise, as a tab's does in its own
        /// flipped space.
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        /// A press on a pane that waits is a press on the pane: it takes the keyboard, and its
        /// header's tools work.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }

        /// In a window (again): the window server lost what it was turning when the pane left
        /// one, and the screen's scale may be another.
        #[unsafe(method(viewDidMoveToWindow))]
        fn view_did_move_to_window(&self) {
            self.play();
        }

        /// The ring stays in the middle of the pane, whatever makes the pane another size: the
        /// cover follows the pane by its autoresizing, and every size it is given comes here.
        #[unsafe(method(setFrameSize:))]
        fn set_frame_size(&self, size: NSSize) {
            // SAFETY: `NSView`'s own `setFrameSize:`, with the size it was given.
            let _: () = unsafe { msg_send![super(self), setFrameSize: size] };
            self.center();
        }

        #[unsafe(method(viewDidUnhide))]
        fn view_did_unhide(&self) {
            self.play();
        }

        #[unsafe(method(viewDidChangeBackingProperties))]
        fn view_did_change_backing_properties(&self) {
            self.ink();
        }
    }
);

impl ProgramWait {
    /// The cover of a pane born now, in `theme`'s colours; `still` under Reduce Motion.
    pub(crate) fn new(mtm: MainThreadMarker, theme: &Theme, still: bool) -> Retained<Self> {
        let ring = CALayer::new();
        ring.setBounds(NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(RING_ROOM, RING_ROOM),
        ));
        let this = Self::alloc(mtm).set_ivars(WaitIvars {
            ring: ring.clone(),
            born: Instant::now(),
            still,
            ink: Cell::new(None),
            accent: Cell::new(theme.accent),
        });
        // SAFETY: `NSView`'s designated initializer.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.setWantsLayer(true);
        this.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        if let Some(layer) = this.layer() {
            layer.addSublayer(&ring);
        }
        this.paint(theme);
        this
    }

    /// The theme's ground, whole, and the ring in its accent.
    pub(crate) fn paint(&self, theme: &Theme) {
        let [r, g, b] = theme.background_srgb().map(|byte| f64::from(byte) / 255.0);
        let ground = NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, 1.0);
        if let Some(layer) = self.layer() {
            still(|| layer.setBackgroundColor(Some(&ground.CGColor())));
        }
        self.ivars().accent.set(theme.accent);
        self.ink();
    }

    /// Puts the ring in the middle of the cover, on whole points.
    fn center(&self) {
        let size = self.bounds().size;
        let middle = NSPoint::new((size.width / 2.0).round(), (size.height / 2.0).round());
        still(|| self.ivars().ring.setPosition(middle));
    }

    /// There is something to see under the cover: it fades and is gone from the pane.
    pub(crate) fn end(&self) {
        let (fading, leaving) = (self.retain(), self.retain());
        crate::card::animate(
            FADE_OUT,
            move || fading.animator().setAlphaValue(0.0),
            move || leaving.removeFromSuperview(),
        );
    }

    /// Draws the ring's picture for the accent and the scale of the screen the pane is on, when
    /// either is another than it was drawn for.
    fn ink(&self) {
        let iv = self.ivars();
        let scale = self
            .window()
            .map_or(2.0, |window| window.backingScaleFactor());
        let ink = Some((iv.accent.get(), scale.to_bits()));
        if iv.ink.replace(ink) == ink {
            return;
        }
        let accent = iv.accent.get();
        let block = RcBlock::new(move |_: NSRect| -> Bool {
            draw_ring(accent);
            Bool::YES
        });
        let image = NSImage::imageWithSize_flipped_drawingHandler(
            NSSize::new(RING_ROOM, RING_ROOM),
            true,
            &block,
        );
        let picture = image.layerContentsForContentsScale(scale);
        still(|| {
            iv.ring.setContentsScale(scale);
            // SAFETY: what `NSImage::layerContentsForContentsScale` returns is what `contents`
            // takes.
            unsafe { iv.ring.setContents(Some(&picture)) };
        });
    }

    /// Hands the ring to the window server: its coming in after what is left of the hold, and
    /// its turn. A no-op while both are held, and out of a window, where there is no one to play
    /// them to.
    fn play(&self) {
        if self.window().is_none() || self.isHiddenOrHasHiddenAncestor() {
            return;
        }
        self.ink();
        let ring = &self.ivars().ring;
        // SAFETY: reading the animations a layer holds, on the main thread.
        let shown = unsafe { ring.animationForKey(show_key()) }.is_some();
        let left = HOLD.saturating_sub(self.ivars().born.elapsed());
        if !shown && !left.is_zero() {
            let show = CABasicAnimation::animationWithKeyPath(Some(ns_string!("opacity")));
            // SAFETY: plain `NSNumber`s, what an opacity takes.
            unsafe {
                show.setFromValue(Some(&NSNumber::numberWithDouble(0.0)));
                show.setToValue(Some(&NSNumber::numberWithDouble(1.0)));
            }
            show.setDuration(FADE_IN);
            show.setTimingFunction(Some(&CAMediaTimingFunction::functionWithControlPoints(
                0.0, 0.0, 0.58, 1.0,
            )));
            // Held at nothing until its time: the model's value, the whole ring, is what is left
            // when it is over.
            show.setBeginTime(
                ring.convertTime_fromLayer(CACurrentMediaTime(), None) + left.as_secs_f64(),
            );
            // SAFETY: a constant `NSString` Core Animation exposes, only read.
            show.setFillMode(unsafe { kCAFillModeBackwards });
            ring.addAnimation_forKey(&show, Some(show_key()));
        }
        // SAFETY: as above.
        if unsafe { ring.animationForKey(turn_key()) }.is_some() {
            return;
        }
        let age = self.ivars().born.elapsed();
        let Some(turn) = tabs::ring_turn(age, self.ivars().still, TabRunning::TailRing) else {
            return;
        };
        let from = turn.phase * std::f64::consts::TAU;
        let round =
            CABasicAnimation::animationWithKeyPath(Some(ns_string!("transform.rotation.z")));
        // SAFETY: plain `NSNumber`s, what a rotation takes. In the cover's flipped space a
        // positive angle goes clockwise on screen.
        unsafe {
            round.setFromValue(Some(&NSNumber::numberWithDouble(from)));
            round.setToValue(Some(&NSNumber::numberWithDouble(
                from + std::f64::consts::TAU,
            )));
        }
        round.setDuration(turn.period.as_secs_f64());
        round.setRepeatCount(f32::INFINITY);
        round.setTimingFunction(Some(&CAMediaTimingFunction::functionWithControlPoints(
            0.0, 0.0, 1.0, 1.0,
        )));
        ring.addAnimation_forKey(&round, Some(turn_key()));
    }
}

/// Runs `change` without Core Animation's implicit fading of whatever it sets.
fn still(change: impl FnOnce()) {
    CATransaction::begin();
    CATransaction::setDisableActions(true);
    change();
    CATransaction::commit();
}

/// The ring, its head at the top: the accent as strong, a degree at a time, as a tab's running
/// line is that far behind its head, over the faint line that goes all the way round
/// (`tabs::tail_strength`). Drawn in a flipped space [`RING_ROOM`] square.
fn draw_ring(accent: u32) {
    let middle = NSPoint::new(RING_ROOM / 2.0, RING_ROOM / 2.0);
    let [r, g, b] = [accent >> 16, accent >> 8, accent].map(|byte| f64::from(byte & 0xff) / 255.0);
    for degree in 0..360 {
        let behind = f64::from(degree);
        let strength = tabs::tail_strength(behind + 0.5, true);
        if strength <= 0.0 {
            continue;
        }
        let piece = NSBezierPath::bezierPath();
        // A hair over the degree, so that two pieces leave no seam between them.
        piece.appendBezierPathWithArcWithCenter_radius_startAngle_endAngle_clockwise(
            middle,
            RING_SIDE / 2.0,
            -90.0 - behind - 1.2,
            -90.0 - behind,
            false,
        );
        piece.setLineWidth(RING_LINE);
        NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, strength).setStroke();
        piece.stroke();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The hold is shorter than a wait worth a sign and longer than a flicker, and the ring
    /// comes in at the application's pace for something new.
    #[test]
    fn the_ring_holds_back_a_moment_and_comes_in_at_the_applications_pace() {
        assert!(HOLD >= Duration::from_millis(200) && HOLD <= Duration::from_millis(500));
        assert!((FADE_IN - 0.24).abs() < f64::EPSILON);
    }

    /// The ring is a tab's running line: the same pace, and the same tail over the same faint
    /// line, so the two read as one sign.
    #[test]
    fn the_ring_is_a_tabs_running_line() {
        let source = include_str!("program_wait.rs");
        let source = &source[..source.find("#[cfg(test)]").unwrap_or(source.len())];
        assert!(source.contains("tabs::ring_turn(age, self.ivars().still, TabRunning::TailRing)"));
        assert!(source.contains("tabs::tail_strength(behind + 0.5, true)"));
        // Under Reduce Motion the rule gives no turn, and none is played.
        assert_eq!(
            tabs::ring_turn(Duration::from_secs(1), true, TabRunning::TailRing),
            None
        );
    }

    /// The cover asks the pane for no frame and the application for no clock: its hold and its
    /// turn are the window server's.
    #[test]
    fn the_cover_is_core_animations_and_sets_no_clock() {
        let source = include_str!("program_wait.rs");
        let source = &source[..source.find("#[cfg(test)]").unwrap_or(source.len())];
        for clock in [
            "DispatchQueue",
            "DispatchTime",
            "request_frame",
            "Waker",
            "NSTimer",
        ] {
            assert!(!source.contains(clock), "{clock}");
        }
        assert!(source.contains("show.setBeginTime("));
        assert!(source.contains("kCAFillModeBackwards"));
    }
}
