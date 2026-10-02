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

use bt_core::{
    CursorMotion, HostMark, ReduceMotion, SHUTDOWN_GRACE, SYSTEM_THEME, Settings, SettingsEdit,
    ShellIntegration, SmoothScroll, TabId, Teardown, Theme,
};
use bt_gpu::{CellMetrics, DOCK_ROWS, DisplayLink, MIN_SAMPLES, Renderer, Stats};
use dispatch2::{DispatchQueue, DispatchTime};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAlertFirstButtonReturn, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication,
    NSApplicationDelegate, NSApplicationTerminateReply, NSEvent, NSMenu, NSMenuDelegate,
    NSMenuItem, NSWindow, NSWorkspace, NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification,
};
use objc2_foundation::{
    NSArray, NSDictionary, NSKeyValueObservingOptions, NSNotification, NSNumber, NSObject,
    NSObjectNSDelayedPerforming, NSObjectNSKeyValueObserverRegistration, NSObjectProtocol,
    NSRunLoopCommonModes, NSString, NSURL, NSUserDefaults, ns_string,
};

use crate::menu::ShellMenuDelegate;
use crate::notices::{Notices, Source};
use crate::pane::{PaneLaunch, TerminalPane};
use crate::preview_cache;
use crate::remote_files::Sweep;
use crate::settings_window::SettingsWindow;
use crate::split::Axis;
use crate::ssh_route::{self, Masters};
use crate::watch::{Notify, Watch};
use crate::window::{self, CloseScope, Launch, TerminalWindow, WindowHost};
use crate::zoom::Zoom;
use crate::{Options, Run, Workload};
use crate::{child, settings};

/// Guard for zero idle frames: in the [`Workload::Smoke`] workload the window
/// sits idle for ~`run_seconds` seconds after the first draw.
///
/// **The operand of the limit is not `frames` but [`Counters::content`]**
/// (the `content=` token): not the frame the GPU finished but the content
/// frame **decided to be drawn**. The reason is an aged debt in this limit's
/// own doc: "an animation with a forgotten stop condition passes today's gate
/// green". The remedy was not to move the limit but to keep motion frames out
/// of the gate: a cursor slide legitimately raises `frames` to ~24 and never
/// raises `content` (`.tasks/008-hareket-ve-imlec/discussion.md` → Karar 2).
///
/// **The relation between the two counters broke with motion** (a
/// `/code-review` finding): when the operand changed it was written that
/// "every frame that ends without error was a content frame", and that
/// sentence was true in phase-1, **not** after phase-3 — a motion frame also
/// commits a command buffer, so it raises `frames` without raising `content`.
/// The direction has even reversed today: the measured healthy smoke run has
/// `frames` 27–30 while `content` is 2–3 (the 008 row below). So the limit
/// sits on a **looser** counter, not a tighter one — and that is why a
/// number that was **re-measured**, not carried over, was needed; phase-6 measured it.
///
/// **The number was measured twice; the second time in a visible window, and
/// it did not change it.** Run tables, environment and method are in
/// `docs/OLCUMLER.md` → `## Boşta kare`; here only the poles that gave rise
/// to the limit and the derivation are kept.
///
/// - **005 phase-3 (2026-09-12, debug, unbundled process):** `2` → `8`. The
///   basis of the old `2` ("the system suspends the display link, ceiling ~3
///   frames") was refuted in 005 phase-2b: [`Workload::Load`] produced
///   `frames=594` in five seconds in the same window state, so what was
///   measured was not a ceiling but a run corrupted by shutdown locking.
///   Moreover `2` **fell red on a correct build** (a healthy five-second run
///   was `frames=4`). Poles: healthy at most `4`, broken at least `49`.
/// - **006 phase-5 (2026-09-15, debug + release bundle; in the two probed
///   runs the window was on screen and in front):** the highest of fifty
///   healthy runs is `2`, the lowest of six broken runs is `353`. The visible
///   window did **not** raise the legitimate frame count; it did take the broken run to the full refresh rate.
///
/// `8` lies between the poles of the two measurements: twice the highest
/// healthy observation (`4`), a sixth of the lowest broken observation
/// (`49`). 006 only widened the gap; there is no observation that would move
/// the limit — lowering it would mean declaring 005's healthy `4` invalid without re-measuring it.
///
/// - **008 phase-6 (2026-09-16, debug + release bundle):** the **first**
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
/// on the regime. In the throttled regime (005: the measurement load gave
/// `frames=21` in 5 s, i.e. ~4 Hz) a broken three-second smoke makes ~12
/// frames, **1.5 times** `8` — this is the reason not to raise the limit from
/// here. No throttling was seen in 006's visible window. The likeliest
/// variable for the same binary giving two regimes under the measurement load
/// (`frames=21` in one run, `frames=597` in another) is window visibility, but
/// this is **unverified**: 006 saw the window on screen under the smoke load
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
/// **So the limit is not the whole gate, only one tier.** 008 builds the gate
/// in two tiers and both are independent of this one: (a) if an **unsettled**
/// animation remains at the deadline the run is red — independent of speed,
/// needs no measurement, but only sees animations that go through the motion
/// infrastructure; (b) the quiet between the last frame and the deadline
/// ([`QUIET_FLOOR`]) — sees leaks that bypass the infrastructure too and was
/// **measured** (phase-6): it is now the gate's most sensitive tier, because
/// it catches every leak with a period shorter than 868 ms, while this number catches only those above 3 Hz.
///
/// **The mechanism of the variation in healthy runs was not measured.** The
/// frame request (`requests=`) stayed **constant** in all three measurements
/// (2–3 in 006, 4 in 008), so the extra frames do not come from extra
/// **requests** — had it been the geometry/occlusion hooks, `requests` would
/// have risen too. The variation split by profile in all three measurements
/// but its direction **turned** in 008: in 006 `frames` was mostly `1` in
/// debug and `2` in the release bundle; in 008 `content` is mostly `3` in
/// debug and `2` in the release bundle. The request again did not split. What
/// remains is whether requests merge or not (if the startup frame was drawn
/// before the shell's first bytes a second frame is needed; the profile
/// difference could test this with `startup=`, it was not tested) but this is
/// a **hypothesis**, not a measurement.
///
/// **The gate is evaluated only on the `BT_RUN_SECONDS` path**
/// ([`AppDelegate::report_and_exit`]). The only unattended context is `make
/// smoke`; a run opened from the bundle with the same environment is subject
/// to the same limit (006's broken bundle runs fired it). An interactive run never evaluates this limit.
///
/// **When to re-measure:** when a set arrives that changes the frame path or
/// the window's visibility (motion, tabs). The recipe is in `docs/OLCUMLER.md` → `## Nasıl yeniden ölçülür`.
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
/// **The measured `requests ≈ frames + 2` relation became invalid in 008**
/// and what the sentence corrects is not a number but a mechanism: motion
/// frames never touch the `Waker` (the `bt_gpu::link` module header), so they
/// inflate `frames` without inflating `requests`. The new form of the
/// relation was **measured** (phase-6, thirty healthy runs): `requests` is
/// `4` in all thirty runs, `content` `2`–`3`, i.e. `requests ≈ content +
/// 1..2` — while `frames` is 27–30, completely detached from it.
/// (In a later run `requests=3` was seen and its cause was not measured; the record is in `docs/OLCUMLER.md`.)
/// Under the measurement load `requests` and `frames` differ by three orders
/// of magnitude (see `bt_gpu`'s `requests` counter); a gate could be built on
/// the ratio but that was not measured.
/// Re-observed after the 040 move to the wgpu window path (2026-09-30,
/// healthy and broken distributions in `docs/OLCUMLER.md`): unchanged.
const IDLE_FRAME_LIMIT: u64 = 8;

