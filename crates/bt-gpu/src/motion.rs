//! The cursor's and the content's glide — **pure**, no ObjC, no locks.
//!
//! Its precedent is `Gate` and `FailureStreak`: the policy itself is
//! platform-independent, so it lives in a separate type and is tested without a
//! real window. Buried in `link.rs` it could only be tried on screen.
//!
//! **Three animators, one type** ([`Motion`]): the cursor ([`State`], two axes),
//! the offset's slide and the notch's glide (both [`Slide`], one axis).
//! They were not split into separate types because the link's sleep decision is
//! single: `motion.settled()` (the "no damage" branch of `link.rs`). An animator
//! left **outside** that gate would let the link sleep mid-slide and the content
//! would freeze — the visible symptom of the animation would be "stopped halfway".
//!
//! **The glide draws something different from the others**
//! ([`Motion::request_glide`]): its position goes not to the screen but to the
//! `Session`. Its unit is "rows to deliver" and its per-frame **share** (the
//! change in position) is `Session::frame`'s argument; the window scrolls by
//! that share, so the glide is the animation of scrolling, not of the offset.
//!
//! **The two share the physics, not the mode.** The constants, the cubic
//! easing and the closed form of the spring are common ([`ease_axis`],
//! [`spring_axis`], [`axis_settled`]); under Reduce Motion the cursor
//! **fades in** while the offset **snaps** ([`Motion::origin_mode`]). The
//! reason is the reduction itself: the whole screen fading in on every new
//! line would be worse than the motion it tries to remove.
//!
//! **The cursor's target is a screen row**, not a grid row ([`Motion::sync`]):
//! on Enter the grid row goes `r → r+1` while the offset drops by one, so the
//! screen row does not change at all. If the two targets were not in the same
//! space, the cursor would drop a row and climb back.
//!
//! **State is in cell units**, not pixels: when the font, zoom or
//! screen scale changes, the pixel equivalent of the position moves but the cell
//! coordinate stays the same, so a change of measure lands in the right place by
//! itself.
//!
//! **Every animation carries a stop condition** and here it is
//! two-layered: a position+velocity threshold **or** a time ceiling. The second
//! is a belt — a parameter set that never satisfies the first (extremely low
//! damping, endless oscillation) would keep the link awake forever.
//!
//! **Three styles, one state machine** ([`bt_core::CursorMotion`]): `Snap`
//! never starts the animation (so no motion frame is born either), `Ease` has a
//! fixed duration and is structurally overshoot-free, `Spring` is a critically
//! damped spring. The style comes from the settings, already **resolved**:
//! `bt-shell-macos` reads the file, this module only knows the physics. The
//! durations and coefficients are **chosen** numbers, not measured; all of them
//! are at the top of this file, with their docs.
//!
//! **Reduce Motion is a fourth mode** ([`Mode::Fade`]), not a fourth style: the
//! style and the `reduce` flag are combined in [`Motion::mode`] and the
//! reduction lives in **one place**. When on there is no slide — the cursor
//! **fades in** in its new cell for [`FADE_DURATION`]. The flag also arrives
//! resolved: `bt-shell-macos` combines the three-valued `reduce_motion` with the
//! system's answer, because `bt-gpu` does not see AppKit.

use bt_core::{CursorMotion, Erase, Keypress, ScrollGlide};

/// Spring stiffness, rad/s. **A chosen number, not a measured one.**
///
/// In critical damping (ζ = 1) a one-cell slide settles in ~230 ms at this
/// value (the binding threshold is [`VEL_EPSILON`], not the position).
/// The number rests on that target by eye, not on a measurement.
///
/// **The settling time grows with distance**, and this is a direct consequence
/// of the thresholds being **absolute**: in a jump of `D` cells the time grows
/// with `ln(D)` — 1 cell ~230 ms, 200 cells ~430 ms, 400 cells ~460 ms. The
/// difference matters, not the ratio: there is no fixed "settling time" and
/// [`TIME_CEILING`]'s margin has to carry that. (The numbers were **computed**
/// from the closed form, not measured; measuring needs a real window and this is
/// not a gate.)
const OMEGA: f32 = 30.0;

/// Position threshold for counting as settled, **cells**. It is enough for it to
/// stay under half a pixel: a typical cell is 8–20 pixels, so `0.02` cells ≤ 0.4
/// pixels.
const POS_EPSILON: f32 = 0.02;

/// Velocity threshold for counting as settled, **cells/second**. It has to be
/// asked **together with** the position: passing right over the target the
/// position difference momentarily nears zero, and a threshold that looked at the
/// position alone would stop the animation in the middle.
const VEL_EPSILON: f32 = 0.2;

/// Time ceiling, seconds — **a belt, not a measured duration**.
///
/// It is what keeps the animation finite in every case where the threshold path
/// is blocked (a parameter change, a pathological rhythm where `dt` never
/// advances). If it fires the cursor jumps to the target, so the symptom is a
/// visible jump, not an endless stream of frames.
///
/// **The margin's operand is the longest legitimate jump, not the "typical"
/// slide.** The first number was `0.5` and was justified as twice [`OMEGA`]'s
/// ~230 ms for one cell; but since the thresholds are absolute, a 400-cell jump
/// (a line-start return on a wide screen) settles in ~460 ms — so the old
/// ceiling was **10% away from cutting** a legitimate slide and the symptom
/// would be a visible snap at the end of the slide. `0.7` is ~50% above that
/// computation. The numbers were computed from the closed form ([`OMEGA`]'s
/// doc), not measured.
const TIME_CEILING: f32 = 0.7;

/// Upper bound of `dt`, seconds.
///
/// When occlusion lifts (or the system throttles the link) the gap between two
/// stamps can be unbounded. Unclamped, the time ceiling fires **after the wild
/// frame instead of preventing it**: `elapsed` exceeds the ceiling in a single
/// step and the animation ends without ever being seen. The value sits between
/// two frames (16 ms at 120 Hz, 33 ms at 60 Hz) and a blink of an eye; again
/// chosen.
pub(crate) const DT_MAX: f32 = 0.1;

/// Slide duration of the `ease` style, seconds — **a chosen number, not
/// measured.**
///
/// A little under [`OMEGA`]'s ~230 ms for one cell: what sets `ease` apart is
/// that its duration is **independent of distance**, i.e. faster than the spring
/// on a far jump and close to it on a near one. The value has to stay under
/// [`TIME_CEILING`], otherwise the belt would cut a legitimate `ease` slide
/// (`ease_settles_well_inside_the_ceiling`).
const EASE_DURATION: f32 = 0.18;

// `ease`'s stop condition is its own clock, so the time ceiling's belt is **not
// applied** to it; the order of the two numbers is therefore a requirement, not
// a comment sentence. A future change that lowers the ceiling below `ease`
// blows up here, not in a slide cut on screen.
const _: () = assert!(EASE_DURATION < TIME_CEILING);

/// How long a run of screenful scrolls still counts as one burst, seconds
/// ([`Motion::scroll_in`]) — derived, not measured.
///
/// It is the slide's own length: [`EASE_DURATION`] ends the `ease` slide, and
/// at that point the spring has `(1 + ωt)·e^(−ωt) ≈ 0.029` of its distance
/// left (ωt = [`OMEGA`] × 0.18 = 5.4), under one row of a 30-row screen —
/// computed from the closed form, not measured. A screenful that arrives
/// later lands on a slide that is visually over, so output is still pouring;
/// the reads of one short burst arrive within a few frames of each other.
const BURST_WINDOW: f32 = EASE_DURATION;

// The "pause" criterion of the fade-in **leans on** `dt`'s clamping
// (`Motion::sync`): the first frame after the link sleeps is counted as
// `DT_MAX`, and for that to count as a pause the clamp must be longer than the
// fade. Were it reversed, a cursor waking from sleep would never fade in and the
// symptom would be as insidious as "sometimes it doesn't appear".
const _: () = assert!(FADE_DURATION < DT_MAX);

/// Duration of the fade-in when Reduce Motion is on, seconds — **a chosen
/// number, not measured.**
///
/// Apple's Reduce Motion guidance puts a **fade** in place of the slide; here
/// the reduction is not "no animation" but "a short animation that does not
/// change place". 90 ms is under a blink of an eye: long enough to announce the
/// change, short enough not to read as motion.
///
/// **Smaller than [`DT_MAX`] (100 ms), and that was left so on purpose.** A
/// single wild frame (after occlusion, when the system throttles the link) can
/// finish the fade in one step, i.e. it passes unseen. Occlusion's own path
/// already wants this ([`Motion::finish`]); in the remaining case the cost is a
/// fade that goes unseen once, the gain is that `dt`'s clamp stays a single
/// number. Whoever lowers `DT_MAX` should weigh their own reason, not the
/// relation here.
const FADE_DURATION: f32 = 0.09;

/// The effective motion mode: the **one place** where the user's style and
/// Reduce Motion combine ([`Motion::mode`]).
///
/// Not a copy of [`bt_core::CursorMotion`], but a branch on top of it: the
/// setting stays three-valued (the user does not pick "which style when Reduce
/// Motion is on"), the decision is made here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Snap,
    Ease,
    Spring,
    /// Reduce Motion: the position is at the target at once, what changes is the
    /// opacity.
    Fade,
}

/// State of the cursor's slide.
///
/// The inside of the `Option` means "there is an in-flight cursor"; `None` is
/// both the first frame and an invisible cursor. A flag separating the two is
/// **deliberately absent**: the thing to do is the same in both (sit at the
/// next target at once) and two flags would be two truths that can drift apart.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Motion {
    /// The style the user picked ([`Motion::set_style`]).
    ///
    /// Its default is **not here**: `Default` is derived and takes its value from
    /// [`bt_core::CursorMotion`]'s `Default`. Had a `Spring` been written here, the
    /// default would have gained a second owner that could silently drift from the
    /// settings model's — the hermetic timed run takes exactly that value (the
    /// `motion > 0` gate leans on it).
    style: CursorMotion,
    /// Whether Reduce Motion is on — the **resolved** value ([`Motion::set_reduce`]).
    ///
    /// `bt_core::ReduceMotion`'s three values are not here: the answer to "follow the
    /// system" comes from `NSWorkspace` and the layer asking that question is
    /// `bt-shell-macos`. Moving the trio here would make `bt-gpu` a customer of the
    /// system's accessibility setting, not of the settings file.
    ///
    /// `Default` is `false`: the hermetic timed run does not read the system setting
    /// (`bt-shell-macos`'s `Inputs::Hermetic`) and `make smoke`'s `motion > 0`
    /// requirement cannot be tied to the measuring machine's accessibility setting.
    reduce: bool,
    state: Option<State>,
    /// The offset's slide; `None` → no content frame yet.
    ///
    /// The same `Option` contract as the cursor's and for the same reason: absence
    /// and the first frame both mean "sit at the next target at once".
    origin: Option<Slide>,
    /// Age, in seconds, of the current run of content frames that each
    /// scrolled a screen or more ([`Motion::scroll_in`]); `None` once a
    /// content frame scrolls less than a screen.
    ///
    /// A run younger than [`BURST_WINDOW`] is one burst arriving in several
    /// PTY reads and keeps sliding; an older one is a stream. It is a clock,
    /// not a frame count: how many frames a burst is split into is up to the
    /// reader and varies from run to run. The slide's own state cannot tell
    /// the two apart either: the stream branch finishes the slide, so the next
    /// screenful frame finds it at rest.
    screenful_run: Option<f32>,
    /// The scroll offset seen in the last frame; `None` → no frame yet.
    ///
    /// Kept separate because even when `state` empties (an invisible cursor) the
    /// offset's history must not be lost: a TUI hides the cursor and scrolls the
    /// window, then turns it back on.
    offset: Option<i32>,
    /// The notch's glide: [`Slide`]'s second instance, its unit **rows to deliver**
    /// ([`Motion::request_glide`]).
    ///
    /// Not an `Option`: the offset's `None` means "first frame, sit at the target",
    /// while the glide's resting state is a position sitting at zero — with no
    /// request there is no road to travel. The position is **re-anchored to zero**
    /// at every delivery ([`Motion::take_glide`]), so at rest `pos == target == 0`
    /// and the numbers do not grow over a session and eat `f32`'s precision.
    glide: Slide,
    /// The scroll generation the glide belongs to (`bt_core::ScrollGlide`).
    ///
    /// The share goes to `Session::frame` with this generation; when the position is
    /// reset from outside (return to bottom on input, Shift+PgUp) the generation
    /// rises and the in-flight glide is **dropped**
    /// ([`Motion::observe_scroll_generation`]).
    glide_generation: u32,
    /// The scroll position of the last content frame, `(display_offset, fraction)` —
    /// the only witness that sees the glide hit the end ([`Motion::observe_scroll`]).
    ///
    /// An **identity**, not a position, like [`Motion::offset`]: it is compared, not
    /// used as a number.
    glide_at: Option<(i32, f32)>,
    /// The dock band's **extra** rows: how much more than the PTY's reserved share
    /// (`DOCK_ROWS`) the drawn band is, in rows — [`Slide`]'s fourth instance.
    ///
    /// A separate animator and not derived from the offset: the offset's input is
    /// the content's fill and the band's is the dock's row count, and the two can go
    /// in different directions in the same frame (`scroll_in` would have falsely
    /// shrunk the band on content that jumps in a single frame). They meet in the
    /// drawing: the grid's drawn origin is `origin() − band()` (`set_origin` in
    /// `link.rs`), so the grid's bottom edge and the band's top edge slide together
    /// **structurally**.
    ///
    /// **Both directions glide** ([`Motion::sync_band`]): the band's size is a panel's
    /// size, not content — the content offset's "narrowing content snaps" reason (a
    /// descent reads like falling) does not fit here, and the band vanishing while the
    /// grid jumps when a line is deleted would be exactly the jump that rule wants to
    /// prevent.
    ///
    /// An `Option`, the same contract as the offset's: absence and the first frame
    /// both mean "sit at the next target at once".
    band: Option<Slide>,
}

#[derive(Clone, Copy, Debug)]
struct State {
    /// In cell units `(column, row)`; it need not be an integer.
    pos: [f32; 2],
    /// Cells/second. **Always zero** in `ease`: that style's position is a function
    /// of time, not the integral of a velocity.
    vel: [f32; 2],
    /// Where the slide started — only `ease`'s operand.
    ///
    /// The spring carries velocity so it needs no history; `ease` **recomputes** the
    /// position between `from → target` from `elapsed`, so it cannot forget its
    /// starting point. When the target changes mid-flight (or the style changes)
    /// this is pulled to the current position: otherwise the cursor would restart
    /// from the old start, i.e. jump back.
    from: [f32; 2],
    target: [f32; 2],
    /// Time since the target was set; the time ceiling's **and** `ease`'s progress
    /// operand.
    elapsed: f32,
    /// Time since the last **target change** — only [`Mode::Fade`]'s operand, and
    /// not a copy of `elapsed`: that one is the fade's own clock; this is the gap
    /// **between** two movements.
    ///
    /// The fade's "does it restart" question looks at this (`Motion::sync`): a move
    /// after a pause is a new fade, not the cursor that streaming output moves every
    /// frame.
    since_move: f32,
}

/// State of the offset's slide — and of the notch's glide: [`State`]'s
/// **single-axis** sibling.
///
/// A separate type, not `State` with its second axis left empty: an unused axis
/// would be a lie told by the type, and `since_move` (the fade's clock) never
/// enters here — the offset does not fade ([`Motion::origin_mode`]). What is
/// shared is the **physics**: [`ease_axis`], [`spring_axis`] and
/// [`axis_settled`] are under both.
#[derive(Clone, Copy, Debug, Default)]
struct Slide {
    /// In rows; it need not be an integer. The side that converts to pixels and
    /// **rounds to the device grid** is `Frame::set_origin_rows`.
    pos: f32,
    /// Rows/second. Always zero in `ease` (same reason as [`State::vel`]).
    vel: f32,
    /// Where the slide started — `ease`'s operand and both modes' "no road to
    /// travel" condition.
    from: f32,
    target: f32,
    elapsed: f32,
}

impl Motion {
    /// Where the style and Reduce Motion come down to a single decision.
    ///
    /// **`Snap` sits above the flag** and this is a product decision: a user who
    /// says `cursor_motion = "snap"` has already turned motion off, and Reduce Motion
    /// should not *add* a fade to them. The accessibility setting shortens the
    /// animation, it does not create one that does not exist (`docs/SETTINGS.md` →
    /// `[motion]`).
    fn mode(self) -> Mode {
        match (self.style, self.reduce) {
            (CursorMotion::Snap, _) => Mode::Snap,
            (_, true) => Mode::Fade,
            (CursorMotion::Ease, false) => Mode::Ease,
            (CursorMotion::Spring, false) => Mode::Spring,
        }
    }

