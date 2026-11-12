//! Which events of a trackpad or a mouse wheel a floating card takes: ⌘⌥ with two fingers moves
//! the card, with a pinch it sizes it, with a wheel it steps it — a state that never sees
//! `NSEvent` and is tested.
//!
//! The monitor that watches the app's scroll and magnify events turns each into an [`Event`] and
//! does what the [`Lock`] answers. The lock is what makes a gesture the card's whole: it is
//! taken at the gesture's beginning, **with** the keys held and the pointer over a card, and it
//! keeps every event of that gesture — the fingers' movement, the momentum after them — even
//! once the keys are let go, so that the tail of a gesture is never the terminal's scrolling.
//! Without ⌘⌥ it takes nothing at all: a gesture that began without them is the view's, and one
//! that is the card's is not given up for a key.

/// The part of a gesture an event is, as AppKit's phases say it (a cancelled gesture is an ended
/// one, a stationary finger a changing one).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Stage {
    /// Not in a phase: a wheel's step, or an event of the other kind (a scroll's own phase while
    /// its momentum has one).
    #[default]
    None,
    /// The fingers are on the pad and may move.
    MayBegin,
    Began,
    Changed,
    Ended,
}

/// What an event is of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sort {
    /// Two fingers' scrolling, its momentum, or a wheel's turn.
    Scroll,
    /// A pinch.
    Pinch,
}

/// One scroll or magnify event, as the lock needs it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    pub sort: Sort,
    /// The fingers' own phase (a pinch has only this one).
    pub phase: Stage,
    /// The momentum's phase, after the fingers have left.
    pub momentum: Stage,
    /// ⌘ and ⌥ held, ⌃ not (⌃ with a scroll is the system's zoom).
    pub chosen: bool,
    /// The pane of the floating card under the pointer, if one is: what the event would take.
    pub over: Option<u64>,
}

/// What the monitor does with an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Not the card's: the event goes on to the view.
    Pass,
    /// The card's, and nothing to do: swallowed (a momentum's tail, the fingers coming down on a
    /// card before they move).
    Swallow,
    /// A gesture takes the card of this pane: it is taken, then the event's movement is applied.
    Hold(u64),
    /// The movement of the gesture that holds the card of this pane.
    Go(u64),
    /// The fingers left: the movement of the event is applied, then the card is let go (it comes
    /// to rest, a pinch's size is kept).
    Let(u64),
    /// A wheel's turn over the card of this pane, with the keys held: the card is stepped.
    Step(u64),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum State {
    #[default]
    Idle,
    /// The fingers are down on a card's gesture.
    Held(Sort, u64),
    /// The fingers are up; a scroll's momentum may follow and is the card's still.
    Tail,
}

/// The lock of the gesture that holds a card: [`Lock::event`] for each event, in order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Lock {
    state: State,
}

impl Lock {
    /// Whether a gesture holds a card, or its momentum is yet to end: the monitor looks at the
    /// keys and at what is under the pointer only when this is not so.
    pub fn idle(&self) -> bool {
        self.state == State::Idle
    }

    /// Whether a gesture holds a card now: the monitor lets the card of a gesture the lock no
    /// longer holds go, whatever the way the lock let it — an end that came, or one that never
    /// reached us.
    pub fn holding(&self) -> bool {
        matches!(self.state, State::Held(..))
    }

    /// Whether `event` is one the pointer's place can decide: a gesture's beginning or a
    /// wheel's turn, with the keys held. The monitor asks what is under the pointer only
    /// then ([`Event::over`]).
    pub fn needs_card(event: &Event) -> bool {
        event.chosen && fresh(event)
    }

    /// What to do with `event`.
    pub fn event(&mut self, event: Event) -> Step {
        // A new gesture, or a wheel's turn, is a new question: whatever was left of the last one
        // (a momentum that did not come, an end that never reached us — the app lost the key, a
        // busy watch let it by) is over, and what it held is let go ([`Lock::holding`]).
        if fresh(&event) {
            self.state = State::Idle;
        }
        match self.state {
            State::Held(sort, pane) => self.held(sort, pane, event),
            State::Tail => self.tail(event),
            State::Idle => self.idle_event(event),
        }
    }

