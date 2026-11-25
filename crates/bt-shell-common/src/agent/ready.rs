//! `haptic` and `sound`: what the `bateri` command says about the kit's ready patterns and sounds.
//!
//! The names are the kit's (`haptic::NAMES`, `sound::NAMES`): the command carries a name as it was
//! written and this decides what it means. The playing is the application's; what it did is told to
//! this as an [`Outcome`], so that what the command prints is a pure function of the two and is
//! tested without a trackpad or a speaker.

use super::message::{Failure, Kind, Reply};

use crate::native_view::{haptic, sound};

/// What is asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ready {
    /// One of the kit's vibrations.
    Haptic,
    /// One of the kit's sounds.
    Sound,
}

impl Ready {
    /// The names there are.
    #[must_use]
    pub fn names(self) -> &'static [&'static str] {
        match self {
            Ready::Haptic => &haptic::NAMES,
            Ready::Sound => &sound::NAMES,
        }
    }
}

/// What the application did with a name it knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// It plays.
    Played,
    /// A vibration where the motor is driven and no finger is on the glass: nothing plays, and
    /// nothing is kept for when a finger comes.
    NoFinger,
    /// A vibration handed to the system, which plays it only while a finger is on the glass and
    /// does not say whether it did.
    Handed,
    /// A vibration where no trackpad vibrates.
    NoTrackpad,
    /// A vibration that has nothing in it this trackpad plays: only taps are played where the
    /// system's own patterns are (a held event is not), and a pattern may be weaker than what is felt.
    Unplayable,
    /// A sound the system could not start.
    Failed,
    /// A vibration that `[feedback] haptics` keeps from playing.
    HapticsOff,
    /// A sound that `[feedback] sounds` keeps from playing.
    SoundsOff,
}

/// The answer to `haptic [NAME]` or `sound [NAME]`: with no name, the names there are, one to a
/// line; with one, `play` plays it and the answer is one line that says what came of it; with one
/// `play` does not know (`None`), an error that says the names there are.
pub fn reply(
    ready: Ready,
    name: Option<&str>,
    play: impl FnOnce(&str) -> Option<Outcome>,
) -> Reply {
    let Some(name) = name else {
        let mut lines = ready.names().join("\n");
        lines.push('\n');
        return Reply::Text(lines);
    };
    match play(name) {
        Some(outcome) => said(name, outcome),
        None => Reply::Failed(Failure::new(
            Kind::Usage,
            format!(
                "`{}` is not a ready name: {}",
                name.chars().filter(|c| !c.is_control()).collect::<String>(),
                ready.names().join(" ")
            ),
        )),
    }
}

/// The answer to a name that was played, once what came of it is known: for a vibration the
/// application plays on a thread of its own, which says it when it has looked for a finger.
#[must_use]
pub fn said(name: &str, outcome: Outcome) -> Reply {
    Reply::Text(line(name, outcome))
}

fn line(name: &str, outcome: Outcome) -> String {
    match outcome {
        Outcome::Played => format!("played {name}\n"),
        Outcome::NoFinger => {
            format!("{name} did not play: no finger is on the trackpad\n")
        }
        Outcome::Handed => format!(
            "sent {name} to the trackpad: the system plays it only while a finger is on it\n"
        ),
        Outcome::NoTrackpad => {
            format!("{name} did not play: this computer has no trackpad that vibrates\n")
        }
        Outcome::Unplayable => {
            format!("{name} did not play: nothing in it is one this trackpad can play\n")
        }
        Outcome::Failed => format!("{name} did not play: the system could not start it\n"),
        Outcome::HapticsOff => format!(
            "{name} did not play: vibrations are off in bateri's settings (feedback.haptics)\n"
        ),
        Outcome::SoundsOff => {
            format!("{name} did not play: sounds are off in bateri's settings (feedback.sounds)\n")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(reply: Reply) -> String {
        match reply {
            Reply::Text(text) => text,
            other => panic!("a text was said, not {other:?}"),
        }
    }

    #[test]
    fn with_no_name_the_names_there_are_are_said_one_to_a_line() {
        for (ready, names) in [
            (Ready::Haptic, &haptic::NAMES[..]),
            (Ready::Sound, &sound::NAMES[..]),
        ] {
            let said = text(reply(ready, None, |_| panic!("nothing is played")));
            assert_eq!(said.lines().collect::<Vec<_>>(), names);
            assert!(said.ends_with('\n'));
        }
        // They are the kit's: a name of one is no name of the other.
        assert_ne!(Ready::Haptic.names(), Ready::Sound.names());
    }

    #[test]
    fn a_name_is_played_once_and_what_came_of_it_is_one_line() {
        for (outcome, said) in [
            (Outcome::Played, "played tick\n"),
            (
                Outcome::NoFinger,
                "tick did not play: no finger is on the trackpad\n",
            ),
            (
                Outcome::Handed,
                "sent tick to the trackpad: the system plays it only while a finger is on it\n",
            ),
            (
                Outcome::NoTrackpad,
                "tick did not play: this computer has no trackpad that vibrates\n",
            ),
            (
                Outcome::Unplayable,
                "tick did not play: nothing in it is one this trackpad can play\n",
            ),
            (
                Outcome::Failed,
                "tick did not play: the system could not start it\n",
            ),
            (
                Outcome::HapticsOff,
                "tick did not play: vibrations are off in bateri's settings (feedback.haptics)\n",
            ),
            (
                Outcome::SoundsOff,
                "tick did not play: sounds are off in bateri's settings (feedback.sounds)\n",
            ),
        ] {
            let mut asked = Vec::new();
            let answer = reply(Ready::Haptic, Some("tick"), |name| {
                asked.push(name.to_owned());
                Some(outcome)
            });
            assert_eq!(text(answer), said);
            assert_eq!(asked, ["tick"]);
        }
    }

    #[test]
    fn what_came_of_a_name_is_said_the_same_whenever_it_is_known() {
        let at_once = reply(Ready::Haptic, Some("tick"), |_| Some(Outcome::NoFinger));
        assert_eq!(text(at_once), text(said("tick", Outcome::NoFinger)));
    }

    #[test]
    fn a_name_that_is_not_ready_is_an_error_that_says_the_ones_there_are() {
        for ready in [Ready::Haptic, Ready::Sound] {
            let Reply::Failed(failure) = reply(ready, Some("nope"), |_| None) else {
                panic!("an error");
            };
            assert_eq!(failure.kind, Kind::Usage);
            assert_eq!(
                failure.text,
                format!("`nope` is not a ready name: {}", ready.names().join(" "))
            );
        }
        // What the command is given is not trusted to be printable.
        let Reply::Failed(failure) = reply(Ready::Sound, Some("a\u{1b}[2Jb"), |_| None) else {
            panic!("an error");
        };
        assert!(!failure.text.contains('\u{1b}'), "{:?}", failure.text);
    }

    #[test]
    fn every_name_the_kit_says_is_asked_of_the_player_as_it_is() {
        for ready in [Ready::Haptic, Ready::Sound] {
            for name in ready.names() {
                let mut asked = None;
                let _ = reply(ready, Some(name), |given| {
                    asked = Some(given.to_owned());
                    Some(Outcome::Played)
                });
                assert_eq!(asked.as_deref(), Some(*name));
            }
        }
    }
}
