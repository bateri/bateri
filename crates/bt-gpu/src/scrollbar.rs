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

use bt_core::ScrollPosition;

use crate::metrics::CellMetrics;

/// The thumb's width while scrolling, points — a design constant (the
/// approved design), like the sizes below; scaled by
/// [`CellMetrics::pt_px`], the gutter's own conversion.
const THUMB_PT: f32 = 6.0;
/// The gap between the thumb and the window's right edge, points.
const INSET_PT: f32 = 4.0;
/// The thumb's shortest length, points: in a long scrollback the visible
/// share of the history is a sliver and a thumb that short could not be seen
/// or grabbed.
const MIN_THUMB_PT: f32 = 24.0;
/// The track's margin at its top and bottom, points: the thumb at either end
/// stops short of the title bar and the dock.
const TRACK_PAD_PT: f32 = 3.0;
/// The strip at the window's right edge the bar owns, points — the region
/// the pointer will widen the bar in. Published with the layout so the mouse
/// side never computes it a second time.
const STRIP_PT: f32 = 16.0;

/// The thumb's opacity over the theme's foreground while scrolling.
pub(crate) const THUMB_ALPHA: f32 = 0.36;

/// How long the bar takes to appear, seconds.
const FADE_IN: f64 = 0.12;
/// How long the bar stays after the last poke, seconds — spent asleep.
const HOLD: f64 = 1.0;
/// How long the bar takes to fade, seconds.
const FADE_OUT: f64 = 0.32;

/// The bar's visibility over time; `Copy`, kept in a `Cell` in the link
/// (blink's precedent).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Scrollbar {
    /// A poke no tick has stamped yet.
    poked: bool,
    /// When the bar began to appear, absolute; `None` → hidden.
    since: Option<f64>,
    /// The last poke's stamp: the hold runs [`HOLD`] from here.
    last: f64,
    /// The opacity the last step handed out — what [`Scrollbar::advance`]
    /// compares against to say whether this tick has anything new to draw.
    shown: f32,
}

impl Scrollbar {
    /// Scrolling input arrived; the next tick stamps it. Ignored when the bar
    /// cannot be drawn (`drawable` is the last content frame's answer — no
    /// travel, the alternate screen): a poke there would draw invisible fade
    /// frames and arm a clock for nothing. `true` → a frame is wanted.
    pub(crate) fn poke(&mut self, drawable: bool) -> bool {
        self.poked |= drawable;
        drawable
    }

    /// Advances the state to `now`; `true` if the opacity differs from the
    /// one handed out last — this tick has something to draw (blink's
    /// `advance` precedent: asked in the sleep question **before** settling,
    /// or the frame the bar settles in would never be drawn and a faint
    /// thumb would stay on screen).
    ///
    /// A poke **continues from the opacity on screen**: re-poked while
    /// fading, the bar climbs back from where it is instead of blinking out.
    /// When the bar cannot be drawn it is hidden at once — no fade frames for
    /// a bar nobody sees.
    pub(crate) fn advance(&mut self, now: f64, drawable: bool) -> bool {
        if !drawable {
            self.poked = false;
            self.since = None;
        } else if std::mem::take(&mut self.poked) {
            let alpha = f64::from(self.alpha(now));
            self.since = Some(now - alpha * FADE_IN);
            self.last = now;
        }
        if self.since.is_some() && now >= self.last + HOLD + FADE_OUT {
            self.since = None;
        }
        let alpha = self.alpha(now);
        let changed = alpha != self.shown;
        self.shown = alpha;
        changed
    }

    /// Hides the bar at once — the stop for paths that can no longer draw it:
    /// a window going invisible (no tick would play the fade, and the bar
    /// would count as unsettled until it came back) and a frame budget spent
    /// on draw errors (the fade would retry a failing draw on every tick).
    /// The next step draws the bar-less frame, because the opacity on screen
    /// is still the old one.
    pub(crate) fn hide(&mut self) {
        self.poked = false;
        self.since = None;
    }

    /// The bar's opacity at `now`, `0..=1`: the rise and the fall, whichever
    /// is lower.
    pub(crate) fn alpha(self, now: f64) -> f32 {
        let Some(since) = self.since else {
            return 0.0;
        };
        let rise = ((now - since) / FADE_IN).clamp(0.0, 1.0);
        let fall = (1.0 - (now - (self.last + HOLD)) / FADE_OUT).clamp(0.0, 1.0);
        rise.min(fall) as f32
    }

