//! The application delegate: everything **app-wide** — reads and watches the
//! settings, resolves the theme and appearance, owns the subtitle slots, opens
//! and lists windows, distributes the on-save paths to every window and runs
//! the shutdown sequence. Everything per-window (surface, renderer, session,
//! display link, dock share, temporary point size) lives in `window`. There is
//! **no** drawing call here; this file's job is wiring.

use std::cell::{Cell, OnceCell, Ref, RefCell};
use std::ffi::{OsString, c_void};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use block2::RcBlock;
use bt_core::{
    AdoptMode, CursorMotion, HostMark, InitialInput, KeepRunning, MarkSubject, ReduceMotion,
    RestoreWindows, SHUTDOWN_GRACE, SYSTEM_THEME, Scrollbar, Settings, SettingsEdit,
    ShellIntegration, SmoothScroll, TabId, Teardown, Theme,
};
use bt_gpu::{CellMetrics, DOCK_ROWS, DisplayLink, MIN_SAMPLES, Renderer, ScrollbarMode, Stats};
use dispatch2::{DispatchQueue, DispatchTime};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAlertFirstButtonReturn, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication,
    NSApplicationDelegate, NSApplicationTerminateReply, NSControlStateValueOff,
    NSControlStateValueOn, NSDragOperation, NSEvent, NSEventModifierFlags, NSMenu, NSMenuDelegate,
    NSMenuItem, NSPreferredScrollerStyleDidChangeNotification, NSScreen, NSScroller,
    NSScrollerStyle, NSWindow, NSWindowNumberListOptions, NSWindowStyleMask,
    NSWindowUserTabbingPreference, NSWorkspace,
    NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification,
    NSWorkspaceWillPowerOffNotification,
};
use objc2_foundation::{
    NSArray, NSBundle, NSDictionary, NSKeyValueObservingOptions, NSNotification,
    NSNotificationCenter, NSNumber, NSObject, NSObjectNSDelayedPerforming,
    NSObjectNSKeyValueObserverRegistration, NSObjectProtocol, NSPoint, NSRect,
    NSRunLoopCommonModes, NSSize, NSString, NSURL, NSUserDefaults, ns_string,
};

use crate::handover::{self, Arrival, HeldPane, PaneState};
use crate::keeper::{self, Keeper, QuitKind, QuitPath};
use crate::menu::ShellMenuDelegate;
use crate::notices::{Notices, Source};
use crate::pane::{PaneLaunch, TerminalPane};
use crate::preview_cache;
use crate::remote_files::Sweep;
use crate::restore::{self, Frame, Saved, SavedPane, SavedWindow};
use crate::settings_window::SettingsWindow;
use crate::split::Axis;
use crate::ssh_route::{self, Masters};
use crate::tab::{Histories, TabHost, TerminalTab};
use crate::tab_drag::TabDragSource;
use crate::tabs::Landing;
use crate::watch::{Notify, Watch};
use crate::window::{
    self, Adopted, CloseScope, Launch, Note, Placement, TerminalWindow, fallen_back,
};
use crate::zoom::Zoom;
use crate::{Options, Run, Workload};
use crate::{child, focus, jobs, settings};

/// Guard for zero idle frames: in the [`Workload::Smoke`] workload the window
/// sits idle for ~`run_seconds` seconds after the first draw.
///
/// **The operand of the limit is not `frames` but [`Counters::content`]**
/// (the `content=` token): not the frame the GPU finished but the content
/// frame **decided to be drawn**. The reason is an aged debt in this limit's
/// own doc: "an animation with a forgotten stop condition passes today's gate
/// green". The remedy was not to move the limit but to keep motion frames out
/// of the gate: a cursor slide legitimately raises `frames` to ~24 and never
/// raises `content`.
///
/// **The relation between the two counters broke with motion** (a code
/// review finding): when the operand changed it was written that
/// "every frame that ends without error was a content frame", and that
/// sentence was true before motion landed, **not** after — a motion frame also
/// commits a command buffer, so it raises `frames` without raising `content`.
/// The direction has even reversed today: the measured healthy smoke run has
/// `frames` 27–30 while `content` is 2–3 (the 2026-09-16 row below). So the limit
/// sits on a **looser** counter, not a tighter one — and that is why a
/// number that was **re-measured**, not carried over, was needed.
///
/// **The number was measured twice; the second time in a visible window, and
/// it did not change it.** Here only the poles that gave rise to the limit
/// and the derivation are kept.
///
/// - **2026-09-12 (debug, unbundled process):** `2` → `8`. The
///   basis of the old `2` ("the system suspends the display link, ceiling ~3
///   frames") was refuted by measurement: [`Workload::Load`] produced
///   `frames=594` in five seconds in the same window state, so what was
///   measured was not a ceiling but a run corrupted by shutdown locking.
///   Moreover `2` **fell red on a correct build** (a healthy five-second run
///   was `frames=4`). Poles: healthy at most `4`, broken at least `49`.
/// - **2026-09-15 (debug + release bundle; in the two probed
///   runs the window was on screen and in front):** the highest of fifty
///   healthy runs is `2`, the lowest of six broken runs is `353`. The visible
///   window did **not** raise the legitimate frame count; it did take the broken run to the full refresh rate.
///
/// `8` lies between the poles of the two measurements: twice the highest
/// healthy observation (`4`), a sixth of the lowest broken observation
/// (`49`). The second measurement only widened the gap; there is no observation that would move
/// the limit — lowering it would mean declaring the first healthy `4` invalid without re-measuring it.
///
/// - **2026-09-16 (debug + release bundle):** the **first**
///   measurement after the operand moved from `frames` to `content`, so the
///   numbers in the two rows above now belong to another counter. In thirty
///   healthy runs `content` is at most `3`, in the broken arm (an
///   unconditional `wake()`) at least `357`. The rule holds at both ends and
///   there is **no observation that moves the limit**: `8` is larger than
///   twice `3` (`6`) and far below `357`.
///
/// **The profile tied to the gate is debug**, because the only unattended
/// context is `make smoke` and it builds debug. The release bundle is subject
/// to the same limit (the gate is profile-independent) and its distribution
/// was measured separately; the same number carries both.
///
/// The limit is safe in both regimes of this machine, but the margin depends
/// on the regime. In the throttled regime (2026-09-12: the measurement load gave
/// `frames=21` in 5 s, i.e. ~4 Hz) a broken three-second smoke makes ~12
/// frames, **1.5 times** `8` — this is the reason not to raise the limit from
/// here. No throttling was seen in the 2026-09-15 visible window. The likeliest
/// variable for the same binary giving two regimes under the measurement load
/// (`frames=21` in one run, `frames=597` in another) is window visibility, but
/// this is **unverified**: the 2026-09-15 runs saw the window on screen under the smoke load
/// and did not run the measurement load.
///
/// **As the limit grew, the gate's detection floor rose too** and its cost
/// will be paid later, not today. The gate fires at `n > limit`, so catching
/// needs `limit + 1` frames: in a three-second run the old `2` would catch a
/// **1 Hz** leak, today's `8` catches only **3 Hz**. (Both derive from `make
/// smoke`'s 3 seconds; if the duration changes, so does the threshold.) A
/// 2 Hz blink with a forgotten stop condition makes ~6 frames in three
/// seconds — below the limit, so this number alone **cannot see** it.
///
/// **So the limit is not the whole gate, only one tier.** The gate is built
/// in two tiers and both are independent of this one: (a) if an **unsettled**
/// animation remains at the deadline the run is red — independent of speed,
/// needs no measurement, but only sees animations that go through the motion
/// infrastructure; (b) the quiet between the last frame and the deadline
/// ([`QUIET_FLOOR`]) — sees leaks that bypass the infrastructure too and was
/// **measured**: it is now the gate's most sensitive tier, because
/// it catches every leak with a period shorter than 868 ms, while this number catches only those above 3 Hz.
///
/// **The mechanism of the variation in healthy runs was not measured.** The
/// frame request (`requests=`) stayed **constant** in all three measurements
/// (2–3 on 2026-09-15, 4 on 2026-09-16), so the extra frames do not come from extra
/// **requests** — had it been the geometry/occlusion hooks, `requests` would
/// have risen too. The variation split by profile in all three measurements
/// but its direction **turned** on 2026-09-16: on 2026-09-15 `frames` was mostly `1` in
/// debug and `2` in the release bundle; on 2026-09-16 `content` is mostly `3` in
/// debug and `2` in the release bundle. The request again did not split. What
/// remains is whether requests merge or not (if the startup frame was drawn
/// before the shell's first bytes a second frame is needed; the profile
/// difference could test this with `startup=`, it was not tested) but this is
/// a **hypothesis**, not a measurement.
///
/// **The gate is evaluated only on the `BT_RUN_SECONDS` path**
/// ([`AppDelegate::report_and_exit`]). The only unattended context is `make
/// smoke`; a run opened from the bundle with the same environment is subject
/// to the same limit (the 2026-09-15 broken bundle runs fired it). An interactive run never evaluates this limit.
///
/// **When to re-measure:** when a set arrives that changes the frame path or
/// the window's visibility (motion, tabs). Tabs arrived (2026-10-08): the
/// smoke run now measures the second of two tabs, born the way ⌘T opens one,
/// with the deadline at its birth — re-observed over three runs, its
/// `content`, `motion` and `quiet` stayed inside the bands above; not
/// re-measured.
///
/// **Known false positive (stays):** `DisplayLink::resize` requests a frame
/// unconditionally, so dragging the window during the run produces legitimate
/// frames and can exceed eight. `make smoke` runs unattended, the cost was
/// accepted; the lasting fix is to keep frames coming from the geometry path out of the counter.
///
/// The limit stands on the **drawn** frame because of its name: "zero frames
/// when idle" is a claim about drawing. `requests=` counts earlier but is not
/// a gate — its threshold was not measured.
///
/// **The measured `requests ≈ frames + 2` relation became invalid with motion**
/// and what the sentence corrects is not a number but a mechanism: motion
/// frames never touch the `Waker` (the `bt_gpu::link` module header), so they
/// inflate `frames` without inflating `requests`. The new form of the
/// relation was **measured** (2026-09-16, thirty healthy runs): `requests` is
/// `4` in all thirty runs, `content` `2`–`3`, i.e. `requests ≈ content +
/// 1..2` — while `frames` is 27–30, completely detached from it.
/// (In a later run `requests=3` was seen and its cause was not measured.)
/// Under the measurement load `requests` and `frames` differ by three orders
/// of magnitude (see `bt_gpu`'s `requests` counter); a gate could be built on
/// the ratio but that was not measured.
/// Re-observed after the move to the wgpu window path (2026-09-30):
/// unchanged.
const IDLE_FRAME_LIMIT: u64 = 8;

/// The **minimum** quiet expected at the end of a smoke run: between the last
/// drawn frame and the deadline (the `quiet=` token). Below it is red, `quiet=none` is red too.
///
/// The **complement of [`IDLE_FRAME_LIMIT`], not a copy.** That one sees a
/// leak drawing more than eight content frames in three seconds, i.e. only
/// above ~3 Hz; this one sees every leak whose **period** is shorter than
/// this value (above ~1.15 Hz). The measured gap was exactly this: a
/// half-second leak passes with `content=8` **without exceeding** the limit
/// and that run fell green (the "slow leak" measurement).
///
/// **The rule's direction is reversed in this token:** `quiet` is large in a
/// healthy run, small in a broken one. So the floor is "at most half the
/// lowest healthy observation **and** above the highest broken observation"
/// and is chosen from the **largest** end of the interval — a number chosen
/// from the middle would blind the gate to a slow leak. Derivation
/// (2026-09-17, twenty healthy runs): lowest healthy `1737.12 ms` → ceiling
/// `868.56 ms`; highest broken `129.25 ms` (2026-09-16). `868` is the largest whole millisecond of that interval.
///
/// **It has already fired once, and that is this constant's real lesson.**
/// The 2026-09-16 derivation had given `870` from an end of `1742.29 ms`;
/// after the smoke recipe changed, a twenty-run re-observation lowered
/// the band's lower end to `1737.12` and `870` exceeded the rule's ceiling by
/// **1.44 ms**. The gate was green in those runs — the excess hid in the
/// denominator, not in the number. Lesson: this constant's trigger is narrow
/// and **silent**; if a healthy three-second run drops below `1737 ms` what
/// breaks is not the gate but **the rule itself**, and the number must be re-derived.
///
/// **Four numbers are tied together**: `BT_RUN_SECONDS`'s 3,
/// [`bt_core::smoke_shell`]'s 1-second sleep, the same recipe's cursor jump
/// **distance** and this floor. The quiet is `run duration − (sleep +
/// settling)`, so **if either of the two moves this number must move too**:
/// with `BT_RUN_SECONDS=2` the tail shrinks to ~0.75 seconds and the gate
/// falls while the code is right. If the three are spread over three files,
/// when one moves the gate silently becomes fragile.
///
/// **A fifth input arrived with tabs:** the smoke run's background tab is
/// born first, from the same recipe, and its `\033[2G` must land while it is
/// hidden (the `back_wakes=` witness). The deadline counts from the measured
/// tab's birth, so this floor's tail is the measured tab's alone — but the
/// sleep now serves two clocks: shortened below the background tab's first
/// frame, its print lands before it hides and the run falls red on the
/// background arm, not here.
///
/// **Known false positive** (same root as [`IDLE_FRAME_LIMIT`]'s): dragging
/// the window, covering and uncovering it or waking the screen during the
/// last `QUIET_FLOOR` of the run gives birth to a legitimate frame and resets
/// the tail. The lasting remedy is the same: keep geometry-caused frames out
/// of the counter (a recorded debt).
///
/// Asked only in [`Workload::Smoke`]: the measurement load streams output
/// until the deadline, so there the quiet **must** be near zero (with the
/// same rationale as [`Verdict::MotionUnsettled`] being exempt in the same arm).
///
/// Re-observed after the move to the wgpu window path (2026-09-30): the
/// lowest healthy run was `1746.88 ms` (half: `873.44`), the highest broken
/// one `155.21 ms` — `868` is still inside the rule, unchanged.
const QUIET_FLOOR: Duration = Duration::from_millis(868);

/// The **single** branch of the entries that open onto the user's world.
///
/// A timed run (`make smoke`, measurement) does not see the settings file,
/// file watching, the system's light/dark appearance, the Reduce Motion
/// setting and the Theme menu filling from `themes/`:
/// the gate's result must not depend on that machine's `~/.config/bateri/`:
/// [`AppDelegate::load_settings`], which reads the file and sets up the
/// watch, and [`AppDelegate::reload_settings`], [`AppDelegate::apply_appearance`],
/// which reads the appearance, and `menuNeedsUpdate:`, which fills Theme ▸.
/// "Settings…", which creates the file ([`AppDelegate::edit_settings`]), and
/// the theme choice that writes ([`AppDelegate::save_theme`]) look at this
/// value too and do not write their own `run.is_some()` condition — the day
/// one of five separate conditions is forgotten the gate would silently be
/// tied to the user's file.
///
/// **The fifth is Reduce Motion** ([`resolve_reduce_motion`]) and
/// its branch is not in the settings file but in the system: if
/// `NSWorkspace`'s accessibility setting were read, `make smoke`'s `motion=`
/// token would be tied to the measuring machine's accessibility preference,
/// i.e. the gate would fall green on one machine and red on another. The
/// [`AppDelegate::observe_reduce_motion`] that sets up the observer looks at
/// the same value too.
///
/// The cost: no gate sees the wire from file to screen; temporary-directory
/// tests (`settings`) and visual inspection carry it.
///
/// **Not stored**, derived every time it is asked from [`AppDelegate::inputs`]
/// and `Ivars.run`: a separate ivar would be a second copy of the same
/// decision and the day the two diverge a timed run would read the user's file.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Inputs {
    /// Timed run: embedded defaults, nothing from outside.
    Hermetic,
    /// The user's session. `config_root` `None` → the home directory could not
    /// be resolved and the settings file is not searched for.
    User { config_root: Option<PathBuf> },
}

/// [`Inputs`]'s decision — pure, tested.
fn decide_inputs(run: Option<Run>, home: Option<PathBuf>) -> Inputs {
    match run {
        Some(_) => Inputs::Hermetic,
        None => Inputs::User {
            config_root: home.as_deref().map(settings::config_root),
        },
    }
}

/// Three-valued `[motion] reduce_motion` + the system's answer → a single `bool`.
///
/// **The combination is here because this is the layer that sees the system:**
/// `bt-gpu` does not see AppKit (the layer rule) and `bt-core`'s
/// settings model is already the counterpart of a file, not of an
/// accessibility setting. A **resolved** `bool` descends below (the `Renderer::set_font` precedent).
///
/// `system` is a **closure**, not a `bool`: in the session of a user who says
/// `"on"`/`"off"` `NSWorkspace` is never consulted. A timed run never
/// consults it either and this is not laziness but a gate — in
/// [`Inputs::Hermetic`] `make smoke`'s line would be tied to the measuring
/// machine's accessibility setting.
/// Pure and therefore testable: no real `AppDelegate` is needed
/// (`hermetic_run_does_not_read_reduce_motion`).
fn resolve_reduce_motion(
    inputs: &Inputs,
    setting: ReduceMotion,
    system: impl FnOnce() -> bool,
) -> bool {
    if let Inputs::Hermetic = inputs {
        return false;
    }
    match setting {
        ReduceMotion::On => true,
        ReduceMotion::Off => false,
        ReduceMotion::System => system(),
    }
}

/// `[terminal] scrollbar` + the system's scroll bar preference → the bar's
/// one resolved form ([`ScrollbarMode`]).
///
/// The [`resolve_reduce_motion`] precedent, for its reasons: this is the
/// layer that sees the system, `bt-gpu` gets the resolved value. `overlay`
/// is a **closure** — "Show scroll bars" in System Settings, as
/// `NSScroller.preferredScrollerStyle` answers it: macOS already resolves
/// "Automatically based on mouse or trackpad" for the devices attached, so
/// no device detection is written here. Overlay scrollers are the
/// self-hiding form (`Auto`), legacy ones the permanent one (`Always`).
///
/// **A timed run never asks the system and gets `Auto`** — `make smoke`'s
/// grid and tokens must not depend on the measuring machine's preference
/// (or a mouse plugged into it): `Always` would take columns from the grid.
fn resolve_scrollbar(
    inputs: &Inputs,
    setting: Scrollbar,
    overlay: impl FnOnce() -> bool,
) -> ScrollbarMode {
    if let Inputs::Hermetic = inputs {
        return ScrollbarMode::Auto;
    }
    match setting {
        Scrollbar::Auto => ScrollbarMode::Auto,
        Scrollbar::Always => ScrollbarMode::Always,
        Scrollbar::Never => ScrollbarMode::Never,
        Scrollbar::System if overlay() => ScrollbarMode::Auto,
        Scrollbar::System => ScrollbarMode::Always,
    }
}

/// `[motion] smooth_scroll` + Reduce Motion + `cursor_motion` → a single
/// `bool`: does the wheel go smooth.
///
/// If any of the three turns motion off, line stepping — scrolling does not
/// *add* animation for one who turned motion off (the same as
/// `cursor_motion = "snap"`'s relation to Reduce Motion). Quantization is
/// **at the source**, not in `bt-gpu`'s `Motion`: the `false` arm stays as
/// today's line path.
///
/// `reduce` is [`resolve_reduce_motion`]'s resolved answer, i.e. a timed run
/// does not read the system here either. Pure, tested.
fn resolve_smooth_scroll(settings: &Settings, reduce: bool) -> bool {
    settings.smooth_scroll == SmoothScroll::On
        && !reduce
        && settings.cursor_motion != CursorMotion::Snap
}

/// How a new window is opened — [`AppDelegate::open_window`]'s two decisions
/// come from here: tab or window, and whether the shell gets a first input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Opening {
    /// ⌘N, Dock icon, the startup's first window: a separate window, local shell.
    Window,
    /// ⌘T and the tab bar's `+`: a tab in `from`'s window, right of the
    /// selected one; to the same host if `from` is remote.
    Tab,
    /// Shell ▸ New Local Tab (⌥⌘T): a tab, always a local shell.
    LocalTab,
    /// Shell ▸ Split Right / Split Down (⌘D / ⇧⌘D): a split next to the focused
    /// pane; by ⌘T's rule, to the same host from a remote pane.
    /// The axis is carried by [`AppDelegate::open_split`].
    Split,
    /// Session restore: a saved pane — its directory, identity, zoom,
    /// history and ready remote line come from the save, not from a `from`
    /// ([`restored_launch`]).
    Restore,
}

/// A restored window and its built tabs: each tab's id with its index in
/// the saved window's tab list ([`AppDelegate::restore_window`]).
type RestoredWindow = (Retained<TerminalWindow>, Vec<(usize, u64)>);

/// Whether ⌘N opens a tab in `key` rather than a window — the system's
/// "Prefer tabs when opening documents" (Desktop & Dock): always, or in full
/// screen while `key` is. With macOS's own tabs off this is no longer
/// AppKit's to do, so it is read here.
fn prefers_tabs(mtm: MainThreadMarker, key: &TerminalWindow) -> bool {
    match NSWindow::userTabbingPreference(mtm) {
        NSWindowUserTabbingPreference::Always => true,
        NSWindowUserTabbingPreference::InFullScreen => key
            .ns_window()
            .styleMask()
            .contains(NSWindowStyleMask::FullScreen),
        _ => false,
    }
}

/// The new shell's first input: only with ⌘T and splits and only from a remote
/// `from` — the line is `from`'s remote target's escaped line ([`bt_core::Session::remote_line`]).
/// ⌘N is a new workspace, ⌥⌘T the escape route; both are local.
fn initial_line(opening: Opening, remote_line: Option<String>) -> Option<String> {
    match opening {
        Opening::Tab | Opening::Split => remote_line,
        Opening::Window | Opening::LocalTab | Opening::Restore => None,
    }
}

/// The session directory ([`restore::directory`]) — `None` in a timed run and
/// in an unbundled process (`cargo run`), which neither restore nor save,
/// and when the home directory cannot be resolved.
///
/// `bundle_id` and `home` are **closures**, the precedent of
/// [`shell_integration_env`]: a timed run never asks either, so `make smoke`
/// cannot depend on — or touch — the user's saved session
/// (`hermetic_run_does_not_restore_or_save`).
fn restore_dir(
    inputs: &Inputs,
    bundle_id: impl FnOnce() -> Option<String>,
    home: impl FnOnce() -> Option<PathBuf>,
) -> Option<PathBuf> {
    if let Inputs::Hermetic = inputs {
        return None;
    }
    let bundle_id = bundle_id()?;
    let home = home()?;
    Some(restore::directory(
        &home.join("Library/Application Support"),
        &bundle_id,
    ))
}

/// A saved pane's start: its directory, its identity, its history
/// and its remote target's line **ready, not run** — the user's ⏎
/// connects. A directory that no longer exists is the session's to handle
/// (an unreachable one is inherited, `SessionOptions::working_directory`).
fn restored_launch(pane: &SavedPane, replay: Option<Vec<u8>>) -> Launch {
    Launch {
        working_directory: pane.dir.clone(),
        initial_input: pane.remote_line.clone().map(InitialInput::ready),
        tab_id: Some(pane.tab_id.clone()),
        replay,
        adopt: None,
    }
}

/// Where a deliberate handover goes ([`AppDelegate::hand_over`]).
enum Target {
    /// The update's holder, spawned when the quit was found to be a relaunch.
    Update(handover::Spawned),
    /// The bound holder, over the connection it has had since launch, and
    /// which process it is (a failed handover ends it).
    Bound(handover::Bound, keeper::HolderId),
}

impl Target {
    /// Nothing is handed over after all: the update's holder goes with its
    /// connection, the bound one leaves quietly — dropped, it would take the
    /// end of its connection for a crash and keep the programs on.
    fn dismiss(self) {
        match self {
            Target::Update(holder) => drop(holder),
            Target::Bound(bound, _) => bound.quit(),
        }
    }
}

/// A copy of a frozen bundle with every master duplicated — the spare a
/// failed handover falls back on. `None` if a duplicate fails (no spare,
/// rather than a partial one that would leave a pane behind silently).
fn copy_bundle(bundle: &handover::Bundle) -> Option<handover::Bundle> {
    let panes = bundle
        .panes
        .iter()
        .map(|pane| {
            Some(HeldPane::new(
                pane.tab.clone(),
                pane.pid,
                pane.start,
                pane.blob.clone(),
                pane.buffer.clone(),
                pane.master.try_clone().ok()?,
            ))
        })
        .collect::<Option<Vec<_>>>()?;
    Some(handover::Bundle {
        layout: bundle.layout.clone(),
        panes,
    })
}

/// How long after the first windows are built a launch counts as settled
/// ([`AppDelegate::settled`]): the attempt markers go and the layout starts
/// going to disk. Not counted in frames — a background tab, a pane hidden by
/// a zoom or a sleeping display draws none, and a launch with two tabs would
/// never settle. What can still crash a launch after its windows are built is
/// the readers parsing each adopted pane's carried output, at most a holder's
/// buffer ([`handover::BUFFER_LIMIT`]) each, which takes a fraction of this;
/// a crash later than this is another fault than the restore and must not
/// push the next launch toward giving the programs up. A design constant.
const SETTLE_DELAY: Duration = Duration::from_secs(5);

/// How long the system waits before it shows ⌘Q's reminder that programs
/// keep running ([`AppDelegate::leave_quit_notice`]). A notification arriving
/// while bateri is still in front is silenced, so it must arrive after the
/// process is gone: what is left of the quit once the reminder is added is at
/// most [`NOTICE_WAIT`] and the process's exit, and two seconds clear that
/// while still reading as the quit's own answer. A design constant.
const NOTICE_DELAY: Duration = Duration::from_secs(2);

/// How long the quit waits for the system to take the reminder: the request
/// must leave the process before it exits, and the system answers within a
/// scheduling round; a system that does not answer within a second does not
/// get to hold the quit. A design constant.
const NOTICE_WAIT: Duration = Duration::from_secs(1);

/// The layout source of the bound holder ([`keeper::LayoutSource`]): the
/// application's live windows.
fn current_layout(mtm: MainThreadMarker) -> Option<Vec<u8>> {
    delegate(mtm)?.layout_blob()
}

/// A pane the holder gave that cannot be carried on: handed
/// back for release, with the history its blob carried if it decoded and
/// whether its program is known to have ended (the holder saw it end, or its
/// pid is gone).
#[derive(Debug)]
struct Refused {
    pane: HeldPane,
    history: Option<Vec<u8>>,
    ended: bool,
}

/// Whether a held pane can be carried on: its program alive as the holder
/// saw it, its blob of a version this binary reads, and an exit watch on
/// its child (`watch` is [`jobs::exit_fd`]: `None` if the pid died or was
/// reused). `bt-core`'s own blob is checked by `Session::adopt` itself, in
/// the pane, which falls back there.
fn adoption(
    held: HeldPane,
    watch: impl FnOnce(u32, u64) -> Option<std::os::fd::OwnedFd>,
) -> Result<Adopted, Box<Refused>> {
    let Some(state) = PaneState::decode(&held.blob) else {
        return Err(Box::new(Refused {
            pane: held,
            history: None,
            ended: false,
        }));
    };
    let history = || Some(state.history.clone()).filter(|bytes| !bytes.is_empty());
    if held.ended {
        return Err(Box::new(Refused {
            history: history(),
            pane: held,
            ended: true,
        }));
    }
    let Some(exit) = watch(held.pid, held.start) else {
        return Err(Box::new(Refused {
            history: history(),
            pane: held,
            ended: true,
        }));
    };
    Ok(Adopted {
        master: held.master,
        exit,
        pid: held.pid,
        state,
        prefix: held.buffer,
        taken_from: None,
        mode: AdoptMode::Update,
        nudge: false,
        note: Note::Update,
    })
}

/// What the first windows take from the holders
/// ([`AppDelegate::restore_arrival`]).
struct Arriving<'a> {
    arrival: &'a mut Arrival,
    /// The second attempt at these holders ([`restore::AttemptMode::Safe`]).
    safe: bool,
    /// The holder whose layout is being built — whose kind a pane it names
    /// but nobody holds speaks for ([`fallen_note`]).
    layout_of: usize,
    /// Per holder, [`deliberate_holders`].
    deliberate: Vec<bool>,
}

impl Arriving<'_> {
    /// The kind of the holder at `link`: its socket's name, then whether it
    /// was handed its panes.
    fn kind(&self, link: usize) -> HolderKind {
        let bound = self
            .arrival
            .holders
            .get(link)
            .is_some_and(|holder| handover::is_bound_socket(&holder.socket));
        if bound {
            HolderKind::Bound {
                deliberate: self.deliberate.get(link).copied().unwrap_or(false),
            }
        } else {
            HolderKind::Update
        }
    }
}

/// Which holder a pane comes from — the socket's name tells (the update's
/// `handover`, a bound one's `handover-<pid>`), the frame does not: it
/// carries a quit's handover and an update's to a bound holder alike.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HolderKind {
    /// The update's holder, spawned at the moment of the update.
    Update,
    /// A bound holder; `deliberate` if it was handed the panes (a quit, or an
    /// update that found it) rather than left with them by a crash.
    Bound { deliberate: bool },
}

impl HolderKind {
    fn mode(self) -> AdoptMode {
        match self {
            HolderKind::Update => AdoptMode::Update,
            HolderKind::Bound { .. } => AdoptMode::Bound,
        }
    }
}

/// Per holder, whether it was handed its panes deliberately: any of them
/// carries a frozen screen — only a freeze makes one; a crash bundle's
/// screens, when it has any, were rebuilt from the journal and say so
/// (`HeldPane::crashed`).
fn deliberate_holders(arrival: &Arrival) -> Vec<bool> {
    deliberate_of(arrival.holders.len(), &arrival.panes)
}

/// [`deliberate_holders`] over `holders` holders and their panes.
fn deliberate_of(holders: usize, panes: &[(usize, HeldPane)]) -> Vec<bool> {
    let mut deliberate = vec![false; holders];
    for (link, pane) in panes {
        if !pane.crashed
            && PaneState::decode(&pane.blob).is_some_and(|state| !state.vt.is_empty())
            && let Some(slot) = deliberate.get_mut(*link)
        {
            *slot = true;
        }
    }
    deliberate
}

/// The note of a pane whose program did not come back, by the holder it
/// came from (or whose layout named it) and whether the program is known to
/// have ended.
fn fallen_note(kind: HolderKind, ended: bool) -> Note {
    match kind {
        HolderKind::Update => Note::Update,
        HolderKind::Bound { .. } if ended => Note::Ended,
        HolderKind::Bound { deliberate: true } => Note::NotCarried,
        HolderKind::Bound { deliberate: false } => Note::Crash,
    }
}

/// The replay of a pane that falls back to a new shell, by
/// `restore_windows`: `"all"` its history and the note — no history in the
/// second attempt (`safe`: what went through the parser before the crash is
/// not replayed again) —, `"layout"` the note alone, `"off"` nothing at all:
/// the pane does not come back.
fn fallen_replay(
    setting: RestoreWindows,
    safe: bool,
    history: Option<Vec<u8>>,
    note: Note,
) -> Option<Vec<u8>> {
    match setting {
        RestoreWindows::Off => None,
        RestoreWindows::Layout => Some(fallen_back(None, note)),
        RestoreWindows::All => Some(fallen_back(history.filter(|_| !safe), note)),
    }
}

/// The screen an adopted pane comes back with; `true` if its program is to
/// be nudged into redrawing it (`Session::nudge_size`).
///
/// A frozen screen (a deliberate handover) comes back whole; one that lost
/// output while bateri was closed (the holder's cut) gets the note under it.
/// A crash's pane has no screen: the note goes on the empty grid's first
/// line, where the program's redraw covers it rather than mixing with it.
/// The second attempt (`safe`) replays nothing of what went through the
/// parser before the crash — no screen, no state blob, no carried output.
fn adopted_screen(adopted: &mut Adopted, cut: bool, safe: bool) -> bool {
    if safe {
        adopted.state.vt = Note::Screenless.line();
        adopted.state.core.clear();
        adopted.prefix.clear();
        return true;
    }
    if adopted.state.vt.is_empty() {
        adopted.state.vt = Note::Screenless.line();
        return true;
    }
    if cut {
        adopted.state.vt.extend_from_slice(b"\r\n");
        adopted.state.vt.extend_from_slice(&Note::Cut.line());
        return true;
    }
    false
}

/// What an update's relaunch waits for ([`AppDelegate::postpone_update`]):
/// the application's unfinished transfers and an open password
/// sheet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct UpdateWait {
    transfers: usize,
    sheet: bool,
}

impl UpdateWait {
    /// Whether the relaunch must wait — the user's answer:
    /// it waits rather than cutting a transfer or a sign-in short.
    fn holds(self) -> bool {
        self.transfers > 0 || self.sheet
    }
}

/// **The handover's sequence point** in this process
/// ([`handover::arrive`]): called by [`crate::run`] first, before the
/// application delegate is born — its ssh registry's sweep runs `ssh` on a
/// thread of its own ([`masters`]), and no child may be spawned while a
/// received master is not yet close-on-exec (macOS' `recvmsg` has no
/// `MSG_CMSG_CLOEXEC`). Never in a timed run nor in an unbundled process.
///
/// `bound`: whether the bound holders are asked — not with ⇧ held
/// ([`shift_held_at_launch`]); the update's holder is asked either way.
pub(crate) fn arrive(opts: &Options, bound: bool) -> Option<Arrival> {
    if opts.run.is_some() {
        return None;
    }
    let bundle_id = NSBundle::mainBundle().bundleIdentifier()?.to_string();
    // SAFETY: `getuid` has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    let roots = ssh_route::socket_bases(child::home().as_deref(), uid);
    handover::arrive(&roots, uid, std::process::id(), &bundle_id, bound)
}

/// Whether ⇧ is held as this launch begins — macOS' "launch without
/// restoring" gesture: the bound holders are not asked (their programs wait
/// for the next launch, untouched) and the saved session is not read, so a
/// restore that crashes every launch has a way out. An update's holder is
/// still taken: it ends its programs if nobody comes ([`arrive`]). Read at
/// the sequence point, before [`arrive`] and before `NSApplication` exists:
/// the class's modifier state is the window server's, it needs no
/// application. Never in a timed run.
pub(crate) fn shift_held_at_launch(opts: &Options) -> bool {
    opts.run.is_none() && NSEvent::modifierFlags_class().contains(NSEventModifierFlags::Shift)
}

/// A saved window frame clamped onto a visible screen: the screen
/// it overlaps most, or the main one (the first) if it overlaps none — a
/// display unplugged since the quit; the size shrinks to the screen and the
/// origin moves inside. Frames are AppKit's (bottom-left origin, points), the
/// screens their `visibleFrame` (no menu bar, no Dock). No screen: unchanged.
fn clamp_frame(frame: Frame, screens: &[Frame]) -> Frame {
    let overlap = |screen: &Frame| {
        let width = (frame.x + frame.width).min(screen.x + screen.width) - frame.x.max(screen.x);
        let height = (frame.y + frame.height).min(screen.y + screen.height) - frame.y.max(screen.y);
        width.max(0.0) * height.max(0.0)
    };
    let mut best: Option<(&Frame, f64)> = None;
    for screen in screens {
        let area = overlap(screen);
        if area > 0.0 && best.is_none_or(|(_, most)| area > most) {
            best = Some((screen, area));
        }
    }
    let Some(screen) = best.map(|(screen, _)| screen).or_else(|| screens.first()) else {
        return frame;
    };
    let width = frame.width.min(screen.width);
    let height = frame.height.min(screen.height);
    Frame {
        x: frame.x.clamp(screen.x, screen.x + screen.width - width),
        y: frame.y.clamp(screen.y, screen.y + screen.height - height),
        width,
        height,
    }
}

/// The environment shell integration adds to the child — empty if not set up.
///
/// **The whole decision is here and pure**: which shell, which setting, where
/// the script is. Its place is `app` not `child`, because the gate's first tier is [`Inputs`] and it is private to this module
/// ([`resolve_reduce_motion`] precedent; there too the side that reads the
/// system is `bt-shell-macos` but the decision is gated by `Inputs`).
///
/// `shell` and `script_dir` are **closures**: in a timed run and in the
/// session of a user who says `"off"` neither is ever consulted. The timed
/// run's is not laziness but a **gate** — `make smoke`'s result would be tied
/// to the measuring machine's shell configuration and a test whose closure
/// panics holds the gate (`hermetic_run_does_not_set_up_shell_integration`).
///
/// `zdotdir` is **eager**: it is our own process's environment, not an entry
/// open to the user's world, and in the hermetic arm its value never reaches the child anyway.
///
/// The return is a `Vec`, not an `Option`: the environment set up can be not
/// one pair but **two** (if the user has an original `ZDOTDIR` the second goes
/// too) and the caller chains it next to `locale_env()`.
fn shell_integration_env(
    inputs: &Inputs,
    setting: ShellIntegration,
    shell: impl FnOnce() -> Option<PathBuf>,
    script_dir: impl FnOnce() -> Option<PathBuf>,
    zdotdir: Option<OsString>,
) -> Vec<(String, String)> {
    if matches!(inputs, Inputs::Hermetic) || !setting.installs_wrapper() {
        return Vec::new();
    }
    // A shell we do not recognize silently falls back: the terminal works as
    // today, only the marks do not arrive.
    if !shell().is_some_and(|shell| child::is_zsh(&shell)) {
        return Vec::new();
    }
    // A non-UTF-8 path is the same silent fallback: `SessionOptions.env`
    // wants a `String` and a session without integration is better than a
    // half-set-up `ZDOTDIR`.
    let Some(dir) = script_dir().and_then(|dir| dir.into_os_string().into_string().ok()) else {
        return Vec::new();
    };
    // The user's original `ZDOTDIR`: the script will put it back. All three
    // arms say "the second pair should not go" but their reasons differ:
    let original = match zdotdir {
        // An empty value counts as undefined (`decide_locale`'s rule) —
        // "putting back" an empty `ZDOTDIR` would create a variable pointing
        // at `$HOME`.
        None => None,
        Some(value) if value.is_empty() => None,
        Some(value) => match value.into_string() {
            // **A self-pointing value** (found in code review): if the
            // `ZDOTDIR` in the environment already points at the script's
            // directory (set by hand or leaked), handing it back as "the
            // user's original value" makes the script reload its own
            // `.zshenv` and recurse to zsh's `FUNCNEST` limit; the session is
            // left without `ZDOTDIR`. The script has a layer for this too, this is the first layer.
            Ok(value) if value == dir => None,
            Ok(value) => Some(value),
            // **A non-UTF-8 value rejects the integration entirely** and this
            // arm is the reason it wants `var_os` instead of `var`
            // (found in code review): `var().ok()` dropped it to `None`,
            // i.e. it counted as "the user had no `ZDOTDIR`" and the script
            // **deleted** the variable at the end of the session — the user's
            // entire configuration would be lost without a diagnostic. Every
            // neighboring edge (a non-UTF-8 script path, an unrecognized
            // `$SHELL`) falls back by rejecting the integration; `decide_locale`
            // also deliberately separates "absent" from "unusable".
            Err(_) => return Vec::new(),
        },
    };
    let mut env = vec![("ZDOTDIR".to_owned(), dir)];
    if let Some(original) = original {
        env.push(("BATERI_ZDOTDIR".to_owned(), original));
    }
    // **Sent only at the `blocks` tier** (the same shape as `BATERI_ZDOTDIR`
    // being conditional): in the default arm we add not a single byte to the
    // environment and the script's "no variable → the prompt is the terminal's" rule becomes the default's **only** record. If it were
    // written in two places, when one changed the other would silently age.
    //
    // The variable's name states the decision, not its result: the script
    // derives **three** things from it (should the prompt be reset, should the
    // mirror be set up, should the branch be printed) and all three are the
    // answer to "is there a dock in this session". The terminal gives the
    // decision, the shell is not asked (`ShellIntegration::wants_dock`).
    if !setting.wants_dock() {
        env.push(("BATERI_DOCK".to_owned(), "off".to_owned()));
    }
    env
}