    /// Reduction of the dock's typing effects: the two effects the user picked
    /// → the ones to be drawn in this mode.
    ///
    /// **It lives here** because the rule is the same as [`Motion::mode`]'s and the
    /// reduction's one place is this module: `snap` is the declaration
    /// of one who has already turned motion off, and it turns both off; Reduce
    /// Motion lowers the arrival to the cursor's own mode — a fade-in — and turns
    /// the ghost off, because the ghost is content that is not there. The
    /// accessibility setting **does not add** animation: an arrival that is off
    /// stays off.
    pub(crate) fn glyph_fx(self, keypress: Keypress, erase: Erase) -> (Keypress, Erase) {
        match (self.style, self.reduce) {
            (CursorMotion::Snap, _) => (Keypress::Off, Erase::Off),
            (_, true) if keypress == Keypress::Off => (Keypress::Off, Erase::Off),
            (_, true) => (Keypress::Fade, Erase::Off),
            _ => (keypress, erase),
        }
    }

    /// The offset's mode: the same as [`Motion::mode`], **except the fade**.
    ///
    /// Under Reduce Motion the offset does not slide but **does not fade either**, it
    /// snaps. [`Mode::Fade`] means "the position is at the target at once,
    /// what changes is the opacity" and opacity is nobody's field here: what slides
    /// is the whole grid and fading it in on every new line would be worse than the
    /// motion the reduction tries to remove.
    ///
    /// The rule "the reduction's one place is `bt-gpu::motion`" stays
    /// in place: the **place** is the same, the **mode** is two.
    fn origin_mode(self) -> Mode {
        match self.mode() {
            Mode::Fade => Mode::Snap,
            mode => mode,
        }
    }

    /// The content frame's tip: a new cursor has arrived.
    ///
    /// All **four** snap cases are here, in a single expression:
    ///
    /// - **first frame** — there is no `state`,
    /// - **a cursor that opens while invisible** — invisibility emptied `state`,
    /// - **scrolling in history** — the offset moved,
    /// - **geometry** (window, font, zoom) — the caller says `geometry`.
    ///
    /// The common reason: in none of them did the cursor move, the **grid under it**
    /// moved. Inventing an animation would show the cursor as coming from a place it
    /// was not.
    ///
    /// **Retargeting to the same target is a no-op** and this is a requirement:
    /// content frames that do not move the cursor (a colour change, text on a lower
    /// line) can arrive dozens of times a second and had each of them reset the time
    /// ceiling, the ceiling would never fill — the belt itself would snap.
    ///
    /// **`CursorMotion::Snap` is not a fifth snap case, it is above all of them:**
    /// in that style every `sync` sits at once, so the animation never starts and
    /// `settled()` is never `false` — no motion frame is born either.
    ///
    /// **In the fade mode the position also sits at once**, only the opacity is
    /// animated: if `pos` were not pulled to the target here, the cursor would be
    /// drawn at alpha zero in its **old** cell in the first content frame and would
    /// jump to its place only when the fade ended — a defect no counter sees,
    /// because the motion frames and frames are in the right number.
    ///
    /// **The target `at` is in screen cells and the caller computes it.** It used to
    /// be that the signature took `(col, row, origin_rows)` and summed them here; now
    /// the caret has **two homes** (the grid and the dock) and the dock's row is not
    /// an integer — the dock starts lower by the breathing margin, so the target is
    /// fractional. The single animator has two homes because if
    /// the caret glides while typing in the grid it must glide in the dock too and
    /// between the two — not three separate behaviours but one movement.
    ///
    /// `None` → no caret in either home (a program hiding the cursor, no dock and an
    /// invisible cursor); the state drops.
    ///
    /// **The offset is set before `visible` and unconditionally.** A program hiding
    /// the cursor (not vim, a script running with `tput civis`) keeps streaming
    /// output and freezing the offset would leave the content in the wrong place —
    /// the cursor's visibility cannot decide where the grid stands.
    ///
    /// **`band_target` is the dock band's extra-row target** ([`Motion::band`]):
    /// fractional and signed — in a remote session the band is shorter than the PTY
    /// share and the target is negative. There is no direction rule, it glides
    /// in both directions. If the band's target changed in this frame the offset's
    /// direction rule is relaxed for that frame too — `filled`'s sibling, but the
    /// bit does not come from the caller, it is born here: this type is the only
    /// place that knows the band's history. The reason is a pit: when the band
    /// shrinks and the content's target rises (rows return to the grid) the two
    /// must cancel each other; had the rising target snapped, the grid would jump in
    /// a frame while the band glided.
    pub(crate) fn sync(
        &mut self,
        at: Option<[f32; 2]>,
        origin_rows: u16,
        band_target: f32,
        offset: i32,
        geometry: bool,
        filled: bool,
    ) {
        let scrolled = self.offset != Some(offset);
        self.offset = Some(offset);
        let band_changed = self.band.is_some_and(|band| band.target != band_target);
        self.sync_band(band_target, geometry);
        // The offset's snap trigger **covers** the cursor's: the wheel and
        // geometry snap both, and the offset also has its own direction gate
        // ([`Motion::sync_origin`]). The item "the grid moving for another reason
        // does not slide" in `docs/SETTINGS.md` is these two triggers.
        //
        // **`filled` is not a third trigger**, it is the direction gate's exception: it
        // does not set the snap, it relaxes the direction rule while `snap` is clear
        // ([`Motion::sync_origin`]). The bit is computed in the caller (`link.rs`:
        // `cursor.fill > 0`) — `bt-gpu` does not learn terminal semantics, it is the
        // same class as `offset` and `geometry`.
        self.sync_origin(
            f32::from(origin_rows),
            scrolled || geometry,
            filled || band_changed,
        );
        let Some(target) = at else {
            self.state = None;
            return;
        };
        // Read **before** the guard: reading a second field under the borrow of
        // `self.state` is not accepted in a match guard.
        let mode = self.mode();
        let animated = mode != Mode::Snap;
        match &mut self.state {
            Some(state) if animated && !scrolled && !geometry => {
                if state.target != target {
                    // **The fade restarts after a pause, not on every
                    // frame** (a review finding). In the two sliding
                    // styles resetting the clock unconditionally is right: a new
                    // target means a new road. In the fade the clock drives
                    // the **opacity**, not the road, and an unconditional reset
                    // turned Reduce Motion upside down: in streaming output every content
                    // frame moved the target, so alpha was nailed to zero and the
                    // cursor was **never visible**. "Reset only when settled"
                    // is no cure either — in that case the cursor fades in again every
                    // 90 ms, i.e. it **blinks** at ~11 Hz; a flicker is
                    // worse than the motion we are trying to remove, and inside the
                    // accessibility setting at that.
                    //
                    // The discriminating criterion is the gap between movements: a movement
                    // after a pause of `FADE_DURATION` is a **separate**
                    // movement and deserves a fade; a target change that comes more
                    // often is part of a single stream and does not
                    // refresh the opacity. The fade's own duration is used as the
                    // threshold — a second constant would have asked for a second reason.
                    let resumed = state.since_move >= FADE_DURATION;
                    state.since_move = 0.0;
                    state.from = state.pos;
                    state.target = target;
                    if mode != Mode::Fade || resumed {
                        state.elapsed = 0.0;
                    }
                    if mode == Mode::Fade {
                        state.pos = target;
                    }
                }
            }
            // Snap: both the in-flight state and absence land in the same place.
            state => {
                *state = Some(State {
                    pos: target,
                    vel: [0.0; 2],
                    from: target,
                    target,
                    elapsed: 0.0,
                    since_move: 0.0,
                });
            }
        }
    }

    /// Scrolled `rows` rows into history from the top of the screen: the offset is
    /// taken down that much **from where it stands** and glides to its target again.
    ///
    /// A movement that [`Motion::sync_origin`] cannot see. Its input is the fill
    /// (`rows - content_rows`) and once the grid is full the fill is fixed: new
    /// lines scroll the content **inside** the cells, the target does not move and
    /// the animator sees nothing — the slide existed until the grid filled, then did
    /// not (the user reported). What the screen sees is the same, though: the
    /// content flowed upward. To draw the same movement with the same animator the
    /// offset's **position** is taken back by the scrolled amount — the content
    /// starts from where it stood in the previous frame — and the target is left
    /// untouched.
    ///
    /// It obeys the direction rule ([`Motion::sync_origin`]): the slide goes from
    /// bottom to top, i.e. content *arriving*. The scrolled lines close the strip
    /// that opens at the top (`bt_core::Cursor::fill`).
    ///
    /// **Order:** after [`Motion::advance`], before [`Motion::sync`]. Not before,
    /// because the elapsed time must be applied to the old path; not after, because
    /// `sync`'s snap triggers (wheel, geometry) must erase this frame's scroll too —
    /// shrinking the window pushes lines into history and that is not a scroll. If
    /// the target is also dropping in the same frame (the grid filled in this
    /// frame) `sync` takes the slide over from the position set up here.
    ///
    /// **The ceiling is `limit` rows** (the caller gives the grid's height): in
    /// streaming output the scrolls pile on top of each other and `ease` takes only
    /// a fraction of the road at each new start, so an offset without a ceiling
    /// would fall screens behind. The ceiling does not pull the position **down**,
    /// it only stops pushing further.
    ///
    /// **If a single frame scrolled `limit` rows or more there are two branches and
    /// what tells them apart is time** ([`Motion::screenful_run`],
    /// [`BURST_WINDOW`]). A short burst (`seq 1 200` on a full screen, `ls -la`)
    /// arrives from the PTY in one, two or three reads, i.e. in that many content
    /// frames, and the number varies from run to run: screenful frames inside the
    /// window fall to the ordinary branch — the position is clamped to the ceiling,
    /// the slide does not end and the last screen glides from a full screen below.
    /// If screenful frames continue past the window the output is **streaming** and
    /// the slide is ended — the second half of the ceiling. Measured
    /// (`BT_SCROLL_TEST`, 45–467 rows per frame): without this branch the offset
    /// hung at the ceiling and the window showed the newest output **a screen
    /// behind** throughout the stream. Tying the distinction to the frame count
    /// ("the second big frame is a stream" or "a screenful frame that arrives while
    /// the slide is in flight is a stream") cut a burst that arrives in pieces at
    /// random: the first `ls -la` glided, the ones after it did not (the user
    /// reported, 2026-09-30). Its cost: the first [`BURST_WINDOW`] of a stream glides
    /// at the ceiling like a burst.
    ///
    /// The stream is **remembered**, not re-derived from the slide: the ending
    /// branch leaves the slide settled and in a continuous stream every second
    /// screenful frame rebuilt the burst — the grid was drawn a screen lower, the
    /// fill band grew by a screen (measured 2026-09-30: release
    /// `cpu_encode` p95 0.23 → 0.33). A content frame that scrolls less than a
    /// screen (zero included) ends the run, so the next burst glides again. **Known
    /// limit:** screenful frames separated by a gap with no such frame between them
    /// (`while sleep 1; do seq 200; done`) glide once and are then counted as a
    /// stream — it was so under the previous rule too.
    ///
    /// In the fade and `Snap` modes nothing: the offset does not slide in those
    /// modes anyway ([`Motion::origin_mode`]). Nothing in the first frame either —
    /// there is no position to slide.
    pub(crate) fn scroll_in(&mut self, rows: u16, limit: u16) {
        // Recorded before any early return: only content frames reach this
        // call, so a frame that scrolls less than a screen (including zero
        // rows) is the only thing that ends a run.
        let stream = if rows >= limit {
            let age = *self.screenful_run.get_or_insert(0.0);
            age >= BURST_WINDOW
        } else {
            self.screenful_run = None;
            false
        };
        if rows == 0 || self.origin_mode() == Mode::Snap {
            return;
        }
        if let Some(slide) = &mut self.origin {
            if stream {
                slide.pos = slide.target;
                slide.vel = 0.0;
                slide.from = slide.pos;
                slide.elapsed = 0.0;
                return;
            }
            // A burst, whole or in pieces: on a resting grid this lands
            // exactly a screen low, on a sliding one it continues at the cap.
            let cap = slide.target + f32::from(limit);
            slide.pos = (slide.pos + f32::from(rows)).min(cap).max(slide.pos);
            slide.from = slide.pos;
            slide.elapsed = 0.0;
        }
    }

    /// The offset's target: the single-axis half of [`Motion::sync`].
    ///
    /// The snap cases are **the same class** as the cursor's and for the same
    /// reason: the first frame, the wheel and geometry. In all three the content did
    /// not rise by its own growth — the grid moved for another reason and inventing
    /// an animation would show it as coming from a place it did not come from.
    ///
    /// **The fourth is the offset's own: direction.** Only a **falling** target
    /// slides, a rising one snaps. The offset is `rows - content_rows`, so the
    /// target falling is the content **growing** (the grid flows up), rising is it
    /// **shrinking** (the grid comes down). Flowing up reads as content *arriving*
    /// and is pleasant; coming down reads as *falling* and is odd — the shell
    /// gliding down as it leaves vim, `clear` on a full screen dropping the prompt
    /// from top to bottom. The rule therefore looks at the **sign**, not the
    /// distance: a threshold would be an unmeasured number, direction is free (an
    /// eye check).
    ///
    /// This has a cost and it is named: a program that writes and deletes lines one
    /// after another (a spinner) glides as it grows and jumps as it shrinks. A
    /// sawtooth instead of a symmetric oscillation; it was accepted in the eye
    /// check, because the only alternative was that unmeasured threshold.
    ///
    /// **The direction rule's named exception is `filled`**: if the gap
    /// above fills with the ledger's newest rows, what comes down is not a gap but
    /// **history arriving** from above — the "reads as falling" reason is moot in
    /// that branch. So the rule is not lifted, it is **narrowed**: with `fill == 0`
    /// shrinking content still snaps — a window with no dock, a deliberately cleared
    /// screen (Ctrl-L, `clear` on a full screen) and a window scrolled into history.
    ///
    /// **Leaving the alternate screen is not in this list and once was written in**
    /// (measured 2026-09-20): because `vim`'s entry `2J` does not set the
    /// flag all four gates are open in the exit frame and `fill` comes
    /// greater than zero — i.e. the offset **starts gliding** in that frame. What
    /// brings the snap is not `fill`, it is the `geometry` flag that the resize
    /// which restores the dock will plant in the **next** main-queue turn; the slide
    /// therefore lasts a frame in practice. Its direction is also defensible —
    /// history really does pass into the gap above — but it is **not designed**, so
    /// it is written by name: either `bt-core` should set a gate in the exit frame or
    /// this sentence should endorse the decision. The `filled = false` in the test
    /// below measures not that branch but **the branch itself** (`fill == 0`).
    ///
    /// **The term is inside `!snap`** and this is not a placement taste: written
    /// outside it Rust's precedence would make the expression
    /// `(animated && !snap && …) || filled`, i.e. with a fill the wheel and geometry
    /// would slide too — `bt-core`'s `display_offset == 0` gate cuts the
    /// wheel but does **not** cut the **geometry** branch. The same mistake would
    /// also skip `animated` and puncture `cursor_motion = "snap"` and Reduce Motion
    /// in the fill. Its guards are `scrolling_and_geometry_snap_the_origin` and
    /// `snap_style_never_slides_the_origin`.
    ///
    /// **Known limit:** `snap` today is `scrolled || geometry` and `fill` is already
    /// zero while `display_offset != 0`, so `filled` and `scrolled` normally cannot
    /// both be true in the same frame — the one exception is the wheel landing on
    /// the frame in which `fill` is computed. The guard's `!snap` swallows it (that
    /// frame snaps) and the error's direction is safe.
    ///
    /// **Retargeting to the same target is a no-op** (the same requirement as
    /// [`Motion::sync`]): frames that do not grow the content (a colour change,
    /// in-line typing) arrive dozens of times a second and had each of them reset
    /// `elapsed`, the time ceiling would never fill.
    ///
    /// **Its input is not monotonic** (`bt_core::Cursor::content_rows`): a program
    /// that moves the cursor up and erases the bottom line with `\e[K` can narrow and
    /// widen the target. The stop condition copes with this, because it looks not at
    /// the target but at the **distance to the target**: every new target restarts
    /// the slide from where it stands and each is finite on its own
    /// ([`Slide::settled`]). If the oscillation itself goes on forever the link stays
    /// awake — but what asks for those frames is not the animation but the
    /// **damage of the output** producing the oscillation.
    fn sync_origin(&mut self, target: f32, snap: bool, filled: bool) {
        let mode = self.origin_mode();
        let animated = mode != Mode::Snap;
        match &mut self.origin {
            // `<=`, not `<`: an **equal** target goes in no direction and must fall into
            // the no-op inside. Written `<`, every frame whose target does not change would
            // enter the snap branch, rebuild the in-flight slide on every frame and kill the
            // animation altogether.
            Some(slide) if animated && !snap && (target <= slide.target || filled) => {
                if slide.target != target {
                    slide.from = slide.pos;
                    slide.target = target;
                    slide.elapsed = 0.0;
                }
            }
            slide => {
                *slide = Some(Slide {
                    pos: target,
                    vel: 0.0,
                    from: target,
                    target,
                    elapsed: 0.0,
                });
            }
        }
    }

