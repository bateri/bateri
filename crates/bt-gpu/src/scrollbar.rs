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
//!
//! **The pointer engages the bar** ([`Scrollbar::set_hover`],
//! [`Scrollbar::set_drag`]): over its strip `Auto` shows and widens to the
//! wide form in 150 ms, the thumb darkens in 120 ms (more while dragged), and
//! while engaged the bar neither holds nor fades — up and settled, no clock,
//! no frame. Leaving narrows it, then the hold and the fade run as after a
//! scroll. `Always` only darkens. Like a poke, the pointer's change is a bit
//! the next tick stamps, and a transition starts from what is on screen, so a
//! reversal midway does not jump. Reduce Motion and `snap` make the widening
//! instant, read when a transition **starts** (the one in flight ends on its
//! own within 150 ms); the fades and the tone stay, they move nothing.

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
/// The thumb's opacity with the pointer over the strip: it can be grabbed.
pub(crate) const HOVER_THUMB_ALPHA: f32 = 0.48;
/// The thumb's opacity while it is dragged.
pub(crate) const DRAG_THUMB_ALPHA: f32 = 0.62;
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
/// How long the bar takes to widen or narrow as the pointer comes and goes,
/// seconds; zero under Reduce Motion and `snap`.
const WIDEN: f64 = 0.15;
/// How long the thumb takes to change its tone, seconds — kept under Reduce
/// Motion: a tone moves nothing.
const TONE: f64 = 0.12;

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

    /// The layout **as the mouse may use it** in this form: the window's
    /// geometry as laid out, but not drawable in `Never` — the layout does not
    /// know the form, and the strip of a bar that is never drawn must not take
    /// the pointer's presses from the grid.
    pub(crate) fn region(self, layout: ScrollbarLayout) -> ScrollbarLayout {
        if self == Mode::Never {
            ScrollbarLayout::default()
        } else {
            layout
        }
    }
}

/// The track's width in physical pixels — **the one conversion** of
/// [`TRACK_PT`]: the grid's reserve ([`Mode::reserve_px`]) and the drawn
/// strip ([`ScrollbarLayout::new`]) both come from here.
fn track_px(cell: CellMetrics) -> f32 {
    cell.pt_px(TRACK_PT)
}

/// The width of the strip the bar owns at the window's right edge, physical
/// pixels — the pointer's region: `bt-shell` sizes its tracking area from
/// here, then asks the drawn frame's layout ([`ScrollbarLayout::contains`])
/// whether a point is in the strip now. The track's one conversion
/// ([`track_px`]), so the region and the drawn track are the same pixels.
pub fn strip_px(cell: CellMetrics) -> f32 {
    track_px(cell)
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

    /// [`Mode::Auto`]'s look at visibility `alpha`, `wide` of the way to the
    /// wide form, the thumb at opacity `thumb`. A fully faded one is
    /// [`Look::HIDDEN`], so "hidden" has one representation and the step's
    /// comparison cannot see a change that draws nothing.
    pub(crate) fn auto(alpha: f32, wide: f32, thumb: f32) -> Look {
        if alpha > 0.0 {
            Look { alpha, wide, thumb }
        } else {
            Look::HIDDEN
        }
    }
}

/// A value easing linearly from one end to another over a span, absolute —
/// the width's and the tone's transitions. A new target starts from the
/// value on screen, so a reversal midway does not jump.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Ramp {
    from: f32,
    to: f32,
    /// When the transition started.
    at: f64,
    /// How long it runs, seconds; zero → at its target at once.
    span: f64,
}

impl Ramp {
    /// A ramp resting at `value`.
    fn rest(value: f32) -> Ramp {
        Ramp {
            from: value,
            to: value,
            at: 0.0,
            span: 0.0,
        }
    }

    /// The value at `now`.
    fn value(self, now: f64) -> f32 {
        if self.span <= 0.0 || now >= self.at + self.span {
            return self.to;
        }
        let progress = ((now - self.at) / self.span).clamp(0.0, 1.0) as f32;
        self.from + (self.to - self.from) * progress
    }

    /// When the value reaches its target, absolute.
    fn end(self) -> f64 {
        self.at + self.span.max(0.0)
    }

