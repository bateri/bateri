//! The scroll bar — **pure**: its sizes, its visibility over time and where
//! it lands on the window. No GPU, no ObjC, no lock; [`crate::link`] steps
//! it, [`crate::frame::Frame`] turns it into one rounded quad.
//!
//! **What shows it is scrolling input, never output.** The shell's scroll
//! gates (the wheel, page scrolls, search navigation) poke it; a window
//! scrolled up while output streams below does not light it up — alacritty
//! pins a scrolled window against output by growing the offset, so watching
//! the position would show the bar on every line a build prints. Input is
//! also the only signal that sees a scroll that could not move (the wheel at
//! the bottom): the bar answers "you are at the end".
//!
//! **Timing is absolute, like [`crate::blink`]'s** and for the same reason: a
//! state accumulating `dt` would lose most of the 120 ms appearance to
//! [`crate::motion::DT_MAX`]'s clamp on the first frame after a sleep. Stamps
//! are the tick's own `now`; a poke is a bit the next tick stamps, so the poke
//! path reads no clock.
//!
//! **The hold is sleep.** For the second the bar stays up after the last poke
//! nothing on screen changes, so the state counts as settled, the link goes
//! to sleep and arms its clock at the hold's end in the **motion** flavour
//! ([`Scrollbar::next_deadline`]): the bar's frames change only the bar, never
//! the content, so they must not count in `content=`. The fade then wakes the
//! link and draws until the bar is gone. The stop condition is named: the
//! hold ends, the fade completes, and with no poke nothing is armed.
//!
//! **What need not be hooked**: occlusion, a draw error and a missing
//! drawable leave the state alone, because a state of absolute stamps has
//! nothing to finish — the next step's `advance(now)` lands where the clock
//! says, straight on hidden after a long sleep.
//!
//! **Three forms** ([`Mode`], resolved by `bt-shell` from the setting and the
//! system's preference): `Auto` is the timeline above, thin; `Always` is the
//! wide form over its track, fully up and never fading — no timeline, no
//! clock, nothing in flight; `Never` draws nothing and ignores pokes. The form
//! is a [`Look`] the frame paints from, so a change of form redraws from the
//! kept layout without a content frame.

use bt_core::ScrollPosition;

use crate::metrics::CellMetrics;

/// The thumb's width while scrolling, points — a design constant (the
/// approved design), like the sizes below; scaled by
/// [`CellMetrics::pt_px`], the gutter's own conversion.
const THUMB_PT: f32 = 6.0;
/// The gap between the thumb and the window's right edge, points.
const INSET_PT: f32 = 4.0;
/// The wide form's thumb ([`Look::wide`]): wider and nearer the edge, so it
/// sits centred-right on its track.
const WIDE_THUMB_PT: f32 = 10.0;
/// The wide thumb's gap from the window's right edge, points.
const WIDE_INSET_PT: f32 = 3.0;
/// The hairline at the track's left edge, points: where the text's area ends.
const HAIRLINE_PT: f32 = 1.0;
/// The thumb's shortest length, points: in a long scrollback the visible
/// share of the history is a sliver and a thumb that short could not be seen
/// or grabbed.
const MIN_THUMB_PT: f32 = 24.0;
/// The track's margin at its top and bottom, points: the thumb at either end
/// stops short of the title bar and the dock.
const TRACK_PAD_PT: f32 = 3.0;
/// The track's width, points: the strip at the window's right edge the bar
/// owns — the region the pointer will widen the bar in, and the room the grid
/// gives up in [`Mode::Always`]. Converted in one place ([`track_px`]):
/// `bt-shell`'s grid arithmetic gets it through [`Mode::reserve_px`], the
/// layout through the same function, so the reserve and the drawn track are
/// the same pixels. **Everything the bar draws stays inside it**, the
/// hairline included (its leftmost point), so text in a reserved grid never
/// runs under any of it. Published with the layout so the mouse side never
/// computes it a second time.
pub(crate) const TRACK_PT: f32 = 16.0;

