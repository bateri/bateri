//! ⌘⌥ with two fingers, a pinch or a wheel over a floating card: the card moves, is sized about
//! its centre, or is stepped — the app's one watch of the scroll and magnify events that can
//! take them before the view under the pointer does.
//!
//! **What the watch takes is the [`Lock`]'s to say** (`bt_shell_common::card_gesture`, tested):
//! a gesture is the card's from its beginning, with ⌘ and ⌥ held (⌃ not) and the pointer over a
//! floating card of the key window, and stays the card's, momentum and all, once the keys are let
//! go; without ⌘⌥ the watch takes no event and looks at nothing, so the terminal's scrolling
//! and a page's pinch go on as before. This module only turns an `NSEvent` into the lock's
//! [`Event`] and does what the lock says to the card's container ([`SplitView`]): the movement
//! goes the way a header's drag goes (`hold_card`, `carry_card`, `let_go_card`: against the
//! margins' spring, and back inside them at the end, eased, at once under Reduce Motion), a
//! pinch the way a grip's drag sizes it ([`Area::scaled`]), and a wheel's turn the way ⌃⌥⌘ + arrow
//! moves it.
//!
//! **The watch lives here and not in a pane's module**: a pane does not reach the container
//! or the window (`make audit`), and a card is the container's.
//!
//! **No frame is asked for when nothing is held**: the watch is one block that returns the event
//! at once when no gesture holds a card and the keys are not both down, which is every scroll of
//! an ordinary session.
//!
//! [`Area::scaled`]: bt_shell_common::floating::Area::scaled

use std::cell::RefCell;
use std::ptr::NonNull;

use block2::RcBlock;
use bt_shell_common::card_gesture::{Event, Lock, Sort, Stage, Step};
use objc2::Message;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags, NSEventPhase, NSEventType};
use objc2_foundation::{MainThreadMarker, NSPoint};

use crate::app;
use crate::split_view::SplitView;

/// How far a step of a mouse wheel's line moves a card, in points: a line of a wheel is a dozen
/// points, as it is for a page (`view.rs`). A trackpad says its own points.
const WHEEL_LINE_PT: f64 = 12.0;

/// The gesture the watch holds a card for: the container of the card, and which kind of
/// gesture holds it.
struct Held {
    container: Retained<SplitView>,
    sort: Sort,
}

/// The watch's state: the lock, and the card the gesture in progress holds.
#[derive(Default)]
struct Watch {
    lock: Lock,
    held: Option<Held>,
}

/// The app's watch of the scroll and magnify events: its token, kept for the process's lifetime
/// (`AppDelegate::card_monitor`).
pub(crate) fn watch() -> Option<Retained<AnyObject>> {
    let state = RefCell::new(Watch::default());
    let block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
        // audit: a local monitor runs on the main thread, before `sendEvent:`.
        let mtm = MainThreadMarker::new().expect("a local event monitor runs on the main thread");
        // SAFETY: AppKit gives the monitor a valid event.
        let taken = unsafe { event.as_ref() };
        // A watch that is asked again while it answers (a layout that spins the run loop) lets
        // the event go on: it is the view's, as every event it does not take is.
        let swallowed = state
            .try_borrow_mut()
            .is_ok_and(|mut state| state.take(taken, mtm));
        if swallowed {
            std::ptr::null_mut()
        } else {
            event.as_ptr()
        }
    });
    // SAFETY: the block returns either the valid event it was given or null, which the monitor
    // API takes as "swallowed".
    unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(
            NSEventMask::ScrollWheel | NSEventMask::Magnify,
            &block,
        )
    }
}