    fn idle_event(&mut self, event: Event) -> Step {
        let Some(pane) = event.over.filter(|_| event.chosen) else {
            return Step::Pass;
        };
        if wheel(&event) {
            return Step::Step(pane);
        }
        match event.phase {
            // Only a gesture that begins here, with the keys held, is the card's: one that began
            // without them, or whose beginning went to another view, is not.
            Stage::Began => {
                self.state = State::Held(event.sort, pane);
                Step::Hold(pane)
            }
            // The fingers coming down on a card: the view does not hear of a touch whose
            // beginning and end it would never be told the rest of.
            Stage::MayBegin => Step::Swallow,
            _ => Step::Pass,
        }
    }

    fn held(&mut self, sort: Sort, pane: u64, event: Event) -> Step {
        // The other kind of event is no part of the gesture that holds the card (the lock is
        // not left holding it for what it never sees the end of): it is the view's.
        if event.sort != sort {
            return Step::Pass;
        }
        if event.phase == Stage::Ended {
            // A scroll's momentum may follow its fingers; a pinch has none.
            self.state = if sort == Sort::Scroll {
                State::Tail
            } else {
                State::Idle
            };
            return Step::Let(pane);
        }
        if event.momentum != Stage::None {
            return Step::Swallow;
        }
        Step::Go(pane)
    }

    fn tail(&mut self, event: Event) -> Step {
        if event.sort != Sort::Scroll || event.momentum == Stage::None {
            return Step::Pass;
        }
        if event.momentum == Stage::Ended {
            self.state = State::Idle;
        }
        Step::Swallow
    }
}

/// A wheel's turn: no phase of the fingers' and none of a momentum's.
fn wheel(event: &Event) -> bool {
    event.sort == Sort::Scroll && event.phase == Stage::None && event.momentum == Stage::None
}