    /// The band's extra-row target: [`Motion::sync_origin`]'s sibling, **without the
    /// direction rule**.
    ///
    /// Snap cases: the first frame, geometry (window, font, point size — the band's
    /// pixel size has already changed and gliding from the old size would invent a
    /// movement that is not there) and the offset mode's snap (`cursor_motion =
    /// "snap"`, Reduce Motion: [`Motion::origin_mode`]). The wheel **does not
    /// snap**: scrolling into history does not change the dock's row count, so the
    /// band's target does not move in that frame anyway.
    ///
    /// Retargeting to the same target is a no-op (the same requirement as the
    /// offset): every key is a content frame and must not reset the time ceiling.
    fn sync_band(&mut self, target: f32, snap: bool) {
        let animated = self.origin_mode() != Mode::Snap;
        match &mut self.band {
            Some(slide) if animated && !snap => {
                if slide.target != target {
                    slide.from = slide.pos;
                    slide.target = target;
                    slide.elapsed = 0.0;
                }
            }
            slide => {
                *slide = Some(Slide {
                    pos: target,
                    vel: 0.0,
                    from: target,
                    target,
                    elapsed: 0.0,
                });
            }
        }
    }

    /// The user changed the style (the settings file was saved).
    ///
    /// Return: **whether an in-flight slide was ended in this call**. The caller has
    /// to know this, because the link's "no damage" branch sleeps without drawing on
    /// a settled animation — otherwise the cursor of a user who switched to `Snap`
    /// would hang in an intermediate cell and only an unrelated content frame would
    /// put it in place (the same pattern as `Renderer::set_font`'s "did it change"
    /// return; a save rewriting the same style is a no-op).
    ///
    /// **No teleport.** `Snap` ends the in-flight slide **at its target**
    /// ([`Motion::finish`]); the other two styles take the slide over from where it
    /// stands — `from` is pulled to the current position and `elapsed` to zero.
    /// Without taking over, `ease` would restart from the old start point, i.e. the
    /// cursor would jump back; the spring keeps its velocity so it is fine anyway,
    /// but writing two separate rules for two styles gains nothing.
    ///
    /// **The "in flight?" question must be asked with the old style** and this is
    /// not an ordering subtlety, it is the teleport itself: the stop condition
    /// depends on the style ([`State::settled`]), so if the style is written first a
    /// spring that has flown longer than 180 ms looks "settled" by `ease`'s clock,
    /// the handover is skipped and the next `advance` throws the cursor to the target
    /// with `t = 1` — **without drawing**, because the link counts that frame as
    /// settled and sleeps. The defect fixed in `snap` returned by this road
    /// (`a_long_spring_flight_does_not_teleport_when_the_style_changes`).
    /// **In the fade mode `ease` ↔ `spring` is nothing.** There is no slide anyway
    /// and no position to take over; running the takeover branch regardless would
    /// pull `from` to the current position and `from == target` would show the fade
    /// as **settled** — the link would sleep without drawing that frame and the
    /// cursor would hang half transparent (`style_change_during_a_fade_keeps_fading`).
    pub(crate) fn set_style(&mut self, style: CursorMotion) -> bool {
        if self.style == style {
            return false;
        }
        let in_flight = !self.settled();
        let was_fading = self.mode() == Mode::Fade;
        self.style = style;
        if !in_flight {
            return false;
        }
        if style == CursorMotion::Snap {
            self.finish();
            return true;
        }
        if was_fading {
            return false;
        }
        if let Some(state) = &mut self.state {
            state.from = state.pos;
            state.elapsed = 0.0;
        }
        // The offset takes over as well, for the same reason: `ease` remembers its
        // start point and if it were not refreshed the grid would jump back to its old
        // start.
        if let Some(slide) = &mut self.origin {
            slide.from = slide.pos;
            slide.elapsed = 0.0;
        }
        // The band too, for the same reason.
        if let Some(slide) = &mut self.band {
            slide.from = slide.pos;
            slide.elapsed = 0.0;
        }
        self.glide.from = self.glide.pos;
        self.glide.elapsed = 0.0;
        false
    }

    /// Reduce Motion was turned on or off (system setting or
    /// `[motion] reduce_motion`).
    ///
    /// The return has the same contract as [`Motion::set_style`]: **did this call
    /// turn an unsettled state into a settled one**. The caller has to know this,
    /// because the link's "no damage" branch sleeps without drawing on a settled
    /// animation.
    ///
    /// **Both directions end the in-flight one** and this has to be a harsher rule
    /// than the style change — taking over has no meaning in either direction:
    ///
    /// - **On turning on** what would be taken over is a slide and the mode no
    ///   longer slides.
    /// - **On turning off** what would be taken over is a fade and `ease` would
    ///   take it for a position animation: `from` stands in the previous cell, so
    ///   the cursor would return to the cell it came to and slide again
    ///   (`reduce_off_mid_fade_does_not_slide_backwards`). The `spring` also settles
    ///   at once because it is at its target with no velocity, so it would leave a
    ///   half-transparent cursor on screen.
    pub(crate) fn set_reduce(&mut self, reduce: bool) -> bool {
        if self.reduce == reduce {
            return false;
        }
        let in_flight = !self.settled();
        self.reduce = reduce;
        if !in_flight {
            return false;
        }
        self.finish();
        true
    }

    /// Advances the physics by `dt` seconds. `dt` is clamped **here**, not in the
    /// caller: the clamp is part of this module's stop condition and cannot be left
    /// to the caller to remember.
    ///
    /// At the moment settling is decided the position is set **exactly to the
    /// target**. Left to the threshold, the cursor would rest `POS_EPSILON` cells
    /// off and the block's rectangle would part sub-pixel from the glyph under it —
    /// no counter sees it, the eye does.
    pub(crate) fn advance(&mut self, dt: f32) {
        let dt = dt.clamp(0.0, DT_MAX);
        if let Some(age) = &mut self.screenful_run {
            *age += dt;
        }
        self.advance_origin(dt);
        self.advance_glide(dt);
        // The band has **the same physics and the same mode** as the offset: both are
        // the grid's displacement and Reduce Motion snaps both.
        let mode = self.origin_mode();
        if let Some(band) = &mut self.band {
            band.advance(mode, dt);
        }
        let mode = self.mode();
        let Some(state) = &mut self.state else {
            return;
        };
        state.elapsed += dt;
        state.since_move += dt;
        match mode {
            // In both, `sync` already sat at the target; there is no position to advance.
            // In `Fade` what advances is `elapsed` itself, because the opacity is its
            // function ([`Motion::alpha`]).
            Mode::Snap | Mode::Fade => {}
            Mode::Ease => state.ease(),
            Mode::Spring => state.spring(dt),
        }
        if state.settled(mode) {
            state.pos = state.target;
            state.vel = [0.0; 2];
            // `from` is also pulled to the target, so the settling branch leaves **the same**
            // state as [`Motion::finish`]. Had it not (a review finding) a slide that
            // settled early by the spring's threshold would not satisfy `ease`'s and
            // `fade`'s "no road to travel" condition (`from == target`), i.e. it would look
            // unsettled when the mode changed — and the link would already have counted that
            // frame as settled and **gone to sleep**, so `advance` would never run again.
            state.from = state.target;
        }
    }

    /// The offset half of [`Motion::advance`]; `dt` arrives **clamped** in the caller
    /// (one clamp, one rule).
    fn advance_origin(&mut self, dt: f32) {
        let mode = self.origin_mode();
        if let Some(slide) = &mut self.origin {
            slide.advance(mode, dt);
        }
    }

    /// The glide half of [`Motion::advance`]; **the same physics and the same mode**
    /// as the offset ([`Motion::origin_mode`]): both are the grid's displacement and
    /// Reduce Motion snaps both.
    fn advance_glide(&mut self, dt: f32) {
        let mode = self.origin_mode();
        let glide = &mut self.glide;
        glide.elapsed += dt;
        match mode {
            Mode::Ease => glide.ease(),
            Mode::Spring => glide.spring(dt),
            // In `Snap` the request was already delivered at once
            // ([`Motion::request_glide`]); `Fade` never comes from
            // [`Motion::origin_mode`].
            Mode::Snap | Mode::Fade => {}
        }
        if glide.settled(mode) {
            // The same triple as the other two animators: the position **exactly** at the
            // target. Here it is also a contract — had the remainder at the target not been
            // delivered, the sum of the shares would fall short of the request by
            // `POS_EPSILON` and every notch would leave the window at a fraction of a row.
            glide.pos = glide.target;
            glide.vel = 0.0;
            glide.from = glide.target;
        }
    }

    /// The scroll's **glide request** arrived (`Session::take_scroll_glide`):
    /// `request.rows` more rows are to be delivered.
    ///
    /// The generation is asked first ([`Motion::observe_scroll_generation`]): if the
    /// request belongs to a new generation the in-flight glide carries the old
    /// position's share and is dropped, the request starts from zero.
    ///
    /// **It is added to the in-flight glide**, not started afresh: the target grows,
    /// the slide is re-set from where it stands (`from`, `elapsed`) and the spring
    /// keeps its velocity — notches arriving one after another are a single stream.
    /// The rule of retargeting like the offset's ([`Motion::sync_origin`]).
    ///
    /// **The link calls this after `advance`** and the order is a requirement: the
    /// first `dt` of a link waking from sleep is a fabrication clamped to
    /// [`DT_MAX`] and if applied to the new request half the notch would go in a
    /// single frame in `ease`. The first share is therefore zero, the glide starts in
    /// the next frame — the cursor's and the offset's "elapsed time first, then the
    /// new target" rule.
    ///
    /// In the `Snap` mode (`cursor_motion = "snap"`, Reduce Motion) the request is
    /// delivered **at once**: a notch does not add animation for a user who has
    /// turned motion off.
    pub(crate) fn request_glide(&mut self, request: ScrollGlide) {
        self.observe_scroll_generation(request.generation);
        if request.rows == 0.0 {
            return;
        }
        let snap = self.origin_mode() == Mode::Snap;
        let glide = &mut self.glide;
        glide.target += request.rows;
        glide.from = glide.pos;
        glide.elapsed = 0.0;
        if snap {
            glide.pos = glide.target;
            glide.from = glide.target;
            glide.vel = 0.0;
        }
    }

    /// This frame's **share**: the road the glide has covered since the last
    /// delivery, in rows — `Session::frame`'s argument, with the generation it
    /// belongs to.
    ///
    /// The position is re-anchored to zero here (`from`, `target` and `pos` are
    /// shifted by the same amount): both the cubic easing and the spring's closed
    /// form are invariant under translation, so the slide does not move, only the
    /// numbers stay small. A side gain is the definition of settling: in a resting
    /// glide all three are exactly zero ([`Motion::glide_idle`]).
    pub(crate) fn take_glide(&mut self) -> ScrollGlide {
        let glide = &mut self.glide;
        let rows = glide.pos;
        glide.from -= rows;
        glide.target -= rows;
        glide.pos = 0.0;
        ScrollGlide {
            rows,
            generation: self.glide_generation,
        }
    }

    /// The scroll's current generation; if the glide belongs to another generation
    /// it is **dropped**.
    ///
    /// The remaining share is not delivered, and this is the opposite of finishing
    /// ([`Motion::finish`]), on purpose: what raises the generation is an input that
    /// resets the position from outside (return to bottom, Shift+PgUp, a line step)
    /// or the scroll becoming invalid (`CSI 3 J`, the alternate screen) — that place
    /// is where it wants to go, and the remaining share would pull the window back
    /// from there. The link asks it twice: when taking the request and after
    /// `frame()`, because `frame()` too can raise the generation.
    ///
    /// The link's only call after `frame()` is [`Motion::observe_scroll`]; this is
    /// the half common to it and to [`Motion::request_glide`].
    pub(crate) fn observe_scroll_generation(&mut self, generation: u32) {
        if generation != self.glide_generation {
            self.glide = Slide::default();
            self.glide_generation = generation;
        }
    }

    /// `frame()`'s answer: the scroll's generation and the position the share left.
    ///
    /// The generation goes to [`Motion::observe_scroll_generation`] — `frame()`
    /// raises the generation itself when it finds the fraction invalid. **The
    /// position is the second question**: if the share is non-zero and the position
    /// did not move, the window has hit the end of history (`bt-core` clamps) and
    /// the glide **ends**. If it did not end, the remaining share asked for beyond the
    /// end would have frames that change nothing drawn until it settled, and it would
    /// also swallow the first notch coming in the opposite direction — after a wheel
    /// flung down at the bottom an upward notch did nothing. A
    /// notch in the opposite direction is a separate request anyway, so ending only
    /// drops the unreachable remainder.
    ///
    /// If the end of a whole row is overshot **partially** (share `0.3`, `0.1` left
    /// to the end) that frame moves the position and the glide ends in the next
    /// frame.
    pub(crate) fn observe_scroll(&mut self, generation: u32, at: (i32, f32), share: f32) {
        self.observe_scroll_generation(generation);
        let before = self.glide_at.replace(at);
        if share != 0.0 && before == Some(at) {
            self.glide = Slide::default();
        }
    }

    /// Is the glide resting: no road, **and no undelivered share either**.
    ///
    /// The two questions together, because a glide that was finished
    /// ([`Motion::finish`]) may have settled at its target but not yet given its
    /// share, and that share can only be delivered in a **content** frame. The link
    /// therefore skips the "no damage" branch by looking at it; while it is `false` a
    /// content frame is drawn, not a motion frame.
    pub(crate) fn glide_idle(&self) -> bool {
        self.glide.settled(self.origin_mode()) && self.glide.pos == 0.0
    }

    /// **Ends** the in-flight slide **at its target** — for the moments the
    /// animation cannot advance.
    ///
    /// Today's callers are an occluded window and the setting switching to `snap`.
    /// When occluded the link stops, so `advance` would never run again and the
    /// state would stay "unsettled" forever. The cost would be twofold — the timed
    /// run would say `MotionUnsettled` at the deadline and go red **while the code is
    /// right**, and the diagnosis would point at the wrong place as "a stop
    /// condition is broken"; and when occlusion lifted the cursor would come sliding
    /// from a point the user never saw.
    ///
    /// What the snap policy already says: visibility return is without
    /// animation. Here only the same rule is applied a frame early.
    pub(crate) fn finish(&mut self) {
        if let Some(state) = &mut self.state {
            state.pos = state.target;
            state.from = state.target;
            state.vel = [0.0; 2];
            state.elapsed = 0.0;
        }
        // **Both end together.** Had half been left, the link would say "unsettled" and
        // stay awake, and in an occluded window the hole [`Motion::finish`] wants to
        // close would stay open.
        // The band too: had half been left, the same hole.
        for slide in [&mut self.origin, &mut self.band].into_iter().flatten() {
            slide.pos = slide.target;
            slide.from = slide.target;
            slide.vel = 0.0;
            slide.elapsed = 0.0;
        }
        // **The glide also ends at its target and delivers its share**, it is not
        // dropped: had it been dropped the window would rest in the middle of a row. The
        // share goes in the next content frame and every path that ends it already asks
        // for a frame (`DisplayLink::set_visible`, `set_cursor_motion`,
        // `set_reduce_motion`); until then [`Motion::glide_idle`] is `false`. The
        // rule for a generation change is the opposite of this
        // ([`Motion::observe_scroll_generation`]).
        let glide = &mut self.glide;
        glide.pos = glide.target;
        glide.from = glide.target;
        glide.vel = 0.0;
        glide.elapsed = 0.0;
    }

    /// Where the cursor will be drawn this frame, in **screen cells** — not grid
    /// cells ([`Motion::sync`]). With no state there is no target: the caller does
    /// not draw an invisible cursor anyway.
    pub(crate) fn position(&self) -> Option<[f32; 2]> {
        self.state.map(|state| state.pos)
    }

    /// The cursor's opacity in this frame; **always `1.0`** outside the fade.
    ///
    /// Both the block and the text colour under it are multiplied by this
    /// (`Frame::push_caret`): had the two been separated the letter would be painted
    /// in the colour of a block that is not yet visible — a letter in the ground's
    /// colour on the ground, i.e. an unreadable cell.
    ///
    /// A settled fade gives `1.0`, not `elapsed / FADE_DURATION`: the cases with no
    /// road to travel ([`Motion::finish`], the snap cases) are settled while
    /// `elapsed` is zero and the ratio would make them invisible.
    pub(crate) fn alpha(&self) -> f32 {
        if self.mode() != Mode::Fade {
            return 1.0;
        }
        self.state.map_or(1.0, |state| {
            if state.settled(Mode::Fade) {
                1.0
            } else {
                (state.elapsed / FADE_DURATION).clamp(0.0, 1.0)
            }
        })
    }

