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
//!   asleep on a settled animation is not ticked again. **One calendar waits
//!   with motion**: the ripple's top line is a wave that stirs from [`SHOW`]
//!   until the prompt or the cap ([`Arrival::stirs`]). It is unsettled in
//!   that stretch — stopped by the cap, which arrives the scene — and only for
//!   a pane someone is watching, so a window in the background still sleeps.
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

// The wave. Lengths are in points (the design's unit, scaled to pixels where
// the line is drawn), times in seconds. Every number is the design's — chosen
// by eye, not measured.

/// The resting wave's peak while the shell starts, points.
const WAVE_AMP_PT: f32 = 1.8;

/// How long the resting wave takes to reach its height after [`SHOW`].
const WAVE_FADE_IN: f32 = 0.22;

/// How long the resting wave takes to calm after [`CAP`]. A live scene is
/// arrived by the cap and never waits that long: the tail is the envelope's
/// own shape, kept so the function says what the design says.
const WAVE_CALM: f32 = 0.30;

/// Seconds the resting wave's phase takes to move one radian.
const WAVE_PHASE_SECS: f32 = 0.24;

/// The resting wave dies away after the prompt as `exp(−t / WAVE_SETTLE)`.
const WAVE_SETTLE: f32 = 0.09;

/// How fast the ring's front leaves the ›, points per second.
const RING_SPEED: f32 = 1600.0;

/// The ring's peak at the prompt, points, and how it dies away
/// (`exp(−t / RING_FADE)`).
const RING_KICK_PT: f32 = 2.4;
const RING_FADE: f32 = 0.17;

/// When the line stops being a wave, seconds after the prompt: it is flat by
/// now and the dock draws it.
const WAVE_END: f32 = 0.56;

/// The line's colour moves from the quiet tone to its own between these.
const TONE_AT: f32 = 0.20;
const TONE_SPAN: f32 = 0.34;

/// How far the line may leave its row, points: both peaks at once. The
/// renderer reserves this much room above and below the dock's top.
pub(crate) const WAVE_REACH_PT: f32 = WAVE_AMP_PT + RING_KICK_PT;

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

/// How one of the dock's two lines comes in.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Line {
    /// There from the start.
    Whole,
    /// Drawn from the left: `(start, duration)`.
    Drawn(f32, f32),
    /// The wave draws it until the scene is over ([`WAVE_END`]); the dock's
    /// own line is held back meanwhile.
    Waved,
}

impl Line {
    /// Seconds from the arrival at which the line is whole.
    const fn ends_at(self) -> f32 {
        match self {
            Self::Whole => 0.0,
            Self::Drawn(start, length) => start + length,
            Self::Waved => WAVE_END,
        }
    }
}

/// One kind's timetable, seconds from the arrival.
struct Calendar {
    /// The ›'s.
    entrance: Entrance,
    /// The context row's letters'.
    letters_entrance: Entrance,
    /// Whether the dock climbs from [`RISE_PT`] below its place.
    rise: bool,
    /// The dock's fade-in (ground and lines).
    band: f32,
    /// How the top line and the one between the input and the context row
    /// come in.
    lines: [Line; 2],
    /// Whether the top line ripples: it waves while the shell starts and a
    /// ring spreads from the › when it speaks.
    wave: bool,
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
    letters_entrance: Entrance::Pop,
    rise: true,
    band: 0.20,
    lines: [Line::Drawn(0.04, 0.24), Line::Drawn(0.09, 0.24)],
    wave: false,
    mark: (0.10, 0.24),
    letters: (0.16, 0.011, 0.17),
    caret: (0.06, 0.06),
};

/// What the dock does at the first prompt when its top line has been rippling:
/// the › pops in and a ring spreads from it along the line, which settles
/// flat; the second line is drawn from the left and the context row fades in
/// all at once, the cursor right behind it.
const RIPPLED: Calendar = Calendar {
    entrance: Entrance::Pop,
    letters_entrance: Entrance::Fade,
    rise: false,
    band: 0.16,
    lines: [Line::Waved, Line::Drawn(0.12, 0.24)],
    wave: true,
    mark: (0.04, 0.24),
    letters: (0.20, 0.0, 0.22),
    caret: (0.02, 0.06),
};

