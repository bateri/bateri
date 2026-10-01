//! bt-shell-macos — the AppKit shell: windows, tabs, splits, menus, keyboard,
//! services.
//!
//! AppKit directly through `objc2-app-kit`; it never sees Metal, leaves drawing
//! to `bt-gpu` and lets `Renderer::system_default` set up the device — one
//! renderer per pane (`pane`). It does not drive the frame either: it wires the
//! pane, the session and the display link together and the rest is `bt-gpu`'s
//! rhythm; the vsync ticks come from this crate's `Pacer` (`pacer`:
//! `NSView.displayLink` used as a timer, `CACurrentMediaTime` as the time base).
//! App-wide state (`app`), per-window state (`window`: chrome, tabs, the close
//! question, tab actions) and per-session state (`pane`: an `NSView` subclass;
//! the session's core, the search bar, the upload queue and the pane-level menu
//! selectors) live in separate objects. The boundary between a pane and its
//! owner has three parts (039 Karar 1–3): inputs arrive in one package at birth
//! (`pane::PaneLaunch`), events go through a trait (`pane::PaneHost`; today's
//! owner is `window::WindowHost`), and every menu job is a named method on the
//! pane — the selector is a wrapper calling it. The pane module does not reach
//! into `AppDelegate`; main-queue callbacks find the pane by id through the
//! path its owner hands over (`pane::PaneLookup`).
//!
//! The keyboard flows to the PTY from here (`view`, `clipboard`) and the mouse
//! to the session (selection and scrolling, `view`); a file dropped from Finder
//! lands on the input line through `view`'s drag destination. The main menu
//! (`menu`), the settings window (`settings_window`, writing through the
//! settings edit path), the floating search bar (`search_bar`), the split
//! container (`split_view`), the remote upload's sheet, queue driver and
//! notifications (`uploader`), the updater (`updater`, Sparkle loaded at run
//! time) and the shell's locale from `NSLocale` (`locale`) are here too. The
//! system's light/dark appearance is read here (`app`, KVO on
//! `NSApp.effectiveAppearance`) and the window chrome is painted to the theme
//! (`window`). This crate also owns the teardown order and the smoke watchdog.
//!
//! The platform-independent half — settings reading, the split tree, zoom,
//! notices, the gesture ledger, shell quoting, key encoding, the upload rules,
//! the process table, the child's command and environment, and the file watch
//! — lives in `bt-shell-common` (043) and is imported at the crate root below,
//! so `crate::settings` and friends keep resolving.

pub(crate) mod app;
mod clipboard;
mod hyperlink;
mod locale;
mod menu;
mod pacer;
mod pane;
mod search_bar;
mod settings_window;
mod split_view;
mod updater;
mod uploader;
mod view;
mod window;

// The platform-independent half lives in `bt-shell-common` (043); imported
// at the crate root so `crate::settings` and friends keep resolving.
use bt_shell_common::{
    child, gesture, jobs, keys, links, notices, quote, settings, split, upload, watch, zoom,
};

use std::time::{Duration, Instant};

use objc2::MainThreadMarker;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

use bt_core::SHUTDOWN_GRACE;

pub use bt_gpu::GpuError;

/// The shell of the smoke and measurement runs. **Not** the user's `$SHELL`:
/// the result must not depend on the rc files.
///
/// It is a separate type because `run_seconds.is_some()` carried three
/// distinct meanings: pick the fixed shell, set the deadline, set the
/// watchdog. The workload choice concerns only the first; if not split, a
/// measurement run would lose either the deadline or the watchdog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Workload {
    /// `make smoke`: one shot, then idle. The source of the `cells`/`glyphs`/
    /// `rules` counts and the guard of zero frames at idle — content frames
    /// are **upper-bounded** here (`app::IDLE_FRAME_LIMIT`), while the quiet
    /// at the end of the run is **lower-bounded** (`app::QUIET_FLOOR`).
    Smoke,
    /// `BT_SCROLL_TEST`: output streaming for the whole run. The frame flow
    /// is the point of the work, there is no upper bound.
    Load,
}

