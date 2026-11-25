//! A pattern as the motor is driven: what a page says it wants to be felt (strengths and
//! sharpnesses over time, `pencere_view::haptic`) as a list of short bursts of one frequency, each at
//! a time from the start of the pattern.
//!
//! Pure: nothing here touches a device, so the numbers a person feels are all here and can be
//! checked without one. **A strength is the motor's own share**: an `intensity` of 1 is all the
//! motor gives, 0.3 is a firm tap and 0.18 a light one on the trackpads tried, and the same number
//! drives the built-in trackpad and an external one alike (there is no gain per device). The ready
//! patterns are written on that scale. **The frequencies and the lengths are first guesses, not
//! tuned**: how a sharpness feels can only be told with a finger on the glass, so the frequencies,
//! the length of a burst and of a piece, the moment two bursts are one and how late a piece may
//! come ([`super::schedule::MOST_LATE`]) are named constants to change after trying them on the
//! device.

use std::time::Duration;

use bt_shell_common::native_view::haptic::Pattern;

/// The frequency a sharpness of 0 (the dullest, a thud) is played at, in hertz.
pub(super) const DULL_HZ: f32 = 90.0;
/// The frequency a sharpness of 1 (the crispest, a click) is played at, in hertz.
pub(super) const CRISP_HZ: f32 = 300.0;
/// The most strength the motor is asked for, and the level of an `intensity` of 1: the whole of
/// what it gives. `intensity` is the share of this, so a level is `intensity * MOST_LEVEL` and a
/// strength past it is held to it, never scaled beyond. It is 1 because the number *is* the share:
/// were it less, a page's 1 would be a fraction of the motor and the scale a page is told of
/// (0.18 light, 0.3 firm, 1 the whole) would no longer be the motor's.
pub(super) const MOST_LEVEL: f32 = 1.0;
/// The least strength worth a burst: below it nothing is felt and the call is saved.
pub(super) const LEAST_LEVEL: f32 = 0.03;
/// How many whole waves a tap's burst has, before it is held between the two lengths below.
pub(super) const TAP_CYCLES: f32 = 2.0;
/// The shortest and the longest burst of a tap, in milliseconds.
pub(super) const SHORTEST_TAP_MS: f32 = 6.0;
pub(super) const LONGEST_TAP_MS: f32 = 24.0;
/// How long each piece is that a held event is cut into, in milliseconds: its strength and sharpness
/// are those of the middle of the piece.
pub(super) const PIECE_MS: f32 = 10.0;
/// Bursts that start within this of the first of them are one moment, of which the strongest plays:
/// the bursts of a held event are a piece long, and a burst a few milliseconds from the next is not
/// a shape a person could tell from one of them.
pub(super) const SAME_MOMENT_MS: f32 = 4.0;

/// One burst of the motor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Step {
    /// When it starts, from the start of the pattern (an offset: the sum of nothing earlier).
    pub at: Duration,
    /// The frequency of its sine, in hertz.
    pub hz: f32,
    /// How strong, from 0 to [`MOST_LEVEL`].
    pub level: f32,
    /// How long, in milliseconds.
    pub ms: f32,
    /// It is a tap, not a piece of a held event: the only kind a device that takes no more than
    /// a click plays.
    pub tap: bool,
}

/// The frequency of `sharpness` (from 0 to 1; a number past them is the nearest): the band is
/// crossed by ratio, as the ear and the hand tell a pitch, not by difference.
pub(super) fn frequency(sharpness: f32) -> f32 {
    let sharpness = if sharpness.is_nan() {
        0.0
    } else {
        sharpness.clamp(0.0, 1.0)
    };
    DULL_HZ * (CRISP_HZ / DULL_HZ).powf(sharpness)
}

/// The motor's strength for `intensity`, which is the motor's own share: `intensity` of the most it
/// gives, held to that at most.
pub(super) fn level(intensity: f32) -> f32 {
    if intensity.is_nan() {
        0.0
    } else {
        (intensity * MOST_LEVEL).clamp(0.0, MOST_LEVEL)
    }
}

/// How long a tap of `hz` lasts: a few whole waves, between the shortest and the longest burst.
fn tap_ms(hz: f32) -> f32 {
    (TAP_CYCLES * 1000.0 / hz).clamp(SHORTEST_TAP_MS, LONGEST_TAP_MS)
}

