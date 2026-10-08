//! The dock's arrival — **pure**, no ObjC, no locks.
//!
//! A new pane's dock is empty until the shell speaks. This module owns the
//! scene that covers the gap: nothing while the shell starts, a short entrance
//! once it gives its first prompt. It knows time only; *what* the dock draws
//! stays `bt-core`'s, *how* a scene lands on the frame is [`crate::frame::Frame`]'s.
//!
//! **`Copy`, and a function of three stamps.** The state is the kind, the birth
//! stamp and the arrival stamp; every frame the scene is recomputed from them
//! ([`Arrival::scene`]) and no list of in-flight pieces is kept. That is why it
//! lives inside [`crate::motion::Motion`] (a `Cell` of a `Copy` type) rather
//! than beside it like the typing effects: the sleep decision, the finishing
//! paths (occlusion, a style change, a draw error) and the Reduce Motion
//! reduction already run through `Motion`, and a term outside it would have to
//! repeat each of them.
//!
//! **Three phases, two clocks.**
//!
//! - *Waiting* (birth → first prompt): the scene is held back. It is
//!   **settled** — the link sleeps — and its two deadlines ([`SHOW`], [`CAP`])
//!   are armed on the motion clock ([`Arrival::next_deadline`]) so the sleeping
//!   link is woken for them. Without the clock the cap would never fire: a link
//!   asleep on a settled animation is not ticked again.
//! - *Playing* (prompt, or [`CAP`] without one → the calendar's end): unsettled,
//!   so every vsync draws. Its stop condition is the calendar's own end, which
//!   [`ARRIVAL_MAX`] bounds for every input ([`Arrival::advance`]).
//! - *Ended*: absorbing. Anything that finishes the scene — a key, losing the
//!   screen or focus, the dock going away, a setting changing — lands here, and
//!   nothing re-opens it: a prompt that comes after the cap plays no scene.
//!
//! **The clock is absolute.** Waiting is spent asleep, so the scene cannot
//! count frames or `dt` (the link clamps `dt` to a tenth of a second); every
//! stamp is the tick's time and the scene is a function of `now − arrival`.

use bt_core::{DockArrival, Keypress};

use crate::glyph_fx::Effect;
use crate::motion::ease_axis;

/// How long a new pane shows nothing before a waiting scene may start
/// moving, seconds — **a chosen number, not a measured one**: a prompt that
/// comes sooner is not worth a waiting scene, and the quiet hold keeps the
/// scene from flickering in and out for a shell that is nearly there.
pub(crate) const SHOW: f64 = 0.18;

/// When the scene arrives without a prompt, seconds after birth — **chosen**.
/// A shell that has said nothing by now is slow, hung or not ours, and the
/// dock must not stay empty for as long as it takes.
pub(crate) const CAP: f64 = 3.0;

/// The longest an arrival may play, seconds. Every calendar ends inside it,
/// whatever the text it types ([`LETTER_CAP`] is what guarantees that).
pub(crate) const ARRIVAL_MAX: f64 = 1.0;

/// How far below its place the dock starts and climbs from, points.
pub(crate) const RISE_PT: f32 = 10.0;

/// Letters past this column come in together with the one at it. Without it a
/// long path would stretch the arrival past [`ARRIVAL_MAX`].
const LETTER_CAP: u16 = 48;

/// How the › and the letters come in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Entrance {
    /// Grows from a little under full size with a small overshoot.
    Pop,
    /// Fades in without moving: Reduce Motion's.
    Fade,
}

impl Entrance {
    /// The typing effect that draws it ([`crate::glyph_fx`]).
    pub(crate) fn fx_id(self) -> u32 {
        match self {
            Self::Pop => Keypress::Pop,
            Self::Fade => Keypress::Fade,
        }
        .id()
        .unwrap_or(0)
    }
}

/// One kind's timetable, seconds from the arrival.
struct Calendar {
    entrance: Entrance,
    /// Whether the dock climbs from [`RISE_PT`] below its place.
    rise: bool,
    /// The dock's fade-in (ground and lines).
    band: f32,
    /// `(start, duration)` of the two lines' drawing from the left; `None` →
    /// both are there from the start.
    lines: Option<[(f32, f32); 2]>,
    /// `(start, duration)` of the ›'s entrance.
    mark: (f32, f32),
    /// The first context letter's start, the step to the next column and one
    /// letter's entrance.
    letters: (f32, f32, f32),
    /// The cursor's fade-in: its start after the last letter's, and its length.
    caret: (f32, f32),
}