    /// Whether the bar needs no frames at `now`: hidden, or fully up and
    /// holding. A pending poke is not settled — its frame is on the way.
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
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScrollbarLayout {
    /// The thumb's travel: the track's top and bottom.
    track: [f32; 2],
    /// The thumb's rectangle: `[x0, y0, x1, y1]`.
    thumb: [f32; 4],
    /// The left edge of the strip the bar owns at the window's right edge.
    strip_x: f32,
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
    /// The track runs from the window's top (the title bar is outside the
    /// view) to the dock's top, both inset by the track's margin; the thumb's
    /// share of it is the visible share of the history, never shorter than
    /// its minimum, and its place on the track is the window's place on its
    /// travel. `None`, a window too narrow or too short for the thumb, give
    /// the non-drawable default.
    pub(crate) fn new(
        position: Option<ScrollPosition>,
        width_px: f32,
        floor_px: f32,
        cell: CellMetrics,
    ) -> Self {
        let Some(position) = position else {
            return Self::default();
        };
        let x1 = width_px - cell.pt_px(INSET_PT);
        let x0 = x1 - cell.pt_px(THUMB_PT);
        let pad = cell.pt_px(TRACK_PAD_PT);
        let (top, bottom) = (pad, floor_px - pad);
        let length = bottom - top;
        if !(x0 >= 0.0 && x1 > x0 && length > 0.0) {
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
            thumb: [x0, y0, x1, y0 + thumb],
            strip_x: width_px - cell.pt_px(STRIP_PT),
            room: position.room,
            top: position.top,
        }
    }

    /// Whether there is a bar to draw and grab.
    pub fn drawable(&self) -> bool {
        self.room > 0
    }

    /// The thumb's rectangle, `[x0, y0, x1, y1]`.
    pub fn thumb(&self) -> [f32; 4] {
        self.thumb
    }

    /// The thumb's travel: the track's top and bottom.
    pub fn track(&self) -> [f32; 2] {
        self.track
    }

    /// The left edge of the strip the bar owns at the window's right edge.
    pub fn strip_x(&self) -> f32 {
        self.strip_x
    }

    /// The pixel inverse: the window's distance from the top of the history
    /// (rows, fractional, `[0, room]`) that puts the thumb's top at `y − grab`
    /// — `grab` is where in the thumb the pointer holds it, so a drag does not
    /// jump the thumb's top to the pointer.
    ///
    /// A thumb that fills its track has no travel and answers the window's
    /// own place: there is nowhere to drag it.
    pub fn position_at(&self, y: f32, grab: f32) -> f32 {
        let travel = (self.track[1] - self.track[0]) - (self.thumb[3] - self.thumb[1]);
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
        let [x0, y0, x1, y1] = top.thumb();
        assert_eq!(x1, 400.0 - INSET_PT);
        assert_eq!(x1 - x0, THUMB_PT);
        assert_eq!(y0, TRACK_PAD_PT, "position 0 is not the track's top");
        // The visible share: 20 of 120 rows.
        let length = 260.0 - 2.0 * TRACK_PAD_PT;
        assert!((y1 - y0 - length * 20.0 / 120.0).abs() < 1e-3);
        let bottom = ScrollbarLayout::new(position(100, 100.0, 20), 400.0, 260.0, cell);
        assert!(
            (bottom.thumb()[3] - (260.0 - TRACK_PAD_PT)).abs() < 1e-3,
            "the bottom is not the track's end, above the dock: {bottom:?}"
        );
        assert_eq!(bottom.strip_x(), 400.0 - STRIP_PT);
    }

    #[test]
    fn a_long_history_keeps_the_thumb_grabbable() {
        let cell = at_1x();
        let layout = ScrollbarLayout::new(position(100_000, 50_000.0, 20), 400.0, 260.0, cell);
        let [_, y0, _, y1] = layout.thumb();
        assert_eq!(y1 - y0, MIN_THUMB_PT, "the thumb shrank under its minimum");
        // At @2x the minimum doubles with every other size.
        let retina = CellMetrics::new(16, 32, 16, 16, 2, 2.0).expect("non-zero cell");
        let layout = ScrollbarLayout::new(position(100_000, 0.0, 20), 800.0, 520.0, retina);
        let [x0, y0, x1, y1] = layout.thumb();
        assert_eq!(y1 - y0, 2.0 * MIN_THUMB_PT);
        assert_eq!(x1 - x0, 2.0 * THUMB_PT);
        assert_eq!(y0, 2.0 * TRACK_PAD_PT);
    }

    #[test]
    fn the_pixel_inverse_gives_the_position_back() {
        let cell = at_1x();
        for top in [0.0, 13.5, 50.0, 99.0, 100.0] {
            let layout = ScrollbarLayout::new(position(100, top, 20), 400.0, 260.0, cell);
            let [_, y0, _, _] = layout.thumb();
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
        // Narrower than the thumb and its gap.
        assert!(!ScrollbarLayout::new(position(10, 0.0, 2), 8.0, 260.0, cell).drawable());
        // A track shorter than the minimum: the thumb fills it and a drag
        // answers the window's own place.
        let short = ScrollbarLayout::new(position(10, 4.0, 2), 400.0, 20.0, cell);
        assert!(short.drawable());
        assert_eq!(
            short.thumb()[3] - short.thumb()[1],
            20.0 - 2.0 * TRACK_PAD_PT
        );
        assert_eq!(short.position_at(0.0, 0.0), 4.0);
    }
}
