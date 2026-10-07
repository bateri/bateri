//! What drives the frame: a platform [`Pacer`] (the vsync tick and its
//! switch), the platform-free [`Ticker::tick`] it calls, and the [`Waker`]
//! that switches it on from afar.
//!
//! The contract in one sentence: **the pacer stays paused.** It starts when
//! new content arrives (`Wake::wake` → [`Waker`]) and the tick pauses it
//! again once damage **and motion** run out. "Zero frames at idle" lives in
//! those two lines; every `set_running(true)` needs a reason and every frame
//! carries a stop condition.
//!
//! **The platform's four jobs are behind [`Pacer`]**: the
//! vsync tick, `set_running` from any thread, one delayed wakeup, and the
//! time base (`now()`). Everything else — the frame's decision, drawing,
//! completion, the clock — is here and platform-free. The macOS pacer lives
//! in `bt-shell` (`NSView.displayLink` as a timer, `dispatch2` for the
//! delayed wakeup, `CACurrentMediaTime` for `now`); Linux's comes with the
//! winit set.
//!
//! **Three things ask for a frame**:
//!
//! - **Damage** — through the `Waker`, from another thread, by planting a
//!   flag.
//! - **Motion** ([`crate::motion`]) — without waking anyone, because the tick
//!   that is already running decides: while an animation has not settled,
//!   the tick refuses to sleep. **The notch glide takes this road too, only
//!   its drawing is on the other branch**: its request is motion's (no
//!   wakeup, no damage), but its share scrolls the window inside
//!   `Session::frame`, so a frame in flight is drawn as a **content** frame
//!   and counted in `content=` — for the reason the clock's content flavour
//!   has: what the grid draws really changes. Its stop condition is the
//!   glide's own settling.
//! - **The clock** (`Core::arm_clock`) — a single delayed wakeup armed as
//!   the pacer goes to sleep. **It has two flavours**, chosen by the kind of
//!   work that waits: the *content flavour* plants damage through
//!   [`Waker::wake`] (a running command's duration counter; the grid really
//!   changes, so counting it in `content=` is right), the *motion flavour*
//!   does not, through [`Waker::resume`] (the cursor's blink; only the
//!   caret's alpha changes — and the completion poll of a frame still in
//!   flight). The armed wakeup is still **one**: the nearest
//!   deadline wins ([`due_clock`]), because a delayed wakeup cannot be
//!   cancelled and a second one would invalidate the first's generation.
//!
//! **Motion must not touch the `Waker`:** [`Waker::wake`] plants the damage
//! flag unconditionally, so a motion frame requested there would count
//! itself as "content", rescan the grid for nothing and inflate the operand
//! of the zero-frames-at-idle gate (`content=`). Hence: **an animation's
//! time-driven frame request goes through the motion clock.** A new
//! animation (blink, smooth scrolling) enters there, not [`Waker::wake`].
//!
//! **The clock's content flavour is not an exception to that ban, it is
//! something else.** An animation draws the same content differently; the
//! clock changes **the content itself** (the running command's counter: what
//! the grid draws really differs). So going through `Waker::wake` and
//! counting in `content=` is **right** — the ban protected the opposite. The
//! test has three conditions: the content really changes, its period is
//! **much** longer than the refresh, and it carries a **named stop
//! condition**. A time-driven request that fails any of them cannot go there.
//!
//! **Blink fails the first, and that is why the motion flavour exists.** The
//! grid does not change, only the caret's alpha — so blink is a motion frame.
//! But it cannot be tied to the refresh either (a frame per refresh for a
//! 2 Hz change), so it lives in its own type ([`crate::blink`]), not inside
//! `Motion`, and its trigger is the clock. [`Waker::resume`] plants no
//! damage, so the woken tick lands on the "no damage" branch and draws the
//! motion frame there — no grid scan, no `Term` lock, no trip into
//! `bt-core`. **The sleep test therefore asks three questions** (four since
//! the typing effects — they live outside `Motion` too, below): blink lives
//! outside `Motion`, so `settled()` does not see it, and without asking
//! about a pending phase change `resume` would create a wake/sleep spin that
//! draws nothing.
//!
//! **The dock's typing effects take the motion road too**
//! ([`crate::glyph_fx`]): they live outside `Motion` (blink's precedent) and
//! enter the sleep test under their own named term — while an arrival or a
//! ghost is in flight the pacer does not sleep, when the list empties it
//! does. They plant no damage: a frame an effect keeps alive raises `frames`,
//! not `content`.
//!
//! **The scroll bar takes the motion road too** ([`crate::scrollbar`]): it
//! lives outside `Motion`, enters the sleep test under its own term, and its
//! one-second hold after the last scrolling input is **spent asleep** — the
//! clock wakes the link at the hold's end in the motion flavour and the fade
//! is drawn as motion frames. What shows it is scrolling **input**
//! ([`DisplayLink::poke_scrollbar`], through [`Waker::resume`]), never
//! output; a change of its form ([`DisplayLink::set_scrollbar_mode`]) and the
//! pointer over its strip ([`DisplayLink::set_scrollbar_hover`],
//! [`DisplayLink::set_scrollbar_drag`]) go the same way. The always-up form
//! has no timeline: it is drawn with the content and puts nothing in flight
//! but the pointer's tone. A bar the pointer holds is up and settled: no
//! clock, no frame until the pointer moves away. **Its marks are not motion**:
//! a search pass that changes them asks for one content frame
//! ([`DisplayLink::marks_changed`], [`Waker::wake`]) while the bar is up, and
//! a bar that was down when they changed asks for it as it comes up — the
//! marks are bucketed in `bt-core`'s frame, which a motion frame never
//! reaches.
//!
//! The contract's consequence in one sentence: a window with a running
//! command, **a blinking cursor or a scroll bar shown by scrolling** is **not
//! idle**; every other window — one whose bar is always up included — is idle
//! and draws zero frames. All carry a named stop condition — the command
//! ends; blink is off by default and even when on stops after keyboard
//! silence ([`crate::blink::Blink`]); the bar fades out a second after the
//! last scrolling input.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use bt_core::{
    BlockHandle, Blocks, CaretStyle, Clusters, Cursor, CursorMotion, DirtyFlag, DockBudget,
    DockCols, DockContext, DockState, Erase, Keypress, LinearRgba, SearchRuns, SelectionRun,
    SelectionRuns, Session, Theme, TrackBlock, TrackMarks,
};

use crate::blink::Blink;
use crate::frame::Frame;
use crate::glyph_fx::GlyphFx;
use crate::metrics::CellMetrics;
use crate::motion::Motion;
use crate::scrollbar::{Look, Mode, Scrollbar, ScrollbarLayout};
use crate::stats::Stats;
use crate::surface::{self, Acquired};
use crate::{GpuError, Renderer, Surface};

/// The platform's side of the frame loop — **four jobs**.
///
/// 1. **The vsync tick.** The pacer calls [`Ticker::tick`] once per display
///    refresh while running, on the thread that created the
///    [`DisplayLink`] (the `Ticker` is not `Send`, so the type holds it
///    there). [`Pacer::stop`] is this job's teardown: the tick source is
///    torn down for good (on macOS: invalidated and off the run loop).
/// 2. **[`Pacer::set_running`] from any thread.** **Starting is
///    asynchronous**: a start lands after the current tick has returned,
///    even when asked from the tick's own thread. The frame policy relies on
///    it — `Retry` asks for one more frame from inside a tick that may pause
///    before returning, and a synchronous start would be swallowed by that
///    pause. **Pausing is immediate** when asked from the tick's thread. A
///    start may race with the visibility gate closing; the tick's first
///    check (a closed gate pauses) settles it, and under the timer-only
///    provider that stray tick costs no drawable (acquisition is the tick's,
///    after the gate). After [`Pacer::stop`], starting does nothing.
/// 3. **[`Pacer::after`] — one delayed wakeup.** `wake` runs once after
///    `delay`, on any thread; it cannot be cancelled, which is why the
///    clock keeps a generation ([`Core::arm_clock`]).
/// 4. **[`Pacer::now`] — the time base.** Seconds on **the same base as the
///    tick's stamp**. The stamp is the target presentation time when the
///    provider knows it (macOS's `targetTimestamp`), otherwise `now()`.
///    `dt`, the content deadline, blink, the clock's delay and `quiet=` all
///    read this one base; a second clock would create two times (the ban is
///    `Core::last_update_at`'s).
pub trait Pacer: Send + Sync {
    /// Starts or pauses the tick (job 2).
    fn set_running(&self, running: bool);
    /// Runs `wake` once after `delay` (job 3).
    fn after(&self, delay: Duration, wake: Box<dyn FnOnce() + Send>);
    /// Now, on the tick stamps' base (job 4).
    fn now(&self) -> f64;
    /// Tears the tick down for good (job 1's end).
    fn stop(&self);
}

/// Where this tick draws — the seam of the two texture providers.
///
/// Today only (b): the pacer is a timer and the frame takes its texture from
/// the window's [`Surface`]. The (a) provider (a display link handing over a
/// drawable, wrapped with `create_texture_from_hal`) would add a variant
/// carrying that texture, if the window path's measurement asks for it;
/// no variant is written ahead of its provider.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TickTarget {
    /// Acquire this frame's texture from the window's surface.
    Surface,
}

/// **The single definition of asking for a frame from damage**: plant the
/// damage flag, start the pacer.
///
/// Callable from any thread; `Clone`, `Send + Sync`.
///
/// **There is a second door, on purpose** ([`Waker::resume`]): it starts
/// without planting damage. For a long time the text said "a path doing the
/// two separately does not exist on purpose", and the reason was right — a
/// pacer started without the flag says "no damage" and goes straight back to
/// sleep. Blink **answered** that reason: the tick's "no damage" branch can now
/// have work to do (blink's phase change), so the pacer does not wake for
/// nothing. The scroll bar's appearing and fading is the same kind of work —
/// only the bar changes, never the content. Those are the legitimate reasons
/// to start without the flag: work the "no damage" branch itself draws.
///
/// **An animation does not ask [`Waker::wake`] for frames** (module header):
/// motion is the running tick's own decision. An animation wired to this
/// door would plant damage on every frame and fill the `content=` counter —
/// the zero-frames-at-idle gate — with its own frames. The ban's subject is
/// **this function**, not the type.
///
/// **The clock does go through here** (`Core::arm_clock`) and it is not a
/// contradiction: the counter's tick really changes the content, so counting
/// it in `content=` is right. The three conditions are in the module header.
#[derive(Clone)]
pub struct Waker {
    inner: Arc<WakerInner>,
}

struct WakerInner {
    /// The damage flag — **not** the session itself.
    ///
    /// `Arc<Session>` (even `Weak`, since `upgrade()` materialises it for the
    /// call) cannot be here: the reader thread holds this body too, and if
    /// the last strong reference dropped there, `Drop for Session` →
    /// `shutdown()` would run on that thread — `join` is on a separate,
    /// bounded thread now, so not a panic, but a half-second stall and a
    /// shutdown that never finishes. `wake.rs`'s ownership paragraph bans it
    /// by name.
    dirty: DirtyFlag,
    /// The platform's tick switch and clock ([`Pacer`]); every thread
    /// reaches it through here.
    ///
    /// **Its drop may block off the main thread** on macOS (the pacer holds
    /// a main-thread-bound display link, whose drop synchronously hops to
    /// the main queue). The shutdown path therefore stops the link first and
    /// does not drop the `DisplayLink` **while waiting**: the last reference
    /// stays on the main thread. A non-waiting shutdown (one pane closing in
    /// `bt-shell`) may drop it; `bt-shell` removes the reader side's copy on
    /// close.
    pacer: Arc<dyn Pacer>,
    /// Whether frames are drawn and the rhythm turns.
    gate: Gate,
    /// The frame **request** counter — a deeper measure of zero frames at
    /// idle than `frames`.
    ///
    /// `frames` counts what the GPU finished without error: requests born and
    /// dying in the [`Gate`] or merged on our side are invisible to it. This
    /// counter rises **before** the gate, so it counts the request itself —
    /// on an occluded window the gate swallows the frame but the request
    /// leaves a trace here.
    ///
    /// **What it counts:** *every* call to [`Waker::wake`]. So not only shell
    /// output: [`Retry::draw_failed`]'s retry, [`DisplayLink::resize`]'s
    /// unconditional request and the reader's last wakeups after the
    /// `stopped` latch dropped are written here too. The number is "frames
    /// asked for", not "requests that can produce a frame"; a permanent draw
    /// error inflates it and the reader tells them apart by comparing with
    /// `frames`.
    ///
    /// **What it does not count: motion frames** — neither those the running
    /// tick draws on its own decision nor those the clock's motion flavour
    /// ([`Waker::resume`]) wakes; `resume` does not touch this counter on
    /// purpose. An animation never touches [`Waker::wake`] (module header), so
    /// this counter stays close to `content` while `frames` drifts away from it
    /// during an animation. The `requests ≈ frames + 2` relation of the "smoke
    /// load" measurement below **stopped holding once motion frames existed** for exactly that
    /// reason; the numbers themselves (that day's observations) stay, the new
    /// form was **measured** through `content` (thirty healthy
    /// runs): `requests` was `4` in all thirty while `content` was `2`–`3` and
    /// `frames` 27–30. The counter is still **not constant** — a later run gave
    /// `3`, probably because of the coalescing; not measured.
    ///
    /// A **counter, not a gate**: its threshold was not measured and an
    /// unmeasured number is not written into a gate (the rule of `slots=`).
    /// What was measured (2026-09-12, debug, this machine) shows two separate
    /// regimes, and both say why the counter is a separate number:
    ///
    /// - **Smoke load** — in a healthy run `frames=1–2` while `requests=2–3`; with
    ///   zero frames at idle broken on purpose `frames=82–354`, `requests=84–357`.
    ///   They move together, so here it is no more telling than `frames`.
    /// - **Measurement load** — `frames=9` (2 s) / `21` (5 s) while `requests` is
    ///   **25 000–72 000**. Frames do not flow but requests do: `frames` cannot
    ///   see the three orders of magnitude in between.
    ///
    /// I **did not measure the mechanism** of the second regime (the gate
    /// swallowing, the main thread saturating, the system throttling the
    /// link); I wrote the two numbers seen from outside.
    ///
    /// `Relaxed`, because it orders nothing — it is read once at shutdown.
    ///
    /// **Not gated, and the cost was measured.** The only reader is the timed
    /// run (`report_and_exit`), so nobody looks in an interactive session; no
    /// gate was fitted anyway, because the cost is not above the gate's own:
    /// the highest wakeup rate measured is **~15 000/s** (45 167 requests /
    /// 3 s, measurement load), which is one more `fetch_add` per wakeup on a
    /// cache line the pacer's coalescing already dirties — **ten microseconds
    /// a second** in order of magnitude. An `Option` branch costs the same
    /// order, and adds a parameter to `DisplayLink::new`.
    requests: AtomicU64,
}

impl Waker {
    /// Callable from any thread; hands the start to the pacer and **returns
    /// at once**. By the `Wake` contract it does not block or take a lock.
    ///
    /// The pacer coalesces starts (the macOS one drops a start while one is
    /// queued) and that is lossless: the flag is planted on every call, and
    /// a queued start opens the tick, which reads the flag.
    pub fn wake(&self) {
        // The counter comes **before** the gate and the flag: what we want
        // to measure is the request itself, not what the gate does with it.
        self.inner.requests.fetch_add(1, Ordering::Relaxed);
        // Damage is ALWAYS planted; while invisible only the pacer is not
        // started. The flag is not consumed, so when visibility returns the
        // accumulated damage is drawn.
        self.inner.dirty.mark();
        if !self.inner.gate.is_open() {
            return;
        }
        self.inner.pacer.set_running(true);
    }

    /// Starts the pacer **without planting damage** — the clock's second
    /// flavour, and the scroll bar's poke ([`DisplayLink::poke_scrollbar`]),
    /// form and pointer ([`DisplayLink::set_scrollbar_hover`]).
    ///
    /// [`Waker::wake`] with its middle job removed: the same gate, the same
    /// start; no `dirty.mark()`. The woken tick therefore lands on the "no
    /// damage" branch and draws a **motion** frame there — the grid is not
    /// rescanned, the `Term` lock is not taken, `content=` does not rise.
    ///
    /// `requests` does not rise either: that counter's contract is "content
    /// frames asked for" and it leaves motion frames out on purpose (its
    /// doc).
    ///
    /// **Sharing the pacer's coalescing with `wake` is lossless:** both end
    /// in the same start; `wake` plants its flag **before** the gate,
    /// unconditionally, so a `resume` merged into it loses no damage, and the
    /// reverse loses no start.
    pub(crate) fn resume(&self) {
        if !self.inner.gate.is_open() {
            return;
        }
        self.inner.pacer.set_running(true);
    }

    /// Plants the damage flag again without starting — a frame that could
    /// not be drawn (no texture this tick) keeps its content for the next
    /// request.
    fn keep_damage(&self) {
        self.inner.dirty.mark();
    }

    fn gate(&self) -> &Gate {
        &self.inner.gate
    }

    fn pacer(&self) -> &dyn Pacer {
        &*self.inner.pacer
    }

    /// Frame requests so far.
    fn requests(&self) -> u64 {
        self.inner.requests.load(Ordering::Relaxed)
    }
}

/// The drawn frame's vertical origin, in pixels — **the frame path writes,
/// the mouse path reads**. The fill band's height (rows) travels with it.
///
/// **The band is part of the origin's geometry**, not a second subject: the
/// band's own viewport derives from here (`Frame::fill_origin_px`,
/// `origin_px − fill_px`), and what the mouse mapping asks is what lies
/// **above** the origin — a gap or history. Kept in separate bodies, the
/// two values could be published in two different frames and a mouse
/// translation with a new origin and an old band would be born.
///
/// The value has **one owner**, [`DisplayLink`]: `Session::frame` computes it
/// (`bt_core::Cursor::content_rows`, one computation), but the frame path is
/// what turns it into an offset and draws it, so the reading side must read
/// from there too — a second computation would be a drift that shows as "the
/// mouse is one row off".
///
/// **Not atomic, a `Cell`**, and that is not a shortcut but a fact: the tick
/// runs on the thread that created the link — the main thread, where the
/// macOS pacer's timer is on the main run loop — and `point_to_cell`'s
/// caller (an `NSView` mouse event) is on the main thread too. Both sides are
/// on one thread, so there is no race. Escaping to `Arc<AtomicU32>` would
/// write as if atomics were needed and bring back `make test-yaris`'s "shared
/// state" trigger without reason.
///
/// So `Rc` is not `Send`, and must not be: the type itself says "main
/// thread".
///
/// The value read is **the last encoded frame's** origin. Not staleness but
/// design: a click lands on a pixel on screen and that pixel was drawn in
/// that frame. The publication is therefore in `draw`'s `Ok` arm
/// (`Core::publish_origin`) — a frame that could not be encoded changed
/// nothing on screen, and publishing its offset would translate the mouse
/// against an invisible grid. "Encoded", not "drawn": `Ok` means submitted,
/// not presented, and asynchronous completion can still fail — the
/// remaining window is one frame, because `draw_failed` plants the damage
/// flag again and the next frame is drawn again with the same offset.
#[derive(Clone, Default)]
pub struct Origin(Rc<Published>);

/// What [`Origin`] holds: one drawn frame's publication, written in one
/// call ([`Origin::set`]).
///
/// **The block marks beside the geometry, not in it**: `Drawn` is copied out
/// whole on every mouse event, and the marks are a list — up to a mark per
/// bucket of the track. A list kept next to the copied geometry, written in
/// the same call, is still one frame's picture: both are written on the
/// frame path and read by the mouse on the same thread, never in between.
#[derive(Default)]
struct Published {
    drawn: Cell<Drawn>,
    /// The block marks the frame **drew** (`bt_core::TrackMarks::blocks`);
    /// empty when it drew none — a thin bar has no block lane.
    blocks: RefCell<Vec<TrackBlock>>,
}

/// [`Origin`]'s geometry: one frame's, published together.
#[derive(Clone, Copy, Default)]
struct Drawn {
    px: f32,
    fill_rows: u16,
    /// The top of the dock's input block (physical pixels, from the top) and
    /// the number of input rows; `None` → no dock in this frame.
    dock: Option<(f32, u16)>,
    /// The scroll bar's layout the frame was drawn with.
    scrollbar: ScrollbarLayout,
}

impl Origin {
    /// The drawn frame's vertical origin, **physical pixels** — the scroll
    /// fraction included (`Frame::set_scroll_frac`), so the mouse reads the
    /// grid where it was drawn.
    pub fn px(&self) -> f32 {
        self.0.drawn.get().px
    }

    /// The height of the fill channel above the origin, **rows**: the band
    /// (`bt_core::Cursor::fill`) plus the scroll fraction's top row
    /// (`bt_core::Cursor::top_row`). Zero means a gap there, otherwise
    /// history — in a fractional position the half row at the top cannot be
    /// selected either, like the band's.
    pub fn fill_rows(&self) -> u16 {
        self.0.drawn.get().fill_rows
    }

    /// The drawn frame's dock geometry: the top of the input block
    /// (**physical pixels**, from the texture's top) and the number of input
    /// rows; `None` → no dock, or never drawn yet.
    ///
    /// **In the same body as the origin** and for the same reason:
    /// while the band grows the grid moves up and the input block widens row
    /// by row, and published from separate frames a click could translate
    /// against a new origin and an old block. The value is the **layout's**
    /// (`Frame::dock_hit`): the text is bottom-aligned and stays in place
    /// through the animation.
    pub fn dock(&self) -> Option<(f32, u16)> {
        self.0.drawn.get().dock
    }

