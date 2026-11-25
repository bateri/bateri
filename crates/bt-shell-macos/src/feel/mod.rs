//! What a page's vibration and sounds are played with: the trackpad's motor and the speaker, and
//! the service the application owns that keeps each view's part of them ([`Feel`]).
//!
//! **The framework this drives is private.** A Force Touch trackpad has a motor under its glass
//! that macOS plays three fixed clicks on in public; the shape and strength of a pattern, and which
//! trackpad a finger is on, are only in `MultitouchSupport`, which Apple does not document and may
//! change with any system update. It is opened with `dlopen` while the application runs, with no link
//! to it, and every function in it is optional (`symbols`): what is missing is a step down, never an
//! application that does not start. The same functions are what a notarized program, signed
//! with the hardened runtime and with no entitlement for it, drives the motor with today
//! (github.com/os6/nospeaker, MIT); nothing here asks for one.
//!
//! The order of what plays is [`symbols::Backend`]'s: the motor, with the frames of the framework
//! saying which trackpad a finger is on; failing that, AppKit's own three patterns, which the system
//! plays only while a finger is on the glass (a held event is not played, every tap is one
//! click); failing that, nothing, where no trackpad vibrates. The rules are the same on each
//! step: nothing plays where no finger is on the glass and nothing is kept for when one comes, a
//! vibration is cut when the finger lifts, and a new pattern takes the place of the one that plays.
//!
//! Nothing here is started until the first vibration is asked for, and none of the tests
//! reaches the framework: the choice, the numbers, the clock and the contact table are pure and the
//! thread is driven by a recording stand-in.

mod contact;
mod fallback;
mod motor;
mod player;
mod schedule;
mod service;
mod steps;
mod symbols;

use std::time::Duration;

use bt_shell_common::agent::ready::Outcome;
use bt_shell_common::native_view::haptic::Pattern;

pub use service::{APPLICATION, Engine, Feel, Speaker, is_felt};

use fallback::Fallback;
use motor::Motor;
use player::Player;
use symbols::{Backend, Facts, Symbols};

/// How long [`Haptics::play`] waits for the thread to say whether it found a finger: the wait for
/// the first frames ([`motor::FIRST_FRAMES`]) and some for the thread to wake.
const BEGIN_WAIT: Duration = motor::FIRST_FRAMES.saturating_add(Duration::from_millis(20));

/// What plays the vibration, as far as there is anyone to say it to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The motor of `devices` trackpads, driven through the framework.
    Motor { devices: usize },
    /// AppKit's three patterns.
    Fallback,
    /// No trackpad vibrates.
    Nothing,
}

/// The vibration of the trackpad the finger is on.
pub struct Haptics {
    kind: Kind,
    player: Option<Player>,
}

impl Haptics {
    /// Opens the framework, finds the trackpads and starts the thread that plays: what is found is
    /// the choice ([`Haptics::kind`]).
    #[must_use]
    pub fn open() -> Self {
        let symbols = Symbols::load();
        let (actuate, fingers) = (symbols.can_actuate(), symbols.can_feel_fingers());
        let (facts, motor) = if actuate && fingers {
            match Motor::open(symbols) {
                Ok(motor) => (Some(motor.len()), Some(motor)),
                Err(motor::Unavailable::NoDevice) => (Some(0), None),
                Err(motor::Unavailable::Unlisted) => (None, None),
            }
        } else {
            (motor::count(&symbols), None)
        };
        let backend = symbols::choose(Facts {
            actuate,
            fingers,
            devices: facts,
        });
        match (backend, motor) {
            (Backend::Motor, Some(motor)) => {
                let devices = motor.len();
                Self::with(Kind::Motor { devices }, Player::spawn(motor))
            }
            (Backend::Nothing, _) => Self::with(Kind::Nothing, None),
            _ => Self::with(Kind::Fallback, Player::spawn(Fallback)),
        }
    }

    fn with(kind: Kind, player: Option<Player>) -> Self {
        // A system that gives no thread plays nothing, which is what `Nothing` is.
        let kind = if player.is_none() {
            Kind::Nothing
        } else {
            kind
        };
        Self { kind, player }
    }

    /// What plays.
    #[must_use]
    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// Whether a finger is on the glass of a trackpad that vibrates, where the frames say so: the
    /// fallback leaves that to AppKit and does not know.
    #[must_use]
    pub fn finger_on_glass(&self) -> bool {
        matches!(self.kind, Kind::Motor { .. }) && contact::CONTACTS.latest_down().is_some()
    }