/// The thumb's opacity over the theme's foreground while scrolling.
pub(crate) const THUMB_ALPHA: f32 = 0.36;
/// The thumb's opacity in [`Mode::Always`]: quieter than while scrolling,
/// because it never leaves the screen.
pub(crate) const ALWAYS_THUMB_ALPHA: f32 = 0.30;
/// The track's opacity over the foreground, at full width.
pub(crate) const TRACK_ALPHA: f32 = 0.05;
/// The hairline's opacity over the foreground, at full width.
pub(crate) const HAIRLINE_ALPHA: f32 = 0.10;

/// How long the bar takes to appear, seconds.
const FADE_IN: f64 = 0.12;
/// How long the bar stays after the last poke, seconds — spent asleep.
const HOLD: f64 = 1.0;
/// How long the bar takes to fade, seconds.
const FADE_OUT: f64 = 0.32;

/// The scroll bar's form — `[terminal] scrollbar` **resolved**: `bt-shell`
/// combines the setting with the system's scroll bar preference and gives
/// one of these (the `set_reduce_motion` precedent); this crate sees neither
/// the settings file nor the system.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// Shows on scrolling input, thin, and fades after the hold.
    #[default]
    Auto,
    /// Always on screen, wide, over a track the grid makes room for; never
    /// fades, so it puts nothing in flight.
    Always,
    /// Never drawn; pokes are ignored.
    Never,
}

impl Mode {
    /// Whether this form takes the track's width from the grid — `Always`
    /// alone. A change that flips it is a resize; any other is not.
    pub fn reserves(self) -> bool {
        self == Mode::Always
    }

    /// The width the grid gives up at the window's right edge in this form,
    /// **physical pixels**: the track in `Always`, nothing otherwise.
    ///
    /// **A function of the form alone** — not of the history or the
    /// alternate screen, where the bar is not drawn: the strip stays reserved
    /// and empty there, or the first line into history, every clear and every
    /// full-screen program would resize the grid (`TIOCSWINSZ`). The track's
    /// one conversion ([`track_px`]), so the reserve and the drawn track are
    /// the same pixels.
    pub fn reserve_px(self, cell: CellMetrics) -> f32 {
        if self.reserves() { track_px(cell) } else { 0.0 }
    }
}

/// The track's width in physical pixels — **the one conversion** of
/// [`TRACK_PT`]: the grid's reserve ([`Mode::reserve_px`]) and the drawn
/// strip ([`ScrollbarLayout::new`]) both come from here.
fn track_px(cell: CellMetrics) -> f32 {
    cell.pt_px(TRACK_PT)
}

/// What the frame paints the bar with — the state's answer at one moment.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Look {
    /// The bar's visibility, `0..=1`; every part's opacity is multiplied by
    /// it. Zero → nothing is drawn.
    pub(crate) alpha: f32,
    /// How wide the bar is, `0` thin `..=1` wide; the track and its hairline
    /// show in proportion.
    pub(crate) wide: f32,
    /// The thumb's own opacity over the foreground.
    pub(crate) thumb: f32,
}

impl Look {
    /// No bar.
    pub(crate) const HIDDEN: Look = Look {
        alpha: 0.0,
        wide: 0.0,
        thumb: 0.0,
    };

    /// [`Mode::Always`]'s look: wide, over its track, fully up.
    pub(crate) const ALWAYS: Look = Look {
        alpha: 1.0,
        wide: 1.0,
        thumb: ALWAYS_THUMB_ALPHA,
    };

    /// [`Mode::Auto`]'s look at visibility `alpha`: thin, no track. A fully
    /// faded one is [`Look::HIDDEN`], so "hidden" has one representation and
    /// the step's comparison cannot see a change that draws nothing.
    pub(crate) fn auto(alpha: f32) -> Look {
        if alpha > 0.0 {
            Look {
                alpha,
                wide: 0.0,
                thumb: THUMB_ALPHA,
            }
        } else {
            Look::HIDDEN
        }
    }
}

/// The bar's visibility over time; `Copy`, kept in a `Cell` in the link
/// (blink's precedent).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Scrollbar {
    /// The form ([`Mode`]); only `Auto` has a timeline.
    mode: Mode,
    /// A poke no tick has stamped yet.
    poked: bool,
    /// When the bar began to appear, absolute; `None` → hidden.
    since: Option<f64>,
    /// The last poke's stamp: the hold runs [`HOLD`] from here.
    last: f64,
    /// The look the last step handed out — what [`Scrollbar::advance`]
    /// compares against to say whether this tick has anything new to draw.
    shown: Look,
}