impl Watch {
    /// What the watch does with `event`; `true` when the event is the card's and goes no further.
    fn take(&mut self, event: &NSEvent, mtm: MainThreadMarker) -> bool {
        let chosen = chosen(event.modifierFlags());
        // Nothing holds a card and the keys are not down: the ordinary scroll of a terminal, and
        // it costs no more than this.
        if self.lock.idle() && !chosen {
            return false;
        }
        let sort = if event.r#type() == NSEventType::Magnify {
            Sort::Pinch
        } else {
            Sort::Scroll
        };
        let momentum = if sort == Sort::Scroll {
            stage_of(event.momentumPhase())
        } else {
            Stage::None
        };
        let mut seen = Event {
            sort,
            phase: stage_of(event.phase()),
            momentum,
            chosen,
            over: None,
        };
        // What is under the pointer is asked only of what it can decide: a gesture's beginning or
        // a wheel's turn with the keys down.
        let mut under = None;
        if Lock::needs_card(&seen) {
            under = under_pointer(event, mtm);
            seen.over = under.as_ref().map(|(_, pane)| *pane);
        }
        let taken = match self.lock.event(seen) {
            Step::Pass => false,
            Step::Swallow => true,
            Step::Hold(pane) => {
                // A gesture whose end never came (the window went away) does not stay held.
                self.let_go();
                if let Some((container, _)) = under {
                    let held = match sort {
                        Sort::Scroll => container.hold_card(pane),
                        Sort::Pinch => container.begin_card_pinch(pane),
                    };
                    if held {
                        self.held = Some(Held { container, sort });
                        self.go(event);
                    }
                }
                true
            }
            Step::Go(_) => {
                self.go(event);
                true
            }
            Step::Let(_) => {
                self.go(event);
                true
            }
            Step::Step(pane) => {
                if let Some((container, _)) = under {
                    let (dx, dy) = points(
                        event.scrollingDeltaX(),
                        event.scrollingDeltaY(),
                        event.hasPreciseScrollingDeltas(),
                    );
                    container.step_card(pane, dx, dy);
                }
                true
            }
        };
        // A card the lock no longer holds is let go, whichever way the lock let it: the fingers
        // lifted, or an end that never came (a new beginning, a wheel's turn) — a card must not
        // stay carried, with the window's cursors off, for a gesture that is over.
        if !self.lock.holding() {
            self.let_go();
        }
        taken
    }

    /// The movement of `event` goes to the card the gesture holds.
    fn go(&self, event: &NSEvent) {
        let Some(held) = &self.held else {
            return;
        };
        match held.sort {
            // The card goes the way the fingers go, whichever way the system scrolls its content.
            Sort::Scroll => {
                let (dx, dy) = fingers(
                    event.scrollingDeltaX(),
                    event.scrollingDeltaY(),
                    event.isDirectionInvertedFromDevice(),
                );
                held.container.carry_card(dx, dy);
            }
            Sort::Pinch => held.container.pinch_card(event.magnification()),
        }
    }

    /// The card the gesture holds is let go: it comes to rest, or keeps the size the fingers made.
    fn let_go(&mut self) {
        let Some(held) = self.held.take() else {
            return;
        };
        match held.sort {
            Sort::Scroll => {
                held.container.let_go_card();
            }
            Sort::Pinch => {
                held.container.end_card_pinch();
            }
        }
    }
}

/// The container and the pane of the floating card of the key window under the pointer; `None` for
/// anything else: another window, a sheet's, a flat pane, a card that fills the tab.
fn under_pointer(event: &NSEvent, mtm: MainThreadMarker) -> Option<(Retained<SplitView>, u64)> {
    let window = event.window(mtm)?;
    let terminal = app::delegate(mtm)?.window_owning(&window)?;
    // A sheet's events are the sheet's, and a window the person is not in is left as it is.
    if !std::ptr::eq(terminal.ns_window(), &*window) || !window.isKeyWindow() {
        return None;
    }
    let container = terminal.selected_tab().container().retain();
    let at: NSPoint = event.locationInWindow();
    let pane = container.floating_card_at(at)?;
    Some((container, pane))
}

/// ⌘ and ⌥ held, ⌃ not: ⌃ with a scroll is the system's zoom, and ⇧ is the wheel's own axis swap.
pub(crate) fn chosen(flags: NSEventModifierFlags) -> bool {
    flags.contains(NSEventModifierFlags::Command | NSEventModifierFlags::Option)
        && !flags.contains(NSEventModifierFlags::Control)
}