    /// Plays `pattern` in place of what plays, if a finger is on the glass now: what was done.
    ///
    /// Where the motor is driven, what is said is what the playing thread decided (the answer comes
    /// back to this one; it is a few microseconds, and some frames the first time after the devices
    /// were started, where no finger is on the glass), not a second look at the glass that may
    /// disagree with it.
    pub fn play(&self, pattern: &Pattern) -> Outcome {
        let Some(player) = &self.player else {
            return Outcome::NoTrackpad;
        };
        let steps = steps::plan(pattern);
        match self.kind {
            Kind::Nothing => Outcome::NoTrackpad,
            // AppKit's patterns are taps and what is not one is not played.
            Kind::Fallback if !steps.iter().any(|step| step.tap) => {
                player.stop();
                Outcome::Unplayable
            }
            Kind::Fallback => {
                player.play(steps);
                Outcome::Handed
            }
            Kind::Motor { .. } if steps.is_empty() => {
                player.stop();
                Outcome::Unplayable
            }
            Kind::Motor { .. } => {
                let began = player
                    .play_told(steps, BEGIN_WAIT)
                    .unwrap_or_else(|| self.finger_on_glass());
                if began {
                    Outcome::Played
                } else {
                    Outcome::NoFinger
                }
            }
        }
    }

    /// Ends what plays.
    pub fn stop(&self) {
        if let Some(player) = &self.player {
            player.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use bt_shell_common::native_view::haptic::{Event, Pattern};

    use super::*;
    use steps::Step;

    /// A driver that plays nothing anywhere: only what is told of it is looked at.
    struct Silent;

    impl player::Driver for Silent {
        fn begin(&mut self) -> Option<usize> {
            None
        }

        fn fire(&mut self, _target: usize, _step: &Step) -> bool {
            true
        }
    }

    /// A driver that has a finger on the glass of a device.
    struct Resting;

    impl player::Driver for Resting {
        fn begin(&mut self) -> Option<usize> {
            Some(0)
        }

        fn fire(&mut self, _target: usize, _step: &Step) -> bool {
            true
        }
    }

    fn tap() -> Pattern {
        Pattern::new([Event::tap(0.0, 0.5, 0.5)]).expect("a pattern")
    }

    const MOTOR: Kind = Kind::Motor { devices: 1 };

    #[test]
    fn where_the_motor_is_driven_what_is_said_is_what_the_thread_found() {
        let resting = Haptics::with(MOTOR, Player::spawn(Resting));
        assert_eq!(resting.play(&tap()), Outcome::Played);
        let lifted = Haptics::with(MOTOR, Player::spawn(Silent));
        assert_eq!(lifted.play(&tap()), Outcome::NoFinger);
    }

    #[test]
    fn a_pattern_with_nothing_the_trackpad_plays_is_said_so_and_ends_what_played() {
        let held =
            Pattern::new([Event::held(0.0, 0.2, (0.8, 0.8), (0.5, 0.5))]).expect("a pattern");
        // AppKit's patterns are taps: a held event is not one.
        let fallback = Haptics::with(Kind::Fallback, Player::spawn(Silent));
        assert_eq!(fallback.play(&held), Outcome::Unplayable);
        assert_eq!(fallback.play(&tap()), Outcome::Handed);
        // The motor plays a held event.
        let motor = Haptics::with(MOTOR, Player::spawn(Resting));
        assert_eq!(motor.play(&held), Outcome::Played);
        // What is too weak to be felt is nothing.
        let faint = Pattern::new([Event::tap(0.0, 0.0, 0.5)]).expect("a pattern");
        assert_eq!(motor.play(&faint), Outcome::Unplayable);
    }

    #[test]
    fn what_was_done_is_told_by_what_plays_and_not_by_a_guess() {
        // Where no trackpad vibrates, nothing is played and it is said.
        let none = Haptics::with(Kind::Nothing, None);
        assert_eq!(none.kind(), Kind::Nothing);
        assert_eq!(none.play(&tap()), Outcome::NoTrackpad);
        // A system that gives no thread plays nothing, whatever it was going to be.
        assert_eq!(Haptics::with(Kind::Fallback, None).kind(), Kind::Nothing);
        // AppKit's patterns are handed over and the system does not say whether it played them.
        let fallback = Haptics::with(Kind::Fallback, Player::spawn(Silent));
        assert_eq!(fallback.play(&tap()), Outcome::Handed);
        assert!(
            !fallback.finger_on_glass(),
            "the fallback does not know where a finger is"
        );
    }
}