impl Scrollbar {
    /// The form changed; `true` if it is a different one — the caller asks
    /// for a frame. The timeline starts over hidden: a bar leaving `Always`
    /// goes at once, one coming from it does not fade, and `Never` has no
    /// timeline at all.
    pub(crate) fn set_mode(&mut self, mode: Mode) -> bool {
        if self.mode == mode {
            return false;
        }
        self.mode = mode;
        self.poked = false;
        self.since = None;
        true
    }

    /// Scrolling input arrived; the next tick stamps it. Ignored when the bar
    /// cannot be drawn (`drawable` is the last content frame's answer — no
    /// travel, the alternate screen): a poke there would draw invisible fade
    /// frames and arm a clock for nothing. Ignored outside `Auto` too:
    /// `Always` is up already and `Never` shows nothing. `true` → a frame is
    /// wanted.
    pub(crate) fn poke(&mut self, drawable: bool) -> bool {
        let wanted = drawable && self.mode == Mode::Auto;
        self.poked |= wanted;
        wanted
    }

    /// Advances the state to `now`; `true` if the look differs from the
    /// one handed out last — this tick has something to draw (blink's
    /// `advance` precedent: asked in the sleep question **before** settling,
    /// or the frame the bar settles in would never be drawn and a faint
    /// thumb would stay on screen).
    ///
    /// A poke **continues from the opacity on screen**: re-poked while
    /// fading, the bar climbs back from where it is instead of blinking out.
    /// When the bar cannot be drawn it is hidden at once — no fade frames for
    /// a bar nobody sees; outside `Auto` there is no timeline to run.
    pub(crate) fn advance(&mut self, now: f64, drawable: bool) -> bool {
        if !drawable || self.mode != Mode::Auto {
            self.poked = false;
            self.since = None;
        } else if std::mem::take(&mut self.poked) {
            let alpha = f64::from(self.fade(now));
            self.since = Some(now - alpha * FADE_IN);
            self.last = now;
        }
        if self.since.is_some() && now >= self.last + HOLD + FADE_OUT {
            self.since = None;
        }
        let look = if drawable {
            self.look(now)
        } else {
            Look::HIDDEN
        };
        let changed = look != self.shown;
        self.shown = look;
        changed
    }

    /// Hides the bar at once — the stop for paths that can no longer draw it:
    /// a window going invisible (no tick would play the fade, and the bar
    /// would count as unsettled until it came back) and a frame budget spent
    /// on draw errors (the fade would retry a failing draw on every tick).
    /// The next step draws the bar-less frame, because the look on screen
    /// is still the old one. `Always` keeps its form — it is not an
    /// animation, nothing of it is in flight — and the next content frame
    /// draws it again.
    pub(crate) fn hide(&mut self) {
        self.poked = false;
        self.since = None;
    }

    /// What the bar looks like at `now` in its form ([`Look`]).
    pub(crate) fn look(self, now: f64) -> Look {
        match self.mode {
            Mode::Auto => Look::auto(self.fade(now)),
            Mode::Always => Look::ALWAYS,
            Mode::Never => Look::HIDDEN,
        }
    }

    /// The bar's visibility at `now`, `0..=1`.
    #[cfg(test)]
    pub(crate) fn alpha(self, now: f64) -> f32 {
        self.look(now).alpha
    }

    /// `Auto`'s timeline at `now`, `0..=1`: the rise and the fall, whichever
    /// is lower.
    fn fade(self, now: f64) -> f32 {
        let Some(since) = self.since else {
            return 0.0;
        };
        let rise = ((now - since) / FADE_IN).clamp(0.0, 1.0);
        let fall = (1.0 - (now - (self.last + HOLD)) / FADE_OUT).clamp(0.0, 1.0);
        rise.min(fall) as f32
    }

    /// Whether the bar needs no frames at `now`: hidden, or fully up and
    /// holding — and always outside `Auto`, whose forms have no timeline. A
    /// pending poke is not settled — its frame is on the way.
    pub(crate) fn settled(self, now: f64) -> bool {
        if self.poked {
            return false;
        }
        match self.since {
            None => true,
            Some(since) => now >= since + FADE_IN && now < self.last + HOLD,
        }
    }