    /// The drawn frame's scroll bar layout — where the thumb, its track and
    /// the bar's strip are, and whether there is a bar at all.
    ///
    /// **In the same body as the origin** for the dock's reason: a drag
    /// translated against a new thumb and an old origin would grab a thumb
    /// that is not on screen. The layout is published whether the bar is
    /// showing or not — its strip is the bar's while there is something to
    /// scroll, and the mouse reads that from here, not from a second copy of
    /// the bar's arithmetic.
    pub fn scrollbar(&self) -> ScrollbarLayout {
        self.0.drawn.get().scrollbar
    }

    /// The drawn block mark under a point — physical pixels from the
    /// window's top-left — as its handle, for `bt_core::Session::block_info`,
    /// and its target ([`ScrollbarLayout::block_target`]); `None` off every
    /// mark, or when the frame drew none. The hit is the layout's
    /// ([`ScrollbarLayout::block_at`]) against the drawn marks, so the
    /// pointer takes what is on screen.
    pub fn block_at(&self, x: f32, y: f32) -> Option<(BlockHandle, [f32; 4])> {
        let layout = self.scrollbar();
        let blocks = self.0.blocks.borrow();
        let at = layout.block_at(x, y, blocks.iter().map(|block| block.position))?;
        blocks
            .get(at)
            .map(|block| (block.handle, layout.block_target(block.position)))
    }

    /// The drawn block marks' targets, `[x0, y0, x1, y1]` in physical
    /// pixels — where the pointer takes them
    /// ([`ScrollbarLayout::block_target`]): the hand cursor's rectangles.
    pub fn block_targets(&self) -> Vec<[f32; 4]> {
        let layout = self.scrollbar();
        self.0
            .blocks
            .borrow()
            .iter()
            .map(|block| layout.block_target(block.position))
            .collect()
    }

    /// Only the frame path writes; not `pub`, and must not be. `blocks` are
    /// the drawn block marks, or empty. `true` when where the pointer takes
    /// a mark changed — other marks, or the same ones on another layout
    /// (the track grew, the thumb they may lie on moved).
    fn set(
        &self,
        px: f32,
        fill_rows: u16,
        dock: Option<(f32, u16)>,
        scrollbar: ScrollbarLayout,
        blocks: &[TrackBlock],
    ) -> bool {
        let before = self.0.drawn.replace(Drawn {
            px,
            fill_rows,
            dock,
            scrollbar,
        });
        let mut kept = self.0.blocks.borrow_mut();
        let changed =
            kept.as_slice() != blocks || (!blocks.is_empty() && before.scrollbar != scrollbar);
        kept.clear();
        kept.extend_from_slice(blocks);
        changed
    }
}

/// The open/closed gate of asking for frames — **the whole stop policy**.
///
/// A separate type, like `FailureStreak` and for the same reason: lock-free
/// and platform-free, so it can be tested; embedded in the `Waker` it could
/// only be tried with a real window.
///
/// The gate is read on **both** sides: on the drawing side (the tick returns
/// early) and on the waking side ([`Waker::wake`]). Read only on the drawing
/// side, a chatty shell in an occluded window would start and stop the tick
/// at the refresh rate — no frame drawn, but a main-thread tick every vsync.
/// Drawing stops, the rhythm does not; the letter of the contract stays, its
/// spirit goes.
struct Gate {
    /// Whether the window is visible. Both ways:
    /// `windowDidChangeOcclusionState:` reports being occluded and coming
    /// back.
    open: AtomicBool,
    /// Permanent stop latch — drops once and never rises again.
    ///
    /// **Not** the same as `open = false`: the window delegate is not removed
    /// at shutdown, so a visibility notice arriving after `shutdown()` would
    /// reopen the gate and keep sending work to the waiting main thread.
    stopped: AtomicBool,
}

impl Gate {
    fn new() -> Self {
        Self {
            open: AtomicBool::new(true),
            stopped: AtomicBool::new(false),
        }
    }

    /// The latch is asked on the **reading** side, not the writing side:
    /// reading and writing the two flags separately (`set_open` misses the
    /// latch → `stop` runs → `set_open` opens the gate) would reopen a
    /// stopped gate, and that race would destroy the very reason the latch
    /// exists.
    fn is_open(&self) -> bool {
        !self.stopped.load(Ordering::Acquire) && self.open.load(Ordering::Acquire)
    }

    fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }

    /// A visibility notice. Order matters: writing this **first** when
    /// switching to `true` guarantees that the `request_frame` right after it
    /// passes the gate.
    fn set_open(&self, open: bool) {
        self.open.store(open, Ordering::Release);
    }

    /// Drops the latch; from then on the gate is closed whatever `set_open`
    /// writes.
    fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
    }
}

/// The policy for a frame that could not be drawn: **one** place.
///
/// A frame can fail in two places — before it is encoded (synchronous `Err`)
/// or on the GPU (asynchronous, seen by the completion poll) — but both are
/// the same class and want the same answer. Writing the policy twice has a
/// real cost: when the synchronous leg invented its own stop (a pause), it
/// could put the tick to sleep on top of a damage flag the reader had just
/// planted and swallow the frame. The stop is now single: the "no damage →
/// sleep" branch.
struct Retry {
    waker: Waker,
    streak: FailureStreak,
}

impl Retry {
    /// A frame could not be drawn. On the first error one more frame is
    /// requested; on the second in a row **nothing** is done and the stop
    /// kicks in by itself (see [`FailureStreak`]).
    ///
    /// Returns **whether the budget is spent** (`false` → one more frame was
    /// requested). On the damage path the caller need not look, the stop
    /// comes there from the flag not being planted; **on the motion path it
    /// must**, because the stop there is the animation settling, not damage.
    fn draw_failed(&self, e: &GpuError) -> bool {
        eprintln!("bateri: frame could not be drawn: {e}");
        if self.streak.failed() {
            self.waker.wake();
            return false;
        }
        true
    }
}

/// Frames that failed in a row — **the whole stop condition**.
///
/// A separate type because it had to be testable: the policy itself is
/// lock-free and platform-free; embedded in the `Waker` it could only be
/// tried with a real window.
#[derive(Default)]
struct FailureStreak(AtomicU32);

impl FailureStreak {
    /// A frame completed: the budget is given back.
    fn succeeded(&self) {
        self.0.store(0, Ordering::Release);
    }

    /// Reports an error. `true` → one more frame is requested. On the second
    /// error in a row `false`: the flag is not planted, so the next tick
    /// finds "no damage", pauses and waits for the next `Wakeup`. Without this
    /// a permanent error would turn "plant, try, fail" into an endless loop at
    /// the refresh rate.
    ///
    /// **Since motion frames exist this alone is not enough:** the "no damage" branch no
    /// longer sleeps unconditionally, it draws a motion frame while an
    /// animation has not settled. When the budget is spent that branch also
    /// finishes the animation ([`crate::motion::Motion::finish`]) — otherwise
    /// a permanent error would print an error line at the refresh rate until
    /// the slide's duration cap. Two stops together: the flag is not planted
    /// **and** no animation is left pending.
    fn failed(&self) -> bool {
        self.0.fetch_add(1, Ordering::AcqRel) == 0
    }
}

/// How long [`Core::arm_clock`] waits before polling a frame still in flight
/// as the link goes to sleep ("the last frame before sleep is not
/// lost").
///
/// A design constant, not a measurement: one refresh at 120 Hz, the fastest
/// display the product runs on. A frame is submitted at most one refresh
/// before the tick that goes to sleep, and the GPU's work for it is far
/// shorter than a refresh on this renderer's load, so one period is the
/// shortest wait that is not a busy poll. If the frame is still in flight,
/// the poll re-arms; the stop condition is an empty queue.
const POLL_DELAY: f64 = 1.0 / 120.0;

/// How long [`DisplayLink::drain`] waits for the frames still in flight at
/// shutdown before the report reads `frames=`.
///
/// A ceiling, not an expectation: the link is already stopped, so at most a
/// frame or two are in flight and they finish within a refresh; the ceiling
/// only bounds a GPU that hangs, well inside the smoke watchdog's budget.
const DRAIN_TIMEOUT: Duration = Duration::from_millis(100);

/// The frame loop's state — the old display-link delegate, now a plain type
/// behind an `Rc`. Main-thread state is in `Cell`/`RefCell`;
/// nothing here crosses threads except through the [`Waker`].
struct Core {
    /// `Rc`, not `Arc`: `Renderer` is not `Sync`. The removable reason is the
    /// glyph atlas's `CFRetained<CTFont>` (not `Send`), the **structural**
    /// reason the `RefCell` around the atlas — even with a fully thread-safe
    /// font, `Arc` would make a false promise of "may cross threads".
    /// `session` really crosses, so it stays `Arc`.
    renderer: Rc<Renderer>,
    /// The window's surface; `bt-shell` sizes it, the tick acquires from it.
    surface: Rc<Surface>,
    session: Arc<Session>,
    retry: Retry,
    /// The drawing side of the gate is read from here; the body's only owner
    /// is the `Waker` (the waking side must look at the same gate).
    waker: Waker,
    /// The measurement gate. `None` → the gate is closed and the frame path
    /// runs as it did **before** measurement existed: not a single clock read
    /// The GPU delta comes from the renderer's completion poll, on
    /// this thread.
    stats: Option<Arc<Stats>>,
    /// The frame list is long-lived: it is refilled with `clear` every frame
    /// and the allocated space is kept (no reallocation per frame).
    frame: RefCell<Frame>,
    /// The command blocks' buffer; long-lived for the same reason as `frame`
    /// — `Session::frame` empties and refills it every frame and the
    /// allocated space is kept.
    ///
    /// **Next to `Frame`, not inside it**: the same call borrows both the
    /// `frame.push` closure and the buffer, and in one `RefCell` that would
    /// panic at run time. The list that draws the stripe (pixel quads) is
    /// `Frame`'s own `stripes`; this buffer is its **input**, not the list —
    /// `Frame::push_block` reads the ranges here and turns them into quads.
    blocks: RefCell<Blocks>,
    /// The selection's row runs and two colours; same lifetime and
    /// reason as `blocks` — next to `Frame`, not inside it;
    /// `Frame::push_selection` turns it into quads.
    selection: RefCell<SelectionRuns>,
    /// The search highlight's runs; same lifetime and reason as
    /// `selection`.
    search: RefCell<SearchRuns>,
    /// The scroll bar's marks of the whole history; same lifetime and reason
    /// as `search` — and kept between content frames like the bar's layout,
    /// because the motion frame that fades the bar draws them from here.
    marks: RefCell<TrackMarks>,
    /// The marks changed while the bar was down ([`DisplayLink::marks_changed`]),
    /// and no content frame has drawn them since: the next time the bar
    /// shows, it asks for a **content** frame instead of a motion one, or it
    /// would come up over the old marks.
    marks_stale: Cell<bool>,
    /// The dock selection's runs per visual row; same reason as
    /// `selection` — `bt_core::Session::dock` empties and refills it every
    /// content frame, the capacity is kept.
    dock_selection: RefCell<Vec<SelectionRun>>,
    /// The filled rows' buffer; same lifetime and **same reason** as
    /// `blocks`: next to `Frame`, not inside it.
    ///
    /// The reason is the borrow rule itself: `Session::frame` takes two
    /// sinks, and if both borrowed `frame` the same call would give birth to
    /// two `&mut`. The buffer holds that second end; when the call returns
    /// the cells move to the band's own lists through `Frame::push_fill`. It
    /// is emptied and refilled, so no allocation per frame; in a window
    /// without fill (a dockless shell, the timed run) the boundary never
    /// calls the sink and the buffer stays empty.
    fill: RefCell<Vec<bt_core::Cell>>,
    /// The mirror's buffer; long-lived for the same reason as `blocks` —
    /// [`Session::dock`] refreshes it in place every frame and its capacity
    /// stays, so no allocation per frame.
    ///
    /// Untouched in a window without a dock: an empty `DockState` is three
    /// empty strings and an empty `Vec`, so it does not allocate either.
    dock: RefCell<DockState>,
    /// The context row's buffer; same reason and lifetime as `dock`.
    ///
    /// A separate buffer because a separate lifetime: the mirror resets at
    /// `line-finish`, the directory and the branch persist from prompt to
    /// prompt (`bt_core::DockContext`).
    dock_context: RefCell<DockContext>,
    /// How many rows the dock has; `0` → no dock in this window.
    ///
    /// **A `Cell`, because it moves now:** the dock goes away on the
    /// alternate screen and comes back when leaving it, so the value
    /// is refreshed by [`DisplayLink::resize`]. The cost of moving is the
    /// grid's height, i.e. a `TIOCSWINSZ` — paid **per transition, not per
    /// command**: commands that do not enter the alternate screen,
    /// like `git log`, never see a resize.
    ///
    /// Zero means **two different things** and both mean "no dock drawn":
    /// the window has no dock at all (a session without integration) or we
    /// are on the alternate screen right now. Telling them apart is not
    /// needed here — the birth value's owner is `bt-shell` and so is the one
    /// who brings it back.
    dock_rows: Cell<u16>,
    /// The alternate screen's **last seen** state; the watch compares against
    /// it.
    alt_screen: Cell<bool>,
    /// The notifier called when the alternate screen changes; `None` → the
    /// path does not run in this window at all.
    ///
    /// **Injected, not called** (the `bt_core::Wake` precedent): `bt-gpu`
    /// cannot see `bt-shell`, the layer direction is one-way. The closure is
    /// built in `bt-shell` and carries the same three bans: it runs on the
    /// main thread, it **does not block** and it **does not change** the
    /// window geometry in place — it only sends work to the main queue. The
    /// reason is where this is called: the frame has just been drawn, and
    /// changing the texture size, the grid or the layout there would dig
    /// under the drawn frame.
    ///
    /// **It carries no payload.** When the notifier runs it re-reads the
    /// truth, so two transitions chasing each other (vim open–close) cannot
    /// act on a stale value.
    ///
    /// `None` in a window without a dock, and **structurally**: the path is
    /// never set up in that session, not switched off by a condition.
    alt_screen_changed: Option<Box<dyn Fn()>>,
    /// Told when the published block marks changed
    /// ([`DisplayLink::on_marks_published`]) — the pointer's targets moved
    /// under a still pointer. The alternate-screen notifier's contract: main
    /// thread, no block, only work sent to the main queue, no payload.
    marks_published: RefCell<Option<Box<dyn Fn()>>>,
    /// The dock's width, columns; goes into [`Session::frame`]'s budget and
    /// [`Session::dock`] so the dock wraps its overflowing row at the width
    /// it is drawn in.
    ///
    /// **Not the grid's width**: in the scroll bar's `Always` form the grid
    /// gives the track's columns up and the dock below the bar does not —
    /// `bt-shell`'s `split_into_grid` computes both. One value for every
    /// reading here (the wrap's budget, the dock's columns, the context row's
    /// budget): were one of them the grid's, the band would be a row short of
    /// what the dock draws, or the dock's last columns would not count.
    ///
    /// `Cell`: [`DisplayLink::resize`] writes, the content frame reads — both
    /// on the main thread. Kept **separately** from `cell` because their
    /// sources differ: the cell size is refreshed only if the session accepts
    /// the size, the column count is the window's own answer.
    dock_cols: Cell<u16>,
    /// `CellMetrics`, not a tuple: the grid geometry (cell size **and**
    /// gutter) arrives here from `Renderer::cell_metrics` through `bt-shell`
    /// as a type, **stays** a type while stored and enters `Frame::clear` as
    /// a type — the draw origin reads the gutter there. (The tuple `resize`
    /// passes to `Session::resize` is another value: the **incoming** size
    /// goes there, not the stored one — a rejected size is never written
    /// here.)
    cell: Cell<CellMetrics>,
    /// The drawn frame's vertical origin; the mouse path shares this body.
    ///
    /// Not a twin of `Frame::origin_px` but its **publication**: the frame
    /// list is internal to this crate (`pub(crate)`) and `bt-shell` has no
    /// reason to see it, while the mouse mapping **must** see the drawn
    /// origin. One call writes both (`Core::set_origin`), so they cannot
    /// drift.
    origin: Origin,
    /// **Content** frame: `session.frame()` found damage and the frame was
    /// decided to be drawn. This is the zero-frames-at-idle gate's operand.
    ///
    /// A number separate from `frames` (frames the GPU finished without error):
    /// **motion** and **slide** frames are drawn frames too, so they raise
    /// `frames`, but the grid is not dirty — a 200 ms cursor glide is ~24 frames
    /// at 120 Hz and a `frames ≤ IDLE_FRAME_LIMIT` gate would go red while the
    /// code is right. So the gate is tied to "**content** frames at idle";
    /// what changed is not the limit's number but its **operand**.
    ///
    /// No subtraction (`frames − motion`) on purpose: the two counters rise at
    /// different moments (`frames` when the completion poll sees the frame
    /// finished, this one when the frame is decided), so a deadline in the
    /// middle of an animation would leave the difference open to `u64`
    /// wrap-around. The gate looks at one of them only.
    ///
    /// `Cell`, not atomic: only the tick writes it and the tick is on the
    /// main thread; the reader is on the main thread too
    /// ([`DisplayLink::content_frames`]).
    content_frames: Cell<u64>,
    /// **Motion** frame: no damage but an animation that has not settled.
    ///
    /// `content_frames`' sibling and separate from it on purpose: both count
    /// drawn frames but only one is the zero-frames-at-idle gate's operand.
    /// The timed run prints this as `motion=` and it is the smoke gate's
    /// **required** counter (the recipe has a cursor move, see
    /// `bt_core::smoke_shell`).
    motion_frames: Cell<u64>,
    /// **Slide** frame: no damage but the offset's animation has not
    /// settled.
    ///
    /// [`Self::motion_frames`]' sibling and separate from it, because there
    /// are two animators and whoever reads a red run should see from the line
    /// which one did not settle. Both can rise in the same frame — the
    /// numbers do not add up to drawn frames, each is its own animator's
    /// witness.
    ///
    /// **Not** in the gate: the smoke recipe has a cursor move
    /// (`bt_core::smoke_shell`), but with bottom-aligned content a one-row
    /// prompt may produce no slide — an unmeasured threshold is not written
    /// into the gate.
    slide_frames: Cell<u64>,
    /// The cursor's and the content's glide — the only thing tying the frame
    /// to time.
    ///
    /// `Cell`, not `RefCell`: [`crate::motion::Motion`] is `Copy` and the
    /// only place touching it is the tick (main thread). A `RefCell` would
    /// work but would mean a second runtime borrow next to `frame`'s, and it
    /// buys nothing.
    motion: Cell<Motion>,
    /// The dock's typing effects: arrivals and ghosts in flight.
    ///
    /// **Next to `motion`, not inside it**, and a `RefCell`: the list is not
    /// `Copy`, and taking `Motion` out of `Copy` or copying on every
    /// `get`/`set` was a cost. Only the tick and `DisplayLink`'s settings
    /// paths borrow it, both on the main thread and both release it at the
    /// call boundary.
    glyph_fx: RefCell<GlyphFx>,
    /// The geometry (window, font, zoom) moved: the next content frame should
    /// move the cursor without animation.
    ///
    /// A flag, because [`DisplayLink::resize`] is not the tick — the path
    /// changing the cell size and the path drawing it run at different
    /// moments and only this flag links the frame in between. The next
    /// content frame **consumes** it: left unconsumed, every frame after a
    /// geometry change would snap.
    geometry_changed: Cell<bool>,
    /// The previous tick's stamp; the base of `dt`.
    ///
    /// Its source is the **same** as `last_frame_at` (the tick's stamp) and
    /// that is required: two bases create two times, and `quiet=` and the
    /// animation's clock would not agree. No clock read, a field copy.
    ///
    /// A field separate from `last_frame_at`, because that one is only
    /// refreshed on a frame that **leaves**; `dt` must advance on a frame
    /// that could not be encoded too, or after an error the animation would
    /// jump that much time in one step.
    last_update_at: Cell<Option<f64>>,
    /// A copy of the theme read in the content frame — the motion frame's
    /// palette.
    ///
    /// The motion frame **does not call** `session.theme()`: that takes a
    /// leaf lock, and the motion frame not touching `Session` at all is the
    /// design itself. A theme swap asks for a frame anyway
    /// (`Session::set_theme`), so the next frame is a content frame and the
    /// copy is refreshed there.
    theme: Cell<Theme>,
    /// The absolute time of the next **content** tick `bt-core` asked for.
    ///
    /// It has two sources and `bt-core` gives the nearer
    /// (`bt_core::shell::sooner`): the running command's duration counter,
    /// and in a docked window the caret handover's **hold**. `bt-gpu` does not
    /// tell them apart — both change the content, so counting `content=` is
    /// right.
    ///
    /// `None` → nothing is expected: the command ended, the running block's
    /// anchor left the screen, there is no integration, or no handover hold
    /// is pending.
    ///
    /// **A deadline, not a duration**, and that is blink's requirement: in
    /// the old form `arm_clock` rebuilt `Cursor::next_tick` at every sleep
    /// point, and a blink waking twice a second would push the counter's tick
    /// one second forward every time — the tick would **never** fire. An
    /// absolute stamp is not affected by the wakeups in between.
    ///
    /// A side gain: the defect written in `arm_clock`'s doc — "the motion
    /// frame does not refresh `Cursor`, so a tick armed after a long animation
    /// can be one animation late" — closes too.
    ///
    /// `None` **clears** (a lesson learned the hard way): a stale deadline after the
    /// command ended would ask for one frame too many.
    content_deadline: Cell<Option<f64>>,
    /// The cursor's blink; the phase's owner is the painting side
    /// ([`crate::blink`]).
    blink: Cell<Blink>,
    /// The scroll bar's visibility ([`crate::scrollbar`]); `Copy`, blink's
    /// slot.
    scrollbar: Cell<Scrollbar>,
    /// The scroll bar's layout from the last **content** frame.
    ///
    /// Kept for two readers that never reach `bt-core`: the motion frame,
    /// which fades the bar where the content frame put it, and
    /// [`DisplayLink::poke_scrollbar`], whose gate is "can a bar be drawn" —
    /// asked of the last frame, not recomputed off the frame path.
    scrollbar_layout: Cell<ScrollbarLayout>,
    /// Whether the window is **focused** — `bt-shell`'s answer.
    ///
    /// Never enters `bt-core`: focus is a window fact and has nothing to
    /// do with the terminal's state. `bt-gpu` reads it in two places — the
    /// blink's gate and the caret's hollowing.
    ///
    /// Default `true` and **never written in a hermetic run**: the timed run
    /// (`BT_RUN_SECONDS`) does not read focus, so `make smoke` does not go
    /// green on one machine and red on another. The gate is at the call site
    /// (`bt-shell`'s delegate), not in the default — a default alone would
    /// not do, because a Spotlight opened during the run would produce
    /// `windowDidResignKey:` and ask for a frame.
    focused: Cell<bool>,
    /// Whether the keyboard is **in the terminal** — `bt-shell`'s answer
    /// `false` when the search panel's field becomes first
    /// responder.
    ///
    /// **Focus is two bits** and they combine here, in one
    /// place ([`Core::caret_focused`]): the caret's hollowing and blink's
    /// stopping answer "window key **and** keyboard in the terminal" — the
    /// caret is the one signal saying where the keyboard goes. The selection
    /// and search highlight fading come from [`Core::focused`] alone: while
    /// typing in the field the highlights should stay at full colour. With a
    /// single bit one of the two would be wrong.
    keyboard: Cell<bool>,
    /// The cursor's drawing numbers **from settings**.
    ///
    /// In the same slot as `cell`/`motion`/`blink` and for the same reason:
    /// the frame path hands it to `Frame` on every content frame (`clear`'s
    /// second argument), and the motion frame does not call `clear`, so it
    /// keeps the value.
    ///
    /// Not in the sense of **skipping** `bt-core` — the value lives in
    /// `bt_core::Settings` and the one owner of its default is there; what it
    /// skips is `TerminalOptions`/`Session`, the terminal's state machine.
    caret_style: Cell<CaretStyle>,
    /// Blink's **requested** half period, seconds
    /// ([`DisplayLink::set_blink_interval`]).
    ///
    /// A separate slot, because applying it needs a **frame stamp**:
    /// `Blink`'s tick is an absolute deadline and rebuilding it needs `now`.
    /// Keeping the value here and applying it on the frame path preserves the
    /// single time base.
    blink_interval: Cell<f64>,
    /// The armed tick's generation — so a stale tick recognises itself and
    /// stays quiet.
    ///
    /// [`Pacer::after`] cannot be cancelled, so when a content frame comes in
    /// between and re-arms the clock, the old tick still fires. If the
    /// generation does not match, that tick is void and does not touch the
    /// `Waker`; otherwise every re-arming would produce one extra content
    /// frame.
    ///
    /// `Arc<AtomicU64>`, because the closure must be `Send` — whichever
    /// thread the pacer fires it on.
    clock_generation: Arc<AtomicU64>,
    /// The colour of the text under the caret block, from the last content
    /// frame.
    ///
    /// The motion frame never goes to `bt-core` and cannot take this value
    /// from there. **Two sources** — the grid's cursor or the dock's caret,
    /// whichever is the caret's home in that frame; what they share is not
    /// depending on the position, so both write this field and the motion
    /// frame does not ask which one wrote. `None` → no caret in that frame.
    last_caret_text: Cell<Option<LinearRgba>>,
    /// The **target** of the caret in the last content frame; the single
    /// source of blink's "is the user typing" question ([`Blink::wake`]).
    ///
    /// The position is in `Motion` too, but that holds the **intermediate**
    /// position (a different value every frame while animating); the target
    /// itself sits here and the comparison only makes sense against it.
    last_caret_at: Cell<Option<[f32; 2]>>,
    /// The stamp of the last **drawn** frame (the tick's target presentation
    /// time), the base of the `quiet=` token.
    ///
    /// The tick's stamp is a **field copy**, not a clock read: reading the
    /// clock every frame would read it even with the measurement gate closed
    /// and break the frame path's "with the gate closed, not a single clock
    /// read" contract (`stats`' doc). The one read is at the deadline, in
    /// [`DisplayLink::quiet_since`].
    ///
    /// Written in the `Ok` arm: a frame that could not be encoded does not
    /// break the silence, because nothing happened on screen.
    ///
    /// `None` → no frame drawn yet; the token is then `quiet=none`.
    last_frame_at: Cell<Option<f64>>,
}