    /// Heads for `to` from the value at `now`, over `span`; a no-op if that
    /// is the target already.
    fn toward(&mut self, to: f32, now: f64, span: f64) {
        if self.to == to {
            return;
        }
        *self = Ramp {
            from: self.value(now),
            to,
            at: now,
            span,
        };
    }

    /// Whether the value is at its target at `now`.
    fn done(self, now: f64) -> bool {
        self.from == self.to || now >= self.end()
    }
}

/// The bar's visibility over time; `Copy`, kept in a `Cell` in the link
/// (blink's precedent).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Scrollbar {
    /// The form ([`Mode`]); only `Auto` has a timeline.
    mode: Mode,
    /// A poke no tick has stamped yet.
    poked: bool,
    /// When the bar began to appear, absolute; `None` → hidden.
    since: Option<f64>,
    /// The hold runs [`HOLD`] from here: the last poke's stamp, or the end
    /// of the narrowing once the pointer let the bar go.
    last: f64,
    /// The pointer is over the strip — what `bt-shell` said last; a tick
    /// stamps the change ([`Scrollbar::engaged`]).
    hover: bool,
    /// The thumb is held — what `bt-shell` said last.
    drag: bool,
    /// Whether the pointer engaged the bar as of the last step: the hover or
    /// the drag the tick has stamped. While engaged the bar neither holds nor
    /// fades.
    engaged: bool,
    /// The width, `0` thin `..=1` wide ([`Look::wide`]).
    width: Ramp,
    /// The thumb's opacity over the foreground ([`Look::thumb`]).
    tone: Ramp,
    /// A change of form not drawn yet: the next step reports a change even
    /// when the look is the same, so the frame that publishes the bar's
    /// region to the mouse ([`crate::Origin::scrollbar`]) follows the form.
    restyled: bool,
    /// The look the last step handed out — what [`Scrollbar::advance`]
    /// compares against to say whether this tick has anything new to draw.
    shown: Look,
}

impl Default for Scrollbar {
    /// Hidden, `Auto`, the pointer away — the transitions at rest at that
    /// form's targets, so a fresh bar is settled.
    fn default() -> Self {
        let mut bar = Scrollbar {
            mode: Mode::Auto,
            poked: false,
            since: None,
            last: 0.0,
            hover: false,
            drag: false,
            engaged: false,
            width: Ramp::default(),
            tone: Ramp::default(),
            restyled: false,
            shown: Look::HIDDEN,
        };
        bar.rest_pointer();
        bar
    }
}

impl Scrollbar {
    /// The form changed; `true` if it is a different one — the caller asks
    /// for a frame. The timeline starts over hidden: a bar leaving `Always`
    /// goes at once, one coming from it does not fade, and `Never` has no
    /// timeline at all. The pointer stays where it is: over the strip, the
    /// new form shows engaged.
    pub(crate) fn set_mode(&mut self, mode: Mode) -> bool {
        if self.mode == mode {
            return false;
        }
        self.mode = mode;
        self.poked = false;
        self.since = None;
        self.rest_pointer();
        self.restyled = true;
        true
    }

