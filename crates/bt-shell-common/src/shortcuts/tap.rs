//! Counting a modifier pressed twice (`⇧ ⇧`): the one machine behind the live trigger and the
//! recorder of Settings ▸ Keyboard.
//!
//! The toolkit feeds it what it sees — the modifiers held after each change, and "something else
//! happened" for a key, a click or a scroll — each with the time the event carries, and it says
//! when a modifier has been tapped once and when twice. It reads no clock and sets none: a timer
//! would be a second opinion on what the events already say, and the events' own times make the
//! rules below tried with plain numbers.
//!
//! A **tap** is one modifier going down and coming up alone in under `tap` seconds; the
//! **double press** is a second tap whose press comes within `gap` seconds of the first one's
//! release, and it is reported when that second tap is *complete* — a ⇧ that is still held may
//! yet be a capital letter, or a hint. What spoils the count:
//!
//! - a key, a click or a scroll between the taps ([`Taps::interrupt`]) — and one during a tap, so
//!   the ⇧ of a capital letter never counts;
//! - a press that lasts `tap` seconds or longer, such as ⌘ held for the tab strip's hints;
//! - another modifier, held with the first or tapped between the two: the count starts again
//!   from the new one;
//! - a gap that is too long: the late tap is the first of a new count.
//!
//! The left and the right key are the same key: the toolkit's flags do not tell them apart. The
//! fn key is counted too, only to be refused — macOS keeps it twice for itself.

use bt_core::{Modifier, Mods};

use super::mapping::modifier_glyph;

/// What was tapped: one of the four modifiers a shortcut uses, or fn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tapped {
    Modifier(Modifier),
    /// fn (🌐): never a shortcut's.
    Function,
}

impl Tapped {
    /// How the key is shown in a note or a field: `⇧`, `fn`.
    pub fn glyph(self) -> String {
        match self {
            Self::Modifier(modifier) => modifier_glyph(modifier).to_string(),
            Self::Function => "fn".to_owned(),
        }
    }
}

/// The modifier keys held at a moment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Held {
    pub mods: Mods,
    pub function: bool,
}

impl Held {
    fn is_empty(self) -> bool {
        self.mods.is_empty() && !self.function
    }

    /// The one key held, when it is the only one.
    fn alone(self) -> Option<Tapped> {
        let mut keys = self
            .mods
            .iter()
            .map(Tapped::Modifier)
            .chain(self.function.then_some(Tapped::Function));
        let key = keys.next()?;
        keys.next().is_none().then_some(key)
    }
}

/// What an event came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Nothing to tell.
    Nothing,
    /// A tap, the first of a possible two: the recorder says "once more…".
    Once(Tapped),
    /// The second tap: the double press.
    Twice(Tapped),
}

/// The count of taps.
#[derive(Clone, Debug)]
pub struct Taps {
    /// A press shorter than this, in seconds, is a tap.
    tap: f64,
    /// A second press within this long, in seconds, of the first tap's release is its pair.
    gap: f64,
    held: Held,
    /// The key held alone now and when it went down.
    down: Option<(Tapped, f64)>,
    /// The tap that was made and when it was let go.
    once: Option<(Tapped, f64)>,
}

impl Taps {
    /// A count where a tap lasts less than `tap` seconds and the second one starts within `gap`
    /// seconds of the first one's release.
    pub fn new(tap: f64, gap: f64) -> Taps {
        Taps {
            tap,
            gap,
            held: Held::default(),
            down: None,
            once: None,
        }
    }

    /// The modifiers held after a change, at the time `at` (seconds).
    pub fn flags(&mut self, at: f64, held: Held) -> Step {
        let before = std::mem::replace(&mut self.held, held);
        if held == before {
            return Step::Nothing;
        }
        if !held.is_empty() {
            // A press from nothing, of one key, begins a tap; anything else — two keys, or one
            // let go of the pair — is no tap and spoils the count.
            match (before.is_empty(), held.alone()) {
                (true, Some(key)) => {
                    self.down = Some((key, at));
                    if self
                        .once
                        .is_some_and(|(first, released)| first != key || at - released > self.gap)
                    {
                        self.once = None;
                    }
                }
                _ => self.interrupt(),
            }
            return Step::Nothing;
        }
        let Some((key, pressed)) = self.down.take() else {
            self.once = None;
            return Step::Nothing;
        };
        if at - pressed >= self.tap {
            self.once = None;
            return Step::Nothing;
        }
        match self.once.take() {
            Some(_) => Step::Twice(key),
            None => {
                self.once = Some((key, at));
                Step::Once(key)
            }
        }
    }