/// Adds `BATERI_BIN` to a session's shell integration: the path of the
/// running bateri, which the wrapper's `ssh` function asks for the wrapping
/// decision (`bateri ssh-argv`). Only where the wrapper is installed — an
/// empty `env` stays empty, so neither the timed run nor a non-zsh shell nor
/// `[shell] integration = "off"` gets it — and only a UTF-8 path
/// (`SessionOptions.env` wants a `String`; without it the function falls back
/// to plain `ssh`). With it, `BATERI_SSH_INSTANCE`: the masters' instance
/// directory name, where a wrapped session becomes a master
/// (`ssh_route::session_socket`) — none without masters (the timed run).
fn with_bateri_bin(
    mut env: Vec<(String, String)>,
    bin: Option<PathBuf>,
    instance: Option<&str>,
) -> Vec<(String, String)> {
    if env.is_empty() {
        return env;
    }
    if let Some(bin) = bin.and_then(|bin| bin.into_os_string().into_string().ok()) {
        env.push(("BATERI_BIN".to_owned(), bin));
        if let Some(instance) = instance {
            env.push(("BATERI_SSH_INSTANCE".to_owned(), instance.to_owned()));
        }
    }
    env
}

/// The grid derived from the window geometry + the cell size.
///
/// Its name is not `Metrics`: the owner of the cell metrics is now `bt-gpu`
/// ([`CellMetrics`]) and the two types are read side by side in this file.
/// Here "how many columns how many rows **and** with which cell", there only the cell.
///
/// Open to the `view` module too: mouse translation wants the same triple and
/// the derivation stays here once instead of being repeated at two call sites.
#[derive(Clone, Copy)]
pub(crate) struct Grid {
    pub(crate) cols: u16,
    pub(crate) rows: u16,
    /// The dock's width, columns: the window's, with no scroll bar reserve
    /// taken off — the always-up bar's track stops at the dock's top, and
    /// the dock below it stays full width. Equal to `cols` unless the grid
    /// reserves the track. The link's dock readings and the mouse's dock hit
    /// come from here ([`split_into_grid`] is the owner).
    pub(crate) dock_cols: u16,
    /// Not a tuple but `CellMetrics`: the metrics pass from here to
    /// `DisplayLink::resize` as they are. The value stored in `Grid` alone
    /// descends to a tuple when it enters `SessionOptions` — the tuple that
    /// `split_into_grid` splits is another value: the **incoming** metrics
    /// enter it, `Grid` is born after it.
    pub(crate) cell: CellMetrics,
}

/// Pixel geometry + grid metrics → grid.
///
/// It stands apart from `TerminalPane::sync_geometry` because this is the
/// only pure piece; the rest is window and layer, i.e. untestable. The
/// metrics are an **argument**: a constant hidden in this body would make `cell_metrics_come_from_outside` fail.
///
/// The scope is this much, no more: the line where `CELL_PX` really stood was
/// the `cell_metrics(scale)` call in `sync_geometry` and it is not tested
/// because it wants a window and a Metal device. `CellMetrics::new` is
/// deliberately `pub`, so a placeholder like `CellMetrics::new(9, 18, 7, 8,
/// 1, 1.0)` written there would revive and the two tests here would stay green.
///
/// **The left gutter is subtracted from the columns**: so the
/// stripe does not overlap the text. The gutter is always reserved — the
/// accepted cost is that it stays empty in a session without integration
/// (bash/fish, `shell.integration = false`, SSH); the alternative was a
/// SIGWINCH at the first prompt and three consumers being updated at once.
///
/// **The dock share is subtracted from the rows** and, unlike the left
/// gutter, is **conditional**: the dock exists only in an integrated zsh
/// session and the decision is made while the session is born
/// (`TerminalPane::start`). Reserving the share unconditionally would take
/// two rows for no reason from a window without a dock — a cost not
/// comparable with the left gutter's eight points.
///
/// **The share varies during the run**: it drops to zero on the
/// alternate screen and returns to its birth value on exit (`dock_rows_for`,
/// `TerminalPane::alt_screen_did_change`). The cost of varying is one `TIOCSWINSZ` and that cost is paid **per
/// transition, not per command** — commands like `git log` that do not enter
/// the alternate screen never move the flag, so this function is not called again either.
///
/// **The scroll bar's reserve is subtracted from the grid's columns only**
/// (`reserve_px`, [`ScrollbarMode::reserve_px`]): in the always-up form the
/// track takes the window's right edge down to the dock's top, so the grid
/// gives its width up and the dock does not — the dock's column count is a
/// separate answer ([`Grid::dock_cols`]). The reserve is a function of the
/// form alone, never of the history or the alternate screen: a reserve that
/// came and went with them would resize the grid on the first line into
/// history, every clear and every full-screen program.
///
/// **The top edge's reserve is subtracted from the rows** (`top_px`,
/// [`bt_gpu::edge_reserve_px`]): with the content fading at the pane's top,
/// that much of the height is kept free above the grid **even when the height
/// divides into rows**, and the leftover goes **to the top** — the grid sits
/// on the dock's share, a row at rest never enters the fade, and the drawing
/// side fades the whole leftover ([`bt_gpu::edge_drawn_px`]). Its source is the
/// left margin, so changing the margin moves the row count too. The cost is a
/// row less at the heights whose leftover is shorter than the reserve; zero
/// where the content is cut instead, every row the height allows as before.
/// Like the scroll bar's reserve it is a function of the mode and the cell
/// alone — never of the history or the alternate screen.
pub(crate) fn split_into_grid(
    width_px: f64,
    height_px: f64,
    cell: CellMetrics,
    dock_rows: u16,
    reserve_px: f32,
    top_px: f32,
) -> Grid {
    let (cell_w, cell_h) = cell.cell_px();
    // `as u16` saturates in f64 (NaN and negative → 0, large → 65535) and the
    // truncation is exactly the floor rounding we want; `Session::resize`
    // already ignores zero columns/rows (a minimized window). The divisor
    // cannot be zero and the type carries this: `CellMetrics`'s field is
    // private and its constructor (`CellMetrics::new`) rejects zero; its
    // source in production is `Renderer::cell_metrics`, and the guarantee of the ratio is `bt-atlas`'s ≥ 1 clamp.
    //
    // The subtraction is **in `f64`** and this is not a preference but a
    // requirement: in a window narrower than the gutter the difference goes
    // negative, the division stays negative and `as u16` saturates it to zero
    // — i.e. the existing behavior (zero columns, `Session::resize` ignores
    // it) is preserved. Had the same subtraction been done in `u16` it would
    // **overflow** and produce a column count near 65535, a `TIOCSWINSZ` of
    // that size. No new lower bound is deliberately introduced: the end of the chain is already right.
    let usable_width = width_px - f64::from(cell.gutter_px());
    // The scroll bar's reserve is subtracted the same way and for the same
    // reason: in a window narrower than the gutter and the track the
    // difference goes negative and saturates to zero columns. The width here
    // is unrounded and the drawn track's left edge is the texture's rounded
    // width minus the same reserve; the text still ends at or before it,
    // because where the text ends — the gutter plus whole cells — is a whole
    // pixel not past `width − reserve`, so not past its floor either.
    let grid_width = usable_width - f64::from(reserve_px);
    // The dock share is also **in `f64`** and for the same reason: in a window
    // shorter than the dock the difference goes negative, the division stays
    // negative and `as u16` saturates it to zero — `Session::resize` already
    // ignores that size. Done in `u16` it would overflow and produce a
    // 65535-row `TIOCSWINSZ`. The formula is **bt-gpu's** ([`bt_gpu::dock_px`]):
    // the dock's share carries two breathing margins next to the rows and were
    // it rewritten here it would diverge for one frame on resize — the same
    // discipline as consuming `DOCK_ROWS`, no second copy is kept.
    //
    // The top edge's reserve is subtracted the same way: it is a pixel height
    // like the dock's share, and the window that cannot hold it gets no rows.
    let usable_height = height_px - f64::from(bt_gpu::dock_px(dock_rows, cell)) - f64::from(top_px);
    Grid {
        cols: (grid_width / f64::from(cell_w)) as u16,
        rows: (usable_height / f64::from(cell_h)) as u16,
        dock_cols: (usable_width / f64::from(cell_w)) as u16,
        cell,
    }
}

/// The daily preview sweep's period — "once a day", a design
/// constant.
const DAILY_SWEEP: Duration = Duration::from_secs(24 * 60 * 60);

/// The application's delegate — the way back from a window to the app level.
///
/// A window does **not** hold a reference to the app delegate: the delegate
/// lives for the whole process and can be found each time from `NSApp`'s
/// `delegate` property, so a stored reference would only add a cycle or a
/// dangling possibility. Its callers: main-queue jobs (the alternate-screen
/// messenger, the title news, the shell's exit, a closing window's removal
/// from the list — from id to window), point-size actions (the setting's
/// font) and geometry (the font diagnostic's subtitle). `None` → the delegate is not bound yet; the caller silently drops.
pub(crate) fn delegate(mtm: MainThreadMarker) -> Option<Retained<AppDelegate>> {
    let delegate = NSApplication::sharedApplication(mtm).delegate()?;
    let object: &AnyObject = (*delegate).as_ref();
    object.downcast_ref::<AppDelegate>().map(Message::retain)
}

/// The open pane whose id is `id` — the lookup path in the pane's birth
/// package (`pane::PaneLookup`): jobs returning to the main queue from the
/// reader thread and from background jobs find the pane with it.
/// A plain `fn`, i.e. `Send`, and the pane's module does not see `AppDelegate`.
pub(crate) fn pane_by_id(mtm: MainThreadMarker, id: u64) -> Option<Retained<TerminalPane>> {
    delegate(mtm)?.pane(id)
}

/// The notification of the watch sources ([`notify_settings_changed`]).
///
/// The watch notifies on its own background queue (`watch`'s contract); the
/// applier needs the main thread, so the event hops there.
///
/// **At most one hop in flight** ([`WATCH_PENDING`]): earlier the sources
/// ran on the main queue and libdispatch merged the events that arrived while
/// main was busy into one handler call. Without the flag a burst (a chunked
/// write, a rename-over's directory + file events) would reload the settings
/// once per event on the main thread.
fn watch_notify() -> Notify {
    Arc::new(|| {
        if !WATCH_PENDING.swap(true, Ordering::AcqRel) {
            DispatchQueue::main().exec_async(|| {
                // Cleared **before** the reload reinstalls and rereads: an
                // event after this point posts a new hop, so no save is lost.
                WATCH_PENDING.store(false, Ordering::Release);
                notify_settings_changed();
            });
        }
    })
}

/// Whether a watch event's hop to the main queue is already queued
/// ([`watch_notify`]).
static WATCH_PENDING: AtomicBool = AtomicBool::new(false);

/// Carries a watch event to the applier: **with a targetless action** to
/// `settingsDidChange:`, through the path of the appearance change (`view.rs`).
///
/// It captures nothing: had the source's context held a delegate reference
/// the cancel handler would drop it and tie its lifetime to libdispatch's
/// cancel timing. The responder chain reaches `NSApp` and its delegate even
/// when no window is key (the user is in the editor).
fn notify_settings_changed() {
    // audit: the only caller is `watch_notify`, which runs this through
    // `DispatchQueue::main().exec_async`, and work on the main queue is on the
    // main thread by definition.
    let mtm = MainThreadMarker::new().expect("the watch notification is hopped to the main queue");
    let app = NSApplication::sharedApplication(mtm);
    // SAFETY: the selector is valid; the target `None` → the responder chain.
    // Its receiver is `AppDelegate::settings_did_change`, which takes a single
    // `Option<&AnyObject>` argument and does not look at the sender. If there
    // is no receiver (the delegate is not bound yet) it returns `false` and the event drops; the next save arrives again.
    let _ = unsafe { app.sendAction_to_from(sel!(settingsDidChange:), None, None) };
}

/// The dock share to reserve while the session is born.
///
/// **Both conditions are necessary and separate questions.** If `integration`
/// is empty the wrapper was never set up — hermetic run, `"off"`, an
/// unrecognized shell, a non-UTF-8 script path — i.e. there is no mirror to
/// fill the dock. `wants_dock` is **the user's choice**: at the `"blocks"`
/// tier the wrapper is set up (blocks and marks are its whole reason) but the
/// input line stays in the grid, i.e. no share is reserved.
///
/// Deriving one from the other would bring back a closed defect:
/// **two prompts** on screen (the user's in the grid, the dock's
/// below) and a caret jumping between them.
fn dock_rows_at_birth(integration: &[(String, String)], setting: ShellIntegration) -> u16 {
    if integration.is_empty() || !setting.wants_dock() {
        0
    } else {
        DOCK_ROWS
    }
}

/// This moment's dock share: **zero** on the alternate screen, otherwise the birth value
/// — except a **remote** session's alternate screen, where the dock stays as the
/// one-row status bar (`⇄ host`, the transfer line): vim on the server still
/// shows where it runs. One row is exactly the context band (`band_px(0) ==
/// dock_px(1)`), so the remote app's grid is not offset.
///
/// The birth value is a separate input and this is mandatory: in a session
/// without integration (`birth == 0`) leaving the alternate screen must not
/// **give birth** to a dock. Had a single `dock_rows` field been written over,
/// one would have to rebuild from the `DOCK_ROWS` constant, and that is exactly the way to conjure a dock that does not exist.
///
/// Pure: this is `bt-shell-macos`'s only half testable without AppKit.
pub(crate) fn dock_rows_for(alt_screen: bool, remote: bool, birth: u16) -> u16 {
    match (alt_screen, remote) {
        _ if birth == 0 => 0,
        (true, true) => 1,
        (true, false) => 0,
        (false, _) => birth,
    }
}

/// The first item that holds the key and is **not closed** — the single rule
/// for lookups by id (`AppDelegate::pane`, `window_by_tab`).
/// `key` gives, per item, `(matches, closed)`.
///
/// A closed item is `None` even if it matches: the pane's window leaves the
/// list one turn later (`forget_window`) and in the meantime a stale message
/// from the reader thread or a `bateri://tab/` open must not find a closed
/// session. Pure, tested.
fn find_open<T>(items: impl IntoIterator<Item = T>, key: impl Fn(&T) -> (bool, bool)) -> Option<T> {
    items.into_iter().find(|item| {
        let (matches, closed) = key(item);
        matches && !closed
    })
}

/// Opens the file in the user's editor; `false` if no way could open it.
///
/// First the file type's default application (`NSWorkspace`, the path of a
/// double-click in Finder). The application that claims `.toml` does not
/// exist on every machine — even if the system does not recognize the type
/// nobody may open it; then the default **text** editor (`open -t`, TextEdit
/// on most machines). The second is a child process and its return is awaited: `open` hands the job to LaunchServices and exits at once.
///
/// **Known limit — the main thread waits.** `open` does not return while the
/// editor starts cold and the display link is on the main thread: meanwhile
/// the window draws no frames and no keys are processed (a code review
/// finding, waived). Only when no application claims `.toml`
/// and on the user's own click; focus is moving to the editor anyway. Not waiting would cut the error's path to the subtitle.
fn open_in_editor(path: &Path) -> bool {
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    if NSWorkspace::sharedWorkspace().openURL(&url) {
        return true;
    }
    std::process::Command::new("/usr/bin/open")
        .arg("-t")
        .arg(path)
        .status()
        .is_ok_and(|status| status.success())
}

/// Turns off the **accent popover** of a held-down letter: in a terminal a
/// held key means **repeat** (`j` in vim, `u` in the shell), the popover would swallow it.
///
/// The side effect comes with `NSTextInputClient` itself: in a
/// view that does not implement the protocol the popover did not appear anyway.
///
/// The place written is the app's **own `registerDefaults`**, i.e. the
/// in-memory registration domain: the user's plist is left untouched and the
/// setting does not carry from run to run. The rationale is the same as the
/// rule of not touching the shell's rc file — we do not write to the user's
/// file. The evidence of an installed product shows the same key: iTerm2 keeps
/// `ApplePressAndHoldEnabled = 0` in its own domain.
///
/// **Half was measured** (2026-09-20): in this machine's `NSGlobalDomain`
/// `ApplePressAndHoldEnabled` **does not exist** (`defaults read -g`), i.e.
/// in the lookup order there is no link above the registration domain to
/// override it — the set gate's objection "NSGlobalDomain or MDM may be overriding" is moot in this setup.
///
/// **The second half was measured too** (2026-09-20, the user in a real
/// window): when a letter is held the popover **does not appear**, i.e.
/// AppKit reads the decision through `NSUserDefaults` and sees the
/// registration domain. The source of the suspicion was the possibility of
/// looking at `CFPreferences` directly (iTerm2 keeping the value in the
/// *persistent* domain was a hint of that possibility; ghostty uses the same
/// `registerDefaults` path) and it fell. A hermetic test would depend on the machine; what closed it was holding `e` down.
///
/// **Had it not held, the symptom would have had two halves** and the second
/// silent: the noisy half is the held key not repeating, the silent half is
/// the letter chosen from the popover going to the shell **twice** — that call
/// is `insertText:"é" replacementRange:{n-1,1}` and since
/// `view::BateriView` skips the range, `eé` is typed. What opens the popover
/// is `NSTextInputClient` itself, i.e. the same change that causes both
/// symptoms and whose only remedy is here.
fn disable_press_and_hold() {
    let key = ns_string!("ApplePressAndHoldEnabled");
    let off = NSNumber::numberWithBool(false);
    let defaults = NSDictionary::from_slices::<NSString>(&[key], &[off.as_ref()]);
    // SAFETY: the dictionary's key is an `NSString`, its value an `NSNumber`
    // that can enter a property list — the types `registerDefaults` wants.
    unsafe { NSUserDefaults::standardUserDefaults().registerDefaults(&defaults) };
}

/// The delegate's state — **app-wide**. Everything per-window (window, view,
/// surface, renderer, session, link, dock share, temporary point size) is in
/// [`TerminalWindow`]; here are the settings, watch sources, subtitle slots,
/// the measurement ledger, the timed run's recipe and the window list.
pub(crate) struct Ivars {
    /// The timed run's recipe; `None` → the user's own session. The deadline,
    /// the guard, the fixed shell and the report **all** depend on this together.
    run: Option<Run>,
    /// The timed run's measured pane: the one whose counters, `quiet=` and
    /// `teardown=` the report prints ([`AppDelegate::measured_pane`]). Set
    /// when it is born — the first window's pane under [`Workload::Load`],
    /// the second tab's under [`Workload::Smoke`]
    /// ([`AppDelegate::open_measured_tab`]). Held by identity because a
    /// smoke run has two tabs and "the first window's focused pane" would
    /// silently become whichever is selected at the deadline.
    measured: Cell<Option<u64>>,
    /// The smoke run asked for its measured tab — once, by whichever came
    /// first: the background tab's first content frame or the backstop
    /// ([`AppDelegate::open_measured_after_first_frame`]).
    measured_asked: Cell<bool>,
    /// The smoke run's background tab, from the moment it left the screen;
    /// `None` in every other run and before then.
    background: Cell<Option<Hidden>>,
    /// The subtitle's slots; written only by [`AppDelegate::post_notices`].
    /// App-wide, because their sources (settings, theme, font, write) are too:
    /// every window's subtitle shows the same text.
    notices: RefCell<Notices>,
    /// The current settings: [`AppDelegate::load_settings`] writes at startup,
    /// [`AppDelegate::reload_settings`] on save; the appearance applier reads
    /// for `theme_for`, Theme ▸ for the checked item. In a timed run the
    /// defaults, and the appearance applier never reads them (`Inputs::Hermetic`).
    ///
    /// Stored because the appearance change must know which theme to choose
    /// without rereading the file, and the live refresh takes its diff against
    /// this. An unusable save does **not change** it: the next appearance
    /// change chooses with the last good settings.
    settings: RefCell<Settings>,
    /// The settings directory's sources: the root, `themes/`, `settings.toml`
    /// ([`settings::watched_paths`]). Never set up in a timed run nor when the
    /// home directory cannot be resolved.
    config_watch: RefCell<Option<Watch>>,
    /// The active user theme's file. A separate slot, because its name also
    /// changes with the appearance and an appearance change does not reread
    /// the settings file; if the embedded theme is chosen there is no file and the slot has no source.
    theme_watch: RefCell<Option<Watch>>,
    /// The measurement ledger — `None` and never allocated when the gate is off.
    ///
    /// `bt-gpu`'s type but its owner is here: `DisplayLink` and the completion
    /// block each write a copy, and the one read at shutdown (the report) is this copy.
    stats: Option<Arc<Stats>>,
    /// The open windows, each carrying its tabs. **Owned here**: the window's
    /// delegate property is weak and `TerminalWindow` is held nowhere else.
    /// Windows are created by [`AppDelegate::open_window`], the restore, and
    /// the move of a tab to a window of its own
    /// ([`AppDelegate::move_tab_to_new_window`]); a closing window leaves one
    /// turn later ([`AppDelegate::forget_window`]).
    ///
    /// The on-save paths walk this list and, while walking, take a **copy** of
    /// it ([`AppDelegate::windows`]): a call going to a window can come back
    /// and reach here (`sync_geometry` → [`AppDelegate::post_notices`]).
    windows: RefCell<Vec<Retained<TerminalWindow>>>,
    /// The one id counter: windows, tabs and panes draw from it
    /// ([`TerminalWindow::id`], [`TerminalTab::id`], [`TerminalPane::id`] —
    /// one namespace); ids are not reused, so a stale message going to a
    /// closed window, tab or pane cannot find another.
    next_id: Cell<u64>,
    /// Was the last-seen system appearance dark — the gate of
    /// [`AppDelegate::apply_appearance`]. `None`: no change has arrived yet (the first news always passes).
    ///
    /// The gate is a **saving**, not a correctness requirement: the KVO news
    /// also arrives when the appearance's name changes (accent color, high
    /// contrast) and the theme depends only on the light/dark bit; if the bit
    /// is the same there is no reason to reread the theme file and repaint all windows.
    appearance_dark: Cell<Option<bool>>,
    /// The scroll bar's last applied resolved form — the gate of
    /// [`AppDelegate::apply_scrollbar`], the one "the resolved value changed"
    /// point the settings file and the system's preference both reach.
    /// `None`: nothing applied yet (the launch's call always passes).
    scrollbar: Cell<Option<ScrollbarMode>>,
    /// The settings window (bateri ▸ Settings…): born on first open, hidden
    /// when closed and lives for the whole process. **Not** a
    /// terminal window — it does not enter [`Ivars::windows`], i.e. ⌘Q's
    /// confirmation, settings propagation and tab jobs do not see it. Never born in a timed run.
    settings_window: RefCell<Option<Retained<SettingsWindow>>>,
    /// The settings file's state at its last read: the settings
    /// window's lock and line diagnostics come from here. Written at startup
    /// and at every live read, so even if the window opens later it sees the file's state.
    settings_state: RefCell<settings::FileState>,
    /// The Shell menu's delegate ([`crate::menu::install`]): the menu holds it
    /// weakly, this is what keeps it alive.
    shell_menu: OnceCell<Retained<ShellMenuDelegate>>,
    /// Sparkle's updater ([`crate::updater`]): "Check for Updates…" holds it
    /// weakly, this is what keeps it alive.
    /// Empty in an unbundled and timed run.
    updater: OnceCell<crate::updater::Updater>,
    /// bateri's ssh masters: one registry for every pane, so two jobs to
    /// one host open one master ([`PaneLaunch::masters`]). `None` in a timed run
    /// (no askpass, no master — the remote jobs take today's argv) and when the
    /// running binary's path is unknown (it is the askpass program).
    masters: Option<Arc<Masters>>,
    /// The session directory's lock, held from launch to the
    /// save at quit ([`AppDelegate::save_session`] takes it — the one-shot).
    /// `None`: a timed run, an unbundled process, or another instance of the
    /// same bundle holds it — this one neither restores nor saves.
    restore_lock: RefCell<Option<restore::Lock>>,
    /// What the update's holders gave at launch
    /// ([`arrive`] — taken before this delegate was born); consumed by the
    /// first windows ([`AppDelegate::restore_or_open`]).
    arrival: RefCell<Option<Arrival>>,
    /// The update's holder, spawned when the quit was found to be a
    /// relaunch ([`AppDelegate::terminate_reply`]) and given the panes in
    /// [`AppDelegate::shutdown`].
    holder: RefCell<Option<handover::Spawned>>,
    /// The bound holder's driver (`[terminal] keep_running`,
    /// [`crate::keeper`]): spawned at launch for `"crash"` and `"quit"`, on a
    /// live switch, and when one dies. `None` in a timed run, an unbundled
    /// process and without an ssh registry (its directories hold the socket).
    keeper: Option<Rc<Keeper>>,
    /// This quit hands the panes to the bound holder
    /// ([`AppDelegate::terminate_reply`] → [`AppDelegate::shutdown`]).
    hand_to_bound: Cell<bool>,
    /// bateri ▸ Quit and End Programs asked for the next quit — consumed by
    /// it ([`AppDelegate::terminate_reply`]), whatever it turns into.
    end_programs: Cell<bool>,
    /// The Mac is logging out, restarting or shutting down
    /// (`NSWorkspaceWillPowerOffNotification`): the programs end with it, so
    /// a quit leaves no reminder that they keep running. Never cleared — a
    /// logout cancelled after bateri saw it costs that reminder only.
    powering_off: Cell<bool>,
    /// The reminder this quit leaves once the programs are held
    /// ([`AppDelegate::leave_quit_notice`]); read before the freeze.
    quit_notice: RefCell<Option<window::Notice>>,
    /// The handover test item asked for this quit: bateri starts
    /// itself again once this process is gone.
    relaunch_after: Cell<bool>,
    /// Sparkle's install handler while the relaunch waits for the transfers
    /// and the password sheets to end ([`AppDelegate::postpone_update`]).
    postponed_update: RefCell<Option<RcBlock<dyn Fn()>>>,
    /// ⇧ was held when this launch began ([`shift_held_at_launch`]): the
    /// holders were not asked and nothing is restored.
    skip_restore: bool,
    /// The holders' directories whose attempt marker this launch counted
    /// ([`Arrival::marked`]): cleared once the launch settles
    /// ([`AppDelegate::settled`]) and at a clean quit.
    attempt_marks: RefCell<Vec<PathBuf>>,
    /// The launch settled: the layout goes to disk on its edges
    /// ([`AppDelegate::save_layout_later`]).
    layout_writer: Cell<bool>,
    /// A delayed layout write is in the main queue (at most one).
    layout_save_pending: Cell<bool>,
    /// The ⌘ watch of the tabs' key hints (`tab_bar::watch_command_key`):
    /// the monitor's token, kept for the process's lifetime.
    command_monitor: RefCell<Option<Retained<AnyObject>>>,
    /// ⌘ was last seen held alone: only then does a release or a key
    /// press walk the bars ([`AppDelegate::command_held`]).
    command_hinted: Cell<bool>,
    /// The source of the tab being carried between windows
    /// ([`tab_drag`](crate::tab_drag)), kept here from the session's start to a
    /// turn after its end: the window the drag began in may close first, and
    /// the session's own hold on its source is not relied on.
    tab_drag: RefCell<Option<Retained<TabDragSource>>>,
    /// Where the carried tab was let go on a bar — the window and the decision
    /// ([`tabs::landing`](crate::tabs::landing)) — until the session ends and
    /// [`AppDelegate::tab_drag_ended`] carries it out: the windows are not
    /// touched inside the drop.
    tab_drop: Cell<Option<(u64, Landing)>>,
}