impl Core {
    /// The caret's focus: window key **and** keyboard in the terminal
    /// ("focus is two bits"). Its hollowing, blink's gate and its
    /// redraw in the motion frame come from here; highlights and selection
    /// colour from `focused` alone.
    fn caret_focused(&self) -> bool {
        self.focused.get() && self.keyboard.get()
    }

    /// Hands every frame the GPU has finished to its four jobs:
    /// a frame finished without error gives the failure budget back, closes
    /// `startup=` (the first one) and records the GPU delta when measured; a
    /// frame that failed on the GPU goes to the same policy as a synchronous
    /// error. `frames=` itself is counted by the renderer ([`Renderer::poll`]).
    ///
    /// Returns whether a frame is still in flight.
    fn complete(&self) -> bool {
        let timed = self.renderer.gpu_timing_supported();
        self.renderer.poll(|result| match result {
            Ok(span) => {
                self.retry.streak.succeeded();
                // The measurement gate is **here**: closed, not a single
                // extra call is made. `startup=` closes here, not in
                // `draw` — what is measured is "main to the first **finished**
                // frame" and submitting is not finishing.
                if let Some(stats) = &self.stats {
                    stats.mark_startup();
                    match span {
                        // The GPU's own clock; not correlated with a CPU
                        // stamp, because the question is not "which frame"
                        // but the **distribution**.
                        Some(span) => stats.record_gpu(span.start, span.end),
                        // Measured but no usable span (the readback failed):
                        // counted as rejected, like Metal's zero stamps, so
                        // an empty GPU column is not "no frame drawn". With
                        // no timestamp support there is nothing to reject —
                        // the token says `unsupported`.
                        None if timed => stats.reject_gpu(),
                        None => {}
                    }
                }
            }
            Err(e) => {
                self.retry.draw_failed(&e);
            }
        })
    }

    /// Draws `frame` into this tick's texture and presents it.
    ///
    /// `Ok` means submitted and queued for presentation — asynchronous, like
    /// Metal's `commit`; the frame's real fate reaches [`Core::complete`] at
    /// a later tick. On `Err` the texture is dropped unpresented (wgpu
    /// discards it) and the caller sends the error to the frame policy.
    fn draw(
        &self,
        texture: wgpu::SurfaceTexture,
        clear: LinearRgba,
        frame: &Frame,
    ) -> Result<(), GpuError> {
        self.renderer
            .draw(&surface::target(&texture), clear, frame)?;
        self.renderer.present(texture);
        Ok(())
    }

    /// This tick's texture, or the reason there is none. Nothing is acquired
    /// on the paths that go to sleep: under the timer-only provider a
    /// drawable is paid only by a frame that is drawn.
    fn acquire(&self) -> Option<wgpu::SurfaceTexture> {
        match self.surface.acquire() {
            Acquired::Frame(texture) => Some(texture),
            // Not an error: the window is occluded or no drawable came. The
            // damage is kept and the pacer **pauses**. Retrying every vsync
            // would tick at the refresh rate for as long as the window stays
            // occluded (measured: a window the hal reports occluded from
            // birth gets no occlusion *change* notice, so the gate never
            // closes). What brings the frame back is the visibility notice —
            // becoming visible is a change, `set_visible(true)` asks for a
            // frame — or the next wake. `Timeout` cannot happen on Metal
            // (wgpu-hal turns `allowsNextDrawableTimeout` off).
            Acquired::Skip => {
                self.waker.keep_damage();
                self.waker.pacer().set_running(false);
                None
            }
            Acquired::Failed(e) => {
                self.retry.draw_failed(&e);
                None
            }
        }
    }

    /// One vsync tick: the old display-link callback, now platform-free.
    ///
    /// Runs on the thread that created the link — on macOS the main thread,
    /// where the pacer's timer is on the main run loop.
    fn tick(&self, now: f64, target: TickTarget) {
        // Only (b) exists today: the texture comes from the surface
        // (`TickTarget`'s doc).
        let TickTarget::Surface = target;
        // **Completion first**, before the gate: a frame submitted before
        // the window was occluded is still counted when the next tick comes.
        self.complete();
        // Drawing into an invisible window is not wasted work, it breaks the
        // battery contract: a chatty shell in an occluded window would draw a
        // full frame every refresh.
        if !self.waker.gate().is_open() {
            self.waker.pacer().set_running(false);
            return;
        }
        // No size yet (the view has not been laid out, or it is minimised):
        // nothing to draw into. The damage stays; the resize that configures
        // the surface asks for a frame.
        if !self.surface.is_configured() {
            self.waker.pacer().set_running(false);
            return;
        }
        // The first frame has no `dt`: starting at `0.0` beats inventing an
        // unknown interval. `Motion::advance` does the clamping and it must be
        // there (reason: `motion::DT_MAX`).
        let dt = self
            .last_update_at
            .replace(Some(now))
            .map_or(0.0, |prev| (now - prev) as f32);
        // audit: the tick is on the main thread and not re-entered; the sink
        // does not re-enter `Session`, so no second borrow is born.
        let mut frame = self.frame.borrow_mut();
        let mut motion = self.motion.get();
        // **The damage question comes before the scan**: the
        // motion frame uses the list without clearing it, so "clear or not" is
        // decided before `clear`. `Session::frame`'s old `Option` made exactly
        // this order impossible.
        //
        // **While the glide is in flight a frame without damage is a content
        // frame too**: its share scrolls the window inside
        // `Session::frame`, and the motion frame never goes to `bt-core`. The
        // request is motion's — nobody wakes, `Waker::wake` is not touched —
        // the drawing is content's (module header).
        let damaged = self.session.take_damage();
        if !damaged && motion.glide_idle() {
            self.motion_tick(&mut frame, motion, now, dt);
            return;
        }
        // The texture before the CPU spans: acquiring may wait for a free
        // drawable, and Metal's spans never included `nextDrawable` either
        // (the display link handed the drawable over before its callback).
        let Some(texture) = self.acquire() else {
            // The damage was taken above; `acquire` planted it again.
            return;
        };
        frame.clear(self.cell.get(), self.caret_style.get());
        // **Two** CPU spans, not one: the lock wait is inside
        // `session.frame`, the encode inside `draw`. One span would add them
        // and erase the split.
        //
        // With the gate closed the clock is **never** read: `then`
        // runs its closure only on the full side, so a closed gate costs one
        // branch.
        let t0 = self.stats.is_some().then(Instant::now);
        // **The second sink flows into a buffer, not straight into `Frame`**
        // and the reason is the borrow rule: if both sinks borrowed `frame`
        // the same call would give birth to two `&mut` (`Core::fill`, the
        // twin of `blocks`' reason). The cells move to the band when the call
        // returns.
        let mut fill = self.fill.borrow_mut();
        fill.clear();
        // **Where the grid is drawn is reported before the scan**: while the
        // slide is in flight the grid is below its target and the strip that
        // opens at its top is covered by the fill band
        // (`Session::set_grid_top`). The value is the position before this
        // frame's `advance` — for a slide heading to settle a little larger
        // than needed, so the excess is off screen.
        //
        // **The band's excess is subtracted**: the grid is drawn that
        // much higher and the opening strip is that much higher.
        let grid_top = motion.origin() - motion.band();
        // **Elapsed time is processed before the scan**, because of the
        // glide: its share is the position's change in this frame and an
        // argument of `frame()`, so it must be known before `frame()`. For the
        // cursor and the offset the order is unchanged — `advance` still
        // before `sync`, and nobody reads `motion` in between.
        motion.advance(dt);
        // The request comes **after** `advance` (`Motion::request_glide`): a
        // link waking from sleep must not apply its clamped `dt` to the new
        // notch.
        motion.request_glide(self.session.take_scroll_glide());
        let glide = motion.take_glide();
        // The band being shorter than the PTY share (remote session) is
        // separate: the grid is that much lower for good and the strip must be
        // covered in a scrolled window too (`Session::slide_fill_rows`).
        let lowered = (-motion.band()).max(0.0).ceil() as u16;
        self.session
            .set_grid_top(grid_top.max(0.0).ceil() as u16, lowered);
        // The cluster table is **outside** `Frame` for the call: the sinks
        // borrow `frame` (`Frame::take_clusters`). `clear` above emptied it;
        // both sinks write to the same table.
        let mut clusters = frame.take_clusters();
        let cursor = self.session.frame(
            |cell| frame.push(cell),
            |cell| fill.push(cell),
            &mut self.blocks.borrow_mut(),
            &mut self.selection.borrow_mut(),
            &mut self.search.borrow_mut(),
            &mut self.marks.borrow_mut(),
            &mut clusters,
            // The share **does not wake**: this tick draws the frame anyway
            // (`Session::frame`). If its generation changed it drops there.
            glide,
            // **The cap is a ratio** (`DOCK_MAX_SHARE`): `frame()`
            // reads the row count under the `Term` lock, this layer keeps no
            // copy of it. The wrapping width is the dock's, the one
            // `Session::dock` draws at below (`Core::dock_cols`).
            DockBudget {
                share: crate::frame::DOCK_MAX_SHARE,
                cols: self.dock_cols.get(),
            },
        );
        // **The generation a second time, after `frame()`**: when `frame()`
        // finds the fraction invalid (`CSI 3 J`, the alternate screen, mouse
        // mode) it raises the generation itself and the glide in flight must
        // end in that frame. The position is asked too: if the share did not
        // move the position, the window is at the end of history and the
        // glide ends there (`Motion::observe_scroll`) — without either, frame
        // after frame would be drawn for shares hitting the clamp.
        frame.put_clusters(clusters);
        motion.observe_scroll(
            cursor.scroll_generation,
            (cursor.display_offset, cursor.scroll_frac),
            glide.rows,
        );
        // The fraction **before** the origin (`Frame::set_origin_rows` adds it
        // the moment it is written) and before the caret (`Frame::push_caret`
        // adds it to the grid's caret).
        frame.set_scroll_frac(cursor.scroll_frac);
        // **The channel's height before the cells** (`Frame::set_fill_rows`):
        // `push_fill`'s guard measures the row against it. The channel is the
        // band plus the fraction's top row; if zero, the boundary never called
        // the second sink, so the loop is empty too and the frame is
        // bit-identical to its fill-less form.
        frame.set_fill_rows(cursor.top_row + cursor.fill);
        for cell in fill.drain(..) {
            frame.push_fill(cell);
        }
        // The band's own block marks (`Blocks::fill_slice`): **after** the
        // cells, because `push_fill_block`'s guard reads the band's height and
        // that is written in the same frame as the cells. It does not go
        // through the grid's `borrow` round — the band's list is separate and
        // lands in `fill_rules`.
        for block in self.blocks.borrow().fill_slice() {
            frame.push_fill_block(*block);
        }
        // Stripes in the **same** frame as the cells and from the same
        // `frame()` call: read from a separate query they would lag one frame
        // behind on a scroll frame. After the
        // sink rather than inside it, because the block list is resolved per
        // frame, not per cell — and `borrow_mut` dropped at the end of the
        // expression above, so this `borrow` does not clash.
        //
        // **No animation**: the stripe appears at once, `motion`
        // gains no second consumer and this path asks for no frame — zero
        // frames at idle stays untouched. The motion frame's path
        // (`Core::motion_tick`, `move_caret`) never comes here; the grid did
        // not change, so the stripe must not either and `Frame` keeps it.
        for block in self.blocks.borrow().as_slice() {
            frame.push_block(*block);
        }
        // **The selection's colour comes from focus**: both
        // colours arrive ready from the boundary, which one is drawn is
        // decided here. Not a new source of frames — a focus change asks for a
        // content frame already ([`DisplayLink::set_focused`]) and the colour
        // turns in that frame; the motion frame keeps the list, and the colour.
        let selection = self.selection.borrow();
        let rgba = selection.color(self.focused.get());
        // The slice at once: the corner decision looks at the neighbouring
        // row's run.
        frame.push_selection(selection.as_slice(), rgba);
        drop(selection);
        // **The search highlight by the same rule**: two roles,
        // colour from focus; the grid's and the band's runs from the same
        // `frame()` round. The band's after `set_fill_rows` — its guard reads
        // the band's height — and their colours are the uniform `push_search`
        // wrote. The motion frame keeps the lists and does not scan.
        let search = self.search.borrow();
        let focused = self.focused.get();
        frame.push_search(
            search.as_slice(),
            search.match_color(focused),
            search.current_color(focused),
        );
        frame.push_fill_search(search.fill_slice());
        drop(search);
        // The gate's operand rises here: damage was found, the frame will be
        // drawn. Before `frames` and independent of it — it does not wait for
        // the GPU to finish (see `Core::content_frames`).
        self.content_frames.set(self.content_frames.get() + 1);
        // `frame()` just refreshed the scroll bar's marks: none are owed.
        self.marks_stale.set(false);
        // Clear and cursor colour from the session's theme: the same source as
        // `frame()`'s background skip and the colour query's answer. The
        // theme is read only on a full frame — the idle tick returned above —
        // and kept for the motion frame.
        let theme = self.session.theme();
        self.theme.set(theme);
        // The duration counter's tick becomes an **absolute** stamp; `None`
        // clears the pending deadline.
        self.content_deadline
            .set(content_deadline(now, cursor.next_tick));
        // **The dock now comes BEFORE the cursor** and the order is required:
        // the caret's target must be able to ask for the dock's caret
        // (`Dock::caret`), so that answer has to be in hand before
        // `motion.sync`. The order of entering the list does **not** decide
        // the drawing order — the dock has its own lists and its own encode
        // (the renderer's plan), so the order there is fixed and the order
        // here is only data dependency.
        //
        // The mirror asks for a frame on every keystroke: the payload reaches
        // the parser too and alacritty sends `Event::Wakeup` for every byte
        // processed, so `dirty` is already planted before entering this arm.
        // So the dock carries no frame request of its own — zero frames at
        // idle stays untouched.
        let dock_rows = self.dock_rows.get();
        // The window's bottom, **in window space**: the dock's band and the
        // caret's target both lean on it. The height is read from the
        // texture, because that is its one correct source — `rows * cell_h`
        // would not see the strip (the pixels left over when the height is
        // divided by the cell height) and the caret would stand up to a cell
        // too high. The renderer's dock viewport is built with the same
        // subtraction, so both are the same line.
        let viewport_height = texture.texture.height() as f32;
        let mut dock_caret = None;
        // The typing effects' clock advances on the content frame too: in fast
        // typing every tick finds damage and the motion arm never runs.
        let mut glyph_fx = self.glyph_fx.borrow_mut();
        glyph_fx.advance(dt);
        // The band's rows, `None` for no band: one reading for the layout, the
        // dock's cells and the band's target below.
        let band_rows = cursor.band_rows();
        if dock_rows > 0 {
            // **The layout before the cells** (`Frame::set_dock_input_rows`):
            // input rows + the context row, from the number `frame()` gave in
            // the same read as the suppression. The band's top is not written
            // here — the band is the animation's value and comes after `sync`
            // (`Core::set_origin`). With no band the surface still opens: the
            // band slides to zero height and the mouse reads a zero-row dock.
            frame.set_dock_input_rows(band_rows);
            let mut dock_state = self.dock.borrow_mut();
            let mut dock_context = self.dock_context.borrow_mut();
            // **The second sink flows into a local slot**, not straight into
            // `Frame`: both sinks cannot borrow `frame` (`fill`'s reason). There
            // is at most one edit per frame, so the buffer is an `Option`.
            let mut edit = None;
            // The twin of the grid's table, same reason.
            let mut dock_clusters = frame.take_dock_clusters();
            // The handover's answer comes from `frame()`, the dock does not
            // recompute it: only `frame()` knows the three preconditions (a
            // window with a dock, the alternate screen, the mirror's
            // freshness).
            let dock = self.session.dock(
                DockCols {
                    input: self.dock_cols.get(),
                    // The context row's budget: **the same pixel width, a
                    // smaller step**. The dock shares the gutter with the grid
                    // (`Frame::dock_pos`), so the strip both rows occupy is the
                    // same; the only thing that differs is how many pixels a
                    // letter advances. The arithmetic is here, because
                    // `bt-core` does not see pixels.
                    context: crate::frame::context_cols(self.dock_cols.get(), self.cell.get()),
                },
                // The number passes from where it was computed, the dock does
                // not derive it again (`caret_in_dock`'s precedent).
                band_rows,
                &mut dock_state,
                &mut dock_context,
                cursor.caret_in_dock,
                &mut self.dock_selection.borrow_mut(),
                &mut dock_clusters,
                |cell| frame.push_dock(cell),
                |dock_edit| edit = Some(dock_edit),
            );
            frame.put_dock_clusters(dock_clusters);
            // The order is required: an edit can shift and finish the ones in
            // flight, an arrival whose static glyph cannot be found is only
            // known **after** the dock is printed, and the list to draw comes
            // last.
            if cursor.input_rows == 0 {
                // No input row (remote session, no band): no surface for the
                // effect either. It ends unconditionally — `Reset` only comes
                // when the mirror changed, and an arrival left in flight would
                // be drawn on row 0, i.e. now in the context row's place, at
                // the wrong size.
                glyph_fx.finish();
            } else if let Some(edit) = edit {
                // The vertical window's height is the very number passed to
                // `dock()`: an effect the scroll pushes out of the window drops.
                glyph_fx.apply(edit, motion, cursor.input_rows, frame.dock_clusters());
            }
            frame.suppress_dock(&mut glyph_fx);
            frame.set_dock_fx(glyph_fx.iter(), glyph_fx.clusters(), theme.cursor_linear());
            dock_caret = dock.caret.map(|at| (at, dock.caret_text));
            // The dock's selection is the grid's shape and colour uniform;
            // the colour was written above with `push_selection` — one
            // selection per window, one colour.
            frame.push_dock_selection(&self.dock_selection.borrow());
            // No mark → the input's first row is outside the vertical window.
            if let Some(sigil) = dock.sigil {
                frame.push_dock_sigil(sigil);
            }
            // The surface opens **after** the cells: the call bringing the
            // colours is the very call printing the cells (`Frame::open_dock`).
            frame.open_dock(dock.ground, dock.edge, dock.separator);
            frame.set_dock_progress(dock.progress, dock.track);
            frame.set_dock_buttons(dock.buttons);
        } else {
            // No dock (alternate screen): the effect has no subject either.
            glyph_fx.finish();
        }
        drop(glyph_fx);
        // **The caret's single target.** There are two homes and both enter
        // the same animator: the dock if it took over, otherwise the grid's
        // cursor. With separate animators there would be no glide in the dock
        // and the handover would stay a teleport — two separate user
        // complaints, one cause.
        //
        // The dock has priority and the two **cannot** be `Some` at once: the
        // handover's answer is computed in one place (`Session::frame`) and
        // given from the same value to both the grid's `visible` and the
        // dock's caret (`Cursor::caret_in_dock`). This sentence was once
        // false: `dock::render` asked the predicate itself, did not know
        // `frame()`'s three preconditions, and on a stale mirror both were
        // born — the `.or_else` below picked the dock and the fresh row lost
        // its caret. The order still stays written:
        // a caret drawn in the wrong place is a visible defect, rather than
        // one drawn in two places.
        //
        // **The band's excess in both targets**: the grid's caret is
        // that much higher together with the grid by the band's **target**
        // excess, the dock's is in the bottom-aligned input block, on the
        // wrapped row's own row.
        let band_target = band_target(band_rows, dock_rows, self.cell.get());
        let caret = dock_caret
            .map(|(at, text)| {
                // A dock caret exists only on a band with input rows: the
                // dock gives none with no band.
                let at = dock_caret_at(
                    at.col,
                    at.row,
                    cursor.input_rows,
                    viewport_height,
                    self.cell.get(),
                );
                (at, text)
            })
            .or_else(|| {
                cursor.visible.then(|| {
                    (
                        [
                            f32::from(cursor.col),
                            f32::from(cursor.row) + f32::from(origin_target(cursor)) - band_target,
                        ],
                        cursor.text,
                    )
                })
            });
        self.last_caret_text.set(caret.map(|(_, text)| text));
        // **Blink AFTER the caret** and the order is required: "is the user
        // typing" is answered by the caret's target moving, and that target
        // is only known here.
        //
        // The setting and the application combine in `bt-core`
        // (`Cursor::blink`); Reduce Motion **turns it off** — an accessibility
        // setting does not *add* animation, and the side gain is
        // structural: `Mode::Fade` and blink exclude each other, so the
        // `alpha()` channel gets no second writer.
        let at = caret.map(|(at, _)| at);
        let moved = self.last_caret_at.replace(at) != at;
        let mut blink = self.blink.get();
        // **The third term is focus**: in an unfocused window blink
        // stops and the cursor stays visible. Not a new mechanism — the
        // "disabled blink stays visible" invariant (`content_frame`,
        // `enabled=false` → `lit=true`, `next_flip=None`) protects the hidden
        // cursor today; this is its third consumer. The side gain is on the
        // zero-frames-at-idle side: an unfocused idle window arms no clock.
        // The caret's focus is two bits combined ([`Core::caret_focused`]):
        // while typing in the field blink stops too and the caret is hollow.
        let focused = self.caret_focused();
        // **The setting's period is applied here** and before
        // `content_frame`: the tick is an absolute deadline, so rebuilding it
        // needs this frame's stamp. A no-op on the same value.
        blink.set_half_period(now, self.blink_interval.get());
        blink.content_frame(now, cursor.blink && !motion.reduce() && focused);
        // If the caret moved the phase goes back to lit: the cursor does not
        // fade while typing.
        if moved {
            blink.wake(now);
        }
        // **The phase advances here too** and the return value is dropped:
        // the frame is drawn anyway, no separate wakeup is needed. Without it
        // the phase would **freeze** in flowing output (every tick finds
        // damage) — and it could freeze in the faded phase, leaving the caret
        // invisible for the whole output.
        blink.advance(now);
        self.blink.set(blink);
        // `motion.advance` ran before the scan and the order is required:
        // first the elapsed time is applied to the old target, then the new
        // target is set. In the reverse order `dt` would be applied to the new
        // target and the cursor would accelerate for one frame towards a
        // direction it never went.
        //
        // **A full grid's slide** (`Motion::scroll_in`): if rows scrolled into
        // history while the target stayed put, the offset glides again from
        // that much lower. Before `sync`, so the wheel and geometry snap erase
        // this too.
        motion.scroll_in(cursor.scrolled, cursor.rows);
        motion.sync(
            caret.map(|(at, _)| at),
            origin_target(cursor),
            band_target,
            cursor.display_offset,
            // The geometry flag is **consumed** here: left unconsumed, every
            // frame after a window drag would snap.
            self.geometry_changed.replace(false),
            // **The direction rule's exception is computed here**:
            // if the gap above is filling with history, what comes down is not
            // the gap but the arriving history — the offset glides while
            // rising too. `bt-gpu` does not learn a terminal concept called
            // "fill"; what it gets is a single bit, like `offset` and
            // `geometry`.
            cursor.fill > 0,
        );
        self.motion.set(motion);
        // **The offset after `sync`** and the order is required: the value to
        // draw is not the target but the animation's place in this frame.
        // Being **before** `push_caret` is required too — the caret's rectangle
        // bakes this offset in.
        self.set_origin(&mut frame, motion, viewport_height);
        // **The scroll bar**: the layout is this frame's — the position comes
        // from the same read as the cells — and the step's answer is dropped:
        // the frame is drawn anyway (blink's precedent). Without this arm the
        // bar would freeze half-appeared while scrolling, where every tick
        // finds damage and the motion arm never runs.
        //
        // **The track ends at the band's target top**, not the drawn one: the
        // motion frames keep this layout while the band slides, and against
        // the target the thumb is right at rest and otherwise either waiting
        // for a growing band to rise to it or under a shrinking band's opaque
        // ground until it uncovers it. Against the drawn top it would stop
        // short of a band that has finished shrinking, for as long as the
        // hold lasts. The band's height is `compose`'s gate and formula.
        let band_px = if dock_rows > 0 && frame.dock().is_some() {
            crate::frame::band_px(band_rows, self.cell.get())
        } else {
            0.0
        };
        let layout = ScrollbarLayout::new(
            cursor.scroll_position(),
            texture.texture.width() as f32,
            crate::frame::band_top(viewport_height, band_px),
            self.cell.get(),
        );
        self.step_scrollbar(&mut frame, now, Some(layout));
        if let (Some(at), Some((_, text))) = (motion.position(), caret) {
            frame.push_caret(
                at,
                text,
                theme.cursor_linear(),
                motion.alpha() * blink.alpha(),
                cursor.shape,
                focused,
            );
        }
        // The first span closes here — **after** `push_caret`: putting the
        // cursor in the list is sink work, not encode. Had the stamp been one
        // line higher, `cpu_encode` would measure it next to `draw` and the
        // token's name would lie. The pair travels in one `Option`, so "both
        // or neither" is the only representable state.
        let spans = t0.map(|t0| (t0, Instant::now()));

        // `frame()` consumed the flag before drawing started; on an error it
        // must be planted again or this content is never asked for again and
        // the window stays stale. Synchronous and asynchronous errors go
        // through the same door.
        let drawn = self.draw(texture, theme.background_linear(), &frame);
        // The encode span closes with `draw`'s return (planning, submit and
        // present — Metal's span ran from the command buffer to `commit`,
        // with `presentDrawable` encoded in it): the second stamp lands here,
        // **before** the decision arms.
        let spans = spans.map(|(t0, t1)| (t1 - t0, Instant::now() - t1));
        match drawn {
            // A sample is written only for a frame that **leaves**: a frame
            // that could not be encoded measured nothing.
            Ok(()) => {
                // The offset is published only here too: the mouse mapping
                // must read the offset of the frame standing on screen.
                self.publish_origin(&frame);
                // The silence's base is refreshed only on a frame that
                // **leaves**, for the same reason: a frame that could not be
                // encoded changed nothing on screen. The stamp is the tick's
                // `now`; reading it twice would tie `dt`'s base and `quiet=`'s
                // base to two reads — what `last_update_at`'s doc bans by name.
                self.last_frame_at.set(Some(now));
                if let Some((stats, (cpu_frame, cpu_encode))) = self.stats.as_ref().zip(spans) {
                    stats.record_cpu(cpu_frame, cpu_encode);
                }
            }
            // The return is not read here: this arm's stop is the flag not
            // being planted, and that is inside `draw_failed`.
            Err(e) => {
                self.retry.draw_failed(&e);
            }
        }
        // **The alternate screen watch, after the measurement stamps.** The
        // gate is a comparison and an atomic read; the notifier only runs on a
        // transition (vim opens/closes), so an ordinary frame pays nothing.
        // Outside the stamps, because a `dispatch` cost on the transition
        // frame would land in `cpu_encode`, and that token claims the
        // drawing's duration.
        self.notice_alt_screen();
    }