    /// A key, a click or a scroll: whatever the user was counting is lost.
    pub fn interrupt(&mut self) {
        self.down = None;
        self.once = None;
    }

    /// The key whose second tap is awaited at `at` (seconds): one tap was made and the gap has
    /// not run out.
    pub fn waiting(&self, at: f64) -> Option<Tapped> {
        self.once
            .filter(|(_, released)| at - released <= self.gap)
            .map(|(key, _)| key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TAP: f64 = 0.3;
    const GAP: f64 = 0.5;

    const SHIFT: Held = held(Modifier::Shift);
    const OPTION: Held = held(Modifier::Option);
    const CMD: Held = held(Modifier::Cmd);
    const NONE: Held = Held {
        mods: Mods::NONE,
        function: false,
    };
    const FN: Held = Held {
        mods: Mods::NONE,
        function: true,
    };

    const fn held(modifier: Modifier) -> Held {
        Held {
            mods: Mods::NONE.with(modifier),
            function: false,
        }
    }

    fn shift() -> Tapped {
        Tapped::Modifier(Modifier::Shift)
    }

    fn both(first: Modifier, second: Modifier) -> Held {
        Held {
            mods: Mods::NONE.with(first).with(second),
            function: false,
        }
    }

    /// Feeds `(time, held)` pairs and returns what each one said, but the quiet ones.
    fn feed(taps: &mut Taps, events: &[(f64, Held)]) -> Vec<Step> {
        events
            .iter()
            .map(|(at, held)| taps.flags(*at, *held))
            .filter(|step| *step != Step::Nothing)
            .collect()
    }

    fn counting() -> Taps {
        Taps::new(TAP, GAP)
    }

    #[test]
    fn two_quick_taps_are_a_double_press_when_the_second_is_over() {
        let mut taps = counting();
        let said = feed(
            &mut taps,
            &[
                (1.00, SHIFT),
                (1.08, NONE),
                (1.25, SHIFT),
                // The second press alone says nothing: it may yet be held.
            ],
        );
        assert_eq!(said, [Step::Once(shift())]);
        assert_eq!(taps.flags(1.33, NONE), Step::Twice(shift()));
    }

    #[test]
    fn a_press_that_lasts_is_no_tap() {
        // Held past the tap: the tab strip's hint, not a tap — and it does not start a count.
        let mut taps = counting();
        let said = feed(
            &mut taps,
            &[(1.0, SHIFT), (1.3, NONE), (1.4, SHIFT), (1.45, NONE)],
        );
        assert_eq!(
            said,
            [Step::Once(shift())],
            "the long press is not the first"
        );
        // The second tap too long to be one.
        let mut taps = counting();
        let said = feed(
            &mut taps,
            &[(1.0, SHIFT), (1.1, NONE), (1.2, SHIFT), (1.6, NONE)],
        );
        assert_eq!(said, [Step::Once(shift())]);
        assert_eq!(
            taps.waiting(1.7),
            None,
            "a long second press spoils the first"
        );
    }

    #[test]
    fn a_second_tap_after_the_gap_is_the_first_of_a_new_count() {
        let mut taps = counting();
        let said = feed(
            &mut taps,
            &[(1.0, SHIFT), (1.1, NONE), (1.7, SHIFT), (1.8, NONE)],
        );
        assert_eq!(said, [Step::Once(shift()), Step::Once(shift())]);
        // And that one pairs with the next.
        assert_eq!(
            feed(&mut taps, &[(2.0, SHIFT), (2.1, NONE)]),
            [Step::Twice(shift())]
        );
    }

    #[test]
    fn the_gap_is_from_the_release_of_the_first_to_the_press_of_the_second() {
        let mut taps = counting();
        // The first tap lasts a quarter second; the second begins half a second after its end.
        let said = feed(
            &mut taps,
            &[(1.00, SHIFT), (1.25, NONE), (1.75, SHIFT), (1.80, NONE)],
        );
        assert_eq!(said, [Step::Once(shift()), Step::Twice(shift())]);
    }

    #[test]
    fn a_key_a_click_or_a_scroll_between_the_taps_spoils_the_count() {
        let mut taps = counting();
        feed(&mut taps, &[(1.0, SHIFT), (1.1, NONE)]);
        taps.interrupt();
        assert_eq!(taps.waiting(1.2), None);
        assert_eq!(
            feed(&mut taps, &[(1.2, SHIFT), (1.3, NONE)]),
            [Step::Once(shift())],
            "the tap after it is the first of a new count"
        );
    }

    #[test]
    fn a_shift_that_made_a_capital_letter_is_not_a_tap() {
        // "Hello": ⇧ down, the key, ⇧ up — and again.
        let mut taps = counting();
        assert_eq!(taps.flags(1.00, SHIFT), Step::Nothing);
        taps.interrupt();
        assert_eq!(taps.flags(1.10, NONE), Step::Nothing);
        assert_eq!(taps.flags(1.20, SHIFT), Step::Nothing);
        taps.interrupt();
        assert_eq!(taps.flags(1.30, NONE), Step::Nothing);
        // A plain tap just after is a first one, not the second of anything.
        assert_eq!(
            feed(&mut taps, &[(1.4, SHIFT), (1.5, NONE)]),
            [Step::Once(shift())]
        );
    }

    #[test]
    fn another_modifier_starts_the_count_again() {
        let mut taps = counting();
        let option = Tapped::Modifier(Modifier::Option);
        // ⇧, ⌥, ⌥: the second ⌥ pairs with the first, not with the ⇧.
        let said = feed(
            &mut taps,
            &[
                (1.0, SHIFT),
                (1.1, NONE),
                (1.2, OPTION),
                (1.3, NONE),
                (1.4, OPTION),
                (1.5, NONE),
            ],
        );
        assert_eq!(
            said,
            [Step::Once(shift()), Step::Once(option), Step::Twice(option)]
        );
        // ⇧, ⌥, ⇧ is nothing.
        let mut taps = counting();
        let said = feed(
            &mut taps,
            &[
                (1.0, SHIFT),
                (1.1, NONE),
                (1.2, OPTION),
                (1.3, NONE),
                (1.4, SHIFT),
                (1.5, NONE),
            ],
        );
        assert!(
            said.iter().all(|step| matches!(step, Step::Once(_))),
            "{said:?}"
        );
    }

    #[test]
    fn two_modifiers_together_are_no_tap() {
        let mut taps = counting();
        let said = feed(
            &mut taps,
            &[
                (1.0, SHIFT),
                (1.02, both(Modifier::Shift, Modifier::Cmd)),
                (1.1, CMD),
                (1.15, NONE),
            ],
        );
        assert!(
            said.is_empty(),
            "⇧⌘ pressed and let go is a chord: {said:?}"
        );
        // One of two let go leaves the other, which is not a fresh press.
        assert_eq!(taps.flags(2.0, SHIFT), Step::Nothing);
        assert_eq!(taps.flags(2.05, NONE), Step::Once(shift()));
    }

    #[test]
    fn left_and_right_are_one_key() {
        // The flags carry no side: the right ⇧ goes down while the left is held, and nothing
        // changes until both are up.
        let mut taps = counting();
        let said = feed(
            &mut taps,
            &[
                (1.00, SHIFT),
                (1.02, SHIFT),
                (1.05, SHIFT),
                (1.08, NONE),
                (1.20, SHIFT),
                (1.25, NONE),
            ],
        );
        assert_eq!(said, [Step::Once(shift()), Step::Twice(shift())]);
    }

    #[test]
    fn a_third_tap_begins_a_new_count() {
        let mut taps = counting();
        let said = feed(
            &mut taps,
            &[
                (1.0, SHIFT),
                (1.1, NONE),
                (1.2, SHIFT),
                (1.3, NONE),
                (1.4, SHIFT),
                (1.5, NONE),
            ],
        );
        assert_eq!(
            said,
            [
                Step::Once(shift()),
                Step::Twice(shift()),
                Step::Once(shift())
            ]
        );
    }

    #[test]
    fn fn_is_counted_so_that_it_can_be_refused() {
        let mut taps = counting();
        let said = feed(&mut taps, &[(1.0, FN), (1.1, NONE), (1.2, FN), (1.3, NONE)]);
        assert_eq!(
            said,
            [Step::Once(Tapped::Function), Step::Twice(Tapped::Function)]
        );
    }

    #[test]
    fn what_is_awaited_is_known_until_the_gap_runs_out() {
        let mut taps = counting();
        assert_eq!(taps.waiting(0.0), None);
        feed(&mut taps, &[(1.0, SHIFT), (1.1, NONE)]);
        assert_eq!(taps.waiting(1.2), Some(shift()));
        assert_eq!(taps.waiting(1.5), Some(shift()));
        assert_eq!(taps.waiting(1.7), None);
    }

    #[test]
    fn a_release_with_nothing_down_says_nothing() {
        // The count starts with a modifier already held (the app was not in front when it went
        // down): its release is no tap.
        let mut taps = counting();
        assert_eq!(taps.flags(5.0, NONE), Step::Nothing);
        taps.flags(6.0, SHIFT);
        taps.interrupt();
        assert_eq!(taps.flags(6.1, NONE), Step::Nothing);
    }
}