impl Workload {
    /// The value of the `load=` token.
    ///
    /// The string lives **next to** the type, not at the call site: the token
    /// is a machine contract and the contract's text lives beside the
    /// definition. At the call site, when a third workload is added the reader
    /// rather than the compiler would have to remember the mapping.
    pub(crate) fn token(self) -> &'static str {
        match self {
            Self::Load => "load",
            Self::Smoke => "smoke",
        }
    }
}

/// Timed run: `make smoke` and measurement. `None` → the user's own session.
///
/// The three fields are born **together** and sit under a single `Option`,
/// because all three depend on the duration: an unbounded load never ends, an
/// unbounded measurement is never reported (the report is in
/// `report_and_exit` and only the deadline reaches it). With separate
/// `Option`s the type would allow these impossible states and the cost would
/// surface as `unwrap_or(0)` and "unreachable branch" comments.
#[derive(Clone, Copy, Debug)]
pub struct Run {
    /// `BT_RUN_SECONDS`: when it expires, the frame count is checked and the
    /// process exits (`make smoke`).
    pub seconds: u64,
    /// Which fixed shell.
    pub workload: Workload,
    /// `BT_FRAME_STATS`: if `Some`, measurement is on **and** the stamp was
    /// taken on the first line of `main()` (**not** at process start; both
    /// ends of the stamp are in [`bt_gpu::Stats::startup`]). With a `bool`,
    /// the stamp would have to be taken wherever the flag is read — in
    /// [`run`] or later, at the latest right before the first window's
    /// `Renderer::system_default()` — and the ordering's correctness would
    /// rest on a comment. The first renderer is now born with the first
    /// window, after the main loop has started; the stamp is still **before**
    /// it, so the most expensive part of startup (creating the GPU device and
    /// the pipelines) is inside the measurement.
    pub stats_since: Option<Instant>,
}

pub struct Options {
    pub run: Option<Run>,
}

/// Sets up the application and enters the main loop with
/// `NSApplication::run`. **It does not return:** the interactive session exits
/// only with Quit (Cmd-Q, `terminate:`) — the app stays open both when the
/// last window closes and when the shell exits (that window closes). The
/// `BT_RUN_SECONDS` path exits with `process::exit`; there, the shell's exit
/// (`child_exit` → `terminate:`) and the last window's closing also end the
/// process. `Ok(())` is seen only if AppKit's `run` ever returns.
///
/// **`Err` is not returned today.** The first window now sets up the renderer
/// (one renderer per window, `.tasks/026-sekmeler/discussion.md` → Karar 2a)
/// and by then we are inside the main loop: a setup error is printed in
/// `applicationDidFinishLaunching:` with the same line (`bateri: {error}`)
/// and the same exit code (1). The signature stays so as not to change the
/// call site of the bin crate.
///
/// Teardown work (PTY, settings write) goes not after this but into
/// `app::AppDelegate::shutdown`, **which both exit paths pass through** —
/// not into `applicationWillTerminate:`: the smoke deadline deliberately
/// does not go through it, and a step put there would be silently skipped on
/// that path.
pub fn run(opts: Options) -> Result<(), GpuError> {
    // audit: entry point; being called from outside the main thread is a programming error.
    let mtm = MainThreadMarker::new().expect("bt_shell_macos::run runs on the main thread");
    // The startup stamp must be taken **before** the first renderer (first
    // window), and the type enforces it: `Options` carries an `Instant`, not
    // a flag.
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    // `delegate` outlives `app.run()` in this scope: AppKit's and the
    // window's delegate properties are weak, this Retained is the owner.
    let delegate = app::AppDelegate::new(mtm, opts);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.run();
    Ok(())
}