define_class!(
    // SAFETY: NSObject subclassing carries no requirement; AppDelegate does not implement Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriAppDelegate"]
    #[ivars = Ivars]
    pub(crate) struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _n: &NSNotification) {
            let mtm = self.mtm();
            disable_press_and_hold();
            // macOS's own tabs are off: every window carries its tabs itself
            // (`window`'s header). Turned off before the menu exists, so AppKit
            // adds no Show Tab Bar / Show All Tabs to View and no tab items
            // to Window — they would act on a tab bar that is not there.
            NSWindow::setAllowsAutomaticWindowTabbing(false, mtm);
            // The updater comes **before** the menu: it is its item's target. A
            // timed run does not go out to the network and an update question must not cover the window.
            if self.ivars().run.is_none()
                && let Some(updater) = crate::updater::start()
            {
                let _ = self.ivars().updater.set(updater);
            }
            // The handover's test item: a defaults key, never read
            // in a timed run; the product's code names no bundle.
            let handover_test = self.ivars().run.is_none()
                && NSUserDefaults::standardUserDefaults()
                    .boolForKey(ns_string!("BateriHandoverTestMenu"));
            let shell_menu = crate::menu::install(
                mtm,
                ProtocolObject::from_ref(self),
                self.ivars().updater.get().map(|u| &*u.controller),
                handover_test,
            );
            let _ = self.ivars().shell_menu.set(shell_menu);
            // The settings are read **before** the first window: `scrollback` and
            // the theme enter `SessionOptions`, and the font setting also
            // determines the cell size, i.e. the first grid and the first
            // `TIOCSWINSZ` the shell sees. The window no longer needs to be born
            // first for the diagnostics to reach the subtitle: the new window takes
            // its subtitle over from the slots (`open_window`).
            self.load_settings();
            // Quit and End Programs is there only under `keep_running = "quit"`.
            crate::menu::set_end_programs_visible(mtm, self.settings().keep_running);
            // The preview cache's launch sweep and the daily one:
            // on their own thread and the main queue's timer, never the frame
            // path; a timed run never touches the user's cache.
            self.sweep_previews(Sweep::Launch);
            self.schedule_daily_sweep();
            // The bound holder before the first windows: a carried-on pane
            // registers with it before the holder it came from is
            // acknowledged ([`AppDelegate::restore_arrival`]).
            let bundled = NSBundle::mainBundle().bundleIdentifier().is_some();
            if keeper::wants_holder(
                self.settings().keep_running,
                self.ivars().run.is_some(),
                bundled,
            ) {
                self.start_keeper();
            }
            NSApplication::sharedApplication(mtm).activate();
            // `"quit"`'s reminder needs the permission to notify: asked now,
            // while bateri is in front, never at the quit itself.
            if self.ivars().run.is_none()
                && keeper::asks_notification_permission(None, self.settings().keep_running)
            {
                crate::uploader::request_notification_permission();
            }
            // The renderer is born with the window and its error
            // lands here. `didFinishLaunching` cannot return an error; a terminal
            // window without Metal or without a shell is an empty box, and formerly
            // the error `run` returned was printed in `main` with the same line and
            // the same exit code. **Only for the first window**: the error of
            // ⌘T/⌘N does not end the process ([`AppDelegate::open_window_or_report`]).
            //
            // The saved session comes back here if there is one; otherwise
            // — or if not a single window of it could be built — today's first
            // window ([`AppDelegate::restore_or_open`]).
            if let Err(e) = self.restore_or_open() {
                eprintln!("bateri: {e}");
                std::process::exit(1);
            }
            // The launch settles a moment after its windows are built
            // ([`AppDelegate::settled`]).
            self.settle_later();
            // The system's Reduce Motion notification is app-wide and once; the
            // window's first value descended to its own link in `start`.
            self.observe_reduce_motion();
            // ⌘ held alone shows the tabs' keys; one watch for every window.
            self.ivars()
                .command_monitor
                .replace(crate::tab_bar::watch_command_key());
            // The scroll bar's system preference likewise; the panes were born
            // with the resolved form (their grid's first `TIOCSWINSZ` sees it).
            self.observe_scroller_style();
            // The light/dark appearance is also app-wide and once; the first
            // window's theme was already chosen from the appearance (`open_window` → `resolve_theme`).
            self.observe_appearance();
            // A quit while the Mac logs out leaves no reminder.
            self.observe_power_off();

            if let Some(run) = self.ivars().run {
                match run.workload {
                    // One tab: measured from its birth, which was just now.
                    Workload::Load => {
                        let first = self.first_pane().map(|pane| pane.id());
                        self.ivars().measured.set(first);
                        self.arm_deadline(run);
                    }
                    // Two tabs: this one draws first, then goes behind the
                    // measured one ([`AppDelegate::open_measured_tab`]).
                    Workload::Smoke => self.open_measured_after_first_frame(),
                }
            }
        }

        /// After the last window closes the app **stays open**:
        /// macOS's multi-window app convention; the Dock icon and ⌘N open a new
        /// window.
        ///
        /// **In a timed run** `true` and this is a contract: if the smoke recipe
        /// ends before the deadline the report is printed from the `child_exit`
        /// → `terminate:` path (`ShellWake::child_exit`) and the window never
        /// leaves the list on that path; the app must still end with the single closed window.
        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn should_terminate_after_last_window(&self, _app: &NSApplication) -> bool {
            self.ivars().run.is_some()
        }

        /// The Dock icon was clicked. **If there is no window at all** a new
        /// window opens and AppKit's default is skipped; if there is a window
        /// (even minimized) the default stays — AppKit restores the minimized
        /// one, and opening a new one would hide the session the user minimized.
        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        fn should_handle_reopen(&self, _app: &NSApplication, _has_visible_windows: bool) -> bool {
            // No `return`: `define_class!` converts the body's last expression
            // to `Bool`, it does not convert an early `return`'s `bool`.
            //
            // The criterion is only the terminal window list: `has_visible_windows`
            // also counts a non-terminal window like the About panel and an open
            // panel would block the new window (found in code review); the list already
            // covers the minimized ones.
            let default = !self.ivars().windows.borrow().is_empty();
            if !default {
                self.open_window_or_report(None, Opening::Window);
            }
            default
        }

        /// `bateri://…` was opened (`open`, the browser, another app).
        /// URLs are processed in order, the last one comes to the front.
        ///
        /// **Security invariant: this path only focuses.** Any app can open the
        /// scheme; here not a single byte goes to the shell, no command runs, no
        /// window opens. Arms: `bateri://tab/<id>` and a live pane → its tab to
        /// the front and the keyboard to that pane ([`TerminalWindow::bring_to_front`]);
        /// a recognized but dead id →
        /// only the app to the front; every other form (`block/` included) → nothing.
        ///
        /// On a cold start the list is empty (the URL can arrive before
        /// `applicationDidFinishLaunching:`) and the arm is "dead id"; the first
        /// window opens once by its usual path.
        #[unsafe(method(application:openURLs:))]
        fn open_urls(&self, _app: &NSApplication, urls: &NSArray<NSURL>) {
            for url in urls {
                let Some(text) = url.absoluteString() else {
                    continue;
                };
                let Some(id) = TabId::from_url(&text.to_string()) else {
                    continue;
                };
                match self.pane_by_tab(&id) {
                    Some((window, tab, pane)) => window.bring_to_front(&tab, &pane),
                    None => NSApplication::sharedApplication(self.mtm()).activate(),
                }
            }
        }

        /// ⌘Q, Dock ▸ Quit, logout and restart: should it ask before quitting.
        /// The question is **one** alert for all windows;
        /// `runModal` is synchronous, i.e. the answer returns directly and
        /// `NSTerminateLater` is not needed.
        ///
        /// A timed run passes **on the first line** without touching the process
        /// table: the shell's `exit` arrives here through `child_exit` →
        /// `terminate:` and a headless `runModal` would hang before the guard is set up.
        #[unsafe(method(applicationShouldTerminate:))]
        fn should_terminate(&self, _app: &NSApplication) -> NSApplicationTerminateReply {
            self.terminate_reply()
        }

        /// AppKit's shutdown path: bateri ▸ Quit (Cmd-Q, `terminate:` from the
        /// menu) and in a timed run the shell that writes `exit` (`child_exit` →
        /// `terminate:`) arrive here; in an interactive session the red button
        /// and `exit` close only that window (`TerminalWindow`'s `windowWillClose:`).
        /// With a running job Cmd-Q asks first (`applicationShouldTerminate:`);
        /// if we arrived here the decision has been made.
        /// The smoke deadline does not come by here, `terminate:` always exits
        /// with 0 and `runDeadline:` must be able to fall red. What is shared is
        /// not the notification but the order: both paths call [`AppDelegate::shutdown`]
        /// and every step to be added to shutdown is added there.
        #[unsafe(method(applicationWillTerminate:))]
        fn will_terminate(&self, _n: &NSNotification) {
            // The stamp **before** shutdown, for the reason in `runDeadline:`:
            // `shutdown()` can wait up to half a second and if that wait is
            // written into the quiet, the token does not measure what it thinks it measures.
            //
            // The gate is asked **before reading**: `quiet_since` is a clock read
            // (`CACurrentMediaTime`) and in an untimed run this value will be
            // discarded. "Not even a single clock read when the gate is off"
            // holds on the Cmd-Q path too; the `if let` below alone
            // discarded the value but did not prevent the read.
            let quiet = self
                .ivars()
                .run
                .is_some()
                .then(|| self.quiet_since())
                .flatten();
            let teardown = self.shutdown();
            // A smoke run can also end before reaching the deadline: if the shell
            // exits by itself (`BT_RUN_SECONDS` longer than the script's sleep, or
            // a real shell dying at once) `ChildExit` brings it here. Exiting
            // without printing the report would show a run that measured nothing
            // as green with exit 0 to `make smoke` — this was the only way the
            // gate gave a false green.
            if let Some(run) = self.ivars().run {
                self.report_and_exit(run, teardown, quiet);
            }
        }
    }

    unsafe impl NSMenuDelegate for AppDelegate {
        /// Theme ▸ is opening — the delegate is tied only to it (`menu::install`).
        /// The list is built at that moment: a file dropped into `themes/` shows
        /// at the next opening, the directory is not watched for the list. The
        /// checked item is the current setting's `theme`.
        ///
        /// In a timed run and when the home directory cannot be resolved it is
        /// not filled ([`Inputs`]): there is no file for the choice to write.
        #[unsafe(method(menuNeedsUpdate:))]
        fn menu_needs_update(&self, menu: &NSMenu) {
            let Inputs::User {
                config_root: Some(root),
            } = self.inputs()
            else {
                return;
            };
            let embedded: Vec<&str> = Theme::embedded_names().collect();
            let user = settings::user_theme_names(&root);
            let settings = self.ivars().settings.borrow();
            crate::menu::fill_themes(self.mtm(), menu, &settings.theme, &embedded, &user);
        }

        /// "Does this menu have a counterpart for this key": no, the theme items
        /// have no shortcut.
        ///
        /// The only reason to define it is cost: if the delegate does not define
        /// it, AppKit fills the menu with `menuNeedsUpdate:` at every Command key
        /// (Cmd-C included) to look for a counterpart — `themes/` would be read
        /// on every key. `objc2-app-kit` does not generate this method (arguments
        /// with pointer returns); the signature is by hand. The two out arguments
        /// (`id *`, `SEL *`) are opaque pointers: `Sel` carries no pointer
        /// encoding and a method that returns `false` never writes to them.
        #[unsafe(method(menuHasKeyEquivalent:forEvent:target:action:))]
        fn menu_has_key_equivalent(
            &self,
            _menu: &NSMenu,
            _event: &NSEvent,
            _target: *mut c_void,
            _action: *mut c_void,
        ) -> bool {
            false
        }
    }

    impl AppDelegate {
        /// KVO: `NSApp.effectiveAppearance` changed — the system's light/dark
        /// appearance ([`AppDelegate::observe_appearance`]). This is the only key
        /// path this class observes, so the path and object are not asked.
        #[unsafe(method(observeValueForKeyPath:ofObject:change:context:))]
        fn observe_value(
            &self,
            _key_path: Option<&NSString>,
            _object: Option<&AnyObject>,
            _change: Option<&AnyObject>,
            _context: *mut c_void,
        ) {
            self.apply_appearance();
        }

        /// A watch source gave notice (`notify_settings_changed`, targetless
        /// action): the settings or theme file was saved.
        #[unsafe(method(settingsDidChange:))]
        fn settings_did_change(&self, _sender: Option<&AnyObject>) {
            self.reload_settings();
        }

        /// macOS's accessibility display settings changed; the sender is
        /// `NSWorkspace`'s **own** notification center ([`AppDelegate::observe_reduce_motion`]).
        ///
        /// The notification is not specific to Reduce Motion — contrast,
        /// transparency and color differentiation come from here too. There is
        /// no need to tell them apart: the path below rereads the value and if
        /// it did not change the call going to the link is a no-op anyway
        /// (`Motion::set_reduce`).
        #[unsafe(method(accessibilityDisplayDidChange:))]
        fn accessibility_display_did_change(&self, _note: Option<&AnyObject>) {
            // audit: this path does not **structurally** guarantee the main
            // thread — the `NSNotificationCenter` observer fires synchronously on
            // the posting thread and `NSWorkspace`'s center does not make this a
            // contract (an `/audit` finding). The work underneath assumes the main
            // thread: `settings`'s `RefCell` and the link's `Cell<Motion>`. So
            // the claim stands in the code — if wrong, the symptom is a panic that
            // blows up here, not a silent data race.
            let _mtm = MainThreadMarker::new()
                .expect("accessibility notification expected on the main thread");
            self.apply_reduce_motion();
            // The settings window's motion rows look at the system's answer
            // (`settings_window::motion_override`): if open it must refresh too,
            // otherwise Reduce Motion turned on from the system would not look like it overrides the rows.
            self.refresh_settings_window();
        }

        /// macOS's "Show scroll bars" changed — or its automatic choice did,
        /// a mouse plugged in or out; the sender is the **default** centre
        /// ([`AppDelegate::observe_scroller_style`]). The path rereads the
        /// preference and does nothing if the resolved form stayed the same.
        ///
        /// The centre delivers on the posting thread and Apple documents no
        /// thread for this notification, while the work underneath wants the
        /// main one (the settings' `RefCell`, the panes' geometry). Off the
        /// main thread the change **hops** there rather than being refused —
        /// a panic here could not unwind out of an Objective-C method and
        /// would abort every window.
        #[unsafe(method(preferredScrollerStyleDidChange:))]
        fn preferred_scroller_style_did_change(&self, _note: Option<&AnyObject>) {
            if MainThreadMarker::new().is_some() {
                self.scroller_style_changed();
                return;
            }
            DispatchQueue::main().exec_async(|| {
                if let Some(mtm) = MainThreadMarker::new()
                    && let Some(app) = delegate(mtm)
                {
                    app.scroller_style_changed();
                }
            });
        }

        /// bateri ▸ Quit and End Programs (⌥⌘Q, only under `keep_running =
        /// "quit"`): today's quit — its question, the programs end, the
        /// bound holder leaves quietly, no reminder.
        #[unsafe(method(quitAndEndPrograms:))]
        fn quit_and_end_programs(&self, _sender: Option<&AnyObject>) {
            self.ivars().end_programs.set(true);
            NSApplication::sharedApplication(self.mtm()).terminate(None);
        }

        /// The Mac is logging out, restarting or shutting down; the sender is
        /// `NSWorkspace`'s own notification center
        /// ([`AppDelegate::observe_power_off`]).
        #[unsafe(method(workspaceWillPowerOff:))]
        fn workspace_will_power_off(&self, _note: Option<&AnyObject>) {
            self.ivars().powering_off.set(true);
        }

        /// The handover's test item (the defaults key
        /// `BateriHandoverTestMenu`): the update's quit without Sparkle — the
        /// relaunch flag, the quit, and bateri starting itself again once this
        /// process is gone ([`AppDelegate::spawn_relauncher`]).
        #[unsafe(method(relaunchWithHandover:))]
        fn relaunch_with_handover(&self, _sender: Option<&AnyObject>) {
            crate::updater::request_relaunch();
            self.ivars().relaunch_after.set(true);
            NSApplication::sharedApplication(self.mtm()).terminate(None);
        }

        /// Shell ▸ New Window (⌘N): a new window in the active window's
        /// directory and with its point-size delta. Here, not
        /// in the window: it must work when there is no window too.
        #[unsafe(method(newWindow:))]
        fn new_window(&self, _sender: Option<&AnyObject>) {
            self.open_from_key_window(Opening::Window);
        }

        /// Shell ▸ Close Tab (⌘W) while a non-terminal window is key (the About
        /// panel): the responder chain brings it here and that window closes by
        /// AppKit's own path. In a terminal window the window's delegate
        /// answers the action first (`TerminalWindow`'s `closeTab:`) — so that
        /// once the menu is separated from `performClose:` ⌘W does not silently
        /// die in panels (found in code review).
        #[unsafe(method(closeTab:))]
        fn close_tab(&self, _sender: Option<&AnyObject>) {
            if let Some(key) = NSApplication::sharedApplication(self.mtm()).keyWindow() {
                key.performClose(None);
            }
        }

        /// The title of ⌘W while a non-terminal window is key: so the "Close"
        /// that a split tab left behind (`TerminalWindow`'s
        /// `validateMenuItem:`) does not stay in the panel. **An
        /// unknown item is `true`** — the behavior before it was defined.
        #[unsafe(method(validateMenuItem:))]
        fn validate_menu_item(&self, item: &NSMenuItem) -> bool {
            if item.action() == Some(sel!(closeTab:)) {
                item.setTitle(&NSString::from_str(window::close_title(1)));
            }

            // Shell ▸ Shell Integration on “{host}”: the key tab's host
            // and its resolved answer; locally grey.
            if item.action() == Some(sel!(toggleHostIntegration:)) {
                let remote = self.key_remote_mark().map(|(host, _)| {
                    let on = self.settings().integration_for(&host);
                    (host, on)
                });
                let model = crate::menu::integration_menu(
                    remote.as_ref().map(|(host, on)| (host.as_str(), *on)),
                );
                item.setTitle(&NSString::from_str(&model.title));
                item.setState(if model.checked {
                    NSControlStateValueOn
                } else {
                    NSControlStateValueOff
                });
                model.enabled
            } else if item.action() == Some(sel!(mergeWindows:)) {
                // Merging needs a second window to take tabs from.
                self.windows().len() > 1
            } else {
                true
            }
        }

        /// Shell ▸ New Tab (⌘T): a new tab in the active window, right of the
        /// selected one; a new window if there is no window. If the active tab
        /// is remote the new tab is born with the same ssh/mosh command
        /// ([`initial_line`]).
        #[unsafe(method(newTab:))]
        fn new_tab(&self, _sender: Option<&AnyObject>) {
            self.open_from_key_window(Opening::Tab);
        }

        /// Window ▸ Merge All Windows: every other window's tabs join the key
        /// window's, at its end; the emptied windows close without ending a
        /// shell. Here, not in the window: it reaches all of them.
        #[unsafe(method(mergeWindows:))]
        fn merge_windows(&self, _sender: Option<&AnyObject>) {
            self.merge_all_windows();
        }

        /// Shell ▸ New Local Tab (⌥⌘T): **always** a local tab, even from a
        /// remote tab — ⌘T's escape route; in a local tab the same
        /// as ⌘T.
        #[unsafe(method(newLocalTab:))]
        fn new_local_tab(&self, _sender: Option<&AnyObject>) {
            self.open_from_key_window(Opening::LocalTab);
        }

        /// bateri ▸ Settings… (Cmd-,), from the targetless menu item (`menu`):
        /// opens the settings window or brings it to the front.
        #[unsafe(method(openSettings:))]
        fn open_settings(&self, _sender: Option<&AnyObject>) {
            self.show_settings_window();
        }

        /// View ▸ Theme ▸ {name}: the item's title is the theme's name
        /// (`menu::fill_themes`).
        #[unsafe(method(selectTheme:))]
        fn select_theme(&self, sender: Option<&AnyObject>) {
            let Some(item) = sender.and_then(|sender| sender.downcast_ref::<NSMenuItem>()) else {
                return;
            };
            self.save_theme(&item.title().to_string());
        }

        /// Shell ▸ Mark “{host}” as ▸ {mark}: the item's `tag` is
        /// the mark ([`crate::menu::mark_of_tag`]), the host is the active tab's
        /// remote host or its database client's server
        /// ([`AppDelegate::key_mark_target`]). The menu only **writes** — the
        /// path that reads the file applies ([`AppDelegate::save_edit`], the
        /// Theme ▸ precedent); nothing is written to an unparseable file, the
        /// diagnostic goes to the write slot. A no-op if the tab has no such
        /// host any more.
        #[unsafe(method(markHost:))]
        fn mark_host(&self, sender: Option<&AnyObject>) {
            let Some(mark) = sender
                .and_then(|sender| sender.downcast_ref::<NSMenuItem>())
                .and_then(|item| crate::menu::mark_of_tag(item.tag()))
            else {
                return;
            };
            if let Some((host, _, subject)) = self.key_mark_target() {
                self.save_edit(&SettingsEdit::RemoteHostMark {
                    host,
                    mark,
                    subject,
                });
            }
        }

        /// Shell ▸ Shell Integration on “{host}”: writes the opposite of
        /// the host's resolved answer as the host's own `[remote] hosts` entry
        /// (`SettingsEdit::RemoteHostIntegration`) — the Mark … as ▸ path: the
        /// menu only writes, nothing is written to an unparseable file, and the
        /// next `ssh` reads the file. A no-op if the tab became local.
        ///
        /// It also forgets the server's `plain` row
        /// (`ssh_wrap::forget_plain`), the one way back for a server branded
        /// shell-less by mistake — which the check mark cannot show (it is the
        /// setting's; the row's key is `ssh -G`'s, too slow for validation).
        /// So a click on a **checked** item asks first: when a `plain` row was
        /// there, forgetting it is the click's whole effect and the setting
        /// stays on (the next `ssh` is wrapped); only without one is the
        /// integration turned off. Turning it on writes at once and forgets.
        #[unsafe(method(toggleHostIntegration:))]
        fn toggle_host_integration(&self, _sender: Option<&AnyObject>) {
            if let Some((host, _)) = self.key_remote_mark() {
                if self.settings().integration_for(&host) {
                    self.forget_plain(Some(host));
                } else {
                    self.save_edit(&SettingsEdit::RemoteHostIntegration { host, on: true });
                    self.forget_plain(None);
                }
            }
        }

        /// View ▸ Theme ▸ Match System.
        #[unsafe(method(matchSystemTheme:))]
        fn match_system_theme(&self, _sender: Option<&AnyObject>) {
            self.save_theme(SYSTEM_THEME);
        }

        #[unsafe(method(runDeadline:))]
        fn run_deadline(&self, _arg: Option<&AnyObject>) {
            // The quiet stamp is read **before** shutdown and the order is
            // deliberate: `shutdown()` waits at most `SHUTDOWN_GRACE` (half a
            // second) and in a quarter of the measurement runs it really
            // waits (`teardown=abandoned`). Had it been read after, `quiet=`
            // would be "last frame → end of shutdown" instead of "last frame →
            // deadline" and the shutdown's variability would mix into the gate's
            // floor ([`QUIET_FLOOR`] stands at half of the measured distribution;
            // a half-second shutdown wait alone would eat that margin).
            let quiet = self.quiet_since();
            let teardown = self.shutdown();
            // The timer was set up only while `run` is filled; the `if let` here
            // is not a branch but the reading of that invariant. It is not an
            // `expect`, because this is the report path and a panic at shutdown would swallow the report itself.
            if let Some(run) = self.ivars().run {
                self.report_and_exit(run, teardown, quiet);
            }
        }
    }
);

/// Counters of the smoke **gate** — not the whole line, only as much as
/// [`verdict`] sees.
///
/// A struct, because they are all numbers: if passed positionally, when
/// `cells` and `glyphs` swapped places **it would compile** and since the test
/// uses the same order the two would be wrong together (a code review finding).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Counters {
    /// Frames the GPU finished without error.
    frames: u64,
    /// Content frames **decided to be drawn**: the operand of the zero-idle-frame
    /// gate ([`IDLE_FRAME_LIMIT`]). It does not replace `frames`, it comes beside
    /// it — the two answer separate questions and the token prints both.
    content: u64,
    /// Background cells the sink produced in the last frame (cursor excluded).
    cells: usize,
    /// Glyphs drawn in the last frame.
    glyphs: usize,
    /// Underlines / strikeouts drawn in the last frame.
    rules: usize,
    /// Frames drawn because of an unsettled **cursor** animation — not the twin
    /// of `slide` but its complement: only the offset raises the other, only
    /// the cursor raises this and both can rise in one frame.
    ///
    /// `content`'s sibling and in the **opposite direction** at the gate:
    /// `content` has an upper bound, this has a **lower** bound (`> 0`). The
    /// smoke recipe contains a cursor movement (`bt_core::smoke_shell`), so zero
    /// means "the animation never ran" — just as `cells=0` means "no shell output".
    ///
    /// **The hidden tie, now by name:** this requirement depends on the
    /// hermetic run's cursor style being **animated** and that style comes from
    /// `bt_core::Settings::default().cursor_motion`, i.e. from the defaults'
    /// single owner (a timed run does not read the settings file,
    /// [`Inputs::Hermetic`]). If the default one day becomes `CursorMotion::Snap`
    /// this gate silently falls — that change must either pin the hermetic
    /// run's style explicitly in the run or will find this sentence facing it.
    ///
    /// **It does not count blink**: the cursor's blinking lives
    /// outside `bt_gpu::motion`, i.e. `cursor_settled()` never sees it and this
    /// counter does not rise. A blink frame has **no CPU witness at all** —
    /// `requests=` does not rise (`Waker::resume` does not touch the counter),
    /// nor `content=` (that is the design's purpose). The token was
    /// **deliberately not added** (the `cpu_elenen=` precedent, below): since
    /// the default is off it would print zero in every observable run and a
    /// token is not deleted, it is added. No tier of the gate sees a broken
    /// blink; **the protection is not a token but the default itself** and this is written in the set's `teslim.md`.
    motion: u64,
    /// Frames drawn because of an unsettled **slide** (the content's offset).
    ///
    /// `motion`'s sibling and **not in the gate**: the recipe's cursor movement
    /// is guaranteed (`bt_core::smoke_shell`) but whether the slide will be
    /// born there was not measured and an unmeasured number is not written into
    /// the gate. It is in the line for diagnosis: in a red run, read together
    /// with `motion`, it tells which animator did not settle.
    ///
    /// The two are not added up to **give** the drawn frame: both can rise in
    /// the same frame.
    slide: u64,
}

/// The animation's state at the deadline — the gate's half that **needs no measurement**.
///
/// Not a `bool` and the reason is the call site: [`verdict`] already takes
/// five numbers and a bare `true` would not say which question it answered.
/// Not [`Counters`] either, because this is not a number but a **state**: an
/// unsettled animation turns the run red, its existence matters, not its count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MotionState {
    Settled,
    Unsettled,
}

/// The smoke run's background tab as it left the screen
/// ([`AppDelegate::open_measured_tab`]) — the baseline [`Background`] is
/// counted from.
#[derive(Clone, Copy, Debug)]
struct Hidden {
    /// Its pane (the recipe's tab has one).
    pane: u64,
    /// Its link's main-thread frames then ([`drawn_frames`]).
    frames: u64,
    /// Its link's damage notices then ([`DisplayLink::requests`]).
    wakes: u64,
}

/// What the smoke run's background tab did while hidden, from the moment it
/// left the screen to the deadline — the `back=` and `back_wakes=` tokens.
///
/// **Two numbers, two gates, and neither means anything alone:** `frames`
/// must be zero (a hidden tab draws nothing), `wakes` must not be — without
/// a damage notice arriving while it was hidden, a zero would only say that
/// nothing asked it to draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Background {
    /// Content, motion and slide frames — the **main-thread** counters, decided
    /// frames. Not the GPU's finished `frames`: a frame legitimately in flight
    /// as the tab hid finishes afterwards and would read as a hidden frame.
    frames: u64,
    /// Damage notices ([`bt_gpu::Waker::wake`]'s count, which rises before the
    /// visibility gate) — of **any** source: output, a resize's request.
    /// Each is a request a visible tab would have drawn; the one the recipe
    /// guarantees is its second print, and it counts only if it lands after
    /// the baseline, taken once the measured tab is born — a print that
    /// lands earlier makes a false **red**, never a false green.
    wakes: u64,
}

/// A link's main-thread frame counters summed: content, motion and slide —
/// the frames it **decided** to draw ([`Background::frames`]). A frame both
/// animators kept alive counts twice; the gate asks only whether it is zero.
fn drawn_frames(link: &DisplayLink) -> u64 {
    link.content_frames() + link.motion_frames() + link.slide_frames()
}

/// The teardown the report gets ([`AppDelegate::shutdown`]), from every
/// pane's `(id, result)`: the `measured` pane's (the first pane's when there
/// is none) — unless a pane's panicked, and then the first such: a panic must
/// not pass the gate because it happened in the smoke run's background tab,
/// which is not measured.
fn reported_teardown(
    results: &[(u64, Option<Teardown>)],
    measured: Option<u64>,
) -> Option<Teardown> {
    let teardowns = || results.iter().map(|(_, teardown)| *teardown);
    teardowns()
        .find(|teardown| panic_site(*teardown).is_some())
        .or_else(|| match measured {
            Some(measured) => results
                .iter()
                .find(|(pane, _)| *pane == measured)
                .map(|(_, teardown)| *teardown),
            None => teardowns().next(),
        })
        .flatten()
}

/// Where a teardown panicked; `None` if it did not. The gate's
/// [`Verdict::ShutdownPanicked`] and the choice of which pane's teardown the
/// report gets ([`AppDelegate::shutdown`]) read the same answer.
fn panic_site(teardown: Option<Teardown>) -> Option<&'static str> {
    match teardown {
        Some(Teardown::ReaderPanicked) => Some("reader thread"),
        Some(Teardown::Panicked) => Some("teardown thread"),
        _ => None,
    }
}

/// The measurement ledger's summary at shutdown: read from the ring, not yet formatted.
///
/// The counter half (`samples`, `dropped`, `discarded`) is read **before** p95, because
/// [`bt_gpu::Samples::p95_and_worst`] consumes itself — the order is forced by
/// the type, not by the comment.
///
/// # The honest limits of the measurement
///
/// The field docs below state the **scope** limit that falls to their own field.
/// The GPU column is not stable enough to be a floor.
///
struct Measured {
    /// From `main()`'s first line to the first **completed** frame. `None` → no
    /// frame finished; the limit of the two ends is in [`bt_gpu::Stats::startup`].
    startup: Option<Duration>,
    /// The length of the CPU columns. A single number, because the two CPU
    /// columns are written together (`Stats::record_cpu`) and their alignment is bound in the type.
    cpu_samples: usize,
    /// Samples that did not fit the ring and dropped — the highest of the three columns.
    ///
    /// Its rule and rationale are beside the ring ([`bt_gpu::Samples::dropped`]);
    /// here it is only applied, and from the **snapshots in hand**: a fresh
    /// read would not take `samples=` and `dropped=` from the same instant.
    dropped: u64,
    /// The GPU column's length — **can be shorter** than the CPU's.
    gpu_samples: usize,
    /// Frames that could not be written at all because of Metal's zero/NaN
    /// stamp. Without this number an empty GPU column could not be told apart
    /// from "the hardware gives no stamp" and "no frame was drawn".
    gpu_rejected: u64,
    cpu_frame: Option<(Duration, Duration)>,
    cpu_encode: Option<(Duration, Duration)>,
    gpu: Option<(Duration, Duration)>,
    /// The adapter gives GPU timestamps (wgpu's `TIMESTAMP_QUERY`);
    /// `false` → the GPU column's tokens say `unsupported` — the
    /// keys stay (a token is never deleted), the value names the absence.
    gpu_supported: bool,
}

impl Measured {
    /// Reads the ledger. Runs once, at teardown.
    ///
    /// `link.stop()` has already been called, but **not every ring is quiescent**:
    /// this thread writes the two CPU columns (so those are quiescent), while
    /// Metal's completion thread writes the GPU column and a frame still in
    /// flight can land while the report is being read. The result is a one-sample
    /// drift: `frames` and `gpu_samples + gpu_discarded` can therefore differ by
    /// up to one. The `samples=` token makes this visible; whoever interprets the
    /// number must not expect equality.
    fn read(stats: &Stats, gpu_supported: bool) -> Self {
        let cpu_frame = stats.cpu_frame();
        let cpu_encode = stats.cpu_encode();
        let gpu = stats.gpu();
        Self {
            startup: stats.startup(),
            cpu_samples: cpu_frame.nanos.len(),
            dropped: cpu_frame.dropped.max(cpu_encode.dropped).max(gpu.dropped),
            gpu_samples: gpu.nanos.len(),
            gpu_rejected: gpu.rejected,
            cpu_frame: cpu_frame.p95_and_worst(),
            cpu_encode: cpu_encode.p95_and_worst(),
            gpu: gpu.p95_and_worst(),
            gpu_supported,
        }
    }
}

/// The **whole** input of the success line.
///
/// The function that builds the line had to be pure: so it can be tested
/// without a real window and display link. Had it been passed as six separate
/// arguments, it would repeat, one layer up, the mistake `Counters` avoided.
struct Report {
    counters: Counters,
    /// Used/total slots of the atlas's **mask** plane. A counter, **not** a
    /// gate.
    atlas: (usize, usize),
    /// Used/total slots of the atlas's **colour** plane; the reason for having a
    /// second token (`slots2=`) is in the doc of `Atlas::color_occupancy`.
    /// The token was **added, not deleted**: `slots=` stays and its meaning is unchanged.
    color_atlas: (usize, usize),
    workload: Workload,
    /// Frames requested over the run — not drawn ones.
    requests: u64,
    /// Time between the last drawn frame and the deadline; `None` → no frame
    /// was drawn (`quiet=none`). **Gate** ([`QUIET_FLOOR`]): in the smoke load,
    /// both a value below the floor and `None` are red.
    quiet: Option<Duration>,
    /// Outcome of the teardown; `None` → the session was never born.
    teardown: Option<Teardown>,
    /// Measurement ledger; `None` → the gate was closed (`BT_FRAME_STATS` not given).
    measured: Option<Measured>,
    /// The smoke run's background tab while hidden; `None` → there was none
    /// (the measurement load's single tab, or a measured tab that never
    /// opened). **Gate** in the smoke load ([`Verdict::BackgroundUnwoken`],
    /// [`Verdict::BackgroundDrew`]).
    background: Option<Background>,
}

impl Report {
    /// The success line — **pure**, i.e. testable without a real window.
    ///
    /// Token contract: **never deleted, only added.** The old five (`frames`,
    /// `cells`, `glyphs`, `rules`, `slots`) plus `load` and `pipeline=ok` stay in
    /// place; the new ones go in between.
    ///
    /// **Language rule, in one place:** the *keys* are English; the contract is
    /// still "never delete a token, only add". The keys were renamed once, from
    /// Turkish to English (2026-10-01). The *values* are English too, because what
    /// reads them is not a diagnostic text but a `match` arm or a CI grep
    /// (`load=smoke|load` and `pipeline=ok` had set this pattern before this line
    /// existed). The only place still in Turkish is **diagnostic text**: stderr lines and `assert!` messages.
    ///
    /// The line is printed **explicitly** (a `println!` in `report_and_exit`);
    /// no path is left to a buffer that would be flushed in `Drop` —
    /// `process::exit` runs no `Drop`, and the guard's `_exit(70)` skips even
    /// atexit.
    fn token_line(&self) -> String {
        let Counters {
            frames,
            content,
            cells,
            glyphs,
            rules,
            motion,
            slide,
        } = self.counters;
        let (used, total) = self.atlas;
        let (color_used, color_total) = self.color_atlas;
        // `profile=` is printed even with the gate closed: `make smoke` runs
        // **debug**, measurement demands **release**, and mistaking a debug number
        // for a floor becomes impossible only if the line itself states its profile.
        let profile = if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        };
        // The `content`/`motion`/`slide`/`quiet` quartet sits next to `requests=`:
        // all of them are frame **accounting** and whoever reads the line wants
        // them together. `slide` sits next to `motion`, because both answer the
        // same question for two animators. The first four counters must stay in
        // place (`smoke_counts_unchanged`).
        let mut line = format!(
            "frames={frames} cells={cells} glyphs={glyphs} rules={rules} \
slots={used}/{total} slots2={color_used}/{color_total} load={workload} \
requests={requests} content={content} \
motion={motion} slide={slide} quiet={quiet} teardown={teardown} \
profile={profile}",
            workload = self.workload.token(),
            requests = self.requests,
            // **Not `quiet=0`:** zero would mean "frames were flowing at the
            // deadline" and would be confused with a run where no frame was drawn
            // — the same rule as `samples=off`: the absence's own word instead of
            // an invented number.
            quiet = self.quiet.map_or_else(|| "none".to_owned(), ms),
            teardown = teardown_token(self.teardown),
        );
        match &self.measured {
            // The gate was closed. **Not `samples=0`:** zero would look the same as
            // "the gate was open but no samples were collected", and that is exactly
            // the blindness this closes. The measurement tokens are not
            // printed at all either; the contract allows reading a token's absence,
            // not a false value.
            None => line.push_str(" samples=off"),
            Some(m) => {
                // `write!` cannot return an error on a `String`; `let _` makes that
                // visible and leaves no `unwrap` in the report path.
                let _ = write!(
                    line,
                    " samples={} dropped={} gpu_samples={} gpu_discarded={} floor={MIN_SAMPLES}",
                    m.cpu_samples, m.dropped, m.gpu_samples, m.gpu_rejected
                );
                push_span(&mut line, "cpu_frame", m.cpu_frame);
                push_span(&mut line, "cpu_encode", m.cpu_encode);
                if m.gpu_supported {
                    push_span(&mut line, "gpu", m.gpu);
                } else {
                    line.push_str(" gpu_p95=unsupported gpu_max=unsupported");
                }
                // Startup is a single number, not a distribution: it happens once per run.
                let _ = match m.startup {
                    Some(startup) => write!(line, " startup={}", ms(startup)),
                    None => write!(line, " startup=none"),
                };
            }
        }
        // The background tab's pair, last before `pipeline=ok`, so the line's
        // start and the `slots=`/`slots2=` neighbours stay where they were.
        // Without a background tab both say `none` — the `quiet=none` rule:
        // `back=0` would read as "it stayed dark", which nothing measured.
        let _ = match self.background {
            Some(Background { frames, wakes }) => {
                write!(line, " back={frames} back_wakes={wakes}")
            }
            None => write!(line, " back=none back_wakes=none"),
        };
        line.push_str(" pipeline=ok");
        line
    }
}

/// The two tokens of one column.
///
/// Below the floor there is **no** number: `insufficient` is printed and
/// the reason can be read from the `samples=`/`gpu_samples=` and `floor=` pair on
/// the same line. The two fall silent **together**, because both come from the
/// same `Option`: below the floor the p95 is just a copy of the worst anyway, so
/// what would be printed is not two numbers but one number and two names.
fn push_span(line: &mut String, name: &str, span: Option<(Duration, Duration)>) {
    let _ = match span {
        Some((p95, worst)) => write!(line, " {name}_p95={} {name}_max={}", ms(p95), ms(worst)),
        None => write!(line, " {name}_p95=insufficient {name}_max=insufficient"),
    };
}

/// Turns a duration into a token value: milliseconds with two decimals.
///
/// One format, `startup=` included. Two separate precisions would force the
/// reader to memorise a rule per token; the exact opposite of what a machine contract wants.
fn ms(value: Duration) -> String {
    format!("{:.2}ms", value.as_secs_f64() * 1e3)
}

/// The **diagnostic** form of the quiet time: not a token, a phrase read inside a sentence.
///
/// The token line is printed only on a green run ([`Report::token_line`]), so
/// the `quiet` of a failing run was visible nowhere. The `quiet ≥ T` gate, on
/// the other hand, derives from **two** distributions and the second is
/// exactly the failing runs: had a deliberately broken arm not printed its
/// `quiet`, `T` would be derived from one side only, i.e. its lower bound would be an unmeasured number.
///
/// The reason this is a separate function is the token contract: the line's
/// `quiet=` is read by machines and this phrase must **not resemble** it — a
/// CI step grepping for `quiet=` must not read a number from a failing run.
fn quiet_phrase(quiet: Option<Duration>) -> String {
    quiet.map_or_else(
        || "no frames drawn".to_owned(),
        |q| format!("{} of quiet after the last frame", ms(q)),
    )
}

/// Value of the `teardown=` token.
///
/// A run that hit the bound and a reader that ended in a panic used to pass with
/// a **green line**: there was a line on stderr, but no trace in the token.
/// Each outcome is a separate word, because each is a separate fault — with a
/// `bool`, the reader would have to look outside the line to find out which one.
///
/// The values are English and the `kebab-case` form of the variant name; the
/// rule's rationale is in one place, in the doc of [`Report::token_line`].
fn teardown_token(teardown: Option<Teardown>) -> &'static str {
    match teardown {
        // The session was never born: there was nothing to tear down either.
        None => "none",
        Some(Teardown::Clean) => "clean",
        Some(Teardown::ReaderPanicked) => "reader-panicked",
        Some(Teardown::Abandoned) => "abandoned",
        Some(Teardown::Panicked) => "panicked",
        Some(Teardown::Unbounded) => "unbounded",
        Some(Teardown::AlreadyDone) => "already-done",
        Some(Teardown::HungUp) => "hung-up",
    }
}

/// The smoke gate's verdict.
///
/// **Not** a `bool`: the failure path has two separate messages and a `bool`
/// would force re-deriving them outside the gate. The policy would then be
/// scattered over three places (success line, "zero" message, "excess" message)
/// and only one of them would be tested — a run that describes the wrong
/// fault as the gate fails is born exactly like that.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Verdict {
    Pass,
    /// One of the counters is zero: a link of the pipeline never ran.
    /// `required` is the requirement half of the message and varies with the load — `Load`
    /// streams plain text, where `cells` and `rules` are structurally zero.
    MissingCounter {
        required: &'static str,
    },
    /// The smoke run's background tab got no damage notice while hidden — or
    /// there was no background tab: the witness never ran, so its zero frames
    /// ([`Verdict::BackgroundDrew`]) would prove nothing. A missing counter of
    /// the second tab, hence right after the first tab's.
    BackgroundUnwoken,
    /// The frame count exceeded the upper bound: zero-frames-at-idle is broken.
    ExcessFrames {
        limit: u64,
    },
    /// The smoke run's background tab drew while hidden: zero frames in a
    /// background tab is broken. Recognised by its count like
    /// [`Verdict::ExcessFrames`], hence right after it; asked only once the
    /// witness is in ([`Verdict::BackgroundUnwoken`]).
    BackgroundDrew,
    /// At the deadline there was an animation that had not settled: its stop condition is broken.
    ///
    /// The **complement** of [`ExcessFrames`](Verdict::ExcessFrames), not a copy:
    /// that one sees frames flowing fast enough to exceed the bound and in a
    /// three-second run catches only what is above ~3 Hz; this one is
    /// **independent** of rate. A 0.2 Hz animation whose stop condition was
    /// forgotten never exceeds any frame bound but is still unsettled at the
    /// deadline, and that is exactly what violates the battery contract.
    ///
    /// Asked only in [`Workload::Smoke`]: the measurement load streams output
    /// until the deadline, so with the last line the cursor changes target and
    /// the deadline lands mid-stream. Tied to that arm, every measurement run
    /// would fall red while the code is right — the same reason
    /// `ExcessFrames` is exempt in the same arm.
    MotionUnsettled,
    /// The quiet between the last frame and the deadline is below the measured floor —
    /// or no frame was drawn at all ([`QUIET_FLOOR`]).
    ///
    /// The **last** of the leak arms (only
    /// [`ShutdownPanicked`](Verdict::ShutdownPanicked) comes after it, and that
    /// one describes the teardown path, not what the run measured): the two
    /// arms above recognise a leak either by its rate (`ExcessFrames`) or by the
    /// motion infrastructure (`MotionUnsettled`); this one recognises a path that
    /// skips both — code that asks for frames too rarely to exceed the bound,
    /// without going through the infrastructure — only by the trace it leaves.
    /// The rationale for the order is in the body of `verdict`, the arms are pinned by `a_short_tail_fails_the_gate`.
    QuietTooShort {
        floor: Duration,
    },
    /// A **panic** happened on the teardown path. The counters may be in place but
    /// the run cannot pass green: the project's "no panics on the PTY and parsing
    /// path" rule is violated, and even if **nobody reads** the `teardown=`
    /// token the gate must see it.
    ///
    /// [`Teardown::Abandoned`] and [`Teardown::Unbounded`] do **not** belong here:
    /// both are recorded debts (the child stuck inside exit; the OS thread
    /// limit) and the first happens in one of the four runs of the measurement
    /// load — tied to the gate, `make smoke` would fall red over a known debt.
    ShutdownPanicked {
        which: &'static str,
    },
}

/// The pure form of the gate — testable without a real display link and window.
///
/// Had the decision stayed in the body of [`AppDelegate::report_and_exit`], the
/// direction of the bound (8 or 180, is `Load` exempt) would be known only to
/// `make smoke` and written down in no test.
fn verdict(
    counters: Counters,
    workload: Workload,
    teardown: Option<Teardown>,
    motion: MotionState,
    quiet: Option<Duration>,
    background: Option<Background>,
) -> Verdict {
    let Counters {
        frames: n,
        content: c,
        cells: k,
        glyphs: g,
        rules: r,
        motion: m,
        // **Not** in the gate, and this is deliberate: whether the recipe
        // produces a slide has not been measured ([`Counters::slide`]). The token is still printed — for diagnosis.
        slide: _,
    } = counters;
    // Panic is asked **last** and the order of these arms is a diagnostic
    // preference, not a gate decision: whichever arm is chosen, the run is red and the exit is 1.
    // The order was set as "the more fundamental fault first" — missing counter
    // (a link never ran) > flowing frames > unsettled animation > short tail >
    // teardown panic. The smoke run's background tab adds one arm to each of
    // the first two classes and takes the second place in both: its missing
    // witness right after the measured tab's missing counters, its drawn
    // frames right after the measured tab's excess — the measured tab's own
    // arms keep their order, and a background tab that drew is asked only
    // once its witness is in (`a_background_tab_must_stay_dark`).
    // The three leak arms are ordered among themselves by
    // **recognising power**: `content` recognises it by its count, the settling
    // question by its infrastructure; the tail only by the trace it leaves, i.e. it says the least.
    // Panic goes last, because the others say that what the run **measured** is
    // broken; panic is about the path after the run ended. When both happen the
    // line writes only the first, but the `teardown=` token already carries the second —
    // `motion_and_panic_report_the_more_fundamental_fault` and
    // `a_short_tail_fails_the_gate` pin this order.
    // Reversing it would also break today's order of `ExcessFrames`.
    let panicked = panic_site(teardown);
    match workload {
        // The measurement load streams plain text: there is **no** background or
        // rule and there will not be. Asking for them would be asking a run that
        // never ran the smoke recipe for that recipe's numbers — the gate would
        // fall on every measurement run. Frame flow is the job itself here: no upper bound either.
        Workload::Load => {
            if n == 0 || g == 0 {
                Verdict::MissingCounter {
                    required: "frames and glyphs must be >0",
                }
            } else if let Some(which) = panicked {
                Verdict::ShutdownPanicked { which }
            } else {
                Verdict::Pass
            }
        }
        // Smoke recipe: all four > 0 **and** the content frames are bounded above.
        //
        // The lower bound is on `frames`, the upper bound on `content`, and this is
        // deliberate: the question "did the pipeline run" is answered by the frame the
        // GPU finished, the question "are frames flowing at idle" by the frame decided
        // to be drawn — motion frames legitimately inflate `frames`.
        Workload::Smoke => {
            // `motion` is the fifth requirement and in the same class as the
            // others: the smoke recipe has a cursor motion (`bt_core::smoke_shell`),
            // so zero means "the animation path never ran". The settling question comes
            // **after** it: an animation that never ran is settled anyway and the
            // reader must not be sent to the wrong fault.
            if n == 0 || k == 0 || g == 0 || r == 0 || m == 0 {
                Verdict::MissingCounter {
                    required: "all five must be >0",
                }
            } else if background.is_none_or(|back| back.wakes == 0) {
                // No background tab is the same answer: nothing was shown to
                // stay dark while hidden.
                Verdict::BackgroundUnwoken
            } else if c > IDLE_FRAME_LIMIT {
                Verdict::ExcessFrames {
                    limit: IDLE_FRAME_LIMIT,
                }
            } else if background.is_some_and(|back| back.frames > 0) {
                Verdict::BackgroundDrew
            } else if motion == MotionState::Unsettled {
                Verdict::MotionUnsettled
            } else if quiet.is_none_or(|q| q < QUIET_FLOOR) {
                // `None` falls here too and is **not** a separate arm: both say
                // "there was no quiet at the end of the run" and the message already
                // says which one it is via `quiet_phrase`. A separate variant would add
                // only a second name to the gate, without adding a second decision.
                // eklerdi.
                Verdict::QuietTooShort { floor: QUIET_FLOOR }
            } else if let Some(which) = panicked {
                Verdict::ShutdownPanicked { which }
            } else {
                Verdict::Pass
            }
        }
    }
}

