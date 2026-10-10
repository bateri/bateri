//! What a terminal pane is born with and what its close leaves: the start directory and first
//! input, its persistent identity, a replayed history or a program handed over by a previous
//! bateri ([`Launch`], [`Adopted`]), the line a pane says when it does not come back whole
//! ([`Note`]), and the handle its close returns ([`Closing`]).
//!
//! The pane's own vocabulary, apart from the windows and tabs that hold panes: whoever opens a
//! pane — bateri's tabs, or an application that embeds one — speaks it.

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Instant;

use bt_core::{
    CursorMotion, InitialInput, PaneUuid, ReduceMotion, Scrollbar, Settings, ShellIntegration,
    ShutdownHandle, SmoothScroll, Teardown,
};
use bt_gpu::{DOCK_ROWS, ScrollbarMode};
use objc2::MainThreadMarker;
use objc2_app_kit::{NSScroller, NSScrollerStyle, NSWorkspace};

use crate::child;

/// Who the application that opens a pane is, where bateri would say "bateri" —
/// given by whoever opens the pane, never guessed from the running program
/// (an application that embeds a pane is not bateri's executable, and a
/// guess would fail quietly).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    /// The application's name where a pane names whose it is: a downloaded
    /// file's quarantine record, which Gatekeeper shows.
    pub app_name: String,
    /// The program that answers the shell integration's helper calls
    /// (`ssh-argv`, `ssh-fell-back`, which the wrapper's `ssh` function asks
    /// through `BATERI_BIN`) — bateri's own executable. `None`: the wrapper's
    /// `ssh` runs plain `ssh`.
    pub helper: Option<PathBuf>,
    /// The shell integration's zsh scripts (bateri's `Resources/shell/zsh`).
    /// `None`: no integration — the pane is a plain terminal, without the
    /// dock and the command blocks.
    pub zsh_wrapper_dir: Option<PathBuf>,
}

/// The wrapper's environment `wrapper` ([`integration_env`]) completed: the
/// helper beside it ([`with_helper`]) and the dock share it makes
/// ([`dock_rows_at_birth`]).
pub(crate) fn with_dock(
    identity: &Identity,
    setting: ShellIntegration,
    wrapper: Vec<(String, String)>,
    instance: Option<&str>,
) -> (Vec<(String, String)>, u16) {
    let integration = with_helper(wrapper, identity.helper.clone(), instance);
    let birth = dock_rows_at_birth(&integration, setting);
    (integration, birth)
}