/// What the dock does at the first prompt: it climbs a little as it fades in,
/// its two lines are drawn from the left, the › pops in, the path and the
/// branch type themselves out column by column and the cursor comes last.
const TYPED: Calendar = Calendar {
    entrance: Entrance::Pop,
    rise: true,
    band: 0.20,
    lines: Some([(0.04, 0.24), (0.09, 0.24)]),
    mark: (0.10, 0.24),
    letters: (0.16, 0.011, 0.17),
    caret: (0.06, 0.06),
};

/// Reduce Motion: everything fades in together and nothing moves.
const REDUCED: Calendar = Calendar {
    entrance: Entrance::Fade,
    rise: false,
    band: 0.12,
    lines: None,
    mark: (0.0, 0.12),
    letters: (0.0, 0.0, 0.12),
    caret: (0.0, 0.09),
};

/// How long the climb takes.
const RISE_SECS: f32 = 0.22;

/// The arrival scene of one pane's dock.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Arrival {
    /// The user's choice. Only drawable kinds are armed.
    kind: DockArrival,
    /// Whether Reduce Motion was on when the scene was armed. Turning it on or
    /// off later ends the scene ([`crate::motion::Motion::set_reduce`]), so one
    /// value holds for the scene's whole life.
    reduce: bool,
    /// The pane's birth stamp: the base of both waiting deadlines.
    born: f64,
    /// The stamp of the arrival; `None` while waiting.
    arrived: Option<f64>,
    /// The arrival was the cap's, not a prompt's: the path and branch did not
    /// come with it, so they are not typed out when they do.
    forced: bool,
    ended: bool,
    /// The last stamp the scene was stepped to.
    at: f64,
    /// Columns of context text seen so far (the high-water mark).
    letters: u16,
    /// Where the cursor's fade-in started, once it has: it must not move later
    /// if more text arrives.
    caret_at: Option<f32>,
}

impl Default for Arrival {
    /// A scene that is not there: ended from the start.
    fn default() -> Self {
        Self::new(DockArrival::Off, 0.0, false)
    }
}

/// What the frame draws for the dock at one moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Scene {
    /// The share of [`RISE_PT`] the dock still has to climb, `1 → 0`.
    pub(crate) rise: f32,
    /// The ground's and the lines' opacity.
    pub(crate) band: f32,
    /// How much of its width each line has drawn.
    pub(crate) lines: [f32; 2],
    /// The › entrance's progress, `0 →1`; the curve is the shader's.
    pub(crate) mark: f32,
    /// The cursor's opacity.
    pub(crate) caret: f32,
    pub(crate) entrance: Entrance,
    letters: Letters,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Letters {
    /// Not there yet: the shell has not spoken.
    Hidden,
    /// There from the start.
    Settled,
    /// Typed out: seconds since the arrival, the first letter's start, the step
    /// per column and one letter's entrance.
    Typing {
        since: f32,
        at: f32,
        step: f32,
        span: f32,
    },
}

impl Scene {
    /// Everything held back: what the dock shows while the shell starts.
    fn waiting(entrance: Entrance) -> Self {
        Self {
            rise: 1.0,
            band: 0.0,
            lines: [0.0; 2],
            mark: 0.0,
            caret: 0.0,
            entrance,
            letters: Letters::Hidden,
        }
    }

    /// The entrance progress of the context row's letter at `col`, `0` (not
    /// there) to `1` (settled).
    pub(crate) fn letter(&self, col: u16) -> f32 {
        match self.letters {
            Letters::Hidden => 0.0,
            Letters::Settled => 1.0,
            Letters::Typing {
                since,
                at,
                step,
                span,
            } => unit((since - at - f32::from(col.min(LETTER_CAP)) * step) / span),
        }
    }
}

fn unit(x: f32) -> f32 {
    x.clamp(0.0, 1.0)
}