/// The application's ssh masters: askpass is this very binary, the
/// sockets live in this instance's own directory under the user's cache
/// directory (or `/tmp/bateri-$UID`). What a dead bateri left
/// behind is swept once, off the main thread. A master ends with the user's
/// last session to its host and on quit ([`AppDelegate::shutdown`]).
/// The saved passwords are the login keychain's ([`crate::keychain`]).
///
/// `carried` is the instance an update's holder handed over
/// ([`Arrival::instance`]): its directories are this process's already
/// ([`arrive`]), so the masters opened before the update are recognised
/// again, the focus listener moves into them and the carried shells'
/// `BATERI_SSH_INSTANCE` still names this instance. `None`: a fresh name.
fn masters(carried: Option<&str>) -> Option<Arc<Masters>> {
    let askpass = std::env::current_exe().ok()?;
    // SAFETY: `getuid` has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    let bases = ssh_route::socket_bases(child::home().as_deref(), uid);
    let store = Arc::new(crate::keychain::Keychain);
    let masters = Arc::new(match carried {
        Some(instance) => Masters::with_instance(askpass, bases, store, instance.to_owned()),
        None => Masters::new(askpass, bases, store),
    });
    // `crate::run` took the update's holders before this (the handover's
    // sequence point, [`arrive`]): the sweep's `ssh` children come after it.
    let sweeper = Arc::clone(&masters);
    let _ = std::thread::Builder::new()
        .name("ssh socket sweep".into())
        .spawn(move || {
            // The focus listener in this instance's first directory,
            // **before** the sweep: it runs ssh per dead socket and an outside
            // process asking meanwhile must not wait on it. No directory, no
            // listener — silently; the client then reads `unknown`.
            if let Some(dir) = sweeper.bases().first() {
                let _ = focus::serve(dir, focus_answerer());
            }
            sweeper.sweep();
        });
    Some(masters)
}

/// The focus query's answerer: from the listener's thread, one hop to
/// the main queue — the answer is computed from the live state there, at the
/// moment of the question, with no shared copy — waited for at most
/// [`focus::ANSWER_WAIT`]. A busy main thread, or no delegate yet, is `None`
/// (`pane=unknown`); a hop that runs after the wait sends to a dropped
/// receiver and is lost.
fn focus_answerer() -> focus::Answerer {
    Arc::new(|tab: &TabId| {
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let tab = tab.clone();
        DispatchQueue::main().exec_async(move || {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            let _ = sender.send(delegate(mtm).map(|delegate| delegate.focus_answer(&tab)));
        });
        receiver.recv_timeout(focus::ANSWER_WAIT).ok().flatten()
    })
}

impl AppDelegate {
    pub(crate) fn new(
        mtm: MainThreadMarker,
        opts: Options,
        arrival: Option<Arrival>,
        skip_restore: bool,
    ) -> Retained<Self> {
        // The ring is allocated **only** when the gate is open: a closed gate must
        // cost an `Option` branch, not an allocation. Deriving the capacity
        // from the run duration is `bt-gpu`'s job too — it is the side that knows the refresh rate.
        // taraf o.
        let stats = opts
            .run
            .and_then(|run| run.stats_since.map(|since| Stats::new(since, run.seconds)))
            .map(Arc::new);
        let masters = opts
            .run
            .is_none()
            .then(|| masters(arrival.as_ref().map(|arrival| arrival.instance.as_str())))
            .flatten();
        let bundled = NSBundle::mainBundle().bundleIdentifier().is_some();
        let keeper = masters
            .as_ref()
            .filter(|_| opts.run.is_none() && bundled)
            .and_then(|masters| {
                let exe = std::env::current_exe().ok()?;
                Some(Rc::new(Keeper::new(
                    exe,
                    Arc::clone(masters),
                    current_layout,
                )))
            });
        let this = Self::alloc(mtm).set_ivars(Ivars {
            run: opts.run,
            measured: Cell::new(None),
            measured_asked: Cell::new(false),
            background: Cell::new(None),
            notices: RefCell::new(Notices::default()),
            settings: RefCell::new(Settings::default()),
            config_watch: RefCell::new(None),
            theme_watch: RefCell::new(None),
            stats,
            windows: RefCell::new(Vec::new()),
            next_id: Cell::new(0),
            appearance_dark: Cell::new(None),
            scrollbar: Cell::new(None),
            settings_window: RefCell::new(None),
            settings_state: RefCell::new(settings::FileState::Missing),
            shell_menu: OnceCell::new(),
            updater: OnceCell::new(),
            masters,
            restore_lock: RefCell::new(None),
            arrival: RefCell::new(arrival),
            holder: RefCell::new(None),
            keeper,
            hand_to_bound: Cell::new(false),
            end_programs: Cell::new(false),
            powering_off: Cell::new(false),
            quit_notice: RefCell::new(None),
            relaunch_after: Cell::new(false),
            postponed_update: RefCell::new(None),
            skip_restore,
            attempt_marks: RefCell::new(Vec::new()),
            layout_writer: Cell::new(false),
            layout_save_pending: Cell::new(false),
            command_monitor: RefCell::new(None),
            command_hinted: Cell::new(false),
            tab_drag: RefCell::new(None),
            tab_drop: Cell::new(None),
        });
        // SAFETY: NSObject's init takes no arguments and the ivars have been set.
        unsafe { msg_send![super(this), init] }
    }

    /// Identity of a new window, tab or pane; the counter only goes up.
    pub(crate) fn next_id(&self) -> u64 {
        let id = self.ivars().next_id.get();
        self.ivars().next_id.set(id + 1);
        id
    }

    /// A **copy** of the window list — every walking path uses this.
    ///
    /// A copy, because a call into a window can come back and reach here
    /// (`sync_geometry` → [`AppDelegate::post_notices`]) and walking the list
    /// while holding a borrow would end in a panic (`borrow_mut`) on a path
    /// that later mutates the list. The cost is a few `Retained` clones.
    pub(crate) fn windows(&self) -> Vec<Retained<TerminalWindow>> {
        self.ivars().windows.borrow().clone()
    }

    /// The window with identity `id`; `None` if it has left the list — a
    /// tab's way up (`TerminalTab`'s title and closing), the close question
    /// and the search paths.
    pub(crate) fn window(&self, id: u64) -> Option<Retained<TerminalWindow>> {
        self.ivars()
            .windows
            .borrow()
            .iter()
            .find(|window| window.id() == id)
            .cloned()
    }

    /// Every window's tabs, window by window — a copy, like the window list.
    pub(crate) fn tabs(&self) -> Vec<Retained<TerminalTab>> {
        self.windows()
            .iter()
            .flat_map(|window| window.tabs())
            .collect()
    }

    /// The tab with identity `id`; `None` if its window has left the list —
    /// the panes' owner handle (`tab::TabHost`), the close question's pane
    /// arm and the split path.
    pub(crate) fn tab(&self, id: u64) -> Option<Retained<TerminalTab>> {
        self.tabs().into_iter().find(|tab| tab.id() == id)
    }

    /// The pane with identity `id`; `None` if it is closed — the path of the jobs
    /// that return from the reader thread to the main queue (`ShellWake`, the
    /// alternate-screen notifier, uploads) ([`pane_by_id`]). It asks the pane's
    /// owner for tab- and window-level work (`tab::TabHost`).
    ///
    /// Unlike [`AppDelegate::window`], it does **not** find a pane whose
    /// teardown has started ([`find_open`]): the window leaves the list a turn
    /// later and a stale notification arriving in between must not do work on a
    /// closed session. The search covers all panes of all tabs (splits).
    pub(crate) fn pane(&self, id: u64) -> Option<Retained<TerminalPane>> {
        find_open(self.all_panes(), |pane| (pane.id() == id, pane.is_closed()))
    }

    /// All panes of all tabs — the list for the walking paths (settings
    /// distribution, Dock icon, lookup by identity); a copy, like the window list.
    fn all_panes(&self) -> Vec<Retained<TerminalPane>> {
        self.tabs().iter().flat_map(|tab| tab.panes()).collect()
    }

    /// The upload bar on the app's Dock icon — the total of all panes
    /// (`uploader::refresh_dock_tile`); the pane's
    /// `PaneHost::uploads_changed` event lands here.
    pub(crate) fn refresh_dock_tile(&self) {
        let panes = self.all_panes();
        let panes: Vec<&TerminalPane> = panes.iter().map(|pane| &**pane).collect();
        crate::uploader::refresh_dock_tile(self.mtm(), &panes);
        self.update_may_go();
    }

    /// What an update's relaunch waits for across every pane:
    /// the unfinished transfers — their bytes pass through
    /// bateri — and whether a password question is open (its answer does
    /// too) — a background tab's parked one counts the same: its slot holds
    /// the job's reply until it opens (`crate::sheets`).
    fn update_waits_for(&self) -> UpdateWait {
        let panes = self.all_panes();
        UpdateWait {
            transfers: panes.iter().map(|pane| pane.upload_unfinished()).sum(),
            sheet: panes.iter().any(|pane| pane.password().borrow().is_some()),
        }
    }

    /// Sparkle asks whether the relaunch should wait
    /// (`updater:shouldPostponeRelaunchForUpdate:untilInvokingBlock:`):
    /// `true` and `install` kept while something is waited for — the panes'
    /// lines say so — `false` (install now) otherwise.
    pub(crate) fn postpone_update(&self, install: RcBlock<dyn Fn()>) -> bool {
        let wait = self.update_waits_for();
        if !wait.holds() {
            return false;
        }
        self.ivars().postponed_update.replace(Some(install));
        self.show_update_wait(Some(wait.transfers));
        true
    }

    /// The update was aborted: the kept handler is let go, the lines lose
    /// their lead.
    pub(crate) fn drop_postponed_update(&self) {
        if self.ivars().postponed_update.take().is_some() {
            self.show_update_wait(None);
        }
    }

    /// Something the relaunch waits for changed (a transfer ended or was
    /// cancelled — ⌘. —, a sheet closed, a pane closed): with nothing left
    /// the kept handler runs, once, **on the next main-queue turn** — this
    /// is called from inside a pane's close and upload paths, and the
    /// handler quits for the install (the handover must not freeze a pane
    /// halfway through its own close); otherwise the lines' count follows.
    fn update_may_go(&self) {
        if self.ivars().postponed_update.borrow().is_none() {
            return;
        }
        let wait = self.update_waits_for();
        if wait.holds() {
            self.show_update_wait(Some(wait.transfers));
            return;
        }
        DispatchQueue::main().exec_async(|| {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(app) = delegate(mtm) {
                app.install_postponed_update();
            }
        });
    }

    /// The deferred half of [`Self::update_may_go`]: asks again (a transfer
    /// may have started in between) and runs the kept handler once.
    fn install_postponed_update(&self) {
        let wait = self.update_waits_for();
        if wait.holds() {
            self.show_update_wait(Some(wait.transfers));
            return;
        }
        let install = self.ivars().postponed_update.take();
        if let Some(install) = install {
            self.show_update_wait(None);
            install.call(());
        }
    }

    /// Every pane's line leads with the update's wait, or stops.
    fn show_update_wait(&self, left: Option<usize>) {
        for pane in self.all_panes() {
            pane.set_update_waits(left);
        }
    }

    /// The pane with tab identity `id`, its tab and its window; `None` if
    /// closed (`bateri://tab/`, `application:openURLs:`): bringing to the
    /// front a pane whose teardown has started but which has not yet left the
    /// list would put a sessionless window on screen ([`find_open`]). The
    /// identity is per pane — a tab holds several (`TerminalTab`'s header).
    fn pane_by_tab(
        &self,
        id: &TabId,
    ) -> Option<(
        Retained<TerminalWindow>,
        Retained<TerminalTab>,
        Retained<TerminalPane>,
    )> {
        self.windows().into_iter().find_map(|window| {
            window.tabs().into_iter().find_map(|tab| {
                let pane = find_open(tab.panes(), |pane| (pane.tab_id() == id, pane.is_closed()))?;
                Some((window.clone(), tab, pane))
            })
        })
    }

    /// The focus query's answer for pane `id`: `pane=none` if no
    /// open pane has it ([`Self::pane_by_tab`] — a closing pane is none);
    /// otherwise `focused` — bateri active, the pane active (its window key
    /// **and** its tab the one on screen, `TerminalPane::is_active`) **and**
    /// the tab's focused pane this one (the search field included,
    /// [`TerminalTab::focused_pane`]) — and the whole seconds since its last
    /// input. Main thread, at the moment of the question.
    fn focus_answer(&self, id: &TabId) -> focus::Answer {
        let Some((_, tab, pane)) = self.pane_by_tab(id) else {
            return focus::Answer::None;
        };
        let focused = NSApplication::sharedApplication(self.mtm()).isActive()
            && pane.is_active()
            && tab.focused_pane().id() == pane.id();
        focus::Answer::Live {
            focused,
            idle_secs: focus::idle_secs(pane.input_stamp().get(), focus::Moment::now()),
        }
    }

    /// The active window: `NSApp.keyWindow` is looked up in the list — a
    /// key sheet stands for the window it sits on, and a tab's sheet owner
    /// for its terminal window (`TerminalWindow::owns`), so ⌘T while a
    /// question is open reaches that window (and its selection guard)
    /// instead of opening a window of its own. `None` if the settings window
    /// or a panel is key, and the new window is born at home. The source of
    /// inheritance is its selected tab's **focused pane**
    /// (`TerminalTab::focused_pane`).
    fn key_window(&self) -> Option<Retained<TerminalWindow>> {
        let key = NSApplication::sharedApplication(self.mtm()).keyWindow()?;
        let key = key.sheetParent().unwrap_or(key);
        self.window_owning(&key)
    }

    /// The window a window-wide question of the application goes on: the
    /// key window — a terminal window when the key one is a sheet in it, a
    /// tab's question included, so the report never opens on a sheet; any
    /// other key window (the settings window) as it is.
    fn question_window(&self) -> Option<Retained<NSWindow>> {
        let key = NSApplication::sharedApplication(self.mtm()).keyWindow()?;
        let key = key.sheetParent().unwrap_or(key);
        match self.window_owning(&key) {
            Some(window) => Some(window.ns_window().retain()),
            None => Some(key),
        }
    }

    /// The active tab: the key window's selected tab.
    fn key_tab(&self) -> Option<Retained<TerminalTab>> {
        Some(self.key_window()?.selected_tab())
    }

    /// The active tab's remote host and its resolved mark; `None` in a local tab or
    /// when no terminal window is key — the input of Shell ▸ Mark … as ▸.
    pub(crate) fn key_remote_mark(&self) -> Option<(String, HostMark)> {
        self.key_tab()?.remote_mark()
    }

    /// The active tab's markable host — its remote host, else the server
    /// its database client is connected to — and its resolved mark; `None`
    /// when there is neither or no terminal window is key. The input of
    /// Shell ▸ Mark … as ▸ (`TerminalTab::mark_target`).
    pub(crate) fn key_mark_target(&self) -> Option<(String, HostMark, MarkSubject)> {
        self.key_tab()?.mark_target()
    }

    /// The tabs a split of the key window's selected tab can be moved to
    /// (id, title): the list Window ▸ Move Split to Tab ▸ is filled with on
    /// opening; empty when no terminal window is key or it has one tab.
    pub(crate) fn key_move_targets(&self) -> Vec<(u64, String)> {
        self.key_window()
            .map(|window| window.move_targets())
            .unwrap_or_default()
    }

    /// The ports of the key tab's focused pane — its programs' and, in a
    /// remote session, the server's: the input of Shell ▸ Open Port ▸; empty
    /// when no terminal window is key.
    pub(crate) fn key_ports(&self) -> crate::footer::PortsModel {
        self.key_tab()
            .map(|tab| tab.focused_pane().ports_model())
            .unwrap_or_default()
    }

    /// [`Self::toggle_host_integration`]'s `plain` forgetting: the key tab's
    /// remote ssh argv, `ssh -G` and the state file on a thread of its own.
    /// `turn_off` is the host to turn the integration off for when **no**
    /// row was forgotten — back on the main queue; it is turned off at once
    /// when there is nothing to ask (a timed run, no remote target, no
    /// thread).
    fn forget_plain(&self, turn_off: Option<String>) {
        let off = |delegate: &Self, host: String| {
            delegate.save_edit(&SettingsEdit::RemoteHostIntegration { host, on: false });
        };
        let target = self.key_tab().and_then(|tab| {
            tab.focused_pane()
                .session()
                .and_then(|session| session.remote_target())
        });
        let (Some((_, target, _)), None) = (target, self.ivars().run.as_ref()) else {
            if let Some(host) = turn_off {
                off(self, host);
            }
            return;
        };
        let argv = target.argv;
        let host = turn_off.clone();
        let spawned = std::thread::Builder::new()
            .name("remote plain".into())
            .spawn(move || {
                let forgot = crate::child::home().is_some_and(|home| {
                    bt_shell_common::ssh_wrap::forget_plain(
                        &crate::ssh_route::SystemSsh,
                        &argv,
                        &crate::remote_hosts_path(&home),
                    )
                    .unwrap_or(false)
                });
                if let (false, Some(host)) = (forgot, host) {
                    DispatchQueue::main().exec_async(move || {
                        // audit: a block running on the main queue is on the main thread by definition.
                        let mtm =
                            MainThreadMarker::new().expect("the main queue is the main thread");
                        if let Some(delegate) = delegate(mtm) {
                            off(&delegate, host);
                        }
                    });
                }
            });
        if let (Err(_), Some(host)) = (spawned, turn_off) {
            off(self, host);
        }
    }

    /// The terminal window whose `NSWindow` is `window`; `None` if not in the list
    /// (panel, settings window, closed window).
    pub(crate) fn window_owning(&self, window: &NSWindow) -> Option<Retained<TerminalWindow>> {
        self.ivars()
            .windows
            .borrow()
            .iter()
            .find(|candidate| candidate.owns(window))
            .cloned()
    }

    /// Body of `applicationShouldTerminate:`.
    ///
    /// The settings borrow is released **before** `runModal`: the modal loop spins
    /// the run loop and a save arriving in the meantime (`reload_settings`) could
    /// not `replace` with an open borrow. The app is brought to the front first: the
    /// modal of a background app can stay behind the windows and Dock ▸ Quit is exactly that path.
    /// Quit tam o yol.
    fn terminate_reply(&self) -> NSApplicationTerminateReply {
        let timed = self.ivars().run.is_some();
        if timed {
            return NSApplicationTerminateReply::TerminateNow;
        }
        // Consumed by this quit whatever it turns into.
        let relaunch = crate::updater::take_relaunch();
        let kind = QuitKind::of(relaunch, self.ivars().end_programs.take());
        self.ivars().hand_to_bound.set(false);
        self.ivars().quit_notice.replace(None);
        let windows = self.windows();
        if windows.is_empty() {
            return NSApplicationTerminateReply::TerminateNow;
        }
        // Where the programs go (`keep_running`): to the bound holder, to
        // the update's holder, or nowhere — then today's question. A holder
        // counts only if it answers now; under `"quit"` one that does not is
        // replaced first, so no program ends unasked on an assumption.
        let keep = self.settings().keep_running;
        let mut bound = keeper::pings_for_quit(keep, kind)
            && self.ivars().keeper.as_deref().is_some_and(Keeper::verified);
        if keeper::spawns_for_quit(keep, kind, bound) {
            bound = self.replace_keeper();
        }
        match keeper::quit_path(keep, kind, bound) {
            QuitPath::ToBound => {
                self.ivars().hand_to_bound.set(true);
                // ⌘Q's reminder names the programs it keeps: read now, before
                // the freeze stops the panes ([`AppDelegate::leave_quit_notice`]).
                if kind == QuitKind::Quit {
                    let foregrounds: Vec<_> = self
                        .all_panes()
                        .iter()
                        .map(|pane| pane.foreground())
                        .collect();
                    let notice = window::kept_notice(&foregrounds, self.ivars().powering_off.get());
                    self.ivars().quit_notice.replace(notice);
                }
                return NSApplicationTerminateReply::TerminateNow;
            }
            // Nothing dies, so nothing is asked — unless the holder cannot be
            // born, then today's question.
            QuitPath::ToUpdateHolder if self.prepare_handover() => {
                return NSApplicationTerminateReply::TerminateNow;
            }
            QuitPath::ToUpdateHolder | QuitPath::Close => {}
        }
        let confirm = self.settings().confirm_close;
        // The question collects the running job from the panes.
        let panes = self.all_panes();
        let unit = window::unit_for(panes.len(), self.tabs().len());
        let Some(foregrounds) = window::foregrounds_to_ask(timed, confirm, &panes) else {
            return NSApplicationTerminateReply::TerminateNow;
        };
        let mtm = self.mtm();
        NSApplication::sharedApplication(mtm).activate();
        let alert = window::alert(mtm, &window::prompt(CloseScope::Quit, unit, &foregrounds));
        if alert.runModal() == NSAlertFirstButtonReturn {
            NSApplicationTerminateReply::TerminateNow
        } else {
            self.ivars().relaunch_after.set(false);
            NSApplicationTerminateReply::TerminateCancel
        }
    }

    /// The handover's first step: spawns the holder
    /// ([`handover::spawn_holder`]) with this instance's directories, kept
    /// for [`AppDelegate::shutdown`]. `false` — today's quit — in an
    /// unbundled process (the layout is matched to its bundle on the other
    /// side), without an ssh registry (its directories are the holder's
    /// socket) or when the spawn fails. `restore_windows` does not take
    /// part: `keep_running` is the programs' one authority, and every value
    /// of it keeps them across an update.
    fn prepare_handover(&self) -> bool {
        let Some(masters) = self.ivars().masters.as_deref() else {
            return false;
        };
        if NSBundle::mainBundle().bundleIdentifier().is_none() {
            return false;
        }
        let dirs = masters.bases().to_vec();
        let spawned = std::env::current_exe().and_then(|exe| handover::spawn_holder(&exe, &dirs));
        match spawned {
            Ok(spawned) => {
                self.ivars().holder.replace(Some(spawned));
                true
            }
            Err(error) => {
                eprintln!("bateri: the update's holder could not start: {error}");
                false
            }
        }
    }

    /// The handover's second step, at the head of [`AppDelegate::shutdown`]
    /// after the session restore save: every pane is frozen
    /// ([`TerminalPane::freeze_for_handover`]) and given with the layout to
    /// the holder — the update's, spawned at this quit, or the bound one over
    /// its connection ([`Target`]); `true` once the holder said it holds
    /// them. The panes that could not be frozen close today's way. `false` →
    /// today's quit closes what is left — a frozen pane cannot go back, its
    /// master stays open in this process until it exits and that is its
    /// hang-up.
    fn hand_over(&self, target: Target) -> bool {
        let Some(bundle_id) = NSBundle::mainBundle().bundleIdentifier() else {
            target.dismiss();
            return false;
        };
        // The layout reads the live sessions: before any freeze.
        let (mut saved, _) = self.saved_session(false);
        let with_history = self.settings().restore_windows == RestoreWindows::All;
        let mut held = Vec::new();
        let mut left = Vec::new();
        let mut histories: Histories = Vec::new();
        for window in self.windows() {
            for pane in window.panes() {
                match pane.freeze_for_handover() {
                    Some((frozen, history)) => {
                        histories.push((frozen.tab.clone(), history));
                        held.push(frozen);
                    }
                    None => {
                        // Not frozen: it closes today's way below, its
                        // scrollback read live first.
                        if let Some(session) = pane.session() {
                            histories.push((pane.tab_id().clone(), session.final_history()));
                        }
                        left.push(pane);
                    }
                }
            }
        }
        // The session restore save, the fallback if the new bateri finds no
        // holder — `"off"` deletes what is left, as today's quit does (the
        // programs still cross: `restore_windows` no longer decides that).
        if let Some(lock) = self.ivars().restore_lock.take() {
            let result = if self.settings().restore_windows == RestoreWindows::Off {
                restore::clear(&lock)
            } else {
                histories.retain(|(_, history)| with_history && !history.is_empty());
                for pane in saved
                    .windows
                    .iter_mut()
                    .flat_map(|window| window.tabs.iter_mut())
                    .flat_map(|tab| tab.panes.iter_mut())
                {
                    pane.history = histories.iter().any(|(tab, _)| *tab == pane.tab_id);
                }
                restore::save(&lock, &saved, &histories)
            };
            if let Err(error) = result {
                eprintln!("bateri: could not save the session: {error}");
            }
        }
        if held.is_empty() {
            target.dismiss();
            return false;
        }
        // After the save: the layout carries its history flags, so a pane
        // that was not frozen falls back with the history saved for it.
        let layout = handover::layout_blob(&bundle_id.to_string(), &saved.render());
        let count = held.len();
        let bundle = handover::Bundle {
            layout,
            panes: held,
        };
        let given = match target {
            Target::Update(holder) => holder.give(bundle).map_err(|error| {
                eprintln!("bateri: the update's holder did not take the panes: {error}");
            }),
            Target::Bound(bound, id) => self.give_to_bound((bound, id), bundle),
        };
        if given.is_err() {
            return false;
        }
        eprintln!("bateri: handed {count} pane(s) over to the holder");
        // The panes that could not be frozen close below: their session ends
        // must not `-O exit` a master a carried pane still rides.
        if let Some(masters) = &self.ivars().masters {
            masters.begin_quit();
        }
        let closing: Vec<_> = left.iter().filter_map(|pane| pane.begin_close()).collect();
        let deadline = Instant::now() + SHUTDOWN_GRACE;
        for closing in closing {
            let _ = closing.wait_until(deadline);
        }
        true
    }

    /// [`AppDelegate::hand_over`] to the bound holder: the frame over its
    /// connection. Giving consumes the bundle (this side's copies of the
    /// masters close either way), so a copy of each master is taken first: if
    /// the holder does not take them, the frozen panes go to a **fresh** bound
    /// holder — not the update's kind, whose time limit would end the
    /// programs two minutes later. `Err` if neither takes them (the frozen
    /// panes then cannot go back, a known limit).
    ///
    /// Once the fresh holder has them the first one is **ended**: it may
    /// have taken the frame and answered too late, or refused and kept its
    /// registrations — either way it would go on holding copies of the same
    /// programs. If no fresh holder takes them it is left alone, since it may
    /// still be holding them.
    fn give_to_bound(
        &self,
        (bound, first): (handover::Bound, keeper::HolderId),
        bundle: handover::Bundle,
    ) -> Result<(), ()> {
        let spare = copy_bundle(&bundle);
        let Err(error) = bound.hand_over(bundle) else {
            return Ok(());
        };
        eprintln!("bateri: the holder did not take the panes ({error}); trying a new one");
        let (Some(keeper), Some(spare)) = (self.ivars().keeper.as_deref(), spare) else {
            return Err(());
        };
        let mtm = self.mtm();
        if !keeper.spawn(mtm) {
            return Err(());
        }
        let Some((fresh, _)) = keeper.take() else {
            return Err(());
        };
        match fresh.hand_over(spare) {
            Ok(()) => {
                first.kill();
                Ok(())
            }
            Err(error) => {
                eprintln!("bateri: the new holder did not take the panes either: {error}");
                Err(())
            }
        }
    }

    /// Spawns the bound holder at launch (`keep_running` `"crash"` or
    /// `"quit"`), before the first windows: they register as they are
    /// born. A holder that cannot start leaves the programs unprotected,
    /// said on stderr.
    fn start_keeper(&self) {
        if let Some(keeper) = &self.ivars().keeper {
            keeper.spawn(self.mtm());
        }
    }

    /// A bound holder in place of one that did not answer (or of none): the
    /// old one goes ([`Keeper::discard`]), a new one is spawned and every
    /// live pane registers with it. `true` if one runs.
    fn replace_keeper(&self) -> bool {
        let Some(keeper) = self.ivars().keeper.as_deref() else {
            return false;
        };
        keeper.discard();
        self.spawn_keeper_and_register(keeper)
    }

    /// Spawns a holder and registers every live pane with it — after the
    /// handshake's layout, each registration sends the layout again.
    fn spawn_keeper_and_register(&self, keeper: &Keeper) -> bool {
        let mtm = self.mtm();
        if !keeper.spawn(mtm) {
            self.break_journals();
            return false;
        }
        for pane in self.all_panes() {
            pane.register_with_holder(mtm);
        }
        true
    }

    /// Every pane's journal breaks: no holder confirms a base any more, so
    /// the journals would only fill and stall the panes' reading.
    fn break_journals(&self) {
        for pane in self.all_panes() {
            pane.break_journal();
        }
    }

    /// The bound holder of `generation` went away while bound (its
    /// handle's death news, [`Keeper::spawn`]): another is spawned and
    /// everything registered again — under `"crash"` and `"quit"` only, and
    /// a bounded number of times ([`keeper::RESPAWN_LIMIT`]).
    pub(crate) fn holder_died(&self, generation: u64) {
        let Some(keeper) = self.ivars().keeper.as_deref() else {
            return;
        };
        if !keeper.died(generation) {
            // The run goes on unprotected once the replacements are spent.
            if !keeper.is_active() {
                self.break_journals();
            }
            return;
        }
        if self.settings().keep_running == KeepRunning::Update {
            self.break_journals();
            return;
        }
        eprintln!("bateri: the holder went away; starting another");
        self.spawn_keeper_and_register(keeper);
    }

    /// A live change of `keep_running` ([`keeper::switch`]): away from
    /// `"update"` a holder is spawned and every live pane registers — from
    /// now on its program outlives a crash; to `"update"` the holder leaves
    /// quietly and the programs stay with bateri. `"crash"` ↔ `"quit"` is
    /// only ⌘Q's to read.
    fn switch_keeper(&self, switch: keeper::Switch) {
        let Some(keeper) = self.ivars().keeper.as_deref() else {
            return;
        };
        match switch {
            keeper::Switch::Spawn => {
                self.spawn_keeper_and_register(keeper);
            }
            keeper::Switch::Leave => {
                keeper.leave();
                // Under `"update"` nothing reads a journal: recording stops.
                self.break_journals();
            }
            keeper::Switch::Stay => {}
        }
    }

    /// The bound holder's driver, for the delayed jobs that find it from
    /// the main queue.
    pub(crate) fn keeper(&self) -> Option<&Keeper> {
        self.ivars().keeper.as_deref()
    }

    /// A layout edge (a window, a tab, a split, the focus, a directory, a
    /// close): the bound holder gets the layout once the burst settles
    /// ([`Keeper::layout_changed`]), and so does the disk
    /// ([`AppDelegate::save_layout_later`]).
    pub(crate) fn layout_changed(&self) {
        if let Some(keeper) = &self.ivars().keeper {
            keeper.layout_changed();
        }
        self.save_layout_later();
    }

    /// Schedules [`AppDelegate::settled`] [`SETTLE_DELAY`] after the first
    /// windows are built — never in a timed run, which neither restores nor
    /// saves.
    fn settle_later(&self) {
        if self.ivars().run.is_some() {
            return;
        }
        let Ok(when) = DispatchTime::try_from(SETTLE_DELAY) else {
            self.settled();
            return;
        };
        let _ = DispatchQueue::main().after(when, || {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(app) = delegate(mtm) {
                app.settled();
            }
        });
    }

    /// The launch settled: what it restored did not crash it, so the attempt
    /// markers go, and the layout starts going to disk on its edges — written
    /// once now, so a crash before the next edge still finds it. Not before:
    /// the layout file is read and deleted before a restore replays anything,
    /// and writing it back during a restore that crashes would bring the same
    /// restore back on every launch.
    fn settled(&self) {
        self.clear_attempt_marks();
        self.ivars().layout_writer.set(true);
        self.save_layout();
    }

    /// Removes the attempt markers this launch counted
    /// ([`restore::clear_attempt`]): it settled, or it quits cleanly.
    fn clear_attempt_marks(&self) {
        for dir in self.ivars().attempt_marks.take() {
            restore::clear_attempt(&dir);
        }
    }

    /// A layout edge's disk write, once the burst settles
    /// ([`keeper::LAYOUT_DELAY`]) — only once the launch settled.
    fn save_layout_later(&self) {
        if !self.ivars().layout_writer.get() || self.ivars().layout_save_pending.replace(true) {
            return;
        }
        let Ok(when) = DispatchTime::try_from(keeper::LAYOUT_DELAY) else {
            self.ivars().layout_save_pending.set(false);
            return;
        };
        let _ = DispatchQueue::main().after(when, || {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(app) = delegate(mtm) {
                app.ivars().layout_save_pending.set(false);
                app.save_layout();
            }
        });
    }

    /// The live windows' layout to disk without histories
    /// ([`restore::save_layout`]) — the way back after a crash no holder
    /// carried the programs through (`keep_running = "update"`, no holder,
    /// a power cut). `restore_windows = "off"` writes nothing; neither does a
    /// process without the session directory's lock, nor one that quits —
    /// the quit's save took the lock and writes the whole session.
    fn save_layout(&self) {
        if self.settings().restore_windows == RestoreWindows::Off {
            return;
        }
        let lock = self.ivars().restore_lock.borrow();
        let Some(lock) = lock.as_ref() else {
            return;
        };
        let (saved, _) = self.saved_session(false);
        if let Err(error) = restore::save_layout(lock, &saved) {
            eprintln!("bateri: could not save the window layout: {error}");
        }
    }

    /// The live windows as the frame's layout ([`handover::layout_blob`]):
    /// session restore's text without histories, named by the bundle.
    fn layout_blob(&self) -> Option<Vec<u8>> {
        let bundle_id = NSBundle::mainBundle().bundleIdentifier()?.to_string();
        let (saved, _) = self.saved_session(false);
        Some(handover::layout_blob(&bundle_id, &saved.render()))
    }

    /// The handover test item's relaunch: a waiting shell that
    /// starts this very binary once this process is gone — spawned clean
    /// ([`handover::spawn_clean`]), so no master of a frozen pane rides
    /// along, and with this process's environment (not `open`'s: a test
    /// package started with its own `HOME` stays in it).
    fn spawn_relauncher(&self) {
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        let script = "while /bin/kill -0 \"$0\" 2>/dev/null; do /bin/sleep 0.1; done; exec \"$1\"";
        let args = [
            OsString::from("-c"),
            OsString::from(script),
            OsString::from(std::process::id().to_string()),
            exe.into_os_string(),
        ];
        if let Err(error) = handover::spawn_clean(Path::new("/bin/sh"), &args, None) {
            eprintln!("bateri: could not relaunch: {error}");
        }
    }

    /// Removes the closing window from the list — the job one turn after
    /// `windowWillClose:`. The object is dropped here, on the main thread and after
    /// the borrow is released: lest the `Drop` of the dropped window come back and
    /// reach the list and leave `borrow_mut` open.
    pub(crate) fn forget_window(&self, id: u64) {
        let removed = {
            let mut windows = self.ivars().windows.borrow_mut();
            windows
                .iter()
                .position(|window| window.id() == id)
                .map(|index| windows.remove(index))
        };
        drop(removed);
        self.layout_changed();
    }

    /// Every screen's visible area (no menu bar, no Dock) as frames — what a window frame is
    /// clamped onto ([`clamp_frame`]).
    fn visible_frames(&self) -> Vec<Frame> {
        NSScreen::screens(self.mtm())
            .iter()
            .map(|screen| {
                let rect = screen.visibleFrame();
                Frame {
                    x: rect.origin.x,
                    y: rect.origin.y,
                    width: rect.size.width,
                    height: rect.size.height,
                }
            })
            .collect()
    }

    /// Takes window `id` out of the list at once; its closing is the caller's.
    fn unlist_window(&self, id: u64) {
        let removed = {
            let mut windows = self.ivars().windows.borrow_mut();
            windows
                .iter()
                .position(|window| window.id() == id)
                .map(|index| windows.remove(index))
        };
        // Dropped after the borrow is released, like `forget_window`'s.
        drop(removed);
    }

    /// Opens a new window — the **only** path that spawns windows: the
    /// launch's first window (`from = None`), ⌘N, a tab request without a
    /// window and the Dock icon. A new tab in an existing window is
    /// [`AppDelegate::open_tab`]'s.
    ///
    /// `from` is the active window; the new shell starts in its OSC 7 directory (home
    /// if none), the temporary point-size delta comes from it and the theme from its
    /// session — all windows share the same theme; without `from` the theme is
    /// resolved from settings. The shell's first input comes from `opening` and
    /// `from`'s remote target ([`initial_line`]).
    ///
    /// Order: point size, notice and chrome before the window is visible, the list before placement
    /// (so geometry events find the window in the list), the session **after**
    /// placement — the shell must see its first `TIOCSWINSZ` at the final size.
    ///
    /// The error returns to the caller; if the session could not be born, the window is closed.
    fn open_window(
        &self,
        from: Option<&TerminalWindow>,
        opening: Opening,
    ) -> Result<Retained<TerminalWindow>, String> {
        let mtm = self.mtm();
        let id = self.next_id();
        let tab = self.next_id();
        // Inheritance comes from the active window's selected tab's **focused pane**.
        let source = from.map(|from| from.selected_tab().focused_pane());
        let (launch, theme) = self.pane_launch(tab, source.as_deref(), opening);
        let window = TerminalWindow::new(mtm, id, tab, launch).map_err(|e| e.to_string())?;
        window.set_notice(&self.ivars().notices.borrow().subtitle());
        self.ivars().windows.borrow_mut().push(window.clone());
        // Chrome **before** the window is visible: if painted afterwards, every ⌘N
        // would show the system's grey title bar for a frame. The separator's colour
        // comes from the same theme too (the first form of `TerminalWindow::set_theme`).
        window.set_theme(theme);
        // The top edge's mode beside it, for the container's line: the pane
        // already has it from its birth settings, the same slot.
        let edge = self.settings().content_edge;
        window.set_content_edge(edge);
        window.show_after(from);
        // Timed run: the smoke gate must not depend on which app is in front
        // (`TerminalWindow::float_for_timed_run`).
        if self.ivars().run.is_some() {
            window.float_for_timed_run();
        }
        if let Err(e) = window.start(mtm) {
            window.close();
            return Err(format!("failed to start the shell: {e}"));
        }
        Ok(window)
    }

    /// Opens a new tab in `window`, right of its selected tab
    /// (`TerminalWindow::add_tab`, the window's applier) — ⌘T, ⌥⌘T, the
    /// bar's `+` and ⌘N under the system's "Prefer tabs". Inheritance as
    /// [`AppDelegate::open_window`]'s, from the window's selected tab; the
    /// theme and top edge are the window's.
    ///
    /// While the window holds a sheet nothing is born: a beep, the
    /// selection guard's (`TerminalWindow::selection_free`) — the new tab
    /// would come up under the question. If the session cannot be born the
    /// tab closes again and the error returns.
    fn open_tab(&self, window: &TerminalWindow, opening: Opening) -> Result<(), String> {
        if !window.selection_free() {
            crate::preview::beep();
            return Ok(());
        }
        let mtm = self.mtm();
        let tab_id = self.next_id();
        let source = window.selected_tab().focused_pane();
        let (launch, theme) = self.pane_launch(tab_id, Some(&source), opening);
        let pane = TerminalPane::new(mtm, crate::window::initial_rect(), launch)
            .map_err(|e| e.to_string())?;
        let tab = TerminalTab::new(mtm, tab_id, window.id(), &pane);
        let edge = self.settings().content_edge;
        window.add_tab(&tab, (theme, edge));
        // After the container is attached and sized: the geometry is built
        // from the final frame (`TerminalWindow::with_pane`'s order).
        pane.observe_frame();
        if let Err(e) = tab.start(mtm) {
            window.close_tab_now(tab.id());
            return Err(format!("failed to start the shell: {e}"));
        }
        window.refresh_title();
        Ok(())
    }