    /// The absolute time the bar next needs a frame from a sleeping link: the
    /// hold's end. `None` while hidden — the stop condition. Only read at a
    /// sleep point, where the bar is settled; while it appears or fades the
    /// link is awake and draws every tick anyway.
    pub(crate) fn next_deadline(self) -> Option<f64> {
        self.since.map(|_| self.last + HOLD)
    }
}

/// Where the bar lands on the window — **the one copy** of the bar's pixel
/// arithmetic: the drawing reads the thumb from it and the mouse side reads
/// the same value from the drawn frame's publication
/// ([`crate::Origin::scrollbar`]), so a click and a pixel cannot disagree.
///
/// Physical pixels, from the window's top-left. The default is **not
/// drawable**: no travel, nothing to draw or grab.
///
/// **Both widths are here**, the thin and the wide thumb's: the form is the
/// state's ([`Look::wide`]), and a form that changes between content frames
/// is drawn from the kept layout — the vertical arithmetic does not depend on
/// the width.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScrollbarLayout {
    /// The thumb's travel: the track's top and bottom.
    track: [f32; 2],
    /// The thumb's top and bottom.
    thumb_y: [f32; 2],
    /// The thin thumb's left and right edge.
    thin_x: [f32; 2],
    /// The wide thumb's left and right edge.
    wide_x: [f32; 2],
    /// The strip the bar owns at the window's right edge — the track:
    /// `[x0, y0, x1, y1]`, from the window's top to the dock's.
    strip: [f32; 4],
    /// The hairline's width at the strip's left edge, pixels.
    hairline: f32,
    /// The window's travel in rows ([`ScrollPosition::room`]); zero → not
    /// drawable.
    room: u32,
    /// The window's distance from the top of the history, rows — what a
    /// thumb with no travel left answers to the pixel inverse.
    top: f32,
}

impl ScrollbarLayout {
    /// The layout of a window `width_px` wide whose drawable area above the
    /// dock ends at `floor_px`.
    ///
    /// The thumb's travel runs from the window's top (the title bar is
    /// outside the view) to the dock's top, both inset by the track's margin;
    /// the thumb's share of it is the visible share of the history, never
    /// shorter than its minimum, and its place on the travel is the window's
    /// place on its own. The strip — the wide form's track — is the full
    /// height between the two. `None`, a window too narrow for the strip or
    /// too short for the thumb, give the non-drawable default.
    pub(crate) fn new(
        position: Option<ScrollPosition>,
        width_px: f32,
        floor_px: f32,
        cell: CellMetrics,
    ) -> Self {
        let Some(position) = position else {
            return Self::default();
        };
        let thin_x1 = width_px - cell.pt_px(INSET_PT);
        let wide_x1 = width_px - cell.pt_px(WIDE_INSET_PT);
        let strip_x = width_px - track_px(cell);
        let pad = cell.pt_px(TRACK_PAD_PT);
        let (top, bottom) = (pad, floor_px - pad);
        let length = bottom - top;
        if !(strip_x >= 0.0 && width_px > strip_x && length > 0.0) {
            return Self::default();
        }
        // The travel plus the visible rows is the whole length: at the bottom
        // the thumb's end meets the track's.
        let total = f64::from(position.room) + f64::from(position.visible);
        let share = (f64::from(position.visible) / total) as f32;
        let thumb = (length * share).max(cell.pt_px(MIN_THUMB_PT)).min(length);
        let y0 = top + (length - thumb) * (position.top / position.room as f32);
        Self {
            track: [top, bottom],
            thumb_y: [y0, y0 + thumb],
            thin_x: [thin_x1 - cell.pt_px(THUMB_PT), thin_x1],
            wide_x: [wide_x1 - cell.pt_px(WIDE_THUMB_PT), wide_x1],
            strip: [strip_x, 0.0, width_px, floor_px],
            hairline: cell.pt_px(HAIRLINE_PT),
            room: position.room,
            top: position.top,
        }
    }

    /// Whether there is a bar to draw and grab.
    pub fn drawable(&self) -> bool {
        self.room > 0
    }