    /// The "no damage" branch of [`Core::tick`]: two possibilities left and
    /// both end here — sleep, or draw a **motion** frame.
    fn motion_tick(&self, frame: &mut Frame, mut motion: Motion, now: f64, dt: f32) {
        motion.advance(dt);
        // **The sleep test's third question.** Blink lives outside `Motion`,
        // so `settled()` does not see it; without this line a tick woken by
        // `Waker::resume` would go back to sleep without drawing anything and
        // re-arm the clock — a wake/sleep spin that produces no frame. The
        // term is one-shot and consumed **before** `settled()`'s early return.
        let mut blink = self.blink.get();
        let flipped = blink.advance(now);
        self.blink.set(blink);
        // **The fourth question: typing effects**. Asked **before**
        // `advance`: the last state of an effect finishing in this step (an
        // arrival settled on its static glyph, a ghost gone) is not drawn yet,
        // and sleeping would leave a half-transparent letter hanging on
        // screen. If the list emptied in this frame the frame is drawn, the
        // next tick sleeps.
        let mut glyph_fx = self.glyph_fx.borrow_mut();
        let fx_idle = glyph_fx.is_empty();
        glyph_fx.advance(dt);
        // **The fifth question: the scroll bar**, appearing or fading. It
        // lives outside `Motion` too (blink's precedent) and its step is the
        // same one the content arm takes — only without a fresh layout: the
        // motion frame never reaches `bt-core`, so the bar fades where the last
        // content frame put it. The step writes the frame before the texture
        // is acquired; that is fine, the list is state, and on the sleep path
        // the opacity written is the one already on screen.
        let mut bar = self.step_scrollbar(frame, now, None);
        if at_rest(motion, flipped, fx_idle, bar.idle()) {
            // Zero frames at idle: neither new content nor an unsettled
            // animation, the pacer sleeps. The next `Wakeup` starts it again
            // through the `Waker`.
            //
            // No sample is written either, and that is not a branch but the
            // shape of the path: `draw` never ran in this frame, a fake
            // "encode = 0 ns" sample would pull the p95 down.
            self.motion.set(motion);
            self.waker.pacer().set_running(false);
            // **The clock is armed only here** and the place is required: the
            // pacer only sleeps once it has nothing else to do, so the tick is
            // only needed then. Armed while awake, every content frame would
            // plant one more tick.
            self.arm_clock(now);
            return;
        }
        // **Awake, but nothing new to draw**: only the scroll bar is in flight
        // and its opacity did not move in this tick (the tick that stamps a
        // poke on a hidden bar, the hold's last tick). The pacer keeps running
        // for the next tick; a frame here would be pixel for pixel the one on
        // screen, a drawable and a GPU pass for nothing.
        if nothing_to_draw(motion, flipped, fx_idle, bar) {
            self.motion.set(motion);
            return;
        }
        // **Motion frame** (the cursor or the offset, or both). The `Waker` is
        // not touched (module header): the pacer is awake already and this
        // tick itself keeps it going.
        //
        // The texture first: the offset below needs the window's bottom from
        // it. Without one the animation is finished at its target, the frame
        // is left to the next wake (the damage `acquire` planted brings a
        // content frame drawing the settled state), and the stop holds.
        let Some(texture) = self.acquire() else {
            // No texture: the animation ends at its target and a content
            // frame is asked for to draw that settled state. The effect lists
            // in `Frame` empty too, or the next damage-free frame (blink's)
            // would redraw them frozen halfway (the draw-error arm's reason
            // below).
            motion.finish();
            self.motion.set(motion);
            glyph_fx.finish();
            frame.set_dock_fx(
                std::iter::empty(),
                &Clusters::default(),
                self.theme.get().cursor_linear(),
            );
            self.waker.keep_damage();
            return;
        };
        self.motion.set(motion);
        // Two counters, two animators: `motion=` witnesses only the cursor,
        // `slide=` only the offset. Both can rise in the same frame; their sum
        // is **not** the number of drawn frames.
        if !motion.cursor_settled() {
            self.motion_frames.set(self.motion_frames.get() + 1);
        }
        if !motion.origin_settled() {
            self.slide_frames.set(self.slide_frames.get() + 1);
        }
        let theme = self.theme.get();
        let bottom = texture.texture.height() as f32;
        // **The offset's second write point**. On this arm neither
        // `frame()` nor `clear` is called, so the offset is **kept** — but an
        // animation is defined by *changing* between two content frames, and a
        // kept value cannot change. **Before** `move_caret`: the cursor's
        // rectangle bakes this offset in.
        self.set_origin(frame, motion, bottom);
        // The list is kept, only the cursor moves: the grid is not dirty, so
        // the glyph and rule lists are still valid. Entering the `Term` lock
        // 120 times a second would fight "the render path does not block"
        // right here.
        if let (Some(at), Some(text)) = (motion.position(), self.last_caret_text.get()) {
            frame.move_caret(
                at,
                text,
                theme.cursor_linear(),
                motion.alpha() * self.blink.get().alpha(),
                // **Focus is read fresh every frame**, not from what `Frame`
                // kept: this bit is `bt-gpu`'s own decision and the motion
                // frame reaches it too.
                self.caret_focused(),
            );
        }
        // The dock's static lists are kept, only the effects are printed
        // again (`move_caret`'s precedent).
        if !fx_idle {
            frame.set_dock_fx(glyph_fx.iter(), glyph_fx.clusters(), theme.cursor_linear());
        }
        // No CPU sample is **written**, and that is not a gap: `cpu_frame`
        // measures `session.frame`'s lock wait and that work does not exist in
        // this frame. The microseconds of a `truncate` + `push_caret` in the
        // same column would pull the p95 down — the same ban as the "fake
        // sample".
        //
        // **The GPU column does not follow this and cannot:** the completion
        // poll sees every submitted frame and the motion frame submits one
        // too, so `record_gpu` **sees** these frames. The poll also feeds
        // `FailureStreak`; exempting motion frames from it would hide draw
        // errors, so the split is not deliberate but **structural**. Its
        // consequence is a measurement scope item: `samples=` and `gpu_samples=`
        // count different frame populations (the GPU's includes motion
        // frames) and the two columns' p95 cannot be compared directly in a
        // run where the cursor glides.
        match self.draw(texture, theme.background_linear(), frame) {
            // A motion frame is a frame that **leaves** too: `quiet=` should
            // measure the tail after settling, not the moment the animation
            // started. The offset is published here too — this arm is the
            // only place refreshing the mouse mapping during slide frames.
            Ok(()) => {
                self.publish_origin(frame);
                self.last_frame_at.set(Some(now));
            }
            // **This arm's own stop**: on the damage
            // path the stop was the flag not being planted, here it cannot be
            // — the "no damage" branch does not sleep while an animation has
            // not settled. When the budget is spent the animation is finished
            // at its target, so the next tick finds neither damage nor pending
            // motion and sleeps. Without it, a permanent draw error would
            // print an error line at the refresh rate until the slide's
            // duration cap (0.7 s) — exactly what `FailureStreak` was written
            // to prevent.
            //
            // **Asynchronous errors** reach
            // `draw_failed` from the completion poll at the start of a tick,
            // where the return is not used either: that path's stop for the
            // animation is not `finish()` but the **duration cap**, so even
            // with the budget spent the error line lasts at most 0.7 s. The
            // way to close it is known (the poll could finish the animation on
            // a spent budget) and so is the cost: a second entry point into
            // `Motion`. Not done without a measured need.
            Err(e) => {
                if self.retry.draw_failed(&e) {
                    motion.finish();
                    self.motion.set(motion);
                    glyph_fx.finish();
                    // The effect lists in `Frame` empty too: the pacer sleeps
                    // and the next damage-free frame (blink's tick) would
                    // redraw the old lists frozen halfway without passing
                    // through `set_dock_fx`.
                    frame.set_dock_fx(
                        std::iter::empty(),
                        &Clusters::default(),
                        theme.cursor_linear(),
                    );
                    // The scroll bar ends too, for the same stop: a fade in
                    // flight would retry the failing draw on every tick until
                    // it ran out.
                    self.hide_scrollbar(frame);
                    bar.settled = true;
                }
            }
        }
        // **It sleeps right after the phase frame.** Without a pending
        // animation this frame was drawn only for blink's phase change; left
        // running, one more tick would be paid until the next vsync — two
        // ticks for two **visible** frames a second. If motion continues it
        // is not touched: its rhythm is vsync anyway.
        if motion.settled() && glyph_fx.is_empty() && bar.settled {
            self.waker.pacer().set_running(false);
            self.arm_clock(now);
        }
    }

    /// Hides the scroll bar at once and takes its thumb out of the kept
    /// frame — for the paths that can no longer draw it
    /// ([`Scrollbar::hide`]). The frame's slot is emptied too, or the next
    /// damage-free frame (blink's tick) would redraw the old thumb.
    fn hide_scrollbar(&self, frame: &mut Frame) {
        let mut bar = self.scrollbar.get();
        bar.hide();
        self.scrollbar.set(bar);
        // Hidden: no marks either, whatever the list holds.
        let theme = self.theme.get();
        frame.set_scrollbar(
            self.scrollbar_layout.get(),
            Look::HIDDEN,
            theme.foreground_linear(),
            &[],
            [
                theme.search_mark_linear(),
                theme.search_current_mark_linear(),
            ],
            &[],
            self.marks.borrow().block_colors(),
        );
    }

    /// The scroll bar's **single** step — the one function both arms call
    /// ([`scrollbar_step`] is its body): `fresh` is the content frame's
    /// layout, `None` keeps the last one (the motion frame). The theme is the
    /// frame path's kept copy, so the motion frame takes no lock.
    fn step_scrollbar(
        &self,
        frame: &mut Frame,
        now: f64,
        fresh: Option<ScrollbarLayout>,
    ) -> BarStep {
        let mut bar = self.scrollbar.get();
        let mut kept = self.scrollbar_layout.get();
        let step = scrollbar_step(
            &mut bar,
            &mut kept,
            fresh,
            now,
            // Reduce Motion and `snap` live in `Motion`; the bar reads them
            // here, where a widening it starts this tick is stamped.
            self.motion.get().snaps(),
            frame,
            self.theme.get().foreground_linear(),
            // The last content frame's marks: the motion frame never reaches
            // `bt-core`, so the bar fades over the marks it was shown with.
            &self.marks.borrow(),
        );
        self.scrollbar.set(bar);
        self.scrollbar_layout.set(kept);
        step
    }

    /// Calls the notifier if the alternate screen changed; nothing otherwise.
    ///
    /// **The gate is here, not in the notifier**: the notifier itself sends
    /// work to the main queue, and sending work every frame would silently
    /// break zero frames at idle — every job landing in the
    /// queue wakes the main thread. The comparison is a `Cell` read, so on an
    /// ordinary frame this function's cost is not measurable.
    ///
    /// The last seen value is written **before** the notifier is called: if
    /// the notifier ran synchronously (in a test) and came back here, the
    /// reverse order would produce a second notice.
    fn notice_alt_screen(&self) {
        let Some(notify) = self.alt_screen_changed.as_ref() else {
            return;
        };
        let now = self.session.alt_screen();
        if self.alt_screen.replace(now) == now {
            return;
        }
        notify();
    }