/// The environment shell integration adds to the child — empty if not set up.
///
/// **The whole decision is here and pure**: which shell, which setting, where
/// the script is. Whoever opens a pane asks it the same way; bateri's timed
/// run never does (its gate is the application's).
///
/// `shell` and `script_dir` are **closures**: in the session of a user who
/// says `"off"` neither is ever consulted.
///
/// `zdotdir` is **eager**: it is our own process's environment, not an entry
/// open to the user's world, and in the hermetic arm its value never reaches the child anyway.
///
/// The return is a `Vec`, not an `Option`: the environment set up can be not
/// one pair but **two** (if the user has an original `ZDOTDIR` the second goes
/// too) and the caller chains it next to `locale_env()`.
pub(crate) fn integration_env(
    setting: ShellIntegration,
    shell: impl FnOnce() -> Option<PathBuf>,
    script_dir: impl FnOnce() -> Option<PathBuf>,
    zdotdir: Option<OsString>,
) -> Vec<(String, String)> {
    if !setting.installs_wrapper() {
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
/// helper program ([`Identity::helper`], bateri's own executable in bateri), which the wrapper's `ssh` function asks for the wrapping
/// decision (`bateri ssh-argv`). Only where the wrapper is installed — an
/// empty `env` stays empty, so neither the timed run nor a non-zsh shell nor
/// `[shell] integration = "off"` gets it — and only a UTF-8 path
/// (`SessionOptions.env` wants a `String`; without it the function falls back
/// to plain `ssh`). With it, `BATERI_SSH_INSTANCE`: the masters' instance
/// directory name, where a wrapped session becomes a master
/// (`ssh_route::session_socket`) — none without masters (the timed run).
pub(crate) fn with_helper(
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
pub(crate) fn dock_rows_at_birth(
    integration: &[(String, String)],
    setting: ShellIntegration,
) -> u16 {
    if integration.is_empty() || !setting.wants_dock() {
        0
    } else {
        DOCK_ROWS
    }
}

/// Three-valued `[motion] reduce_motion` + the system's answer → a single
/// `bool`.
///
/// **The combination is here because this is the layer that sees the system:**
/// `bt-gpu` does not see AppKit (the layer rule) and `bt-core`'s settings
/// model is already the counterpart of a file, not of an accessibility
/// setting. A **resolved** `bool` descends below (the `Renderer::set_font`
/// precedent). `system` is a **closure**, not a `bool`: in the session of a
/// user who says `"on"`/`"off"` the system is never consulted.
pub(crate) fn reduce_motion(setting: ReduceMotion, system: impl FnOnce() -> bool) -> bool {
    match setting {
        ReduceMotion::On => true,
        ReduceMotion::Off => false,
        ReduceMotion::System => system(),
    }
}

/// The system's Reduce Motion (System Settings ▸ Accessibility ▸ Display).
pub(crate) fn system_reduce_motion() -> bool {
    NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
}

/// `[terminal] scrollbar` + the system's scroll bar preference → the bar's
/// one resolved form ([`ScrollbarMode`]). `overlay` is a **closure** — "Show
/// scroll bars" in System Settings ([`overlay_scrollers`]): macOS already
/// resolves "Automatically based on mouse or trackpad" for the devices
/// attached, so no device detection is written here. Overlay scrollers are
/// the self-hiding form (`Auto`), legacy ones the permanent one (`Always`).
pub(crate) fn scrollbar(setting: Scrollbar, overlay: impl FnOnce() -> bool) -> ScrollbarMode {
    match setting {
        Scrollbar::Auto => ScrollbarMode::Auto,
        Scrollbar::Always => ScrollbarMode::Always,
        Scrollbar::Never => ScrollbarMode::Never,
        Scrollbar::System if overlay() => ScrollbarMode::Auto,
        Scrollbar::System => ScrollbarMode::Always,
    }
}

/// Whether the system shows overlay scrollers (`NSScroller.preferredScrollerStyle`).
pub(crate) fn overlay_scrollers(mtm: MainThreadMarker) -> bool {
    NSScroller::preferredScrollerStyle(mtm) == NSScrollerStyle::Overlay
}

/// `[motion] smooth_scroll` + Reduce Motion + `cursor_motion` → a single
/// `bool`: does the wheel go smooth.
///
/// If any of the three turns motion off, line stepping — scrolling does not
/// *add* animation for one who turned motion off (the same as
/// `cursor_motion = "snap"`'s relation to Reduce Motion). Quantization is
/// **at the source**, not in `bt-gpu`'s `Motion`: the `false` arm stays as
/// today's line path. `reduce` is [`reduce_motion`]'s resolved answer.
pub(crate) fn smooth_scroll(settings: &Settings, reduce: bool) -> bool {
    settings.smooth_scroll == SmoothScroll::On
        && !reduce
        && settings.cursor_motion != CursorMotion::Snap
}

/// A pane's closing that has begun (`TerminalPane::begin_close`), and so a
/// tab's and a window's, made of their panes'.
pub(crate) enum Closing {
    /// This call started it; the handle knows the result.
    Started(ShutdownHandle),
    /// The closing had begun before (like ⌘Q while the window is closing):
    /// nothing to wait for, the first call knew the real result.
    AlreadyDone,
}

impl Closing {
    /// Waits until `deadline` at the latest ([`ShutdownHandle::wait_until`]).
    pub(crate) fn wait_until(self, deadline: Instant) -> Teardown {
        match self {
            Self::Started(handle) => handle.wait_until(deadline),
            Self::AlreadyDone => Teardown::AlreadyDone,
        }
    }
}

/// The new shell's birth information — the two decisions the birth package
/// (`PaneLaunch::launch`) takes from the caller (`AppDelegate::open_window`).
pub(crate) struct Launch {
    /// Start directory (the active tab's directory, else home).
    pub(crate) working_directory: Option<PathBuf>,
    /// The shell's first input and whether it runs: ⌘T in a
    /// remote tab runs it, a restored remote pane leaves it ready;
    /// `None` → an ordinary local shell.
    pub(crate) initial_input: Option<InitialInput>,
    /// The pane's persistent identity; `None` → a new one. A restored
    /// pane keeps its saved one, so `bateri://tab/<id>` and
    /// `TERM_SESSION_ID` survive the quit.
    pub(crate) uuid: Option<PaneUuid>,
    /// A previous session's scrollback, replayed before the shell starts
    /// (`SessionOptions::replay`); `None` → an empty grid.
    pub(crate) replay: Option<Vec<u8>>,
    /// The update's handover: a running program to carry on
    /// instead of a new shell; `None` → a shell is born.
    pub(crate) adopt: Option<Adopted>,
}

/// A pane the previous bateri froze and its holder gave, checked to be
/// adoptable (`AppDelegate`'s arrival): the master, the exit watch of its
/// child, the pane's state and the bytes to read before the master's.
#[derive(Debug)]
pub(crate) struct Adopted {
    pub(crate) master: std::os::fd::OwnedFd,
    pub(crate) exit: std::os::fd::OwnedFd,
    pub(crate) pid: u32,
    pub(crate) state: crate::handover::PaneState,
    /// The frozen tail, then what the holder drained (`HeldPane::buffer`).
    pub(crate) prefix: Vec<u8>,
    /// The socket of the holder it came from: the pane registers with this
    /// bateri's own holder unconfirmed until that one is acknowledged.
    pub(crate) taken_from: Option<std::path::PathBuf>,
    /// Which kind of holder it came from (`bt_core::AdoptMode`).
    pub(crate) mode: bt_core::AdoptMode,
    /// Its screen did not come back whole: the program is nudged to redraw
    /// it (`Session::nudge_size`).
    pub(crate) nudge: bool,
    /// The note if the session cannot be adopted after all and the pane
    /// falls back to a new shell.
    pub(crate) note: Note,
}

/// The dim line a pane says when it does not come back whole — no pane
/// comes back as a half screen, or as a new shell, without saying so.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Note {
    /// The program did not cross an update.
    Update,
    /// The program was not carried through a crash (its holder did not have
    /// it, or could not give it).
    Crash,
    /// A deliberate handover to a bound holder (a quit, or an update that
    /// found one) did not carry the program — which of the two it was, the
    /// holder cannot tell.
    NotCarried,
    /// The program ended while bateri was closed.
    Ended,
    /// The program runs on, its screen did not come back: it redraws it.
    Screenless,
    /// The program runs on, but the output of while bateri was closed lost
    /// its oldest part.
    Cut,
}

impl Note {
    pub(crate) fn text(self) -> &'static str {
        match self {
            Note::Update => {
                "bateri: the program running here did not survive the update; this is a new shell"
            }
            Note::Crash => {
                "bateri: the program running here did not survive the crash; this is a new shell"
            }
            Note::NotCarried => {
                "bateri: the program running here could not be carried over; this is a new shell"
            }
            Note::Ended => {
                "bateri: the program running here ended while bateri was closed; this is a new shell"
            }
            Note::Screenless => {
                "bateri: the screen could not be restored; the program kept running"
            }
            Note::Cut => "bateri: output from while bateri was closed was cut short",
        }
    }

    /// The note as a line of its own: dim, and the pen reset after it.
    pub(crate) fn line(self) -> Vec<u8> {
        let mut line = b"\x1b[0m\x1b[2m".to_vec();
        line.extend_from_slice(self.text().as_bytes());
        line.extend_from_slice(b"\x1b[0m\r\n");
        line
    }
}

/// `history` (if any) with `note` under it, on a line of its own: the
/// replay of a pane that fell back to a new shell.
pub(crate) fn fallen_back(history: Option<Vec<u8>>, note: Note) -> Vec<u8> {
    let mut replay = history.unwrap_or_default();
    if !replay.is_empty() && !replay.ends_with(b"\n") {
        replay.extend_from_slice(b"\r\n");
    }
    replay.extend_from_slice(&note.line());
    replay
}

#[cfg(test)]
mod tests {
    use super::{Note, fallen_back};

    /// The fallback's note sits on a line of its own under the history,
    /// dim, and resets what the history left on.
    #[test]
    fn the_fallback_note_has_a_line_of_its_own() {
        for kind in [Note::Update, Note::Crash, Note::NotCarried, Note::Ended] {
            let note = format!("\x1b[0m\x1b[2m{}\x1b[0m\r\n", kind.text());
            assert_eq!(kind.line(), note.as_bytes());
            assert_eq!(fallen_back(None, kind), note.as_bytes());
            assert_eq!(fallen_back(Some(Vec::new()), kind), note.as_bytes());
            assert_eq!(
                fallen_back(Some(b"$ ls\r\n".to_vec()), kind),
                [&b"$ ls\r\n"[..], note.as_bytes()].concat()
            );
            assert_eq!(
                fallen_back(Some(b"\x1b[1m$ half".to_vec()), kind),
                [&b"\x1b[1m$ half\r\n"[..], note.as_bytes()].concat()
            );
        }
        // Every event says its own thing, and the update's is today's text.
        assert_eq!(
            Note::Update.text(),
            "bateri: the program running here did not survive the update; this is a new shell"
        );
        let texts = [
            Note::Update,
            Note::Crash,
            Note::NotCarried,
            Note::Ended,
            Note::Screenless,
            Note::Cut,
        ]
        .map(Note::text);
        for (index, text) in texts.iter().enumerate() {
            assert!(!texts[..index].contains(text), "{text}");
        }
    }
}