/// The **minimum** quiet expected at the end of a smoke run: between the last
/// drawn frame and the deadline (the `quiet=` token). Below it is red, `quiet=none` is red too.
///
/// The **complement of [`IDLE_FRAME_LIMIT`], not a copy.** That one sees a
/// leak drawing more than eight content frames in three seconds, i.e. only
/// above ~3 Hz; this one sees every leak whose **period** is shorter than
/// this value (above ~1.15 Hz). The measured gap was exactly this: a
/// half-second leak passes with `content=8` **without exceeding** the limit
/// and that run fell green today (`docs/OLCUMLER.md` → `## Boşta kare`, "yavaş sızıntı").
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
/// after 011 changed the smoke recipe, a twenty-run re-observation lowered
/// the band's lower end to `1737.12` and `870` exceeded the rule's ceiling by
/// **1.44 ms**. The gate was green in those runs — the excess hid in the
/// denominator, not in the number. Lesson: this constant's trigger is narrow
/// and **silent**; if a healthy three-second run drops below `1737 ms` what
/// breaks is not the gate but **the rule itself**, and the number must be re-derived with `/measure`.
///
/// **Four numbers are tied together and their rationales are in the same
/// block** (`docs/OLCUMLER.md` → `## Boşta kare`): `BT_RUN_SECONDS`'s 3,
/// [`bt_core::smoke_shell`]'s 1-second sleep, the same recipe's cursor jump
/// **distance** (011) and this floor. The quiet is `run duration − (sleep +
/// settling)`, so **if either of the two moves this number must move too**:
/// with `BT_RUN_SECONDS=2` the tail shrinks to ~0.75 seconds and the gate
/// falls while the code is right. If the three are spread over three files,
/// when one moves the gate silently becomes fragile.
///
/// **Known false positive** (same root as [`IDLE_FRAME_LIMIT`]'s): dragging
/// the window, covering and uncovering it or waking the screen during the
/// last `QUIET_FLOOR` of the run gives birth to a legitimate frame and resets
/// the tail. The lasting remedy is the same: keep geometry-caused frames out
/// of the counter (a recorded debt, `docs/YOL-HARITASI.md`).
///
/// Asked only in [`Workload::Smoke`]: the measurement load streams output
/// until the deadline, so there the quiet **must** be near zero (with the
/// same rationale as [`Verdict::MotionUnsettled`] being exempt in the same arm).
///
/// Re-observed after the 040 move to the wgpu window path (2026-09-30): the
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
/// tied to the user's file (`.tasks/007-ayarlar-ve-tema/discussion.md` → Karar 1).
///
/// **The fifth is Reduce Motion** ([`resolve_reduce_motion`], 008 phase-5) and
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
/// `bt-gpu` does not see AppKit (`CLAUDE.md` → layer table) and `bt-core`'s
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

/// `[motion] smooth_scroll` + Reduce Motion + `cursor_motion` → a single
/// `bool`: does the wheel go smooth.
///
/// If any of the three turns motion off, line stepping — scrolling does not
/// *add* animation for one who turned motion off (the same as
/// `cursor_motion = "snap"`'s relation to Reduce Motion). Quantization is
/// **at the source**, not in `bt-gpu`'s `Motion`: the `false` arm stays as
/// today's line path (`.tasks/027-yumusak-kaydirma/discussion.md` → Karar 5).
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
    /// ⌘T and the tab bar's `+`: a tab in `from`'s group; to the same host if
    /// `from` is remote (037 Karar 6).
    Tab,
    /// Shell ▸ New Local Tab (⌥⌘T): a tab, always a local shell.
    LocalTab,
    /// Shell ▸ Split Right / Split Down (⌘D / ⇧⌘D): a split next to the focused
    /// pane; by ⌘T's rule, to the same host from a remote pane (039 Karar 9).
    /// The axis is carried by [`AppDelegate::open_split`].
    Split,
}

