//! What a page's vibration is played with: the trackpad's motor.
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
mod steps;
mod symbols;

use bt_shell_common::native_view::haptic::Pattern;

use fallback::Fallback;
use motor::Motor;
use player::Player;
use symbols::{Backend, Facts, Symbols};

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

    /// Plays `pattern` in place of what plays, if a finger is on the glass now.
    pub fn play(&self, pattern: &Pattern) {
        if let Some(player) = &self.player {
            let steps = steps::plan(pattern);
            if steps.is_empty() {
                player.stop();
            } else {
                player.play(steps);
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