    /// The offset at which the content will stand in this frame, in **rows**. With no
    /// state `0.0`: a placement stuck to the ceiling, i.e. the value `Frame::clear`
    /// leaves.
    pub(crate) fn origin(&self) -> f32 {
        self.origin.map_or(0.0, |slide| slide.pos)
    }

    /// The dock band's **extra** rows in this frame ([`Motion::band`]); with no
    /// state `0.0` — a band as big as the PTY's reserved share.
    pub(crate) fn band(&self) -> f32 {
        self.band.map_or(0.0, |slide| slide.pos)
    }

    /// Have **all four** animations stopped — the link's "may I sleep" question.
    ///
    /// The offset has to be **inside** this gate: left outside, the link would
    /// sleep mid-slide in the "no damage" branch and the content would freeze halfway.
    /// The glide is here for the same reason, together with its undelivered share
    /// ([`Motion::glide_idle`]). The band too: left outside, the link would
    /// sleep in the middle of the band's growth and the grid and the band would freeze
    /// halfway. The timed run's gate (`Verdict::MotionUnsettled`) reads this too.
    pub(crate) fn settled(&self) -> bool {
        self.cursor_settled() && self.origin_settled() && self.glide_idle() && self.band_settled()
    }

    /// Only whether the band's glide has stopped. It has no token (`slide=` is the
    /// offset's witness and its meaning does not change); `settled()`'s fourth term.
    fn band_settled(&self) -> bool {
        let mode = self.origin_mode();
        self.band.is_none_or(|slide| slide.settled(mode))
    }

    /// Is Reduce Motion on — the blink's gate ([`crate::blink`]).
    ///
    /// The **raw flag**, not [`Motion::mode`]: in the `Snap` style `mode()` does not
    /// return `Fade` but the reduction is still on and blink must still be off. The
    /// accessibility setting **does not add** animation.
    pub(crate) fn reduce(&self) -> bool {
        self.reduce
    }

    /// Only whether the cursor's animation has stopped — the witness of the
    /// `motion=` token.
    ///
    /// `true` when there is no state: with no cursor to draw there is nothing to
    /// wait for.
    pub(crate) fn cursor_settled(&self) -> bool {
        let mode = self.mode();
        self.state.is_none_or(|state| state.settled(mode))
    }

    /// Only whether the offset's slide has stopped — the witness of the `slide=`
    /// token.
    ///
    /// The two can be asked separately, because the token counts them separately: the
    /// one reading a red run must see from the line which animator did not settle.
    pub(crate) fn origin_settled(&self) -> bool {
        let mode = self.origin_mode();
        self.origin.is_none_or(|slide| slide.settled(mode))
    }
}

impl State {
    /// `ease`: the position is a **function of time** — a cubic easing from `from` to
    /// `target` (`1 − (1−t)³`).
    ///
    /// This is where overshoot is structurally impossible: the expression is
    /// monotonic for `t ∈ [0,1]` and does not exceed `1`, so the overshoot clamp the
    /// spring requires is never needed in this style. The velocity is not integrated
    /// either (`vel` stays zero); the price paid is remembering `from`.
    fn ease(&mut self) {
        let t = (self.elapsed / EASE_DURATION).clamp(0.0, 1.0);
        for axis in 0..2 {
            self.pos[axis] = ease_axis(self.from[axis], self.target[axis], t);
        }
    }

    /// `spring`: one step of a critically damped spring.
    fn spring(&mut self, dt: f32) {
        for axis in 0..2 {
            let (pos, vel) = spring_axis(self.pos[axis], self.vel[axis], self.target[axis], dt);
            self.pos[axis] = pos;
            self.vel[axis] = vel;
        }
    }

    /// The stop condition; **two separate questions** depending on the style.
    ///
    /// `ease`'s is the clock and this is its definition: the slide lasts
    /// [`EASE_DURATION`] whatever the distance. Had it been tied to the threshold the
    /// duration would be silently tied to the distance (in cubic easing the remaining
    /// distance drops under the threshold early on a short jump and late on a long
    /// one), so the style's name would lie.
    ///
    /// The fade's is also a clock and of **the same shape** as `ease`'s: settled if
    /// the time is up or there is no road to travel. They are in one branch because
    /// the question is one — only the constant changes.
    ///
    /// The spring's is two-layered: position+velocity threshold **or** time ceiling.
    /// `snap` goes through the threshold — because `sync` already sat it at the
    /// target, `true` at the first question.
    fn settled(&self, mode: Mode) -> bool {
        if let Mode::Ease | Mode::Fade = mode {
            let duration = if let Mode::Fade = mode {
                FADE_DURATION
            } else {
                EASE_DURATION
            };
            // "The clock is up" **or** there is no road to travel. The second condition is
            // a requirement: an instantly seated cursor (the snap cases,
            // [`Motion::finish`]) puts `from` at the target too and a rule looking at the
            // clock alone would count it as "unsettled" for [`EASE_DURATION`] — the link
            // would draw frames that change nothing, and at every `sync` afresh.
            return self.elapsed >= duration || self.from == self.target;
        }
        // Time ceiling **or** threshold; either is enough on its own.
        self.elapsed >= TIME_CEILING
            || (0..2).all(|axis| axis_settled(self.pos[axis], self.vel[axis], self.target[axis]))
    }
}

impl Slide {
    /// Advances the physics by `dt` (clamped) seconds — the step common to the
    /// offset and the band.
    fn advance(&mut self, mode: Mode, dt: f32) {
        self.elapsed += dt;
        match mode {
            Mode::Ease => self.ease(),
            Mode::Spring => self.spring(dt),
            // In `Snap` `sync` already sat at the target. `Fade` **never comes** here:
            // [`Motion::origin_mode`] turns it into `Snap` and neither the offset nor the
            // band fades.
            Mode::Snap | Mode::Fade => {}
        }
        if self.settled(mode) {
            // **The same triple** as the cursor's and for the same reason
            // ([`Motion::advance`]): position exactly at the target, velocity to zero, `from`
            // also at the target — otherwise a slide that settled early would not satisfy
            // `ease`'s "no road to travel" condition and would look unsettled when the
            // mode changed.
            self.pos = self.target;
            self.vel = 0.0;
            self.from = self.target;
        }
    }

    /// [`State::ease`]'in tek eksenli hâli.
    fn ease(&mut self) {
        let t = (self.elapsed / EASE_DURATION).clamp(0.0, 1.0);
        self.pos = ease_axis(self.from, self.target, t);
    }

    /// [`State::spring`]'in tek eksenli hâli.
    fn spring(&mut self, dt: f32) {
        let (pos, vel) = spring_axis(self.pos, self.vel, self.target, dt);
        self.pos = pos;
        self.vel = vel;
    }

    /// **The same two questions** as [`State::settled`], without the fade branch:
    /// [`Motion::origin_mode`] does not produce `Fade`.
    ///
    /// Because its input is not monotonic ([`Motion::sync_origin`]) the stop
    /// condition looks not at the target but at the **distance**: every time the
    /// target moves the slide restarts from where it stands and each is finite on
    /// its own.
    fn settled(&self, mode: Mode) -> bool {
        if let Mode::Ease = mode {
            return self.elapsed >= EASE_DURATION || self.from == self.target;
        }
        self.elapsed >= TIME_CEILING || axis_settled(self.pos, self.vel, self.target)
    }
}

/// The single axis of the cubic easing: `t ∈ [0,1]` between `from → target`.
///
/// Where overshoot is **structurally** impossible: `1 − (1−t)³` is monotonic and
/// does not exceed `1`, so the clamp the spring requires is never needed in this
/// style.
fn ease_axis(from: f32, target: f32, t: f32) -> f32 {
    let eased = 1.0 - (1.0 - t).powi(3);
    from + (target - from) * eased
}

/// One step of the critically damped spring, single axis — **overshoot clamp
/// included**.
///
/// ζ = 1 means "no overshoot from rest"; [`Motion::sync`] however **deliberately**
/// keeps the velocity in flight (momentum) and that velocity can carry past the
/// target: the expression `(d + c·t)e^{-ωt}` crosses zero when
/// `|v| > OMEGA × remaining distance`. The worst case measured was a small target
/// correction made in the middle of a long jump, **0.87 cells** — almost a full
/// cell, i.e. a visible recoil. The motion design forbids this by name ("near critical
/// damping, **no overshoot**"), so the axis that crosses the target is stopped at
/// the target.
///
/// It is not a hard stop: at the moment the clamp fires the cursor is already
/// exactly over the target, so what is seen is "arrived and stopped".
///
/// **One place, two animators:** the offset pays the same clamp
/// ([`Slide::spring`]). A second copy would silently bring back the recoil of a
/// grid that overshoots its target.
fn spring_axis(pos: f32, vel: f32, target: f32, dt: f32) -> (f32, f32) {
    let before = pos - target;
    let (after, vel) = critically_damped(before, vel, dt);
    if before != 0.0 && (before < 0.0) != (after < 0.0) {
        (target, 0.0)
    } else {
        (target + after, vel)
    }
}

/// The threshold path, single axis: position **and** velocity together.
///
/// They have to be asked together — passing right over the target the position
/// difference momentarily nears zero and a threshold looking at the position
/// alone would stop the animation in the middle ([`VEL_EPSILON`]).
fn axis_settled(pos: f32, vel: f32, target: f32) -> bool {
    (pos - target).abs() <= POS_EPSILON && vel.abs() <= VEL_EPSILON
}