    /// The vertical origin **to draw** in this frame: the animation's row at
    /// this moment → pixels.
    ///
    /// **Two write points, one function**: the content frame calls it
    /// after `sync`, the motion frame before `move_caret`. A second
    /// computation meant a drift that shows as "the mouse is one row off".
    ///
    /// **One write for two consumers.** The pixel value is read back from
    /// `Frame`, not recomputed: this line is what makes the viewport and the
    /// mouse mapping see the same number — through the slide too.
    ///
    /// **Nothing is clipped in a resting frame** and it is the offset's
    /// *definition* that guarantees it, not `setViewport`'s clipping: the
    /// content is in `0..content_rows`, the offset is `rows - content_rows`,
    /// so the lowest filled row ends exactly at `rows` rows. An arbitrary
    /// offset (or a defect inflating `content_rows`) would push the bottom
    /// rows off the texture and the symptom would be "the last row is
    /// missing". **During the slide the offset is larger than its target** —
    /// the content flows up — so part of the lowest row is below the window
    /// in those frames: the new row rises from the bottom edge and settles
    /// into place when the slide ends. The direct consequence of the single
    /// viewport: all four lists move together, so a new row appearing
    /// in place while the others slide is not a representable thing.
    ///
    /// **The scroll fraction is the second deliberate exception**
    /// (`Frame::set_scroll_frac`, added by `Frame::set_origin_rows`): the grid
    /// is that much lower, that much of the bottom row is below the window
    /// (in a docked window below the dock's ground), and the strip opening at
    /// the top is covered by the fill channel's top row. At rest there is no
    /// fraction — the gesture settles on the nearest row.
    ///
    /// **It does not publish to the mouse mapping.** The offset is baked into
    /// the frame here but is written to [`Origin`] only once `draw` returns
    /// `Ok` ([`Self::publish_origin`]): in a frame that could not be encoded
    /// the previous frame stays on screen and a click must be translated
    /// against its offset.
    ///
    /// **This is also where the band joins**: the band's current height
    /// is written here from both frame paths and the grid's drawn origin is
    /// `origin − band` ([`compose`]). The offset keeps a `u16` target, it
    /// does not switch to a signed one — the join happens only in drawing.
    fn set_origin(&self, frame: &mut Frame, motion: Motion, bottom_px: f32) {
        compose(frame, motion, bottom_px, self.dock_rows.get());
    }

    /// Publishes the drawn offset **and the fill band's height** to the mouse
    /// mapping — only when `draw` returns `Ok`.
    ///
    /// A separate step, because [`Origin`]'s contract speaks of the
    /// **encoded** frame: in the `Err` arm the previous frame stays on screen
    /// and publishing this one would translate a click against an offset not
    /// on screen. It narrows the window, it does not close it — `Ok` means
    /// "submitted", not "on screen"; asynchronous completion can still fail,
    /// which is why the contract says "encoded" and not "drawn".
    ///
    /// Both values go in **one** write: they are the same frame's geometry,
    /// and published separately the mouse could translate against a new
    /// origin and an old band. The motion frame comes here too — the band is
    /// kept there (`Frame` is not cleared), so the value published through
    /// the slide is constant.
    fn publish_origin(&self, frame: &Frame) {
        // The block marks the pointer can take are the ones this frame drew:
        // none while the bar is thin or hidden.
        let drawn = frame
            .scrollbar_block_marks()
            .iter()
            .any(|(marks, _)| !marks.is_empty());
        let marks = self.marks.borrow();
        let moved = self.origin.set(
            frame.origin_px(),
            frame.fill_rows(),
            frame.dock_hit(),
            // The mouse's region follows the form: none in `Never`. A change
            // of form draws one frame ([`Scrollbar::set_mode`]), so it is
            // published here too.
            self.scrollbar
                .get()
                .mode()
                .region(self.scrollbar_layout.get()),
            if drawn { marks.blocks() } else { &[] },
        );
        drop(marks);
        // Only on a change: a notifier called every frame would wake the main
        // queue every frame.
        if moved && let Some(notify) = self.marks_published.borrow().as_ref() {
            notify();
        }
    }

    /// **The clock**: the third reason to ask for a frame (module header).
    ///
    /// Called as the pacer goes to sleep, and it has **two flavours**
    /// ([`due_clock`] looks at which one is due):
    ///
    /// - **Content flavour** — if there is a duration counter to advance
    ///   (`Cursor::next_tick`) it is asked through [`Waker::wake`], and
    ///   planting the damage flag is **right**: what the grid draws really
    ///   changes, so counting `content=` is in place.
    /// - **Motion flavour** — blink's phase change through [`Waker::resume`],
    ///   **without** planting damage: the grid does not change, only the
    ///   caret's alpha. The ban on motion protected the opposite (a motion
    ///   frame counting itself as content), so this arm obeys it. **The
    ///   completion poll of a frame still in flight rides this flavour too**
    ///   (the woken tick polls first, then finds nothing to draw and
    ///   sleeps again), `POLL_DELAY` after the sleep, and so does **the
    ///   scroll bar's hold** — the bar waits asleep and its fade is a motion
    ///   frame ([`crate::scrollbar`]).
    ///
    /// The stop condition is `None` **in each**: on the counter's side the
    /// command ended, the anchor left the screen or there is no integration;
    /// on blink's side the setting is off, the caret is not drawn or the
    /// inactivity period ran out; on the poll's side the queue is empty; on
    /// the scroll bar's side it is hidden. With all four `None` no tick is
    /// armed and the window returns to zero frames at idle.
    ///
    /// **Not armed with the gate closed:** in an occluded window the tick
    /// already pauses higher up, so nobody wakes to update an invisible
    /// counter. When visibility returns, the `Gate` asks for a frame and the
    /// clock is armed again from there.
    fn arm_clock(&self, now: f64) {
        // **The generation rises at every sleep point, even without arming.**
        // `after` cannot be cancelled; the only way to cancel is for the
        // pending tick to find its own generation void. Had the increment been
        // caught by the early returns below, the stop condition would take
        // effect one period late.
        let generation = self.clock_generation.fetch_add(1, Ordering::Relaxed) + 1;
        // A frame still in flight: its completion is polled once more.
        let poll = self.renderer.in_flight().then_some(now + POLL_DELAY);
        // **Four deadlines, one wakeup.** Whichever is due first is armed and
        // decides the flavour: the content tick plants damage (counting
        // `content=` is right, the grid really changes), blink, the poll and
        // the scroll bar's hold do not (only the caret's alpha changes /
        // nothing is drawn / only the bar fades). Armed separately, since
        // `after` cannot be cancelled, one would void the other's generation.
        let Some((due, damages)) = due_clock(
            self.content_deadline.get(),
            [
                self.blink.get().next_flip(),
                poll,
                self.scrollbar.get().next_deadline(),
            ],
        ) else {
            // No running counter, no blinking cursor, no frame in flight, no
            // scroll bar on screen: the window returns to zero frames at idle.
            return;
        };
        // A deadline in the past **saturates to zero**: a tick firing at once
        // asks for one frame more, an accumulated delay would be late forever.
        // An infinite/NaN stamp cannot be represented and no clock is armed —
        // not a panic path, the window wakes on the next damage anyway.
        let Ok(delay) = Duration::try_from_secs_f64((due - now).max(0.0)) else {
            return;
        };
        let token = Arc::clone(&self.clock_generation);
        let waker = self.waker.clone();
        self.waker.pacer().after(
            delay,
            Box::new(move || {
                if token.load(Ordering::Relaxed) == generation {
                    if damages {
                        waker.wake();
                    } else {
                        waker.resume();
                    }
                }
            }),
        );
    }
}

/// This frame's offset **target**, rows: how many of the grid's rows stay
/// empty at the top so the content sticks to the bottom.
///
/// **The content sticks to the bottom** is decided here, not in `bt-core`:
/// that side only says how many rows are filled (`Cursor::content_rows`);
/// where they stick is a layout decision and the drawer's (the
/// decision here, the painting there).
///
/// `saturating_sub`: the contract is `content_rows ≤ rows` (a `debug_assert`
/// in `bt-core`) and saturation drops the offset to zero in a release build,
/// i.e. a ceiling-aligned layout — wrapping would throw the grid off screen.
///
/// **A target, not the drawn value:** `crate::motion` closes the gap (the
/// slide) and the drawing side reads the offset from it
/// (`Core::set_origin`).
fn origin_target(cursor: Cursor) -> u16 {
    cursor.rows.saturating_sub(cursor.content_rows)
}

/// This frame's band **excess** target, rows: the drawn band's difference from
/// the PTY share. Zero in a frame without a dock (the alternate screen,
/// a shell without integration) — no band, the grid is not offset.
///
/// **Fractional and signed, one formula**: `(band_px − dock_px)
/// / cell_h`. With one or more input rows the difference is whole rows
/// (`input_rows − 1`; both have the inter-row gap), with zero input rows (a
/// remote session) it is **negative** and one cell plus the inter-row gap —
/// the band shrinks to the context row alone, the grid is drawn that much
/// lower and the fill band covers the strip opening at the top
/// (`Session::set_grid_top`). The formula's single copy is in pixels, so the
/// band's drawn height and the grid's offset come from the same number.
///
/// **No band** (`None`, a program reading the keyboard itself) is the
/// remote arm's extension: `band_px` is zero, so the excess is the whole PTY
/// share, negative — the grid is drawn at the window's bottom and the fill
/// band covers the share's height at the top. The PTY's size never moves.
fn band_target(input_rows: Option<u16>, dock_rows: u16, cell: CellMetrics) -> f32 {
    if dock_rows == 0 {
        return 0.0;
    }
    let cell_h = f32::from(cell.cell_px().1);
    // Against the share actually reserved: `DOCK_ROWS` normally, one row for a
    // remote session's status bar on the alternate screen — `band_px(0)` is
    // `dock_px(1)`, so there the excess is zero and vim's grid is not offset.
    (crate::frame::band_px(input_rows, cell) - crate::frame::dock_px(dock_rows, cell)) / cell_h
}

/// Writes the band's and the offset's **current** value into the frame — the
/// two frame paths' common point (`Core::set_origin`).
///
/// Order: the band first, then the offset; `Frame::origin_px` joins the two
/// when read (`offset − the band's excess`), so the order does not change the
/// result, but the band's top (`Frame::dock_top_px`) must be written
/// **before** the caret — the caret's slot looks at it.
///
/// Without a dock the band is not written at all: the "not said" `clear`
/// left adds zero excess to the grid and the caret's slot limit stays at
/// infinity. A separate function, because the composition guard runs it
/// without a `Core`.
///
/// The gate is the window's dock **and** this frame's open surface: leaving
/// the alternate screen, the window's share is back but the last content
/// frame may be dockless, and a motion frame running in between that wrote
/// the band would drop the caret on the grid's bottom row into an undrawn
/// dock slot and lose it.
fn compose(frame: &mut Frame, motion: Motion, bottom_px: f32, dock_rows: u16) {
    if dock_rows > 0 && frame.dock().is_some() {
        frame.set_dock_share(dock_rows);
        frame.set_dock_band(bottom_px, motion.band());
    }
    frame.set_origin_rows(motion.origin());
}

/// The dock caret's target, in **screen cells** — [`Motion`]'s space.
///
/// The vertical component is **not** an integer and cannot be: the dock band
/// starts a breathing gap lower and the band itself does not sit on the
/// grid's cell raster either (when the height is not a multiple of the cell,
/// the leftover strip stays between the dock and the content). A fractional
/// target is therefore not an evasion but the right answer.
///
/// **Why cells and not pixels:** `Motion`'s spring constants and stop
/// threshold are tuned in cells. Switching the space to pixels would change
/// that threshold silently and tie the animation's feel to an unmeasured
/// number.
///
/// **Bottom-aligned**: the top of input row `row`, in the bottom-aligned
/// layout of an `input_rows`-row band — not from the band's current
/// (animated) height, because the cells stand in the layout and the caret is
/// on them.
fn dock_caret_at(
    col: u16,
    row: u16,
    input_rows: u16,
    bottom_px: f32,
    cell: CellMetrics,
) -> [f32; 2] {
    let cell_h = f32::from(cell.cell_px().1);
    let pad = f32::from(cell.gutter_px());
    let top = bottom_px - crate::frame::band_px(Some(input_rows), cell);
    [f32::from(col), (top + pad) / cell_h + f32::from(row)]
}

/// The pacer's way back into the frame loop: [`Ticker::tick`] once per
/// refresh while running.
///
/// **Weak**: the platform's timer holds its target strongly (macOS's
/// `displayLinkWithTarget:selector:` retains the target), and a strong handle
/// here would close a cycle `timer → target → core → pacer → timer`. Once the
/// [`DisplayLink`] is gone a late tick finds nothing and does nothing.
///
/// Not `Send` (the core is an `Rc`): the pacer must tick on the thread that
/// created the link, and the type says so.
#[derive(Clone)]
pub struct Ticker(Weak<Core>);

impl Ticker {
    /// One refresh. `stamp` is on [`Pacer::now`]'s base: the target
    /// presentation time when the provider knows it, `now()` otherwise.
    pub fn tick(&self, stamp: f64, target: TickTarget) {
        if let Some(core) = self.0.upgrade() {
            core.tick(stamp, target);
        }
    }
}

/// The frame driver tied to the display's refresh.
///
/// Ownership chain: this type holds the core, the core holds `Session`,
/// `Renderer` and the surface. The pacer's timer holds only a weak
/// [`Ticker`], so the cycle does not close: `Session` → `Wake` → [`Waker`] →
/// pacer does not come back to the core with a strong reference.
pub struct DisplayLink {
    core: Rc<Core>,
    waker: Waker,
}

/// The frame path's **opening geometry**: the dock's width, the dock share and
/// the cell size.
///
/// The three are one type because they are born at the same moment from the
/// same place (`bt-shell`'s `Grid` and the dock decision) and enter
/// [`DisplayLink::new`] together. As separate parameters the signature went
/// past seven arguments — but that is not the real gain: a type says "these
/// three change together" in the signature.
///
/// **No** row count, on purpose: the grid's height is the session's
/// (`SessionOptions.rows`) and the frame path takes it from `Cursor::rows`
/// **in the same read** (`bt_core::Cursor::rows`' doc). A second copy is
/// banned exactly there.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    /// The dock's width, columns — the window's, the scroll bar's reserve
    /// not taken off (`Core::dock_cols`); needed for the dock to wrap its
    /// overflowing row. [`DisplayLink::resize`] refreshes it.
    pub dock_cols: u16,
    /// How many rows the dock has; `0` → no dock in this window.
    ///
    /// **The birth value is the session's constant** (is the
    /// integration installed) but this field is its state *at the moment*:
    /// the dock goes away on the alternate screen and comes back when leaving
    /// so [`DisplayLink::resize`] carries it too. Telling apart the
    /// two reasons that drop it to zero is `bt-shell`'s job — leaving the
    /// alternate screen in a session without integration must **not** give
    /// birth to a dock.
    pub dock_rows: u16,
    /// The cell size and gutter; the value `Frame::clear` carries.
    pub cell: CellMetrics,
}

impl DisplayLink {
    /// Built on the thread that will tick it (the main thread on macOS). The
    /// pacer is the platform's; `bt-shell` builds it, then hands it the
    /// [`DisplayLink::ticker`].
    ///
    /// The link is born **paused**: the first frame needs someone to ask for
    /// it too (`request_frame`).
    pub fn new(
        pacer: Arc<dyn Pacer>,
        surface: Rc<Surface>,
        renderer: Rc<Renderer>,
        session: Arc<Session>,
        layout: Layout,
        stats: Option<Arc<Stats>>,
        alt_screen_changed: Option<Box<dyn Fn()>>,
    ) -> Self {
        // The opening theme: the first content frame will refresh it anyway,
        // but there is no reason for the field to be an `Option` — the
        // session's theme is a valid answer at any moment. The alternate
        // screen's opening state is read for the same reason: the watch's
        // first comparison needs a value, and an "I don't know yet" state
        // would produce a fake transition on the first frame for a session not
        // born on the alternate screen.
        let alt_screen = session.alt_screen();
        let theme = session.theme();
        pacer.set_running(false);
        let waker = Waker {
            inner: Arc::new(WakerInner {
                dirty: session.dirty_flag(),
                pacer,
                gate: Gate::new(),
                requests: AtomicU64::new(0),
            }),
        };
        let retry = Retry {
            waker: waker.clone(),
            streak: FailureStreak::default(),
        };
        // The GPU delta is measured only while the measurement gate is open
        // (the timed path costs a readback and a closure per frame).
        renderer.set_gpu_timing(stats.is_some());
        let core = Rc::new(Core {
            renderer,
            surface,
            session,
            retry,
            waker: waker.clone(),
            stats,
            frame: RefCell::new(Frame::default()),
            blocks: RefCell::new(Blocks::default()),
            selection: RefCell::new(SelectionRuns::default()),
            search: RefCell::new(SearchRuns::default()),
            marks: RefCell::new(TrackMarks::default()),
            marks_stale: Cell::new(false),
            dock_selection: RefCell::new(Vec::new()),
            fill: RefCell::new(Vec::new()),
            dock: RefCell::new(DockState::default()),
            dock_context: RefCell::new(DockContext::default()),
            dock_rows: Cell::new(layout.dock_rows),
            alt_screen: Cell::new(alt_screen),
            alt_screen_changed,
            marks_published: RefCell::new(None),
            dock_cols: Cell::new(layout.dock_cols),
            cell: Cell::new(layout.cell),
            // Zero: no offset until the first content frame, and that frame
            // says the value. The mouse path reads a ceiling-aligned grid in
            // the meantime, so in the one-frame window at opening it sees
            // what is drawn too.
            origin: Origin::default(),
            content_frames: Cell::new(0),
            motion_frames: Cell::new(0),
            slide_frames: Cell::new(0),
            motion: Cell::new(Motion::default()),
            glyph_fx: RefCell::new(GlyphFx::default()),
            geometry_changed: Cell::new(false),
            last_frame_at: Cell::new(None),
            last_update_at: Cell::new(None),
            // Unused until the first content frame: the motion frame only
            // draws when `Motion` has a position, and the only place filling
            // it is the content frame — which refreshes the theme too.
            theme: Cell::new(theme),
            content_deadline: Cell::new(None),
            blink: Cell::new(Blink::default()),
            // Hidden, and no layout until the first content frame says where
            // the window stands: a poke before it is ignored.
            scrollbar: Cell::new(Scrollbar::default()),
            scrollbar_layout: Cell::new(ScrollbarLayout::default()),
            focused: Cell::new(true),
            keyboard: Cell::new(true),
            caret_style: Cell::new(CaretStyle::default()),
            blink_interval: Cell::new(bt_core::CURSOR_BLINK_INTERVAL),
            clock_generation: Arc::new(AtomicU64::new(0)),
            last_caret_text: Cell::new(None),
            last_caret_at: Cell::new(None),
        });
        Self { core, waker }
    }

    /// The handle the pacer ticks through ([`Ticker`]).
    pub fn ticker(&self) -> Ticker {
        Ticker(Rc::downgrade(&self.core))
    }

    /// The way to ask for frames from other threads; the `Wake`
    /// implementation holds this.
    pub fn waker(&self) -> Waker {
        self.waker.clone()
    }

    /// The end that reads the drawn frame's vertical origin (and the fill
    /// band's height above it); the mouse mapping holds this.
    ///
    /// The same pattern as [`Self::waker`] — a copy of the shared body — but
    /// the other way round: the `Waker` is **written** from outside, this is
    /// **read** from outside. The writing side is not opened on purpose: the
    /// origin's one owner is the frame path ([`Origin`]).
    pub fn origin(&self) -> Origin {
        self.core.origin.clone()
    }

    /// The number of frames asked for during the run — asked for, not drawn.
    ///
    /// The report prints this as the `requests=` token; its difference from
    /// `frames` is the requests merged and dying at the gate (see
    /// `WakerInner::requests`).
    pub fn requests(&self) -> u64 {
        self.waker.requests()
    }

    /// The zero-frames-at-idle gate's operand: content frames **decided** to
    /// be drawn. The report prints this as the `content=` token.
    ///
    /// **No order relation** to `frames`, and mixing them up means misreading
    /// the gate: a motion frame submits a frame too, so it is written to
    /// `frames` and not here ([`Self::motion_frames`]). The measured healthy
    /// smoke run had `frames` 27–30 while `content` was 2–3; most of the
    /// difference is the cursor glide, the rest
    /// frames that could not be encoded and frames left in flight. The reader
    /// of a red run uses this too: all three high means damage flowing, only
    /// `frames` high means an animation not settling.
    pub fn content_frames(&self) -> u64 {
        self.core.content_frames.get()
    }

    /// Frames drawn because the **cursor** animation had not settled — the
    /// `motion=` token. A pure cursor witness since the slide got its own
    /// counter: frames drawn only
    /// for the slide are counted by `slide=` and this counter does not see
    /// them.
    ///
    /// The smoke gate's **required** counter: the recipe has a cursor move
    /// (`bt_core::smoke_shell`), so zero means "the animation never ran". It
    /// does not enter `content=`, and that is the gate itself.
    pub fn motion_frames(&self) -> u64 {
        self.core.motion_frames.get()
    }

    /// Frames drawn because the **slide** had not settled — the `slide=`
    /// token.
    ///
    /// [`Self::motion_frames`]' sibling, not its summand: both can rise in
    /// the same frame. A **counter, not a gate** — its threshold was not
    /// measured (`Core::slide_frames`).
    pub fn slide_frames(&self) -> u64 {
        self.core.slide_frames.get()
    }