/// The watchdog's budget: `bt-core`'s teardown bound plus a fixed margin.
///
/// **It no longer derives from the run duration, and the reason is the
/// narrowing of scope.** The old measure was `run_seconds × 3`; that was
/// right when the watchdog was the only bound on *all* of teardown. Once
/// teardown was bounded by [`bt_core::SHUTDOWN_GRACE`], the watchdog's scope
/// narrowed to "**other** hangs on the teardown path" and none of those scale
/// with the run duration: the old measure gave a three-second smoke 9
/// seconds and a sixty-second measurement run **3 minutes**.
///
/// The number was **measured** (2026-09-12, debug, this machine, six runs):
/// **five** of the teardowns finished cleanly and stayed well under
/// `SHUTDOWN_GRACE` (the total exceeds the run duration by ~0.18 s and
/// startup is included in that margin); **one** hit the bound
/// (`teardown=abandoned`) and took exactly **+0.49 s**. So the measured
/// ceiling is `SHUTDOWN_GRACE` itself. The two-second margin is **five
/// times** that.
///
/// Ten measurement runs were made with the new budget and **none** fell to
/// the watchdog (no `exit 70`) — against the 6 seconds the old budget gave in
/// those runs.
///
/// What is lost if it is short: a healthy but slow teardown is cut with
/// `_exit(70)` and `make smoke` points at the wrong fault. What is lost if it
/// is long: a run that truly hangs waits that long — and this happens not in
/// front of a person but inside a gate.
const WATCHDOG_BUDGET: Duration = SHUTDOWN_GRACE.saturating_add(Duration::from_secs(2));

/// The last resort that cuts a hung teardown — **only on the
/// `BT_RUN_SECONDS` path** and set up when teardown begins
/// (`AppDelegate::shutdown`).
///
/// Teardown runs on the main thread and enters `Session::shutdown()` from
/// there. That call now **waits with a bound** (`bt-core`'s
/// `SHUTDOWN_GRACE`), so the hang the watchdog was born for — a child that
/// does not die or is stuck inside its exit — can no longer hold the main
/// thread, and the watchdog's scope has **narrowed**: it stays so that
/// *another* hang on the teardown path (a `Drop` that synchronously posts
/// work to the main queue, a change that breaks the lock order) does not make
/// the smoke run wait forever. That is why it is still a separate thread: the
/// thing it must cut is the main thread itself.
///
/// The bound's only exception is the teardown thread failing to be created
/// (OS thread limit). On that branch **there may be nothing to cut it
/// either** and this is not promised: on a machine where a thread cannot be
/// created, this watchdog's own thread cannot be created either, so
/// `Teardown::Unbounded` and having no watchdog meet in the same condition.
/// Both leave a line on stderr; no path stays silent.
///
/// Its budget now comes not from the run duration but from
/// [`WATCHDOG_BUDGET`], so it takes no argument: none of the things it cuts
/// scale with the run duration.
///
/// In interactive use there is **no** watchdog; the guarantee there is
/// `bt-core`'s bound, and the doc of `Session::shutdown` explains what it
/// closes and why the child being left behind persists.
pub(crate) fn watchdog() {
    // **Not** `thread::spawn`: it panics when a thread cannot be created, and
    // this function's call site is an ObjC callback
    // (`applicationWillTerminate:` / `runDeadline:`). A panic cannot cross the
    // `extern "C"` boundary, so the process **aborts**: neither the token line
    // is printed nor `_exit(70)` runs. Moreover this is exactly the scenario
    // (OS thread limit) in which `bt-core` chose to survive with
    // `Teardown::Unbounded` — and in that condition this thread cannot be
    // created either, so the watchdog **cannot cut**. The error is not
    // swallowed, it is reported: a teardown left without a watchdog must not
    // be silent.
    let spawned = std::thread::Builder::new()
        .name("watchdog".to_owned())
        .spawn(|| {
            std::thread::sleep(WATCHDOG_BUDGET);
            // NOT `eprintln!`: Rust's stderr is locked and the main thread may
            // be hung while holding that lock (`shutdown`'s own `eprintln!`,
            // `Retry::draw_failed`, a future logger). The watchdog exists
            // precisely to cut that; entering the same lock and waiting would
            // cancel itself. A fixed text, not even `format!` — `malloc` is a
            // lock too.
            //
            // Not `process::exit` either: it runs the atexit chain and the
            // stdio flush. 70 = EX_SOFTWARE; `make` shows this as "Error 70".
            //
            // SAFETY: `write` and `_exit` are async-signal-safe; neither takes
            // a lock and they end the process without running anything.
            const MESSAGE: &str = "bateri: teardown overran the watchdog budget, cutting\n";
            unsafe {
                libc::write(2, MESSAGE.as_ptr().cast(), MESSAGE.len());
                libc::_exit(70)
            };
        });
    if let Err(err) = spawned {
        eprintln!("bateri: watchdog thread not created ({err}), nothing cuts a hung teardown");
    }
}