/// The new shell's first input: only with ⌘T and splits and only from a remote
/// `from` — the line is `from`'s remote target's escaped line ([`bt_core::Session::remote_line`]).
/// ⌘N is a new workspace, ⌥⌘T the escape route; both are local (037 Karar 6).
fn initial_line(opening: Opening, remote_line: Option<String>) -> Option<String> {
    match opening {
        Opening::Tab | Opening::Split => remote_line,
        Opening::Window | Opening::LocalTab => None,
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
            // **A self-pointing value** (`/code-review`, 009 gate): if the
            // `ZDOTDIR` in the environment already points at the script's
            // directory (set by hand or leaked), handing it back as "the
            // user's original value" makes the script reload its own
            // `.zshenv` and recurse to zsh's `FUNCNEST` limit; the session is
            // left without `ZDOTDIR`. The script has a layer for this too, this is the first layer.
            Ok(value) if value == dir => None,
            Ok(value) => Some(value),
            // **A non-UTF-8 value rejects the integration entirely** and this
            // arm is the reason it wants `var_os` instead of `var`
            // (`/code-review`, 009 gate): `var().ok()` dropped it to `None`,
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
/// 1)` written there would revive and the two tests here would stay green.
///
/// **The left gutter is subtracted from the columns** (010 Karar 3): so the
/// stripe does not overlap the text. The gutter is always reserved — the
/// accepted cost is that it stays empty in a session without integration
/// (bash/fish, `shell.integration = false`, SSH); the alternative was a
/// SIGWINCH at the first prompt and three consumers being updated at once.
///
/// **The dock share is subtracted from the rows** (012) and, unlike the left
/// gutter, is **conditional**: the dock exists only in an integrated zsh
/// session and the decision is made while the session is born
/// (`TerminalPane::start`). Reserving the share unconditionally would take
/// two rows for no reason from a window without a dock — a cost not
/// comparable with the left gutter's eight points.
///
/// **The share varies during the run** (R5.2): it drops to zero on the
/// alternate screen and returns to its birth value on exit (`dock_rows_for`,
/// `TerminalPane::alt_screen_did_change`). The cost of varying is one `TIOCSWINSZ` and that cost is paid **per
/// transition, not per command** — commands like `git log` that do not enter
/// the alternate screen never move the flag, so this function is not called again either.
pub(crate) fn split_into_grid(
    width_px: f64,
    height_px: f64,
    cell: CellMetrics,
    dock_rows: u16,
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
    // The dock share is also **in `f64`** and for the same reason: in a window
    // shorter than the dock the difference goes negative, the division stays
    // negative and `as u16` saturates it to zero — `Session::resize` already
    // ignores that size. Done in `u16` it would overflow and produce a
    // 65535-row `TIOCSWINSZ`. The formula is **bt-gpu's** ([`bt_gpu::dock_px`]):
    // the dock's share carries two breathing margins next to the rows and were
    // it rewritten here it would diverge for one frame on resize — the same
    // discipline as consuming `DOCK_ROWS`, no second copy is kept.
    let usable_height = height_px - f64::from(bt_gpu::dock_px(dock_rows, cell));
    Grid {
        cols: (usable_width / f64::from(cell_w)) as u16,
        rows: (usable_height / f64::from(cell_h)) as u16,
        cell,
    }
}

/// The daily preview sweep's period (045 Karar 9) — "once a day", a design
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
/// reader thread and from background jobs find the pane with it (039 Karar
/// 3). A plain `fn`, i.e. `Send`, and the pane's module does not see `AppDelegate`.
pub(crate) fn pane_by_id(mtm: MainThreadMarker, id: u64) -> Option<Retained<TerminalPane>> {
    delegate(mtm)?.pane(id)
}

/// The notification of the watch sources ([`notify_settings_changed`]).
///
/// The watch notifies on its own background queue (`watch`'s contract); the
/// applier needs the main thread, so the event hops there.
///
/// **At most one hop in flight** ([`WATCH_PENDING`]): before 043 the sources
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

/// The dock share to reserve while the session is born (R5.1).
///
/// **Both conditions are necessary and separate questions.** If `integration`
/// is empty the wrapper was never set up — hermetic run, `"off"`, an
/// unrecognized shell, a non-UTF-8 script path — i.e. there is no mirror to
/// fill the dock. `wants_dock` is **the user's choice**: at the `"blocks"`
/// tier the wrapper is set up (blocks and marks are its whole reason) but the
/// input line stays in the grid, i.e. no share is reserved.
///
/// Deriving one from the other would bring back the defect 012 phase-10
/// closed: **two prompts** on screen (the user's in the grid, the dock's
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
/// the window draws no frames and no keys are processed (a `/code-review`
/// finding, waived at the 007 gate). Only when no application claims `.toml`
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
/// The side effect comes with `NSTextInputClient` itself (018 phase-1): in a
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
/// symptoms and whose only remedy is here (018 phase-1).
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
    /// The open windows (each tab is a window). **Owned here**: the window's
    /// delegate property is weak and `TerminalWindow` is held nowhere else.
    /// The only path that creates is [`AppDelegate::open_window`]; a closing
    /// window leaves one turn later ([`AppDelegate::forget_window`]).
    ///
    /// The on-save paths walk this list and, while walking, take a **copy** of
    /// it ([`AppDelegate::windows`]): a call going to a window can come back
    /// and reach here (`sync_geometry` → [`AppDelegate::post_notices`]).
    windows: RefCell<Vec<Retained<TerminalWindow>>>,
    /// The window-id counter ([`TerminalWindow::id`]);
    /// ids are not reused, so
    /// a stale message going to a closed window cannot find another window.
    next_window_id: Cell<u64>,
    /// Was the last-seen system appearance dark — the gate of
    /// [`AppDelegate::apply_appearance`]. `None`: no change has arrived yet (the first news always passes).
    ///
    /// The gate is a **saving**, not a correctness requirement: the KVO news
    /// also arrives when the appearance's name changes (accent color, high
    /// contrast) and the theme depends only on the light/dark bit; if the bit
    /// is the same there is no reason to reread the theme file and repaint all windows.
    appearance_dark: Cell<Option<bool>>,
    /// The settings window (bateri ▸ Settings…): born on first open, hidden
    /// when closed and lives for the whole process (029 Karar 4). **Not** a
    /// terminal window — it does not enter [`Ivars::windows`], i.e. ⌘Q's
    /// confirmation, settings propagation and tab jobs do not see it. Never born in a timed run.
    settings_window: RefCell<Option<Retained<SettingsWindow>>>,
    /// The settings file's state at its last read (029 Karar 7): the settings
    /// window's lock and line diagnostics come from here. Written at startup
    /// and at every live read, so even if the window opens later it sees the file's state.
    settings_state: RefCell<settings::FileState>,
    /// The Shell menu's delegate ([`crate::menu::install`]): the menu holds it
    /// weakly, this is what keeps it alive.
    shell_menu: OnceCell<Retained<ShellMenuDelegate>>,
    /// Sparkle's updater ([`crate::updater`]): "Check for Updates…" holds it
    /// weakly, this is what keeps it alive.
    /// Empty in an unbundled and timed run.
    updater: OnceCell<Retained<AnyObject>>,
    /// bateri's ssh masters (047): one registry for every pane, so two jobs to
    /// one host open one master ([`PaneLaunch::masters`]). `None` in a timed run
    /// (no askpass, no master — the remote jobs take today's argv) and when the
    /// running binary's path is unknown (it is the askpass program).
    masters: Option<Arc<Masters>>,
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
            // Native tabs are on (026 → Karar 1): `setAllowsAutomaticWindowTabbing`
            // at its default, the windows carry a common `tabbingIdentifier`
            // (`TerminalWindow::new`).
            // The updater comes **before** the menu: it is its item's target. A
            // timed run does not go out to the network and an update question must not cover the window.
            if self.ivars().run.is_none()
                && let Some(updater) = crate::updater::start()
            {
                let _ = self.ivars().updater.set(updater);
            }
            let shell_menu = crate::menu::install(
                mtm,
                ProtocolObject::from_ref(self),
                self.ivars().updater.get().map(|u| &**u),
            );
            let _ = self.ivars().shell_menu.set(shell_menu);
            // The settings are read **before** the first window: `scrollback` and
            // the theme enter `SessionOptions`, and the font setting also
            // determines the cell size, i.e. the first grid and the first
            // `TIOCSWINSZ` the shell sees. The window no longer needs to be born
            // first for the diagnostics to reach the subtitle: the new window takes
            // its subtitle over from the slots (`open_window`).
            self.load_settings();
            // The preview cache's launch sweep and the daily one (045 Karar 9):
            // on their own thread and the main queue's timer, never the frame
            // path; a timed run never touches the user's cache.
            self.sweep_previews(Sweep::Launch);
            self.schedule_daily_sweep();
            NSApplication::sharedApplication(mtm).activate();
            // The renderer is born with the window (026 → Karar 2a) and its error
            // lands here. `didFinishLaunching` cannot return an error; a terminal
            // window without Metal or without a shell is an empty box, and formerly
            // the error `run` returned was printed in `main` with the same line and
            // the same exit code. **Only for the first window**: the error of
            // ⌘T/⌘N does not end the process ([`AppDelegate::open_window_or_report`]).
            if let Err(e) = self.open_window(None, Opening::Window) {
                eprintln!("bateri: {e}");
                std::process::exit(1);
            }
            // The system's Reduce Motion notification is app-wide and once; the
            // window's first value descended to its own link in `start`.
            self.observe_reduce_motion();
            // The light/dark appearance is also app-wide and once; the first
            // window's theme was already chosen from the appearance (`open_window` → `resolve_theme`).
            self.observe_appearance();

            if let Some(run) = self.ivars().run {
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
        }

        /// After the last window closes the app **stays open** (026 → Karar 5):
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
            // panel would block the new window (`/code-review`); the list already
            // covers the minimized ones.
            let default = !self.ivars().windows.borrow().is_empty();
            if !default {
                self.open_window_or_report(None, Opening::Window);
            }
            default
        }

        /// `bateri://…` was opened (`open`, the browser, another app; 038
        /// Karar 4–6). URLs are processed in order, the last one comes to the front.
        ///
        /// **Security invariant: this path only focuses.** Any app can open the
        /// scheme; here not a single byte goes to the shell, no command runs, no
        /// window opens. Arms: `bateri://tab/<id>` and a live pane → its tab to
        /// the front and the keyboard to that pane ([`TerminalWindow::bring_to_front`],
        /// 039 Karar 10); a recognized but dead id →
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
                    Some((window, pane)) => window.bring_to_front(&pane),
                    None => NSApplication::sharedApplication(self.mtm()).activate(),
                }
            }
        }

        /// ⌘Q, Dock ▸ Quit, logout and restart: should it ask before quitting
        /// (028 → Karar 3, 5). The question is **one** alert for all windows;
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
            // (`CLAUDE.md`) holds on the Cmd-Q path too; the `if let` below alone
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

        /// Shell ▸ New Window (⌘N): a new window in the active window's
        /// directory and with its point-size delta (026 → Karar 3, 4). Here, not
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
        /// die in panels (`/code-review`).
        #[unsafe(method(closeTab:))]
        fn close_tab(&self, _sender: Option<&AnyObject>) {
            if let Some(key) = NSApplication::sharedApplication(self.mtm()).keyWindow() {
                key.performClose(None);
            }
        }

        /// The title of ⌘W while a non-terminal window is key: so the "Close"
        /// that a split tab left behind (`TerminalWindow`'s
        /// `validateMenuItem:`, 039 Karar 8) does not stay in the panel. **An
        /// unknown item is `true`** — the behavior before it was defined.
        #[unsafe(method(validateMenuItem:))]
        fn validate_menu_item(&self, item: &NSMenuItem) -> bool {
            if item.action() == Some(sel!(closeTab:)) {
                item.setTitle(&NSString::from_str(window::close_title(1)));
            }
            true
        }

        /// Shell ▸ New Tab (⌘T): a new tab in the active window's group; a new
        /// window if there is no window. If the active tab is remote the new tab
        /// is born with the same ssh/mosh command (037 Karar 6, [`initial_line`]).
        #[unsafe(method(newTab:))]
        fn new_tab(&self, _sender: Option<&AnyObject>) {
            self.open_from_key_window(Opening::Tab);
        }

        /// Shell ▸ New Local Tab (⌥⌘T): **always** a local tab, even from a
        /// remote tab (037 Karar 6) — ⌘T's escape route; in a local tab the same
        /// as ⌘T.
        #[unsafe(method(newLocalTab:))]
        fn new_local_tab(&self, _sender: Option<&AnyObject>) {
            self.open_from_key_window(Opening::LocalTab);
        }

        /// The tab bar's `+` button. AppKit shows the button only if someone in
        /// the responder chain recognizes this selector; the job is ⌘T's, the
        /// same host included in a remote tab.
        #[unsafe(method(newWindowForTab:))]
        fn new_window_for_tab(&self, _sender: Option<&AnyObject>) {
            self.open_from_key_window(Opening::Tab);
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

        /// Shell ▸ Mark “{host}” as ▸ {mark} (037 Karar 5): the item's `tag` is
        /// the mark ([`crate::menu::mark_of_tag`]), the host is the active tab's
        /// remote host. The menu only **writes** — the path that reads the file
        /// applies ([`AppDelegate::save_edit`], the Theme ▸ precedent); nothing
        /// is written to an unparseable file, the diagnostic goes to the write
        /// slot. A no-op if the tab became local in the meantime.
        #[unsafe(method(markHost:))]
        fn mark_host(&self, sender: Option<&AnyObject>) {
            let Some(mark) = sender
                .and_then(|sender| sender.downcast_ref::<NSMenuItem>())
                .and_then(|item| crate::menu::mark_of_tag(item.tag()))
            else {
                return;
            };
            if let Some((host, _)) = self.key_remote_mark() {
                self.save_edit(&SettingsEdit::RemoteHostMark { host, mark });
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
/// uses the same order the two would be wrong together (a `/code-review` finding).
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
    /// **It does not count blink** (014 phase-2): the cursor's blinking lives
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

/// The measurement ledger's summary at shutdown: read from the ring, not yet formatted.
///
/// The counter half (`samples`, `dropped`, `discarded`) is read **before** p95, because
/// [`bt_gpu::Samples::p95_and_worst`] consumes itself — the order is forced by
/// the type, not by the comment.
///
/// # The honest limits of the measurement
///
/// **The list moved from here.** Its owner since 2026-09-21 is
/// `docs/OLCUMLER.md` → `## Yöntem` → "Kare süresi ve açılış"; that kind's
/// first `/measure` did the move and the same run added a seventh item (the
/// GPU column not being stable enough to be a floor). **No copy is kept**
/// here: a list standing in two places silently diverges, which was this item's own warning.
///
/// The field docs below repeat from that list's **scope** items only the one
/// that falls to their own field; the whole and the **open items** are in that file.
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
    /// The adapter gives GPU timestamps (wgpu's `TIMESTAMP_QUERY`, 040
    /// phase-5); `false` → the GPU column's tokens say `unsupported` — the
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
    /// atexit (R5.5).
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
        // **debug**, `/measure` demands **release**, and mistaking a debug number
        // for a floor becomes impossible only if the line itself states its profile
        // (R5.3).
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
            // the blindness R5.2 wants to close. The measurement tokens are not
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
        line.push_str(" pipeline=ok");
        line
    }
}

/// The two tokens of one column.
///
/// Below the floor there is **no** number (R5.6): `insufficient` is printed and
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
/// `quiet`, `T` would be derived from one side only, i.e. its lower bound would be an unmeasured number (008 phase-6).
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
    /// The frame count exceeded the upper bound: zero-frames-at-idle is broken.
    ExcessFrames {
        limit: u64,
    },
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
    // teardown panic. The three leak arms are ordered among themselves by
    // **recognising power**: `content` recognises it by its count, the settling
    // question by its infrastructure; the tail only by the trace it leaves, i.e. it says the least.
    // Panic goes last, because the others say that what the run **measured** is
    // broken; panic is about the path after the run ended. When both happen the
    // line writes only the first, but the `teardown=` token already carries the second —
    // `motion_and_panic_report_the_more_fundamental_fault` and
    // `a_short_tail_fails_the_gate` pin this order.
    // Reversing it would also break today's order of `ExcessFrames`.
    let panicked = match teardown {
        Some(Teardown::ReaderPanicked) => Some("reader thread"),
        Some(Teardown::Panicked) => Some("teardown thread"),
        _ => None,
    };
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
        // to be drawn — the next phase's motion frames will legitimately inflate `frames`.
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
            } else if c > IDLE_FRAME_LIMIT {
                Verdict::ExcessFrames {
                    limit: IDLE_FRAME_LIMIT,
                }
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

/// The application's ssh masters (047): askpass is this very binary, the
/// sockets live in this instance's own directory under the user's cache
/// directory (or `/tmp/bateri-$UID`; 047 R9.2). What a dead bateri left
/// behind is swept once, off the main thread. A master ends with the user's
/// last session to its host and on quit ([`AppDelegate::shutdown`], R9.3).
/// The saved passwords are the login keychain's ([`crate::keychain`], 047
/// phase-3).
fn masters() -> Option<Arc<Masters>> {
    let askpass = std::env::current_exe().ok()?;
    // SAFETY: `getuid` has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    let bases = ssh_route::socket_bases(child::home().as_deref(), uid);
    let masters = Arc::new(Masters::new(
        askpass,
        bases,
        Arc::new(crate::keychain::Keychain),
    ));
    let sweeper = Arc::clone(&masters);
    let _ = std::thread::Builder::new()
        .name("ssh socket sweep".into())
        .spawn(move || sweeper.sweep());
    Some(masters)
}

impl AppDelegate {
    pub(crate) fn new(mtm: MainThreadMarker, opts: Options) -> Retained<Self> {
        // The ring is allocated **only** when the gate is open: a closed gate must
        // cost an `Option` branch, not an allocation (R4.1). Deriving the capacity
        // from the run duration is `bt-gpu`'s job too — it is the side that knows the refresh rate.
        // taraf o.
        let stats = opts
            .run
            .and_then(|run| run.stats_since.map(|since| Stats::new(since, run.seconds)))
            .map(Arc::new);
        let this = Self::alloc(mtm).set_ivars(Ivars {
            run: opts.run,
            notices: RefCell::new(Notices::default()),
            settings: RefCell::new(Settings::default()),
            config_watch: RefCell::new(None),
            theme_watch: RefCell::new(None),
            stats,
            windows: RefCell::new(Vec::new()),
            next_window_id: Cell::new(0),
            appearance_dark: Cell::new(None),
            settings_window: RefCell::new(None),
            settings_state: RefCell::new(settings::FileState::Missing),
            shell_menu: OnceCell::new(),
            updater: OnceCell::new(),
            masters: opts.run.is_none().then(masters).flatten(),
        });
        // SAFETY: NSObject's init takes no arguments and the ivars have been set.
        unsafe { msg_send![super(this), init] }
    }

    /// Identity of the new window; the counter only goes up.
    fn next_window_id(&self) -> u64 {
        let id = self.ivars().next_window_id.get();
        self.ivars().next_window_id.set(id + 1);
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

    /// The window with identity `id`; `None` if it has left the list — the pane's owner
    /// handle (`window::WindowHost`), the close question and the search paths.
    pub(crate) fn window(&self, id: u64) -> Option<Retained<TerminalWindow>> {
        self.ivars()
            .windows
            .borrow()
            .iter()
            .find(|window| window.id() == id)
            .cloned()
    }

    /// The pane with identity `id`; `None` if it is closed — the path of the jobs
    /// that return from the reader thread to the main queue (`ShellWake`, the
    /// alternate-screen notifier, uploads) ([`pane_by_id`]). It asks the pane's
    /// owner for window-level work (`window::WindowHost`).
    ///
    /// Unlike [`AppDelegate::window`], it does **not** find a pane whose
    /// teardown has started ([`find_open`]): the window leaves the list a turn
    /// later and a stale notification arriving in between must not do work on a
    /// closed session. The search covers all panes of all windows (splits).
    pub(crate) fn pane(&self, id: u64) -> Option<Retained<TerminalPane>> {
        find_open(self.all_panes(), |pane| (pane.id() == id, pane.is_closed()))
    }

    /// All panes of all windows — the list for the walking paths (settings
    /// distribution, Dock icon, lookup by identity); a copy, like the window list.
    fn all_panes(&self) -> Vec<Retained<TerminalPane>> {
        self.windows()
            .iter()
            .flat_map(|window| window.panes())
            .collect()
    }

    /// The upload bar on the app's Dock icon — the total of all panes
    /// (`uploader::refresh_dock_tile`); the pane's
    /// `PaneHost::uploads_changed` event lands here.
    pub(crate) fn refresh_dock_tile(&self) {
        let panes = self.all_panes();
        let panes: Vec<&TerminalPane> = panes.iter().map(|pane| &**pane).collect();
        crate::uploader::refresh_dock_tile(self.mtm(), &panes);
    }

    /// The pane with tab identity `id` and its window; `None` if closed
    /// (`bateri://tab/`, `application:openURLs:`): bringing to the front a pane
    /// whose teardown has started but which has not yet left the list would put
    /// a sessionless window on screen ([`find_open`]). The identity is per pane (039 Karar 10).
    fn pane_by_tab(
        &self,
        id: &TabId,
    ) -> Option<(Retained<TerminalWindow>, Retained<TerminalPane>)> {
        self.windows().into_iter().find_map(|window| {
            let pane = find_open(window.panes(), |pane| {
                (pane.tab_id() == id, pane.is_closed())
            })?;
            Some((window, pane))
        })
    }

    /// The active window: `NSApp.keyWindow` is looked up in the list. `None` if the
    /// settings window or a panel is key, and the new window is born at home. The
    /// source of inheritance is its **focused pane** (`TerminalWindow::focused_pane`).
    fn key_window(&self) -> Option<Retained<TerminalWindow>> {
        let key = NSApplication::sharedApplication(self.mtm()).keyWindow()?;
        self.window_owning(&key)
    }

    /// The active tab's remote host and its resolved mark; `None` in a local tab or
    /// when no terminal window is key — the input of Shell ▸ Mark … as ▸ (037 Karar 5).
    /// girdisi (037 Karar 5).
    pub(crate) fn key_remote_mark(&self) -> Option<(String, HostMark)> {
        self.key_window()?.remote_mark()
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
        let windows = self.windows();
        if windows.is_empty() {
            return NSApplicationTerminateReply::TerminateNow;
        }
        let confirm = self.settings().confirm_close;
        // The question collects the running job from the panes (039 Karar 11).
        let panes = self.all_panes();
        let unit = window::unit_for(panes.len(), windows.len());
        let Some(foregrounds) = window::foregrounds_to_ask(timed, confirm, &panes) else {
            return NSApplicationTerminateReply::TerminateNow;
        };
        let mtm = self.mtm();
        NSApplication::sharedApplication(mtm).activate();
        let alert = window::alert(mtm, &window::prompt(CloseScope::Quit, unit, &foregrounds));
        if alert.runModal() == NSAlertFirstButtonReturn {
            NSApplicationTerminateReply::TerminateNow
        } else {
            NSApplicationTerminateReply::TerminateCancel
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
    }

    /// Opens a new window (or a new tab in `from`'s group) — the **only** path that
    /// spawns windows: the launch's first window (`from = None`), ⌘N, ⌘T, the tab
    /// bar's `+` and the Dock icon.
    ///
    /// `from` is the active window; the new shell starts in its OSC 7 directory (home
    /// if none, 026 → Karar 4), the temporary point-size delta comes from it (Karar 3) and the theme from its
    /// session — all windows share the same theme; without `from` the theme is
    /// resolved from settings. A tab request without `from` is a separate window.
    /// The shell's first input comes from `opening` and `from`'s remote target
    /// ([`initial_line`]); directory inheritance is the same in all three openings — in a remote tab
    /// `working_directory()` returns the local directory (036 Karar 4).
    ///
    /// Order: point size, subtitle and chrome before the window is visible, the list before placement
    /// (so geometry events find the window in the list), the session **after**
    /// placement — a window added to a tab takes the group's size and the shell must see the first
    /// `TIOCSWINSZ` with that size.
    ///
    /// The error returns to the caller; if the session could not be born, the window is closed.
    fn open_window(
        &self,
        from: Option<&TerminalWindow>,
        opening: Opening,
    ) -> Result<Retained<TerminalWindow>, String> {
        let mtm = self.mtm();
        let id = self.next_window_id();
        // Inheritance comes from the active window's **focused pane** (039 Karar 9).
        let source = from.map(TerminalWindow::focused_pane);
        let (launch, theme) = self.pane_launch(id, source.as_deref(), opening);
        let window = TerminalWindow::new(mtm, id, launch).map_err(|e| e.to_string())?;
        window.set_subtitle(&NSString::from_str(
            &self.ivars().notices.borrow().subtitle(),
        ));
        self.ivars().windows.borrow_mut().push(window.clone());
        // Chrome **before** the window is visible: if painted afterwards, every ⌘T
        // would show the system's grey title bar for a frame. The separator's colour
        // comes from the same theme too (the first form of `TerminalWindow::set_theme`).
        window.set_theme(theme);
        match from {
            Some(from) if opening != Opening::Window => window.show_as_tab_of(from),
            _ => window.show_after(from),
        }
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

    /// The new pane's birth package (039 Karar 3) and its theme — the single source
    /// for both the window-spawning path and splitting. All inputs are here, the pane
    /// does not reach into `AppDelegate`. The pane identity comes from the same counter as
    /// windows' (one namespace); its owner is `window`'s [`WindowHost`].
    ///
    /// `from` is the source of inheritance (the focused pane): the OSC 7 directory (home
    /// if none, 026 → Karar 4), the point-size delta (Karar 3), the theme and the remote line ([`initial_line`]);
    /// without `from` the theme is resolved from settings. The integration is asked **once**
    /// and gives both answers at once (environment + dock share).
    fn pane_launch(
        &self,
        window: u64,
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
            id: self.next_window_id(),
            run: self.ivars().run,
            host: Rc::new(WindowHost::new(window)),
            lookup: pane_by_id,
            stats: self.stats(),
            settings: self.settings().clone(),
            theme,
            launch: Launch {
                working_directory: dir,
                initial_input: initial,
            },
            integration: self.shell_integration(),
            reduce_motion: self.reduce_motion(),
            smooth_scroll: self.smooth_scroll(),
            zoom: from.map_or_else(Zoom::default, TerminalPane::zoom),
            masters: self.ivars().masters.clone(),
        };
        (launch, theme)
    }

    /// ⌘D / ⇧⌘D (`TerminalWindow`'s `splitRight:`/`splitDown:`): a new pane next
    /// to `from`, with `from`'s inheritance ([`Opening::Split`]). The error goes to
    /// stderr; the tab stays open — the other panes' shells must not die because a new one
    /// could not be born.
    pub(crate) fn open_split(&self, window: &TerminalWindow, from: &TerminalPane, axis: Axis) {
        let (launch, _) = self.pane_launch(window.id(), Some(from), Opening::Split);
        if let Err(e) = window.add_pane(self.mtm(), launch, from.id(), axis) {
            eprintln!("bateri: {e}");
        }
    }

    /// A new window or tab derived from the active window (⌘N, ⌘T, ⌥⌘T, `+`).
    fn open_from_key_window(&self, opening: Opening) {
        let from = self.key_window();
        self.open_window_or_report(from.as_deref(), opening);
    }

    /// [`AppDelegate::open_window`], with the error to stderr — the path of ⌘N/⌘T/`+`/Dock.
    /// The process does **not** exit: the other windows' shells must not die because a new one
    /// could not be born (only the first window exits, `didFinishLaunching`).
    fn open_window_or_report(&self, from: Option<&TerminalWindow>, opening: Opening) {
        if let Err(e) = self.open_window(from, opening) {
            eprintln!("bateri: {e}");
        }
    }

    /// The quiet stamp of the timed run's single window (`quiet=`).
    ///
    /// A timed run has a single window and a single pane (039 Karar 12) and the report
    /// reads it; it is the first in the list.
    fn quiet_since(&self) -> Option<Duration> {
        let pane = self.windows().first().map(|window| window.focused_pane());
        pane.and_then(|pane| pane.link().and_then(DisplayLink::quiet_since))
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
        let integration = shell_integration_env(
            &self.inputs(),
            setting,
            child::shell,
            child::zsh_wrapper_dir,
            std::env::var_os("ZDOTDIR"),
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

    /// Sweeps the preview cache (045 Karar 9, R6) on a background thread with
    /// the settings as they are now: `Launch` at startup, `Daily` from
    /// [`AppDelegate::schedule_daily_sweep`] and `ClearNow` — the single method
    /// the settings window's Clear Now calls (phase-6). Edited copies it moved to
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
                    let window = NSApplication::sharedApplication(mtm).keyWindow();
                    crate::preview::report_rescued(mtm, window.as_deref(), &report.rescued, || {});
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

    /// The daily sweep (Karar 9: once a day, only what outlived `preview_keep`):
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
                // The point-size delta is **per pane** (026 → Karar 3, 039 Karar 5)
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
            // The load indicator's form and interval (046 Karar 8): `off`
            // hides it at once, another form redraws the last value.
            if changes.stats {
                for pane in &panes {
                    pane.set_stats_settings(&new.remote_stats);
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
            self.ivars().settings.replace(new);
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

    /// The settings window's "Open settings.toml" button (029 Karar 8; until 029
    /// it was bateri ▸ Settings… itself): creates the file from the template if missing
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
    /// write slot (029 Karar 7): both are a single source, the window keeps no copy of its own.
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
        let reduce = self.reduce_motion();
        window.refresh(&settings, reduce, &state, &write, &embedded, &user);
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
    /// system's change (measured, 026 phase-4 Uygulama Notları), it saw only
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
        let subtitle = NSString::from_str(&subtitle);
        for window in self.windows() {
            window.set_subtitle(&subtitle);
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
    /// **Parallel, a single deadline** (026 → Karar 5): first every window's
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
    /// The returned result is the **first** window's first pane's: the only path asking for a report is
    /// the timed run and there is a single window and a single pane there (026 → Karar 9, 039
    /// Karar 12). On an interactive close the result
    /// is dropped — not collected, since nobody reads it.
    fn shutdown(&self) -> Option<Teardown> {
        // The watchdog's budget starts at **shutdown**, not at process start:
        // startup (GPU device, pipeline setup, first window) can take seconds
        // on a cold machine, and if that were deducted from the budget a
        // healthy run would go red with `_exit(70)`.
        if self.ivars().run.is_some() {
            crate::watchdog();
        }
        let windows = self.windows();
        // The panes' closes below end their remote sessions; the masters'
        // `exit` is `close_all`'s, under the shared deadline (047 R9.3).
        if let Some(masters) = &self.ivars().masters {
            masters.begin_quit();
        }
        // One teardown per pane, across all windows' panes (039): all of them
        // start, then they are awaited in parallel up to a single deadline.
        let closing: Vec<_> = windows
            .iter()
            .flat_map(|window| window.begin_close())
            .collect();
        let deadline = Instant::now() + SHUTDOWN_GRACE;
        // Our ssh masters end in parallel, under the same deadline (047 R9.3);
        // a timed run has none.
        let masters = self.ivars().masters.clone().and_then(|masters| {
            std::thread::Builder::new()
                .name("ssh masters close".into())
                .spawn(move || masters.close_all(deadline))
                .ok()
        });
        // The result feeds the report (`teardown=`): if the session was never
        // born it is `None`, and that is an answer too — nothing to close.
        let mut first = None;
        for (index, closing) in closing.into_iter().enumerate() {
            let teardown = closing.map(|closing| closing.wait_until(deadline));
            if index == 0 {
                first = teardown;
            }
        }
        if let Some(masters) = masters {
            let _ = masters.join();
        }
        first
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
    /// The counters are read from the smoke run's **only** window (the first
    /// in the list). With no window (if startup never built one the process
    /// had already exited) the counters are zero and the gate says `MissingCounter`.
    fn report_and_exit(&self, run: Run, teardown: Option<Teardown>, quiet: Option<Duration>) -> ! {
        let windows = self.windows();
        // The smoke run's only window's only pane (039 Karar 12).
        let pane = windows.first().map(|window| window.focused_pane());
        // The frames still in flight are counted **before** `frames=` is read
        // (040 Karar 6): completion is polled by the ticks, and the link is
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
        // no letters", i.e. silently falling back to 002's blind-writing era:
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
        // The fifth token `slots=U/T` is a **counter**, not a gate: it says how many of the atlas's
        // slots are filled and `/measure` will read the occupancy ratio from it. It stays out of the
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
        };
        // Tokens appear **only** on the success line and only on stdout: that is
        // the machine contract. Error lines carry the same numbers but not in
        // token form, or a CI step looking for `frames=` would read a frame
        // count from a failed run.
        let secs = run.seconds;
        match verdict(counters, run.workload, teardown, motion, quiet) {
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
    use super::*;

    /// The **measured** tail of a healthy smoke run (2026-09-16, the lowest of
    /// thirty-seven runs: `1742,29 ms`; owner `docs/OLCUMLER.md`).
    /// Tests that do not ask about the gate get this so the `quiet` arm does not
    /// shadow what they do ask; the arm's own tests are below and name the
    /// floor explicitly.
    const HEALTHY_QUIET: Option<Duration> = Some(Duration::from_millis(1742));

    /// Grid metrics; the gutter is an **argument**, because `split_into_grid` is asked two
    /// separate things: the cell split (gutter zero) and the gutter's deduction from columns.
    fn metrics(w: u16, h: u16, gutter: u16) -> CellMetrics {
        CellMetrics::new(w, h, w, gutter, 1).expect("non-zero cell")
    }

    /// A window without a dock: the state of an unintegrated session (and of the smoke recipe).
    /// Tests that query column and row arithmetic get this so the dock
    /// gutter does not mix into the numbers they expect; the gutter's own test
    /// is below and names `DOCK_ROWS` explicitly.
    const NO_DOCK: u16 = 0;

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
        // "never deleted" side as well. `slide=` arrived with 011 and sat next to `motion=` —
        // the token is **never deleted, only added**.
        for token in [
            "content=1",
            "motion=3",
            "slide=2",
            "quiet=2950.00ms",
            // Arrived with 023 and sits **right next to** `slots=`: the two are
            // the atlas's two planes and are read side by side. It was put in the list the same
            // day, because the "never deleted" promise is a promise only if
            // a guard exists — the list above protects only the **old** tokens.
            "slots2=0/2048",
        ] {
            assert!(line.contains(token), "{token} yok: {line}");
        }
        // Its position is part of the contract too: `slots=` and `slots2=` side by side. Were they apart,
        // someone reading the line by eye could not connect the two planes.
        assert!(
            line.contains("slots=13/2048 slots2=0/2048 "),
            "the two planes' tokens must stand side by side: {line}"
        );

        // With the gate closed the measurement tokens are **absent**, and `samples=0` is absent too: zero
        // would be confused with "the gate was open but no samples were collected", and
        // the blindness R5.2 wants to close is exactly that.
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
            assert!(line.contains(token), "{token} yok: {line}");
        }
        // A column below the floor **prints no number** (R5.6) and the reason can be read in the same
        // line's `samples=`/`floor=` pair. p95 and worst are silent
        // together: with few samples they are the same element anyway.
        assert!(line.contains("cpu_encode_p95=insufficient"), "{line}");
        assert!(line.contains("cpu_encode_max=insufficient"), "{line}");
        // Without timestamp support the GPU keys stay and say why there is
        // no number (Karar 6: a token is never deleted).
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
        let narrow = split_into_grid(900.0, 600.0, metrics(9, 18, 0), NO_DOCK);
        let wide = split_into_grid(900.0, 600.0, metrics(18, 36, 0), NO_DOCK);
        assert_eq!((narrow.cols, narrow.rows), (100, 33));
        assert_eq!((wide.cols, wide.rows), (50, 16));
    }

    #[test]
    fn the_gutter_costs_columns() {
        // The left gutter is deducted from columns (010 Karar 3): so the stripe does not
        // sit on top of the text. 900 pixels, 9-pixel cells → 100 columns with no gutter; an 8-pixel
        // gutter takes one column, and so does 9 pixels (a full cell).
        let plain = split_into_grid(900.0, 600.0, metrics(9, 18, 0), NO_DOCK);
        let gutter = split_into_grid(900.0, 600.0, metrics(9, 18, 8), NO_DOCK);
        assert_eq!(plain.cols, 100);
        assert_eq!(gutter.cols, 99, "the gutter takes one column");
        // Rows **do not see** the gutter: the gutter is only on the left and does not
        // touch the vertical geometry.
        assert_eq!(gutter.rows, plain.rows);
        // The gutter travels with the metrics: the value that built the grid gives it back
        // and the draw origin and mouse mapping read the same value.
        assert_eq!(gutter.cell.gutter_px(), 8);
    }

    #[test]
    fn the_dock_costs_rows_and_only_when_there_is_one() {
        // The dock gutter is deducted from **rows** and, unlike the left gutter, conditional:
        // not a single row should go from a window without a dock (an unintegrated shell, the smoke recipe) —
        // `smoke_shell`'s `cells=8 glyphs=6`
        // contract is measured in that window.
        let without = split_into_grid(900.0, 600.0, metrics(9, 18, 8), NO_DOCK);
        let with = split_into_grid(900.0, 600.0, metrics(9, 18, 8), DOCK_ROWS);
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
        // Columns **do not see** the dock: the dock uses the same columns as the grid
        // and its gutter is vertical only.
        assert_eq!(with.cols, without.cols);
    }

    #[test]
    fn the_dock_breathing_room_scales_with_the_gutter() {
        // The breathing gutter is **derived**, not chosen: its source is the left
        // gutter itself. With a fixed pixel count the gutter would stay the same while the font
        // grows with Cmd +/− and the ratio would break; this test holds exactly that link.
        let tight = split_into_grid(900.0, 600.0, metrics(9, 18, 0), DOCK_ROWS);
        let loose = split_into_grid(900.0, 600.0, metrics(9, 18, 8), DOCK_ROWS);
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
        let g = split_into_grid(900.0, 20.0, metrics(9, 18, 8), DOCK_ROWS);
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
        let g = split_into_grid(4.0, 600.0, metrics(9, 18, 8), NO_DOCK);
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
            )
        };
        let load = |n, k, g, r| {
            verdict(
                counters(n, k, g, r),
                Workload::Load,
                clean,
                settled,
                HEALTHY_QUIET,
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
        // the deadline. The pure form of phase-3's acceptance scenario "temporary mutation:
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
                HEALTHY_QUIET
            ),
            Verdict::MotionUnsettled
        );
        assert_eq!(
            verdict(
                good,
                Workload::Smoke,
                clean,
                MotionState::Settled,
                HEALTHY_QUIET
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
        // not exceed the limit and without this arm it was **green**
        // (`docs/OLCUMLER.md` → `## Boşta kare`).
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
        let smoke = |quiet| verdict(good, Workload::Smoke, clean, settled, quiet);
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
            ),
            short
        );
    }

    #[test]
    fn motion_and_panic_report_the_more_fundamental_fault() {
        // The order of the arms is a **diagnostic** preference: when two faults coincide the run is red
        // either way, but which does the line write? The `/code-review`
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
            ),
            Verdict::ShutdownPanicked { .. }
        ));
    }

    #[test]
    fn shutdown_panic_cannot_pass_the_gate() {
        // `/code-review` finding: the `teardown=` token became visible but the gate did not
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
                        HEALTHY_QUIET
                    ),
                    Verdict::ShutdownPanicked { .. }
                ),
                "{teardown:?} cannot pass green"
            );
            assert!(
                matches!(
                    verdict(good, Workload::Load, Some(teardown), settled, HEALTHY_QUIET),
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
                verdict(good, Workload::Smoke, teardown, settled, HEALTHY_QUIET),
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
        // If any of the three inputs turns motion off, the line step applies (027 Karar 5).
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
    fn hermetic_run_does_not_read_reduce_motion() {
        // `Inputs`'s fifth condition (008 phase-5): a timed run does **not** read the
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
        // **012 phase-10's acceptance criterion.** At the `"blocks"` tier the wrapper
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
        // `Inputs`'s sixth condition (009 phase-3): a timed run **never** installs
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
    fn only_a_new_tab_follows_a_remote_tab() {
        // The three arms of 037 Karar 6: on a remote tab ⌘T (and `+`) carries the same
        // command; ⌥⌘T and ⌘N are local even from a remote tab; on a local tab ⌘T is local.
        let remote = || Some("ssh -p 2222 prod".to_owned());
        assert_eq!(
            initial_line(Opening::Tab, remote()).as_deref(),
            Some("ssh -p 2222 prod")
        );
        assert_eq!(
            initial_line(Opening::Split, remote()).as_deref(),
            Some("ssh -p 2222 prod"),
            "a split goes to the same host like ⌘T (039 Karar 9)"
        );
        assert_eq!(initial_line(Opening::LocalTab, remote()), None);
        assert_eq!(initial_line(Opening::Window, remote()), None);
        for opening in [
            Opening::Tab,
            Opening::LocalTab,
            Opening::Window,
            Opening::Split,
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
        // (`/code-review`, 009 gate).
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
        let g = split_into_grid(0.0, 0.0, metrics(9, 18, 8), NO_DOCK);
        assert_eq!((g.cols, g.rows), (0, 0));
    }
}