/// The motion module's cubic ease-out over `0..1` (one copy of the curve).
fn ease(x: f32) -> f32 {
    ease_axis(0.0, 1.0, unit(x))
}

impl Arrival {
    /// A scene armed at `born`. `Off` is inert.
    pub(crate) fn new(kind: DockArrival, born: f64, reduce: bool) -> Self {
        Self {
            kind,
            reduce,
            born,
            arrived: None,
            forced: false,
            ended: kind == DockArrival::Off,
            at: born,
            letters: 0,
            caret_at: None,
        }
    }

    fn calendar(&self) -> &'static Calendar {
        if self.reduce {
            return &REDUCED;
        }
        match self.kind {
            // `ripple` and `dust` play this calendar until they have their own.
            DockArrival::Off | DockArrival::Type | DockArrival::Dust | DockArrival::Ripple => {
                &TYPED
            }
        }
    }

    /// Whether the scene is still to come or under way (not ended).
    pub(crate) fn is_armed(&self) -> bool {
        !self.ended
    }

    /// The sleep question. Waiting is settled — nothing moves — and so is an
    /// ended scene; only a playing one keeps the link awake.
    pub(crate) fn settled(&self) -> bool {
        self.ended || self.arrived.is_none()
    }

    /// Steps the scene to `now` and says whether the last drawn frame may
    /// differ from what it shows now: the scene was playing when the step
    /// began, or it ended in it. **The question is about the state before the
    /// step** (the typing effects' rule): the step that ends the scene is
    /// still drawn, or the last frame on screen would be a half-entered dock.
    ///
    /// `active` is whether the pane is on screen in the key window, asked
    /// where the cap would start the scene: a pane nobody is watching is over
    /// without playing, as at the prompt ([`Arrival::arrive`]).
    pub(crate) fn advance(&mut self, now: f64, active: bool) -> bool {
        if self.ended {
            return false;
        }
        self.at = now;
        let playing = self.arrived.is_some();
        if !playing && now >= self.born + CAP {
            if active {
                self.arrived = Some(self.born + CAP);
                self.forced = true;
            } else {
                self.ended = true;
            }
        }
        if let Some(arrived) = self.arrived {
            let since = ((now - arrived) as f32).max(0.0);
            if self.caret_at.is_none() {
                let start = self.caret_start();
                if since >= start {
                    self.caret_at = Some(start);
                }
            }
            // The calendar's own end, and [`ARRIVAL_MAX`] as a belt over it: no
            // input may keep the link awake past the bound.
            if since >= self.end_secs().min(ARRIVAL_MAX as f32) {
                self.ended = true;
            }
        }
        playing || self.ended
    }

    /// The shell gave its first prompt at `now`. A scene that has not
    /// started starts; one that has, or ended, ignores it. `active` is whether
    /// the pane is on screen in the key window: if not, the scene is over
    /// without playing.
    pub(crate) fn arrive(&mut self, now: f64, active: bool) {
        if self.ended || self.arrived.is_some() {
            return;
        }
        if !active {
            self.ended = true;
            return;
        }
        self.at = now;
        self.arrived = Some(now.max(self.born));
    }

    /// Ends the scene at its last state. Says whether it was still armed, i.e.
    /// whether the dock on screen may not be the final one.
    pub(crate) fn end(&mut self) -> bool {
        !std::mem::replace(&mut self.ended, true)
    }

    /// Notes how many columns of context text the dock holds. Only ever grows.
    pub(crate) fn note_letters(&mut self, count: u16) {
        self.letters = self.letters.max(count);
    }

    /// The nearest time the sleeping link must be woken for, if any: the end
    /// of the hold, then the cap.
    pub(crate) fn next_deadline(&self, now: f64) -> Option<f64> {
        if self.ended || self.arrived.is_some() {
            return None;
        }
        let show = self.born + SHOW;
        Some(if now < show { show } else { self.born + CAP })
    }

    /// The column the last typed letter sits at, for the cursor's start.
    fn typed(&self) -> u16 {
        if self.forced {
            0
        } else {
            self.letters.min(LETTER_CAP)
        }
    }

    /// When the cursor starts to fade in, seconds from the arrival.
    fn caret_start(&self) -> f32 {
        let calendar = self.calendar();
        let (at, step, _) = calendar.letters;
        self.caret_at
            .unwrap_or(at + f32::from(self.typed()) * step + calendar.caret.0)
    }

    /// Seconds from the arrival at which the scene is over: the last piece of
    /// the calendar to finish. The text it types is the only input that
    /// varies, and [`LETTER_CAP`] bounds that.
    fn end_secs(&self) -> f32 {
        let calendar = self.calendar();
        let (at, step, span) = calendar.letters;
        let mut end = calendar.band.max(calendar.mark.0 + calendar.mark.1);
        if calendar.rise {
            end = end.max(RISE_SECS);
        }
        for (start, length) in calendar.lines.into_iter().flatten() {
            end = end.max(start + length);
        }
        if !self.forced {
            end = end.max(at + f32::from(self.typed()) * step + span);
        }
        end.max(self.caret_start() + calendar.caret.1)
    }

    /// The scene to draw, or `None` once it is over.
    pub(crate) fn scene(&self) -> Option<Scene> {
        if self.ended {
            return None;
        }
        let calendar = self.calendar();
        let Some(arrived) = self.arrived else {
            return Some(Scene::waiting(calendar.entrance));
        };
        let since = ((self.at - arrived) as f32).max(0.0);
        let (at, step, span) = calendar.letters;
        Some(Scene {
            rise: if calendar.rise {
                1.0 - ease(since / RISE_SECS)
            } else {
                0.0
            },
            band: unit(since / calendar.band),
            lines: match calendar.lines {
                Some(lines) => lines.map(|(start, length)| ease((since - start) / length)),
                None => [1.0; 2],
            },
            mark: unit((since - calendar.mark.0) / calendar.mark.1),
            caret: unit((since - self.caret_start()) / calendar.caret.1),
            entrance: calendar.entrance,
            letters: if self.forced {
                Letters::Settled
            } else {
                Letters::Typing {
                    since,
                    at,
                    step,
                    span,
                }
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn later(a: f32, b: f32) -> f32 {
        if a > b { a } else { b }
    }

    /// The longest a calendar can run, whatever text it types: every piece at its
    /// latest, the letters at [`LETTER_CAP`].
    const fn longest(calendar: &Calendar) -> f32 {
        let (at, step, span) = calendar.letters;
        let last_letter = at + LETTER_CAP as f32 * step;
        let mut end = later(calendar.band, calendar.mark.0 + calendar.mark.1);
        end = later(
            end,
            later(
                last_letter + span,
                last_letter + calendar.caret.0 + calendar.caret.1,
            ),
        );
        if calendar.rise {
            end = later(end, RISE_SECS);
        }
        if let Some(lines) = calendar.lines {
            end = later(end, lines[0].0 + lines[0].1);
            end = later(end, lines[1].0 + lines[1].1);
        }
        end
    }

    const BORN: f64 = 100.0;
    /// The prompt's time after birth in most tests.
    const PROMPT: f64 = 1.2;

    /// A playing scene whose prompt came `PROMPT` after birth, with `columns`
    /// of context text noted.
    fn playing(columns: u16) -> Arrival {
        let mut arrival = Arrival::new(DockArrival::Type, BORN, false);
        arrival.advance(BORN + PROMPT, true);
        arrival.arrive(BORN + PROMPT, true);
        arrival.note_letters(columns);
        arrival
    }

    /// Steps `arrival` to `since` seconds after the prompt.
    fn at(arrival: &mut Arrival, since: f64) -> bool {
        arrival.advance(BORN + PROMPT + since, true)
    }

    /// Drives a scene that arrived at `start` (an absolute stamp) at 120 Hz
    /// until it ends, returns the seconds it took.
    fn run_from(arrival: &mut Arrival, start: f64) -> f64 {
        let mut since = 0.0;
        while arrival.is_armed() {
            since += 1.0 / 120.0;
            arrival.advance(start + since, true);
            assert!(since < 5.0, "the scene never ended");
        }
        since
    }

    /// [`run_from`] a scene whose prompt came at `PROMPT`.
    fn run(arrival: &mut Arrival) -> f64 {
        run_from(arrival, BORN + PROMPT)
    }

    #[test]
    fn a_waiting_scene_holds_everything_back_and_is_settled() {
        let mut arrival = Arrival::new(DockArrival::Type, BORN, false);
        assert!(arrival.settled(), "waiting is spent asleep");
        arrival.advance(BORN + 0.5, true);
        let scene = arrival.scene().expect("armed");
        assert_eq!(scene.band, 0.0);
        assert_eq!(scene.lines, [0.0; 2]);
        assert_eq!(scene.mark, 0.0);
        assert_eq!(scene.caret, 0.0);
        assert_eq!(scene.letter(0), 0.0);
        assert!(arrival.settled(), "stepping a waiting scene starts nothing");
    }

    #[test]
    fn the_clock_is_asked_for_the_hold_then_the_cap() {
        let arrival = Arrival::new(DockArrival::Type, BORN, false);
        assert_eq!(arrival.next_deadline(BORN), Some(BORN + SHOW));
        assert_eq!(arrival.next_deadline(BORN + 0.1), Some(BORN + SHOW));
        assert_eq!(arrival.next_deadline(BORN + SHOW), Some(BORN + CAP));
        assert_eq!(arrival.next_deadline(BORN + 2.0), Some(BORN + CAP));
        // Playing or over: awake or nothing to wait for.
        assert_eq!(playing(0).next_deadline(BORN + 2.0), None);
        let mut ended = Arrival::new(DockArrival::Type, BORN, false);
        ended.end();
        assert_eq!(ended.next_deadline(BORN), None);
    }

    #[test]
    fn the_cap_arrives_without_a_prompt() {
        let mut arrival = Arrival::new(DockArrival::Type, BORN, false);
        assert!(!arrival.advance(BORN + CAP - 0.001, true), "still waiting");
        assert!(arrival.settled());
        arrival.advance(BORN + CAP, true);
        assert!(!arrival.settled(), "the cap starts the scene");
        let scene = arrival.scene().expect("armed");
        assert_eq!(scene.letter(0), 1.0, "text that comes later is not typed");
        assert!(run_from(&mut arrival, BORN + CAP) <= ARRIVAL_MAX);
        assert!(arrival.settled());
    }

    #[test]
    fn a_prompt_after_the_cap_opens_no_scene() {
        let mut arrival = Arrival::new(DockArrival::Type, BORN, false);
        arrival.advance(BORN + CAP, true);
        run_from(&mut arrival, BORN + CAP);
        assert!(arrival.scene().is_none());
        arrival.arrive(BORN + CAP + 2.0, true);
        assert!(arrival.scene().is_none(), "an ended scene stays ended");
        assert!(arrival.settled());
    }

    #[test]
    fn a_wake_long_after_the_cap_arrives_and_ends_in_one_step() {
        // The Mac slept through the cap: the first tick is far past it. The
        // step must say it changed the frame, or the link would sleep with the
        // waiting scene (an empty dock) still on screen.
        let mut arrival = Arrival::new(DockArrival::Type, BORN, false);
        assert!(arrival.advance(BORN + CAP + 60.0, true));
        assert!(arrival.scene().is_none());
    }

    #[test]
    fn the_cap_does_not_start_a_scene_nobody_watches() {
        // A window that is not the key one, with a shell that says nothing for
        // three seconds: the dock comes back as it is, the entrance is not
        // played to an empty room.
        let mut arrival = Arrival::new(DockArrival::Type, BORN, false);
        assert!(!arrival.advance(BORN + 1.0, false), "waiting is waiting");
        assert!(arrival.is_armed());
        assert!(
            arrival.advance(BORN + CAP, false),
            "the step that ends it is drawn"
        );
        assert!(arrival.scene().is_none());
        assert!(arrival.settled());
    }

    #[test]
    fn an_inactive_pane_is_over_without_playing() {
        let mut arrival = Arrival::new(DockArrival::Type, BORN, false);
        arrival.arrive(BORN + PROMPT, false);
        assert!(arrival.scene().is_none());
        assert!(arrival.settled());
    }

    #[test]
    fn off_is_inert() {
        let arrival = Arrival::new(DockArrival::Off, BORN, false);
        assert!(!arrival.is_armed());
        assert!(arrival.scene().is_none());
        assert!(arrival.settled());
        assert_eq!(arrival.next_deadline(BORN), None);
        assert!(!Arrival::default().is_armed());
    }

    #[test]
    fn ending_is_absorbing_and_says_whether_anything_was_cut() {
        let mut arrival = playing(10);
        assert!(arrival.end(), "a playing scene was cut");
        assert!(!arrival.end(), "the second call cuts nothing");
        assert!(arrival.scene().is_none());
        assert!(arrival.settled());
        assert!(!at(&mut arrival, 0.1), "an ended scene steps to nothing");
        arrival.arrive(BORN + 5.0, true);
        assert!(arrival.scene().is_none());
        // Waiting is cut too.
        let mut waiting = Arrival::new(DockArrival::Type, BORN, false);
        assert!(waiting.end());
    }

    #[test]
    fn a_playing_scene_is_unsettled_and_ends_inside_the_maximum() {
        for columns in [0, 1, 12, 48, 49, 300, u16::MAX] {
            let mut arrival = playing(columns);
            assert!(
                !arrival.settled(),
                "{columns}: playing keeps the link awake"
            );
            let took = run(&mut arrival);
            assert!(took <= ARRIVAL_MAX + 1.0 / 120.0, "{columns}: {took}s");
            assert!(arrival.settled());
        }
        let mut reduced = Arrival::new(DockArrival::Type, BORN, true);
        reduced.advance(BORN + PROMPT, true);
        reduced.arrive(BORN + PROMPT, true);
        reduced.note_letters(u16::MAX);
        assert!(run(&mut reduced) <= ARRIVAL_MAX);
    }

    #[test]
    fn the_scene_never_runs_past_its_calendars_worst_case() {
        for columns in [0, 1, 20, LETTER_CAP, u16::MAX] {
            let arrival = playing(columns);
            assert!(arrival.end_secs() <= longest(&TYPED), "{columns}");
        }
        // The bound is the calendars', not the belt's: the belt never fires.
        assert!(longest(&TYPED) < ARRIVAL_MAX as f32);
        assert!(longest(&REDUCED) < ARRIVAL_MAX as f32);
    }

    #[test]
    fn the_step_that_ends_the_scene_is_reported() {
        let mut arrival = playing(20);
        let mut since = 0.0;
        let mut reports = Vec::new();
        while arrival.is_armed() {
            since += 1.0 / 120.0;
            reports.push(at(&mut arrival, since));
        }
        assert!(reports.iter().all(|&drawn| drawn), "{reports:?}");
        assert!(!at(&mut arrival, since + 0.1));
    }

    #[test]
    fn the_type_calendar_draws_the_dock_before_the_letters_and_the_cursor_last() {
        let mut arrival = playing(20);
        at(&mut arrival, 0.0);
        let first = arrival.scene().expect("armed");
        assert_eq!(
            (first.rise, first.band, first.mark, first.caret),
            (1.0, 0.0, 0.0, 0.0)
        );
        assert_eq!(first.lines, [0.0; 2]);
        // The lines start before the ›, the ›  before the letters.
        at(&mut arrival, 0.06);
        let early = arrival.scene().expect("armed");
        assert!(early.lines[0] > 0.0 && early.lines[1] == 0.0, "{early:?}");
        assert_eq!(early.mark, 0.0);
        at(&mut arrival, 0.13);
        let middle = arrival.scene().expect("armed");
        assert!(middle.lines[1] > 0.0 && middle.mark > 0.0, "{middle:?}");
        assert_eq!(middle.letter(0), 0.0, "letters wait for the dock");
        at(&mut arrival, 0.25);
        let later = arrival.scene().expect("armed");
        assert_eq!(later.rise, 0.0, "the climb is over by 220 ms");
        assert!(later.letter(0) > 0.0, "{later:?}");
        assert!(later.letter(0) > later.letter(5), "left to right");
        assert!(later.letter(5) >= later.letter(19));
        assert_eq!(later.caret, 0.0, "the cursor comes after the last letter");
    }

    #[test]
    fn the_cursor_waits_for_the_last_letter_and_the_dock_has_stopped_climbing() {
        for columns in [0, 3, 20, 60] {
            let mut arrival = playing(columns);
            let mut since = 0.0;
            let mut last_letter_done = 0.0f64;
            while arrival.is_armed() {
                since += 0.002;
                at(&mut arrival, since);
                let Some(scene) = arrival.scene() else { break };
                if scene.rise > 0.0 {
                    assert_eq!(
                        scene.caret, 0.0,
                        "{columns}: the cursor over a climbing dock"
                    );
                }
                if scene.letter(columns.saturating_sub(1)) < 1.0 {
                    last_letter_done = since;
                }
                if scene.caret > 0.0 && columns > 0 {
                    let last = scene.letter(columns - 1);
                    assert!(
                        last > 0.0,
                        "{columns}: the cursor at {since} before the last letter began ({last_letter_done})"
                    );
                }
            }
        }
    }

    #[test]
    fn the_cursor_never_fades_back_when_more_text_arrives() {
        let mut arrival = playing(0);
        let mut alphas = Vec::new();
        let mut since = 0.0;
        while arrival.is_armed() {
            since += 0.004;
            at(&mut arrival, since);
            // The branch lands late, after the cursor started.
            if since > 0.3 {
                arrival.note_letters(30);
            }
            if let Some(scene) = arrival.scene() {
                alphas.push(scene.caret);
            }
        }
        assert!(
            alphas.windows(2).all(|pair| pair[1] >= pair[0]),
            "{alphas:?}"
        );
        assert_eq!(alphas.last(), Some(&1.0));
    }

    #[test]
    fn text_that_comes_in_the_first_instant_is_typed_before_the_cursor() {
        // The path arrives a frame after the prompt: the count grows while the
        // cursor has not started, and it moves the cursor's start with it.
        let mut with = playing(0);
        let mut late = playing(0);
        late.note_letters(30);
        let (mut a, mut b) = (0.0, 0.0);
        while with.is_armed() {
            a += 0.002;
            at(&mut with, a);
        }
        while late.is_armed() {
            b += 0.002;
            at(&mut late, b);
        }
        assert!(b > a + 0.2, "a long path takes longer to type: {a} vs {b}");
    }

    #[test]
    fn columns_past_the_cap_come_in_together() {
        let mut arrival = playing(200);
        at(&mut arrival, 0.5);
        let scene = arrival.scene().expect("armed");
        assert_eq!(scene.letter(LETTER_CAP), scene.letter(LETTER_CAP + 100));
        assert_eq!(
            arrival.scene(),
            arrival.scene(),
            "a pure function of its state"
        );
    }

    #[test]
    fn reduce_motion_fades_everything_in_together_and_moves_nothing() {
        let mut arrival = Arrival::new(DockArrival::Type, BORN, true);
        arrival.advance(BORN + PROMPT, true);
        arrival.arrive(BORN + PROMPT, true);
        arrival.note_letters(30);
        at(&mut arrival, 0.06);
        let scene = arrival.scene().expect("armed");
        assert_eq!(scene.entrance, Entrance::Fade);
        assert_eq!(scene.rise, 0.0, "no climb");
        assert_eq!(scene.lines, [1.0; 2], "no drawing from the left");
        assert!(scene.band > 0.0 && scene.band < 1.0);
        assert_eq!(scene.letter(0), scene.letter(29), "all at once");
        assert!(scene.mark > 0.0 && scene.mark < 1.0);
    }

    #[test]
    fn the_entrance_is_a_drawn_typing_effect() {
        for entrance in [Entrance::Pop, Entrance::Fade] {
            assert_ne!(
                entrance.fx_id(),
                0,
                "{entrance:?} must resolve to a shader id"
            );
        }
        assert_ne!(Entrance::Pop.fx_id(), Entrance::Fade.fx_id());
    }

    #[test]
    fn the_scene_is_deterministic() {
        let mut one = playing(25);
        let mut two = playing(25);
        for since in [0.0, 0.03, 0.11, 0.2, 0.37, 0.5] {
            at(&mut one, since);
            at(&mut two, since);
            assert_eq!(one.scene(), two.scene(), "{since}");
        }
    }
}