/// The **closed form** of the critically damped spring: given the position `d`
/// relative to the target and the velocity `v`, what they are after `dt` seconds.
///
/// Not an Euler step, because its stability would depend on `dt`: a single wild
/// frame with `OMEGA * dt > 2` (after occlusion) would diverge and `DT_MAX` would
/// then be not a belt but a **requirement**. The closed form is right for any
/// `dt`; `dt` is clamped anyway but for another reason (the time ceiling, see
/// [`DT_MAX`]).
///
/// ζ = 1 was chosen: **no overshoot**. Any value below it would throw the cursor
/// past the target and bring it back, and the design explicitly does not want that
/// ("near critical damping, no overshoot").
fn critically_damped(d: f32, v: f32, dt: f32) -> (f32, f32) {
    let c = v + OMEGA * d;
    let decay = (-OMEGA * dt).exp();
    let pos = (d + c * dt) * decay;
    let vel = (c - OMEGA * (d + c * dt)) * decay;
    (pos, vel)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 120 Hz frame; the tests' common step.
    const TICK: f32 = 1.0 / 120.0;

    /// A cursor in flight from `(0,0)` to `(10,4)`, **with the given style**.
    fn moving_with(style: CursorMotion) -> Motion {
        let mut motion = Motion::default();
        motion.set_style(style);
        // The first `sync` snaps: there is no state.
        motion.sync(Some([0.0, 0.0]), 0, 0.0, 0, false, false);
        assert!(motion.settled(), "the first frame started an animation");
        motion.sync(Some([10.0, 4.0]), 0, 0.0, 0, false, false);
        motion
    }

    /// The common setup of those testing the spring's physics; the default is
    /// already `Spring` but what the test measures must be written at the call site.
    fn moving() -> Motion {
        moving_with(CursorMotion::Spring)
    }

    /// Advances until `settled`; returns how many frames it took. The ceiling cuts
    /// an infinite loop — so the test does not hang.
    fn run_to_rest(motion: &mut Motion, dt: f32) -> u32 {
        for frames in 1..=10_000 {
            motion.advance(dt);
            if motion.settled() {
                return frames;
            }
        }
        panic!("the animation did not settle");
    }

    #[test]
    fn the_handover_to_the_dock_is_one_animation_not_two() {
        // **The user's two complaints, one cause**: when `sleep 5` ended
        // the caret *teleported* to the dock and while typing in the dock it never
        // slid left or right. Both came from the dock's caret never visiting the
        // animator; now the two homes are two values of the same target.
        let mut motion = moving();
        motion.finish();
        assert!(motion.settled(), "the setup did not settle");

        // The dock is **below** the grid and its target is fractional: the breathing
        // margin does not sit on the cell grid.
        motion.sync(Some([2.0, 12.4]), 0, 0.0, 0, false, false);
        assert!(
            !motion.settled(),
            "the handover did not start an animation: teleport"
        );
        let steps = run_to_rest(&mut motion, TICK);
        assert!(
            steps > 1,
            "the handover finished in a single frame: {steps}"
        );
        assert_eq!(
            motion.position(),
            Some([2.0, 12.4]),
            "the handover did not reach the target"
        );

        // Typing **inside** the dock is the same animation: the column changes, the
        // row does not. It used to produce no motion at all.
        motion.sync(Some([3.0, 12.4]), 0, 0.0, 0, false, false);
        assert!(
            !motion.settled(),
            "the caret did not slide while typing in the dock"
        );
        run_to_rest(&mut motion, TICK);
        assert_eq!(motion.position(), Some([3.0, 12.4]));
    }

    #[test]
    fn the_handover_snaps_when_motion_is_off() {
        // The handover does not give birth to a third mode: `snap` seats it at once
        // too, otherwise we would have **added** animation for a user who turned motion
        // off.
        let mut motion = moving_with(CursorMotion::Snap);
        motion.sync(Some([2.0, 12.4]), 0, 0.0, 0, false, false);
        assert!(
            motion.settled(),
            "the handover started an animation in the snap style"
        );
        assert_eq!(motion.position(), Some([2.0, 12.4]));
    }

    #[test]
    fn a_caretless_frame_drops_the_state_wherever_it_was() {
        // `None` means "no caret in either home": a program hiding the cursor or a
        // window without a dock while the mirror cannot be shown. The state must drop,
        // otherwise in a frame without a caret the old block would hang on screen.
        let mut motion = moving();
        motion.sync(None, 0, 0.0, 0, false, false);
        assert!(motion.settled());
        assert_eq!(motion.position(), None);
    }

    #[test]
    fn every_start_settles_in_finite_steps() {
        // The stop condition itself: from whatever distance it starts it stops in a
        // finite number of steps. Without this test the "zero frames at idle" contract
        // would be just a comment sentence.
        for (col, row) in [(1, 0), (0, 1), (200, 60), (10, 4)] {
            let mut motion = Motion::default();
            motion.sync(Some([0.0, 0.0]), 0, 0.0, 0, false, false);
            motion.sync(Some([col as f32, row as f32]), 0, 0.0, 0, false, false);
            assert!(
                !motion.settled(),
                "the target change did not start an animation"
            );
            let frames = run_to_rest(&mut motion, TICK);
            assert!(
                frames < u32::try_from((TIME_CEILING / TICK).ceil() as i64 + 2).unwrap(),
                "({col},{row}) exceeded the time ceiling: {frames} frames"
            );
            assert_eq!(
                motion.position(),
                Some([col as f32, row as f32]),
                "the settled cursor did not sit on the exact cell"
            );
        }
    }

    #[test]
    fn the_time_ceiling_settles_a_run_the_epsilon_never_would() {
        // **The belt itself, independent of the threshold.** In a normal slide the
        // threshold comes much earlier, so testing the ceiling with a real animation is
        // impossible: what it measured would always be the threshold. The state is
        // therefore set directly — 200 cells away from the target and **standing still**,
        // so neither the position threshold nor the velocity threshold is ever met. The
        // only thing that can stop it is the ceiling.
        let far = State {
            pos: [0.0; 2],
            vel: [0.0; 2],
            from: [0.0; 2],
            target: [200.0, 0.0],
            elapsed: TIME_CEILING,
            since_move: TIME_CEILING,
        };
        assert!(
            far.settled(Mode::Spring),
            "the time ceiling did not stop the run that had filled it"
        );
        assert!(
            !State {
                elapsed: TIME_CEILING - TICK,
                ..far
            }
            .settled(Mode::Spring),
            "stopped before the ceiling filled: the belt fires early"
        );
    }

    #[test]
    fn retarget_in_flight_keeps_velocity() {
        // When the target changes in flight the velocity must be kept: if it were reset,
        // every key while typing would stop the cursor and accelerate it again and the
        // motion would be jagged.
        let mut motion = moving();
        for _ in 0..6 {
            motion.advance(TICK);
        }
        let before = motion.state.expect("in flight").vel;
        assert!(before[0] > 0.0, "never accelerated: {before:?}");

        motion.sync(Some([20.0, 4.0]), 0, 0.0, 0, false, false);
        let after = motion.state.expect("in flight").vel;
        assert_eq!(after, before, "the retarget reset the velocity");
        assert_eq!(
            motion.state.expect("in flight").elapsed,
            0.0,
            "the new target did not reset the time ceiling"
        );
    }

    #[test]
    fn a_retarget_in_flight_does_not_overshoot() {
        // The design: "near critical damping, **no overshoot**". ζ = 1 gives this only
        // from rest; since `sync` deliberately keeps the velocity, a small correction in
        // the middle of a long jump would overshoot the target — the worst case
        // measured without the clamp was 0.87 cells.
        let mut motion = Motion::default();
        motion.sync(Some([0.0, 0.0]), 0, 0.0, 0, false, false);
        motion.sync(Some([10.0, 0.0]), 0, 0.0, 0, false, false);
        for _ in 0..6 {
            motion.advance(TICK);
        }
        let pos = motion.position().expect("in flight")[0];
        let vel = motion.state.expect("in flight").vel[0];
        // The condition for overshoot: the velocity is greater than OMEGA times the
        // remaining distance.
        let target = pos + 0.5;
        assert!(
            vel > OMEGA * 0.5,
            "the scenario does not produce overshoot: velocity {vel}, threshold {}",
            OMEGA * 0.5
        );

        // We cannot set the target fractionally (`sync` takes cells), so we set the
        // state directly: what is being asked is `advance`'s clamp.
        motion.state = Some(State {
            pos: [pos, 0.0],
            vel: [vel, 0.0],
            from: [pos, 0.0],
            target: [target, 0.0],
            elapsed: 0.0,
            since_move: 0.0,
        });
        for _ in 0..60 {
            motion.advance(TICK);
            assert!(
                motion.position().expect("in flight")[0] <= target + POS_EPSILON,
                "the cursor overshot the target {target}: {:?}",
                motion.position()
            );
        }
    }

    #[test]
    fn even_the_longest_jump_settles_before_the_ceiling() {
        // The ceiling is a **belt**: it must not cut a legitimate slide. Since the
        // thresholds are absolute the settling time grows with distance and on a wide
        // screen a line-start return can be 400 cells. Had it cut, the symptom would be a
        // visible snap at the end of the slide — and no counter would see it.
        for distance in [200u16, 400] {
            let mut motion = Motion::default();
            motion.sync(Some([0.0, 0.0]), 0, 0.0, 0, false, false);
            motion.sync(Some([distance as f32, 0.0]), 0, 0.0, 0, false, false);
            let frames = run_to_rest(&mut motion, TICK);
            let elapsed = motion.state.expect("settled").elapsed;
            assert!(
                elapsed < TIME_CEILING,
                "the ceiling cut the {distance}-cell jump: {elapsed}s ({frames} frames)"
            );
        }
    }

    #[test]
    fn retarget_to_the_same_cell_does_not_reset_the_ceiling() {
        // Had content frames that do not move the cursor (a colour change, text on a
        // lower line) reset the ceiling, the belt would never fill.
        let mut motion = moving();
        for _ in 0..6 {
            motion.advance(TICK);
        }
        let elapsed = motion.state.expect("in flight").elapsed;
        motion.sync(Some([10.0, 4.0]), 0, 0.0, 0, false, false);
        assert_eq!(motion.state.expect("in flight").elapsed, elapsed);
    }

    #[test]
    fn a_clipped_dt_does_not_teleport() {
        // The wild stamp that arrives when occlusion lifts: unclamped, it teleports to
        // the target in a single step and the animation is never seen. `DT_MAX` bounds
        // how far one step can advance.
        let mut clipped = moving();
        clipped.advance(30.0);
        let mut stepped = moving();
        stepped.advance(DT_MAX);
        assert_eq!(
            clipped.position(),
            stepped.position(),
            "`dt` was not clamped: the wild stamp advanced it extra"
        );
    }

    #[test]
    fn scrolling_snaps_instead_of_animating() {
        // Scrolling into history moves the cursor on screen but the cursor did not
        // move; this is the only thing indistinguishable from `row` and the offset
        // distinguishes it.
        let mut motion = moving();
        run_to_rest(&mut motion, TICK);

        motion.sync(Some([10.0, 7.0]), 0, 0.0, 3, false, false);
        assert!(motion.settled(), "scrolling started an animation");
        assert_eq!(motion.position(), Some([10.0, 7.0]));

        // The same row change is animated while the offset is **constant**.
        motion.sync(Some([10.0, 4.0]), 0, 0.0, 3, false, false);
        assert!(!motion.settled(), "the cursor's own movement was snapped");
    }

    #[test]
    fn finishing_a_flight_settles_it_at_the_target() {
        // The occluded window's branch: the link stops, so `advance` will not run
        // again. Had it not been finished, the state would stay "unsettled" forever and
        // the timed run would say `MotionUnsettled` while the code is right.
        let mut motion = moving();
        motion.advance(TICK);
        assert!(!motion.settled());

        motion.finish();
        assert!(motion.settled(), "the finished slide did not settle");
        assert_eq!(motion.position(), Some([10.0, 4.0]));

        // The content frame that arrives when occlusion lifts reports the same target:
        // the animation must not restart.
        motion.sync(Some([10.0, 4.0]), 0, 0.0, 0, false, false);
        assert!(
            motion.settled(),
            "the visibility return started an animation"
        );
    }

    #[test]
    fn geometry_and_visibility_snap() {
        // Geometry: the window/font/zoom moved, the grid shifted.
        let mut motion = moving();
        motion.sync(Some([3.0, 1.0]), 0, 0.0, 0, true, false);
        assert!(motion.settled(), "geometry started an animation");
        assert_eq!(motion.position(), Some([3.0, 1.0]));

        // Invisibility empties the state; a cursor that turns back on is born in its new
        // place. TUIs do exactly this: while drawing they hide the cursor and move it.
        motion.sync(None, 0, 0.0, 0, false, false);
        assert!(motion.settled());
        assert_eq!(
            motion.position(),
            None,
            "the invisible cursor gave a position"
        );
        motion.sync(Some([40.0, 20.0]), 0, 0.0, 0, false, false);
        assert!(
            motion.settled(),
            "the visibility return started an animation"
        );
        assert_eq!(motion.position(), Some([40.0, 20.0]));
    }

    #[test]
    fn snap_never_starts_an_animation() {
        // The definition of the style: no slide. Its consequence is not only visual but
        // also in the bookkeeping — because `settled()` is never `false` the link sleeps
        // in the "no damage" branch, i.e. `motion=0`.
        let mut motion = moving_with(CursorMotion::Snap);
        assert!(motion.settled(), "snap started an animation");
        assert_eq!(motion.position(), Some([10.0, 4.0]));

        // Every possibility of a flight: a far jump, a single cell, the same cell.
        for (col, row) in [(400, 0), (11, 4), (11, 4), (0, 0)] {
            motion.sync(Some([col as f32, row as f32]), 0, 0.0, 0, false, false);
            assert!(
                motion.settled(),
                "({col},{row}) started an animation under snap"
            );
            assert_eq!(motion.position(), Some([col as f32, row as f32]));
        }
    }

    #[test]
    fn ease_takes_the_same_time_at_every_distance() {
        // This is the one place where `ease` parts from the spring: the duration is
        // independent of distance. The spring spends ~230 ms on 1 cell, ~460 ms on 400
        // cells (`OMEGA`).
        for (col, row) in [(1, 0), (200, 60), (10, 4)] {
            let mut motion = moving_with(CursorMotion::Ease);
            motion.sync(Some([0.0, 0.0]), 0, 0.0, 0, true, false);
            motion.sync(Some([col as f32, row as f32]), 0, 0.0, 0, false, false);
            assert!(
                !motion.settled(),
                "the target change did not start an animation"
            );
            let frames = run_to_rest(&mut motion, TICK);
            let expected = (EASE_DURATION / TICK).ceil() as u32;
            assert_eq!(
                frames, expected,
                "({col},{row}) did not settle in the fixed time: {frames} frames"
            );
            assert_eq!(
                motion.position(),
                Some([col as f32, row as f32]),
                "the settled cursor did not sit on the exact cell"
            );
        }
    }

    #[test]
    fn ease_approaches_the_target_without_overshooting() {
        // The style where overshoot is **structurally** impossible: the position moves
        // monotonically between `from → target` and the velocity is not integrated. The
        // counterpart of the spring's clamp here is a test, not a branch.
        let mut motion = moving_with(CursorMotion::Ease);
        let mut last = 0.0;
        for _ in 0..40 {
            motion.advance(TICK);
            let pos = motion.position().expect("in flight")[0];
            assert!(pos >= last, "ease went backwards: {last} → {pos}");
            assert!(pos <= 10.0, "ease overshot the target: {pos}");
            last = pos;
        }
    }

    #[test]
    fn ease_settles_well_inside_the_ceiling() {
        // `ease`'s stop condition is its own clock, so the time ceiling is not applied
        // to it; if the order broke, the belt would cut a legitimate slide. The `const`
        // assert next to the constants ties this to compilation and this test ties it to
        // a real run.
        let mut motion = moving_with(CursorMotion::Ease);
        run_to_rest(&mut motion, TICK);
        let elapsed = motion.state.expect("settled").elapsed;
        assert!(elapsed < TIME_CEILING, "the ceiling cut ease: {elapsed}s");
    }

    #[test]
    fn switching_to_snap_finishes_the_flight_and_asks_for_a_frame() {
        // A user who switches to `snap` mid-slide: the cursor cannot hang in an
        // intermediate cell. The return is `true` because the link's "no damage" branch
        // sleeps without drawing on a settled animation — what asks for the frame is
        // that return.
        let mut motion = moving();
        motion.advance(TICK);
        assert!(!motion.settled());

        assert!(
            motion.set_style(CursorMotion::Snap),
            "no frame was asked for"
        );
        assert!(motion.settled(), "the switch to snap did not end the slide");
        assert_eq!(motion.position(), Some([10.0, 4.0]));

        // On a settled cursor and with the same style it is a no-op: asking for a frame
        // would be a wasted wakeup.
        assert!(
            !motion.set_style(CursorMotion::Snap),
            "the same style asked for a frame"
        );
        assert!(
            !motion.set_style(CursorMotion::Spring),
            "the settled cursor asked for a frame"
        );
    }

    #[test]
    fn switching_style_in_flight_does_not_teleport() {
        // A style change is not a target change: the cursor must continue from where it
        // stands. The real risk is in `ease` because it remembers its start point — had
        // `from` not been refreshed the cursor would jump back to the old start.
        for style in [CursorMotion::Ease, CursorMotion::Spring] {
            let mut motion = moving_with(match style {
                CursorMotion::Ease => CursorMotion::Spring,
                _ => CursorMotion::Ease,
            });
            for _ in 0..8 {
                motion.advance(TICK);
            }
            let before = motion.position().expect("in flight");
            assert!(before[0] > 0.0, "never advanced: {before:?}");

            assert!(
                !motion.set_style(style),
                "the style change asked for a frame"
            );
            assert_eq!(
                motion.position(),
                Some(before),
                "the style change teleported"
            );
            // And the slide ends with the new style, it does not get stuck.
            run_to_rest(&mut motion, TICK);
            assert_eq!(motion.position(), Some([10.0, 4.0]));
        }
    }

    #[test]
    fn a_long_spring_flight_does_not_teleport_when_the_style_changes() {
        // The most insidious form of a style change: switching to `ease` when the spring
        // has flown longer than `EASE_DURATION`. Since the stop condition changes with
        // the style, if the "in flight?" question is not asked with the **old** style
        // the slide looks settled, the handover is skipped and the next frame throws the
        // cursor to the target — and since the link sleeps without drawing that frame
        // the cursor stays in an intermediate cell.
        let mut motion = moving();
        for _ in 0..25 {
            motion.advance(TICK);
        }
        let elapsed = motion.state.expect("in flight").elapsed;
        assert!(
            elapsed > EASE_DURATION,
            "the scenario was not set up: the spring flew {elapsed}s, threshold {EASE_DURATION}s"
        );
        let before = motion.position().expect("in flight");
        assert!(
            !motion.settled(),
            "the spring must not have settled at this point"
        );

        assert!(
            !motion.set_style(CursorMotion::Ease),
            "the style asked for a frame"
        );
        assert!(
            !motion.settled(),
            "the style change showed the slide as settled: teleport a frame later"
        );
        motion.advance(TICK);
        let after = motion.position().expect("in flight");
        let remaining = 10.0 - before[0];
        assert!(
            after[0] - before[0] < remaining,
            "jumped to the target in a single step: {before:?} → {after:?}"
        );
        run_to_rest(&mut motion, TICK);
        assert_eq!(motion.position(), Some([10.0, 4.0]));
    }

    /// Reduce Motion on, a fade in flight: `(0,0)` → `(10,4)`.
    fn fading() -> Motion {
        let mut motion = Motion::default();
        motion.set_reduce(true);
        motion.sync(Some([0.0, 0.0]), 0, 0.0, 0, false, false);
        assert!(motion.settled(), "the first frame started a fade");
        motion.sync(Some([10.0, 4.0]), 0, 0.0, 0, false, false);
        motion
    }

    #[test]
    fn reduce_motion_fades_in_place_instead_of_sliding() {
        // The definition of the reduction: the cursor fades in **in its new cell**, it
        // does not set out on a road. The position must be set in `sync` — if it is set
        // one frame later the cursor is drawn at alpha zero in its old cell in the first
        // content frame and no counter sees it.
        let mut motion = fading();
        assert_eq!(motion.position(), Some([10.0, 4.0]), "the fade slid");
        assert_eq!(motion.alpha(), 0.0, "the fade started opaque");
        assert!(!motion.settled(), "the fade never started");

        // The opacity rises monotonically and the position never moves.
        let mut last = 0.0;
        while !motion.settled() {
            motion.advance(TICK);
            let alpha = motion.alpha();
            assert!(alpha >= last, "the fade went backwards: {last} → {alpha}");
            assert!(alpha <= 1.0, "the opacity exceeded 1: {alpha}");
            assert_eq!(
                motion.position(),
                Some([10.0, 4.0]),
                "the fade slid the position"
            );
            last = alpha;
        }
        assert_eq!(motion.alpha(), 1.0, "the settled fade is not opaque");

        // The duration is **independent of distance** and equals `FADE_DURATION`: the
        // same shape as `ease`'s clock, only the constant differs. `run_to_rest` also
        // gives the pause in between, so this movement counts as "a separate movement"
        // and fades in again (the pause criterion of `Motion::sync`).
        for (col, row) in [(1u16, 0u16), (200, 60)] {
            let mut motion = fading();
            run_to_rest(&mut motion, TICK);
            motion.sync(Some([col as f32, row as f32]), 0, 0.0, 0, false, false);
            let frames = run_to_rest(&mut motion, TICK);
            assert_eq!(
                frames,
                (FADE_DURATION / TICK).ceil() as u32,
                "({col},{row})"
            );
        }
    }

    #[test]
    fn reduce_motion_does_not_fade_what_did_not_move() {
        // The snap cases do not fade: the cursor did not move, the **grid under it**
        // moved. A rule looking at the clock alone would count them as
        // unsettled for `FADE_DURATION` and the link would draw frames that change
        // nothing — the same reason as `ease`'s second condition, the same branch.
        let mut motion = fading();
        run_to_rest(&mut motion, TICK);

        // Scrolling, geometry and visibility return: all three are instant and opaque.
        motion.sync(Some([10.0, 7.0]), 0, 0.0, 3, false, false);
        assert!(motion.settled(), "scrolling started a fade");
        assert_eq!(motion.alpha(), 1.0);
        motion.sync(Some([3.0, 1.0]), 0, 0.0, 3, true, false);
        assert!(motion.settled(), "geometry started a fade");
        assert_eq!(motion.alpha(), 1.0);
        motion.sync(None, 0, 0.0, 3, false, false);
        motion.sync(Some([40.0, 20.0]), 0, 0.0, 3, false, false);
        assert!(motion.settled(), "the visibility return started a fade");
        assert_eq!(motion.alpha(), 1.0);

        // Retargeting to the same cell is not a fade either.
        motion.sync(Some([40.0, 20.0]), 0, 0.0, 3, false, false);
        assert!(motion.settled(), "a cursor standing in place faded in");
    }

    #[test]
    fn snap_outranks_reduce_motion() {
        // A product decision: the accessibility setting does not **add** an animation
        // for a user who has already turned motion off. The mode says so too: `Snap`
        // is above the flag.
        let mut motion = Motion::default();
        motion.set_style(CursorMotion::Snap);
        motion.set_reduce(true);
        motion.sync(Some([0.0, 0.0]), 0, 0.0, 0, false, false);
        motion.sync(Some([10.0, 4.0]), 0, 0.0, 0, false, false);
        assert!(motion.settled(), "snap + reduce started an animation");
        assert_eq!(motion.position(), Some([10.0, 4.0]));
        assert_eq!(
            motion.alpha(),
            1.0,
            "the snap cursor was drawn half transparent"
        );
    }

    #[test]
    fn turning_reduce_motion_on_finishes_the_flight() {
        // Turning the setting on while a slide is in flight: the mode no longer slides,
        // so there is nothing to take over. The return is `true` because the link's "no
        // damage" branch sleeps without drawing on a settled animation.
        let mut motion = moving();
        motion.advance(TICK);
        assert!(!motion.settled());

        assert!(motion.set_reduce(true), "no frame was asked for");
        assert!(motion.settled(), "turning on did not end the slide");
        assert_eq!(motion.position(), Some([10.0, 4.0]));
        assert_eq!(
            motion.alpha(),
            1.0,
            "the finished slide stayed half transparent"
        );

        // Writing the same value again, and a settled cursor, are no-ops.
        assert!(!motion.set_reduce(true), "the same value asked for a frame");
        assert!(
            !motion.set_reduce(false),
            "the settled cursor asked for a frame"
        );
    }

    #[test]
    fn a_cursor_that_keeps_moving_still_becomes_visible_while_fading() {
        // A review finding and the place where the reduction **turned upside
        // down**: had every target change reset the clock, a cursor moving fast could
        // never fill 90 ms, i.e. the Reduce Motion cursor would blink (while typing) or
        // vanish entirely (in streaming output).
        //
        // The scenario is streaming output: a cursor that advances a cell every frame.
        let mut motion = fading();
        let mut col = 10;
        for _ in 0..30 {
            motion.advance(TICK);
            col += 1;
            motion.sync(Some([col as f32, 4.0]), 0, 0.0, 0, false, false);
        }
        assert_eq!(
            motion.alpha(),
            1.0,
            "the cursor stayed transparent in streaming output: opacity {}",
            motion.alpha()
        );
        // The position is always at the target: the fade does not slide.
        assert_eq!(motion.position(), Some([col as f32, 4.0]));
        // And it settles — the link does not stay awake forever for this cursor.
        assert!(motion.settled(), "the fade did not settle");

        // The next movement inside the stream does not refresh the fade either: fading
        // in again every 90 ms would be a flicker at ~11 Hz.
        motion.advance(TICK);
        motion.sync(Some([(col + 1) as f32, 4.0]), 0, 0.0, 0, false, false);
        assert_eq!(
            motion.alpha(),
            1.0,
            "the movement inside the stream faded in again (flicker)"
        );
        assert!(
            motion.settled(),
            "the movement inside the stream woke the link"
        );

        // A movement **after a pause** is a separate movement and fades in: the
        // reduction's promise stands here. The first frame of a link waking from sleep
        // falls here too — `dt` is clamped to `DT_MAX` and the clamp is longer than the
        // fade (the `const _` at the top of the file).
        motion.advance(DT_MAX);
        motion.sync(Some([(col + 2) as f32, 4.0]), 0, 0.0, 0, false, false);
        assert_eq!(
            motion.alpha(),
            0.0,
            "the movement after a pause did not fade in"
        );
        assert!(!motion.settled(), "the fade never started");
    }

    #[test]
    fn reduce_off_mid_fade_does_not_slide_backwards() {
        // Had the closing had a takeover branch, the most insidious defect would be
        // here: `from` stands in the previous cell and `ease` recomputes the position
        // from it — the cursor would return to the cell it came to and slide again. The
        // `spring` settles at once because it is at its target with no velocity, so it
        // would leave a half-transparent cursor on screen.
        for style in [CursorMotion::Ease, CursorMotion::Spring] {
            let mut motion = fading();
            motion.set_style(style);
            for _ in 0..4 {
                motion.advance(TICK);
            }
            let alpha = motion.alpha();
            assert!(
                alpha > 0.0 && alpha < 1.0,
                "the scenario was not set up: {alpha}"
            );

            assert!(
                motion.set_reduce(false),
                "{style:?}: no frame was asked for"
            );
            assert!(motion.settled(), "{style:?}: the fade did not end");
            assert_eq!(
                motion.position(),
                Some([10.0, 4.0]),
                "{style:?}: teleported"
            );
            assert_eq!(motion.alpha(), 1.0, "{style:?}: stayed half transparent");
        }
    }

    #[test]
    fn style_change_during_a_fade_keeps_fading() {
        // In the fade mode `ease` ↔ `spring` is nothing: had the takeover branch run,
        // `from` would be pulled to the current position, `from == target` would hold and
        // the fade would look **settled** — the link would sleep without drawing that
        // frame and the cursor would hang half transparent.
        for style in [CursorMotion::Ease, CursorMotion::Snap] {
            let mut motion = fading();
            for _ in 0..3 {
                motion.advance(TICK);
            }
            let before = motion.alpha();
            assert!(
                before > 0.0 && before < 1.0,
                "the scenario was not set up: {before}"
            );

            let asked = motion.set_style(style);
            if style == CursorMotion::Snap {
                // `snap` ends the fade too and asks for the frame: the style says "no
                // animation" and a half-finished opacity is an animation as well.
                assert!(asked, "the switch to snap did not ask for a frame");
                assert!(motion.settled());
                assert_eq!(motion.alpha(), 1.0);
            } else {
                assert!(!asked, "{style:?}: the fade asked for a frame");
                assert!(!motion.settled(), "{style:?}: the fade looked settled");
                assert_eq!(motion.alpha(), before, "{style:?}: the opacity jumped");
                run_to_rest(&mut motion, TICK);
                assert_eq!(motion.alpha(), 1.0);
            }
        }
    }

    #[test]
    fn a_flight_that_settles_early_stays_settled_in_every_mode() {
        // A review finding. `advance`'s settling branch pulled `pos` and `vel`
        // to the target but left `from` **where it was** — yet `ease`'s and `fade`'s
        // stop condition looks exactly at `from == target` ("no road to travel"). A
        // slide that settled early by the spring's threshold would therefore look
        // unsettled in another mode, and the real cost is this: the link has already
        // **gone to sleep** counting that frame as "settled", so `advance` never runs
        // again. The result is `MotionUnsettled` in the timed run because of an
        // animation that does not exist, and a half-transparent cursor for no reason in
        // the fade.
        //
        // The scenario is the overshoot clamp: putting the target right in front of the
        // velocity settles the spring **in the first step**, so `elapsed` is far below
        // both durations.
        let mut motion = moving();
        for _ in 0..6 {
            motion.advance(TICK);
        }
        let pos = motion.position().expect("in flight")[0];
        let vel = motion.state.expect("in flight").vel[0];
        motion.state = Some(State {
            pos: [pos, 0.0],
            vel: [vel, 0.0],
            from: [pos, 0.0],
            target: [pos + 0.5, 0.0],
            elapsed: 0.0,
            since_move: 0.0,
        });
        motion.advance(TICK);
        assert!(
            motion.settled(),
            "the spring did not settle by the overshoot clamp"
        );
        let elapsed = motion.state.expect("settled").elapsed;
        // The **smaller** of the two durations: which one is smaller is not this test's
        // claim and `min` writes it without tying it to the order of the constants.
        assert!(
            elapsed < FADE_DURATION.min(EASE_DURATION),
            "the scenario was not set up: {elapsed}s is not below both durations"
        );

        // A settled slide must not look "in flight" in any mode — and this is also the
        // requirement for those two setters not to ask for a frame: there is nobody to
        // draw the frame they do not ask for.
        let mut eased = motion;
        assert!(
            !eased.set_style(CursorMotion::Ease),
            "a frame was asked for"
        );
        assert!(
            eased.settled(),
            "a settled slide looked in flight in `ease`"
        );

        let mut faded = motion;
        assert!(!faded.set_reduce(true), "a frame was asked for");
        assert!(
            faded.settled(),
            "a settled slide looked in flight in the fade"
        );
        assert_eq!(
            faded.alpha(),
            1.0,
            "a settled cursor was drawn half transparent"
        );
    }

    /// The frame pair of an Enter: in a 30-row grid the cursor is on row 2 and the
    /// content is three rows (offset 27), then the cursor goes down to row 3 and the
    /// content becomes four rows (offset 26). The cursor's **screen** row is 29 in
    /// both — the bottom row.
    fn after_enter() -> Motion {
        let mut motion = Motion::default();
        motion.sync(Some([0.0, 29.0]), 27, 0.0, 0, false, false);
        assert!(motion.settled(), "the first frame started an animation");
        motion.sync(Some([0.0, 29.0]), 26, 0.0, 0, false, false);
        motion
    }

    #[test]
    fn the_cursor_does_not_move_while_the_origin_slides() {
        // **The crux.** On Enter the grid row goes `r → r+1` while the
        // offset drops by one; had the two targets not been in the same space the cursor
        // would drop a row and the spring would bring it back (a known earlier
        // intermediate state). In screen space the target does **not** change at all.
        let mut motion = after_enter();
        assert!(motion.cursor_settled(), "the cursor set out on Enter");
        assert_eq!(
            motion.position(),
            Some([0.0, 29.0]),
            "the cursor is not at the bottom"
        );
        assert!(!motion.origin_settled(), "the offset did not start sliding");

        // While the offset slides the cursor does **not** move on screen at all: two
        // animators under the single `settled()` gate but on their own.
        while !motion.settled() {
            motion.advance(TICK);
            assert_eq!(motion.position(), Some([0.0, 29.0]), "the cursor slid");
        }
        assert_eq!(
            motion.origin(),
            26.0,
            "the offset did not sit at its target"
        );
    }

    #[test]
    fn the_origin_settles_and_then_lets_the_link_sleep() {
        // The checklist: "no frames are asked for after the slide settles". The link's
        // sleep decision is a single expression (the "no damage" branch of `link.rs`):
        // `motion.settled()`. The offset is **inside** that gate, so when the
        // slide ends the frames end too — the "zero frames at idle" contract stands.
        let mut motion = after_enter();
        assert!(!motion.settled(), "the slide did not wake the link");

        // The slide is **progressing**: somewhere between the two ends. If the test did
        // not ask this, code that never slides at all would pass too.
        motion.advance(TICK);
        let mid = motion.origin();
        assert!(mid < 27.0 && mid > 26.0, "the offset did not slide: {mid}");

        let frames = run_to_rest(&mut motion, TICK);
        assert!(
            frames < u32::try_from((TIME_CEILING / TICK).ceil() as i64 + 2).unwrap(),
            "the slide exceeded the time ceiling: {frames} frames"
        );
        assert_eq!(motion.origin(), 26.0);
        // And a settled slide does not wake again: content frames reporting the same
        // target (a colour change, the cursor blinking) must not restart the slide.
        motion.sync(Some([0.0, 29.0]), 26, 0.0, 0, false, false);
        assert!(motion.settled(), "the same target restarted the slide");
    }

    #[test]
    fn a_shrinking_origin_snaps_unless_history_fills_the_gap() {
        // **The direction rule** (eye check): the offset is
        // `rows - content_rows`, so the target **falling** is the content growing (the
        // grid flows up) and **rising** is it shrinking (the grid comes down). Flowing
        // up reads as content arriving, coming down as falling.
        //
        // **The fill narrowed the rule**, it did not remove it: if the gap above fills
        // with the ledger's rows what comes down is not a gap but history arriving, and
        // the rule's reason is moot in that branch. The test's name changed for that reason
        // too — the old name (`a_growing_origin_slides_and_a_shrinking_one_snaps`) would
        // now lie.
        let mut motion = after_enter();
        run_to_rest(&mut motion, TICK);

        // Entering vim: the fill jumps to `rows` in one move and the offset **drops** to
        // 0 — the interface arrives by gliding and this is wanted.
        motion.sync(Some([0.0, 0.0]), 0, 0.0, 0, false, false);
        assert!(!motion.origin_settled(), "growing content was snapped");
        run_to_rest(&mut motion, TICK);
        assert_eq!(motion.origin(), 0.0);

        // Shrinking content, `fill == 0`: the offset **rises** but the shell does not
        // glide down, it sits in place at once.
        //
        // **The scene's name is not "leaving `vim`"** and this was corrected at a review
        // gate: there `fill` is **not** zero (the entry `2J` does not set the flag, all
        // four gates are open) and what brings the snap is the next turn's `geometry`.
        // The zero here is the common state of a window with no dock, a deliberately
        // cleared screen and a window scrolled into history; the reason is in
        // [`Motion::sync_origin`]'s doc.
        motion.sync(Some([0.0, 29.0]), 26, 0.0, 0, false, false);
        assert!(motion.origin_settled(), "shrinking content slid");
        assert_eq!(motion.origin(), 26.0);

        // `clear` on a full screen is the same class: the offset rises from top to
        // bottom. The deliberate-clear flag already zeroes `fill` (`bt-core`), so
        // `filled = false` comes here.
        motion.sync(Some([0.0, 0.0]), 0, 0.0, 0, false, false);
        run_to_rest(&mut motion, TICK);
        motion.sync(Some([0.0, 29.0]), 29, 0.0, 0, false, false);
        assert!(motion.origin_settled(), "clear came down sliding");
        assert_eq!(motion.origin(), 29.0);
    }

    #[test]
    fn a_filled_gap_slides_the_origin_down_and_settles() {
        // **The crux branch of the fill exception:** the Tab list closes, the fill narrows and the
        // offset **rises** — but the ledger's newest rows enter the gap, so the screen
        // does not come down, history arrives from above. This is exactly what should
        // glide.
        let mut motion = after_enter();
        run_to_rest(&mut motion, TICK);
        motion.sync(Some([0.0, 0.0]), 0, 0.0, 0, false, false);
        run_to_rest(&mut motion, TICK);
        assert_eq!(motion.origin(), 0.0, "the scenario was not set up");

        motion.sync(Some([0.0, 8.0]), 8, 0.0, 0, false, true);
        assert!(!motion.origin_settled(), "snapped while there was a fill");
        // Really on the road in the middle frame: code that teleports to the target
        // would pass the `origin_settled()` test too.
        motion.advance(TICK);
        let mid = motion.origin();
        assert!(
            mid > 0.0 && mid < 8.0,
            "the slide is not at an intermediate position: {mid}"
        );

        // **Finite**: no new animator, `Slide::settled()` applies as is and the
        // time ceiling ends the slide.
        let frames = run_to_rest(&mut motion, TICK);
        assert!(frames > 0, "the slide never ran a frame");
        assert!(
            frames < u32::try_from((TIME_CEILING / TICK).ceil() as i64 + 2).unwrap(),
            "the slide exceeded the time ceiling: {frames} frames"
        );
        assert_eq!(motion.origin(), 8.0);

        // And a settled slide does not wake again: content frames arriving while the fill
        // goes on (blink, keys) report the same target and `filled` does not touch the
        // no-op — had it, every frame with a fill would ask for a motion frame and the
        // "zero frames at idle" contract would fall.
        motion.sync(Some([0.0, 8.0]), 8, 0.0, 0, false, true);
        assert!(
            motion.settled(),
            "the same target started the slide in the fill"
        );
    }

    #[test]
    fn scrolling_and_geometry_snap_the_origin() {
        // The item "the grid moving for another reason does not slide" of
        // `docs/SETTINGS.md`: the wheel follows the finger, and a
        // window/font/point size change moves the grid without animation too. In both
        // the content did not rise by its own growth.
        let mut motion = after_enter();
        run_to_rest(&mut motion, TICK);

        motion.sync(Some([0.0, 23.0]), 20, 0.0, 1, false, false);
        assert!(motion.settled(), "scrolling started a slide");
        assert_eq!(motion.origin(), 20.0);

        motion.sync(Some([0.0, 13.0]), 10, 0.0, 1, true, false);
        assert!(motion.settled(), "geometry started a slide");
        assert_eq!(motion.origin(), 10.0);

        // But the same change slides while the offset is **constant**: what produces the
        // snap is not the target itself but the grid moving for another reason.
        motion.sync(Some([0.0, 12.0]), 9, 0.0, 1, false, false);
        assert!(!motion.origin_settled(), "content growth was snapped");

        // **The fill does not puncture these two triggers** and this is the
        // test's second job: had the `filled` term been written **outside** `!snap` in
        // the guard, Rust's precedence would make the expression
        // `(… && !snap && …) || filled` and in a window with a fill the wheel and window
        // resizing would start to animate. `bt-core`'s `display_offset == 0` gate cuts
        // the wheel but does not cut the **geometry** branch.
        run_to_rest(&mut motion, TICK);
        motion.sync(Some([0.0, 15.0]), 12, 0.0, 1, true, true);
        assert!(motion.settled(), "geometry started a slide in the fill");
        assert_eq!(motion.origin(), 12.0);

        motion.sync(Some([0.0, 18.0]), 15, 0.0, 2, false, true);
        assert!(motion.settled(), "scrolling started a slide in the fill");
        assert_eq!(motion.origin(), 15.0);
    }

    #[test]
    fn a_hidden_cursor_does_not_freeze_the_origin() {
        // Had `sync`'s `!visible` early return skipped the offset too, in a script that
        // hides the cursor and streams output the content would freeze in the wrong
        // place. The cursor's visibility cannot decide where the grid stands.
        let mut motion = after_enter();
        run_to_rest(&mut motion, TICK);

        motion.sync(None, 25, 0.0, 0, false, false);
        assert_eq!(
            motion.position(),
            None,
            "the invisible cursor gave a position"
        );
        assert!(
            !motion.origin_settled(),
            "the invisible cursor froze the offset"
        );
        run_to_rest(&mut motion, TICK);
        assert_eq!(motion.origin(), 25.0);
    }

    #[test]
    fn a_shrinking_content_settles_too() {
        // Its input is not monotonic — a program that moves the cursor up and
        // erases the bottom line with `\e[K` can grow and shrink the offset. Because the
        // stop condition looks at the distance not the target, every new target is finite
        // on its own; if the oscillation itself continues, what asks for those frames is
        // not the animation but the damage of the output producing the oscillation.
        let mut motion = after_enter();
        let mut previous = 26u16;
        for origin in [26u16, 27, 25, 27, 26] {
            motion.advance(TICK);
            motion.sync(
                Some([0 as f32, (29 - origin + origin) as f32]),
                origin,
                0.0,
                0,
                false,
                false,
            );
            // The claim is **settling**, not how many frames it ran: since the direction rule
            // the two halves of the oscillation go through two roads — the shrinking
            // direction (the target rises) sits without running a frame, the growing one by
            // sliding. What is wanted is for both to be **finite**.
            if origin > previous {
                assert!(
                    motion.origin_settled(),
                    "shrinking content ({previous} → {origin}) slid"
                );
            }
            run_to_rest(&mut motion, TICK);
            assert!(
                motion.origin_settled(),
                "the offset {origin} did not settle"
            );
            assert_eq!(motion.origin(), f32::from(origin));
            previous = origin;
        }
    }

    #[test]
    fn reduce_motion_snaps_the_origin_instead_of_fading_it() {
        // While the cursor fades in the offset snaps: the whole screen fading
        // in on every new line would be worse than the motion the reduction tries to
        // remove. The rule "the reduction's one place is `bt-gpu::motion`" stands — the
        // **place** is the same, the **mode** is two.
        let mut motion = Motion::default();
        motion.set_reduce(true);
        motion.sync(Some([0.0, 29.0]), 27, 0.0, 0, false, false);
        // The cursor changes **column too**: since the screen row does not move on Enter,
        // a one-row advance alone would not give birth to a fade either — for
        // the test to tell the two modes apart the cursor has to really move.
        motion.sync(Some([5.0, 29.0]), 26, 0.0, 0, false, false);
        assert_eq!(motion.origin(), 26.0, "the offset went into a fade");
        assert!(motion.origin_settled(), "the offset slid");
        // The cursor, meanwhile, is inside the fade: two separate modes in the same
        // frame.
        assert_eq!(motion.alpha(), 0.0, "the cursor did not fade in");
        assert!(!motion.cursor_settled());

        // Turning the setting on while a slide is in flight ends it **at its target**:
        // the mode no longer slides, there is nothing to take over.
        let mut motion = after_enter();
        motion.advance(TICK);
        assert!(!motion.origin_settled());
        assert!(motion.set_reduce(true), "no frame was asked for");
        assert_eq!(motion.origin(), 26.0, "turning on did not end the slide");

        // **The fill is no exception either**: the accessibility setting does not
        // *add* animation. `origin_mode()` turns `Fade` into `Snap` and the `filled`
        // term stands **inside** `animated`.
        let mut motion = Motion::default();
        motion.set_reduce(true);
        motion.sync(Some([0.0, 29.0]), 27, 0.0, 0, false, false);
        motion.sync(Some([0.0, 29.0]), 20, 0.0, 0, false, false);
        run_to_rest(&mut motion, TICK);
        motion.sync(Some([0.0, 29.0]), 25, 0.0, 0, false, true);
        assert!(motion.origin_settled(), "the fill slid under Reduce Motion");
        assert_eq!(motion.origin(), 25.0);
    }

    /// Full grid: the offset settled at zero, the target will not move again.
    fn full_grid() -> Motion {
        let mut motion = Motion::default();
        motion.sync(Some([0.0, 29.0]), 0, 0.0, 0, false, false);
        assert!(motion.settled(), "the first frame started an animation");
        motion
    }

    #[test]
    fn a_full_grid_still_slides_when_rows_scroll_off() {
        // **The guard of the defect the user reported** (2026-09-23): once the grid
        // filled the target was fixed and `sync` saw nothing — the slide existed until
        // the grid filled, then did not. The same frame, two rows scrolled.
        let mut motion = full_grid();
        motion.sync(Some([0.0, 29.0]), 0, 0.0, 0, false, false);
        assert!(motion.settled(), "a frame without a scroll started a slide");

        motion.advance(TICK);
        motion.scroll_in(2, 30);
        motion.sync(Some([0.0, 29.0]), 0, 0.0, 0, false, false);
        // The content starts from where it stood in the previous frame: two rows below.
        assert_eq!(motion.origin(), 2.0);
        assert!(!motion.origin_settled(), "scrolling did not start a slide");
        // And it glides upward and settles in place.
        let mut last = motion.origin();
        for _ in 0..3 {
            motion.advance(TICK);
            assert!(motion.origin() < last, "the offset does not flow upward");
            last = motion.origin();
        }
        run_to_rest(&mut motion, TICK);
        assert_eq!(motion.origin(), 0.0);
    }

    #[test]
    fn a_scroll_while_sliding_continues_from_where_the_grid_is() {
        // Streaming output: every frame brings new lines. The slide does not restart
        // from the beginning, it is added to **where it stands** — otherwise the grid
        // would jump back on every line.
        let mut motion = full_grid();
        motion.scroll_in(1, 30);
        motion.advance(TICK);
        let mid = motion.origin();
        assert!(mid > 0.0 && mid < 1.0, "{mid}");
        motion.scroll_in(1, 30);
        assert_eq!(motion.origin(), mid + 1.0);
    }

    #[test]
    fn the_scroll_slide_is_capped_at_the_limit() {
        // Fast streaming output can scroll dozens of rows per frame; the offset must not
        // fall screens behind. The ceiling does not pull the position down, it only stops
        // pushing further.
        let mut motion = full_grid();
        for _ in 0..20 {
            motion.scroll_in(5, 30);
        }
        assert_eq!(motion.origin(), 30.0);
        motion.scroll_in(5, 30);
        assert_eq!(motion.origin(), 30.0);

        // A screenful while the slide is in flight is not a stream by itself:
        // it continues at the cap ([`BURST_WINDOW`] decides, not the flight).
        motion.scroll_in(30, 30);
        assert_eq!(motion.origin(), 30.0);
        assert!(!motion.origin_settled());
    }

    #[test]
    fn a_burst_on_a_resting_grid_slides_in_one_screen() {
        // **The guard of the defect the user reported** (2026-09-23): on a full grid
        // `seq 1 200` scrolls more than a screen in one frame and the slide never
        // started — while the same command glided on an empty grid. On a steady grid the
        // burst brings the last screen from a full screen below.
        let mut motion = full_grid();
        motion.scroll_in(200, 30);
        assert_eq!(
            motion.origin(),
            30.0,
            "the burst did not start from a screen below"
        );
        assert!(!motion.origin_settled(), "the burst did not start a slide");
        let mut last = motion.origin();
        for _ in 0..3 {
            motion.advance(TICK);
            assert!(motion.origin() < last, "the offset does not flow upward");
            last = motion.origin();
        }

        run_to_rest(&mut motion, TICK);
        assert_eq!(motion.origin(), 0.0);
    }

    #[test]
    fn a_stream_of_screenful_scrolls_never_bursts_again() {
        // Guard for the 2026-09-30 regression: under sustained output every
        // content frame scrolls a screen or more. The finish branch leaves the
        // slide at rest, so the next screenful frame used to read as a burst
        // on a resting grid: every other frame drew the grid one screen low
        // and stretched the fill band by a screen (cpu_encode p95 0.23 -> 0.33).
        // Within [`BURST_WINDOW`] the run is still one burst and slides at the
        // cap; past it the stream finishes the slide once and never re-arms.
        let mut motion = full_grid();
        let mut age = 0.0;
        let mut frame = 0;
        loop {
            motion.advance(TICK);
            if frame > 0 {
                age += TICK;
            }
            frame += 1;
            motion.scroll_in(200, 30);
            if age >= BURST_WINDOW {
                break;
            }
            assert_eq!(motion.origin(), 30.0, "frame {frame} left the burst");
            assert!(!motion.origin_settled(), "frame {frame} left the burst");
        }
        assert_eq!(motion.origin(), 0.0, "the stream did not finish the slide");
        assert!(motion.settled(), "the stream did not finish the slide");
        for later in 1..=30 {
            motion.advance(TICK);
            motion.scroll_in(200, 30);
            assert_eq!(
                motion.origin(),
                0.0,
                "frame {later} past the window re-armed the burst"
            );
            assert!(motion.settled(), "frame {later} past the window slid");
        }
    }

    #[test]
    fn a_burst_after_a_stream_still_slides_once_output_rests() {
        // The run is cleared only by a content frame that scrolls less than a
        // screen (typing the next command, the prompt repaint); idle produces
        // no frames. Once cleared, `seq 1 200` must slide in again.
        let mut motion = full_grid();
        for _ in 0..40 {
            motion.advance(TICK);
            motion.scroll_in(200, 30);
        }
        assert!(
            motion.settled(),
            "40 screenful frames did not read as a stream"
        );
        for _ in 0..3 {
            motion.advance(TICK);
            motion.scroll_in(0, 30);
        }
        run_to_rest(&mut motion, TICK);
        motion.advance(TICK);
        motion.scroll_in(200, 30);
        assert_eq!(
            motion.origin(),
            30.0,
            "a burst after the stream did not slide"
        );
        assert!(
            !motion.origin_settled(),
            "a burst after the stream did not slide"
        );
    }

    #[test]
    fn a_burst_split_across_frames_still_slides_in() {
        // Guard for the 2026-09-30 eye check: a short burst (`ls -la`,
        // `seq 1 200`) reaches the grid in one, two or three PTY reads, i.e.
        // content frames. Only the first one used to slide; the second
        // screenful frame read as a stream and finished the slide.
        for chunks in [2, 3] {
            let mut motion = full_grid();
            for chunk in 0..chunks {
                motion.advance(TICK);
                motion.scroll_in(200, 30);
                assert_eq!(
                    motion.origin(),
                    30.0,
                    "chunk {chunk} of {chunks} did not keep the burst a screen low"
                );
                assert!(
                    !motion.origin_settled(),
                    "chunk {chunk} of {chunks} finished the burst"
                );
            }
            run_to_rest(&mut motion, TICK);
            assert_eq!(motion.origin(), 0.0);
        }

        // A first chunk shorter than a screen starts an ordinary slide; the
        // screenful that follows continues it at the cap instead of ending it.
        let mut motion = full_grid();
        motion.advance(TICK);
        motion.scroll_in(20, 30);
        motion.advance(TICK);
        motion.scroll_in(30, 30);
        assert_eq!(motion.origin(), 30.0, "the screenful chunk ended the slide");
        assert!(
            !motion.origin_settled(),
            "the screenful chunk ended the slide"
        );
        run_to_rest(&mut motion, TICK);
        assert_eq!(motion.origin(), 0.0);
    }

    #[test]
    fn back_to_back_bursts_both_slide() {
        // The user's "art arda": a burst, the prompt repaint in between (a
        // content frame that scrolls less than a screen), then another burst —
        // at rest or while the first one is still sliding in.
        let mut motion = full_grid();
        for burst in 0..2 {
            motion.advance(TICK);
            motion.scroll_in(200, 30);
            motion.advance(TICK);
            motion.scroll_in(200, 30);
            assert_eq!(motion.origin(), 30.0, "burst {burst} did not slide");
            assert!(!motion.origin_settled(), "burst {burst} did not slide");
            run_to_rest(&mut motion, TICK);
            motion.advance(TICK);
            motion.scroll_in(0, 30);
        }

        let mut motion = full_grid();
        motion.advance(TICK);
        motion.scroll_in(200, 30);
        for _ in 0..3 {
            motion.advance(TICK);
        }
        motion.scroll_in(1, 30);
        motion.advance(TICK);
        motion.scroll_in(200, 30);
        assert_eq!(
            motion.origin(),
            30.0,
            "a burst over a sliding grid was finished"
        );
        assert!(
            !motion.origin_settled(),
            "a burst over a sliding grid was finished"
        );
    }

    #[test]
    fn geometry_and_the_wheel_cancel_the_scroll_slide() {
        // Shrinking the window pushes lines into history and that is not a scroll;
        // `sync`'s snap must erase the same frame's `scroll_in` too.
        let mut motion = full_grid();
        motion.scroll_in(3, 30);
        motion.sync(Some([0.0, 20.0]), 0, 0.0, 0, true, false);
        assert!(motion.settled(), "geometry did not erase the slide");
        assert_eq!(motion.origin(), 0.0);

        motion.scroll_in(3, 30);
        motion.sync(Some([0.0, 20.0]), 0, 0.0, 4, false, false);
        assert!(motion.settled(), "the wheel did not erase the slide");
        assert_eq!(motion.origin(), 0.0);
    }

    #[test]
    fn snap_and_reduce_motion_never_slide_on_scroll() {
        // Scrolling does not add animation for a user who has turned motion off; Reduce
        // Motion already snaps the offset ([`Motion::origin_mode`]).
        let mut motion = full_grid();
        motion.set_style(CursorMotion::Snap);
        motion.scroll_in(3, 30);
        assert!(motion.settled());
        assert_eq!(motion.origin(), 0.0);

        let mut motion = full_grid();
        motion.set_reduce(true);
        motion.scroll_in(3, 30);
        assert!(motion.settled());
        assert_eq!(motion.origin(), 0.0);
    }

    #[test]
    fn snap_style_never_slides_the_origin() {
        // The slide follows `cursor_motion`, there is no new key. `"snap"`'s
        // promise "this is how to turn motion off completely" stands on this line —
        // `docs/SETTINGS.md` writes it.
        let mut motion = Motion::default();
        motion.set_style(CursorMotion::Snap);
        motion.sync(Some([0.0, 29.0]), 27, 0.0, 0, false, false);
        motion.sync(Some([0.0, 29.0]), 26, 0.0, 0, false, false);
        assert!(motion.settled(), "snap started a slide");
        assert_eq!(motion.origin(), 26.0);

        // **The fill does not puncture `"snap"` either**: the direction rule's
        // exception is inside `animated`, not above it — for a user who has turned
        // motion off the fill does not *add* an animation.
        motion.sync(Some([0.0, 29.0]), 28, 0.0, 0, false, true);
        assert!(motion.settled(), "the fill started a slide under snap");
        assert_eq!(motion.origin(), 28.0);

        // Switching to `"snap"` mid-slide ends it at its target and asks for a frame
        // too: the link sleeps without drawing on a settled animation.
        let mut motion = after_enter();
        motion.advance(TICK);
        assert!(
            motion.set_style(CursorMotion::Snap),
            "no frame was asked for"
        );
        assert!(motion.settled());
        assert_eq!(motion.origin(), 26.0);
    }

    #[test]
    fn switching_style_mid_slide_does_not_teleport_the_origin() {
        // The same rule as the cursor's (`switching_style_in_flight_does_not_teleport`):
        // a style change is not a target change, the grid must continue from where it
        // stands. The real risk is in `ease` because it remembers its start point — had
        // `from` not been refreshed the content would jump back to the old start.
        let mut motion = after_enter();
        for _ in 0..8 {
            motion.advance(TICK);
        }
        let before = motion.origin();
        assert!(
            before < 27.0 && before > 26.0,
            "the scenario was not set up: {before}"
        );

        motion.set_style(CursorMotion::Ease);
        assert_eq!(motion.origin(), before, "the style change teleported");
        run_to_rest(&mut motion, TICK);
        assert_eq!(motion.origin(), 26.0);
    }

    #[test]
    fn covering_the_window_settles_the_slide_too() {
        // The offset half of `Motion::finish`. When occluded the link stops, so
        // `advance` will not run again; had half been finished `settled()` would stay
        // `false` forever and the timed run would say `MotionUnsettled` **while the code
        // is right**.
        let mut motion = after_enter();
        motion.advance(TICK);
        assert!(!motion.settled());

        motion.finish();
        assert!(motion.settled(), "the finished slide did not settle");
        assert_eq!(motion.origin(), 26.0);
    }

    /// A notch request of `rows` rows, in generation `generation`.
    fn notch(rows: f32, generation: u32) -> ScrollGlide {
        ScrollGlide { rows, generation }
    }

    /// The link's per-frame order (`link.rs`): first the elapsed time, then taking the
    /// share. The return is this frame's share.
    fn glide_frame(motion: &mut Motion) -> f32 {
        motion.advance(TICK);
        motion.take_glide().rows
    }

    /// Delivers frame by frame until the glide ends; returns the sum of the shares and
    /// the biggest single share.
    fn deliver_to_rest(motion: &mut Motion) -> (f32, f32) {
        let (mut sum, mut largest) = (0.0_f32, 0.0_f32);
        for _ in 0..10_000 {
            let share = glide_frame(motion);
            sum += share;
            largest = largest.max(share.abs());
            if motion.glide_idle() {
                return (sum, largest);
            }
        }
        panic!("the glide did not settle");
    }

    #[test]
    fn a_glide_delivers_exactly_the_rows_it_was_asked_for() {
        // **The contract:** the window goes exactly as many rows as the notch
        // asked — the shares enter `Session::frame` frame by frame and had their sum
        // been short or over, every notch would shift the window by a fraction and leave
        // it there. In both sliding styles, overshoot-free.
        for style in [CursorMotion::Spring, CursorMotion::Ease] {
            let mut motion = Motion::default();
            motion.set_style(style);
            motion.request_glide(notch(3.0, 0));
            assert!(
                !motion.settled(),
                "{style:?}: the request did not wake the link"
            );
            // The request comes **after** this frame's `advance`: the first share is zero,
            // so the clamped `dt` of a link waking from sleep does not deliver half the
            // request in a single frame.
            assert_eq!(
                motion.take_glide().rows,
                0.0,
                "{style:?}: the first frame jumped"
            );

            let mut shares = Vec::new();
            while !motion.glide_idle() {
                shares.push(glide_frame(&mut motion));
                assert!(shares.len() < 10_000, "{style:?}: the glide did not settle");
            }
            let sum: f32 = shares.iter().sum();
            assert!((sum - 3.0).abs() < 1e-5, "{style:?}: total {sum}");
            assert!(
                shares.iter().all(|&share| share >= 0.0),
                "{style:?}: the glide recoiled: {shares:?}"
            );
            // The glide **glides**: had a single frame carried most of the request the notch
            // would be a jump again.
            assert!(shares.len() > 5, "{style:?}: {} frames", shares.len());
            assert!(
                motion.settled(),
                "{style:?}: the settled glide does not let it sleep"
            );
            assert_eq!(
                motion.take_glide().rows,
                0.0,
                "{style:?}: a share was left after settling"
            );
        }
    }

    #[test]
    fn a_second_notch_joins_the_glide_in_flight() {
        // The wheel's notches pile up: the second is added to the first's **remainder**,
        // it does not start afresh — otherwise the first notch's undelivered share would
        // be lost.
        let mut motion = Motion::default();
        motion.request_glide(notch(1.0, 0));
        let mut sum = 0.0;
        for _ in 0..4 {
            sum += glide_frame(&mut motion);
        }
        assert!(sum > 0.0 && sum < 1.0, "the scenario was not set up: {sum}");
        motion.request_glide(notch(1.0, 0));
        let (rest, _) = deliver_to_rest(&mut motion);
        assert!((sum + rest - 2.0).abs() < 1e-5, "total {}", sum + rest);
    }

    #[test]
    fn snap_and_reduce_motion_deliver_the_glide_at_once() {
        // A notch does not add animation for a user who has turned motion off; under
        // Reduce Motion it snaps like the offset ([`Motion::origin_mode`]). The request
        // is delivered in full **in the same frame** and the link does not stay awake.
        let mut motion = Motion::default();
        motion.set_style(CursorMotion::Snap);
        motion.request_glide(notch(3.0, 0));
        assert_eq!(motion.take_glide().rows, 3.0);
        assert!(motion.settled(), "a glide was left under snap");

        let mut motion = Motion::default();
        motion.set_reduce(true);
        motion.request_glide(notch(-2.0, 0));
        assert_eq!(motion.take_glide().rows, -2.0);
        assert!(motion.settled(), "a glide was left under Reduce Motion");
    }

    #[test]
    fn a_new_generation_drops_the_glide_in_flight() {
        // The position was reset from outside (return to bottom on input, Shift+PgUp):
        // the remaining share must not pull the window back that has returned to the
        // bottom. It is **dropped**, not delivered — the return to bottom is already
        // where it wants to go.
        let mut motion = Motion::default();
        motion.request_glide(notch(5.0, 0));
        glide_frame(&mut motion);
        glide_frame(&mut motion);
        assert!(!motion.glide_idle());

        motion.observe_scroll_generation(1);
        assert!(
            motion.glide_idle(),
            "the new generation did not end the glide"
        );
        assert!(motion.settled());
        assert_eq!(
            motion.take_glide(),
            notch(0.0, 1),
            "the dropped share was delivered"
        );

        // The same generation is a no-op: every content frame reports the generation.
        motion.request_glide(notch(1.0, 1));
        motion.observe_scroll_generation(1);
        assert!(!motion.glide_idle(), "the same generation ended the glide");

        // A request from a new generation does not carry the old one's remainder: only
        // itself is delivered.
        motion.request_glide(notch(2.0, 2));
        let (sum, _) = deliver_to_rest(&mut motion);
        assert!(
            (sum - 2.0).abs() < 1e-5,
            "the old generation's share was carried over: {sum}"
        );
        assert_eq!(motion.take_glide().generation, 2);
    }

    #[test]
    fn a_glide_that_hits_the_edge_ends() {
        // A wheel flung down at the bottom: the remaining share hits the clamp and the
        // position does not move. The glide must end there — otherwise it draws empty
        // content frames until it settles and swallows the next upward notch.
        let mut motion = Motion::default();
        motion.observe_scroll(0, (0, 0.0), 0.0);
        motion.request_glide(notch(-30.0, 0));
        let share = glide_frame(&mut motion);
        assert!(share < 0.0);
        // `bt-core` scrolled two rows: the position changed, the glide continues.
        motion.observe_scroll(0, (2, 0.0), share);
        assert!(!motion.glide_idle(), "a glide that moved ended");
        let share = glide_frame(&mut motion);
        motion.observe_scroll(0, (2, 0.0), share);
        assert!(motion.glide_idle(), "a glide that hit the end continued");
        assert!(motion.settled());

        // A notch in the opposite direction is delivered in full: the remainder at the
        // end does not eat it.
        motion.request_glide(notch(3.0, 0));
        let (sum, _) = deliver_to_rest(&mut motion);
        assert!(
            (sum - 3.0).abs() < 1e-5,
            "the upward notch was eaten: {sum}"
        );

        // A frame whose share is zero (the request's first frame, a settled window) does
        // not move the position but does not end anything either.
        motion.request_glide(notch(1.0, 0));
        motion.observe_scroll(0, (2, 0.0), 0.0);
        motion.observe_scroll(0, (2, 0.0), 0.0);
        assert!(!motion.glide_idle(), "a zero share ended the glide");
    }

    #[test]
    fn finishing_a_glide_delivers_what_is_left() {
        // Occlusion, the switch to `snap` and Reduce Motion end the glide **at its
        // target**: had the remaining share been dropped the window would rest in the
        // middle of a row — the end of the gesture would not keep the "settles on the
        // nearest row" promise. The remaining share is delivered in the next content
        // frame; until then `settled()` is `false`, because an undelivered share is work
        // the link cannot sleep through.
        type Finisher = fn(&mut Motion) -> bool;
        let finishers: [(&str, Finisher); 3] = [
            ("finish", |motion| {
                motion.finish();
                true
            }),
            ("snap", |motion| motion.set_style(CursorMotion::Snap)),
            ("reduce", |motion| motion.set_reduce(true)),
        ];
        for (name, finisher) in finishers {
            let mut motion = Motion::default();
            motion.request_glide(notch(4.0, 0));
            let mut sum = glide_frame(&mut motion) + glide_frame(&mut motion);
            assert!(
                sum > 0.0 && sum < 4.0,
                "{name}: the scenario was not set up: {sum}"
            );

            assert!(finisher(&mut motion), "{name}: no frame was asked for");
            assert!(
                !motion.settled(),
                "{name}: the pending share put the link to sleep"
            );
            sum += motion.take_glide().rows;
            assert!((sum - 4.0).abs() < 1e-5, "{name}: total {sum}");
            assert!(motion.settled(), "{name}: did not settle after delivery");
        }
    }

    #[test]
    fn scrolling_does_not_end_the_glide() {
        // The glide moves the offset at every row boundary and `sync`'s offset
        // snap belongs to the cursor and the offset. Had it touched the glide, a notch
        // that passed the first row would be cut there.
        let mut motion = Motion::default();
        motion.sync(Some([0.0, 29.0]), 0, 0.0, 0, false, false);
        motion.request_glide(notch(3.0, 0));
        let mut sum = 0.0;
        for offset in 1..4 {
            sum += glide_frame(&mut motion);
            motion.sync(Some([0.0, 29.0]), 0, 0.0, offset, false, false);
            assert!(!motion.glide_idle(), "the offset change ended the glide");
        }
        let (rest, _) = deliver_to_rest(&mut motion);
        assert!((sum + rest - 3.0).abs() < 1e-5, "total {}", sum + rest);
    }

    #[test]
    fn switching_style_mid_glide_does_not_jump() {
        // The same rule as the offset and the cursor: `ease` remembers its start point
        // and unless it is refreshed it would restart from the old start — it would be
        // added once more on top of the share already delivered.
        let mut motion = Motion::default();
        motion.request_glide(notch(3.0, 0));
        let mut sum = 0.0;
        for _ in 0..3 {
            sum += glide_frame(&mut motion);
        }
        assert!(
            !motion.set_style(CursorMotion::Ease),
            "the takeover asked for a frame"
        );
        let (rest, largest) = deliver_to_rest(&mut motion);
        assert!((sum + rest - 3.0).abs() < 1e-5, "total {}", sum + rest);
        assert!(largest < 1.0, "the style change caused a jump: {largest}");
    }

    #[test]
    fn a_hidden_cursor_keeps_the_scroll_history() {
        // The offset must live apart from `state`: a TUI hides the cursor, scrolls the
        // window, then turns it back on. Had the offset emptied, the cursor turning back
        // on would say "there was no scroll" and take the scroll for an animation — here
        // it is a snap anyway so there is no symptom, but in the opposite direction
        // (when there was **no** scroll while hidden) it would produce a wrong snap.
        let mut motion = Motion::default();
        motion.sync(None, 0, 0.0, 5, false, false);
        motion.sync(Some([0.0, 0.0]), 0, 0.0, 5, false, false);
        assert_eq!(motion.offset, Some(5));
    }

    // ---- The dock band's extra rows ----

    /// The frame that starts the band from one row (extra `0`): the cursor and offset
    /// are also settled. `style` decides the mode, `reduce` is Reduce Motion.
    fn band_at_rest(style: CursorMotion, reduce: bool) -> Motion {
        let mut motion = Motion::default();
        motion.set_style(style);
        motion.set_reduce(reduce);
        motion.sync(Some([0.0, 29.0]), 26, 0.0, 0, false, false);
        assert!(motion.settled(), "the first frame started an animation");
        motion
    }

    #[test]
    fn the_band_slides_both_ways_and_keeps_the_link_awake() {
        // The band's extra rows are in their own `Slide` and glide in **both
        // directions** — the panel's size is not content, the direction rule does
        // not fit it. `settled()` is wrong before it settles: left outside, the link
        // would sleep in the middle of the growth and the grid and the band would
        // freeze halfway.
        let mut motion = band_at_rest(CursorMotion::Spring, false);
        motion.sync(Some([0.0, 29.0]), 26, 2.0, 0, false, false);
        assert!(!motion.settled(), "the growing band was counted as settled");
        assert_eq!(motion.band(), 0.0, "the band jumped in sync");
        motion.advance(TICK);
        let mid = motion.band();
        assert!(mid > 0.0 && mid < 2.0, "the band does not glide: {mid}");
        run_to_rest(&mut motion, TICK);
        assert_eq!(motion.band(), 2.0);

        // Shrinking glides too: when a line is deleted the band must not vanish with the
        // grid jumping.
        motion.sync(Some([0.0, 29.0]), 26, 0.0, 0, false, false);
        assert!(!motion.settled(), "the shrinking band was snapped");
        motion.advance(TICK);
        assert!(motion.band() < 2.0 && motion.band() > 0.0);
        run_to_rest(&mut motion, TICK);
        assert_eq!(motion.band(), 0.0);
    }

    #[test]
    fn a_negative_band_slides_both_ways_and_snaps_when_asked() {
        // **Remote session**: when the input line goes away the band's
        // extra is negative and fractional (a cell plus the line gap). There is no
        // direction rule: the band glides in two directions with an input line of 1 → 0
        // → 1 and settles; instantly under `snap` and Reduce Motion.
        const REMOTE: f32 = -34.0 / 18.0;
        let mut motion = band_at_rest(CursorMotion::Spring, false);
        motion.sync(Some([0.0, 29.0]), 26, REMOTE, 0, false, false);
        assert!(
            !motion.settled(),
            "the shortening band was counted as settled"
        );
        motion.advance(TICK);
        let mid = motion.band();
        assert!(mid < 0.0 && mid > REMOTE, "the band does not glide: {mid}");
        run_to_rest(&mut motion, TICK);
        assert_eq!(motion.band(), REMOTE);

        motion.sync(Some([0.0, 29.0]), 26, 0.0, 0, false, false);
        assert!(!motion.settled(), "the returning row was snapped");
        motion.advance(TICK);
        assert!(motion.band() > REMOTE && motion.band() < 0.0);
        run_to_rest(&mut motion, TICK);
        assert_eq!(motion.band(), 0.0);

        for (style, reduce) in [(CursorMotion::Snap, false), (CursorMotion::Spring, true)] {
            let mut motion = band_at_rest(style, reduce);
            motion.sync(Some([0.0, 29.0]), 26, REMOTE, 0, false, false);
            assert_eq!(motion.band(), REMOTE, "{style:?}/{reduce}");
            assert!(
                motion.settled(),
                "{style:?}/{reduce}: a frame was asked for"
            );
        }
    }

    #[test]
    fn snap_reduce_motion_and_geometry_snap_the_band() {
        // Reduce Motion and `cursor_motion = "snap"` seat the band in one frame (like the
        // offset — the band does not fade either); a geometry change does in every mode.
        for (style, reduce) in [(CursorMotion::Snap, false), (CursorMotion::Spring, true)] {
            let mut motion = band_at_rest(style, reduce);
            motion.sync(Some([0.0, 29.0]), 26, 3.0, 0, false, false);
            assert_eq!(motion.band(), 3.0, "{style:?}/{reduce}: the band slid");
            assert!(
                motion.settled(),
                "{style:?}/{reduce}: a frame was asked for"
            );
        }
        let mut motion = band_at_rest(CursorMotion::Spring, false);
        motion.sync(Some([0.0, 29.0]), 26, 3.0, 0, true, false);
        assert_eq!(motion.band(), 3.0, "geometry glided the band");
        assert!(motion.settled());
        // The wheel does not snap the band: scrolling does not change the dock's row
        // count.
        let mut motion = band_at_rest(CursorMotion::Spring, false);
        motion.sync(Some([0.0, 29.0]), 26, 3.0, 4, false, false);
        assert!(!motion.settled(), "the wheel snapped the band");
    }

    #[test]
    fn a_band_change_lets_the_rising_content_target_glide() {
        // **The direction rule's second exception**: in the frame where the band's
        // target changes, the rising content target glides too. As rows pass from the
        // grid to the dock the suppressed rows drop out of the fill (the target rises)
        // and the band grows by that much; had either of them snapped the grid would
        // jump in one frame while the other glided.
        let mut motion = band_at_rest(CursorMotion::Spring, false);
        // Band 0 → 2 and in the same frame the offset 26 → 28 (rising).
        motion.sync(Some([0.0, 29.0]), 28, 2.0, 0, false, false);
        assert!(!motion.origin_settled(), "the rising target was snapped");
        // The drawn origin is `origin − band` — the two curves have the same physics and
        // the same distance, so they cancel each other: the grid stands still.
        for _ in 0..8 {
            motion.advance(TICK);
            let drawn = motion.origin() - motion.band();
            assert!((drawn - 26.0).abs() < 1e-4, "the grid moved: {drawn}");
        }
        // In a frame where the band does not change the rule stands: the rising target
        // snaps.
        run_to_rest(&mut motion, TICK);
        motion.sync(Some([0.0, 29.0]), 29, 2.0, 0, false, false);
        assert!(
            motion.origin_settled(),
            "the band did not change but the target glided"
        );
    }
}