    /// The form.
    pub(crate) fn mode(self) -> Mode {
        self.mode
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

    /// The pointer came over the strip or left it; the next tick stamps the
    /// change. `true` → a frame is wanted. Where nothing can be drawn (no
    /// travel, the alternate screen, `Never`) the change is taken at once and
    /// wants nothing — no frames for a bar nobody sees.
    pub(crate) fn set_hover(&mut self, on: bool, drawable: bool) -> bool {
        let changed = std::mem::replace(&mut self.hover, on) != on;
        self.pointer_changed(changed, drawable)
    }

    /// The thumb was grabbed or let go — [`Scrollbar::set_hover`]'s rule. A
    /// grab engages the bar without a hover too (a press with no motion
    /// before it).
    pub(crate) fn set_drag(&mut self, on: bool, drawable: bool) -> bool {
        let changed = std::mem::replace(&mut self.drag, on) != on;
        self.pointer_changed(changed, drawable)
    }

    /// The common tail of the pointer's two setters.
    fn pointer_changed(&mut self, changed: bool, drawable: bool) -> bool {
        if !changed {
            return false;
        }
        if !drawable || self.mode == Mode::Never {
            self.rest_pointer();
            return false;
        }
        true
    }

    /// Takes the pointer's bits as they are, the transitions at rest — for a
    /// bar that is not drawn, where there is nothing to animate.
    fn rest_pointer(&mut self) {
        self.engaged = self.hover || self.drag;
        self.width = Ramp::rest(self.width_target());
        self.tone = Ramp::rest(self.tone_target());
    }

    /// The width the form and the pointer ask for.
    fn width_target(self) -> f32 {
        match self.mode {
            Mode::Auto if self.engaged => 1.0,
            Mode::Auto | Mode::Never => 0.0,
            Mode::Always => 1.0,
        }
    }

    /// The thumb's opacity the form and the pointer ask for.
    fn tone_target(self) -> f32 {
        if self.drag {
            DRAG_THUMB_ALPHA
        } else if self.hover {
            HOVER_THUMB_ALPHA
        } else if self.mode == Mode::Always {
            ALWAYS_THUMB_ALPHA
        } else {
            THUMB_ALPHA
        }
    }

    /// Advances the state to `now`; `true` if the look differs from the
    /// one handed out last — this tick has something to draw (blink's
    /// `advance` precedent: asked in the sleep question **before** settling,
    /// or the frame the bar settles in would never be drawn and a faint
    /// thumb would stay on screen).
    ///
    /// A poke **continues from the opacity on screen**: re-poked while
    /// fading, the bar climbs back from where it is instead of blinking out;
    /// the pointer coming over the strip shows the bar the same way. When the
    /// bar cannot be drawn it is hidden at once — no fade frames for a bar
    /// nobody sees; outside `Auto` there is no timeline to run.
    ///
    /// `instant` is Reduce Motion or `snap`: a widening that starts in this
    /// step takes no time.
    pub(crate) fn advance(&mut self, now: f64, drawable: bool, instant: bool) -> bool {
        let want = self.hover || self.drag;
        if !drawable || self.mode == Mode::Never {
            self.poked = false;
            self.since = None;
            self.rest_pointer();
        } else {
            // What is on screen, before this step changes anything: where a
            // rise starts from.
            let alpha = f64::from(self.fade(now));
            let edge = want != self.engaged;
            self.engaged = want;
            let auto = self.mode == Mode::Auto;
            // The pointer arriving shows the bar, and so does a bar that
            // became drawable under a pointer resting on its strip.
            let show = std::mem::take(&mut self.poked) || (want && (edge || self.since.is_none()));
            if auto && show {
                if self.since.is_none() {
                    // Appearing from nothing: in the form the pointer asks
                    // for — there is no width on screen to ease from.
                    self.width = Ramp::rest(self.width_target());
                    self.tone = Ramp::rest(self.tone_target());
                }
                self.since = Some(now - alpha * FADE_IN);
                self.last = now;
            }
            if edge {
                let span = if instant { 0.0 } else { WIDEN };
                self.width.toward(self.width_target(), now, span);
            }
            self.tone.toward(self.tone_target(), now, TONE);
            if auto {
                // Let go: the hold starts once the bar has narrowed.
                if edge && !want {
                    self.last = self.last.max(self.width.end());
                }
                if !want && self.since.is_some() && now >= self.last + HOLD + FADE_OUT {
                    self.since = None;
                }
                if self.since.is_none() {
                    // Gone, or never shown: nothing on screen to ease.
                    self.rest_pointer();
                }
            }
        }
        let look = if drawable {
            self.look(now)
        } else {
            Look::HIDDEN
        };
        let changed = std::mem::take(&mut self.restyled) | (look != self.shown);
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
    /// draws it again. The transitions end at their targets.
    pub(crate) fn hide(&mut self) {
        self.poked = false;
        self.since = None;
        self.rest_pointer();
    }

    /// What the bar looks like at `now` in its form ([`Look`]).
    pub(crate) fn look(self, now: f64) -> Look {
        match self.mode {
            Mode::Auto => Look::auto(self.fade(now), self.width.value(now), self.tone.value(now)),
            Mode::Always => Look {
                thumb: self.tone.value(now),
                ..Look::ALWAYS
            },
            Mode::Never => Look::HIDDEN,
        }
    }

    /// The bar's visibility at `now`, `0..=1`.
    #[cfg(test)]
    pub(crate) fn alpha(self, now: f64) -> f32 {
        self.look(now).alpha
    }

    /// `Auto`'s timeline at `now`, `0..=1`: the rise and the fall, whichever
    /// is lower — only the rise while the pointer engages the bar.
    fn fade(self, now: f64) -> f32 {
        let Some(since) = self.since else {
            return 0.0;
        };
        let rise = ((now - since) / FADE_IN).clamp(0.0, 1.0);
        if self.engaged {
            return rise as f32;
        }
        let fall = (1.0 - (now - (self.last + HOLD)) / FADE_OUT).clamp(0.0, 1.0);
        rise.min(fall) as f32
    }

    /// Whether the bar needs no frames at `now`: hidden, or fully up and
    /// holding or engaged, with its width and tone at rest — and always
    /// outside `Auto` once the tone rests. A pending poke, pointer change or
    /// form is not settled — its frame is on the way.
    pub(crate) fn settled(self, now: f64) -> bool {
        let pending = self.poked
            || self.restyled
            || self.engaged != (self.hover || self.drag)
            || self.tone.to != self.tone_target();
        if pending {
            return false;
        }
        if self.mode == Mode::Never {
            return true;
        }
        let at_rest = self.width.done(now) && self.tone.done(now);
        match (self.mode, self.since) {
            (Mode::Auto, Some(since)) => {
                at_rest && now >= since + FADE_IN && (self.engaged || now < self.last + HOLD)
            }
            _ => at_rest,
        }
    }

    /// The absolute time the bar next needs a frame from a sleeping link: the
    /// hold's end. `None` while hidden and while the pointer engages the bar
    /// — up with nothing to wait for; the stop condition. Only read at a
    /// sleep point, where the bar is settled; while it appears, changes or
    /// fades the link is awake and draws every tick anyway.
    pub(crate) fn next_deadline(self) -> Option<f64> {
        self.since
            .filter(|_| !self.engaged)
            .map(|_| self.last + HOLD)
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

    /// Whether a point — physical pixels from the window's top-left — is in
    /// the strip the bar owns, from the window's top to the dock's, while
    /// there is a bar to draw and grab: the pointer's region, where a press,
    /// a drag and a hover are the bar's and never the grid's.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        let [x0, y0, x1, y1] = self.strip;
        self.drawable() && (x0..x1).contains(&x) && (y0..y1).contains(&y)
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
    /// **The track's end is the bottom, not a row**: a place that rounds to
    /// the travel's last row answers [`f32::MAX`], past any travel, so the
    /// scroll lands at the bottom however much the history grew since this
    /// frame was drawn — with output streaming, the drawn travel's last row
    /// is already a few rows up, and a window left there would stop
    /// following the output. The top needs no such care: its distance from
    /// the top is zero whatever arrives below.
    ///
    /// A thumb that fills its track has no travel and answers the window's
    /// own place: there is nowhere to drag it.
    pub fn position_at(&self, y: f32, grab: f32) -> f32 {
        let travel = (self.track[1] - self.track[0]) - (self.thumb_y[1] - self.thumb_y[0]);
        if travel.is_nan() || travel <= 0.0 {
            return self.top;
        }
        let fraction = ((y - grab - self.track[0]) / travel).clamp(0.0, 1.0);
        let room = self.room as f32;
        let top = fraction * room;
        if top >= room - 0.5 { f32::MAX } else { top }
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
        bar.advance(0.0, true, false);
        assert_eq!(bar.alpha(0.0), 0.0);
        // Appearing: from zero to one over 120 ms, and not settled — the link
        // stays awake for it.
        assert!(bar.advance(0.06, true, false));
        assert!((bar.alpha(0.06) - 0.5).abs() < 1e-6);
        assert!(!bar.settled(0.06));
        assert!(
            bar.advance(FADE_IN, true, false),
            "the last rising step was not drawn"
        );
        assert_eq!(bar.alpha(FADE_IN), 1.0);
        // The hold is sleep: settled, and the clock is the hold's end.
        for now in [FADE_IN, 0.5, 0.99] {
            assert!(!bar.advance(now, true, false), "the hold changed at {now}");
            assert!(bar.settled(now), "the hold is not settled at {now}");
            assert_eq!(bar.next_deadline(), Some(HOLD));
        }
        // Fading: from the hold's end over 320 ms, awake again.
        assert!(!bar.settled(HOLD));
        assert!(bar.advance(HOLD + FADE_OUT / 2.0, true, false));
        assert!((bar.alpha(HOLD + FADE_OUT / 2.0) - 0.5).abs() < 1e-6);
        assert!(!bar.settled(HOLD + FADE_OUT / 2.0));
        // Gone: the last step is drawn (the change), then nothing is armed.
        assert!(bar.advance(HOLD + FADE_OUT, true, false));
        assert_eq!(bar.alpha(HOLD + FADE_OUT), 0.0);
        assert!(bar.settled(HOLD + FADE_OUT));
        assert_eq!(bar.next_deadline(), None, "a hidden bar armed a clock");
    }

    #[test]
    fn a_poke_in_the_hold_extends_it() {
        let mut bar = Scrollbar::default();
        bar.poke(true);
        bar.advance(0.0, true, false);
        bar.advance(0.5, true, false);
        bar.poke(true);
        assert!(
            !bar.advance(0.8, true, false),
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
        bar.advance(0.0, true, false);
        let fading = HOLD + FADE_OUT * 0.75;
        bar.advance(fading, true, false);
        let on_screen = bar.alpha(fading);
        assert!(on_screen > 0.0 && on_screen < 0.5, "{on_screen}");
        bar.poke(true);
        bar.advance(fading, true, false);
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
        bar.advance(0.0, true, false);
        bar.advance(0.5, true, false);
        assert!(
            bar.advance(0.6, false, false),
            "the vanished bar was not redrawn"
        );
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
        bar.advance(0.0, true, false);
        bar.advance(0.06, true, false);
        bar.poke(true);
        bar.hide();
        assert!(bar.settled(0.07), "a hidden bar is unsettled");
        assert_eq!(bar.next_deadline(), None);
        assert!(
            bar.advance(0.07, true, false),
            "the bar-less frame was not drawn"
        );
        assert_eq!(bar.alpha(0.07), 0.0);
    }

    #[test]
    fn a_long_sleep_lands_straight_on_hidden() {
        // The window was occluded through the hold: the first step after it
        // jumps to the end without playing the fade.
        let mut bar = Scrollbar::default();
        bar.poke(true);
        bar.advance(0.0, true, false);
        bar.advance(0.5, true, false);
        assert!(
            bar.advance(60.0, true, false),
            "the gone bar was not redrawn"
        );
        assert_eq!(bar.alpha(60.0), 0.0);
        assert!(bar.settled(60.0));
        assert_eq!(bar.next_deadline(), None);
        assert!(
            !bar.advance(60.1, true, false),
            "a hidden bar keeps changing"
        );
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
        for top in [0.0, 13.5, 50.0, 99.0] {
            let layout = ScrollbarLayout::new(position(100, top, 20), 400.0, 260.0, cell);
            let [_, y0, _, _] = layout.thumb(0.0);
            // Held 5 px below its top edge: the pointer is there.
            let back = layout.position_at(y0 + 5.0, 5.0);
            assert!((back - top).abs() < 1e-3, "{top} → {back}");
        }
        // Past the top the inverse clamps to it; the travel's last row and
        // anything past it is the bottom, beyond any travel — the history
        // may have grown since the frame was drawn.
        let layout = ScrollbarLayout::new(position(100, 50.0, 20), 400.0, 260.0, cell);
        assert_eq!(layout.position_at(-50.0, 0.0), 0.0);
        assert_eq!(layout.position_at(10_000.0, 0.0), f32::MAX);
        let bottom = ScrollbarLayout::new(position(100, 100.0, 20), 400.0, 260.0, cell);
        let [_, y0, _, _] = bottom.thumb(0.0);
        assert_eq!(bottom.position_at(y0 + 5.0, 5.0), f32::MAX);
        let near = ScrollbarLayout::new(position(100, 99.4, 20), 400.0, 260.0, cell);
        let [_, y0, _, _] = near.thumb(0.0);
        assert!(
            near.position_at(y0, 0.0) < 99.5,
            "a row above the end is the end"
        );
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
        assert!(bar.advance(0.0, true, false), "the form was not drawn");
        assert_eq!(bar.look(0.0), Look::ALWAYS);
        assert!(bar.settled(0.0));
        assert_eq!(bar.next_deadline(), None);
        assert!(
            !bar.poke(true),
            "a poke on an always-up bar asked for a frame"
        );
        for now in [0.5, HOLD + FADE_OUT, 60.0] {
            assert!(!bar.advance(now, true, false), "the bar changed at {now}");
            assert_eq!(bar.alpha(now), 1.0, "the bar faded at {now}");
            assert!(bar.settled(now));
            assert_eq!(bar.next_deadline(), None);
        }
        // Where it cannot be drawn (the alternate screen) it is gone, and
        // back when it can.
        assert!(bar.advance(61.0, false, false));
        assert!(bar.settled(61.0));
        assert!(bar.advance(62.0, true, false));
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
        assert!(!bar.settled(0.0), "the change of form is not on its way");
        // The change of form is drawn once — the frame that publishes the
        // bar's region to the mouse — and then nothing.
        assert!(
            bar.advance(0.0, true, false),
            "the change of form was not drawn"
        );
        assert!(!bar.advance(0.1, true, false), "a never-shown bar changed");
        assert_eq!(bar.look(0.1), Look::HIDDEN);
        assert!(bar.settled(0.1));
        assert_eq!(bar.next_deadline(), None);
        // The pointer over a strip that is never drawn wants nothing.
        assert!(
            !bar.set_hover(true, true),
            "a hover on `Never` asked for a frame"
        );
        assert!(!bar.set_drag(true, true));
        assert!(bar.settled(0.2));
        assert!(!bar.advance(0.2, true, false));
    }

    /// A bar shown by a poke and fully up at `FADE_IN`: thin, at the
    /// scrolling tone.
    fn shown_bar() -> Scrollbar {
        let mut bar = Scrollbar::default();
        bar.poke(true);
        bar.advance(0.0, true, false);
        bar.advance(FADE_IN, true, false);
        assert_eq!(bar.look(FADE_IN), Look::auto(1.0, 0.0, THUMB_ALPHA));
        bar
    }

    #[test]
    fn the_pointer_widens_the_bar_and_holds_it_without_a_clock() {
        let mut bar = shown_bar();
        let t = 0.5;
        assert!(bar.set_hover(true, true), "the hover asked for no frame");
        assert!(!bar.set_hover(true, true), "the same hover asked again");
        assert!(!bar.settled(t), "a pending hover is settled");
        // 150 ms to the wide form, 120 ms to the hover's tone; awake meanwhile.
        bar.advance(t, true, false);
        let half = t + WIDEN / 2.0;
        assert!(bar.advance(half, true, false));
        let look = bar.look(half);
        assert!((look.wide - 0.5).abs() < 1e-3, "{look:?}");
        assert!(!bar.settled(half));
        assert!(
            bar.advance(t + WIDEN, true, false),
            "the last widening step was not drawn"
        );
        assert_eq!(bar.look(t + WIDEN), Look::auto(1.0, 1.0, HOVER_THUMB_ALPHA));
        // Engaged: up, settled, no clock — however long the pointer stays.
        for now in [t + WIDEN, t + 2.0, t + 60.0] {
            assert!(
                !bar.advance(now, true, false),
                "the engaged bar changed at {now}"
            );
            assert!(bar.settled(now), "the engaged bar is unsettled at {now}");
            assert_eq!(bar.next_deadline(), None, "the engaged bar armed a clock");
            assert_eq!(bar.alpha(now), 1.0, "the engaged bar faded at {now}");
        }
    }

    #[test]
    fn letting_go_narrows_then_holds_then_fades() {
        let mut bar = shown_bar();
        bar.set_hover(true, true);
        bar.advance(0.5, true, false);
        bar.advance(1.0, true, false);
        // The pointer leaves at 2.0: narrowing, awake.
        assert!(bar.set_hover(false, true));
        bar.advance(2.0, true, false);
        assert!(!bar.settled(2.0 + WIDEN / 2.0), "the narrowing is settled");
        bar.advance(2.0 + WIDEN, true, false);
        assert_eq!(bar.look(2.0 + WIDEN), Look::auto(1.0, 0.0, THUMB_ALPHA));
        // Then the hold, asleep, a second from the end of the narrowing.
        assert!(bar.settled(2.0 + WIDEN));
        assert_eq!(bar.next_deadline(), Some(2.0 + WIDEN + HOLD));
        assert!(bar.settled(2.0 + WIDEN + HOLD - 0.01));
        // Then the fade, and nothing.
        let gone = 2.0 + WIDEN + HOLD + FADE_OUT;
        assert!(!bar.settled(2.0 + WIDEN + HOLD));
        assert!(bar.advance(gone, true, false));
        assert_eq!(bar.look(gone), Look::HIDDEN);
        assert!(bar.settled(gone));
        assert_eq!(bar.next_deadline(), None);
    }

    #[test]
    fn a_reversal_midway_eases_back_from_the_width_on_screen() {
        let mut bar = shown_bar();
        bar.set_hover(true, true);
        bar.advance(0.5, true, false);
        let mid = 0.5 + WIDEN * 0.4;
        bar.advance(mid, true, false);
        let on_screen = bar.look(mid).wide;
        bar.set_hover(false, true);
        bar.advance(mid, true, false);
        assert!(
            (bar.look(mid).wide - on_screen).abs() < 1e-6,
            "the width jumped on a reversal"
        );
        assert!(
            bar.look(mid + 0.01).wide < on_screen,
            "it did not narrow back"
        );
    }

    #[test]
    fn reduced_motion_widens_at_once_and_keeps_the_tone_and_the_fades() {
        let mut bar = shown_bar();
        bar.set_hover(true, true);
        assert!(bar.advance(0.5, true, true));
        let look = bar.look(0.5);
        assert_eq!(look.wide, 1.0, "the widening took time under Reduce Motion");
        assert!(
            look.thumb < HOVER_THUMB_ALPHA,
            "the tone did not ease: {look:?}"
        );
        assert!(!bar.settled(0.5), "the tone's transition is settled");
        bar.advance(0.5 + TONE, true, true);
        assert!(bar.settled(0.5 + TONE));
        // Leaving narrows at once too, then the hold and a real fade.
        bar.set_hover(false, true);
        bar.advance(1.0, true, true);
        assert_eq!(bar.look(1.0).wide, 0.0);
        assert_eq!(bar.next_deadline(), Some(1.0 + HOLD));
        let fading = 1.0 + HOLD + FADE_OUT / 2.0;
        bar.advance(fading, true, true);
        assert!((bar.alpha(fading) - 0.5).abs() < 1e-6, "the fade was cut");
    }

    #[test]
    fn the_thumb_darkens_over_the_strip_and_more_while_dragged() {
        let mut bar = shown_bar();
        let steps = [
            (true, false, HOVER_THUMB_ALPHA),
            (true, true, DRAG_THUMB_ALPHA),
            (false, true, DRAG_THUMB_ALPHA),
            (true, false, HOVER_THUMB_ALPHA),
            (false, false, THUMB_ALPHA),
        ];
        let mut now = 0.5;
        for (hover, drag, tone) in steps {
            bar.set_hover(hover, true);
            bar.set_drag(drag, true);
            bar.advance(now, true, false);
            now += TONE;
            bar.advance(now, true, false);
            assert_eq!(bar.look(now).thumb, tone, "hover {hover}, drag {drag}");
            now += 0.2;
        }
        // A grab with no hover before it — a press with no motion — engages
        // the bar too.
        let mut bar = shown_bar();
        assert!(bar.set_drag(true, true));
        bar.advance(0.5, true, false);
        bar.advance(0.5 + WIDEN, true, false);
        assert_eq!(
            bar.look(0.5 + WIDEN),
            Look::auto(1.0, 1.0, DRAG_THUMB_ALPHA)
        );
        assert_eq!(bar.next_deadline(), None);
    }

    #[test]
    fn the_pointer_shows_a_hidden_bar_wide() {
        let mut bar = Scrollbar::default();
        assert!(bar.set_hover(true, true));
        bar.advance(0.0, true, false);
        assert_eq!(bar.alpha(0.0), 0.0);
        assert!(bar.advance(FADE_IN / 2.0, true, false));
        let look = bar.look(FADE_IN / 2.0);
        assert_eq!(
            (look.wide, look.thumb),
            (1.0, HOVER_THUMB_ALPHA),
            "{look:?}"
        );
        bar.advance(FADE_IN, true, false);
        assert!(bar.settled(FADE_IN));
        assert_eq!(bar.next_deadline(), None);
    }

    #[test]
    fn the_pointer_over_an_undrawable_bar_wants_no_frame() {
        // No travel or the alternate screen: the change is taken at once and
        // nothing is in flight; once the bar can be drawn under the resting
        // pointer, it shows.
        let mut bar = Scrollbar::default();
        assert!(
            !bar.set_hover(true, false),
            "an undrawable bar asked for a frame"
        );
        assert!(bar.settled(0.0));
        assert!(!bar.advance(0.0, false, false));
        assert_eq!(bar.next_deadline(), None);
        bar.advance(0.1, true, false);
        assert!(
            !bar.settled(0.1),
            "the bar under the pointer did not start showing"
        );
        bar.advance(0.1 + FADE_IN, true, false);
        assert_eq!(
            bar.look(0.1 + FADE_IN),
            Look::auto(1.0, 1.0, HOVER_THUMB_ALPHA)
        );
    }

    #[test]
    fn always_darkens_under_the_pointer_and_leaving_arms_nothing() {
        let mut bar = Scrollbar::default();
        bar.set_mode(Mode::Always);
        bar.advance(0.0, true, false);
        assert!(bar.set_hover(true, true));
        bar.advance(1.0, true, false);
        assert!(
            !bar.settled(1.0 + TONE / 2.0),
            "the tone's transition is settled"
        );
        bar.advance(1.0 + TONE, true, false);
        assert_eq!(
            bar.look(1.0 + TONE),
            Look {
                thumb: HOVER_THUMB_ALPHA,
                ..Look::ALWAYS
            }
        );
        assert!(bar.settled(1.0 + TONE));
        assert_eq!(bar.next_deadline(), None);
        // Leaving: back to the quiet tone, then settled with no clock — the
        // always-up form has no hold to wait for.
        assert!(bar.set_hover(false, true));
        bar.advance(2.0, true, false);
        assert!(!bar.settled(2.0));
        bar.advance(2.0 + TONE, true, false);
        assert_eq!(bar.look(2.0 + TONE), Look::ALWAYS);
        assert!(bar.settled(2.0 + TONE));
        assert_eq!(
            bar.next_deadline(),
            None,
            "leaving the always-up bar armed a clock"
        );
        assert!(!bar.advance(30.0, true, false));
    }

    #[test]
    fn a_new_form_starts_from_hidden_at_once() {
        // A bar holding in `Auto` that becomes `Never` goes in the next step,
        // without a fade and without a clock.
        let mut bar = Scrollbar::default();
        bar.poke(true);
        bar.advance(0.0, true, false);
        bar.advance(0.5, true, false);
        assert!(bar.set_mode(Mode::Never));
        assert!(
            bar.advance(0.6, true, false),
            "the vanished bar was not redrawn"
        );
        assert_eq!(bar.look(0.6), Look::HIDDEN);
        assert_eq!(bar.next_deadline(), None);
        // `Always` back to `Auto`: hidden until the next scroll, not a fade.
        bar.set_mode(Mode::Always);
        bar.advance(1.0, true, false);
        bar.set_mode(Mode::Auto);
        assert!(bar.advance(1.1, true, false));
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
    fn the_strip_is_the_pointers_region_while_there_is_a_bar() {
        // A 400×300 window, the dock's top at 260: the strip is the right
        // 16 px from the top to the dock, the region's width `strip_px`.
        let cell = at_1x();
        let layout = ScrollbarLayout::new(position(100, 50.0, 20), 400.0, 260.0, cell);
        let x0 = 400.0 - strip_px(cell);
        assert_eq!(layout.strip_x(), x0);
        assert!(layout.contains(x0, 0.0));
        assert!(layout.contains(399.5, 259.5));
        assert!(!layout.contains(x0 - 0.5, 100.0), "left of the strip");
        assert!(!layout.contains(390.0, 260.0), "the dock is not the bar's");
        assert!(!layout.contains(400.0, 100.0), "past the window's edge");
        // Nothing to scroll: no region at all.
        assert!(!ScrollbarLayout::new(None, 400.0, 260.0, cell).contains(395.0, 100.0));
    }

    #[test]
    fn a_bar_that_is_never_drawn_has_no_region() {
        let layout = ScrollbarLayout::new(position(100, 50.0, 20), 400.0, 260.0, at_1x());
        assert!(Mode::Auto.region(layout).contains(395.0, 100.0));
        assert!(Mode::Always.region(layout).contains(395.0, 100.0));
        assert!(
            !Mode::Never.region(layout).contains(395.0, 100.0),
            "the strip of a bar never drawn takes the grid's presses"
        );
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