/// The phase AppKit says, as the lock knows it.
pub(crate) fn stage_of(phase: NSEventPhase) -> Stage {
    if phase.contains(NSEventPhase::Began) {
        Stage::Began
    } else if phase.intersects(NSEventPhase::Ended | NSEventPhase::Cancelled) {
        Stage::Ended
    } else if phase.intersects(NSEventPhase::Changed | NSEventPhase::Stationary) {
        Stage::Changed
    } else if phase.contains(NSEventPhase::MayBegin) {
        Stage::MayBegin
    } else {
        Stage::None
    }
}

/// The fingers' own movement from a scroll's deltas: with the system's natural scrolling (the
/// device's direction inverted) the deltas are the fingers', without it the content's, which go
/// the other way.
fn fingers(dx: f64, dy: f64, natural: bool) -> (f64, f64) {
    if natural { (dx, dy) } else { (-dx, -dy) }
}

/// A step of a wheel's turn in points: a trackpad (or any device that says so) gives points, a
/// wheel its lines.
fn points(dx: f64, dy: f64, precise: bool) -> (f64, f64) {
    if precise {
        (dx, dy)
    } else {
        (dx * WHEEL_LINE_PT, dy * WHEEL_LINE_PT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_and_option_keys_together_choose_and_control_spoils_it() {
        let command = NSEventModifierFlags::Command;
        let option = NSEventModifierFlags::Option;
        assert!(chosen(command | option));
        assert!(chosen(command | option | NSEventModifierFlags::Shift));
        assert!(!chosen(command));
        assert!(!chosen(option));
        assert!(!chosen(NSEventModifierFlags::empty()));
        assert!(!chosen(command | option | NSEventModifierFlags::Control));
        assert!(!chosen(NSEventModifierFlags::Control | command));
    }

    #[test]
    fn a_phase_of_appkit_is_the_stage_the_lock_knows() {
        assert_eq!(stage_of(NSEventPhase::empty()), Stage::None);
        assert_eq!(stage_of(NSEventPhase::MayBegin), Stage::MayBegin);
        assert_eq!(stage_of(NSEventPhase::Began), Stage::Began);
        assert_eq!(stage_of(NSEventPhase::Changed), Stage::Changed);
        assert_eq!(stage_of(NSEventPhase::Stationary), Stage::Changed);
        assert_eq!(stage_of(NSEventPhase::Ended), Stage::Ended);
        assert_eq!(stage_of(NSEventPhase::Cancelled), Stage::Ended);
    }

    #[test]
    fn the_card_goes_the_way_the_fingers_go_with_natural_scrolling_or_without() {
        assert_eq!(fingers(3.0, -4.0, true), (3.0, -4.0));
        assert_eq!(fingers(3.0, -4.0, false), (-3.0, 4.0));
    }

    #[test]
    fn a_wheels_lines_are_a_dozen_points_and_a_trackpads_points_are_its_own() {
        assert_eq!(points(1.0, -2.0, false), (12.0, -24.0));
        assert_eq!(points(1.5, -2.5, true), (1.5, -2.5));
    }

    /// The watch's block is where an event goes before the view, so what it takes must be
    /// the lock's answer alone, and the app's own mask must be the two kinds it reads.
    #[test]
    fn the_watch_returns_every_event_it_does_not_take_and_reads_scroll_and_magnify_only() {
        let source = include_str!("card_gesture.rs");
        let take = &source[source.find("fn take(").expect("take")..];
        let take = &take[..take.find("\n    }\n").expect("its end")];
        // Idle and without the keys, before anything is asked of the window or the container.
        let quick = take
            .find("self.lock.idle() && !chosen")
            .expect("the quick way out");
        let asked = take
            .find("under_pointer(event, mtm)")
            .expect("asks what is under");
        assert!(quick < asked);
        // What is under the pointer is asked only of what it can decide.
        assert!(take.contains("if Lock::needs_card(&seen) {"));
        // A card the lock does not hold any more is let go, however the lock let it.
        assert!(take.contains("if !self.lock.holding() {"));
        // The event goes on unless the watch says it is taken; a busy watch lets it go on.
        assert!(source.contains("std::ptr::null_mut()"));
        assert!(source.contains(".try_borrow_mut()"));
        assert!(source.contains("NSEventMask::ScrollWheel | NSEventMask::Magnify,"));
    }
}