fn lerp(from: f32, to: f32, by: f32) -> f32 {
    from + (to - from) * by
}

fn at(seconds: f32) -> Duration {
    Duration::from_secs_f32(seconds.max(0.0))
}

/// The bursts of `pattern`, in the order of their times. A tap is one burst; a held event is cut
/// into pieces of [`PIECE_MS`], each at the strength and sharpness the event has in its middle. A
/// burst too weak to be felt is dropped, and bursts of one moment ([`SAME_MOMENT_MS`]) are the
/// strongest of them: an event that begins as a tap and goes on held (`thud`) plays the tap, not
/// two things at once.
///
/// What is kept is bounded by the length of the pattern, not by what it asks for: a pattern may have
/// 256 events that each last 60 seconds, a million and a half pieces, which are folded into one
/// another as they are made.
pub(super) fn plan(pattern: &Pattern) -> Vec<Step> {
    plan_folding(pattern, FOLD_AT)
}

/// How many bursts are made before they are folded into one another the first time.
const FOLD_AT: usize = 4096;

fn plan_folding(pattern: &Pattern, fold_at: usize) -> Vec<Step> {
    let mut steps: Vec<Step> = Vec::new();
    let mut fold_at = fold_at.max(2);
    for event in pattern.events() {
        if event.seconds <= 0.0 {
            let hz = frequency(event.sharpness);
            steps.push(Step {
                at: at(event.at),
                hz,
                level: level(event.intensity),
                ms: tap_ms(hz),
                tap: true,
            });
            continue;
        }
        let total_ms = event.seconds * 1000.0;
        // The small allowance keeps 400 ms from becoming 41 pieces of float noise.
        let pieces = (total_ms / PIECE_MS - 0.001).ceil().max(1.0);
        let mut index = 0.0_f32;
        while index < pieces {
            let began_ms = index * PIECE_MS;
            let by = (index + 0.5) / pieces;
            let strength = level(lerp(event.intensity, event.intensity_end, by));
            // A piece too weak to be felt is not kept at all.
            if strength >= LEAST_LEVEL {
                steps.push(Step {
                    at: at(event.at + began_ms / 1000.0),
                    hz: frequency(lerp(event.sharpness, event.sharpness_end, by)),
                    level: strength,
                    ms: PIECE_MS.min(total_ms - began_ms),
                    tap: false,
                });
            }
            index += 1.0;
            if steps.len() >= fold_at {
                steps = merged(steps);
                // What is left of a fold may be most of what is asked for: the next one is when
                // that has doubled, so there are few folds.
                fold_at = fold_at.max(steps.len() * 2);
            }
        }
    }
    merged(steps)
}