/// An event that begins something: a gesture's first (the fingers coming down or moving) or a
/// wheel's turn.
fn fresh(event: &Event) -> bool {
    wheel(event) || matches!(event.phase, Stage::Began | Stage::MayBegin)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CARD: u64 = 7;

    fn scroll(phase: Stage, momentum: Stage, chosen: bool, over: Option<u64>) -> Event {
        Event {
            sort: Sort::Scroll,
            phase,
            momentum,
            chosen,
            over,
        }
    }

    fn pinch(phase: Stage, chosen: bool, over: Option<u64>) -> Event {
        Event {
            sort: Sort::Pinch,
            phase,
            momentum: Stage::None,
            chosen,
            over,
        }
    }

    #[test]
    fn nothing_is_taken_without_command_and_option_whatever_is_under_the_pointer() {
        let mut lock = Lock::default();
        let gesture = [
            scroll(Stage::MayBegin, Stage::None, false, Some(CARD)),
            scroll(Stage::Began, Stage::None, false, Some(CARD)),
            scroll(Stage::Changed, Stage::None, false, Some(CARD)),
            scroll(Stage::Ended, Stage::None, false, Some(CARD)),
            scroll(Stage::None, Stage::Began, false, Some(CARD)),
            scroll(Stage::None, Stage::Changed, false, Some(CARD)),
            scroll(Stage::None, Stage::Ended, false, Some(CARD)),
            // A wheel's turn.
            scroll(Stage::None, Stage::None, false, Some(CARD)),
            pinch(Stage::Began, false, Some(CARD)),
            pinch(Stage::Changed, false, Some(CARD)),
            pinch(Stage::Ended, false, Some(CARD)),
        ];
        for event in gesture {
            assert_eq!(lock.event(event), Step::Pass, "{event:?}");
            assert!(lock.idle());
        }
    }

    #[test]
    fn nothing_is_taken_with_the_keys_held_where_no_floating_card_is() {
        let mut lock = Lock::default();
        for event in [
            scroll(Stage::Began, Stage::None, true, None),
            scroll(Stage::Changed, Stage::None, true, None),
            scroll(Stage::Ended, Stage::None, true, None),
            scroll(Stage::None, Stage::None, true, None),
            pinch(Stage::Began, true, None),
            pinch(Stage::Ended, true, None),
        ] {
            assert_eq!(lock.event(event), Step::Pass, "{event:?}");
        }
        assert!(lock.idle());
    }

    #[test]
    fn a_scroll_begun_over_a_card_with_the_keys_is_the_cards_to_its_momentums_end() {
        let mut lock = Lock::default();
        // The fingers coming down are the card's too: the view is not told of a touch whose
        // movement and end it would never hear.
        assert_eq!(
            lock.event(scroll(Stage::MayBegin, Stage::None, true, Some(CARD))),
            Step::Swallow
        );
        assert!(lock.idle());
        assert_eq!(
            lock.event(scroll(Stage::Began, Stage::None, true, Some(CARD))),
            Step::Hold(CARD)
        );
        assert!(lock.holding());
        assert!(!lock.idle());
        // The keys are let go and the pointer leaves the card: the gesture is the card's still.
        assert_eq!(
            lock.event(scroll(Stage::Changed, Stage::None, false, None)),
            Step::Go(CARD)
        );
        assert_eq!(
            lock.event(scroll(Stage::Ended, Stage::None, false, None)),
            Step::Let(CARD)
        );
        // The momentum that follows is swallowed, and moves nothing.
        assert_eq!(
            lock.event(scroll(Stage::None, Stage::Began, false, None)),
            Step::Swallow
        );
        assert_eq!(
            lock.event(scroll(Stage::None, Stage::Changed, false, None)),
            Step::Swallow
        );
        assert!(!lock.idle());
        assert_eq!(
            lock.event(scroll(Stage::None, Stage::Ended, false, None)),
            Step::Swallow
        );
        assert!(lock.idle());
        // And what comes after is no one's but the view's.
        assert_eq!(
            lock.event(scroll(Stage::Changed, Stage::None, false, None)),
            Step::Pass
        );
    }

    #[test]
    fn a_scroll_that_began_without_the_keys_is_not_taken_when_they_are_pressed_in_it() {
        let mut lock = Lock::default();
        assert_eq!(
            lock.event(scroll(Stage::Began, Stage::None, false, Some(CARD))),
            Step::Pass
        );
        assert_eq!(
            lock.event(scroll(Stage::Changed, Stage::None, true, Some(CARD))),
            Step::Pass
        );
        assert_eq!(
            lock.event(scroll(Stage::Ended, Stage::None, true, Some(CARD))),
            Step::Pass
        );
        // Nor is its momentum.
        assert_eq!(
            lock.event(scroll(Stage::None, Stage::Began, true, Some(CARD))),
            Step::Pass
        );
        assert!(lock.idle());
    }

    #[test]
    fn a_scroll_that_ends_without_momentum_leaves_the_next_gesture_free() {
        let mut lock = Lock::default();
        lock.event(scroll(Stage::Began, Stage::None, true, Some(CARD)));
        assert_eq!(
            lock.event(scroll(Stage::Ended, Stage::None, true, Some(CARD))),
            Step::Let(CARD)
        );
        // No momentum came: the next scroll, without the keys, begins as the view's.
        assert_eq!(
            lock.event(scroll(Stage::Began, Stage::None, false, Some(CARD))),
            Step::Pass
        );
        assert!(lock.idle());
        // With them, it is the card's again — the same card, or another.
        let mut lock = Lock::default();
        lock.event(scroll(Stage::Began, Stage::None, true, Some(CARD)));
        lock.event(scroll(Stage::Ended, Stage::None, true, Some(CARD)));
        assert_eq!(
            lock.event(scroll(Stage::Began, Stage::None, true, Some(CARD + 1))),
            Step::Hold(CARD + 1)
        );
    }

    #[test]
    fn a_pinch_with_the_keys_sizes_the_card_and_lets_go_at_its_end() {
        let mut lock = Lock::default();
        assert_eq!(
            lock.event(pinch(Stage::Began, true, Some(CARD))),
            Step::Hold(CARD)
        );
        assert_eq!(
            lock.event(pinch(Stage::Changed, false, None)),
            Step::Go(CARD)
        );
        // A scroll event in the middle of it is no part of the pinch: the view's.
        assert_eq!(
            lock.event(scroll(Stage::Changed, Stage::None, false, None)),
            Step::Pass
        );
        assert!(lock.holding());
        assert_eq!(
            lock.event(pinch(Stage::Ended, false, None)),
            Step::Let(CARD)
        );
        assert!(lock.idle());
        assert_eq!(lock.event(pinch(Stage::Began, false, None)), Step::Pass);
    }

    #[test]
    fn a_beginning_of_the_other_kind_ends_the_hold_and_is_judged_on_its_own() {
        let mut lock = Lock::default();
        lock.event(pinch(Stage::Began, true, Some(CARD)));
        assert!(lock.holding());
        assert_eq!(
            lock.event(scroll(Stage::Began, Stage::None, true, Some(CARD + 1))),
            Step::Hold(CARD + 1)
        );
        // The old pinch's late end is the view's, and the new hold is not spoiled by it.
        assert_eq!(lock.event(pinch(Stage::Ended, false, None)), Step::Pass);
        assert!(lock.holding());
        // Without the keys the beginning is the view's, and nothing is held any more.
        let mut lock = Lock::default();
        lock.event(pinch(Stage::Began, true, Some(CARD)));
        assert_eq!(
            lock.event(scroll(Stage::Began, Stage::None, false, Some(CARD))),
            Step::Pass
        );
        assert!(!lock.holding());
    }

    #[test]
    fn a_gesture_whose_end_never_came_leaves_every_scroll_and_pinch_to_the_view() {
        // A pinch held, its end lost: the scroll of the terminal is not dead after it.
        let mut lock = Lock::default();
        lock.event(pinch(Stage::Began, true, Some(CARD)));
        assert_eq!(
            lock.event(scroll(Stage::Changed, Stage::None, false, None)),
            Step::Pass
        );
        assert_eq!(
            lock.event(scroll(Stage::None, Stage::None, false, Some(CARD))),
            Step::Pass
        );
        assert!(!lock.holding());
        // A scroll held, its end lost: a wheel without the keys is the view's, not the card's.
        let mut lock = Lock::default();
        lock.event(scroll(Stage::Began, Stage::None, true, Some(CARD)));
        assert_eq!(
            lock.event(scroll(Stage::None, Stage::None, false, Some(CARD))),
            Step::Pass
        );
        assert!(!lock.holding());
        // And so is a pinch of the view's.
        let mut lock = Lock::default();
        lock.event(scroll(Stage::Began, Stage::None, true, Some(CARD)));
        assert_eq!(
            lock.event(pinch(Stage::Began, false, Some(CARD))),
            Step::Pass
        );
        assert!(!lock.holding());
    }

    #[test]
    fn the_pointers_place_is_asked_only_of_a_beginning_or_a_wheel_with_the_keys_held() {
        let ask = |event: Event| Lock::needs_card(&event);
        assert!(ask(scroll(Stage::Began, Stage::None, true, None)));
        assert!(ask(scroll(Stage::MayBegin, Stage::None, true, None)));
        assert!(ask(scroll(Stage::None, Stage::None, true, None)));
        assert!(ask(pinch(Stage::Began, true, None)));
        assert!(!ask(scroll(Stage::Began, Stage::None, false, None)));
        assert!(!ask(scroll(Stage::Changed, Stage::None, true, None)));
        assert!(!ask(scroll(Stage::Ended, Stage::None, true, None)));
        assert!(!ask(scroll(Stage::None, Stage::Changed, true, None)));
        assert!(!ask(pinch(Stage::Changed, true, None)));
    }

    #[test]
    fn a_pinch_that_began_without_the_keys_is_the_views() {
        let mut lock = Lock::default();
        assert_eq!(
            lock.event(pinch(Stage::Began, false, Some(CARD))),
            Step::Pass
        );
        assert_eq!(
            lock.event(pinch(Stage::Changed, true, Some(CARD))),
            Step::Pass
        );
        assert_eq!(
            lock.event(pinch(Stage::Ended, true, Some(CARD))),
            Step::Pass
        );
    }

    #[test]
    fn a_wheel_with_the_keys_over_a_card_steps_it_and_holds_nothing() {
        let mut lock = Lock::default();
        let turn = scroll(Stage::None, Stage::None, true, Some(CARD));
        assert_eq!(lock.event(turn), Step::Step(CARD));
        assert!(lock.idle());
        assert_eq!(lock.event(turn), Step::Step(CARD));
        // Over no card, or without the keys, it is the view's.
        assert_eq!(
            lock.event(scroll(Stage::None, Stage::None, true, None)),
            Step::Pass
        );
        assert_eq!(
            lock.event(scroll(Stage::None, Stage::None, false, Some(CARD))),
            Step::Pass
        );
    }

    #[test]
    fn a_gesture_that_lost_its_end_does_not_hold_the_next_one() {
        let mut lock = Lock::default();
        lock.event(scroll(Stage::Began, Stage::None, true, Some(CARD)));
        // The end never came (the window went away): a new beginning is judged on its own.
        assert_eq!(
            lock.event(scroll(Stage::Began, Stage::None, false, Some(CARD))),
            Step::Pass
        );
        assert!(lock.idle());
    }
}