    /// Whether the animation stopped — the half of the gate that **needs no
    /// measurement**.
    ///
    /// The timed run asks this once at the deadline: `false` makes the run
    /// red (`Verdict::MotionUnsettled`). Being independent of speed is its
    /// whole value — `IDLE_FRAME_LIMIT` only sees a fast enough leak, this
    /// question sees **every** animation whose stop condition was forgotten,
    /// however slow.
    ///
    /// The limit of what it sees: only animations going through
    /// [`crate::motion`], the dock's typing effects ([`crate::glyph_fx`]) and
    /// the scroll bar ([`crate::scrollbar`]). A path that skips the
    /// infrastructure and asks for frames on its own is invisible to this
    /// question; its gate is [`Self::quiet_since`]'s measured threshold.
    ///
    /// The scroll bar is asked at **the last tick's stamp**, not a fresh
    /// clock read: the question is whether the link left anything in flight,
    /// and a bar asleep in its hold is settled — its clock is armed. A bar
    /// appearing or fading is not.
    pub fn motion_settled(&self) -> bool {
        let core = &self.core;
        let now = core.last_update_at.get().unwrap_or(0.0);
        core.motion.get().settled()
            && core.glyph_fx.borrow().is_empty()
            && core.scrollbar.get().settled(now)
    }

    /// Time since the last drawn frame — the `quiet=` token. `None` → no
    /// frame drawn.
    ///
    /// **The run's one clock read.** On the frame path the stamp is a field
    /// copy (`Core::last_frame_at`); [`Pacer::now`] is called only here, at
    /// the deadline, once. The contract of no clock reads per frame with the
    /// measurement gate (`BT_FRAME_STATS`) closed rests on this split.
    ///
    /// **What it measures between:** the stamp is the frame's *target
    /// presentation* time, i.e. a point in the future. If the deadline falls
    /// within one refresh of the last frame the difference is negative; the
    /// value saturates to zero. The reader should read `quiet=0.00ms` as
    /// "frames were flowing at the deadline", not "drawn exactly then".
    ///
    /// **The gate's most sensitive layer** and evaluated outside `bt-gpu`: the
    /// threshold is a measured contract (`bt-shell`'s `QUIET_FLOOR`) and red
    /// below it on the smoke load. The responsibility here is
    /// only producing the number honestly — `None` "no frame drawn",
    /// `0.00ms` "frames were flowing at the deadline".
    ///
    /// The limit of what it sees is [`Self::motion_settled`]'s complement:
    /// that one sees an animation going through the infrastructure regardless
    /// of speed; this one sees **every** frame source skipping the
    /// infrastructure, but only if its period is shorter than the threshold.
    pub fn quiet_since(&self) -> Option<Duration> {
        let last = self.core.last_frame_at.get()?;
        let now = self.waker.pacer().now();
        Some(Duration::try_from_secs_f64(now - last).unwrap_or(Duration::ZERO))
    }

    /// Waits (bounded) for the frames still in flight and counts them — the
    /// pending poll at shutdown, which comes **before** the report reads
    /// `frames=`. The link is already stopped: no tick would count
    /// them otherwise.
    pub fn drain(&self) {
        self.core.renderer.wait_in_flight(DRAIN_TIMEOUT);
        self.core.complete();
    }

    /// Ask for a frame.
    ///
    /// What matters to its caller is not that the grid changed but that
    /// **what was drawn is no longer valid**: the texture size moved, the
    /// occlusion lifted. So it plants the damage flag too — and does it with
    /// the [`Waker`], so "ask for a frame" has one definition. Its cost is one
    /// frame; the stop condition is the tick itself.
    pub fn request_frame(&self) {
        self.waker.wake();
    }

    /// Scrolling **input** arrived — the wheel, a page scroll, a search jump —
    /// so the scroll bar shows ([`crate::scrollbar`]). Main thread, at the
    /// shell's scroll gates.
    ///
    /// **Input, never output**: nothing on the frame path calls this, so a
    /// window scrolled up while output streams below keeps its bar hidden.
    ///
    /// Ignored when the last content frame had no bar to draw (no travel,
    /// the alternate screen): a poke there would wake the link for invisible
    /// fade frames. Otherwise the poke is a bit the next tick stamps and the
    /// link is started through [`Waker::resume`] — **never** [`Waker::wake`]
    /// for the bar itself: its frames change only the bar, so they are motion
    /// frames and stay out of `content=` and `requests=`. The one content
    /// frame a showing bar asks for is for marks owed to it
    /// ([`DisplayLink::marks_changed`]).
    pub fn poke_scrollbar(&self) {
        let core = &self.core;
        let mut bar = core.scrollbar.get();
        let wanted = bar.poke(core.scrollbar_layout.get().drawable());
        core.scrollbar.set(bar);
        self.show_scrollbar(wanted);
    }

    /// Starts the link for a bar that is coming up: a motion frame, through
    /// [`Waker::resume`] — unless its marks changed while it was down
    /// ([`DisplayLink::marks_changed`]): then the frame is a **content** one,
    /// [`Waker::wake`], because what it draws first is the new marks and
    /// those come only from `bt-core`. Not the bar asking for content: the
    /// marks are content that was left undrawn while nobody could see it.
    fn show_scrollbar(&self, wanted: bool) {
        if !wanted {
            return;
        }
        if self.core.marks_stale.get() {
            self.waker.wake();
        } else {
            self.waker.resume();
        }
    }

    /// Sets who is told when the drawn block marks change under a still
    /// pointer — the bar widened and drew them, output moved them, the thumb
    /// moved over them — so the pointer's hand and tip follow without a
    /// mouse move. `bt-shell` re-asks the pointer's place on its next turn;
    /// the notifier must only send that work to the main queue.
    pub fn on_marks_published(&self, notify: Box<dyn Fn()>) {
        self.core.marks_published.replace(Some(notify));
    }

    /// The scroll bar's marks of the whole history changed — a search pass
    /// reached the top with different rows (`bt_core::SearchReport::marks_changed`).
    /// Main thread, from `bt-shell`'s search driver.
    ///
    /// **A content frame**, [`Waker::wake`]: the marks are bucketed in
    /// `bt-core`'s frame, so only a content frame draws them — but only while
    /// the bar is up or on its way ([`Scrollbar::up`]). A bar that is down
    /// owes them instead: the next time it shows it asks for the content
    /// frame itself, so a pass that ends unseen costs no frame. The stop
    /// condition is the pass's end, which sends this once.
    pub fn marks_changed(&self) {
        let core = &self.core;
        if core.scrollbar_layout.get().drawable() && core.scrollbar.get().up() {
            self.waker.wake();
        } else {
            core.marks_stale.set(true);
        }
    }

    /// The pointer came over the scroll bar's strip or left it
    /// ([`Origin::scrollbar`]'s region): over it the bar shows wide and its
    /// thumb darkens. Main thread, from `bt-shell`'s tracking area — which
    /// sees the pointer over an unfocused pane and leaving the window too.
    ///
    /// **A no-op on the same value; a change starts the link through
    /// [`Waker::resume`]**, never [`Waker::wake`] — the widening is a motion
    /// frame (but for marks owed to it, [`DisplayLink::marks_changed`]) — so
    /// a sleeping link widens the bar at once, not a second later.
    /// While the pointer stays the bar is settled and asks for nothing.
    /// Where no bar can be drawn the change wants no frame.
    pub fn set_scrollbar_hover(&self, on: bool) {
        let core = &self.core;
        let mut bar = core.scrollbar.get();
        let wanted = bar.set_hover(on, core.scrollbar_layout.get().drawable());
        core.scrollbar.set(bar);
        self.show_scrollbar(wanted);
    }

    /// The thumb was grabbed or let go — [`DisplayLink::set_scrollbar_hover`]'s
    /// rule: held, the bar stays up and wide at its darkest tone however far
    /// the pointer wanders; let go off the strip, it narrows, holds and fades.
    pub fn set_scrollbar_drag(&self, on: bool) {
        let core = &self.core;
        let mut bar = core.scrollbar.get();
        let wanted = bar.set_drag(on, core.scrollbar_layout.get().drawable());
        core.scrollbar.set(bar);
        self.show_scrollbar(wanted);
    }

    /// The scroll bar's form changed — `bt-shell` gives the **resolved**
    /// value (the setting combined with the system's preference; this crate
    /// sees neither, the [`DisplayLink::set_reduce_motion`] precedent).
    ///
    /// **A no-op on the same value; a change starts the link through
    /// [`Waker::resume`]**, never [`Waker::wake`]: the new form changes only
    /// the bar, so the tick draws it as a motion frame over the kept layout
    /// and `content=` does not rise (but for marks owed to it,
    /// [`DisplayLink::marks_changed`]). When `Always` comes or goes the grid's
    /// width changes too — that is `bt-shell`'s [`DisplayLink::resize`], which
    /// asks for its own content frame.
    pub fn set_scrollbar_mode(&self, mode: Mode) {
        let core = &self.core;
        let mut bar = core.scrollbar.get();
        let changed = bar.set_mode(mode);
        core.scrollbar.set(bar);
        self.show_scrollbar(changed);
    }

    /// The window's visibility changed.
    ///
    /// While invisible both drawing and the **rhythm** stop: the pacer is
    /// paused, the tick returns early and the `Waker` never starts it again
    /// (it still plants damage). When visibility returns a frame is asked for
    /// — the compositor may have thrown away the layer's content while
    /// occluded, so it must be redrawn even if the content is the same.
    pub fn set_visible(&self, visible: bool) {
        self.waker.gate().set_open(visible);
        if visible {
            self.request_frame();
        } else {
            // The slide in flight is **finished at its target**: the pacer
            // stops, so `advance` never runs again and the animation would
            // stay "unsettled" forever — the timed run would say
            // `MotionUnsettled` at the deadline while the code is right. The
            // full reason is in [`Motion::finish`].
            let core = &self.core;
            let mut motion = core.motion.get();
            motion.finish();
            core.motion.set(motion);
            // The typing effects too: an effect frozen in a background tab
            // would resume from a phase never seen when it comes back.
            core.glyph_fx.borrow_mut().finish();
            // And the scroll bar: no tick plays its fade while hidden, and a
            // bar left appearing or fading would count as unsettled until the
            // window came back.
            core.hide_scrollbar(&mut core.frame.borrow_mut());
            self.waker.pacer().set_running(false);
        }
    }

    /// The cursor's glide style changed: the user saved `settings.toml` or
    /// the window is opening (`bt-shell` gives the resolved value, the
    /// `Renderer::set_font` precedent — `bt-gpu` does not see the settings
    /// file).
    ///
    /// **The reason it asks for a frame is the shape of the "no damage"
    /// branch:** there a settled animation puts the pacer to sleep without
    /// drawing. A user switching to `snap` gets the cursor in flight finished
    /// at its target ([`Motion::set_style`]), but that new position reaches
    /// the screen only if a frame is drawn — without the request the cursor
    /// would hang on an intermediate cell and the thing putting it in place
    /// would be some unrelated shell output. The request goes only when a
    /// glide is really finished: a save writing the same style again and a
    /// settled cursor are no-ops (`Session::set_theme`'s rule of not swapping
    /// the same theme).
    ///
    /// Switching to the other two styles asks for no frame: if the glide in
    /// flight continues the pacer is awake anyway and the next motion frame
    /// applies the new style.
    pub fn set_cursor_motion(&self, style: CursorMotion) {
        let core = &self.core;
        let mut motion = core.motion.get();
        let mut finished = motion.set_style(style);
        core.motion.set(motion);
        // `snap` turns the typing effects off too (`Motion::glyph_fx`) and the
        // ones in flight end at their target — same reason, same frame
        // request.
        if style == CursorMotion::Snap {
            let mut glyph_fx = core.glyph_fx.borrow_mut();
            finished |= !glyph_fx.is_empty();
            glyph_fx.finish();
        }
        if finished {
            self.request_frame();
        }
    }

    /// The dock's typing effects changed (`[motion] keypress` / `erase`): the
    /// user saved `settings.toml` or the window is opening.
    ///
    /// The names come **raw** — unlike [`DisplayLink::set_cursor_motion`],
    /// there is nothing for `bt-shell` to resolve here: `snap`'s and Reduce
    /// Motion's reduction is in the same place as the cursor's mode, in
    /// `bt-gpu` (`Motion::glyph_fx`), and both inputs are already in the
    /// link.
    ///
    /// The change finishes the ones in flight ([`GlyphFx::set_effects`]) and
    /// asks for a frame if something was finished — `set_cursor_motion`'s
    /// reason: the "no damage" branch sleeps without drawing a settled
    /// animation, and without the request a half-transparent letter would
    /// hang on screen. The same choice and an empty list are no-ops.
    pub fn set_glyph_fx(&self, keypress: Keypress, erase: Erase) {
        let finished = self.core.glyph_fx.borrow_mut().set_effects(keypress, erase);
        if finished {
            self.request_frame();
        }
    }

    /// Reduce Motion was switched on or off — `bt-shell` gives the
    /// **resolved** value: it combines the three-valued `reduce_motion` with
    /// the system's answer, and `bt-gpu` sees neither the settings file nor
    /// `NSWorkspace` (the same pattern as [`DisplayLink::set_cursor_motion`]).
    ///
    /// **Both directions may ask for a frame** and the reason is again the
    /// shape of the "no damage" branch: the animation in flight is finished
    /// at its target in both directions ([`crate::motion::Motion::set_reduce`])
    /// and a settled animation sleeps on that branch without drawing —
    /// without the request the cursor would hang on an intermediate cell or
    /// half-transparent. A no-op for a settled cursor and the same value.
    pub fn set_reduce_motion(&self, reduce: bool) {
        let core = &self.core;
        let mut motion = core.motion.get();
        let changed = motion.reduce() != reduce;
        let finished = motion.set_reduce(reduce);
        core.motion.set(motion);
        // The typing effects end in both directions too (`Motion::set_reduce`'s
        // reason); the frame request comes from `changed` below.
        if changed {
            core.glyph_fx.borrow_mut().finish();
        }
        // **The change itself asks for a frame, not only a half-finished
        // animation.** The old form was right while `Motion` owned every
        // animation; blink lives outside it and its gate is read only on a
        // **content** frame (combined with `Cursor::blink`). In an idle window
        // switching Reduce Motion on without a frame request would leave the
        // blink fading — the rule "while on, blink never starts" would be
        // a lie.
        if finished || changed {
            self.request_frame();
        }
    }

    /// The cursor's drawing numbers changed — `bt-shell` gives them from the
    /// settings file.
    ///
    /// **A no-op on the same value, a frame on change**, and the reason is
    /// its siblings' ([`DisplayLink::set_cursor_motion`],
    /// [`DisplayLink::set_focused`]): a radius saved in an idle window would
    /// never reach the screen until the next damage and the user would think
    /// the setting does not work.
    pub fn set_caret_style(&self, style: CaretStyle) {
        if self.core.caret_style.replace(style) == style {
            return;
        }
        self.request_frame();
    }

    /// Blink's half period changed — `bt-shell` gives it from the settings
    /// file.
    ///
    /// **The pending tick is rebuilt** ([`crate::blink::Blink::set_half_period`])
    /// and a frame is asked for; both are required. An armed wakeup **cannot
    /// be cancelled** (`arm_clock`), so in a sleeping window writing only the
    /// field would delay the new rhythm until the next flip — the user saves
    /// and nothing happens.
    ///
    /// **The value goes to a slot, the tick is not armed here**: this path
    /// runs outside the tick and has no frame stamp in hand. The store's one
    /// time base is the tick's stamp; reading a clock here would create a
    /// second time. It is applied on the frame path, when the requested frame
    /// arrives.
    pub fn set_blink_interval(&self, half_period: f64) {
        if self.core.blink_interval.replace(half_period) == half_period {
            return;
        }
        self.request_frame();
    }

    /// The keyboard came to the terminal or left — `bt-shell`'s view gives it
    /// when it becomes/resigns first responder (the search panel's
    /// field).
    ///
    /// [`DisplayLink::set_focused`]'s rule: a no-op on the same value, a frame
    /// on change — the caret will hollow or fill, blink will stop or start.
    pub fn set_keyboard_in_terminal(&self, keyboard: bool) {
        if self.core.keyboard.replace(keyboard) == keyboard {
            return;
        }
        self.request_frame();
    }

    /// The window's focus changed — `bt-shell`'s `NSWindowDelegate` gives it.
    ///
    /// **A no-op on the same value** (precedent
    /// [`crate::Session::set_theme`]): the opening `windowDidBecomeKey:`
    /// falls exactly on this path and would write a free content frame.
    ///
    /// **The change itself asks for a frame**, and the reason is
    /// `set_reduce_motion`'s: the caret will hollow or fill, blink will stop
    /// or start — an idle window would show none of it until the next damage.
    pub fn set_focused(&self, focused: bool) {
        if self.core.focused.replace(focused) == focused {
            return;
        }
        self.request_frame();
    }

    /// Cuts the rhythm **for good**: the wake latch drops and the pacer is
    /// torn down. No way back — `set_visible(true)` does nothing any more
    /// either, and that is not a promise but the `stopped` latch itself.
    ///
    /// The shutdown path calls this instead of `Drop` because the
    /// `DisplayLink` itself **must live** through the shutdown (the reason is
    /// in `bt-shell`'s shutdown order). The waking side closes too: left open,
    /// the reader's last `Wakeup`s would keep sending work to the main queue
    /// and keep the main thread busy during shutdown.
    pub fn stop(&self) {
        // Idempotent through the latch, not by assuming the platform's
        // teardown is — `Drop` comes here too.
        if self.waker.gate().is_stopped() {
            return;
        }
        self.waker.gate().stop();
        self.waker.pacer().stop();
    }

    /// The window geometry moved: update the grid and the cell size, ask for
    /// a frame.
    ///
    /// `Session::resize` returns early and marks nothing if **no** component
    /// of the size (columns, rows, cell pixel size) changed, while the texture
    /// size may have: a drag that does not cross a cell boundary leaves the
    /// grid the same, `windowDidChangeBackingProperties:` may fire without the
    /// scale moving. So `request_frame` asks for the frame unconditionally;
    /// otherwise the layer would stretch the old texture.
    ///
    /// The cell pixel size is applied **only if the session accepts it**: a
    /// degenerate size is ignored (a minimised window computes 0 columns) and
    /// applying it here would leave the grid at the old size and shift the
    /// drawing to the new one — it would also drift from the `TIOCSWINSZ` the
    /// PTY knows. **The gutter goes through the same gate.** With split gates,
    /// on a rejected size the gutter would be new, the grid old, and the
    /// glyphs would shift from the `cols` computation. **But "already the
    /// same" is not a rejection**: a metric whose cell size is the grid's
    /// own — where only the gutter or the scale moved (a zoom and a display
    /// change landing on the same cell, a fractional scale rounding the
    /// gutter and the cell apart) — is applied too. Kept out, the gutter would
    /// shift `Frame::pos_at` against `point_to_cell` and the scroll bar would
    /// keep its point sizes at the old scale. The one rejection that remains
    /// with an unchanged cell size is a degenerate grid (zero columns or
    /// rows), where nothing is drawn.
    /// **The cursor snaps in this frame.** On a geometry change the cursor did
    /// not move, the grid under it did — an animation would show
    /// it coming from where it never was. The flag is planted
    /// unconditionally, not tied to `Session::resize`'s acceptance: the window
    /// may have moved even if the cell size did not.
    pub fn resize(&self, cols: u16, rows: u16, cell: CellMetrics, dock_rows: u16, dock_cols: u16) {
        let core = &self.core;
        // The rest of the metric (gutter, scale, rule) follows the cell size
        // whenever the cell size is the one the grid already has: the gate
        // keeps a **rejected cell size** out, and an unchanged one is not
        // rejected — `Session::resize` only says "already the same".
        if core.session.resize(cols, rows, cell.cell_px())
            || core.cell.get().cell_px() == cell.cell_px()
        {
            core.cell.set(cell);
        }
        // The dock share is **outside the gate** and for `cols`' reason: at a
        // rejected size (a minimised window) the dock draws nothing anyway,
        // but leaving the share at the old value would bring the dock back one
        // frame late when leaving the alternate screen.
        core.dock_rows.set(dock_rows);
        // The dock's column count is **outside the gate**: the dock's wrapping
        // must see the drawn width, and at a rejected size (a minimised
        // window) it is zero anyway — the dock draws no text in that frame
        // (`bt_core::dock::render`), so it does nothing contradicting the grid
        // staying at the old size. `cols` itself is the grid's and goes only to
        // the session: in the scroll bar's `Always` form the two differ by the
        // track (`Core::dock_cols`).
        core.dock_cols.set(dock_cols);
        core.geometry_changed.set(true);
        // The typing effects end too (the sibling of `Motion`'s geometry
        // snap): when the column count or the cell changes, the dock's
        // wrapping may shift without a new mirror and the ones in flight
        // would stay on their old columns over another letter.
        core.glyph_fx.borrow_mut().finish();
        self.request_frame();
    }
}

impl Drop for DisplayLink {
    fn drop(&mut self) {
        // The platform's timer holds its target: without tearing it down the
        // tick would keep firing (weakly, into nothing) — a battery drain with
        // no symptom. On the creating thread: `DisplayLink` is not `Send`, it
        // drops where it was born. Guarded by the `stop` latch: if the
        // shutdown path already called it, this does nothing.
        self.stop();
    }
}

/// Turns the duration counter's tick into an **absolute** deadline; `None`
/// **clears** the pending deadline.
///
/// A separate function, for `arm_clock`'s reason: the defect itself lived
/// here and could not be tested inside the tick's body. `None` clearing is
/// a lesson learned the hard way — a finished command's stale deadline would ask for
/// one frame too many.
fn content_deadline(now: f64, tick: Option<Duration>) -> Option<f64> {
    tick.map(|tick| now + tick.as_secs_f64())
}