    /// Move Tab to New Window: tab `tab` of window `from` leaves it for a
    /// window of its own, the size and place it had, cascaded — the tab
    /// itself, not a copy: its shells go on and its questions, indicators and
    /// name come with it. Nothing moves while either window holds a question
    /// of its own (a beep, like a selection would), and a window's only tab
    /// stays where it is.
    ///
    /// The order is the applier's: the tab leaves the source
    /// ([`TerminalWindow::release_tab`]), a window is built around it
    /// ([`TerminalWindow::with_tab`]'s order, as [`AppDelegate::open_window`]
    /// builds one), and the tab comes up in it ([`TerminalWindow::show_arrived`]).
    /// No shell is started or told to end.
    pub(crate) fn move_tab_to_new_window(&self, from: &TerminalWindow, tab: u64) {
        self.tab_to_new_window(from, tab, None);
    }

    /// A tab let go over no bar becomes a window of its own, its title row under the
    /// pointer at `at` (screen points) and the window kept on a visible screen — Move Tab to
    /// New Window, where the user chose the place. Otherwise as
    /// [`AppDelegate::move_tab_to_new_window`], whose cascade a chosen place replaces.
    fn tab_to_new_window(&self, from: &TerminalWindow, tab: u64, at: Option<NSPoint>) {
        if from.tab_count() < 2 || !from.selection_free() {
            crate::preview::beep();
            return;
        }
        let Some(moved) = from.release_tab(tab) else {
            return;
        };
        self.open_window_around(from, moved, at);
    }

    /// Move Split to New Window: pane `pane` of `from` becomes a window of
    /// its own, the size and place its window had, cascaded — the pane itself,
    /// not a copy: its shell, programs and questions go on. It is a tab of
    /// that window ([`TerminalTab::new`]), not a split of one. A tab's only
    /// pane is its tab, so that moves as it does in Move Tab to New Window
    /// and a window's only pane stays where it is. Nothing moves while the
    /// window holds a question of its own (a beep).
    pub(crate) fn pane_to_new_window(&self, from: &TerminalWindow, pane: u64) {
        let Some(tab) = from.tab_holding(pane) else {
            return;
        };
        if tab.panes().len() == 1 {
            self.tab_to_new_window(from, tab.id(), None);
            return;
        }
        if !from.selection_free() {
            crate::preview::beep();
            return;
        }
        let Some(released) = from.release_pane(tab.id(), pane) else {
            return;
        };
        let moved = TerminalTab::new(self.mtm(), self.next_id(), from.id(), &released);
        self.open_window_around(from, moved, None);
    }

    /// A window around `moved`, a tab that has left `from`: the size and
    /// place `from` has (or, with `at`, its title row under that screen
    /// point), the theme of its focused pane, shown and brought up
    /// ([`TerminalWindow::show_arrived`]). No shell is started or told to end.
    fn open_window_around(
        &self,
        from: &TerminalWindow,
        moved: Retained<TerminalTab>,
        at: Option<NSPoint>,
    ) {
        let theme = moved
            .focused_pane()
            .session()
            .map_or_else(|| self.resolve_theme(), |session| session.theme());
        let source = from.ns_window().frame();
        let frame = match at {
            Some(at) => {
                // The pointer lands in the middle of the title row: the bar's height, which is
                // the window's one copy of it.
                let row = from.bar().frame().size.height;
                let wanted = Frame {
                    x: at.x - source.size.width / 2.0,
                    y: at.y + row / 2.0 - source.size.height,
                    width: source.size.width,
                    height: source.size.height,
                };
                let placed = clamp_frame(wanted, &self.visible_frames());
                NSRect::new(
                    NSPoint::new(placed.x, placed.y),
                    NSSize::new(placed.width, placed.height),
                )
            }
            None => source,
        };
        let window =
            TerminalWindow::with_tab(self.mtm(), self.next_id(), from.run(), moved, Some(frame));
        window.set_notice(&self.ivars().notices.borrow().subtitle());
        self.ivars().windows.borrow_mut().push(window.clone());
        window.set_theme(theme);
        window.set_content_edge(self.settings().content_edge);
        if at.is_some() {
            window.show_at(frame);
        } else {
            window.show_after(Some(from));
        }
        if self.ivars().run.is_some() {
            window.float_for_timed_run();
        }
        window.show_arrived();
        self.layout_changed();
    }

    /// The window that holds tab `tab` now.
    fn window_holding(&self, tab: u64) -> Option<Retained<TerminalWindow>> {
        self.windows()
            .into_iter()
            .find(|window| window.index_of(tab).is_some())
    }

    /// A tab carried out of its strip is in a session ([`crate::tab_drag`]); its source is kept
    /// until the session's end is carried out ([`AppDelegate::tab_drag_ended`]).
    pub(crate) fn hold_tab_drag(&self, source: Retained<TabDragSource>) {
        self.ivars().tab_drag.replace(Some(source));
    }

    /// The carried tab was let go on window `onto`'s bar with the decision `landing`
    /// ([`crate::tabs::landing`]); carried out when the session ends
    /// ([`AppDelegate::tab_drag_ended`]).
    pub(crate) fn tab_dropped(&self, onto: u64, landing: Landing) {
        self.ivars().tab_drop.set(Some((onto, landing)));
    }

    /// The carried tab's session ended at screen point `at` with `operation`, `taken_back` if the
    /// user ended it with Esc ([`crate::tab_drag::TabDragSource`]). Carried
    /// out one main-queue turn later, outside AppKit's teardown of the session: a drop on a bar
    /// lands the tab where it was let go ([`AppDelegate::land_tab`]); a drop on nothing makes it a
    /// window there; a tab taken back stays in its strip.
    pub(crate) fn tab_drag_ended(
        &self,
        tab: u64,
        at: NSPoint,
        operation: NSDragOperation,
        taken_back: bool,
    ) {
        let dropped = self.ivars().tab_drop.take();
        let detach = dropped.is_none() && operation == NSDragOperation::None && !taken_back;
        DispatchQueue::main().exec_async(move || {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            let Some(app) = delegate(mtm) else {
                return;
            };
            // The session is over; its source may go.
            drop(app.ivars().tab_drag.take());
            if let Some(from) = app.window_holding(tab) {
                from.bar().drag_ended();
            }
            match dropped {
                Some((onto, landing)) => {
                    if let Some(onto) = app.window(onto) {
                        app.land_tab(tab, &onto, landing);
                    }
                }
                None if detach => {
                    if let Some(from) = app.window_holding(tab) {
                        app.tab_to_new_window(&from, tab, Some(at));
                    }
                }
                None => {}
            }
            // The room a carried tab made on a bar closes — where it landed the window already
            // took the tab into it, and where it did not (a refusal, a vow taken back) this does.
            for window in app.windows() {
                window.bar().open_gap(None);
            }
        });
    }

    /// A carried tab lands on window `onto`'s strip ([`crate::tabs::Landing`]): in its own
    /// window it takes its place, in another it leaves its window as itself and joins at the
    /// place ([`TerminalWindow::release_tab`], [`TerminalWindow::adopt_tab`]) — and the
    /// window it left closes if that was its last tab, as in Merge All Windows. Nothing moves while either
    /// window holds a question of its own (a beep).
    fn land_tab(&self, tab: u64, onto: &TerminalWindow, landing: Landing) {
        let Some(from) = self.window_holding(tab) else {
            return;
        };
        match landing {
            Landing::Reorder(index) => from.move_tab(tab, index),
            Landing::Join(index) if from.id() != onto.id() => {
                if !from.selection_free() || !onto.selection_free() {
                    crate::preview::beep();
                    return;
                }
                let Some(moved) = from.release_tab(tab) else {
                    return;
                };
                onto.adopt_tab(&moved, Placement::At(index));
                if from.tab_count() == 0 {
                    // Out of the list now, as in `merge_all_windows`.
                    self.unlist_window(from.id());
                    from.close();
                }
                onto.select();
                self.layout_changed();
            }
            // A join on its own window is a reorder; the bar says so, not this.
            Landing::Join(_) | Landing::Detach => {}
        }
    }

    /// Merge All Windows: every other terminal window's tabs, in strip order,
    /// join the key window's at its end, the key window's selection staying
    /// where it was; each emptied window closes — it holds no tab, so no shell
    /// ends with it. Nothing moves if any of the windows holds a question of
    /// its own (a beep).
    pub(crate) fn merge_all_windows(&self) {
        let Some(into) = self.key_window().or_else(|| {
            self.front_terminal_window()
                .and_then(|window| self.window_owning(&window))
        }) else {
            return;
        };
        let others: Vec<Retained<TerminalWindow>> = self
            .windows()
            .into_iter()
            .filter(|window| window.id() != into.id())
            .collect();
        if others.is_empty() {
            return;
        }
        if !into.selection_free() || others.iter().any(|window| !window.selection_free()) {
            crate::preview::beep();
            return;
        }
        for other in &others {
            // Strip order in, the selected tab last out: it is the one that
            // leaves the screen, and the window it leaves is closing anyway.
            let tabs = other.tabs();
            let selected = other.selected_tab().id();
            let mut moved: Vec<Retained<TerminalTab>> = Vec::new();
            for tab in tabs.iter().filter(|tab| tab.id() != selected) {
                moved.extend(other.release_tab(tab.id()));
            }
            moved.extend(other.release_tab(selected));
            moved.sort_by_key(|tab| tabs.iter().position(|first| first.id() == tab.id()));
            for tab in &moved {
                into.adopt_tab(tab, Placement::End);
            }
            // Out of the list now, not a turn after it closes
            // ([`AppDelegate::forget_window`]): a delayed save that fires in
            // between must not walk a window without a tab. `other` keeps it
            // alive through its closing.
            self.unlist_window(other.id());
            other.close();
        }
        into.refresh_title();
        into.select();
        self.layout_changed();
    }

    /// The bar's `+` in window `window` (`tab_bar::TabBar`): ⌘T's job there.
    pub(crate) fn new_tab_in(&self, window: u64) {
        if let Some(window) = self.window(window) {
            self.open_window_or_report(Some(&window), Opening::Tab);
        }
    }

    /// The new pane's birth package and its theme — the single source
    /// for both the window-spawning path and splitting. All inputs are here, the pane
    /// does not reach into `AppDelegate`. The pane identity comes from the same counter as
    /// windows' and tabs' (one namespace); its owner is tab `tab`'s [`TabHost`].
    ///
    /// `from` is the source of inheritance (the focused pane): the OSC 7 directory (home
    /// if none), the point-size delta, the theme and the remote line ([`initial_line`]);
    /// without `from` the theme is resolved from settings. The integration is asked **once**
    /// and gives both answers at once (environment + dock share).
    fn pane_launch(
        &self,
        tab: u64,
        from: Option<&TerminalPane>,
        opening: Opening,
    ) -> (PaneLaunch, Theme) {
        let session = from.and_then(|from| from.session());
        let theme = session.map_or_else(|| self.resolve_theme(), |session| session.theme());
        let dir = session
            .and_then(|session| session.working_directory())
            .or_else(child::working_directory);
        let initial = initial_line(opening, session.and_then(|session| session.remote_line()));
        let launch = PaneLaunch {
            id: self.next_id(),
            run: self.ivars().run,
            host: Rc::new(TabHost::new(tab)),
            lookup: pane_by_id,
            stats: self.stats(),
            settings: self.settings().clone(),
            theme,
            launch: Launch {
                working_directory: dir,
                initial_input: initial.map(InitialInput::run),
                tab_id: None,
                replay: None,
                adopt: None,
            },
            integration: self.shell_integration(),
            reduce_motion: self.reduce_motion(),
            smooth_scroll: self.smooth_scroll(),
            scrollbar: self.scrollbar_mode(),
            zoom: from.map_or_else(Zoom::default, TerminalPane::zoom),
            masters: self.ivars().masters.clone(),
            keeper: self.ivars().keeper.clone(),
        };
        (launch, theme)
    }

    /// ⌘D / ⇧⌘D (`TerminalWindow`'s `splitRight:`/`splitDown:`, through
    /// [`TerminalTab::split`]): a new pane next to `from` in `tab`, with
    /// `from`'s inheritance ([`Opening::Split`]). The error goes to
    /// stderr; the tab stays open — the other panes' shells must not die because a new one
    /// could not be born.
    pub(crate) fn open_split(&self, tab: &TerminalTab, from: &TerminalPane, axis: Axis) {
        let (launch, _) = self.pane_launch(tab.id(), Some(from), Opening::Split);
        if let Err(e) = tab.add_pane(self.mtm(), launch, from.id(), axis) {
            eprintln!("bateri: {e}");
        }
    }

    /// A new window or tab derived from the active window (⌘N, ⌘T, ⌥⌘T).
    fn open_from_key_window(&self, opening: Opening) {
        let from = self.key_window();
        self.open_window_or_report(from.as_deref(), opening);
    }

    /// A new tab ([`AppDelegate::open_tab`]) or window
    /// ([`AppDelegate::open_window`]), with the error to stderr — the path of
    /// ⌘N/⌘T/`+`/Dock. A tab request with a window is a tab in it, and so
    /// is ⌘N when the system prefers tabs ([`prefers_tabs`]); otherwise a
    /// window. The process does **not** exit: the other windows' shells must
    /// not die because a new one could not be born (only the first window
    /// exits, `didFinishLaunching`).
    fn open_window_or_report(&self, from: Option<&TerminalWindow>, opening: Opening) {
        let result = match from {
            Some(from) if opening != Opening::Window || prefers_tabs(self.mtm(), from) => {
                self.open_tab(from, opening)
            }
            _ => self.open_window(from, opening).map(drop),
        };
        if let Err(e) = result {
            eprintln!("bateri: {e}");
        }
    }

    /// Launch's first windows: the holders' programs if any came
    /// ([`AppDelegate::restore_arrival`]), else the saved session if at least
    /// one of its windows comes back, otherwise today's single window. The
    /// gate order is [`AppDelegate::take_saved`]'s.
    ///
    /// Two launches skip both: one with ⇧ held ([`shift_held_at_launch`] — the
    /// bound holders were not even asked, they wait for the next launch; only
    /// an update's holder, which cannot wait, came) and the third attempt at
    /// holders that crashed two launches before it
    /// ([`restore::AttemptMode::GiveUp`]: their programs end). Neither reads
    /// the saved session; the directory's lock is still taken for the save at
    /// quit.
    fn restore_or_open(&self) -> Result<(), String> {
        let arrival = self.ivars().arrival.take();
        let mut safe = false;
        if let Some(arrival) = arrival {
            self.ivars().attempt_marks.replace(arrival.marked.clone());
            match restore::attempt_mode(arrival.attempt) {
                restore::AttemptMode::GiveUp => {
                    eprintln!(
                        "bateri: restoring crashed bateri twice; the programs it kept end here"
                    );
                    arrival.release_all();
                    return self.open_unrestored();
                }
                mode => {
                    safe = mode == restore::AttemptMode::Safe;
                    if self.restore_arrival(arrival, safe) {
                        return Ok(());
                    }
                }
            }
        }
        if self.ivars().skip_restore {
            return self.open_unrestored();
        }
        if let Some(saved) = self.take_saved(!safe)
            && self.restore_saved(&saved, None)
        {
            return Ok(());
        }
        self.open_window(None, Opening::Window).map(drop)
    }

    /// Today's single window, with nothing restored and the saved session
    /// left unread — its lock taken for the save at quit.
    fn open_unrestored(&self) -> Result<(), String> {
        let lock = self.restore_lock();
        self.ivars().restore_lock.replace(lock);
        self.open_window(None, Opening::Window).map(drop)
    }

    /// The session directory's lock ([`restore_dir`], [`restore::lock`]);
    /// `None` in a timed run, an unbundled process, without a home, or while
    /// another instance holds it.
    fn restore_lock(&self) -> Option<restore::Lock> {
        restore_dir(
            &self.inputs(),
            || {
                NSBundle::mainBundle()
                    .bundleIdentifier()
                    .map(|id| id.to_string())
            },
            child::home,
        )
        .and_then(|dir| restore::lock(&dir))
    }

    /// The first windows from the holders: every holder's layout with its own
    /// windows, the newest first, each pane carried on or fallen back
    /// ([`AppDelegate::restored_pane_launch`]) — a program two layouts name
    /// (an older holder's, and the holder of the bateri that took it and
    /// crashed) comes once, where the newest places it
    /// ([`Saved::place_after`]); a pane is looked up among every holder's,
    /// since the copy kept of a repeated program is the older holder's.
    /// Once every window is
    /// built the holders are acknowledged — the panes nobody placed released,
    /// a layout that does not read among them — and the session restore save,
    /// which describes the same session, is deleted. The session directory's
    /// lock is taken here for the save at quit.
    ///
    /// `safe` is the second attempt at the same holders
    /// ([`restore::AttemptMode::Safe`]): the programs are taken but nothing is
    /// replayed ([`adopted_screen`], [`fallen_replay`]).
    ///
    /// `false` (the holders hang everything up) if no layout reads or no
    /// window comes back; the caller goes on with session restore's path.
    fn restore_arrival(&self, mut arrival: Arrival, safe: bool) -> bool {
        let mut taken = Vec::new();
        let layouts: Vec<(usize, Saved)> = arrival
            .holders
            .iter()
            .enumerate()
            .filter_map(|(link, holder)| {
                Some((link, Saved::parse(&holder.layout)?.place_after(&mut taken)))
            })
            .collect();
        if layouts.is_empty() {
            arrival.release_all();
            return false;
        }
        let lock = self.restore_lock();
        self.ivars().restore_lock.replace(lock);
        let deliberate = deliberate_holders(&arrival);
        let mut arriving = Arriving {
            arrival: &mut arrival,
            safe,
            layout_of: 0,
            deliberate,
        };
        let mut restored = false;
        let mut key = None;
        for (link, saved) in &layouts {
            arriving.layout_of = *link;
            let (built, layout_key) = self.restore_windows(saved, Some(&mut arriving));
            restored |= built;
            key = key.or(layout_key);
        }
        if let Some(key) = key {
            key.select();
        }
        // The test hook of the attempt marker: a crash in the middle of a
        // restore, after the programs were taken and before their holders
        // were acknowledged — the next launches must find them waiting. A
        // launch argument only, never a stored default: a key left behind
        // would crash every restore until the programs were given up.
        if std::env::args().any(|arg| arg == "-BateriCrashDuringRestore") {
            // SAFETY: a signal to this process; `SIGKILL` is the crash it stands for.
            unsafe { libc::kill(libc::getpid(), libc::SIGKILL) };
        }
        if !restored {
            arrival.release_all();
            // `take_saved` takes the lock again.
            self.ivars().restore_lock.replace(None);
            return false;
        }
        arrival.finish();
        // The holders are acknowledged and gone: the carried-on panes are
        // the bound holder's to drain from now on.
        if let Some(keeper) = &self.ivars().keeper {
            keeper.confirm_taken();
        }
        if let Some(lock) = self.ivars().restore_lock.borrow().as_ref()
            && let Err(error) = restore::clear(lock)
        {
            eprintln!("bateri: could not delete the saved session: {error}");
        }
        true
    }

    /// Takes the saved layout. Gates in order, each falling to
    /// today's single window: a timed run, an unbundled process, an
    /// unresolvable home ([`restore_dir`]); the directory's lock held by
    /// another instance; `restore_windows = "off"` — which also deletes what
    /// is left, once the lock is ours; no or an unreadable layout
    /// ([`restore::take`] deletes it before anything is replayed). Under
    /// `"layout"` — or without `histories` (the second attempt at holders, whose
    /// fallback must not replay either) — the layout comes back but an
    /// earlier `"all"` save's histories are deleted unread. The lock
    /// stays in [`Ivars::restore_lock`] for the save at quit.
    fn take_saved(&self, histories: bool) -> Option<Saved> {
        let lock = self.restore_lock()?;
        let saved = match self.settings().restore_windows {
            RestoreWindows::Off => {
                let _ = restore::clear(&lock);
                None
            }
            RestoreWindows::All => restore::take(&lock, histories),
            RestoreWindows::Layout => restore::take(&lock, false),
        };
        self.ivars().restore_lock.replace(Some(lock));
        saved
    }

    /// Builds the saved windows ([`AppDelegate::restore_windows`]) and
    /// selects, last, the key window. `true` if at least one window came
    /// back.
    fn restore_saved(&self, saved: &Saved, arriving: Option<&mut Arriving<'_>>) -> bool {
        let (restored, key) = self.restore_windows(saved, arriving);
        if let Some(key) = key {
            key.select();
        }
        restored
    }

    /// The saved windows: per window the first tab that can be built at
    /// its frame (clamped onto a visible screen, [`clamp_frame`]) and the
    /// rest joining it as tabs, in order — each placed before its shells
    /// start ([`TerminalWindow::restore`], [`TerminalWindow::restore_tab`]);
    /// then every window's saved selected tab. A tab that cannot be built is
    /// skipped (its error to stderr). `true` if at least one window came
    /// back, and the key window for the caller to bring forward last.
    ///
    /// A save from the time a window's tabs were macOS's own reads the same:
    /// its tab group was already one saved window, and it comes back as one
    /// window with those tabs.
    fn restore_windows(
        &self,
        saved: &Saved,
        mut arriving: Option<&mut Arriving<'_>>,
    ) -> (bool, Option<Retained<TerminalWindow>>) {
        let screens = self.visible_frames();
        let mut key = None;
        let mut restored = false;
        for window in &saved.windows {
            let Some((built, tabs)) =
                self.restore_window(window, &screens, arriving.as_deref_mut())
            else {
                continue;
            };
            let selected = tabs
                .iter()
                .find(|(index, _)| *index == window.selected)
                .or_else(|| tabs.first())
                .map(|(_, tab)| *tab);
            if let Some(selected) = selected {
                built.select_tab(selected);
            }
            restored = true;
            if window.key {
                key = Some(built);
            }
        }
        (restored, key)
    }

    /// One saved window: its tabs built in order into one window; the
    /// return pairs each built tab's id with its index in `window.tabs`. A
    /// pane that does not come back ([`AppDelegate::restored_pane_launch`]'s
    /// `None`) leaves its tab without it ([`restore::SavedTab::retain`]), a
    /// tab left with none is not built; `None` if no tab was.
    fn restore_window(
        &self,
        window: &SavedWindow,
        screens: &[Frame],
        mut arriving: Option<&mut Arriving<'_>>,
    ) -> Option<RestoredWindow> {
        let mtm = self.mtm();
        let frame = clamp_frame(window.frame, screens);
        let frame = NSRect::new(
            NSPoint::new(frame.x, frame.y),
            NSSize::new(frame.width, frame.height),
        );
        let mut built: Option<Retained<TerminalWindow>> = None;
        let mut tabs: Vec<(usize, u64)> = Vec::new();
        for (index, tab) in window.tabs.iter().enumerate() {
            let tab_id = self.next_id();
            let mut theme = None;
            let launches: Vec<Option<PaneLaunch>> = tab
                .panes
                .iter()
                .map(|pane| {
                    let (launch, pane_theme) =
                        self.restored_pane_launch(tab_id, pane, arriving.as_deref_mut())?;
                    theme.get_or_insert(pane_theme);
                    Some(launch)
                })
                .collect();
            let keep: Vec<bool> = launches.iter().map(Option::is_some).collect();
            let Some(tab) = tab.retain(&keep) else {
                continue;
            };
            let launches: Vec<PaneLaunch> = launches.into_iter().flatten().collect();
            let theme = theme.unwrap_or_else(|| self.resolve_theme());
            let edge = self.settings().content_edge;
            let result = match built.clone() {
                Some(window) => window
                    .restore_tab(mtm, tab_id, &tab, launches, (theme, edge))
                    .map(|_| ()),
                None => {
                    let id = self.next_id();
                    TerminalWindow::restore(mtm, id, tab_id, &tab, launches, |this| {
                        // `open_window`'s order: notice, list, chrome, then shown.
                        this.set_notice(&self.ivars().notices.borrow().subtitle());
                        self.ivars().windows.borrow_mut().push(this.clone());
                        this.set_theme(theme);
                        this.set_content_edge(edge);
                        this.show_at(frame);
                    })
                    .map(|this| built = Some(this))
                }
            };
            match result {
                Ok(()) => tabs.push((index, tab_id)),
                Err(e) => eprintln!("bateri: could not restore a tab: {e}"),
            }
        }
        built.map(|window| (window, tabs))
    }

    /// A saved pane's birth package: [`AppDelegate::pane_launch`]'s without a
    /// `from` ([`Opening::Restore`]), with the save's start
    /// ([`restored_launch`]: directory, identity, ready remote line, history
    /// read and deleted here) and point-size step.
    ///
    /// With `arriving` the pane a holder gave under the same identity is
    /// carried on if it can be ([`adoption`], its screen [`adopted_screen`]);
    /// one that cannot — or that the old bateri could not freeze or register
    /// — falls back to a new shell with the note of what happened
    /// ([`fallen_note`]) and the history `restore_windows` allows
    /// ([`fallen_replay`]), and the holder hangs a refused one up at once.
    /// `None`: the pane does not come back — `restore_windows = "off"` keeps
    /// no pane without its program.
    fn restored_pane_launch(
        &self,
        tab: u64,
        pane: &SavedPane,
        arriving: Option<&mut Arriving<'_>>,
    ) -> Option<(PaneLaunch, Theme)> {
        let setting = self.settings().restore_windows;
        let saved_history = || {
            if pane.history {
                self.ivars()
                    .restore_lock
                    .borrow()
                    .as_ref()
                    .and_then(|lock| restore::history(lock, &pane.tab_id))
            } else {
                None
            }
        };
        let mut adopt = None;
        let replay = match arriving {
            None => saved_history(),
            Some(arriving) => {
                let held = arriving
                    .arrival
                    .panes
                    .iter()
                    .position(|(_, held)| held.tab == pane.tab_id)
                    .map(|index| arriving.arrival.panes.remove(index));
                let safe = arriving.safe;
                // A history the setting throws away is not read from disk.
                let fallen = |note: Note, history: Option<Vec<u8>>| {
                    let wanted = setting == RestoreWindows::All && !safe;
                    let history = history.or_else(|| wanted.then(saved_history).flatten());
                    fallen_replay(setting, safe, history, note)
                };
                match held {
                    None => {
                        let note = fallen_note(arriving.kind(arriving.layout_of), false);
                        Some(fallen(note, None)?)
                    }
                    Some((link, held)) => {
                        let kind = arriving.kind(link);
                        let cut = held.cut;
                        match adoption(held, jobs::exit_fd) {
                            Ok(mut adopted) => {
                                adopted.taken_from = arriving
                                    .arrival
                                    .holders
                                    .get(link)
                                    .map(|holder| holder.socket.clone());
                                // An emptied state blob must not refuse.
                                adopted.mode = if safe { AdoptMode::Bound } else { kind.mode() };
                                adopted.note = fallen_note(kind, false);
                                adopted.nudge = adopted_screen(&mut adopted, cut, safe);
                                if setting != RestoreWindows::All || safe {
                                    adopted.state.history.clear();
                                }
                                adopt = Some(adopted);
                                None
                            }
                            Err(refused) => {
                                let Refused {
                                    pane: held,
                                    history,
                                    ended,
                                } = *refused;
                                arriving.arrival.release(link, held);
                                Some(fallen(fallen_note(kind, ended), history)?)
                            }
                        }
                    }
                }
            }
        };
        let (mut launch, theme) = self.pane_launch(tab, None, Opening::Restore);
        launch.launch = restored_launch(pane, replay);
        launch.launch.adopt = adopt;
        launch.zoom = Zoom::from_steps(pane.zoom_steps, &launch.settings.font);
        Some((launch, theme))
    }

    /// Session restore's save, at the head of
    /// [`AppDelegate::shutdown`], before any pane closes. **One-shot**: it
    /// takes the lock — a second `shutdown` would find sessionless panes and
    /// overwrite the save with nothing. `restore_windows = "off"` deletes what
    /// is left; `"layout"` saves without scrollback; `"all"` with it. No
    /// window → nothing to restore, deleted ([`restore::save`]). Without a lock
    /// (timed run, unbundled, another instance) nothing is touched.
    fn save_session(&self) {
        let Some(lock) = self.ivars().restore_lock.take() else {
            return;
        };
        let setting = self.settings().restore_windows;
        let result = match setting {
            RestoreWindows::Off => restore::clear(&lock),
            RestoreWindows::All | RestoreWindows::Layout => {
                let (saved, histories) = self.saved_session(setting == RestoreWindows::All);
                restore::save(&lock, &saved, &histories)
            }
        };
        if let Err(e) = result {
            eprintln!("bateri: could not save the session: {e}");
        }
    }

    /// The frontmost of our windows on the current space (z-order from
    /// `windowNumbersWithOptions:`), for [`AppDelegate::saved_session`] when
    /// neither the key nor the main window is ours.
    fn front_terminal_window(&self) -> Option<Retained<NSWindow>> {
        let numbers = NSWindow::windowNumbersWithOptions(NSWindowNumberListOptions(0), self.mtm())?;
        let windows = self.windows();
        numbers.iter().find_map(|number| {
            windows
                .iter()
                .find(|window| window.ns_window().windowNumber() == number.integerValue())
                .map(|window| window.ns_window().retain())
        })
    }

    /// The live windows as the save's model: one saved window per window,
    /// in the window list's order; its tabs in strip order, its selected tab
    /// and whether it is the front terminal window (`keyWindow`, else
    /// `mainWindow` — the first of them that is ours —, else our frontmost
    /// window, [`AppDelegate::front_terminal_window`]: ⌘Q's alert can leave
    /// no key window and the Settings window can be key and main). A tab
    /// with nothing live to save is left out, a window without tabs too
    /// ([`TerminalTab::saved_tab`]).
    fn saved_session(&self, with_history: bool) -> (Saved, Histories) {
        let app = NSApplication::sharedApplication(self.mtm());
        let key = app
            .keyWindow()
            .into_iter()
            .chain(app.mainWindow())
            .find(|window| self.window_owning(window).is_some())
            .or_else(|| self.front_terminal_window());
        let mut windows = Vec::new();
        let mut histories = Vec::new();
        for window in self.windows() {
            // A window that gave up its last tab and is closing has none.
            let Some(selected) = window.try_selected_tab().map(|tab| tab.id()) else {
                continue;
            };
            let mut tabs = Vec::new();
            let mut selected_index = 0;
            for live in window.tabs() {
                let Some((tab, tab_histories)) = live.saved_tab(with_history) else {
                    continue;
                };
                if live.id() == selected {
                    selected_index = tabs.len();
                }
                histories.extend(tab_histories);
                tabs.push(tab);
            }
            if tabs.is_empty() {
                continue;
            }
            let rect = window.ns_window().frame();
            windows.push(SavedWindow {
                frame: Frame {
                    x: rect.origin.x,
                    y: rect.origin.y,
                    width: rect.size.width,
                    height: rect.size.height,
                },
                tabs,
                selected: selected_index,
                key: key.as_deref().is_some_and(|key| window.owns(key)),
            });
        }
        (Saved { windows }, histories)
    }

    /// The quiet stamp of the timed run's measured pane (`quiet=`).
    fn quiet_since(&self) -> Option<Duration> {
        self.measured_pane()
            .and_then(|pane| pane.link().and_then(DisplayLink::quiet_since))
    }

    /// The first window's selected tab's focused pane — the launch's only
    /// pane in a timed run, before the smoke run's second tab.
    fn first_pane(&self) -> Option<Retained<TerminalPane>> {
        self.windows()
            .first()
            .map(|window| window.selected_tab().focused_pane())
    }

    /// The pane with id `id`, **closing or not**: the report reads its
    /// counters after shutdown has begun every pane's closing, which
    /// [`AppDelegate::pane`] would skip.
    fn timed_pane(&self, id: u64) -> Option<Retained<TerminalPane>> {
        self.all_panes().into_iter().find(|pane| pane.id() == id)
    }

    /// The timed run's measured pane ([`Ivars::measured`]); `None` before
    /// it is born — or if it never was, and then the report's counters are
    /// zero and the gate says `MissingCounter`.
    fn measured_pane(&self) -> Option<Retained<TerminalPane>> {
        self.timed_pane(self.ivars().measured.get()?)
    }

    /// Arms the timed run's deadline (`runDeadline:`), `run.seconds` from
    /// now — when the measured pane is born, so its timeline to the deadline
    /// is the same in both workloads.
    fn arm_deadline(&self, run: Run) {
        // The timer is not a block but `performSelector`: the selector is
        // in this class and needs no cancelling.
        // SAFETY: `runDeadline:` is defined in this class and takes a single
        // Option<&AnyObject> argument. Delegate properties are weak
        // references; what keeps self alive is the `Retained` in `run()`,
        // which outlives `app.run()`. The timer also holds its target itself.
        // Common modes: live resizing puts the run loop in tracking mode,
        // a timer set up in the default mode would be postponed there.
        unsafe {
            self.performSelector_withObject_afterDelay_inModes(
                sel!(runDeadline:),
                None,
                run.seconds as f64,
                &NSArray::from_slice(&[NSRunLoopCommonModes]),
            );
        }
    }

    /// The smoke run's first step: the launch's tab — the background one —
    /// draws its first content frame **selected**, and only then does the
    /// measured tab open over it ([`AppDelegate::open_measured_tab`]). After
    /// that frame the background tab's zero is its hiding's doing, not a
    /// link that never ran.
    ///
    /// The notifier only queues the opening: it is told from inside that
    /// link's tick, and the opening hides that very link.
    ///
    /// **A backstop bounds the run:** a link that never draws (a surface
    /// never sized, a window occluded from birth) never tells, and the
    /// deadline is armed only when the measured tab opens — so the opening
    /// also comes `run.seconds` after launch, whichever is first, and the
    /// run stays within twice its seconds instead of waiting on the recipe's
    /// shell to exit. A pane without a link (no session) opens at once.
    fn open_measured_after_first_frame(&self) {
        let Some(run) = self.ivars().run else {
            return;
        };
        let pane = self.first_pane();
        let Some(link) = pane.as_deref().and_then(TerminalPane::link) else {
            self.open_measured_tab();
            return;
        };
        let open = || {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(app) = delegate(mtm) {
                app.open_measured_tab();
            }
        };
        link.on_first_content_frame(Box::new(move || DispatchQueue::main().exec_async(open)));
        if let Ok(when) = DispatchTime::try_from(Duration::from_secs(run.seconds)) {
            let _ = DispatchQueue::main().after(when, open);
        }
    }

    /// The smoke run's second step: the measured tab opens by ⌘T's own path
    /// (the bar's `+`, [`AppDelegate::new_tab_in`]) and the launch's tab
    /// goes behind it, hidden; its main-thread frames and damage notices
    /// are noted at that moment ([`Hidden`]) — **after** the switch, so the
    /// notice its own focus change plants while leaving the screen is not
    /// its witness. The deadline counts from here, the measured tab's birth:
    /// its timeline to the deadline is the single tab's of before. The
    /// measured tab is never hidden.
    ///
    /// If the tab could not open, nothing is measured: the counters read
    /// zero and the gate says `MissingCounter` at the same deadline.
    fn open_measured_tab(&self) {
        let Some(run) = self.ivars().run else {
            return;
        };
        if self.ivars().measured_asked.replace(true) {
            return;
        }
        let window = self.windows().first().cloned();
        let opened = window.is_some_and(|window| {
            let background = window.selected_tab().focused_pane();
            self.new_tab_in(window.id());
            let measured = window.selected_tab().focused_pane();
            if measured.id() == background.id() {
                return false;
            }
            self.ivars().measured.set(Some(measured.id()));
            let hidden = background.link().map(|link| Hidden {
                pane: background.id(),
                frames: drawn_frames(link),
                wakes: link.requests(),
            });
            self.ivars().background.set(hidden);
            true
        });
        // The counters will read zero and say only that; this says why.
        if !opened {
            eprintln!("bateri: the smoke run's measured tab did not open");
        }
        self.arm_deadline(run);
    }