    /// The thumb's rectangle, `[x0, y0, x1, y1]`, `wide` of the way from the
    /// thin form (`0`) to the wide one (`1`).
    pub fn thumb(&self, wide: f32) -> [f32; 4] {
        let wide = wide.clamp(0.0, 1.0);
        let lerp = |thin: f32, wide_edge: f32| thin + (wide_edge - thin) * wide;
        [
            lerp(self.thin_x[0], self.wide_x[0]),
            self.thumb_y[0],
            lerp(self.thin_x[1], self.wide_x[1]),
            self.thumb_y[1],
        ]
    }

    /// The thumb's travel: the track's top and bottom.
    pub fn track(&self) -> [f32; 2] {
        self.track
    }

    /// The left edge of the strip the bar owns at the window's right edge.
    pub fn strip_x(&self) -> f32 {
        self.strip[0]
    }

    /// The wide form's track and its hairline, `[x0, y0, x1, y1]` each:
    /// side by side, the hairline the strip's leftmost pixels and the track
    /// the rest, so each reads at its own opacity.
    pub(crate) fn track_parts(&self) -> [[f32; 4]; 2] {
        let [x0, y0, x1, y1] = self.strip;
        let edge = (x0 + self.hairline).min(x1);
        [[edge, y0, x1, y1], [x0, y0, edge, y1]]
    }