/// Reduce Motion: everything fades in together and nothing moves.
const REDUCED: Calendar = Calendar {
    entrance: Entrance::Fade,
    letters_entrance: Entrance::Fade,
    rise: false,
    band: 0.12,
    lines: [Line::Whole; 2],
    wave: false,
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
    /// Whether the pane was on screen in the key window at that step: a wave
    /// stirs for a pane someone is watching.
    active: bool,
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
    /// How the › comes in.
    pub(crate) entrance: Entrance,
    /// How the context row's letters come in.
    pub(crate) letter_entrance: Entrance,
    /// The top line as a wave, while the scene gives it one; the lines'
    /// `[0]` is `0` meanwhile so the dock's own line stays out of the way.
    pub(crate) wave: Option<Wave>,
    letters: Letters,
}

/// The top line as a wave at one moment — a resting ripple that fades in
/// while the shell starts, and the ring that spreads from the › once it has
/// spoken. Lengths in points; the frame turns them into pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Wave {
    /// The line's opacity, `0 → 1`.
    pub(crate) alpha: f32,
    /// The resting wave's peak. It fades in while waiting and dies away
    /// after the prompt.
    pub(crate) amp: f32,
    /// The resting wave's phase, radians. It moves while waiting and stops
    /// at the prompt.
    pub(crate) phase: f32,
    /// How far the ring's front has travelled from the ›.
    pub(crate) travel: f32,
    /// The ring's peak; `0` before the prompt.
    pub(crate) kick: f32,
    /// Where the line's colour is between the quiet tone (`0`) and its own
    /// (`1`).
    pub(crate) tone: f32,
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
    /// Everything held back: what the dock shows while the shell starts. A
    /// calendar that does not climb is not lowered either — the wave's line
    /// stands where the dock's will.
    fn waiting(calendar: &Calendar, wave: Option<Wave>) -> Self {
        Self {
            rise: if calendar.rise { 1.0 } else { 0.0 },
            band: 0.0,
            lines: [0.0; 2],
            mark: 0.0,
            caret: 0.0,
            entrance: calendar.entrance,
            letter_entrance: calendar.letters_entrance,
            wave,
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

/// How much of its height the resting wave has `waited` seconds after the pane
/// was born: nothing during the hold, up to full height over
/// [`WAVE_FADE_IN`], and calming over [`WAVE_CALM`] after the cap.
fn envelope(waited: f32) -> f32 {
    unit((waited - SHOW as f32) / WAVE_FADE_IN) * (1.0 - unit((waited - CAP as f32) / WAVE_CALM))
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
            active: false,
            letters: 0,
            caret_at: None,
        }
    }

    fn calendar(&self) -> &'static Calendar {
        if self.reduce {
            return &REDUCED;
        }
        match self.kind {
            // `dust` plays this calendar until it has its own.
            DockArrival::Off | DockArrival::Type | DockArrival::Dust => &TYPED,
            DockArrival::Ripple => &RIPPLED,
        }
    }

    /// Whether the scene is still to come or under way (not ended).
    pub(crate) fn is_armed(&self) -> bool {
        !self.ended
    }

    /// The sleep question. Waiting is settled — nothing moves — and so is an
    /// ended scene; only a playing one keeps the link awake, and a wave that
    /// has started to stir ([`Arrival::stirs`]).
    pub(crate) fn settled(&self) -> bool {
        self.ended || (self.arrived.is_none() && !self.stirs())
    }

    /// Whether a waiting scene is moving: its top line is a wave, the hold is
    /// over and someone is watching. The cap ends it, by arriving the scene.
    fn stirs(&self) -> bool {
        !self.ended
            && self.arrived.is_none()
            && self.active
            && self.calendar().wave
            && self.at >= self.born + SHOW
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
        self.active = active;
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
        playing || self.ended || self.stirs()
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
        for line in calendar.lines {
            end = end.max(line.ends_at());
        }
        if !self.forced {
            end = end.max(at + f32::from(self.typed()) * step + span);
        }
        end.max(self.caret_start() + calendar.caret.1)
    }

    /// The top line as a wave at the stamp the scene was stepped to, if the
    /// calendar has one and it is time for it.
    ///
    /// **Waiting**: nothing until the hold is over (and none for a pane nobody
    /// watches), then a ripple that fades in and moves. **Arrived**: the ripple
    /// keeps the height and opacity it had and the phase it stopped at, and dies
    /// away, and a
    /// ring spreads right from the ›; the line moves from the quiet tone to its
    /// own as it goes flat.
    fn wave(&self, calendar: &Calendar) -> Option<Wave> {
        if !calendar.wave {
            return None;
        }
        let rest = |waited: f32| WAVE_AMP_PT * envelope(waited);
        let phase = |waited: f32| waited / WAVE_PHASE_SECS;
        let Some(arrived) = self.arrived else {
            let waited = ((self.at - self.born) as f32).max(0.0);
            return (self.active && waited >= SHOW as f32).then(|| Wave {
                alpha: unit((waited - SHOW as f32) / WAVE_FADE_IN),
                amp: rest(waited),
                phase: phase(waited),
                travel: 0.0,
                kick: 0.0,
                tone: 0.0,
            });
        };
        let since = ((self.at - arrived) as f32).max(0.0);
        let waited = ((arrived - self.born) as f32).max(0.0);
        // The opacity the waiting line had at the prompt carries on and rises
        // to whole with the ground: a prompt in the middle of the fade-in must
        // not make the line jump.
        let carried = unit((waited - SHOW as f32) / WAVE_FADE_IN);
        (since < WAVE_END).then(|| Wave {
            alpha: carried + (1.0 - carried) * unit(since / calendar.band),
            amp: rest(waited) * (-since / WAVE_SETTLE).exp(),
            phase: phase(waited),
            travel: RING_SPEED * since,
            kick: RING_KICK_PT * (-since / RING_FADE).exp(),
            tone: unit((since - TONE_AT) / TONE_SPAN),
        })
    }

    /// The scene to draw, or `None` once it is over.
    pub(crate) fn scene(&self) -> Option<Scene> {
        if self.ended {
            return None;
        }
        let calendar = self.calendar();
        let wave = self.wave(calendar);
        let Some(arrived) = self.arrived else {
            return Some(Scene::waiting(calendar, wave));
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
            lines: calendar.lines.map(|line| match line {
                Line::Whole => 1.0,
                Line::Drawn(start, length) => ease((since - start) / length),
                Line::Waved => 0.0,
            }),
            mark: unit((since - calendar.mark.0) / calendar.mark.1),
            caret: unit((since - self.caret_start()) / calendar.caret.1),
            entrance: calendar.entrance,
            letter_entrance: calendar.letters_entrance,
            wave,
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
        let mut index = 0;
        while index < 2 {
            end = later(end, calendar.lines[index].ends_at());
            index += 1;
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
            let ripple = rippling(PROMPT, columns);
            assert!(ripple.end_secs() <= longest(&RIPPLED), "{columns}");
        }
        // The bound is the calendars', not the belt's: the belt never fires.
        assert!(longest(&TYPED) < ARRIVAL_MAX as f32);
        assert!(longest(&RIPPLED) < ARRIVAL_MAX as f32);
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
        assert_eq!(scene.letter_entrance, Entrance::Fade);
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

    // **The ripple**: the top line is a wave while the shell starts and a ring
    // spreads from the › when it speaks.

    /// A ripple scene whose prompt came `prompt` seconds after birth, with
    /// `columns` of context text noted. Stepped to the prompt.
    fn rippling(prompt: f64, columns: u16) -> Arrival {
        let mut arrival = Arrival::new(DockArrival::Ripple, BORN, false);
        arrival.advance(BORN + prompt, true);
        arrival.arrive(BORN + prompt, true);
        arrival.note_letters(columns);
        arrival
    }

    /// Steps a ripple scene to `since` seconds after its prompt at `prompt`.
    fn ripple_at(arrival: &mut Arrival, prompt: f64, since: f64) -> Wave {
        arrival.advance(BORN + prompt + since, true);
        arrival
            .scene()
            .expect("armed")
            .wave
            .expect("the wave owns the line")
    }

    #[test]
    fn a_waiting_wave_is_still_for_the_hold_then_stirs_until_the_cap() {
        let mut arrival = Arrival::new(DockArrival::Ripple, BORN, false);
        assert!(arrival.settled(), "the hold is spent asleep");
        assert!(!arrival.advance(BORN + SHOW - 0.01, true));
        assert!(arrival.settled(), "nothing moves before the hold is over");
        assert!(arrival.scene().expect("armed").wave.is_none());
        // The hold is over: the line starts to ripple and the link stays awake.
        assert!(arrival.advance(BORN + SHOW + 0.05, true), "the step drew");
        assert!(!arrival.settled(), "a moving wave keeps the link awake");
        let wave = arrival.scene().expect("armed").wave.expect("stirring");
        assert!(wave.amp > 0.0 && wave.amp < WAVE_AMP_PT, "{wave:?}");
        assert!(wave.alpha > 0.0 && wave.alpha < 1.0, "fading in: {wave:?}");
        assert_eq!(wave.kick, 0.0, "no ring before the shell speaks");
        // Full height by the time it has faded in; it keeps moving.
        let early = wave.phase;
        arrival.advance(BORN + 1.0, true);
        let full = arrival.scene().expect("armed").wave.expect("stirring");
        assert_eq!(full.amp, WAVE_AMP_PT);
        assert_eq!(full.alpha, 1.0);
        assert!(full.phase > early, "the wave travels");
        assert!(!arrival.settled());
        // The cap arrives the scene: the wave is the arrival's from here.
        arrival.advance(BORN + CAP, true);
        assert!(!arrival.settled(), "the arrival plays");
        assert!(arrival.scene().expect("armed").wave.is_some());
    }

    #[test]
    fn the_waiting_amplitude_is_zero_a_calm_after_the_cap() {
        assert_eq!(envelope(0.0), 0.0);
        assert_eq!(envelope(SHOW as f32), 0.0, "nothing before the hold ends");
        assert_eq!(envelope(SHOW as f32 + WAVE_FADE_IN), 1.0);
        assert_eq!(envelope(CAP as f32), 1.0, "full when the cap arrives it");
        let calm = CAP as f32 + WAVE_CALM;
        assert!(envelope(calm) < 1e-5, "{WAVE_CALM} s after the cap");
        assert_eq!(envelope(calm + 1.0), 0.0);
        assert!(envelope(CAP as f32 + WAVE_CALM / 2.0) < 1.0);
    }

    #[test]
    fn a_pane_nobody_watches_does_not_stir() {
        let mut arrival = Arrival::new(DockArrival::Ripple, BORN, false);
        assert!(!arrival.advance(BORN + 1.0, false));
        assert!(arrival.settled(), "an unwatched pane sleeps");
        assert!(arrival.scene().expect("armed").wave.is_none());
        // The pane is watched from now on: the wave starts at once.
        assert!(arrival.advance(BORN + 1.1, true));
        assert!(!arrival.settled());
    }

    #[test]
    fn reduce_motion_and_the_other_kinds_wait_without_moving() {
        let mut reduced = Arrival::new(DockArrival::Ripple, BORN, true);
        reduced.advance(BORN + 1.0, true);
        assert!(reduced.settled(), "Reduce Motion adds no waiting motion");
        assert!(reduced.scene().expect("armed").wave.is_none());
        for kind in [DockArrival::Type, DockArrival::Dust] {
            let mut arrival = Arrival::new(kind, BORN, false);
            arrival.advance(BORN + 1.0, true);
            assert!(arrival.settled(), "{kind:?}");
            assert!(arrival.scene().expect("armed").wave.is_none(), "{kind:?}");
        }
    }

    #[test]
    fn the_ring_spreads_from_the_chevron_and_the_line_flattens() {
        let mut arrival = rippling(1.2, 20);
        let first = ripple_at(&mut arrival, 1.2, 0.0);
        assert_eq!(first.travel, 0.0, "the ring starts at the ›");
        assert_eq!(first.kick, RING_KICK_PT);
        assert_eq!(first.alpha, 1.0, "a line that was whole stays whole");
        assert_eq!(first.tone, 0.0, "it starts in the quiet tone");
        let mut last = first;
        for step in 1..=55 {
            let wave = ripple_at(&mut arrival, 1.2, f64::from(step) * 0.01);
            assert!(wave.travel > last.travel, "the front moves right");
            assert!(wave.kick < last.kick, "the ring dies away");
            assert!(wave.amp < last.amp || last.amp == 0.0, "the rest settles");
            assert_eq!(wave.phase, last.phase, "the phase stops at the prompt");
            assert!(wave.tone >= last.tone, "the tone only moves one way");
            last = wave;
        }
        assert!(
            (last.travel - RING_SPEED * 0.55).abs() < 0.01,
            "{RING_SPEED} points per second: {}",
            last.travel
        );
        assert_eq!(last.tone, 1.0, "the line has its own colour by 540 ms");
        // While the wave is drawn the top line is its own and held back.
        assert_eq!(arrival.scene().expect("armed").lines[0], 0.0);
        // 560 ms: the scene is over and the dock draws its straight line.
        arrival.advance(BORN + 1.2 + f64::from(WAVE_END), true);
        assert!(arrival.scene().is_none());
        assert!(arrival.settled());
    }

    #[test]
    fn the_ring_rises_from_the_waiting_wave_it_found() {
        // A prompt after the wave found its height: the rest decays from it.
        let mut arrival = rippling(1.2, 0);
        let start = ripple_at(&mut arrival, 1.2, 0.0);
        assert_eq!(start.amp, WAVE_AMP_PT);
        let later = ripple_at(&mut arrival, 1.2, WAVE_SETTLE as f64);
        assert!((later.amp - WAVE_AMP_PT / std::f32::consts::E).abs() < 1e-3);
        // A prompt in the hold's first moments: no resting wave, only the ring.
        let mut fast = rippling(0.1, 0);
        let wave = ripple_at(&mut fast, 0.1, 0.0);
        assert_eq!(wave.amp, 0.0);
        assert_eq!(wave.kick, RING_KICK_PT);
        // The cap's arrival finds the wave at full height too.
        let mut forced = Arrival::new(DockArrival::Ripple, BORN, false);
        forced.advance(BORN + CAP, true);
        let wave = forced.scene().expect("armed").wave.expect("owns the line");
        assert_eq!(wave.amp, WAVE_AMP_PT);
    }

    #[test]
    fn the_line_keeps_its_opacity_across_the_prompt() {
        // A prompt in the middle of the line's fade-in: no jump, then whole.
        let mut arrival = Arrival::new(DockArrival::Ripple, BORN, false);
        arrival.advance(BORN + 0.25, true);
        let before = arrival
            .scene()
            .expect("armed")
            .wave
            .expect("stirring")
            .alpha;
        assert!(before > 0.0 && before < 1.0, "{before}");
        arrival.arrive(BORN + 0.25, true);
        let at = arrival
            .scene()
            .expect("armed")
            .wave
            .expect("owns the line")
            .alpha;
        assert_eq!(at, before, "the prompt moved the opacity");
        let mut last = at;
        for step in 1..=17 {
            let wave = ripple_at(&mut arrival, 0.25, f64::from(step) * 0.01);
            assert!(wave.alpha >= last, "the line faded back");
            last = wave.alpha;
        }
        assert_eq!(last, 1.0, "whole with the ground");
        // A shell faster than the hold had no line: it comes in with the ground.
        let mut fast = rippling(0.1, 0);
        assert_eq!(ripple_at(&mut fast, 0.1, 0.0).alpha, 0.0);
        assert!(ripple_at(&mut fast, 0.1, 0.08).alpha > 0.0);
    }

    #[test]
    fn the_ripple_plays_its_own_calendar() {
        let mut arrival = rippling(1.2, 20);
        arrival.advance(BORN + 1.2 + 0.03, true);
        let early = arrival.scene().expect("armed");
        assert_eq!(
            (early.entrance, early.letter_entrance),
            (Entrance::Pop, Entrance::Fade)
        );
        assert_eq!(early.rise, 0.0, "the ripple does not climb");
        assert!(early.band > 0.0 && early.band < 1.0);
        assert_eq!(early.mark, 0.0, "the › starts at 40 ms");
        assert_eq!(early.letter(0), 0.0);
        arrival.advance(BORN + 1.2 + 0.1, true);
        let middle = arrival.scene().expect("armed");
        assert!(middle.mark > 0.0, "{middle:?}");
        assert_eq!(middle.lines[1], 0.0, "the second line starts at 120 ms");
        assert_eq!(middle.caret, 0.0);
        arrival.advance(BORN + 1.2 + 0.15, true);
        assert!(arrival.scene().expect("armed").lines[1] > 0.0);
        // The context row fades in all at once from 200 ms, the cursor after.
        arrival.advance(BORN + 1.2 + 0.25, true);
        let late = arrival.scene().expect("armed");
        assert!(late.letter(0) > 0.0 && late.letter(0) < 1.0, "{late:?}");
        assert_eq!(late.letter(0), late.letter(19), "all at once");
        assert!(late.caret > 0.0, "the cursor starts at 220 ms: {late:?}");
        assert!(run_from(&mut arrival, BORN + 1.2) <= ARRIVAL_MAX);
    }

    #[test]
    fn a_ripple_plays_to_the_end_inside_the_maximum_for_any_text() {
        for columns in [0, 1, 12, LETTER_CAP, u16::MAX] {
            let mut arrival = rippling(1.2, columns);
            assert!(!arrival.settled());
            let took = run_from(&mut arrival, BORN + 1.2);
            assert!(took <= ARRIVAL_MAX + 1.0 / 120.0, "{columns}: {took}s");
            assert!(arrival.settled());
        }
    }

    #[test]
    fn the_ripple_scene_is_deterministic() {
        let mut one = rippling(1.2, 25);
        let mut two = rippling(1.2, 25);
        for since in [0.0, 0.03, 0.11, 0.2, 0.37, 0.5] {
            one.advance(BORN + 1.2 + since, true);
            two.advance(BORN + 1.2 + since, true);
            assert_eq!(one.scene(), two.scene(), "{since}");
        }
    }
}