/// Which of the clock's deadlines is due first and **which flavour** it wants
/// (`true` → the damage-planting content flavour, `false` → the damage-free
/// motion flavour). `motion` is the motion flavour's deadlines: blink's phase
/// change, the completion poll of a frame still in flight (it draws nothing)
/// and the scroll bar's hold; the nearest of them competes with the content
/// tick.
///
/// A separate function, because the new guise of a defect fixed earlier
/// lives exactly here and could not be tested inside `arm_clock`'s
/// body.
///
/// **Contract:** the content deadline is **not affected** by blink's ticks
/// or the poll. In the old form the clock held a duration and was rebuilt at
/// every sleep point; a blink waking twice a second would push a running
/// command's one-second tick one second forward every time and it would
/// never fire. **On a tie the content wins**: the frame will be drawn anyway,
/// the motion flavour needs no second wakeup.
fn due_clock(content: Option<f64>, motion: [Option<f64>; 3]) -> Option<(f64, bool)> {
    let motion = motion.into_iter().flatten().reduce(f64::min);
    match (content, motion) {
        (Some(content), Some(motion)) if motion < content => Some((motion, false)),
        (Some(content), _) => Some((content, true)),
        (None, Some(motion)) => Some((motion, false)),
        (None, None) => None,
    }
}

/// The "no damage" branch's sleep question: motion settled, blink's phase
/// did not turn, **before this step** nothing was in flight in the typing
/// effects, and the scroll bar is idle ([`BarStep::idle`]).
///
/// The effect's question looks at the state before `advance` and that is
/// required: the last state of an effect finishing in this step (an arrival
/// settled on its static glyph, a ghost gone) has not been drawn yet; sleeping
/// would leave a half-transparent letter hanging on screen. The frame the list
/// empties in is drawn, the next tick sleeps. The bar's term carries the same
/// requirement its own way: a step that changed the opacity is drawn even if
/// the bar settles in it.
fn at_rest(motion: Motion, flipped: bool, fx_idle: bool, bar_idle: bool) -> bool {
    motion.settled() && !flipped && fx_idle && bar_idle
}

/// The "no damage" branch's second question, once [`at_rest`] said "stay
/// awake": does this tick have anything to draw? Not when everything but the
/// scroll bar is at rest and the bar's opacity did not move — the bar is in
/// flight (a poke just stamped on a hidden bar, the hold about to end) and
/// the next tick will have the change.
fn nothing_to_draw(motion: Motion, flipped: bool, fx_idle: bool, bar: BarStep) -> bool {
    motion.settled() && !flipped && fx_idle && !bar.changed
}

/// One tick's scroll bar answer — the bar's terms of the sleep question.
#[derive(Clone, Copy, Debug, PartialEq)]
struct BarStep {
    /// The opacity differs from the last one drawn: this tick has something
    /// to draw.
    changed: bool,
    /// The bar needs no frame after this one ([`Scrollbar::settled`]) — the
    /// question **after** drawing, the motion arm's tail.
    settled: bool,
}

impl BarStep {
    /// The question **before** drawing ([`at_rest`]): nothing new to draw
    /// and nothing in flight. A step that settles the bar but changed its
    /// opacity is not idle — its frame is the last one, and sleeping before
    /// it would leave a half-faded thumb on screen.
    fn idle(self) -> bool {
        !self.changed && self.settled
    }
}