    /// The pixel inverse: the window's distance from the top of the history
    /// (rows, fractional, `[0, room]`) that puts the thumb's top at `y − grab`
    /// — `grab` is where in the thumb the pointer holds it, so a drag does not
    /// jump the thumb's top to the pointer.
    ///
    /// A thumb that fills its track has no travel and answers the window's
    /// own place: there is nowhere to drag it.
    pub fn position_at(&self, y: f32, grab: f32) -> f32 {
        let travel = (self.track[1] - self.track[0]) - (self.thumb_y[1] - self.thumb_y[0]);
        if travel.is_nan() || travel <= 0.0 {
            return self.top;
        }
        let fraction = ((y - grab - self.track[0]) / travel).clamp(0.0, 1.0);
        fraction * self.room as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A @1x metric: points are pixels, so the design sizes read as written.
    fn at_1x() -> CellMetrics {
        CellMetrics::new(8, 16, 8, 8, 1, 1.0).expect("non-zero cell")
    }

    fn position(room: u32, top: f32, visible: u16) -> Option<ScrollPosition> {
        Some(ScrollPosition { room, top, visible })
    }

    #[test]
    fn a_poke_fades_in_holds_asleep_and_fades_out() {
        let mut bar = Scrollbar::default();
        assert!(bar.poke(true));
        assert!(!bar.settled(0.0), "a pending poke is settled");
        bar.advance(0.0, true);
        assert_eq!(bar.alpha(0.0), 0.0);
        // Appearing: from zero to one over 120 ms, and not settled — the link
        // stays awake for it.
        assert!(bar.advance(0.06, true));
        assert!((bar.alpha(0.06) - 0.5).abs() < 1e-6);
        assert!(!bar.settled(0.06));
        assert!(
            bar.advance(FADE_IN, true),
            "the last rising step was not drawn"
        );
        assert_eq!(bar.alpha(FADE_IN), 1.0);
        // The hold is sleep: settled, and the clock is the hold's end.
        for now in [FADE_IN, 0.5, 0.99] {
            assert!(!bar.advance(now, true), "the hold changed at {now}");
            assert!(bar.settled(now), "the hold is not settled at {now}");
            assert_eq!(bar.next_deadline(), Some(HOLD));
        }
        // Fading: from the hold's end over 320 ms, awake again.
        assert!(!bar.settled(HOLD));
        assert!(bar.advance(HOLD + FADE_OUT / 2.0, true));
        assert!((bar.alpha(HOLD + FADE_OUT / 2.0) - 0.5).abs() < 1e-6);
        assert!(!bar.settled(HOLD + FADE_OUT / 2.0));
        // Gone: the last step is drawn (the change), then nothing is armed.
        assert!(bar.advance(HOLD + FADE_OUT, true));
        assert_eq!(bar.alpha(HOLD + FADE_OUT), 0.0);
        assert!(bar.settled(HOLD + FADE_OUT));
        assert_eq!(bar.next_deadline(), None, "a hidden bar armed a clock");
    }

    #[test]
    fn a_poke_in_the_hold_extends_it() {
        let mut bar = Scrollbar::default();
        bar.poke(true);
        bar.advance(0.0, true);
        bar.advance(0.5, true);
        bar.poke(true);
        assert!(
            !bar.advance(0.8, true),
            "a poke in the hold changed the bar"
        );
        assert_eq!(bar.alpha(0.8), 1.0);
        assert_eq!(bar.next_deadline(), Some(0.8 + HOLD));
        assert!(
            bar.settled(1.5),
            "the extended hold ended at the old deadline"
        );
    }

    #[test]
    fn a_poke_while_fading_climbs_back_from_the_opacity_on_screen() {
        let mut bar = Scrollbar::default();
        bar.poke(true);
        bar.advance(0.0, true);
        let fading = HOLD + FADE_OUT * 0.75;
        bar.advance(fading, true);
        let on_screen = bar.alpha(fading);
        assert!(on_screen > 0.0 && on_screen < 0.5, "{on_screen}");
        bar.poke(true);
        bar.advance(fading, true);
        assert!(
            (bar.alpha(fading) - on_screen).abs() < 1e-6,
            "the bar jumped on a poke"
        );
        assert!(
            bar.alpha(fading + 0.01) > on_screen,
            "it did not climb back"
        );
        assert_eq!(bar.next_deadline(), Some(fading + HOLD));
    }

    #[test]
    fn a_poke_with_nothing_to_draw_is_ignored() {
        // No travel or the alternate screen: the poke asks for no frame and
        // leaves nothing pending, so no fade frame and no clock.
        let mut bar = Scrollbar::default();
        assert!(!bar.poke(false), "an undrawable bar asked for a frame");
        assert!(bar.settled(0.0));
        assert_eq!(bar.next_deadline(), None);
        // And a bar on screen that stops being drawable goes at once.
        bar.poke(true);
        bar.advance(0.0, true);
        bar.advance(0.5, true);
        assert!(bar.advance(0.6, false), "the vanished bar was not redrawn");
        assert_eq!(bar.alpha(0.6), 0.0);
        assert!(bar.settled(0.6));
        assert_eq!(bar.next_deadline(), None);
    }

    #[test]
    fn a_hidden_bar_is_settled_and_draws_its_last_frame() {
        // Occlusion or a spent draw budget mid-fade: settled at once, nothing
        // armed, and the next step still reports the change so the bar-less
        // frame is drawn.
        let mut bar = Scrollbar::default();
        bar.poke(true);
        bar.advance(0.0, true);
        bar.advance(0.06, true);
        bar.poke(true);
        bar.hide();
        assert!(bar.settled(0.07), "a hidden bar is unsettled");
        assert_eq!(bar.next_deadline(), None);
        assert!(bar.advance(0.07, true), "the bar-less frame was not drawn");
        assert_eq!(bar.alpha(0.07), 0.0);
    }

    #[test]
    fn a_long_sleep_lands_straight_on_hidden() {
        // The window was occluded through the hold: the first step after it
        // jumps to the end without playing the fade.
        let mut bar = Scrollbar::default();
        bar.poke(true);
        bar.advance(0.0, true);
        bar.advance(0.5, true);
        assert!(bar.advance(60.0, true), "the gone bar was not redrawn");
        assert_eq!(bar.alpha(60.0), 0.0);
        assert!(bar.settled(60.0));
        assert_eq!(bar.next_deadline(), None);
        assert!(!bar.advance(60.1, true), "a hidden bar keeps changing");
    }

    #[test]
    fn the_thumb_runs_from_the_top_to_the_dock() {
        // A 400×300 window, the dock's top at 260: the track is inset by the
        // margin at both ends, the thumb by its gap from the right edge.
        let cell = at_1x();
        let top = ScrollbarLayout::new(position(100, 0.0, 20), 400.0, 260.0, cell);
        assert!(top.drawable());
        assert_eq!(top.track(), [TRACK_PAD_PT, 260.0 - TRACK_PAD_PT]);
        let [x0, y0, x1, y1] = top.thumb(0.0);
        assert_eq!(x1, 400.0 - INSET_PT);
        assert_eq!(x1 - x0, THUMB_PT);
        assert_eq!(y0, TRACK_PAD_PT, "position 0 is not the track's top");
        // The visible share: 20 of 120 rows.
        let length = 260.0 - 2.0 * TRACK_PAD_PT;
        assert!((y1 - y0 - length * 20.0 / 120.0).abs() < 1e-3);
        let bottom = ScrollbarLayout::new(position(100, 100.0, 20), 400.0, 260.0, cell);
        assert!(
            (bottom.thumb(0.0)[3] - (260.0 - TRACK_PAD_PT)).abs() < 1e-3,
            "the bottom is not the track's end, above the dock: {bottom:?}"
        );
        assert_eq!(bottom.strip_x(), 400.0 - TRACK_PT);
    }

    #[test]
    fn a_long_history_keeps_the_thumb_grabbable() {
        let cell = at_1x();
        let layout = ScrollbarLayout::new(position(100_000, 50_000.0, 20), 400.0, 260.0, cell);
        let [_, y0, _, y1] = layout.thumb(0.0);
        assert_eq!(y1 - y0, MIN_THUMB_PT, "the thumb shrank under its minimum");
        // At @2x the minimum doubles with every other size.
        let retina = CellMetrics::new(16, 32, 16, 16, 2, 2.0).expect("non-zero cell");
        let layout = ScrollbarLayout::new(position(100_000, 0.0, 20), 800.0, 520.0, retina);
        let [x0, y0, x1, y1] = layout.thumb(0.0);
        assert_eq!(y1 - y0, 2.0 * MIN_THUMB_PT);
        assert_eq!(x1 - x0, 2.0 * THUMB_PT);
        assert_eq!(y0, 2.0 * TRACK_PAD_PT);
    }

    #[test]
    fn the_pixel_inverse_gives_the_position_back() {
        let cell = at_1x();
        for top in [0.0, 13.5, 50.0, 99.0, 100.0] {
            let layout = ScrollbarLayout::new(position(100, top, 20), 400.0, 260.0, cell);
            let [_, y0, _, _] = layout.thumb(0.0);
            // Held 5 px below its top edge: the pointer is there.
            let back = layout.position_at(y0 + 5.0, 5.0);
            assert!((back - top).abs() < 1e-3, "{top} → {back}");
        }
        // Past either end the inverse clamps to the travel.
        let layout = ScrollbarLayout::new(position(100, 50.0, 20), 400.0, 260.0, cell);
        assert_eq!(layout.position_at(-50.0, 0.0), 0.0);
        assert_eq!(layout.position_at(10_000.0, 0.0), 100.0);
    }

    #[test]
    fn nothing_to_travel_or_no_room_is_not_drawable() {
        let cell = at_1x();
        assert!(!ScrollbarLayout::new(None, 400.0, 260.0, cell).drawable());
        // A window shorter than the track's two margins.
        assert!(!ScrollbarLayout::new(position(10, 0.0, 2), 400.0, 5.0, cell).drawable());
        // Narrower than the strip the bar owns.
        assert!(!ScrollbarLayout::new(position(10, 0.0, 2), 8.0, 260.0, cell).drawable());
        // A track shorter than the minimum: the thumb fills it and a drag
        // answers the window's own place.
        let short = ScrollbarLayout::new(position(10, 4.0, 2), 400.0, 20.0, cell);
        assert!(short.drawable());
        assert_eq!(
            short.thumb(0.0)[3] - short.thumb(0.0)[1],
            20.0 - 2.0 * TRACK_PAD_PT
        );
        assert_eq!(short.position_at(0.0, 0.0), 4.0);
    }

    #[test]
    fn always_is_up_wide_and_puts_nothing_in_flight() {
        let mut bar = Scrollbar::default();
        assert!(bar.set_mode(Mode::Always));
        assert!(
            !bar.set_mode(Mode::Always),
            "the same form asked for a frame"
        );
        // The first step draws the form; after it nothing changes, nothing is
        // armed and a poke wants no frame — the bar is up already.
        assert!(bar.advance(0.0, true), "the form was not drawn");
        assert_eq!(bar.look(0.0), Look::ALWAYS);
        assert!(bar.settled(0.0));
        assert_eq!(bar.next_deadline(), None);
        assert!(
            !bar.poke(true),
            "a poke on an always-up bar asked for a frame"
        );
        for now in [0.5, HOLD + FADE_OUT, 60.0] {
            assert!(!bar.advance(now, true), "the bar changed at {now}");
            assert_eq!(bar.alpha(now), 1.0, "the bar faded at {now}");
            assert!(bar.settled(now));
            assert_eq!(bar.next_deadline(), None);
        }
        // Where it cannot be drawn (the alternate screen) it is gone, and
        // back when it can.
        assert!(bar.advance(61.0, false));
        assert!(bar.settled(61.0));
        assert!(bar.advance(62.0, true));
        assert_eq!(bar.look(62.0), Look::ALWAYS);
        // Occlusion's stop leaves the form: nothing of it is in flight.
        bar.hide();
        assert_eq!(bar.look(63.0), Look::ALWAYS);
    }

    #[test]
    fn never_draws_nothing_and_ignores_pokes() {
        let mut bar = Scrollbar::default();
        bar.set_mode(Mode::Never);
        assert!(!bar.poke(true), "a poke on a hidden form asked for a frame");
        assert!(!bar.advance(0.0, true), "a never-shown bar changed");
        assert_eq!(bar.look(0.0), Look::HIDDEN);
        assert!(bar.settled(0.0));
        assert_eq!(bar.next_deadline(), None);
    }

    #[test]
    fn a_new_form_starts_from_hidden_at_once() {
        // A bar holding in `Auto` that becomes `Never` goes in the next step,
        // without a fade and without a clock.
        let mut bar = Scrollbar::default();
        bar.poke(true);
        bar.advance(0.0, true);
        bar.advance(0.5, true);
        assert!(bar.set_mode(Mode::Never));
        assert!(bar.advance(0.6, true), "the vanished bar was not redrawn");
        assert_eq!(bar.look(0.6), Look::HIDDEN);
        assert_eq!(bar.next_deadline(), None);
        // `Always` back to `Auto`: hidden until the next scroll, not a fade.
        bar.set_mode(Mode::Always);
        bar.advance(1.0, true);
        bar.set_mode(Mode::Auto);
        assert!(bar.advance(1.1, true));
        assert_eq!(bar.look(1.1), Look::HIDDEN);
        assert!(bar.settled(1.1));
        assert_eq!(bar.next_deadline(), None);
    }

    #[test]
    fn only_always_reserves_the_track() {
        assert_eq!(Mode::Always.reserve_px(at_1x()), TRACK_PT);
        let retina = CellMetrics::new(16, 32, 16, 16, 2, 2.0).expect("non-zero cell");
        assert_eq!(Mode::Always.reserve_px(retina), 2.0 * TRACK_PT);
        for mode in [Mode::Auto, Mode::Never] {
            assert_eq!(mode.reserve_px(retina), 0.0, "{mode:?}");
        }
    }

    #[test]
    fn the_wide_form_stays_inside_the_reserved_track() {
        // Everything the wide form draws — its track, its hairline and its
        // thumb — lies in the strip the grid gives up, so text never runs
        // under it.
        let cell = at_1x();
        let layout = ScrollbarLayout::new(position(100, 50.0, 20), 400.0, 260.0, cell);
        let strip_x = 400.0 - Mode::Always.reserve_px(cell);
        assert_eq!(layout.strip_x(), strip_x);
        let [track, hairline] = layout.track_parts();
        assert_eq!(hairline, [strip_x, 0.0, strip_x + HAIRLINE_PT, 260.0]);
        assert_eq!(track, [strip_x + HAIRLINE_PT, 0.0, 400.0, 260.0]);
        let [x0, y0, x1, y1] = layout.thumb(1.0);
        assert_eq!(
            [x0, x1],
            [400.0 - WIDE_INSET_PT - WIDE_THUMB_PT, 400.0 - WIDE_INSET_PT]
        );
        assert!(
            x0 > strip_x + HAIRLINE_PT,
            "the thumb overlaps the hairline"
        );
        // The width does not move the thumb vertically.
        assert_eq!([y0, y1], [layout.thumb(0.0)[1], layout.thumb(0.0)[3]]);
    }
}