/// `steps` in the order of their times, the ones of one moment ([`SAME_MOMENT_MS`], counted from the
/// first of them, so that a run of bursts each a little stronger than the last is not one moment
/// that slides) the strongest of them.
fn merged(mut steps: Vec<Step>) -> Vec<Step> {
    steps.retain(|step| step.level >= LEAST_LEVEL);
    steps.sort_by_key(|step| step.at);
    let mut kept: Vec<Step> = Vec::with_capacity(steps.len());
    let mut moment_from = Duration::ZERO;
    for step in steps {
        match kept.last_mut() {
            Some(last)
                if step.at.saturating_sub(moment_from).as_secs_f32() * 1000.0 < SAME_MOMENT_MS =>
            {
                if step.level > last.level {
                    *last = step;
                }
            }
            _ => {
                moment_from = step.at;
                kept.push(step);
            }
        }
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_shell_common::native_view::haptic::{Event, NAMES, preset};

    fn ms(step: &Step) -> f32 {
        step.at.as_secs_f32() * 1000.0
    }

    #[test]
    fn a_sharpness_crosses_the_band_by_ratio_from_the_dull_end_to_the_crisp() {
        assert!((frequency(0.0) - DULL_HZ).abs() < 1e-3);
        assert!((frequency(1.0) - CRISP_HZ).abs() < 1e-2);
        let middle = frequency(0.5);
        assert!((middle - (DULL_HZ * CRISP_HZ).sqrt()).abs() < 1e-2);
        assert!(frequency(0.2) < frequency(0.8));
    }

    #[test]
    fn a_number_past_the_ends_or_none_is_the_nearest_end() {
        assert_eq!(frequency(-3.0), frequency(0.0));
        assert_eq!(frequency(7.0), frequency(1.0));
        assert_eq!(frequency(f32::NAN), frequency(0.0));
        assert_eq!(level(f32::NAN), 0.0);
    }

    #[test]
    fn a_strength_is_held_to_what_the_device_gives_at_most() {
        assert_eq!(level(0.5), 0.5 * MOST_LEVEL);
        assert_eq!(level(3.0), MOST_LEVEL);
        assert_eq!(level(-1.0), 0.0);
        let loud = Pattern::new([Event::tap(0.0, 1.0, 0.5)]).expect("a pattern");
        assert!(plan(&loud).iter().all(|step| step.level <= MOST_LEVEL));
    }

    #[test]
    fn a_tap_is_one_short_burst_at_its_time_and_a_dull_one_lasts_longer_than_a_crisp_one() {
        let dull = plan(&Pattern::new([Event::tap(0.25, 0.8, 0.0)]).expect("a pattern"));
        let crisp = plan(&Pattern::new([Event::tap(0.25, 0.8, 1.0)]).expect("a pattern"));
        assert_eq!(dull.len(), 1);
        assert_eq!(dull[0].at, Duration::from_millis(250));
        assert!(dull[0].tap);
        assert!(dull[0].ms > crisp[0].ms);
        for step in dull.iter().chain(&crisp) {
            assert!((SHORTEST_TAP_MS..=LONGEST_TAP_MS).contains(&step.ms));
        }
    }

    #[test]
    fn a_held_event_is_pieces_one_after_another_whose_strength_goes_from_one_end_to_the_other() {
        let rise = plan(
            &Pattern::new([Event::held(0.1, 0.4, (0.1, 1.0), (0.2, 0.8))]).expect("a pattern"),
        );
        assert_eq!(rise.len(), 40);
        assert!(rise.iter().all(|step| !step.tap));
        for (index, step) in rise.iter().enumerate() {
            assert!((ms(step) - (100.0 + index as f32 * PIECE_MS)).abs() < 0.01);
            assert!((step.ms - PIECE_MS).abs() < 1e-3);
        }
        assert!(rise.windows(2).all(|pair| pair[1].level > pair[0].level));
        assert!(rise.windows(2).all(|pair| pair[1].hz > pair[0].hz));
        assert!(rise[0].level < 0.15 && rise[39].level > 0.95);
    }

    #[test]
    fn the_last_piece_of_a_held_event_ends_where_the_event_does() {
        let held = plan(
            &Pattern::new([Event::held(0.0, 0.025, (0.5, 0.5), (0.5, 0.5))]).expect("a pattern"),
        );
        assert_eq!(held.len(), 3);
        let last = held[2];
        assert!((ms(&last) + last.ms - 25.0).abs() < 0.01);
    }

    #[test]
    fn a_burst_too_weak_to_be_felt_is_dropped() {
        let fade = plan(
            &Pattern::new([Event::held(0.0, 0.4, (1.0, 0.0), (0.5, 0.5))]).expect("a pattern"),
        );
        assert!(fade.len() < 40);
        assert!(fade.iter().all(|step| step.level >= LEAST_LEVEL));
        assert!(plan(&Pattern::new([Event::tap(0.0, 0.0, 0.5)]).expect("a pattern")).is_empty());
    }

    #[test]
    fn a_tap_and_a_held_event_at_one_moment_are_the_stronger_not_both() {
        let thud = plan(&preset("thud").expect("a ready pattern"));
        assert!(thud[0].tap, "the knock is what plays at the start");
        // The ready `thud` is a firm tap, 0.3 of the motor, then a held 0.21 that dies away.
        assert!((thud[0].level - 0.3).abs() < 1e-6, "{thud:?}");
        assert!(ms(&thud[1]) >= SAME_MOMENT_MS, "{thud:?}");
        assert!(thud.iter().skip(1).all(|step| !step.tap));
        assert!(
            thud.windows(2)
                .all(|pair| ms(&pair[1]) - ms(&pair[0]) >= SAME_MOMENT_MS - 0.01)
        );
    }

    #[test]
    fn the_stronger_of_one_moment_plays_whichever_of_the_two_it_is() {
        let pattern = Pattern::new([
            Event::tap(0.0, 0.3, 0.5),
            Event::held(0.0, 0.05, (0.9, 0.9), (0.5, 0.5)),
        ])
        .expect("a pattern");
        let steps = plan(&pattern);
        assert!(!steps[0].tap, "the held piece is the stronger at the start");
        assert!((steps[0].level - 0.9).abs() < 1e-5);
        assert_eq!(steps.len(), 5);
    }

    #[test]
    fn a_moment_is_counted_from_its_first_burst_and_does_not_slide_with_the_stronger() {
        let tap = |at_ms: f32, level: f32| Step {
            at: at(at_ms / 1000.0),
            hz: 100.0,
            level,
            ms: 6.0,
            tap: true,
        };
        // 0, 3 and 6 ms: each a little stronger and each less than a moment from the last, but the
        // third is a moment and a half from the first.
        let kept = merged(vec![tap(0.0, 0.2), tap(3.0, 0.3), tap(6.0, 0.4)]);
        assert_eq!(kept.len(), 2, "{kept:?}");
        assert!((kept[0].level - 0.3).abs() < 1e-6);
        assert!((kept[1].level - 0.4).abs() < 1e-6);
    }

    #[test]
    fn the_order_is_by_time_whatever_the_order_events_were_given_in() {
        let late_first = Pattern::new([Event::tap(0.3, 0.5, 0.5), Event::tap(0.1, 0.5, 0.5)])
            .expect("a pattern");
        let steps = plan(&late_first);
        assert_eq!(steps.len(), 2);
        assert!(steps[0].at < steps[1].at);
    }

    #[test]
    fn every_ready_pattern_plays_something_in_order_and_ends_where_it_says() {
        for name in NAMES {
            let pattern = preset(name).expect("a ready pattern");
            let steps = plan(&pattern);
            assert!(!steps.is_empty(), "{name} plays nothing");
            assert!(
                steps.windows(2).all(|pair| pair[0].at <= pair[1].at),
                "{name}"
            );
            let ends = steps
                .iter()
                .map(|step| step.at.as_secs_f32() + step.ms / 1000.0)
                .fold(0.0, f32::max);
            assert!(
                ends <= pattern.seconds() + LONGEST_TAP_MS / 1000.0 + 0.001,
                "{name} runs past its end"
            );
            assert!(
                steps.iter().all(|step| step.level <= MOST_LEVEL
                    && step.hz >= DULL_HZ - 0.01
                    && step.hz <= CRISP_HZ + 0.01),
                "{name}"
            );
        }
    }

    #[test]
    fn a_pattern_of_the_most_events_that_each_last_the_most_seconds_is_bounded_by_its_length() {
        // Two hundred and fifty-six events of a minute each, begun a hair apart: a million and a half
        // pieces if they were all kept.
        let events: Vec<_> = (0..256)
            .map(|index| {
                Event::held(
                    index as f32 * 0.0001,
                    59.9,
                    (0.2 + index as f32 / 400.0, 0.9),
                    (0.5, 0.5),
                )
            })
            .collect();
        let steps = plan(&Pattern::new(events).expect("a pattern"));
        assert!(
            steps.len() <= (60_000.0 / SAME_MOMENT_MS) as usize + 1,
            "{} bursts",
            steps.len()
        );
        assert!(steps.windows(2).all(|pair| pair[0].at <= pair[1].at));
        assert!(
            steps
                .windows(2)
                .all(|pair| (pair[1].at - pair[0].at).as_secs_f32() * 1000.0 >= SAME_MOMENT_MS)
        );
    }

    #[test]
    fn folding_early_changes_nothing_a_person_could_tell_for_the_ready_patterns() {
        for name in NAMES {
            let pattern = preset(name).expect("a ready pattern");
            assert_eq!(
                plan_folding(&pattern, 2),
                plan(&pattern),
                "{name} is different when it is folded as it is made"
            );
        }
    }

    #[test]
    fn an_empty_pattern_plays_nothing() {
        assert!(plan(&Pattern::default()).is_empty());
    }
}