/// The body of the scroll bar's single step ([`Core::step_scrollbar`]),
/// without the link: the state advances to `now`, the layout is `fresh` (a
/// content frame) or the kept one (a motion frame), the thumb is written into
/// the frame — `None` once hidden, or a kept frame would go on drawing it —
/// and the sleep terms come back.
///
/// **No waker anywhere in here**: the step only answers; the link's clock and
/// sleep question use the answer in the motion flavour, so the bar never
/// counts in `content=` or `requests=`.
// The step's inputs are the bar's state, its two layouts, the clock and
// the frame's colours and marks; gathering them in a struct would create a
// type only for this call (`Session::frame`'s precedent).
#[allow(clippy::too_many_arguments)]
fn scrollbar_step(
    bar: &mut Scrollbar,
    kept: &mut ScrollbarLayout,
    fresh: Option<ScrollbarLayout>,
    now: f64,
    instant: bool,
    frame: &mut Frame,
    foreground: LinearRgba,
    marks: &TrackMarks,
) -> BarStep {
    if let Some(layout) = fresh {
        *kept = layout;
    }
    let changed = bar.advance(now, kept.drawable(), instant);
    frame.set_scrollbar(
        *kept,
        bar.look(now),
        foreground,
        marks.search(),
        [marks.match_color(), marks.current_color()],
        marks.blocks(),
        marks.block_colors(),
    );
    BarStep {
        changed,
        settled: bar.settled(now),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::DOCK_ROWS;

    #[test]
    fn the_origin_publishes_the_drawn_block_marks_for_the_pointer() {
        // The marks the frame drew are what the pointer takes, by the
        // layout's own rectangles; a frame that drew none leaves none.
        let cell = CellMetrics::new(8, 16, 8, 8, 1, 1.0).expect("metrics");
        let position = bt_core::ScrollPosition {
            room: 100,
            top: 40.0,
            visible: 20,
        };
        let layout = ScrollbarLayout::new(Some(position), 400.0, 260.0, cell);
        let marks = [10.0, 50.0].map(|position| TrackBlock {
            position,
            color: 0,
            handle: BlockHandle::default(),
        });
        let origin = Origin::default();
        assert!(
            origin.set(0.0, 0, None, layout, &marks),
            "new marks, no news"
        );
        assert!(
            !origin.set(0.0, 0, None, layout, &marks),
            "the same marks told"
        );
        let target = layout.block_target(50.0);
        let (x, y) = ((target[0] + target[2]) / 2.0, (target[1] + target[3]) / 2.0);
        assert_eq!(
            origin.block_at(x, y),
            Some((BlockHandle::default(), target))
        );
        assert_eq!(origin.block_targets().len(), 2);
        assert_eq!(
            origin.block_at(x - 30.0, y),
            None,
            "the grid's point took a mark"
        );
        assert!(origin.set(0.0, 0, None, layout, &[]), "gone marks, no news");
        assert_eq!(origin.block_at(x, y), None, "a mark the frame did not draw");
        assert!(origin.block_targets().is_empty());
    }

    #[test]
    fn the_dock_caret_target_lands_on_the_band_not_the_grid_row() {
        // **The fractional target is not a dodge, it is the right answer.** The
        // dock band slips off the grid's cell lattice in two ways: it starts
        // lower by the breathing gutter, and when the window height is not
        // exactly divisible by the cell height the band itself sits below the
        // leftover stripe. Had we rounded the target to an integer, the caret
        // would sit up to a cell too high.
        let cell = CellMetrics::new(9, 18, 9, 8, 1, 1.0).expect("metrics");
        // A 600 px window, a two-row dock: 2×18 rows + 2×8 outer gutter +
        // 1×16 row gap = 68, so the band starts at 532.
        let dock_top = 600.0 - crate::frame::dock_px(2, cell);
        assert_eq!(dock_top, 532.0);

        let [col, row] = dock_caret_at(3, 0, 1, 600.0, cell);
        assert_eq!(col, 3.0, "the column is in the same space as the grid");
        // The caret is on the band's **first row**, i.e. below the outer gutter: (532+8)/18.
        assert_eq!(row, 540.0 / 18.0);
        // And that row is **below** the grid's last row (548/18 = 30.4): had it
        // been rounded, the two would collide.
        assert!(row > dock_top / 18.0, "the caret did not land on the band");

        // **Bottom-anchored**: in a three-input-row band the band's top is
        // two rows higher (600 − 104 = 496), the first row is at (496+8)/18 and
        // the **last** row is in the same place as the one-row band's row — the
        // band grows upward, the row the caret types on does not move.
        assert_eq!(dock_caret_at(3, 0, 3, 600.0, cell)[1], 504.0 / 18.0);
        assert_eq!(dock_caret_at(3, 2, 3, 600.0, cell)[1], row);
    }

    #[test]
    fn the_grid_the_fill_band_and_the_dock_band_meet_in_every_frame() {
        // **Composition guard** (`n = 3`): the grid's bottom edge,
        // the fill band and the dock band's top edge coincide **in the same
        // frame** — in the middle of the animation too. Testing the components
        // separately is not enough: the band and the offset are two separate
        // animators, the place they combine is drawing (`compose`), and two
        // separate roundings could diverge by a pixel.
        //
        // @1x, 9×18 cell, gutter 8, 600 px window: the PTY gutter is `2·18 + 2·8
        // + 16 = 68`, the grid is `⌊532/18⌋ = 29` rows and the leftover stripe
        // is 10 px. The stripe must stay 10 px while the band grows too: the
        // grid goes up together with the band.
        let cell = CellMetrics::new(9, 18, 9, 8, 1, 1.0).expect("metrics");
        const BOTTOM: f32 = 600.0;
        const ROWS: f32 = 29.0;
        const FILL: u16 = 2;
        let strip = BOTTOM - crate::frame::dock_px(DOCK_ROWS, cell) - ROWS * 18.0;
        assert_eq!(strip, 10.0);

        // The content is bottom-anchored (offset 5), the band is one row; then
        // the dock wants three input rows: the band's excess goes 0 → 2.
        let mut motion = Motion::default();
        motion.sync(Some([0.0, 30.0]), 5, 0.0, 0, false, false);
        motion.sync(Some([0.0, 30.0]), 5, 2.0, 0, false, false);
        let mut frames = 0;
        let mut mid = false;
        loop {
            let mut frame = Frame::default();
            frame.clear(cell, CaretStyle::default());
            frame.set_dock_rows(4);
            frame.set_fill_rows(FILL);
            frame.open_dock(
                Theme::BATERI.background_linear(),
                Theme::BATERI.accent_linear(),
                Theme::BATERI.accent_linear(),
            );
            compose(&mut frame, motion, BOTTOM, DOCK_ROWS);

            // The content's bottom edge: origin + filled rows (`29 − 5`).
            let grid_bottom = frame.origin_px() + (ROWS - 5.0) * 18.0;
            let band_top = BOTTOM - frame.dock_band_px();
            assert_eq!(
                band_top - grid_bottom,
                strip,
                "frame {frames}: grid and band diverged (band {})",
                motion.band()
            );
            assert_eq!(
                frame.fill_origin_px() + f32::from(FILL) * 18.0,
                frame.origin_px(),
                "frame {frames}: the fill band came off the grid"
            );
            mid |= motion.band() > 0.0 && motion.band() < 2.0;
            if motion.settled() {
                break;
            }
            motion.advance(1.0 / 120.0);
            frames += 1;
            assert!(frames < 1000, "the band did not settle");
        }
        assert!(mid, "the middle of the animation was never tested");
        // Once settled, the band is the layout's height and the grid is two rows higher.
        let mut frame = Frame::default();
        frame.clear(cell, CaretStyle::default());
        frame.set_dock_rows(4);
        frame.open_dock(
            Theme::BATERI.background_linear(),
            Theme::BATERI.accent_linear(),
            Theme::BATERI.accent_linear(),
        );
        compose(&mut frame, motion, BOTTOM, DOCK_ROWS);
        assert_eq!(frame.dock_band_px(), frame.dock_layout_px());
        assert_eq!(frame.origin_px(), (5.0 - 2.0) * 18.0);
    }

    #[test]
    fn the_band_target_is_one_fractional_signed_formula() {
        // @1x, 9×18 cell, gutter 8: the PTY gutter is 68 px, the row gap 16.
        let cell = CellMetrics::new(9, 18, 9, 8, 1, 1.0).expect("metrics");
        // With one or more input rows, whole rows — integer pixels, so
        // bit-for-bit in `f32` too: today's frame does not change.
        assert_eq!(band_target(Some(1), DOCK_ROWS, cell), 0.0);
        assert_eq!(band_target(Some(3), DOCK_ROWS, cell), 2.0);
        // With zero input rows (remote session) it is negative: one cell
        // plus the row gap, `(34 − 68) / 18`.
        let remote = band_target(Some(0), DOCK_ROWS, cell);
        assert!(remote < -1.0, "{remote}");
        assert!((remote * 18.0 + (18.0 + 16.0)).abs() < 1e-4, "{remote}");
        // There is no band in a dock-less frame.
        assert_eq!(band_target(Some(0), 0, cell), 0.0);
        // No band (a program reading the keyboard itself): the whole PTY
        // share, negative — `−68 / 18`.
        let none = band_target(None, DOCK_ROWS, cell);
        assert!((none * 18.0 + crate::frame::dock_px(DOCK_ROWS, cell)).abs() < 1e-4);
        assert!(none < remote, "lower than the remote shape: {none}");
        assert_eq!(band_target(None, 0, cell), 0.0);
    }

    #[test]
    fn no_band_drops_the_grid_to_the_window_bottom_in_every_frame() {
        // **Composition guard** for a program reading the keyboard itself: the
        // band slides to zero height, the grid's origin goes the whole PTY
        // share lower, the fill band stays glued to the grid and the grid's
        // bottom edge meets the band's top in every frame — both ways. The
        // setup is the remote twin's.
        let cell = CellMetrics::new(9, 18, 9, 8, 1, 1.0).expect("metrics");
        const BOTTOM: f32 = 600.0;
        const ROWS: f32 = 29.0;
        const FILL: u16 = 7;
        let share = crate::frame::dock_px(DOCK_ROWS, cell);
        let strip = BOTTOM - share - ROWS * 18.0;
        let frame_at = |motion: Motion, band: Option<u16>| {
            let mut frame = Frame::default();
            frame.clear(cell, CaretStyle::default());
            frame.set_dock_input_rows(band);
            frame.set_fill_rows(FILL);
            frame.open_dock(
                Theme::BATERI.background_linear(),
                Theme::BATERI.separator_linear(),
                Theme::BATERI.separator_linear(),
            );
            compose(&mut frame, motion, BOTTOM, DOCK_ROWS);
            frame
        };
        let mut motion = Motion::default();
        motion.sync(Some([0.0, 30.0]), 5, 0.0, 0, false, false);
        let local_origin = frame_at(motion, Some(1)).origin_px();

        // Both curves, the spring's included: whatever the path to no band,
        // the grid stays glued to the band and the band never goes negative
        // (the clamp itself is `frame::tests::no_band_opens_a_surface_of_zero_rows`).
        let none = band_target(None, DOCK_ROWS, cell);
        for style in [CursorMotion::Ease, CursorMotion::Spring] {
            motion.set_style(style);
            for (band, target) in [(None, none), (Some(1), 0.0)] {
                motion.sync(Some([0.0, 30.0]), 5, target, 0, false, false);
                let mut frames = 0;
                let mut mid = false;
                loop {
                    let frame = frame_at(motion, band);
                    let grid_bottom = frame.origin_px() + (ROWS - 5.0) * 18.0;
                    let band_top = BOTTOM - frame.dock_band_px();
                    assert_eq!(
                        band_top - grid_bottom,
                        strip,
                        "{style:?} {band:?}/{frames}: grid and band diverged ({})",
                        motion.band()
                    );
                    assert!(frame.dock_band_px() >= 0.0, "{style:?}: a negative band");
                    assert_eq!(
                        frame.fill_origin_px() + f32::from(FILL) * 18.0,
                        frame.origin_px(),
                        "{style:?} {band:?}/{frames}: the fill band came off the grid"
                    );
                    mid |= motion.band() < 0.0 && motion.band() > none;
                    if motion.settled() {
                        break;
                    }
                    motion.advance(1.0 / 120.0);
                    frames += 1;
                    assert!(frames < 1000, "the band did not settle");
                }
                assert!(
                    mid,
                    "{style:?} {band:?}: the middle of the animation was never tested"
                );
            }
        }
        motion.set_style(CursorMotion::Ease);

        motion.sync(Some([0.0, 30.0]), 5, none, 0, true, false);
        let frame = frame_at(motion, None);
        assert_eq!(frame.dock_band_px(), 0.0, "no band");
        assert_eq!(frame.dock_layout_px(), 0.0);
        assert!(frame.dock().is_some(), "the surface stays open");
        // The grid is lower by the whole share: its bottom row sits on the
        // window's bottom strip.
        assert_eq!(frame.origin_px(), local_origin + share);
        assert_eq!(frame.origin_px() + (ROWS - 5.0) * 18.0, BOTTOM - strip);
        // Mouse: a zero-row input block, not "no frame yet".
        assert_eq!(frame.dock_hit().map(|(_, rows)| rows), Some(0));
    }

    #[test]
    fn a_remote_band_drops_the_input_row_and_the_grid_moves_down() {
        // **Composition guard**: once the input row goes away, the
        // band's drawn height is `band_px(0)` (the context row only), the grid's
        // origin is lower by that difference, the fill band is glued to the grid
        // and the grid and band coincide in every frame — in both directions.
        // The setup is that of
        // `the_grid_the_fill_band_and_the_dock_band_meet_in_every_frame`.
        let cell = CellMetrics::new(9, 18, 9, 8, 1, 1.0).expect("metrics");
        const BOTTOM: f32 = 600.0;
        const ROWS: f32 = 29.0;
        const FILL: u16 = 7;
        let strip = BOTTOM - crate::frame::dock_px(DOCK_ROWS, cell) - ROWS * 18.0;
        let frame_at = |motion: Motion, input_rows: u16| {
            let mut frame = Frame::default();
            frame.clear(cell, CaretStyle::default());
            frame.set_dock_input_rows(Some(input_rows));
            frame.set_fill_rows(FILL);
            frame.open_dock(
                Theme::BATERI.background_linear(),
                Theme::BATERI.info_linear(),
                Theme::BATERI.accent_linear(),
            );
            compose(&mut frame, motion, BOTTOM, DOCK_ROWS);
            frame
        };
        let mut motion = Motion::default();
        motion.sync(Some([0.0, 30.0]), 5, 0.0, 0, false, false);
        let local = frame_at(motion, 1);
        let local_origin = local.origin_px();
        assert_eq!(
            local_origin,
            5.0 * 18.0,
            "today's frame with a single input row"
        );

        let remote = band_target(Some(0), DOCK_ROWS, cell);
        for (input_rows, target) in [(0, remote), (1, 0.0)] {
            motion.sync(Some([0.0, 30.0]), 5, target, 0, false, false);
            let mut frames = 0;
            let mut mid = false;
            loop {
                let frame = frame_at(motion, input_rows);
                let grid_bottom = frame.origin_px() + (ROWS - 5.0) * 18.0;
                let band_top = BOTTOM - frame.dock_band_px();
                assert_eq!(
                    band_top - grid_bottom,
                    strip,
                    "{input_rows}/{frames}: grid and band diverged ({})",
                    motion.band()
                );
                assert_eq!(
                    frame.fill_origin_px() + f32::from(FILL) * 18.0,
                    frame.origin_px(),
                    "{input_rows}/{frames}: the fill band came off the grid"
                );
                mid |= motion.band() < 0.0 && motion.band() > remote;
                if motion.settled() {
                    break;
                }
                motion.advance(1.0 / 120.0);
                frames += 1;
                assert!(frames < 1000, "the band did not settle");
            }
            assert!(
                mid,
                "{input_rows}: the middle of the animation was never tested"
            );
        }

        motion.sync(Some([0.0, 30.0]), 5, remote, 0, true, false);
        let frame = frame_at(motion, 0);
        let band = crate::frame::band_px(Some(0), cell);
        assert_eq!(band, 18.0 + 2.0 * 8.0, "context row only");
        assert_eq!(frame.dock_band_px(), band);
        assert_eq!(frame.dock_band_px(), frame.dock_layout_px());
        // The grid is lower by one cell plus the row gap.
        assert_eq!(frame.origin_px(), local_origin + 18.0 + 16.0);
        // The separators are in `frame::tests` (`Instance`'s fields belong to that module).
        // Mouse: there is no input block, the row count is zero (not `None`).
        assert_eq!(frame.dock_hit().map(|(_, rows)| rows), Some(0));
        // With a single input row the settled frame is the same as today's.
        motion.sync(Some([0.0, 30.0]), 5, 0.0, 0, true, false);
        let frame = frame_at(motion, 1);
        assert_eq!(frame.origin_px(), local_origin);
        assert_eq!(frame.dock_band_px(), crate::frame::dock_px(DOCK_ROWS, cell));
        assert_eq!(frame.dock_hit().map(|(_, rows)| rows), Some(1));
    }

    #[test]
    fn a_remote_alternate_screen_keeps_a_one_row_band_without_offsetting_the_grid() {
        // vim over ssh: the share is one row and the band is the context row
        // alone — the band is exactly the share and the grid stays put.
        let cell = CellMetrics::new(9, 18, 9, 8, 1, 1.0).expect("metrics");
        let mut motion = Motion::default();
        motion.sync(None, 0, band_target(Some(0), 1, cell), 0, true, false);
        let mut frame = Frame::default();
        frame.clear(cell, CaretStyle::default());
        frame.set_dock_input_rows(Some(0));
        frame.open_dock(
            Theme::BATERI.background_linear(),
            Theme::BATERI.accent_linear(),
            Theme::BATERI.accent_linear(),
        );
        compose(&mut frame, motion, 600.0, 1);
        assert_eq!(frame.dock_band_px(), crate::frame::dock_px(1, cell));
        assert_eq!(frame.dock_band_px(), crate::frame::band_px(Some(0), cell));
        assert_eq!(frame.origin_px(), 0.0, "the remote app's grid moved");
    }

    #[test]
    fn a_full_grid_is_clipped_from_the_top_while_the_band_is_tall() {
        // In a full grid (offset 0) when the band grows the origin goes
        // **negative** and the grid's top ends up outside the window —
        // temporary, it returns when the input ends. The mouse reads the same
        // origin (`Origin::px`), so the point clicked on a visible row is the
        // right row (`point_to_cell`'s negative-origin arm).
        let cell = CellMetrics::new(9, 18, 9, 8, 1, 1.0).expect("metrics");
        let mut motion = Motion::default();
        motion.sync(None, 0, 2.0, 0, false, false);
        let mut frame = Frame::default();
        frame.clear(cell, CaretStyle::default());
        frame.set_dock_rows(4);
        frame.open_dock(
            Theme::BATERI.background_linear(),
            Theme::BATERI.accent_linear(),
            Theme::BATERI.accent_linear(),
        );
        compose(&mut frame, motion, 600.0, DOCK_ROWS);
        assert_eq!(frame.origin_px(), -36.0);
        // In a dock-less window the band is never written: the origin is the offset alone.
        let mut frame = Frame::default();
        frame.clear(cell, CaretStyle::default());
        compose(&mut frame, motion, 600.0, 0);
        assert_eq!(frame.origin_px(), 0.0);
        // The window has a gutter but this frame's surface is off (the gap when
        // leaving vim): the band is not written, the caret's slot boundary stays
        // at infinity.
        let mut frame = Frame::default();
        frame.clear(cell, CaretStyle::default());
        compose(&mut frame, motion, 600.0, DOCK_ROWS);
        assert_eq!(
            frame.origin_px(),
            0.0,
            "a band was written in a surface-less frame"
        );
    }

    #[test]
    fn glyph_effects_keep_the_link_awake_and_draw_their_last_frame() {
        // The typing effect's sleep term: the link does not sleep while
        // something is in flight, the step where the effect ends is still drawn,
        // and only the callback after that sleeps. It plants no damage — `GlyphFx`
        // never sees the `Waker`.
        use crate::glyph_fx::{GlyphFx, KEYPRESS_DURATION};
        let mut fx = GlyphFx::default();
        let arrival = bt_core::DockEdit::Arrive {
            row: 0,
            col: bt_core::DOCK_TEXT_COL,
            cells: [bt_core::Cell {
                col: bt_core::DOCK_TEXT_COL,
                ch: Some('a'),
                ..bt_core::Cell::default()
            }]
            .into_iter()
            .collect(),
            shift: 0,
        };
        fx.apply(arrival, Motion::default(), 1, &Clusters::default());
        let dt = 1.0 / 120.0;
        let mut drawn = 0usize;
        let mut emptied_on_a_drawn_frame = false;
        loop {
            let fx_idle = fx.is_empty();
            fx.advance(dt);
            if at_rest(Motion::default(), false, fx_idle, true) {
                break;
            }
            drawn += 1;
            emptied_on_a_drawn_frame |= fx.is_empty();
            assert!(drawn < 1000, "the effect never settled");
        }
        assert!(
            emptied_on_a_drawn_frame,
            "went to sleep before the effect's last state was drawn"
        );
        let expected = (KEYPRESS_DURATION / dt).ceil() as usize;
        assert!(
            drawn.abs_diff(expected) <= 1,
            "the effect lasted {drawn} frames, its duration is {expected} frames"
        );
        // With an empty list it sleeps on the first question: a window without effects is idle.
        assert!(at_rest(
            Motion::default(),
            false,
            GlyphFx::default().is_empty(),
            true
        ));
    }

    #[test]
    fn stopped_gate_does_not_reopen_on_visibility() {
        // The window delegate is not torn down at shutdown: had a
        // `windowDidChangeOcclusionState:` falling after `stop()` reopened the
        // gate, work would keep being thrown at the main thread waiting in
        // `shutdown()`'s `join` at shutdown. The latch ties this to code, not to
        // a comment sentence.
        let gate = Gate::new();
        assert!(gate.is_open(), "the link is born with a visible window");

        gate.set_open(false);
        assert!(!gate.is_open(), "an occluded window closes the gate");
        gate.set_open(true);
        assert!(gate.is_open(), "the gate reopens when the occlusion lifts");

        gate.stop();
        assert!(!gate.is_open());
        gate.set_open(true);
        assert!(
            !gate.is_open(),
            "a stopped gate does not reopen on a notification"
        );
    }

    #[test]
    fn stop_condition_kicks_in_on_second_failure() {
        // The checklist's "a stop condition is mandatory" item is tied to this
        // test: if a persistent draw error repeats the frame request at the
        // refresh rate, it has no symptom, its bill is the battery. The policy
        // is here, without ObjC.
        let streak = FailureStreak::default();
        assert!(streak.failed(), "the first failure is tried once more");
        assert!(
            !streak.failed(),
            "a second consecutive failure cuts the frame request"
        );
        assert!(!streak.failed(), "and it stays cut afterwards");

        streak.succeeded();
        assert!(streak.failed(), "a completed frame gives the budget back");
    }

    #[test]
    fn the_clock_picks_the_nearer_deadline_and_its_flavour() {
        // A content tick plants damage (the grid really changes), a blink does
        // not (only the caret's alpha).
        assert_eq!(
            due_clock(Some(1.0), [Some(0.5), None, None]),
            Some((0.5, false))
        );
        assert_eq!(due_clock(Some(1.0), [None, None, None]), Some((1.0, true)));
        assert_eq!(due_clock(None, [Some(0.5), None, None]), Some((0.5, false)));
        assert_eq!(
            due_clock(None, [None, None, None]),
            None,
            "a clock was set while idle"
        );
        // On a tie the content wins: the frame will be drawn anyway, the
        // motion flavour needs no second wakeup.
        assert_eq!(
            due_clock(Some(1.0), [Some(1.0), None, None]),
            Some((1.0, true))
        );
        // The completion poll of a frame in flight rides the motion
        // flavour: it draws nothing, so it plants no damage, and the nearest
        // of blink and the poll competes with the content tick.
        assert_eq!(due_clock(None, [None, Some(0.2), None]), Some((0.2, false)));
        assert_eq!(
            due_clock(Some(1.0), [Some(0.5), Some(0.2), None]),
            Some((0.2, false))
        );
        assert_eq!(
            due_clock(Some(0.1), [None, Some(0.2), None]),
            Some((0.1, true))
        );
        // The scroll bar's hold rides the motion flavour too: its fade
        // changes only the bar.
        assert_eq!(
            due_clock(Some(2.0), [Some(0.5), None, Some(0.3)]),
            Some((0.3, false))
        );
        assert_eq!(
            due_clock(Some(0.2), [None, None, Some(0.3)]),
            Some((0.2, true))
        );
    }

    /// A scroll bar with something to travel: a 400×300 window at @1x,
    /// 100 rows above an 18-row window at the bottom.
    fn bar_scene() -> (Frame, ScrollbarLayout) {
        let cell = CellMetrics::new(8, 16, 8, 8, 1, 1.0).expect("non-zero cell");
        let mut frame = Frame::default();
        frame.clear(cell, CaretStyle::default());
        let position = bt_core::ScrollPosition {
            room: 100,
            top: 100.0,
            visible: 18,
        };
        let layout = ScrollbarLayout::new(Some(position), 400.0, 300.0, cell);
        assert!(layout.drawable());
        (frame, layout)
    }

    #[test]
    fn the_scroll_bar_rises_on_content_frames_and_fades_from_the_motion_clock() {
        // **The tick sequence, through the one step both arms call.**
        let foreground = LinearRgba::from_srgb(0xff, 0xff, 0xff);
        let (mut frame, layout) = bar_scene();
        let mut bar = Scrollbar::default();
        let mut kept = ScrollbarLayout::default();
        let tick = 1.0 / 120.0;
        // No content frame yet: nothing laid out, the poke is ignored.
        assert!(!bar.poke(kept.drawable()), "a poke before any layout");
        // The first content frame lays the bar out, hidden: no op.
        let step = scrollbar_step(
            &mut bar,
            &mut kept,
            Some(layout),
            -tick,
            false,
            &mut frame,
            foreground,
            &TrackMarks::default(),
        );
        assert!(step.idle() && frame.scrollbar().is_none(), "{step:?}");
        // Scrolling: the wheel's poke, then **content** frames back to back
        // (the scroll damages every tick and the motion arm never runs). The
        // bar must rise across them, not freeze waiting for a motion frame.
        assert!(bar.poke(kept.drawable()));
        let mut now = 0.0;
        let mut alpha = 0.0f32;
        while alpha < 1.0 {
            scrollbar_step(
                &mut bar,
                &mut kept,
                Some(layout),
                now,
                false,
                &mut frame,
                foreground,
                &TrackMarks::default(),
            );
            let next = bar.alpha(now);
            assert!(
                next > alpha || (now == 0.0 && next == 0.0),
                "the bar did not rise at {now}: {alpha} → {next}"
            );
            assert_eq!(frame.scrollbar().is_some(), next > 0.0, "at {now}");
            alpha = next;
            now += tick;
            assert!(now < 0.2, "the bar never finished appearing");
        }
        // The scrolling stopped: a motion frame finds the bar holding — idle,
        // so the link sleeps, and the clock is armed for the hold's end in
        // the **motion** flavour: no damage, so not a content frame and not a
        // request.
        let step = scrollbar_step(
            &mut bar,
            &mut kept,
            None,
            now,
            false,
            &mut frame,
            foreground,
            &TrackMarks::default(),
        );
        assert!(
            at_rest(Motion::default(), false, true, step.idle()),
            "{step:?}"
        );
        assert_eq!(
            due_clock(None, [None, None, bar.next_deadline()]),
            Some((1.0, false)),
            "the hold's end is not a motion-flavoured wakeup"
        );
        assert!(frame.scrollbar().is_some(), "the held bar left the frame");
        // The clock fires: the fade is drawn in motion frames, and while it
        // runs the link is not settled (`motion_settled`'s term).
        let mut now = 1.0;
        let mut alpha = 1.0f32;
        loop {
            let step = scrollbar_step(
                &mut bar,
                &mut kept,
                None,
                now,
                false,
                &mut frame,
                foreground,
                &TrackMarks::default(),
            );
            assert!(
                !at_rest(Motion::default(), false, true, step.idle()),
                "the link slept mid-fade at {now}"
            );
            if step.settled {
                // The last fading step is drawn without a bar, then nothing.
                assert_eq!(frame.scrollbar(), None, "a gone bar is still drawn");
                break;
            }
            assert!(!bar.settled(now), "a fading bar counts as settled at {now}");
            let next = bar.alpha(now);
            assert!(next < alpha || now == 1.0, "the bar did not fade at {now}");
            alpha = next;
            now += tick;
            assert!(now < 1.5, "the fade never ended");
        }
        assert_eq!(
            due_clock(None, [None, None, bar.next_deadline()]),
            None,
            "a hidden bar armed a clock"
        );
        let step = scrollbar_step(
            &mut bar,
            &mut kept,
            None,
            now + tick,
            false,
            &mut frame,
            foreground,
            &TrackMarks::default(),
        );
        assert!(step.idle(), "the hidden bar keeps the link awake: {step:?}");
    }

    #[test]
    fn a_bar_under_the_pointer_widens_awake_then_sleeps_without_a_clock() {
        // A scroll showed the thin bar and the link sleeps in the hold; the
        // pointer comes over the strip: motion frames widen the bar, then the
        // link sleeps with **no** clock — a deadline left armed under the
        // pointer would fire in the past and spin. The pointer leaves: awake
        // for the narrowing, then the hold's clock in the motion flavour.
        let foreground = LinearRgba::from_srgb(0xff, 0xff, 0xff);
        let (mut frame, layout) = bar_scene();
        let mut bar = Scrollbar::default();
        let mut kept = layout;
        let rest = Motion::default();
        let tick = 1.0 / 120.0;
        let step = |bar: &mut Scrollbar, kept: &mut ScrollbarLayout, frame: &mut Frame, now| {
            scrollbar_step(
                bar,
                kept,
                None,
                now,
                false,
                frame,
                foreground,
                &TrackMarks::default(),
            )
        };
        assert!(bar.poke(true));
        step(&mut bar, &mut kept, &mut frame, 0.0);
        step(&mut bar, &mut kept, &mut frame, 0.2);
        assert!(
            step(&mut bar, &mut kept, &mut frame, 0.3).idle(),
            "the thin bar is not holding"
        );
        // Over the strip at 0.5: awake until the bar is wide.
        assert!(bar.set_hover(true, true), "the hover asked for no frame");
        let mut now = 0.5;
        loop {
            if at_rest(
                rest,
                false,
                true,
                step(&mut bar, &mut kept, &mut frame, now).idle(),
            ) {
                break;
            }
            now += tick;
            assert!(now < 0.8, "the widening never settled");
        }
        assert!(now >= 0.65, "slept before the bar widened: {now}");
        let [x0, _, x1, _] = layout.thumb(1.0);
        let drawn = frame.scrollbar().expect("the bar is up").core;
        assert_eq!([drawn[0], drawn[2]], [x0, x1], "the bar is not wide");
        assert_eq!(
            due_clock(None, [None, None, bar.next_deadline()]),
            None,
            "the engaged bar armed a clock"
        );
        for later in [now + 1.0, now + 30.0] {
            assert!(
                step(&mut bar, &mut kept, &mut frame, later).idle(),
                "the engaged bar woke at {later}"
            );
        }
        // Leaving: narrowing frames, then asleep until the hold's end.
        let left = now + 31.0;
        assert!(bar.set_hover(false, true));
        let narrowing = step(&mut bar, &mut kept, &mut frame, left);
        assert!(
            !at_rest(rest, false, true, narrowing.idle()),
            "slept mid-narrowing"
        );
        assert!(
            step(&mut bar, &mut kept, &mut frame, left + 0.15).changed,
            "the last narrowing step was not drawn"
        );
        assert!(
            step(&mut bar, &mut kept, &mut frame, left + 0.2).idle(),
            "the narrowed bar is not holding"
        );
        assert_eq!(
            due_clock(None, [None, None, bar.next_deadline()]),
            Some((left + 0.15 + 1.0, false)),
            "the hold after leaving is not a motion-flavoured wakeup"
        );
    }

    #[test]
    fn the_always_up_form_is_drawn_with_the_content_and_sleeps() {
        // `Always`: drawn wide over its track by the content frame, then idle
        // — no clock, no motion frame, a poke wants nothing. `Never`: no op
        // in any frame and the poke is ignored.
        let foreground = LinearRgba::from_srgb(0xff, 0xff, 0xff);
        let (mut frame, layout) = bar_scene();
        let mut bar = Scrollbar::default();
        let mut kept = ScrollbarLayout::default();
        assert!(bar.set_mode(Mode::Always));
        let step = scrollbar_step(
            &mut bar,
            &mut kept,
            Some(layout),
            0.0,
            false,
            &mut frame,
            foreground,
            &TrackMarks::default(),
        );
        assert!(step.changed && step.settled, "{step:?}");
        assert!(
            frame.scrollbar().is_some(),
            "the always-up bar is not drawn"
        );
        assert_eq!(frame.scrollbar_track().len(), 2, "no track under it");
        assert!(!bar.poke(kept.drawable()), "a poke asked for a frame");
        for now in [0.5, 1.0, 1.4, 30.0] {
            let step = scrollbar_step(
                &mut bar,
                &mut kept,
                None,
                now,
                false,
                &mut frame,
                foreground,
                &TrackMarks::default(),
            );
            assert!(
                at_rest(Motion::default(), false, true, step.idle()),
                "the link stayed awake for an always-up bar at {now}"
            );
            assert!(frame.scrollbar().is_some(), "the bar left at {now}");
        }
        assert_eq!(due_clock(None, [None, None, bar.next_deadline()]), None);

        // The form changes to `Never` between content frames: one motion
        // frame draws the bar away from the kept layout, then nothing.
        assert!(bar.set_mode(Mode::Never));
        let step = scrollbar_step(
            &mut bar,
            &mut kept,
            None,
            31.0,
            false,
            &mut frame,
            foreground,
            &TrackMarks::default(),
        );
        assert!(step.changed, "the form change was not drawn");
        assert!(frame.scrollbar().is_none() && frame.scrollbar_track().is_empty());
        assert!(
            !bar.poke(kept.drawable()),
            "a poke on `Never` asked for a frame"
        );
        let step = scrollbar_step(
            &mut bar,
            &mut kept,
            Some(layout),
            32.0,
            false,
            &mut frame,
            foreground,
            &TrackMarks::default(),
        );
        assert!(step.idle() && frame.scrollbar().is_none(), "{step:?}");
    }

    #[test]
    fn a_tick_that_changes_nothing_on_screen_waits_instead_of_drawing() {
        // The wheel at the bottom pokes without damage, so a **motion** tick
        // stamps the poke — at an opacity still zero. The link must stay
        // awake (the bar is rising) but draw nothing: the frame would be the
        // one on screen. The hold's last tick is the same case at full
        // opacity.
        let foreground = LinearRgba::from_srgb(0xff, 0xff, 0xff);
        let (mut frame, layout) = bar_scene();
        let mut bar = Scrollbar::default();
        let mut kept = ScrollbarLayout::default();
        let tick = 1.0 / 120.0;
        scrollbar_step(
            &mut bar,
            &mut kept,
            Some(layout),
            -tick,
            false,
            &mut frame,
            foreground,
            &TrackMarks::default(),
        );
        assert!(bar.poke(kept.drawable()));
        let rest = Motion::default();
        let stamped = scrollbar_step(
            &mut bar,
            &mut kept,
            None,
            0.0,
            false,
            &mut frame,
            foreground,
            &TrackMarks::default(),
        );
        assert!(
            !at_rest(rest, false, true, stamped.idle()),
            "slept on a rising bar"
        );
        assert!(
            nothing_to_draw(rest, false, true, stamped),
            "an identical frame was drawn"
        );
        let rising = scrollbar_step(
            &mut bar,
            &mut kept,
            None,
            tick,
            false,
            &mut frame,
            foreground,
            &TrackMarks::default(),
        );
        assert!(
            !nothing_to_draw(rest, false, true, rising),
            "the rise was not drawn"
        );
        // Up and holding: asleep until the clock; it fires at the hold's end.
        scrollbar_step(
            &mut bar,
            &mut kept,
            None,
            0.5,
            false,
            &mut frame,
            foreground,
            &TrackMarks::default(),
        );
        let ending = scrollbar_step(
            &mut bar,
            &mut kept,
            None,
            1.0,
            false,
            &mut frame,
            foreground,
            &TrackMarks::default(),
        );
        assert!(
            !at_rest(rest, false, true, ending.idle()),
            "slept at the hold's end"
        );
        assert!(nothing_to_draw(rest, false, true, ending));
        // Anything else in flight still draws.
        assert!(
            !nothing_to_draw(rest, true, true, ending),
            "a blink flip was not drawn"
        );
        assert!(
            !nothing_to_draw(rest, false, false, ending),
            "an effect was not drawn"
        );
    }

    #[test]
    fn a_long_scroll_keeps_the_bar_up_until_a_second_after_it_stops() {
        // A trackpad scroll of a second and a half: pokes every 16 ms ride
        // the content frames and push the hold forward each time.
        let foreground = LinearRgba::from_srgb(0xff, 0xff, 0xff);
        let (mut frame, layout) = bar_scene();
        let mut bar = Scrollbar::default();
        let mut kept = layout;
        let mut now = 0.0;
        while now < 1.5 {
            bar.poke(kept.drawable());
            scrollbar_step(
                &mut bar,
                &mut kept,
                Some(layout),
                now,
                false,
                &mut frame,
                foreground,
                &TrackMarks::default(),
            );
            if now > 0.2 {
                assert_eq!(bar.alpha(now), 1.0, "the bar faded mid-scroll at {now}");
            }
            now += 0.016;
        }
        let last = now - 0.016;
        assert_eq!(bar.next_deadline(), Some(last + 1.0));
        assert!(
            bar.settled(last + 0.9),
            "the bar is not holding after the scroll"
        );
        assert!(
            !bar.settled(last + 1.0),
            "the bar did not start fading on time"
        );
    }

    #[test]
    fn a_blinking_cursor_does_not_starve_the_duration_counter() {
        // **Regression guard.** A running command's counter must tick at
        // t=1.0; the blink wakes every 0.5. The clock is re-set on every wakeup
        // and in the old (duration-based) state the counter's tick would be
        // pushed a second ahead each time, i.e. would **never** fire.
        //
        // Because the deadline is absolute, the blink's ticks do not move it.
        let content = Some(1.0);
        let mut blink = Blink::default();
        blink.content_frame(0.0, true);

        // t=0: the blink is nearer, the motion flavour is set.
        assert_eq!(
            due_clock(content, [blink.next_flip(), None, None]),
            Some((0.5, false))
        );

        // t=0.5: the blink flipped and pushed its own tick ahead; the counter's is **in place**.
        assert!(blink.advance(0.5), "the blink did not flip");
        assert_eq!(blink.next_flip(), Some(1.0));
        assert_eq!(
            due_clock(content, [blink.next_flip(), None, None]),
            Some((1.0, true)),
            "the counter's tick was pushed by the blink"
        );
    }

    #[test]
    fn a_finished_command_clears_the_clock() {
        // The running command's tick is converted to an absolute
        // stamp; when the command ends (`next_tick` is `None`) the stored
        // deadline is **cleared**. Had the mapping kept the `Some` or ignored the
        // `None`, the stale-deadline defect would come back.
        assert_eq!(
            content_deadline(5.0, Some(Duration::from_millis(400))),
            Some(5.4)
        );
        assert_eq!(
            content_deadline(5.0, None),
            None,
            "a finished command left the clock behind"
        );
        // A cleared deadline and a non-blinking cursor: no clock is set at all.
        assert_eq!(
            due_clock(content_deadline(5.0, None), [None, None, None]),
            None
        );
    }
}