    /// The current settings — the path windows read. The borrow must be kept short:
    /// while the save-time path writes (`replace`), an open borrow ends in a panic.
    pub(crate) fn settings(&self) -> Ref<'_, Settings> {
        self.ivars().settings.borrow()
    }

    /// The copy of the measurement ledger that goes to the window (the link).
    pub(crate) fn stats(&self) -> Option<Arc<Stats>> {
        self.ivars().stats.clone()
    }

    /// The new session's shell integration: the environment to add to the child **and** the dock
    /// share, from a single question ([`shell_integration_env`], [`dock_rows_at_birth`]).
    ///
    /// Both keys from **one borrow**: a reload falling between separate `borrow()`s
    /// could read the two from different files.
    pub(crate) fn shell_integration(&self) -> (Vec<(String, String)>, u16) {
        let setting = self.ivars().settings.borrow().shell_integration;
        let integration = with_bateri_bin(
            shell_integration_env(
                &self.inputs(),
                setting,
                child::shell,
                child::zsh_wrapper_dir,
                std::env::var_os("ZDOTDIR"),
            ),
            std::env::current_exe().ok(),
            self.ivars().masters.as_deref().map(Masters::instance),
        );
        let birth = dock_rows_at_birth(&integration, setting);
        (integration, birth)
    }

    /// Reads the settings at launch, writes them to [`Ivars::settings`] and hands the diagnostics
    /// to the subtitle source by source. The first window resolves the theme
    /// ([`AppDelegate::resolve_theme`]).
    ///
    /// In a timed run the loader is **never called** ([`Inputs::Hermetic`]) and the
    /// theme is the embedded `bateri`, without reading the appearance: `Settings::default()` is now
    /// `"system"` and had it been resolved to that, smoke would be affected by the machine's light mode.
    /// The font also stays at the renderer's launch value, i.e. `FontOptions::default()`
    /// — `cells=8 glyphs=6` is not tied to the machine's settings file.
    /// A broken file leaves the window open, with defaults
    /// ([`settings::Loaded::at_launch`], [`AppDelegate::choose_theme`]).
    ///
    /// The font goes to the window's renderer **only as a request** while the window is born
    /// ([`AppDelegate::open_window`] → [`TerminalPane::request_font`]):
    /// the atlas is opened in the `sync_geometry` that follows immediately and
    /// that also writes the font slot.
    ///
    /// The watching is set up here too, **before** reading (`watch` → setup is a
    /// one-shot): a save falling between launch and the first event must not be lost.
    fn load_settings(&self) {
        let Inputs::User { config_root } = self.inputs() else {
            return;
        };
        // The home directory could not be resolved: the file cannot be looked up and this must be
        // visible too — nobody sees stderr on a Dock launch, and the user's settings
        // would be silently ignored. The rule for an unreadable file
        // (`Loaded::at_launch`): the file may contain `osc52 = "off"`, the
        // clipboard falls to off.
        let (settings, messages) = match &config_root {
            Some(root) => {
                self.watch_config(root);
                let loaded = settings::load(root);
                self.ivars().settings_state.replace(loaded.state());
                loaded.at_launch()
            }
            None => (
                Settings::for_unusable_file(),
                vec![format!(
                    "home directory not found; {} is not read",
                    settings::FILE_NAME
                )],
            ),
        };
        self.post_notices(Source::Settings, messages);
        self.ivars().settings.replace(settings);
    }

    /// Sweeps the preview cache on a background thread with
    /// the settings as they are now: `Launch` at startup, `Daily` from
    /// [`AppDelegate::schedule_daily_sweep`] and `ClearNow` — the single method
    /// the settings window's Clear Now calls. Edited copies it moved to
    /// the download folder are reported on the main thread
    /// ([`crate::preview::report_rescued`]) and the settings window's usage is
    /// measured again ([`AppDelegate::measure_preview_usage`]). Nothing in a timed run.
    pub(crate) fn sweep_previews(&self, sweep: Sweep) {
        if !matches!(self.inputs(), Inputs::User { .. }) {
            return;
        }
        let files = self.ivars().settings.borrow().remote_files.clone();
        let home = child::home();
        let (Some(dir), Some(downloads)) = (
            bt_core::expand_home(&files.preview_dir, home.as_deref()),
            bt_core::expand_home(&files.download_dir, home.as_deref()),
        ) else {
            return;
        };
        let _ = std::thread::Builder::new()
            .name("preview sweep".into())
            .spawn(move || {
                let report = preview_cache::sweep(
                    &dir,
                    &downloads,
                    sweep,
                    files.preview_keep,
                    files.preview_limit,
                    preview_cache::now(),
                );
                DispatchQueue::main().exec_async(move || {
                    // audit: a block running on the main queue is by definition on the main thread.
                    let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
                    if let Some(app) = delegate(mtm) {
                        app.measure_preview_usage();
                    }
                    if report.rescued.is_empty() {
                        return;
                    }
                    // The application's own report: a window-wide seat, the key window's.
                    let key = delegate(mtm).and_then(|app| app.question_window());
                    let seat = key
                        .as_deref()
                        .and_then(|key| crate::sheets::seat(crate::sheets::Asker::Window(key)));
                    crate::preview::report_rescued(mtm, seat, &report.rescued, || {});
                });
            });
    }

    /// Measures the preview folder for the settings window's "In use" row on a
    /// background thread (the scan blocks on the disk) and shows the answer on
    /// the main thread — only while the window is open; a closed window is
    /// measured again when it opens ([`AppDelegate::refresh_settings_window`]).
    fn measure_preview_usage(&self) {
        let Some(window) = self.ivars().settings_window.borrow().clone() else {
            return;
        };
        if !window.is_open() {
            return;
        }
        let generation = window.next_usage_generation();
        let text = self
            .ivars()
            .settings
            .borrow()
            .remote_files
            .preview_dir
            .clone();
        let Some(dir) = bt_core::expand_home(&text, child::home().as_deref()) else {
            window.show_usage(generation, None);
            return;
        };
        let _ = std::thread::Builder::new()
            .name("preview usage".into())
            .spawn(move || {
                let usage = preview_cache::usage(&dir);
                DispatchQueue::main().exec_async(move || {
                    // audit: a block running on the main queue is by definition on the main thread.
                    let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
                    let window =
                        delegate(mtm).and_then(|app| app.ivars().settings_window.borrow().clone());
                    if let Some(window) = window.filter(|window| window.is_open()) {
                        window.show_usage(generation, Some(usage));
                    }
                });
            });
    }

    /// The daily sweep (once a day, only what outlived `preview_keep`):
    /// one delayed block on the main queue that sweeps and sets up the next —
    /// a timer, not a frame; idle frames stay at zero. Nothing in a timed run.
    fn schedule_daily_sweep(&self) {
        if !matches!(self.inputs(), Inputs::User { .. }) {
            return;
        }
        let Ok(when) = DispatchTime::try_from(DAILY_SWEEP) else {
            return;
        };
        // The error arm is not represented today (the link clock's rationale): if
        // it drops, the next launch sweeps.
        let _ = DispatchQueue::main().after(when, || {
            // audit: a block running on the main queue is by definition on the main thread.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(app) = delegate(mtm) {
                app.sweep_previews(Sweep::Daily);
                app.schedule_daily_sweep();
            }
        });
    }

    /// The theme of a window born while no window exists: the theme the settings select,
    /// resolved for the appearance ([`AppDelegate::choose_theme`]). In a timed run
    /// the embedded `bateri` ([`Inputs::Hermetic`]).
    fn resolve_theme(&self) -> Theme {
        let Inputs::User { config_root } = self.inputs() else {
            return Theme::BATERI;
        };
        let settings = self.ivars().settings.borrow();
        self.choose_theme(config_root.as_deref(), &settings)
    }

    /// Resolves the theme the settings select for the current appearance and refreshes the theme slot
    /// — the **shared** path of launch and appearance change. An unusable
    /// theme is replaced by the embedded theme matching the appearance
    /// ([`settings::ThemeLoaded::or_embedded`]); keeping the on-screen theme
    /// would leave the other appearance's theme in place on an appearance change.
    ///
    /// The active theme file's source is also refreshed here, before reading:
    /// the name changes when the appearance changes and the old name's source
    /// would not see a write to its place in the new file.
    fn choose_theme(&self, config_root: Option<&Path>, settings: &Settings) -> Theme {
        let dark = self.dark_appearance();
        let name = settings.theme_for(dark);
        self.watch_theme(config_root, name);
        let (theme, messages) = settings::load_theme(config_root, name).or_embedded(dark);
        self.post_notices(Source::Theme, messages);
        theme
    }

    /// Live reload: a watch source has reported. "Settings…" also comes here after
    /// creating the directory ([`AppDelegate::edit_settings`]) —
    /// no source sees a directory created later (`watch`).
    ///
    /// Order, three rules:
    /// - **Set up first, then read** ([`AppDelegate::watch_config`],
    ///   [`AppDelegate::watch_theme`]); all are rebuilt on every event, and this is
    ///   how the stale handle of a moved file or a deleted directory is dropped.
    /// - **The settings file** ([`settings::Loaded::live`]): from an unusable or momentarily
    ///   missing file nothing is applied and [`Ivars::settings`]
    ///   does not change; the slot fills or empties according to its own source. Otherwise
    ///   a key that is not accepted keeps its current value
    ///   ([`settings::load_keeping`]) and the diff is taken: terminal options go
    ///   **entirely** to the session; the font goes to the renderer with the window's temporary point-size delta
    ///   ([`TerminalPane::apply_font`]) — with the delta reset if `size`
    ///   changed ([`Zoom::after_reload`]); the cursor style goes to the link
    ///   ([`bt_gpu::DisplayLink::set_cursor_motion`]). Once the file is read and applied the write
    ///   slot is emptied too: if the file Theme ▸ rejected was fixed, the rejection
    ///   slot is emptied too: if the file Theme ▸ rejected was fixed, the
    ///   rejection is no longer true.
    /// - **The theme is re-resolved on every event**, even if the settings file is broken
    ///   (with the name from the last good settings): the active theme file is a separate source and
    ///   which file reported is unknown. An unusable theme is not swapped in,
    ///   the on-screen one stays ([`settings::ThemeLoaded::or_current`]);
    ///   swapping in the same theme is a no-op, no frame is requested.
    ///
    /// No coalescing: one save yields several events (directory + file) and the
    /// later ones give an empty diff.
    fn reload_settings(&self) {
        let Inputs::User {
            config_root: Some(root),
        } = self.inputs()
        else {
            return;
        };
        // Every application goes to **every window**: the settings file is one and the windows
        // watch it together. If a window has no session yet (the sources
        // are set up inside `didFinishLaunching`, the event can land on the main queue only when that
        // returns), the methods look at their own slots and silently
        // return; this guards against a change of order.
        let windows = self.windows();
        let panes = self.all_panes();
        self.watch_config(&root);
        // A value that is not accepted comes from the current settings (`load_keeping`): a `scrollback`
        // saved with the wrong type must not truncate the history.
        let loaded = settings::load_keeping(&root, &self.ivars().settings.borrow());
        self.ivars().settings_state.replace(loaded.state());
        let (loaded, messages) = loaded.live();
        self.post_notices(Source::Settings, messages);
        if let Some(new) = loaded {
            let changes = {
                let old = self.ivars().settings.borrow();
                // The point-size delta is **per pane**
                // and is reset in every pane with the same rule.
                for pane in &panes {
                    pane.zoom_after_reload(&old.font, &new.font);
                }
                old.changes(&new)
            };
            if changes.terminal {
                for pane in &panes {
                    pane.set_terminal_options(&new);
                }
            }
            if changes.remote {
                for window in &windows {
                    window.set_host_marks(&new);
                }
            }
            // The load indicator's form and interval: `off`
            // hides it at once, another form redraws the last value.
            if changes.stats {
                for pane in &panes {
                    pane.set_stats_settings(&new.remote_stats);
                }
            }
            // The text's contrast floor: every pane's session draws its next
            // frame with it.
            if changes.contrast {
                for pane in &panes {
                    pane.set_minimum_contrast(new.minimum_contrast);
                }
            }
            // The listening ports start or stop at once in every pane.
            if changes.ports {
                for pane in &panes {
                    pane.set_ports_shown(new.shell_ports);
                }
            }
            // The style and the dock's typing effects go to the link, not the session:
            // they change not which frame we draw but **how** we draw it.
            // The link is born inside `start_session` and this path runs after
            // it, but the order is not a contract: if the slot is empty the
            // launch call will give the same value anyway.
            let motion_changed = changes.motion;
            if motion_changed {
                for pane in &panes {
                    pane.set_cursor_motion(&new);
                }
            }
            // The cursor's drawing numbers go to the link too, for the same reason: they change
            // **how** we draw, not which frame we draw.
            // `Changes::caret` is a separate field, because these do not go into
            // `TerminalOptions` and had they piggybacked on `changes.terminal`, a radius
            // change would have rebuilt the session from scratch.
            if changes.caret {
                for pane in &panes {
                    pane.apply_caret(&new);
                }
            }
            // `keep_running` is read when quitting; its live part is the
            // bound holder's birth or departure, from the old and new value.
            let before = self.settings().keep_running;
            let switch = keeper::switch(before, new.keep_running);
            let ask = keeper::asks_notification_permission(Some(before), new.keep_running);
            crate::menu::set_end_programs_visible(self.mtm(), new.keep_running);
            self.ivars().settings.replace(new);
            self.switch_keeper(switch);
            if ask {
                crate::uploader::request_notification_permission();
            }
            // **After** the settings are written: `apply_reduce_motion` is the shared path of
            // three callers and reads the value from the slot, not from the `new` in hand.
            // The style's path stayed separate because it takes the link directly;
            // merging the two would tie this path to `new` and make it
            // uncallable from the system notification.
            if motion_changed {
                self.apply_reduce_motion();
            }
            // **After** the delta is written: the pane applies the font with the new delta.
            if changes.font {
                let font = self.settings().font.clone();
                for pane in &panes {
                    pane.set_font(&font);
                }
            }
            // The scroll bar's form, by `apply_reduce_motion`'s reasoning: the
            // shared path reads the slot. Its note in the settings window is
            // refreshed at the end. A save that changes the font **and** moves
            // to or from `"always"` resizes the grid twice (each path is its
            // own geometry refresh); only a hand edit does that — the window
            // writes one key at a time.
            if changes.scrollbar {
                self.apply_scrollbar();
            }
            // The top edge's mode goes to every **window** — each tab's panes
            // and its container's line together (`TerminalTab::set_content_edge`).
            // A save that also changes the font resizes the grid twice, the
            // scroll bar's case above.
            if changes.content_edge {
                let edge = self.settings().content_edge;
                for window in &windows {
                    window.set_content_edge(edge);
                }
            }
            // The veil over unfocused splits: each tab reads the choice again.
            if changes.dim_splits {
                for window in &windows {
                    window.refresh_split_look();
                }
            }
            self.post_notices(Source::Write, Vec::new());
        }
        // The borrow is dropped before `set_theme`; the calls inside do not touch
        // `settings` (the pattern of `apply_appearance`).
        let theme = {
            let settings = self.ivars().settings.borrow();
            let name = settings.theme_for(self.dark_appearance());
            self.watch_theme(Some(&root), name);
            let (theme, messages) = settings::load_theme(Some(&root), name).or_current();
            self.post_notices(Source::Theme, messages);
            theme
        };
        if let Some(theme) = theme {
            for window in &windows {
                window.set_theme(theme);
            }
        }
        // The file may have changed from outside too (vnode): an open settings window
        // shows the file's state on every run.
        self.refresh_settings_window();
    }

    /// The settings window's "Open settings.toml" button (it was once
    /// bateri ▸ Settings… itself): creates the file from the template if missing
    /// ([`settings::create_if_missing`]), re-sets up the watching and reads, and
    /// opens the file in the editor ([`open_in_editor`]).
    ///
    /// - A **timed run** creates no file ([`Inputs::Hermetic`]); if the home directory
    ///   could not be resolved, the settings slot has said so since launch.
    /// - **Re-reading after creating** ([`AppDelegate::reload_settings`]):
    ///   if the directory was just born, no source saw it. The template
    ///   states the defaults; for a user without a file the diff is empty and the screen
    ///   does not change.
    /// - **The error goes to the write slot, after reading**: reading empties that slot once the file is
    ///   applied, so if written first it would be erased at once. The slot
    ///   is also shown in the settings window's strip (the button is there); the next successful
    ///   read or write empties it.
    pub(crate) fn edit_settings(&self) {
        let Inputs::User {
            config_root: Some(root),
        } = self.inputs()
        else {
            return;
        };
        let created = settings::create_if_missing(&root);
        self.reload_settings();
        let problem = match created {
            Err(err) => Some(format!(
                "{} could not be created: {err}",
                settings::FILE_NAME
            )),
            Ok(path) if !open_in_editor(&path) => Some(format!(
                "no editor could open {}; it is at {}",
                settings::FILE_NAME,
                path.display()
            )),
            Ok(_) => None,
        };
        // To the write slot: the settings window whose button was pressed shows that slot in its strip
        // — with no terminal window at all there is no subtitle either.
        // It is written after the read has emptied the slot, so it stays visible.
        if let Some(problem) = problem {
            self.post_notices(Source::Write, vec![problem]);
            self.refresh_settings_window();
        }
    }

    /// The View ▸ Theme ▸ choice: writes `theme` to the file
    /// ([`settings::write_edit`]) and applies it through **the path that reads the file**
    /// ([`AppDelegate::reload_settings`]) — the menu has no application path of its own,
    /// the only chain to the screen goes through the file.
    ///
    /// The read comes right after the write, without waiting for the watcher's event: if the
    /// directory was created a moment ago no source sees it (the rationale of "Settings…").
    /// The event that follows is an empty diff and a swap to the same theme, i.e. a no-op.
    ///
    /// The error goes to the **write slot**; a successful write empties the slot. A timed run
    /// does not write ([`Inputs::Hermetic`]); the menu is not populated in that branch anyway.
    fn save_theme(&self, name: &str) {
        self.save_edit(&SettingsEdit::Theme(name.to_owned()));
    }

    /// Writes a single key's new value to the file and applies it through the path that reads
    /// the file — the **shared** path of View ▸ Theme ▸ and the settings window
    /// (the rationale of [`AppDelegate::save_theme`] applies as is). On a write error the
    /// window returns to the file's value: the control must not show a value that could
    /// not be written.
    pub(crate) fn save_edit(&self, edit: &SettingsEdit) {
        let Inputs::User {
            config_root: Some(root),
        } = self.inputs()
        else {
            return;
        };
        match settings::write_edit(&root, edit) {
            Ok(()) => {
                self.post_notices(Source::Write, Vec::new());
                self.reload_settings();
            }
            Err(message) => {
                self.post_notices(Source::Write, vec![message]);
                self.refresh_settings_window();
            }
        }
    }

    /// bateri ▸ Settings…: spawns the settings window (the first time), fills it with the
    /// active settings and brings it to the front. In a timed run and when the home directory
    /// cannot be resolved it does **nothing** ([`Inputs::Hermetic`]): there is no file to
    /// write, and `make smoke` never sees the window.
    fn show_settings_window(&self) {
        let Inputs::User {
            config_root: Some(_),
        } = self.inputs()
        else {
            return;
        };
        let window = self
            .ivars()
            .settings_window
            .borrow_mut()
            .get_or_insert_with(|| SettingsWindow::new(self.mtm()))
            .clone();
        // Show first: refreshing skips a hidden window. Both are in the same main
        // queue turn, no frame is drawn in between.
        window.show();
        self.refresh_settings_window();
    }

    /// Fills the open (or hidden) settings window with the active settings; a no-op if the window
    /// was never born. The settings borrow is **copied** before entering the window:
    /// this path runs from a control's action (write → `reload_settings` → here)
    /// and the window can come back and reach the delegate.
    ///
    /// The file's state comes from [`Ivars::settings_state`], the write error from the subtitle's
    /// write slot: both are a single source, the window keeps no copy of its own.
    /// The write slot is emptied on a successful write and once the file is read and
    /// applied, i.e. the strip goes away at that moment too.
    ///
    /// A closed window is not refreshed: reading the theme directory on every save,
    /// rebuilding four popups and opening CoreText for a missing font would be work
    /// nobody sees; reopening refreshes
    /// ([`AppDelegate::show_settings_window`]).
    pub(crate) fn refresh_settings_window(&self) {
        let Some(window) = self.ivars().settings_window.borrow().clone() else {
            return;
        };
        if !window.is_open() {
            return;
        }
        let Inputs::User {
            config_root: Some(root),
        } = self.inputs()
        else {
            return;
        };
        let settings = self.ivars().settings.borrow().clone();
        let state = self.ivars().settings_state.borrow().clone();
        let write = self.ivars().notices.borrow().get(Source::Write).to_vec();
        let embedded: Vec<&str> = Theme::embedded_names().collect();
        let user = settings::user_theme_names(&root);
        let resolved = crate::settings_window::Resolved {
            reduce: self.reduce_motion(),
            scrollbar: self.scrollbar_mode(),
        };
        window.refresh(&settings, resolved, &state, &write, &embedded, &user);
        // On every refresh, not only on open: a changed `preview_dir` is
        // another folder. The scan is off the main thread.
        self.measure_preview_usage();
    }

    /// Re-sets up the settings directory's sources. The new one is set up before the old one is dropped
    /// (`replace`): there is no gap between the two setups.
    fn watch_config(&self, root: &Path) {
        let watch = Watch::install(&settings::watched_paths(root), &watch_notify());
        self.ivars().config_watch.replace(Some(watch));
    }

    /// Re-sets up the active theme file's source; if there is no home directory the slot
    /// is emptied.
    fn watch_theme(&self, config_root: Option<&Path>, name: &str) {
        let watch = config_root
            .map(|root| Watch::install(&[settings::theme_path(root, name)], &watch_notify()));
        self.ivars().theme_watch.replace(watch);
    }

    /// Starts watching the system's light/dark appearance — **only in a user
    /// session** ([`Inputs`]).
    ///
    /// The source is the KVO of `NSApp.effectiveAppearance`, **not** the view's
    /// `viewDidChangeEffectiveAppearance`: the window's chrome carries the theme's
    /// appearance ([`TerminalWindow::apply_chrome`]) and a window with its appearance set
    /// stops inheriting from the system — from then on the view never sees the
    /// system's change (measured), it saw only
    /// the one we set ourselves.
    ///
    /// The observer is **not removed**: both `AppDelegate` and `NSApp` live for the
    /// lifetime of the process (the precedent of [`AppDelegate::observe_reduce_motion`]).
    fn observe_appearance(&self) {
        let Inputs::User { .. } = self.inputs() else {
            return;
        };
        let app = NSApplication::sharedApplication(self.mtm());
        // SAFETY: the observer is this class and it implements `observeValueForKeyPath:…`;
        // the context is null, because this is the only path watched. Both objects live
        // for the process's lifetime, so the registration leaves no dangling observer.
        unsafe {
            app.addObserver_forKeyPath_options_context(
                self,
                ns_string!("effectiveAppearance"),
                NSKeyValueObservingOptions::empty(),
                std::ptr::null_mut(),
            );
        }
    }

    /// The applier of an appearance change: if the theme follows the system, selects the theme
    /// matching the appearance through the same path as launch ([`AppDelegate::choose_theme`])
    /// and swaps it into the session and the chrome ([`TerminalWindow::set_theme`]).
    ///
    /// Four gates, in order:
    /// - **Timed run** ([`Inputs::Hermetic`]): the appearance is ignored, the theme
    ///   stays `bateri` — `make smoke` is not affected by the machine's light mode.
    /// - **The light/dark bit did not change** ([`Ivars::appearance_dark`]).
    /// - **No window has a session:** the last window closed (the app stays
    ///   open) or the only window's session is not yet born; a window being born
    ///   selects the theme from the appearance itself ([`AppDelegate::open_window`]).
    /// - **Fixed theme** (`theme = "{name}"`): the appearance does not touch the theme.
    ///
    /// If the theme comes out the same (`light_theme` and `dark_theme` the same name) the swap is a no-op
    /// and no frame is requested
    /// (`Session::set_theme`). The theme slot is still rewritten: this read is that
    /// source's current state.
    fn apply_appearance(&self) {
        let Inputs::User { config_root } = self.inputs() else {
            return;
        };
        let dark = self.dark_appearance();
        if self.ivars().appearance_dark.replace(Some(dark)) == Some(dark) {
            return;
        }
        // The bit is written **before** the gate: a change arriving while there is no window
        // counts as seen too, and a window born later selects the theme from the
        // appearance anyway.
        let windows = self.windows();
        if self.all_panes().iter().all(|pane| pane.session().is_none()) {
            return;
        }
        // The borrow is dropped at the end of `choose_theme`; the `post_notices` there
        // borrows only `notices`, it does not touch `settings`.
        let theme = {
            let settings = self.ivars().settings.borrow();
            if !settings.follows_system() {
                return;
            }
            self.choose_theme(config_root.as_deref(), &settings)
        };
        for window in &windows {
            window.set_theme(theme);
        }
    }

    /// Starts watching for the Mac logging out, restarting or shutting down —
    /// only in a user session ([`Inputs`]) — so a quit at that moment leaves
    /// no reminder that the programs keep running ([`window::kept_notice`]).
    /// `NSWorkspace`'s own centre, not removed: the precedent of
    /// [`AppDelegate::observe_reduce_motion`].
    fn observe_power_off(&self) {
        let Inputs::User { .. } = self.inputs() else {
            return;
        };
        let center = NSWorkspace::sharedWorkspace().notificationCenter();
        // SAFETY: `workspaceWillPowerOff:` is defined on this class and takes a
        // single `Option<&AnyObject>` argument; `self` lives for the process's
        // lifetime, so the centre's non-owning reference does not dangle. The
        // constant `NSString` is a name AppKit exposes.
        unsafe {
            center.addObserver_selector_name_object(
                self,
                sel!(workspaceWillPowerOff:),
                Some(NSWorkspaceWillPowerOffNotification),
                None,
            );
        }
    }

    /// ⌘Q under `keep_running = "quit"` kept the programs: the reminder that
    /// they keep running goes to the system with a delay ([`NOTICE_DELAY`] —
    /// it must arrive after bateri is gone) and the quit waits, bounded
    /// ([`NOTICE_WAIT`]), for the system to take it. No permission is asked
    /// here; without one the system drops the request.
    fn leave_quit_notice(&self) {
        let Some(notice) = self.ivars().quit_notice.take() else {
            return;
        };
        if self.ivars().powering_off.get() {
            return;
        }
        if let Some(taken) =
            crate::uploader::schedule_notification(notice.title, &notice.body, NOTICE_DELAY)
        {
            let _ = taken.recv_timeout(NOTICE_WAIT);
        }
    }

    /// Starts watching the system's Reduce Motion setting — **only in a user
    /// session** ([`Inputs`]).
    ///
    /// The notification comes from `NSWorkspace`'s **own** centre, not the default
    /// `NSNotificationCenter`; Apple publishes it that way and subscribing to the wrong
    /// centre would mean silently never hearing anything.
    ///
    /// The observer is **not removed**: `AppDelegate` lives for the process's lifetime
    /// (the `Retained` in `run()`) and the centre holds it without owning it anyway.
    /// The light/dark appearance's KVO ([`AppDelegate::observe_appearance`]) is
    /// likewise not removed.
    fn observe_reduce_motion(&self) {
        let Inputs::User { .. } = self.inputs() else {
            return;
        };
        let center = NSWorkspace::sharedWorkspace().notificationCenter();
        // SAFETY: `accessibilityDisplayDidChange:` is defined on this class and takes a single
        // `Option<&AnyObject>` argument; `self` lives for the process's
        // lifetime, so the centre's non-owning reference does not dangle.
        // The constant `NSString` is a name AppKit exposes (the precedent of
        // `NSRunLoopCommonModes`).
        unsafe {
            center.addObserver_selector_name_object(
                self,
                sel!(accessibilityDisplayDidChange:),
                Some(NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification),
                None,
            );
        }
        self.apply_reduce_motion();
    }

    /// Gives the **resolved** value of Reduce Motion to all windows'
    /// links ([`AppDelegate::reduce_motion`]).
    ///
    /// It has three callers and all three re-ask the same question: launch
    /// ([`AppDelegate::observe_reduce_motion`]), the system notification and the settings
    /// file's save ([`AppDelegate::reload_settings`]). If the value
    /// did not change the call is a no-op (`bt_gpu::DisplayLink::set_reduce_motion`),
    /// so there is no need to merge the three paths. A window's first value
    /// lands in its own `start`; a window without a link is silently skipped.
    ///
    /// The wheel's mode also lands here ([`AppDelegate::smooth_scroll`]):
    /// Reduce Motion is its input, so all three triggers can change it too —
    /// had a second path been written, the system notification would
    /// skip it.
    fn apply_reduce_motion(&self) {
        let reduce = self.reduce_motion();
        let smooth = resolve_smooth_scroll(&self.ivars().settings.borrow(), reduce);
        for pane in self.all_panes() {
            pane.set_reduce_motion(reduce);
            pane.set_smooth_scroll(smooth);
        }
        // The tab bars ask Reduce Motion themselves when they draw: their
        // rings stand still or turn, and their clocks follow.
        for window in self.windows() {
            window.refresh_bar();
        }
    }

    /// ⌘ is held alone (`held`) or not — the key watch
    /// (`tab_bar::watch_command_key`): the key window's bar shows its tabs'
    /// keys, every other bar hides them.
    pub(crate) fn command_held(&self, held: bool) {
        // Every key press says "not held": walk the bars only when ⌘ was.
        if self.ivars().command_hinted.replace(held) == held && !held {
            return;
        }
        let key = self.key_window().map(|window| window.id());
        for window in self.windows() {
            window.bar().command_held(held && Some(window.id()) == key);
        }
    }

    /// Starts watching the system's scroll bar preference — **only in a user
    /// session** ([`Inputs`]; a timed run does not read it,
    /// [`resolve_scrollbar`]).
    ///
    /// Unlike Reduce Motion's, this notification is posted on the **default**
    /// centre (`NSScroller`'s header), not `NSWorkspace`'s; subscribing to
    /// the wrong one would silently never hear anything. Not removed: the
    /// [`AppDelegate::observe_reduce_motion`] precedent. The launch's
    /// resolved form is applied here once, which seeds the gate.
    fn observe_scroller_style(&self) {
        let Inputs::User { .. } = self.inputs() else {
            return;
        };
        let center = NSNotificationCenter::defaultCenter();
        // SAFETY: `preferredScrollerStyleDidChange:` is defined on this class
        // and takes a single `Option<&AnyObject>` argument; `self` lives for
        // the process's lifetime, so the centre's non-owning reference does
        // not dangle. The constant `NSString` is a name AppKit exposes.
        unsafe {
            center.addObserver_selector_name_object(
                self,
                sel!(preferredScrollerStyleDidChange:),
                Some(NSPreferredScrollerStyleDidChangeNotification),
                None,
            );
        }
        self.apply_scrollbar();
    }

    /// The system's scroll bar preference changed (main thread): the form is
    /// re-resolved, and the settings window's note, which says what
    /// "system" gives, follows a change.
    fn scroller_style_changed(&self) {
        if self.apply_scrollbar() {
            self.refresh_settings_window();
        }
    }

    /// **The one point where the scroll bar's resolved form changes**: the
    /// settings file's save ([`AppDelegate::reload_settings`]), the system's
    /// notification and the launch all come here and re-ask the same
    /// question ([`AppDelegate::scrollbar_mode`]). The same answer as last
    /// time does nothing; a new one goes to every pane — which hands it to
    /// its link and, when the always-up form comes or goes, resizes its grid
    /// by the track (`TerminalPane::set_scrollbar_mode`). `true` → the form
    /// changed, and the settings window's note with it.
    fn apply_scrollbar(&self) -> bool {
        let mode = self.scrollbar_mode();
        if self.ivars().scrollbar.replace(Some(mode)) == Some(mode) {
            return false;
        }
        for pane in self.all_panes() {
            pane.set_scrollbar_mode(mode);
        }
        true
    }

    /// The setting and the system's preference, in the form merged in
    /// [`resolve_scrollbar`].
    pub(crate) fn scrollbar_mode(&self) -> ScrollbarMode {
        let setting = self.ivars().settings.borrow().scrollbar;
        let mtm = self.mtm();
        resolve_scrollbar(&self.inputs(), setting, || {
            NSScroller::preferredScrollerStyle(mtm) == NSScrollerStyle::Overlay
        })
    }

    /// Is the wheel smooth ([`resolve_smooth_scroll`]).
    pub(crate) fn smooth_scroll(&self) -> bool {
        let reduce = self.reduce_motion();
        resolve_smooth_scroll(&self.ivars().settings.borrow(), reduce)
    }

    /// The setting's three values and the system's answer, in the form merged in
    /// [`resolve_reduce_motion`].
    pub(crate) fn reduce_motion(&self) -> bool {
        let setting = self.ivars().settings.borrow().reduce_motion;
        resolve_reduce_motion(&self.inputs(), setting, || {
            NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
        })
    }

    /// The app's effective appearance, i.e. whether the system's light/dark setting is dark.
    ///
    /// Reading from `NSApp` is **mandatory**: the window's and view's appearance now
    /// reflect the theme, not the system ([`TerminalWindow::apply_chrome`]) —
    /// reading from the view would say "light" for a user with a fixed light theme while the system is dark
    /// and the question that selects the theme would read its own answer.
    /// `bestMatchFromAppearancesWithNames` is AppKit's way of asking
    /// "is it dark" — comparing names would count the high-contrast dark
    /// appearance (`NSAppearanceNameAccessibilityHighContrastDarkAqua`) as
    /// light.
    fn dark_appearance(&self) -> bool {
        let appearance = NSApplication::sharedApplication(self.mtm()).effectiveAppearance();
        // SAFETY: two constant `NSString`s AppKit exposes; they live for the process
        // and are only read (the precedent of `NSRunLoopCommonModes`).
        let (aqua, dark_aqua) = unsafe { (NSAppearanceNameAqua, NSAppearanceNameDarkAqua) };
        appearance
            .bestMatchFromAppearancesWithNames(&NSArray::from_slice(&[aqua, dark_aqua]))
            .is_some_and(|best| &*best == dark_aqua)
    }

    /// The decision on the entry points that open onto the user's world ([`Inputs`]).
    fn inputs(&self) -> Inputs {
        decide_inputs(self.ivars().run, child::home())
    }

    /// The **only** writer of the window subtitle: refreshes the source's slot,
    /// prints the diagnostics to stderr with the `bateri:` prefix and builds the subtitle.
    ///
    /// If a second path wrote to the subtitle the slots would lose their meaning: one would
    /// silently overwrite the other's diagnostic.
    ///
    /// **If the slot stays the same it does nothing** — neither stderr nor subtitle.
    /// The font slot is written at the end of `sync_geometry` and that path runs on every event during live
    /// resizing: a family that cannot be found would print one stderr line per event
    /// and the subtitle would be rebuilt needlessly. Saving the same
    /// faulty settings file a second time also no longer reprints the line;
    /// it is already in the subtitle.
    pub(crate) fn post_notices(&self, source: Source, messages: Vec<String>) {
        let subtitle = {
            let mut notices = self.ivars().notices.borrow_mut();
            if notices.get(source) == messages.as_slice() {
                return;
            }
            for message in &messages {
                eprintln!("bateri: {message}");
            }
            notices.replace(source, messages);
            notices.subtitle()
        };
        for window in self.windows() {
            window.set_notice(&subtitle);
        }
    }

    /// The **single** place of the app's teardown order; every exit path goes through here
    /// (`applicationWillTerminate:` and `runDeadline:`). A single
    /// window's close does not come here, nor does it wait (`TerminalWindow`'s
    /// `windowWillClose:`).
    ///
    /// Today there is a single call: the timed run reaches `process::exit`, the interactive ⌘Q
    /// AppKit's exit, and while the main thread waits the timer cannot
    /// fire. The steps are idempotent
    /// ([`TerminalWindow::begin_close`]); not a guard — a second call
    /// does not cause a second wait but the result becomes `AlreadyDone`.
    ///
    /// **Parallel, a single deadline**: first every window's
    /// teardown starts (the pacing stops, the `Waker` is removed, `SIGHUP` goes out), then
    /// all are waited for until the **same** `now + SHUTDOWN_GRACE` — the
    /// total wait of N tabs is one `SHUTDOWN_GRACE`, not N × `SHUTDOWN_GRACE`.
    /// A child that does not die is left behind. The only exception is the teardown thread
    /// failing to be created (OS thread limit): that branch has no bound and what will cut it
    /// in a timed run is the guard.
    ///
    /// **Windows stay in the list** (and in the copy here) until the wait ends:
    /// `DisplayLink`s live on the main thread, so the `Waker` copy held by Metal's
    /// completion block could be the last reference in the meantime and cannot hand
    /// synchronous work to the main queue — with the main thread waiting, the two would
    /// deadlock each other. The `ShellWake`s' `Waker` is already removed,
    /// so the copies left on the `"PTY teardown"` thread when the bound expires
    /// carry no `Waker` (`wake.rs` → Sahiplik).
    ///
    /// The returned result is the timed run's **measured** pane's
    /// ([`Ivars::measured`]; the first pane when there is none) — unless
    /// another pane's teardown panicked, which is returned instead: the smoke
    /// run's background tab closes here too, and a panic must not pass the
    /// gate because it happened in the tab that is not measured. On an
    /// interactive close the result is dropped — not collected, since nobody
    /// reads it.
    fn shutdown(&self) -> Option<Teardown> {
        // The watchdog's budget starts at **shutdown**, not at process start:
        // startup (GPU device, pipeline setup, first window) can take seconds
        // on a cold machine, and if that were deducted from the budget a
        // healthy run would go red with `_exit(70)`.
        if self.ivars().run.is_some() {
            crate::watchdog();
        }
        if self.ivars().relaunch_after.get() {
            self.spawn_relauncher();
        }
        // A clean quit — a handover or not — is no crash of a restore.
        self.clear_attempt_marks();
        // A deliberate handover (`keep_running`, [`AppDelegate::terminate_reply`]):
        // the programs go to a holder and nothing below runs — no pane
        // closes, no ssh master ends. It writes the session restore save
        // itself, from the frozen panes: reading the scrollback live first
        // would destroy an alternate screen before the freeze reads it
        // (`Session::final_history`).
        let target = match self.ivars().holder.take() {
            Some(holder) => {
                // A bound holder that did not answer (it would have taken
                // the panes otherwise) goes first: kept until this process
                // ends, it would go detached with copies of the same programs
                // and the next bateri would take them from it, screenless.
                if let Some(keeper) = &self.ivars().keeper {
                    keeper.discard();
                }
                Some(Target::Update(holder))
            }
            None if self.ivars().hand_to_bound.take() => self
                .ivars()
                .keeper
                .as_deref()
                .and_then(Keeper::take)
                .map(|(bound, id)| Target::Bound(bound, id)),
            None => None,
        };
        if let Some(target) = target
            && self.hand_over(target)
        {
            // Only now: had the handover failed, the programs end below and
            // "they keep running" would be false.
            self.leave_quit_notice();
            return None;
        }
        // The quit ends the programs: the bound holder (if any) lets every
        // copy go **first** and is gone before a pane closes — a close that
        // hangs must not leave a copy of a master held anywhere.
        if let Some(keeper) = &self.ivars().keeper {
            keeper.quit();
        }
        // Session restore's save comes **first**: the scrollback
        // is read from live sessions and `begin_close` below drops them. A
        // failed handover has saved already and this is a no-op.
        self.save_session();
        let windows = self.windows();
        // The panes' closes below end their remote sessions; the masters'
        // `exit` is `close_all`'s, under the shared deadline.
        if let Some(masters) = &self.ivars().masters {
            masters.begin_quit();
        }
        // One teardown per pane, across all windows' panes: all of them
        // start, then they are awaited in parallel up to a single deadline.
        let closing: Vec<_> = windows
            .iter()
            .flat_map(|window| window.begin_close())
            .collect();
        let deadline = Instant::now() + SHUTDOWN_GRACE;
        // Our ssh masters end in parallel, under the same deadline;
        // a timed run has none.
        let masters = self.ivars().masters.clone().and_then(|masters| {
            std::thread::Builder::new()
                .name("ssh masters close".into())
                .spawn(move || masters.close_all(deadline))
                .ok()
        });
        // The result feeds the report (`teardown=`): if the session was never
        // born it is `None`, and that is an answer too — nothing to close.
        let results: Vec<_> = closing
            .into_iter()
            .map(|(pane, closing)| (pane, closing.map(|closing| closing.wait_until(deadline))))
            .collect();
        if let Some(masters) = masters {
            let _ = masters.join();
        }
        reported_teardown(&results, self.ivars().measured.get())
    }

    /// The smoke run's report and exit — called **after shutdown**.
    ///
    /// The order is deliberate: `shutdown()` waits, bounded though it is, and
    /// on a shutdown that exceeds even that bound the watchdog cuts the process
    /// with 70, so on such a shutdown the `frames=` line never appears. In the
    /// reverse order `make smoke` would give a green line and a red exit code.
    ///
    /// The line is written **explicitly** here; no path relies on `Drop`.
    /// `process::exit` does not run `Drop`, and the watchdog's `_exit(70)`
    /// skips even atexit.
    ///
    /// `teardown` is an argument, not an ivar: the **caller** knows the
    /// shutdown's result and storing it in an ivar would make it readable a
    /// second time. `quiet` is an argument too but for another reason: its
    /// value must be read **before** shutdown (in both callers' docs) and if
    /// read here `shutdown()`'s wait would be written into the quiet time.
    ///
    /// The counters are read from the **measured** pane ([`Ivars::measured`]),
    /// the background tab's from its own ([`Hidden`]). With no measured pane
    /// (it never opened; with no window at all startup had already exited)
    /// the counters are zero and the gate says `MissingCounter`.
    fn report_and_exit(&self, run: Run, teardown: Option<Teardown>, quiet: Option<Duration>) -> ! {
        let pane = self.measured_pane();
        // The frames still in flight are counted **before** `frames=` is read:
        // completion is polled by the ticks, and the link is
        // stopped, so nothing else would count them.
        if let Some(link) = pane.as_deref().and_then(TerminalPane::link) {
            link.drain();
        }
        let renderer = pane.as_deref().map(TerminalPane::renderer);
        // Four counters say four different things: `frames` is the number of
        // frames the GPU finished without error, `cells` the background cells
        // the sink produced, `glyphs` the glyphs drawn, `rules` the underlines
        // and strikeouts drawn. While one is zero the others cannot pass
        // green — frames>0 & cells=0 means "a window exists, no shell
        // output"; cells>0 & glyphs=0 means "cells are painted but there are
        // no letters", i.e. silently falling back to the blind-writing era:
        // without the `glyphs` gate `frames=1 cells=8 pipeline=ok` would be
        // printed even if the `frame()` boundary passed no characters. The
        // half `rules` closes is separate the same way: the boundary carries
        // five underline kinds and SGR 58, independent of `glyphs` — the smoke recipe's seven rule cells are inkless.
        //
        // What it does not cover — all four are **CPU** counters and `rules`
        // cannot see the style distinction on top of that; the whole boundary
        // is written down in `Frame::rule_count`, in one place. Only the half
        // specific to the smoke gate falls here: the token **never asks about
        // the face half** of the set — a build whose `Face` translation always
        // returns `Regular` prints the same four numbers, since a bold glyph is a glyph too. The gate for that half is
        // `bt-gpu`'s `sgr_flags_translate_to_four_faces` and
        // `bold_and_regular_draw_differently` tests.
        //
        // The four are carried in a **struct**, not as positional arguments:
        // three share a type and would compile if swapped.
        let (n, k, g, r) = (
            renderer.map_or(0, Renderer::frames),
            renderer.map_or(0, Renderer::last_bg_count),
            renderer.map_or(0, Renderer::last_glyph_count),
            renderer.map_or(0, Renderer::last_rule_count),
        );
        // The fifth counter `content` is in the same struct but from elsewhere: `frames` is GPU-side
        // (completion block), `content` on the main thread (`needs_update`).
        // The gate's upper bound depends on it and the lower bound is still on
        // `frames` — which question asks which counter is in [`verdict`].
        let link = pane.as_deref().and_then(TerminalPane::link);
        let counters = Counters {
            frames: n,
            content: link.map_or(0, DisplayLink::content_frames),
            cells: k,
            glyphs: g,
            rules: r,
            motion: link.map_or(0, DisplayLink::motion_frames),
            slide: link.map_or(0, DisplayLink::slide_frames),
        };
        // Settling is a **state, not a number**, hence outside `Counters`.
        // With no link (session never born) there is no pending animation
        // either: the counter half (`motion=0`) already yields `MissingCounter`
        // and the reader must not be sent after a false "animation did not stop" fault.
        let motion = if link.is_none_or(DisplayLink::motion_settled) {
            MotionState::Settled
        } else {
            MotionState::Unsettled
        };
        // The background tab since it left the screen: its link is stopped
        // like the measured one, so these are the deadline's numbers. Not
        // drained — its finished GPU frames are not what is asked.
        let background = self.ivars().background.get().and_then(|hidden| {
            let pane = self.timed_pane(hidden.pane)?;
            let link = pane.link()?;
            Some(Background {
                frames: drawn_frames(link).saturating_sub(hidden.frames),
                wakes: link.requests().saturating_sub(hidden.wakes),
            })
        });
        // The fifth token `slots=U/T` is a **counter**, not a gate: it says how many of the atlas's
        // slots are filled and a measurement will read the occupancy ratio from it. It stays out of the
        // gate because of its meaning: an empty atlas is legitimate (a frame with no glyphs) and so
        // is a full one — the failure threshold is unknown until measured, and an unmeasured number
        // is not written into the gate. Same for `requests=`: the frame **request** sees
        // what `frames` cannot
        // but its threshold was not measured.
        let report = Report {
            counters,
            atlas: renderer.map_or((0, 0), Renderer::atlas_occupancy),
            color_atlas: renderer.map_or((0, 0), Renderer::color_atlas_occupancy),
            workload: run.workload,
            requests: link.map_or(0, DisplayLink::requests),
            quiet,
            teardown,
            // If the gate was closed the ledger was never born; `Option` carries that and the
            // report says `samples=off` — not an invented zero.
            measured: self.ivars().stats.as_deref().map(|stats| {
                Measured::read(stats, renderer.is_some_and(Renderer::gpu_timing_supported))
            }),
            background,
        };
        // Tokens appear **only** on the success line and only on stdout: that is
        // the machine contract. Error lines carry the same numbers but not in
        // token form, or a CI step looking for `frames=` would read a frame
        // count from a failed run.
        let secs = run.seconds;
        match verdict(counters, run.workload, teardown, motion, quiet, background) {
            Verdict::Pass => {
                println!("{}", report.token_line());
                std::process::exit(0);
            }
            // Separate message because separate fault: here all five counters
            // are in place and sending the reader looking for a zero would waste
            // time. The number over the limit is `content`, but the line also
            // states `frames` and `requests`: read together, the fault can be told
            // apart as "damage is streaming" (all three high) or "motion is not
            // settling" (`frames` high, `content` not).
            Verdict::ExcessFrames { limit } => eprintln!(
                "bateri: idle-zero-frames broke — {c} content frames drawn in the {secs}-second run (total frames {n}, frame requests {}, {}), upper limit {limit}",
                report.requests,
                quiet_phrase(report.quiet),
                c = counters.content,
            ),
            // The witness never came, so the background tab's zero says
            // nothing; the message names which half was missing.
            Verdict::BackgroundUnwoken => match background {
                Some(back) => eprintln!(
                    "bateri: the background tab got no damage notice while hidden in the {secs}-second run — its {} frames drawn while hidden prove nothing (the recipe's second print must arrive after the measured tab opens)",
                    back.frames,
                ),
                None => eprintln!(
                    "bateri: the {secs}-second smoke run recorded no background tab — the first tab had no link when the measured one opened"
                ),
            },
            // A hidden tab drew: the count and the notices that came while it
            // was hidden, so the reader can tell "the gate let damage through"
            // (both high) from "something draws without damage".
            Verdict::BackgroundDrew => eprintln!(
                "bateri: zero-frames-in-a-background-tab broke — the hidden tab drew {} frames in the {secs}-second run (content, motion and slide frames since it left the screen; {} damage notices while hidden)",
                background.map_or(0, |back| back.frames),
                background.map_or(0, |back| back.wakes),
            ),
            Verdict::MissingCounter { required } => eprintln!(
                "bateri: in the {secs}-second run frames drawn {n}, content frames {c}, cells produced {k}, glyphs drawn {g}, rules drawn {r}, motion frames {m}, {} ({required})",
                quiet_phrase(report.quiet),
                c = counters.content,
                m = counters.motion,
            ),
            // The stop condition broke. Counters are in place and the frame limit
            // may not be exceeded — a slow animation passes both; what turns it red
            // is that it is still in flight at the deadline.
            Verdict::MotionUnsettled => eprintln!(
                "bateri: at the end of the {secs}-second run the animation had still not settled — a stop condition is broken (motion frames drawn {m}, {})",
                quiet_phrase(report.quiet),
                m = counters.motion,
            ),
            // The leak neither exceeded the limit nor passed through the motion
            // infrastructure: only the trace it left remains. The message states
            // the floor **and** the measured value together, since the gap
            // between them gives the leak's period — so the reader can answer
            // "how often does it ask for frames" from the line.
            Verdict::QuietTooShort { floor } => eprintln!(
                "bateri: at the end of the {secs}-second run frames were still flowing — {} (at least {} expected; content frames {c}, motion frames {m}, frame requests {})",
                quiet_phrase(report.quiet),
                ms(floor),
                report.requests,
                c = counters.content,
                m = counters.motion,
            ),
            // Counters are in place but there was a panic on the shutdown path: the token line
            // is not printed so a CI step looking for `frames=` does not
            // mistake this run for a measurement.
            Verdict::ShutdownPanicked { which } => eprintln!(
                "bateri: panic on the shutdown path ({which}) — counters are in place but the run is not valid"
            ),
        }
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use bt_core::ContentEdge;

    use super::*;

    /// The **measured** tail of a healthy smoke run (2026-09-16, the lowest of
    /// thirty-seven runs: `1742,29 ms`).
    /// Tests that do not ask about the gate get this so the `quiet` arm does not
    /// shadow what they do ask; the arm's own tests are below and name the
    /// floor explicitly.
    const HEALTHY_QUIET: Option<Duration> = Some(Duration::from_millis(1742));

    /// A healthy smoke run's background tab: no frame while hidden, the
    /// recipe's second print as its one damage notice. Tests that do not ask
    /// about the background arms get this; theirs name it explicitly.
    const HEALTHY_BACK: Option<Background> = Some(Background {
        frames: 0,
        wakes: 1,
    });

    /// Grid metrics; the gutter is an **argument**, because `split_into_grid` is asked two
    /// separate things: the cell split (gutter zero) and the gutter's deduction from columns.
    fn metrics(w: u16, h: u16, gutter: u16) -> CellMetrics {
        CellMetrics::new(w, h, w, gutter, 1, 1.0).expect("non-zero cell")
    }

    /// A window without a dock: the state of an unintegrated session (and of the smoke recipe).
    /// Tests that query column and row arithmetic get this so the dock
    /// gutter does not mix into the numbers they expect; the gutter's own test
    /// is below and names `DOCK_ROWS` explicitly.
    const NO_DOCK: u16 = 0;

    /// No scroll bar reserve: the self-hiding forms' grid, and the one every
    /// test that is not about the reserve asks about.
    const NO_RESERVE: f32 = 0.0;

    /// No top edge reserve: the content cut at the top, every row the height
    /// allows — the row arithmetic every test that is not about the fade asks
    /// about, word for word as it was before the fade.
    const NO_TOP: f32 = 0.0;

    fn report(counters: Counters, workload: Workload) -> Report {
        Report {
            counters,
            atlas: (13, 2048),
            // The color plane is **empty**: the smoke recipe runs `/bin/sh` and
            // prints no emoji, so this is the number a healthy run expects.
            color_atlas: (0, 2048),
            workload,
            requests: 4,
            quiet: Some(Duration::from_millis(2950)),
            teardown: Some(Teardown::Clean),
            measured: None,
            // The smoke run's background tab stayed dark while a notice came;
            // the measurement load has none.
            background: HEALTHY_BACK.filter(|_| workload == Workload::Smoke),
        }
    }

    fn smoke_counters() -> Counters {
        Counters {
            frames: 1,
            content: 1,
            cells: 8,
            glyphs: 6,
            rules: 15,
            // The trace of the recipe's cursor motion; if it were zero the gate
            // would say `MissingCounter` (see `Counters::motion`).
            motion: 3,
            // The trace of the slide. A different number from `motion` **on purpose**: the two are
            // not counts of the same frames, they are witnesses of two separate animators.
            slide: 2,
        }
    }

    #[test]
    fn smoke_counts_unchanged() {
        // The half of the smoke contract that must stay **bit for bit** the same. Adding a token
        // is free; these four stand in this order, with these values and at the
        // **start** of the line — whoever reads `make smoke` (and `proje.md`'s
        // verification table) searches for them as text.
        let line = report(smoke_counters(), Workload::Smoke).token_line();
        assert!(
            line.starts_with("frames=1 cells=8 glyphs=6 rules=15 "),
            "smoke counters moved: {line}"
        );
    }

    #[test]
    fn token_line_preserves_old_tokens() {
        // The contract: **never deleted, only added.** A token that drops while the report
        // grows is silent — the reader can skip one it does not know, but
        // cannot look for one that vanished.
        let line = report(smoke_counters(), Workload::Smoke).token_line();
        for token in [
            "frames=1",
            "cells=8",
            "glyphs=6",
            "rules=15",
            "slots=13/2048",
            "load=smoke",
            "requests=4",
            "teardown=clean",
        ] {
            assert!(line.contains(token), "{token} dropped: {line}");
        }
        assert!(line.ends_with(" pipeline=ok"), "{line}");

        // The four new keys are **permanent** too: from today the contract puts them on the
        // "never deleted" side as well. `slide=` arrived later and sat next to `motion=` —
        // the token is **never deleted, only added**.
        for token in [
            "content=1",
            "motion=3",
            "slide=2",
            "quiet=2950.00ms",
            // Arrived with the colour plane and sits **right next to** `slots=`: the two are
            // the atlas's two planes and are read side by side. It was put in the list the same
            // day, because the "never deleted" promise is a promise only if
            // a guard exists — the list above protects only the **old** tokens.
            "slots2=0/2048",
            // The background tab's pair arrived with tabs and is **permanent**
            // from that day, in its place: last before `pipeline=ok`.
            "back=0",
            "back_wakes=1",
        ] {
            assert!(line.contains(token), "{token} missing: {line}");
        }
        assert!(
            line.ends_with(" back=0 back_wakes=1 pipeline=ok"),
            "the background pair stands last, before pipeline=ok: {line}"
        );
        // Its position is part of the contract too: `slots=` and `slots2=` side by side. Were they apart,
        // someone reading the line by eye could not connect the two planes.
        assert!(
            line.contains("slots=13/2048 slots2=0/2048 "),
            "the two planes' tokens must stand side by side: {line}"
        );

        // With the gate closed the measurement tokens are **absent**, and `samples=0` is absent too: zero
        // would be confused with "the gate was open but no samples were collected", and
        // the blindness this closes is exactly that.
        assert!(line.contains(" samples=off"), "{line}");
        assert!(!line.contains("cpu_frame_p95"), "{line}");
        assert!(!line.contains("startup="), "{line}");

        // `load=` changes with the workload and the string sits next to the type.
        let load = report(
            Counters {
                frames: 9,
                content: 9,
                cells: 0,
                glyphs: 12,
                rules: 0,
                motion: 0,
                slide: 0,
            },
            Workload::Load,
        )
        .token_line();
        assert!(load.contains("load=load"), "{load}");
        // The measurement load has one tab: the pair says so in its own word,
        // not with a zero that would read "it stayed dark".
        assert!(load.contains(" back=none back_wakes=none "), "{load}");
    }

    #[test]
    fn quiet_token_says_none_when_nothing_was_drawn() {
        // `quiet=` prints no invented zero: zero would mean "frames were flowing at
        // the deadline" and be confused with a run where no frame was ever drawn
        // (the same rule as `samples=off`).
        //
        // This arm is **unreachable** on the token line — if no frame was drawn `frames=0`
        // and the gate says `MissingCounter`, so the line is never printed. It is tested
        // anyway: `Report` can represent it, and the gate's `quiet` arm
        // puts `None` in the same bucket as a tail below the floor
        // (`a_short_tail_fails_the_gate`).
        let mut r = report(smoke_counters(), Workload::Smoke);
        r.quiet = None;
        let line = r.token_line();
        assert!(line.contains(" quiet=none "), "{line}");
        assert!(!line.contains("quiet=0"), "{line}");
    }

    #[test]
    fn quiet_phrase_stays_out_of_the_token_contract() {
        // The diagnostic phrase is the failed run's **only** `quiet` record (the token line is printed only
        // on green), but it must not look like a token: a reader searching for `quiet=`
        // must not read a number from a failed run.
        let phrase = quiet_phrase(Some(Duration::from_millis(1745)));
        assert!(phrase.contains("1745.00ms"), "{phrase}");
        assert!(!phrase.contains("quiet="), "{phrase}");
        assert_eq!(quiet_phrase(None), "no frames drawn");
    }

    #[test]
    fn measured_tokens_report_every_column() {
        // With the gate open **each** of the three columns gets its own token and the GPU
        // falling short becomes visible: `samples=` and `gpu_samples=` are separate numbers,
        // because Metal's zero timestamp can let a frame be written for the CPU but not
        // for the GPU.
        let mut r = report(smoke_counters(), Workload::Load);
        r.measured = Some(Measured {
            startup: Some(Duration::from_millis(284)),
            cpu_samples: 594,
            dropped: 2,
            gpu_samples: 591,
            gpu_rejected: 3,
            cpu_frame: Some((Duration::from_micros(1800), Duration::from_micros(4100))),
            cpu_encode: None,
            gpu: Some((Duration::from_micros(2200), Duration::from_micros(5000))),
            gpu_supported: true,
        });
        let line = r.token_line();
        for token in [
            "samples=594",
            "dropped=2",
            "gpu_samples=591",
            "gpu_discarded=3",
            &format!("floor={MIN_SAMPLES}"),
            "cpu_frame_p95=1.80ms",
            "cpu_frame_max=4.10ms",
            "gpu_p95=2.20ms",
            "gpu_max=5.00ms",
            "startup=284.00ms",
        ] {
            assert!(line.contains(token), "{token} missing: {line}");
        }
        // A column below the floor **prints no number** and the reason can be read in the same
        // line's `samples=`/`floor=` pair. p95 and worst are silent
        // together: with few samples they are the same element anyway.
        assert!(line.contains("cpu_encode_p95=insufficient"), "{line}");
        assert!(line.contains("cpu_encode_max=insufficient"), "{line}");
        // Without timestamp support the GPU keys stay and say why there is
        // no number (a token is never deleted).
        if let Some(m) = r.measured.as_mut() {
            m.gpu_supported = false;
        }
        let line = r.token_line();
        assert!(line.contains("gpu_p95=unsupported"), "{line}");
        assert!(line.contains("gpu_max=unsupported"), "{line}");
    }

    #[test]
    fn cell_metrics_come_from_outside() {
        // The testable form of the placeholder being dead: same window, two
        // different cell sizes, two different grids. A constant leaking back into the body
        // would make the two equal and this test would fail.
        // Gutter zero: what is asked is that the cell size determines the grid, not the gutter's
        // effect. The gutter's own test is `the_gutter_costs_columns`.
        let narrow = split_into_grid(900.0, 600.0, metrics(9, 18, 0), NO_DOCK, NO_RESERVE, NO_TOP);
        let wide = split_into_grid(
            900.0,
            600.0,
            metrics(18, 36, 0),
            NO_DOCK,
            NO_RESERVE,
            NO_TOP,
        );
        assert_eq!((narrow.cols, narrow.rows), (100, 33));
        assert_eq!((wide.cols, wide.rows), (50, 16));
    }

    #[test]
    fn the_gutter_costs_columns() {
        // The left gutter is deducted from columns: so the stripe does not
        // sit on top of the text. 900 pixels, 9-pixel cells → 100 columns with no gutter; an 8-pixel
        // gutter takes one column, and so does 9 pixels (a full cell).
        let plain = split_into_grid(900.0, 600.0, metrics(9, 18, 0), NO_DOCK, NO_RESERVE, NO_TOP);
        let gutter = split_into_grid(900.0, 600.0, metrics(9, 18, 8), NO_DOCK, NO_RESERVE, NO_TOP);
        assert_eq!(plain.cols, 100);
        assert_eq!(gutter.cols, 99, "the gutter takes one column");
        // Rows **do not see** the gutter as a left margin: it is only on the left.
        // The top edge's reserve, which in `Fade` is the gutter again, is its own
        // parameter (`top_px`, zero here) and its own test
        // (`the_fade_keeps_the_margin_free_at_the_top_and_takes_the_leftover`).
        assert_eq!(gutter.rows, plain.rows);
        // The gutter travels with the metrics: the value that built the grid gives it back
        // and the draw origin and mouse mapping read the same value.
        assert_eq!(gutter.cell.gutter_px(), 8);
    }

    #[test]
    fn the_dock_costs_rows_and_only_when_there_is_one() {
        // The dock gutter is deducted from **rows** and, unlike the left gutter, conditional:
        // not a single row should go **to the dock** from a window without one (an
        // unintegrated shell, the smoke recipe) — `smoke_shell`'s `cells=8 glyphs=6`
        // contract is measured in that window. The top edge's reserve is a separate
        // parameter (`top_px`, zero here): in `Fade` that window gives it a row at
        // some heights (`the_fade_keeps_the_margin_free_at_the_top_and_takes_the_leftover`).
        let without = split_into_grid(900.0, 600.0, metrics(9, 18, 8), NO_DOCK, NO_RESERVE, NO_TOP);
        let with = split_into_grid(
            900.0,
            600.0,
            metrics(9, 18, 8),
            DOCK_ROWS,
            NO_RESERVE,
            NO_TOP,
        );
        // 600 / 18 = 33.3 → 33.
        assert_eq!(without.rows, 33);
        // The dock takes **two rows, two breathing gutters and one row gap**:
        // 2×18 + 2×8 + 16 = 68 px, i.e. 532 / 18 = 29.5 → 29. The row gap
        // (`dock_row_gap`) is **twice** the outer gutter, because a line
        // passes through its middle and one gutter falls on each side of the line; if left out of the sum
        // it would come to 52 px, which gives 30 rows and the difference becomes **visible**.
        // The number's source is `bt_gpu::dock_px`, not `DOCK_ROWS`; if the two
        // drift apart this goes red.
        assert_eq!(with.rows, 29, "dock gutter was not deducted from rows");
        // Columns **do not see** the dock: without the scroll bar's reserve the
        // dock uses the same columns as the grid and its gutter is vertical only.
        assert_eq!(with.cols, without.cols);
    }

    #[test]
    fn the_fade_keeps_the_margin_free_at_the_top_and_takes_the_leftover() {
        // In `Fade` the rows keep the top edge's reserve free — the left
        // margin, 8 px here: a leftover at or above it costs nothing, one under
        // it costs a row. The drawing side's fade over those rows is the whole
        // leftover, never under the reserve and never a cell of it.
        let cell = metrics(9, 18, 8);
        let top = bt_gpu::edge_reserve_px(ContentEdge::Fade, cell);
        assert_eq!(top, 8.0, "the reserve is not the left margin");
        let edge = |height: f64, dock_rows: u16, rows: u16| {
            bt_gpu::edge_drawn_px(ContentEdge::Fade, height as f32, dock_rows, rows, cell)
        };
        // (height, dock, rows cut, rows faded, the fade)
        // 600 with the dock: 532 / 18 = 29.5, a 10 px leftover — the same rows.
        // 596 with the dock: 528 / 18 = 29.3, a 6 px leftover — a row less.
        // 600 without: 600 / 18 = 33.3, a 6 px leftover — a row less.
        for (height, dock_rows, cut_rows, fade_rows, fade_px) in [
            (600.0, DOCK_ROWS, 29, 29, 10.0),
            (596.0, DOCK_ROWS, 29, 28, 24.0),
            (600.0, NO_DOCK, 33, 32, 24.0),
        ] {
            let cut = split_into_grid(900.0, height, cell, dock_rows, NO_RESERVE, NO_TOP);
            let fade = split_into_grid(900.0, height, cell, dock_rows, NO_RESERVE, top);
            assert_eq!((cut.rows, fade.rows), (cut_rows, fade_rows), "{height}");
            assert_eq!(fade.cols, cut.cols, "{height}: the fade took columns");
            assert_eq!(edge(height, dock_rows, fade.rows), fade_px, "{height}");
        }
        // `Cut` and `Line` keep nothing free: today's rows to the row.
        for mode in [ContentEdge::Cut, ContentEdge::Line] {
            assert_eq!(bt_gpu::edge_reserve_px(mode, cell), 0.0, "{mode:?}");
        }
        // A window that cannot hold the reserve gets no rows: the subtraction
        // saturates in `f64`, it does not wrap.
        let g = split_into_grid(900.0, 4.0, cell, NO_DOCK, NO_RESERVE, top);
        assert_eq!(g.rows, 0);
    }

    #[test]
    fn the_dock_breathing_room_scales_with_the_gutter() {
        // The breathing gutter is **derived**, not chosen: its source is the left
        // gutter itself. With a fixed pixel count the gutter would stay the same while the font
        // grows with Cmd +/− and the ratio would break; this test holds exactly that link.
        let tight = split_into_grid(
            900.0,
            600.0,
            metrics(9, 18, 0),
            DOCK_ROWS,
            NO_RESERVE,
            NO_TOP,
        );
        let loose = split_into_grid(
            900.0,
            600.0,
            metrics(9, 18, 8),
            DOCK_ROWS,
            NO_RESERVE,
            NO_TOP,
        );
        // A dock without gutters takes only its rows: 600 − 36 = 564 → 31.
        assert_eq!(tight.rows, 31);
        assert!(
            loose.rows < tight.rows,
            "the gutter grew but the dock covered the same space: {} / {}",
            loose.rows,
            tight.rows
        );
    }

    #[test]
    fn the_always_up_scroll_bar_costs_grid_columns_and_not_dock_columns() {
        // 900 px, 9 px cells, an 8 px gutter: 99 columns. The always-up
        // form's track is 16 pt — 16 px at @1x — so the grid ends at 892 px
        // − 16: 876 / 9 = 97 columns. The dock below the track keeps the
        // window's 99, and the rows do not see the track at all.
        let cell = metrics(9, 18, 8);
        let reserve = ScrollbarMode::Always.reserve_px(cell);
        let plain = split_into_grid(900.0, 600.0, cell, DOCK_ROWS, NO_RESERVE, NO_TOP);
        let always = split_into_grid(900.0, 600.0, cell, DOCK_ROWS, reserve, NO_TOP);
        assert_eq!((plain.cols, plain.dock_cols), (99, 99));
        assert_eq!(always.cols, 97, "the track's columns stayed in the grid");
        assert_eq!(always.dock_cols, 99, "the dock lost columns to the track");
        assert_eq!(always.rows, plain.rows, "the track took rows");
        // The text ends left of the track: nothing runs under the bar.
        let text_end = f64::from(cell.gutter_px()) + f64::from(always.cols) * 9.0;
        assert!(text_end <= 900.0 - f64::from(reserve), "{text_end}");
        // The self-hiding forms reserve nothing.
        for mode in [ScrollbarMode::Auto, ScrollbarMode::Never] {
            let grid =
                split_into_grid(900.0, 600.0, cell, DOCK_ROWS, mode.reserve_px(cell), NO_TOP);
            assert_eq!((grid.cols, grid.dock_cols), (99, 99), "{mode:?}");
        }
        // Narrower than the gutter and the track: no columns — the
        // subtraction is `f64` and saturates, it does not wrap to 65535.
        let narrow = split_into_grid(20.0, 600.0, cell, NO_DOCK, reserve, NO_TOP);
        assert_eq!((narrow.cols, narrow.dock_cols), (0, 1));
    }

    #[test]
    fn a_click_on_the_docks_last_column_beside_the_track_is_the_docks() {
        // The always-up form narrows the grid, not the dock: a click on the
        // dock's last column — under where the track ends, beside the grid's
        // right edge — lands on that column, not clamped back to the grid's.
        let cell = metrics(9, 18, 8);
        let reserve = ScrollbarMode::Always.reserve_px(cell);
        let grid = split_into_grid(900.0, 600.0, cell, DOCK_ROWS, reserve, NO_TOP);
        let top = crate::view::dock_input_top_px(600.0, cell, DOCK_ROWS);
        let last = grid.dock_cols - 1;
        let x = f64::from(cell.gutter_px()) + (f64::from(last) + 0.25) * 9.0;
        let hit = crate::view::point_to_cell(
            (x, top + 4.0),
            cell,
            top,
            crate::view::OutOfGrid::Reject,
            1.0,
            grid.dock_cols,
            1,
        )
        .expect("the dock's last column was rejected");
        assert_eq!(hit.col, last);
        assert!(hit.col >= grid.cols, "the column is not beyond the grid's");
        // With the grid's columns the same click is rejected: the reading
        // the dock must not use.
        assert!(
            crate::view::point_to_cell(
                (x, top + 4.0),
                cell,
                top,
                crate::view::OutOfGrid::Reject,
                1.0,
                grid.cols,
                1,
            )
            .is_none()
        );
    }

    #[test]
    fn lookup_skips_closed_panes() {
        // (id, closed?): a pane whose close has begun is not found even if it is
        // still in the list; an open one is found; an unknown id finds nothing.
        let panes = [(1_u64, false), (2, true), (3, false)];
        let find = |id| find_open(panes, |&(pane, closed)| (pane == id, closed));
        assert_eq!(find(1), Some((1, false)));
        assert_eq!(find(2), None, "a closed pane must not be found");
        assert_eq!(find(3), Some((3, false)));
        assert_eq!(find(4), None);
    }

    #[test]
    fn the_alternate_screen_takes_the_dock_and_gives_it_back() {
        // On the alternate screen the gutter is zero, on exit the **birth value** comes back.
        assert_eq!(dock_rows_for(true, false, DOCK_ROWS), 0);
        assert_eq!(dock_rows_for(false, false, DOCK_ROWS), DOCK_ROWS);
        // **This line is why the birth value is a separate input:**
        // leaving the alternate screen in a window that never had a dock (an unintegrated shell, the smoke
        // recipe) must **not** give birth to a dock. Were it written over a single
        // field, the value to restore would be built from the `DOCK_ROWS`
        // constant and exactly this window would gain a dock.
        assert_eq!(dock_rows_for(true, false, NO_DOCK), 0);
        assert_eq!(dock_rows_for(false, false, NO_DOCK), 0);
        // A remote session's alternate screen keeps the one-row status bar;
        // a window without a dock never gets one.
        assert_eq!(dock_rows_for(true, true, DOCK_ROWS), 1);
        assert_eq!(dock_rows_for(false, true, DOCK_ROWS), DOCK_ROWS);
        assert_eq!(dock_rows_for(true, true, NO_DOCK), 0);
    }

    #[test]
    fn a_window_shorter_than_the_dock_yields_no_rows() {
        // The vertical twin of `a_window_narrower_than_the_gutter_yields_no_columns`
        // and a guard for the same breakage: the subtraction goes negative in `f64` and
        // `as u16` saturates to zero. Done in `u16` it would overflow and
        // produce a 65535-row `TIOCSWINSZ`. `Session::resize` already
        // ignores a zero-row size.
        let g = split_into_grid(
            900.0,
            20.0,
            metrics(9, 18, 8),
            DOCK_ROWS,
            NO_RESERVE,
            NO_TOP,
        );
        assert_eq!(g.rows, 0);
        // Columns stand: a short window eliminates only rows.
        assert_eq!(g.cols, 99);
    }

    #[test]
    fn a_window_narrower_than_the_gutter_yields_no_columns() {
        // Accepted: no new lower bound is **introduced**, the existing chain gives the
        // right answer. The subtraction goes negative in `f64`, the division stays
        // negative and `as u16` saturates to zero; `Session::resize` already
        // ignores a zero-column size. Done in `u16` the same subtraction would
        // **overflow** and produce a `TIOCSWINSZ` with a column count near
        // 65535 — that is the breakage this test guards.
        let g = split_into_grid(4.0, 600.0, metrics(9, 18, 8), NO_DOCK, NO_RESERVE, NO_TOP);
        assert_eq!(g.cols, 0);
        // Rows stand: a narrow window eliminates only columns.
        assert_eq!(g.rows, 33);
    }

    #[test]
    fn idle_limit_catches_excess_frames() {
        // The upper bound's operand is `content`, the lower bound's is `frames`. In a healthy
        // run the two are equal, so the helpers set `frames = content`;
        // the case where they diverge has its own test below.
        let counters = |content, cells, glyphs, rules| Counters {
            frames: content,
            content,
            cells,
            glyphs,
            rules,
            // The trace of a healthy smoke run; what this test asks about is the
            // `content` limit, the motion gate has its own below.
            motion: 3,
            slide: 2,
        };
        let clean = Some(Teardown::Clean);
        let settled = MotionState::Settled;
        let smoke = |n, k, g, r| {
            verdict(
                counters(n, k, g, r),
                Workload::Smoke,
                clean,
                settled,
                HEALTHY_QUIET,
                HEALTHY_BACK,
            )
        };
        let load = |n, k, g, r| {
            verdict(
                counters(n, k, g, r),
                Workload::Load,
                clean,
                settled,
                HEALTHY_QUIET,
                None,
            )
        };
        let excess = Verdict::ExcessFrames {
            limit: IDLE_FRAME_LIMIT,
        };

        // **The whole of this change is in these two lines.** The gate stopped looking at `frames`:
        // motion frames legitimately inflate `frames` and the limit must
        // not see them. The reverse direction is bound too — if `content` overflows, a
        // low `frames` does not save it.
        let mixed = |frames, content| {
            verdict(
                Counters {
                    frames,
                    content,
                    cells: 8,
                    glyphs: 6,
                    rules: 15,
                    motion: 3,
                    slide: 2,
                },
                Workload::Smoke,
                clean,
                settled,
                HEALTHY_QUIET,
                HEALTHY_BACK,
            )
        };
        assert_eq!(
            mixed(200, 1),
            Verdict::Pass,
            "motion frames do not enter the gate"
        );
        assert_eq!(
            mixed(1, 200),
            excess,
            "content frames cannot escape the gate"
        );

        // Today's smoke run itself: one frame, eight cells, six
        // glyphs, fifteen rules.
        assert_eq!(smoke(1, 8, 6, 15), Verdict::Pass);
        // The limit itself passes, one over fails. The old gate (`n > 0`) saw zero
        // but not excess, and the symptom of a change that breaks idle-zero-frames is
        // exactly excess frames.
        assert_eq!(smoke(IDLE_FRAME_LIMIT, 8, 6, 15), Verdict::Pass);
        assert_eq!(smoke(IDLE_FRAME_LIMIT + 1, 8, 6, 15), excess);
        // The **measured** ceiling of a healthy run (2026-09-12, once in thirty-one
        // runs): four frames are legitimate and must pass. The old limit (`2`) turned
        // a correct build red exactly here.
        assert_eq!(smoke(4, 8, 6, 15), Verdict::Pass);
        // The **measured** low end of a broken run: when idle-zero-frames was broken
        // on purpose the lowest count over nine runs was 49. The limit has to
        // catch that.
        assert_eq!(smoke(49, 8, 6, 15), excess);
        assert_eq!(smoke(354, 8, 6, 15), excess);
        // Under the `Load` workload the flow is the work itself: the same number must pass. The limit's
        // dependence on the workload is written in one place and tested here.
        assert_eq!(load(354, 8, 6, 15), Verdict::Pass);
        // The **real** numbers of the measurement workload: plain text flows, background and
        // rules are structurally zero. If all four counters were asked every measurement
        // run would go red. `glyphs=1836` was the same output in all three measurements
        // (2026-09-12); `frames` depends on the environment and that is exactly why
        // there is **no gate** under `Load`: on the same machine the same command gave 9 (2 s)
        // and 21 (5 s) in one regime, 49–234 (2 s) and 597 (5 s) in the other.
        assert_eq!(load(21, 0, 1836, 0), Verdict::Pass);
        assert_eq!(load(594, 0, 1836, 0), Verdict::Pass);

        // Zero comes **before** excess: if both are broken the reader should
        // look for the missing link first. A change that reverses the order of the arms
        // goes red here.
        assert_eq!(
            smoke(200, 0, 6, 15),
            Verdict::MissingCounter {
                required: "all five must be >0"
            }
        );

        // The lower bound holds under both workloads and each counter is a separate gate. The verdict
        // must be `MissingCounter`, not `ExcessFrames`: a failed run must send
        // the reader to the right fault.
        for (got, required) in [
            (smoke(0, 8, 6, 15), "all five must be >0"),
            (smoke(1, 0, 6, 15), "all five must be >0"),
            (smoke(1, 8, 0, 15), "all five must be >0"),
            (smoke(1, 8, 6, 0), "all five must be >0"),
            (load(0, 0, 1836, 0), "frames and glyphs must be >0"),
            (load(3, 0, 0, 0), "frames and glyphs must be >0"),
        ] {
            assert_eq!(got, Verdict::MissingCounter { required });
        }
    }

    #[test]
    fn an_unsettled_animation_fails_the_gate() {
        // The gate's half that **asks for no measurement** and the leak class `IDLE_FRAME_LIMIT`
        // cannot see: all counters in place, the frame limit
        // not exceeded — a slow animation passes both — but still in flight at
        // the deadline. The pure form of the acceptance scenario "temporary mutation:
        // `settled` always `false`".
        let good = Counters {
            frames: 1,
            content: 1,
            cells: 8,
            glyphs: 6,
            rules: 15,
            motion: 3,
            slide: 2,
        };
        let clean = Some(Teardown::Clean);
        assert_eq!(
            verdict(
                good,
                Workload::Smoke,
                clean,
                MotionState::Unsettled,
                HEALTHY_QUIET,
                HEALTHY_BACK
            ),
            Verdict::MotionUnsettled
        );
        assert_eq!(
            verdict(
                good,
                Workload::Smoke,
                clean,
                MotionState::Settled,
                HEALTHY_QUIET,
                HEALTHY_BACK
            ),
            Verdict::Pass
        );

        // **The measurement workload is exempt**: `Load` streams output until the deadline, so
        // with the last line the cursor changes target and the deadline lands in
        // mid-flight. If bound, every measurement run would go red
        // while the code was right.
        assert_eq!(
            verdict(
                Counters {
                    cells: 0,
                    rules: 0,
                    motion: 0,
                    slide: 0,
                    ..good
                },
                Workload::Load,
                clean,
                MotionState::Unsettled,
                HEALTHY_QUIET,
                None,
            ),
            Verdict::Pass
        );

        // If no motion frame was drawn the fault is **a missing counter, not failure
        // to settle**: the smoke recipe has a cursor motion, so zero means
        // "the animation path never ran" and should send the reader there.
        assert_eq!(
            verdict(
                Counters { motion: 0, ..good },
                Workload::Smoke,
                clean,
                MotionState::Settled,
                HEALTHY_QUIET,
                HEALTHY_BACK,
            ),
            Verdict::MissingCounter {
                required: "all five must be >0"
            }
        );

        // The frame limit comes **before** settling: if both are broken
        // the reader should see the flowing frames first.
        assert_eq!(
            verdict(
                Counters {
                    content: IDLE_FRAME_LIMIT + 1,
                    ..good
                },
                Workload::Smoke,
                clean,
                MotionState::Unsettled,
                HEALTHY_QUIET,
                HEALTHY_BACK,
            ),
            Verdict::ExcessFrames {
                limit: IDLE_FRAME_LIMIT
            }
        );
    }

    #[test]
    fn a_short_tail_fails_the_gate() {
        // The gate's third tier and its only measured threshold: counters in place, content
        // frames **below** the limit, animation settled — but the gap between the last frame and
        // the deadline is short, i.e. frames were still flowing at the end of the run.
        // Its measured scenario is a half-second leak: with `content=8` it does
        // not exceed the limit and without this arm it was **green**.
        let good = Counters {
            frames: 30,
            content: 3,
            cells: 8,
            glyphs: 6,
            rules: 15,
            motion: 27,
            slide: 2,
        };
        let clean = Some(Teardown::Clean);
        let settled = MotionState::Settled;
        let smoke = |quiet| verdict(good, Workload::Smoke, clean, settled, quiet, HEALTHY_BACK);
        let short = Verdict::QuietTooShort { floor: QUIET_FLOOR };

        // The tail of the measured slow leak (highest `129,25 ms`) and the lowest
        // of the measured healthy tail (`1742,29 ms`): the gate passes between
        // the two and both distributions stay on their own side.
        assert_eq!(smoke(Some(Duration::from_millis(130))), short);
        assert_eq!(smoke(HEALTHY_QUIET), Verdict::Pass);
        // The floor itself passes, one millisecond below fails.
        assert_eq!(smoke(Some(QUIET_FLOOR)), Verdict::Pass);
        assert_eq!(smoke(Some(QUIET_FLOOR - Duration::from_millis(1))), short);
        // `quiet=none` is not an invented zero but the same answer for the gate:
        // the quiet of a run in which no frame was ever drawn cannot be measured either.
        assert_eq!(smoke(None), short);

        // **The measurement workload is exempt** and the reason is the same as `MotionUnsettled`'s:
        // `Load` streams output until the deadline, so quiet there
        // must be near zero. If bound, every measurement run would go red.
        assert_eq!(
            verdict(
                Counters {
                    cells: 0,
                    rules: 0,
                    motion: 0,
                    slide: 0,
                    ..good
                },
                Workload::Load,
                clean,
                settled,
                Some(Duration::ZERO),
                None,
            ),
            Verdict::Pass
        );

        // Order: if all three are broken the line writes the most basic fault. Quiet is
        // last, because the others recognize the leak **by name**.
        assert_eq!(
            verdict(
                Counters {
                    content: IDLE_FRAME_LIMIT + 1,
                    ..good
                },
                Workload::Smoke,
                clean,
                settled,
                Some(Duration::ZERO),
                HEALTHY_BACK,
            ),
            Verdict::ExcessFrames {
                limit: IDLE_FRAME_LIMIT
            }
        );
        assert_eq!(
            verdict(
                good,
                Workload::Smoke,
                clean,
                MotionState::Unsettled,
                Some(Duration::ZERO),
                HEALTHY_BACK,
            ),
            Verdict::MotionUnsettled
        );
        // Panic comes **even after the tail**: the others say what the run
        // measures broke, panic is the path after the run ended and
        // the `teardown=` token already carries it. Without a test for the combination
        // the "last" claim would be only a comment sentence.
        assert_eq!(
            verdict(
                good,
                Workload::Smoke,
                Some(Teardown::ReaderPanicked),
                settled,
                Some(Duration::ZERO),
                HEALTHY_BACK,
            ),
            short
        );
    }

    #[test]
    fn a_background_tab_must_stay_dark() {
        // The smoke run's second tab: the first one draws, hides behind the
        // measured one and must draw **nothing** while its recipe's second
        // print arrives. Two arms, and the second is worthless without the
        // first — a zero drawn while nothing asked is no proof.
        let good = Counters {
            frames: 28,
            content: 2,
            cells: 8,
            glyphs: 6,
            rules: 15,
            motion: 26,
            slide: 0,
        };
        let clean = Some(Teardown::Clean);
        let settled = MotionState::Settled;
        let smoke = |background| {
            verdict(
                good,
                Workload::Smoke,
                clean,
                settled,
                HEALTHY_QUIET,
                background,
            )
        };
        let back = |frames, wakes| Some(Background { frames, wakes });
        assert_eq!(smoke(back(0, 1)), Verdict::Pass);
        assert_eq!(smoke(back(0, 3)), Verdict::Pass);
        // One frame while hidden is the fault: the gate has no tolerance,
        // the counters are decided frames and a hidden tab decides none.
        assert_eq!(smoke(back(1, 1)), Verdict::BackgroundDrew);
        // No notice while hidden: the zero proves nothing, and nor does a
        // missing background tab.
        assert_eq!(smoke(back(0, 0)), Verdict::BackgroundUnwoken);
        assert_eq!(smoke(None), Verdict::BackgroundUnwoken);
        // A tab that drew with no notice still reads as unwoken: the witness
        // is asked first, like a missing counter before an excess.
        assert_eq!(smoke(back(4, 0)), Verdict::BackgroundUnwoken);

        // **The measurement load is exempt**: it has one tab.
        assert_eq!(
            verdict(
                Counters {
                    cells: 0,
                    rules: 0,
                    motion: 0,
                    slide: 0,
                    ..good
                },
                Workload::Load,
                clean,
                settled,
                Some(Duration::ZERO),
                None,
            ),
            Verdict::Pass
        );

        // Order: the measured tab's missing counter before the background's
        // missing witness; that witness before the measured tab's excess;
        // the excess before the background's frames; those before the
        // measured tab's settling, tail and panic.
        assert_eq!(
            verdict(
                Counters { cells: 0, ..good },
                Workload::Smoke,
                clean,
                settled,
                HEALTHY_QUIET,
                None,
            ),
            Verdict::MissingCounter {
                required: "all five must be >0"
            }
        );
        let excess = Counters {
            content: IDLE_FRAME_LIMIT + 1,
            ..good
        };
        assert_eq!(
            verdict(
                excess,
                Workload::Smoke,
                clean,
                settled,
                HEALTHY_QUIET,
                back(0, 0)
            ),
            Verdict::BackgroundUnwoken
        );
        assert_eq!(
            verdict(
                excess,
                Workload::Smoke,
                clean,
                settled,
                HEALTHY_QUIET,
                back(2, 1)
            ),
            Verdict::ExcessFrames {
                limit: IDLE_FRAME_LIMIT
            }
        );
        assert_eq!(
            verdict(
                good,
                Workload::Smoke,
                Some(Teardown::Panicked),
                MotionState::Unsettled,
                Some(Duration::ZERO),
                back(2, 1),
            ),
            Verdict::BackgroundDrew
        );
    }

    #[test]
    fn the_report_takes_a_panic_from_any_pane() {
        // The smoke run closes two tabs and `teardown=` is the measured
        // pane's — but a panic in the other pane's teardown must not pass the
        // gate (`ShutdownPanicked` reads this one value).
        let clean = Some(Teardown::Clean);
        let abandoned = Some(Teardown::Abandoned);
        let panicked = Some(Teardown::ReaderPanicked);
        // The background tab (pane 1) first, the measured one (pane 3)
        // second, in closing order: the measured's, found by id.
        assert_eq!(
            reported_teardown(&[(1, clean), (3, abandoned)], Some(3)),
            abandoned
        );
        assert_eq!(reported_teardown(&[(1, clean), (3, None)], Some(3)), None);
        // No measured pane (an interactive quit): the first pane's.
        assert_eq!(
            reported_teardown(&[(1, abandoned), (3, clean)], None),
            abandoned
        );
        // A panic anywhere wins over the measured pane's quiet result.
        assert_eq!(
            reported_teardown(&[(1, panicked), (3, clean)], Some(3)),
            panicked
        );
        assert_eq!(
            reported_teardown(&[(1, clean), (3, panicked)], Some(1)),
            panicked
        );
        // No pane, or a measured id that closed nowhere: nothing to report.
        assert_eq!(reported_teardown(&[], None), None);
        assert_eq!(reported_teardown(&[(1, clean)], Some(9)), None);
        // The two panic sites and nothing else.
        assert_eq!(panic_site(panicked), Some("reader thread"));
        assert_eq!(
            panic_site(Some(Teardown::Panicked)),
            Some("teardown thread")
        );
        for quiet in [
            None,
            clean,
            abandoned,
            Some(Teardown::Unbounded),
            Some(Teardown::AlreadyDone),
            Some(Teardown::HungUp),
        ] {
            assert_eq!(panic_site(quiet), None, "{quiet:?}");
        }
    }

    #[test]
    fn motion_and_panic_report_the_more_fundamental_fault() {
        // The order of the arms is a **diagnostic** preference: when two faults coincide the run is red
        // either way, but which does the line write? The code review
        // finding was that this combination was never tested.
        //
        // Failure to settle says what the run **measures** broke, panic is
        // the path after the run ended; sending the reader to the first is right and
        // the `teardown=` token already carries the second. Reversing it
        // would also break `ExcessFrames`'s current order.
        let good = Counters {
            frames: 1,
            content: 1,
            cells: 8,
            glyphs: 6,
            rules: 15,
            motion: 3,
            slide: 2,
        };
        assert_eq!(
            verdict(
                good,
                Workload::Smoke,
                Some(Teardown::Panicked),
                MotionState::Unsettled,
                HEALTHY_QUIET,
                HEALTHY_BACK,
            ),
            Verdict::MotionUnsettled
        );
        // Panic alone is still seen: the order does **not swallow** it.
        assert!(matches!(
            verdict(
                good,
                Workload::Smoke,
                Some(Teardown::Panicked),
                MotionState::Settled,
                HEALTHY_QUIET,
                HEALTHY_BACK,
            ),
            Verdict::ShutdownPanicked { .. }
        ));
    }

    #[test]
    fn shutdown_panic_cannot_pass_the_gate() {
        // Code review finding: the `teardown=` token became visible but the gate did not
        // read it, so a run that panicked on the shutdown path still
        // printed `pipeline=ok` and exited 0 — the exact opposite of the
        // token's reason for being added.
        let good = Counters {
            frames: 1,
            content: 1,
            cells: 8,
            glyphs: 6,
            rules: 15,
            motion: 3,
            slide: 2,
        };
        let settled = MotionState::Settled;
        for teardown in [Teardown::ReaderPanicked, Teardown::Panicked] {
            assert!(
                matches!(
                    verdict(
                        good,
                        Workload::Smoke,
                        Some(teardown),
                        settled,
                        HEALTHY_QUIET,
                        HEALTHY_BACK
                    ),
                    Verdict::ShutdownPanicked { .. }
                ),
                "{teardown:?} cannot pass green"
            );
            assert!(
                matches!(
                    verdict(
                        good,
                        Workload::Load,
                        Some(teardown),
                        settled,
                        HEALTHY_QUIET,
                        None
                    ),
                    Verdict::ShutdownPanicked { .. }
                ),
                "{teardown:?} cannot pass green under the measurement workload either"
            );
        }

        // Recorded debts are **not wired** to the gate: `Abandoned` happens in one of the
        // measurement workload's four runs (measured) and turning `make smoke` red
        // over a known debt would make the gate useless.
        for teardown in [
            Some(Teardown::Clean),
            Some(Teardown::Abandoned),
            Some(Teardown::Unbounded),
            Some(Teardown::AlreadyDone),
            None,
        ] {
            assert_eq!(
                verdict(
                    good,
                    Workload::Smoke,
                    teardown,
                    settled,
                    HEALTHY_QUIET,
                    HEALTHY_BACK
                ),
                Verdict::Pass
            );
        }

        // A missing counter comes **before** panic: sending the reader to the more basic
        // fault first is right.
        assert_eq!(
            verdict(
                Counters { frames: 0, ..good },
                Workload::Smoke,
                Some(Teardown::Panicked),
                settled,
                HEALTHY_QUIET,
                HEALTHY_BACK,
            ),
            Verdict::MissingCounter {
                required: "all five must be >0"
            }
        );
    }

    #[test]
    fn timed_run_does_not_see_the_user() {
        // A timed run does not call the loader: even if the home directory resolves the decision
        // is `Hermetic`. `make smoke` tokens lean on this line — `cells=`
        // and `glyphs=` are not tied to the machine's font, nor `motion=` to the machine's
        // `[motion] cursor_motion`. In a hermetic run the style
        // comes from `Settings::default()` (`start_session`), i.e. from the
        // single owner of the defaults.
        let home = Some(PathBuf::from("/Users/someone"));
        for workload in [Workload::Smoke, Workload::Load] {
            let run = Run {
                seconds: 3,
                workload,
                stats_since: None,
                journal: false,
            };
            assert_eq!(decide_inputs(Some(run), home.clone()), Inputs::Hermetic);
        }
        assert_eq!(
            decide_inputs(None, home),
            Inputs::User {
                config_root: Some(PathBuf::from("/Users/someone/.config/bateri"))
            }
        );
        assert_eq!(
            decide_inputs(None, None),
            Inputs::User { config_root: None }
        );
    }

    #[test]
    fn smooth_scroll_is_off_when_any_input_turns_motion_off() {
        // If any of the three inputs turns motion off, the line step applies.
        let on = Settings::default();
        assert!(resolve_smooth_scroll(&on, false), "default is not smooth");
        assert!(
            !resolve_smooth_scroll(&on, true),
            "glided under Reduce Motion"
        );
        let off = Settings {
            smooth_scroll: SmoothScroll::Off,
            ..Settings::default()
        };
        assert!(!resolve_smooth_scroll(&off, false));
        let snap = Settings {
            cursor_motion: CursorMotion::Snap,
            ..Settings::default()
        };
        assert!(!resolve_smooth_scroll(&snap, false), "glided under snap");
        // The other two styles leave the glide on.
        for style in [CursorMotion::Ease, CursorMotion::Spring] {
            let settings = Settings {
                cursor_motion: style,
                ..Settings::default()
            };
            assert!(resolve_smooth_scroll(&settings, false), "{style:?}");
        }
        // Timed run: settings are not read, Reduce Motion resolves to `false`, i.e.
        // smooth — without depending on the system setting.
        let reduce = resolve_reduce_motion(&Inputs::Hermetic, ReduceMotion::System, || {
            panic!("timed run read the system setting")
        });
        assert!(resolve_smooth_scroll(&Settings::default(), reduce));
    }

    #[test]
    fn the_scroll_bar_form_follows_the_setting_and_the_system() {
        // A timed run never asks the system and never reserves the track:
        // `make smoke`'s grid and tokens must not depend on the measuring
        // machine's "Show scroll bars" (or a mouse plugged into it).
        for setting in [
            Scrollbar::System,
            Scrollbar::Auto,
            Scrollbar::Always,
            Scrollbar::Never,
        ] {
            assert_eq!(
                resolve_scrollbar(&Inputs::Hermetic, setting, || panic!(
                    "timed run read the system's scroller style"
                )),
                ScrollbarMode::Auto,
                "{setting:?}"
            );
        }
        let user = Inputs::User { config_root: None };
        // `"system"`: overlay scrollers hide themselves, legacy ones stay.
        assert_eq!(
            resolve_scrollbar(&user, Scrollbar::System, || true),
            ScrollbarMode::Auto
        );
        assert_eq!(
            resolve_scrollbar(&user, Scrollbar::System, || false),
            ScrollbarMode::Always
        );
        // The other values decide for themselves: the system is never asked.
        for (setting, mode) in [
            (Scrollbar::Auto, ScrollbarMode::Auto),
            (Scrollbar::Always, ScrollbarMode::Always),
            (Scrollbar::Never, ScrollbarMode::Never),
        ] {
            assert_eq!(
                resolve_scrollbar(&user, setting, || panic!("{setting:?} read the system")),
                mode
            );
        }
    }

    #[test]
    fn hermetic_run_does_not_read_reduce_motion() {
        // `Inputs`'s fifth condition: a timed run does **not** read the
        // system's Reduce Motion setting. If it did, `make smoke`'s
        // `motion=` token would depend on the measuring machine's accessibility preference —
        // a gate green on one machine and red on another.
        // The closure's panic tests this more sharply than a "did not read" claim:
        // pinning the return to `false` would also pass code that reads and ignores it.
        for setting in [ReduceMotion::System, ReduceMotion::On, ReduceMotion::Off] {
            assert!(
                !resolve_reduce_motion(&Inputs::Hermetic, setting, || panic!(
                    "timed run read the system setting"
                )),
                "{setting:?} throttled motion in a hermetic run"
            );
        }

        let user = Inputs::User { config_root: None };
        // `"on"` and `"off"` decide for themselves: the system is never consulted.
        assert!(resolve_reduce_motion(&user, ReduceMotion::On, || panic!(
            "\"on\" read the system setting"
        )));
        assert!(!resolve_reduce_motion(&user, ReduceMotion::Off, || panic!(
            "\"off\" read the system setting"
        )));
        // `"system"` only does what the system says.
        assert!(resolve_reduce_motion(&user, ReduceMotion::System, || true));
        assert!(!resolve_reduce_motion(&user, ReduceMotion::System, || {
            false
        }));
    }

    /// `BATERI_BIN` rides only with an installed wrapper, and only as UTF-8.
    #[test]
    fn the_binary_path_rides_only_with_the_wrapper() {
        let wrapper = vec![("ZDOTDIR".to_owned(), "/w".to_owned())];
        let bin = Some(PathBuf::from(
            "/Applications/bateri.app/Contents/MacOS/bateri",
        ));
        assert_eq!(
            with_bateri_bin(wrapper.clone(), bin.clone(), None).last(),
            Some(&(
                "BATERI_BIN".to_owned(),
                "/Applications/bateri.app/Contents/MacOS/bateri".to_owned()
            ))
        );
        // The masters' instance rides with the binary, never alone.
        assert_eq!(
            with_bateri_bin(wrapper.clone(), bin.clone(), Some("0a1b2c3d")).last(),
            Some(&("BATERI_SSH_INSTANCE".to_owned(), "0a1b2c3d".to_owned()))
        );
        assert!(with_bateri_bin(Vec::new(), bin, Some("0a1b2c3d")).is_empty());
        assert_eq!(
            with_bateri_bin(wrapper.clone(), None, Some("0a1b2c3d")),
            wrapper
        );
        use std::os::unix::ffi::OsStringExt as _;
        let odd = std::ffi::OsString::from_vec(b"/x/\xff".to_vec());
        assert_eq!(
            with_bateri_bin(wrapper.clone(), Some(PathBuf::from(odd)), None),
            wrapper
        );
    }

    /// The fixed input of the arm where integration is installed: zsh + a directory with a body.
    fn zsh_and_dir() -> (
        impl FnOnce() -> Option<PathBuf>,
        impl FnOnce() -> Option<PathBuf>,
    ) {
        (
            || Some(PathBuf::from("/bin/zsh")),
            || Some(PathBuf::from("/opt/bateri/shell/zsh")),
        )
    }

    #[test]
    fn blocks_keeps_the_wrapper_and_drops_the_dock() {
        // **The acceptance criterion.** At the `"blocks"` tier the wrapper
        // is installed — `ZDOTDIR` goes, so blocks and marks work —
        // but the window is born **without a dock**: the input line and the prompt both
        // stay in the grid.
        let user = Inputs::User { config_root: None };
        let (shell, dir) = zsh_and_dir();
        let env = shell_integration_env(&user, ShellIntegration::Blocks, shell, dir, None);
        assert!(
            env.iter().any(|(key, _)| key == "ZDOTDIR"),
            "blocks did not install the wrapper: blocks would die too"
        );
        assert_eq!(
            dock_rows_at_birth(&env, ShellIntegration::Blocks),
            0,
            "dock gutter was reserved at the blocks tier: there would be two prompts on screen"
        );

        // `"auto"` sets up the same environment and **reserves** the gutter: what
        // separates the two tiers is not the environment but this decision.
        let (shell, dir) = zsh_and_dir();
        let env = shell_integration_env(&user, ShellIntegration::Auto, shell, dir, None);
        assert_eq!(dock_rows_at_birth(&env, ShellIntegration::Auto), DOCK_ROWS);

        // If the wrapper was never installed there is no gutter whatever the tier:
        // there is no mirror to fill it.
        for setting in [
            ShellIntegration::Auto,
            ShellIntegration::Blocks,
            ShellIntegration::Off,
        ] {
            assert_eq!(dock_rows_at_birth(&[], setting), 0, "{setting:?}");
        }
    }

    #[test]
    fn hermetic_run_does_not_set_up_shell_integration() {
        // `Inputs`'s sixth condition: a timed run **never** installs
        // integration. If it did, `make smoke`'s result would depend on the measuring
        // machine's shell configuration — were the user's `.zshrc` to write a single byte to
        // the window `cells=8` would fail. The closure's panic is sharper than a
        // "did not install" claim: pinning an empty return would also pass code that resolves
        // the shell and throws the result away.
        for setting in [ShellIntegration::Auto, ShellIntegration::Off] {
            let env = shell_integration_env(
                &Inputs::Hermetic,
                setting,
                || panic!("timed run resolved the shell"),
                || panic!("timed run looked for the script"),
                Some("/home/someone/zsh".into()),
            );
            assert!(
                env.is_empty(),
                "{setting:?} added environment in a hermetic run"
            );
        }
    }

    #[test]
    fn hermetic_run_does_not_restore_or_save() {
        // A timed run never reads nor writes the saved session — `make smoke`
        // must neither depend on the user's last quit nor overwrite it. The closures' panic is
        // the gate, as in `hermetic_run_does_not_set_up_shell_integration`.
        let dir = restore_dir(
            &Inputs::Hermetic,
            || panic!("timed run asked the bundle id"),
            || panic!("timed run resolved the home directory"),
        );
        assert_eq!(dir, None);
    }

    #[test]
    fn an_unbundled_process_does_not_restore_or_save() {
        let user = Inputs::User { config_root: None };
        assert_eq!(
            restore_dir(&user, || None, || panic!("no bundle, no home lookup")),
            None,
            "`cargo run` has no bundle id"
        );
        assert_eq!(
            restore_dir(&user, || Some("dev.bateri.bateri".into()), || None),
            None
        );
        assert_eq!(
            restore_dir(
                &user,
                || Some("dev.bateri.agent-check".into()),
                || Some(PathBuf::from("/Users/someone"))
            ),
            Some(PathBuf::from(
                "/Users/someone/Library/Application Support/bateri/session/dev.bateri.agent-check"
            )),
            "named by the bundle: a development copy never shares the installed app's session"
        );
    }

    #[test]
    fn a_restored_pane_keeps_its_identity_and_readies_its_remote_line() {
        let id = TabId::parse("0A1B2C3D-4E5F-4061-8293-A4B5C6D7E8F9").expect("canonical");
        let pane = SavedPane {
            tab_id: id.clone(),
            dir: Some(PathBuf::from("/tmp/proje dizini")),
            zoom_steps: 2,
            remote_line: Some("ssh prod".into()),
            history: true,
        };
        let launch = restored_launch(&pane, Some(b"ls\r\n".to_vec()));
        assert_eq!(launch.tab_id, Some(id));
        assert_eq!(launch.working_directory, pane.dir);
        assert_eq!(launch.replay.as_deref(), Some(&b"ls\r\n"[..]));
        assert_eq!(
            launch.initial_input,
            Some(InitialInput::ready("ssh prod")),
            "the remote line waits for the user's ⏎"
        );
        let local = SavedPane {
            remote_line: None,
            ..pane
        };
        assert_eq!(restored_launch(&local, None).initial_input, None);
    }

    fn frame(x: f64, y: f64, width: f64, height: f64) -> Frame {
        Frame {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn a_frame_on_a_screen_stays_where_it_was() {
        let screens = [frame(0.0, 0.0, 1440.0, 875.0)];
        let saved = frame(100.0, 120.0, 900.0, 600.0);
        assert_eq!(clamp_frame(saved, &screens), saved);
    }

    #[test]
    fn a_frame_of_an_unplugged_screen_comes_to_the_main_one() {
        // The second display is gone; the frame lies wholly outside the remaining one.
        let screens = [frame(0.0, 0.0, 1440.0, 875.0)];
        let clamped = clamp_frame(frame(2000.0, 300.0, 900.0, 600.0), &screens);
        assert_eq!(clamped, frame(540.0, 275.0, 900.0, 600.0));
    }

    #[test]
    fn a_frame_larger_than_the_screen_shrinks_to_it() {
        let screens = [frame(0.0, 25.0, 1280.0, 775.0)];
        let clamped = clamp_frame(frame(-50.0, 0.0, 2560.0, 1400.0), &screens);
        assert_eq!(clamped, frame(0.0, 25.0, 1280.0, 775.0));
    }

    #[test]
    fn a_frame_goes_to_the_screen_it_overlaps_most() {
        let screens = [
            frame(0.0, 0.0, 1440.0, 875.0),
            frame(1440.0, 0.0, 1920.0, 1055.0),
        ];
        // Mostly on the second screen, sticking out past its right edge.
        let clamped = clamp_frame(frame(3000.0, 100.0, 900.0, 600.0), &screens);
        assert_eq!(clamped, frame(2460.0, 100.0, 900.0, 600.0));
        assert_eq!(
            clamp_frame(frame(5.0, 5.0, 10.0, 10.0), &[]),
            frame(5.0, 5.0, 10.0, 10.0)
        );
    }

    #[test]
    fn only_a_new_tab_follows_a_remote_tab() {
        // The three arms: on a remote tab ⌘T (and `+`) carries the same
        // command; ⌥⌘T and ⌘N are local even from a remote tab; on a local tab ⌘T is local.
        let remote = || Some("ssh -p 2222 prod".to_owned());
        assert_eq!(
            initial_line(Opening::Tab, remote()).as_deref(),
            Some("ssh -p 2222 prod")
        );
        assert_eq!(
            initial_line(Opening::Split, remote()).as_deref(),
            Some("ssh -p 2222 prod"),
            "a split goes to the same host like ⌘T"
        );
        assert_eq!(initial_line(Opening::LocalTab, remote()), None);
        assert_eq!(initial_line(Opening::Window, remote()), None);
        assert_eq!(
            initial_line(Opening::Restore, remote()),
            None,
            "a restored pane's line comes from the save, ready and not run"
        );
        for opening in [
            Opening::Tab,
            Opening::LocalTab,
            Opening::Window,
            Opening::Split,
            Opening::Restore,
        ] {
            assert_eq!(initial_line(opening, None), None, "{opening:?}");
        }
    }

    #[test]
    fn shell_integration_off_asks_nothing() {
        // `"off"` decides on its own: neither is a shell resolved nor a script
        // looked up. The key's meaning is "do not install the wrapper" and that job ends
        // here — the path that parses the marks (`bt-core`) does not go through this arm,
        // so a real OSC 133 printed by another tool is still read.
        let env = shell_integration_env(
            &Inputs::User { config_root: None },
            ShellIntegration::Off,
            || panic!("\"off\" resolved the shell"),
            || panic!("\"off\" looked for the script"),
            None,
        );
        assert!(env.is_empty());
    }

    #[test]
    fn shell_integration_needs_zsh_and_a_script() {
        let user = Inputs::User { config_root: None };
        // A shell we do not recognize: not even a script is looked up, there is nothing to install.
        let env = shell_integration_env(
            &user,
            ShellIntegration::Auto,
            || Some(PathBuf::from("/bin/bash")),
            || panic!("script looked up for a non-zsh shell"),
            None,
        );
        assert!(env.is_empty(), "wrapper installed for bash");
        // The shell could not be resolved at all (passwd unreadable, no `$SHELL`): the same silent
        // fallback.
        let env = shell_integration_env(&user, ShellIntegration::Auto, || None, || None, None);
        assert!(env.is_empty(), "wrapper installed for a shell-less session");
        // The shell is zsh but there is no script (missing package): a session without integration
        // is better than a half-installed `ZDOTDIR` — the user's configuration
        // would never have loaded.
        let env = shell_integration_env(
            &user,
            ShellIntegration::Auto,
            || Some(PathBuf::from("/bin/zsh")),
            || None,
            None,
        );
        assert!(env.is_empty(), "betiksiz ZDOTDIR kuruldu");
    }

    #[test]
    fn shell_integration_hands_the_original_zdotdir_to_the_script() {
        let user = Inputs::User { config_root: None };
        // The user has no `ZDOTDIR`: only our own directory goes to the script
        // and `BATERI_ZDOTDIR`'s **absence** means "the user had none either".
        let (shell, dir) = zsh_and_dir();
        let env = shell_integration_env(&user, ShellIntegration::Auto, shell, dir, None);
        assert_eq!(
            env,
            vec![("ZDOTDIR".to_owned(), "/opt/bateri/shell/zsh".to_owned())]
        );
        // An empty value counts as unset (`decide_locale`'s rule): "putting it
        // back" would create a variable pointing at `$HOME`.
        let (shell, dir) = zsh_and_dir();
        let env = shell_integration_env(
            &user,
            ShellIntegration::Auto,
            shell,
            dir,
            Some(OsString::new()),
        );
        assert_eq!(
            env.len(),
            1,
            "an empty ZDOTDIR was counted as a value to restore"
        );
        // The user has a `ZDOTDIR`: the second pair goes too so the script
        // can put it back.
        let (shell, dir) = zsh_and_dir();
        let env = shell_integration_env(
            &user,
            ShellIntegration::Auto,
            shell,
            dir,
            Some("/home/someone/zsh".into()),
        );
        assert_eq!(
            env,
            vec![
                ("ZDOTDIR".to_owned(), "/opt/bateri/shell/zsh".to_owned()),
                ("BATERI_ZDOTDIR".to_owned(), "/home/someone/zsh".to_owned()),
            ]
        );
    }

    #[test]
    fn only_a_shell_prompt_is_announced_to_the_script() {
        // **The default's only record is in the script.** On the `"terminal"` arm not a single
        // byte is added to the environment: the script's "no variable → the prompt is the terminal's" rule
        // carries the default alone. Had a value been sent here too
        // the default would be written in two places and when one changed the other
        // would silently go stale.
        let user = Inputs::User { config_root: None };
        let (shell, dir) = zsh_and_dir();
        let env = shell_integration_env(&user, ShellIntegration::Auto, shell, dir, None);
        assert!(
            !env.iter().any(|(key, _)| key == "BATERI_DOCK"),
            "the default tier wrote something to the environment"
        );

        // `"blocks"`: the wrapper **is installed** (for blocks and marks) but the
        // input line and the prompt stay the shell's. This variable is the only difference
        // that goes to the script; not reserving the dock gutter is a separate decision and is on this side
        // (`ShellIntegration::wants_dock`, `birth`).
        let (shell, dir) = zsh_and_dir();
        let env = shell_integration_env(&user, ShellIntegration::Blocks, shell, dir, None);
        assert_eq!(
            env,
            vec![
                ("ZDOTDIR".to_owned(), "/opt/bateri/shell/zsh".to_owned()),
                ("BATERI_DOCK".to_owned(), "off".to_owned()),
            ]
        );

        // `"off"` is outside the three tiers: with no wrapper installed it does not matter who
        // draws the prompt, and the "nothing is installed" promise is absolute.
        let env = shell_integration_env(
            &user,
            ShellIntegration::Off,
            || panic!("\"off\" resolved the shell"),
            || panic!("\"off\" looked for the script"),
            None,
        );
        assert!(env.is_empty());
    }

    #[test]
    fn a_self_referential_zdotdir_is_not_handed_back() {
        // The `ZDOTDIR` in the environment is already **the script's own directory**: there is no
        // "user value" to put back. If it were given the script would reload its own `.zshenv`
        // and recurse up to zsh's `FUNCNEST` limit; the measured
        // result was 336 lines of errors and a session left without `ZDOTDIR`
        // (found in code review).
        let user = Inputs::User { config_root: None };
        let (shell, dir) = zsh_and_dir();
        let env = shell_integration_env(
            &user,
            ShellIntegration::Auto,
            shell,
            dir,
            Some("/opt/bateri/shell/zsh".into()),
        );
        assert_eq!(
            env,
            vec![("ZDOTDIR".to_owned(), "/opt/bateri/shell/zsh".to_owned())],
            "a self-referential ZDOTDIR was handed back to the script"
        );
    }

    #[test]
    fn a_non_utf8_zdotdir_refuses_the_integration() {
        // "Absent" and "unusable" are separate: `var().ok()` merged the two
        // and the result was silent data loss — the script, thinking "the user had none",
        // would **delete** `ZDOTDIR` at session end, i.e. the user's whole
        // configuration would vanish undiagnosed. Not installing integration at all
        // is the fallback the neighboring edges (non-UTF-8 script path, unrecognized `$SHELL`)
        // already chose.
        use std::os::unix::ffi::OsStringExt as _;
        let user = Inputs::User { config_root: None };
        let (shell, dir) = zsh_and_dir();
        let env = shell_integration_env(
            &user,
            ShellIntegration::Auto,
            shell,
            dir,
            Some(OsString::from_vec(vec![0x2f, 0xff, 0xfe])),
        );
        assert!(env.is_empty(), "wrapper installed with a non-UTF-8 ZDOTDIR");
    }

    #[test]
    fn zero_window_does_not_panic() {
        // A minimized window gives 0×0 bounds; `Session::resize` ignores a
        // zero grid but the path leading here must not panic —
        // not the split, the `as u16` saturation carries it.
        let g = split_into_grid(0.0, 0.0, metrics(9, 18, 8), NO_DOCK, NO_RESERVE, NO_TOP);
        assert_eq!((g.cols, g.rows), (0, 0));
    }

    fn held(blob: Vec<u8>, ended: bool) -> HeldPane {
        let file = std::fs::File::open("/dev/null").unwrap();
        let mut pane = HeldPane::new(
            bt_core::TabId::parse("0F1E2D3C-4B5A-6978-8796-A5B4C3D2E1F0").unwrap(),
            42,
            7,
            blob,
            b"tail".to_vec(),
            std::os::fd::OwnedFd::from(file),
        );
        pane.ended = ended;
        pane
    }

    /// A holder handed its panes carries a frozen screen; a crashed one's
    /// rebuilt screen does not make it deliberate, nor does an empty one.
    #[test]
    fn only_a_frozen_screen_marks_a_deliberate_holder() {
        let frozen = held(pane_state(b"").encode(), false);
        let mut rebuilt = held(pane_state(b"").encode(), false);
        rebuilt.crashed = true;
        let mut screenless = pane_state(b"");
        screenless.vt.clear();
        let screenless = held(screenless.encode(), false);
        assert_eq!(
            deliberate_of(3, &[(0, frozen), (1, rebuilt), (2, screenless)]),
            [true, false, false]
        );
    }

    fn pane_state(history: &[u8]) -> PaneState {
        PaneState {
            cols: 80,
            rows: 24,
            parent: crate::jobs::ShellParent::Login,
            vt: b"vt".to_vec(),
            core: b"core".to_vec(),
            input: Vec::new(),
            history: history.to_vec(),
        }
    }

    fn a_watch(_: u32, _: u64) -> Option<std::os::fd::OwnedFd> {
        Some(std::os::fd::OwnedFd::from(
            std::fs::File::open("/dev/null").unwrap(),
        ))
    }

    /// The relaunch waits for a transfer or an
    /// open password sheet, either alone; with neither it goes at once.
    #[test]
    fn an_update_waits_for_a_transfer_or_a_password_sheet() {
        assert!(!UpdateWait::default().holds());
        assert!(
            UpdateWait {
                transfers: 1,
                sheet: false
            }
            .holds()
        );
        assert!(
            UpdateWait {
                transfers: 0,
                sheet: true
            }
            .holds()
        );
    }

    /// A held pane is carried on only alive, readable and watched; the
    /// others fall back with whatever history their blob carried.
    #[test]
    fn a_held_pane_is_adopted_or_falls_back_with_its_history() {
        let blob = pane_state(b"history").encode();
        let adopted = adoption(held(blob.clone(), false), |pid, start| {
            assert_eq!((pid, start), (42, 7));
            a_watch(pid, start)
        })
        .expect("an alive, readable, watched pane is adopted");
        assert_eq!((adopted.pid, adopted.prefix.as_slice()), (42, &b"tail"[..]));
        assert_eq!(adopted.state, pane_state(b"history"));

        let ended = adoption(held(blob.clone(), true), a_watch).unwrap_err();
        assert_eq!(ended.history.as_deref(), Some(&b"history"[..]));
        let gone = adoption(held(blob, false), |_, _| None).unwrap_err();
        assert_eq!(gone.history.as_deref(), Some(&b"history"[..]));
        let unread = adoption(held(b"not a blob".to_vec(), false), a_watch).unwrap_err();
        assert_eq!(unread.history, None);
        let empty = adoption(held(pane_state(b"").encode(), true), a_watch).unwrap_err();
        assert_eq!(empty.history, None, "an empty history is no history");
        // Whether the program is known to have ended: the holder saw it, or
        // its pid is gone — not an unreadable blob.
        assert!(ended.ended && gone.ended && empty.ended);
        assert!(!unread.ended);
    }

    /// The note of a pane whose program did not come back says what
    /// happened: the update's holder, a crash, a deliberate handover that did
    /// not carry it, or the program's own end while bateri was closed.
    #[test]
    fn a_fallen_pane_says_what_happened() {
        let crash = HolderKind::Bound { deliberate: false };
        let quit = HolderKind::Bound { deliberate: true };
        assert_eq!(fallen_note(HolderKind::Update, false), Note::Update);
        assert_eq!(fallen_note(HolderKind::Update, true), Note::Update);
        assert_eq!(fallen_note(crash, false), Note::Crash);
        assert_eq!(fallen_note(crash, true), Note::Ended);
        assert_eq!(fallen_note(quit, false), Note::NotCarried);
        assert_eq!(fallen_note(quit, true), Note::Ended);
        assert_eq!(HolderKind::Update.mode(), AdoptMode::Update);
        assert_eq!(crash.mode(), AdoptMode::Bound);
    }

    /// `restore_windows` decides what a pane without its program brings:
    /// `"all"` its history (none in the second attempt) and the note,
    /// `"layout"` the note alone, `"off"` nothing — the pane does not come.
    #[test]
    fn restore_windows_decides_a_fallen_panes_fate() {
        let history = || Some(b"$ ls\r\n".to_vec());
        let note = Note::Crash;
        assert_eq!(
            fallen_replay(RestoreWindows::All, false, history(), note),
            Some(fallen_back(history(), note))
        );
        assert_eq!(
            fallen_replay(RestoreWindows::All, true, history(), note),
            Some(fallen_back(None, note)),
            "the second attempt replays no history"
        );
        assert_eq!(
            fallen_replay(RestoreWindows::Layout, false, history(), note),
            Some(fallen_back(None, note))
        );
        assert_eq!(
            fallen_replay(RestoreWindows::Off, false, history(), note),
            None
        );
    }

    fn adopted(vt: &[u8]) -> Adopted {
        let mut state = pane_state(b"history");
        state.vt = vt.to_vec();
        adoption(held(state.encode(), false), a_watch).expect("adopted")
    }

    /// A frozen screen comes back whole; a crash's (none) is the note on the
    /// first line and a nudge; a cut one gets the note under it and a nudge;
    /// the second attempt replays nothing that went through the parser.
    #[test]
    fn an_adopted_panes_screen_and_its_nudge() {
        let mut whole = adopted(b"screen");
        assert!(!adopted_screen(&mut whole, false, false));
        assert_eq!(whole.state.vt, b"screen");

        let mut crash = adopted(b"");
        assert!(adopted_screen(&mut crash, false, false));
        assert_eq!(crash.state.vt, Note::Screenless.line());
        assert_eq!(crash.state.core, b"core", "the state still comes");
        assert_eq!(crash.prefix, b"tail", "the held output still comes");

        let mut cut = adopted(b"screen");
        assert!(adopted_screen(&mut cut, true, false));
        assert_eq!(
            cut.state.vt,
            [&b"screen\r\n"[..], &Note::Cut.line()].concat()
        );

        let mut safe = adopted(b"screen");
        assert!(adopted_screen(&mut safe, true, true));
        assert_eq!(safe.state.vt, Note::Screenless.line());
        assert!(safe.state.core.is_empty() && safe.prefix.is_empty());
    }

    /// The modifier state is readable before `NSApplication` exists — the
    /// sequence point's ⇧ — and a timed run never asks.
    #[test]
    fn shift_is_read_without_an_application() {
        let _ = NSEvent::modifierFlags_class();
        let timed = Options {
            run: Some(Run {
                seconds: 1,
                workload: Workload::Smoke,
                stats_since: None,
                journal: false,
            }),
        };
        assert!(!shift_held_at_launch(&timed));
    }
}
