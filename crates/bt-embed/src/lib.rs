//! bateri's terminal pane for an application not written in Rust: a C interface over
//! `bt_shell_macos::embed`, linked as a static library. Its header is `include/bt_embed.h`,
//! written by hand beside this crate; the two change in the same commit.
//!
//! **A published function never changes.** Its signature and its meaning stay; a new need is a new
//! function, and an old one stays at least one release, marked in the changelog as going. That is
//! why nothing crosses as a struct whose layout could grow: a pane's configuration is built with
//! setters ([`BtPaneConfig`]) and an event is read with getters ([`BtEvent`]), so a field added
//! later is a function added later. [`ABI_VERSION`] would count a breaking change — the rule says
//! there is none.
//!
//! **Every call is made on the main thread**: a pane is an `NSView`. A call from another thread
//! does nothing and returns its failure value. The configuration's setters, the event's getters
//! and [`bt_string_free`] touch no AppKit and may be called anywhere.
//!
//! **Memory crosses one way.** A string this library returns is the caller's, freed with
//! [`bt_string_free`]; a string an event lends lives only for the handler's call. A configuration
//! is consumed by [`bt_pane_open`] — or freed with [`bt_pane_config_free`] — and a pane handle by
//! [`bt_pane_close`].
//!
//! **Nothing unwinds into the host.** A panic inside a call is caught there and the call returns
//! its failure value.

use std::cell::{Cell, RefCell};
use std::ffi::{CStr, CString, OsStr, c_char, c_void};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::rc::Rc;

use bt_core::{
    CommandNews, CommandState, InitialInput, ProgramRecord, ProgramState, Session, Settings, Theme,
};
use bt_shell_macos::embed::{
    self, Cover, Foreground, Host, Identity, LinkRequest, Source, TerminalPane,
};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSAutoresizingMaskOptions, NSView};

mod layout;
mod strip;

pub use layout::*;
pub use strip::*;

/// The interface's version — what [`bt_embed_abi_version`] answers.
pub const ABI_VERSION: u32 = 1;

/// The event kinds [`bt_event_kind`] answers, numbered as the header numbers them.
pub mod kind {
    /// The title, the working directory or the remote state changed: read them again.
    pub const TITLE: u32 = 1;
    /// The shell exited; the pane has nothing left to show and is the host's to close.
    pub const SHELL_EXITED: u32 = 2;
    /// The keyboard arrived at the pane.
    pub const FOCUSED: u32 = 3;
    /// A command started or ended, or the pane went onto or off the alternate screen.
    pub const ACTIVITY: u32 = 4;
    /// A remote transfer's progress or existence changed.
    pub const UPLOADS: u32 = 5;
    /// A notification for the user: the title is the text, the body the detail.
    pub const NOTIFY: u32 = 6;
    /// Notices about the pane's setup (today its font): one per line in the text; the number is
    /// their source.
    pub const NOTICES: u32 = 7;
    /// Files are dragged over the pane (the flag) or no longer are.
    pub const FILES_DRAGGED: u32 = 8;
    /// A press with ⌥⌘ landed on the pane, at x/y in the window's points.
    pub const CARRY_PRESS: u32 = 9;
    /// A question of the pane was put off or taken up.
    pub const QUESTIONS: u32 = 10;
    /// A command started: its line in the text (NULL when the screen no longer shows it), the
    /// flag set for one run in our remote shell, its start time ([`bt_event_started`]).
    pub const COMMAND_STARTED: u32 = 11;
    /// A command ended: as [`COMMAND_STARTED`], with its exit code ([`bt_event_exit_code`]) and
    /// how long it ran ([`bt_event_duration_ms`]).
    pub const COMMAND_FINISHED: u32 = 12;
    /// The shell's directory changed: the path in the text, the host in the detail and the flag
    /// set in a remote session.
    pub const DIRECTORY: u32 = 13;
    /// The ports the pane's programs (or its server) listen on changed: read them again
    /// (`bt_pane_port_count`).
    pub const PORTS: u32 = 14;
    /// A program's status record (`OSC 7501`, or `OSC 9;4` standing in for the root one)
    /// changed or went: its id in the text (empty for the root record), its state in the number
    /// (a `BT_PROGRAM_*`; 0: it went) and its progress ([`bt_event_progress`]).
    pub const PROGRAM_STATUS: u32 = 15;
    /// The user opened a link (⌘-click, or the link menu's Open): the URL or path in the text,
    /// what it is in the number (a `BT_LINK_*`), the flag set for a path on the remote session's
    /// server (its host in the detail), a path's line and column ([`bt_event_line`],
    /// [`bt_event_column`]). The handler that opens it itself calls [`bt_event_set_handled`]
    /// and the pane does nothing more; otherwise the pane opens it as bateri does.
    pub const OPEN_LINK: u32 = 16;
}

/// What an opened link is ([`kind::OPEN_LINK`]'s number).
pub mod link {
    /// A URL, as written: plain text or an OSC 8 link's target, any scheme.
    pub const URL: u32 = 1;
    /// A file that exists.
    pub const FILE: u32 = 2;
    /// A directory that exists.
    pub const DIRECTORY: u32 = 3;
}

/// A program status record's state ([`kind::PROGRAM_STATUS`], `bt_pane_program_state`).
pub mod program {
    /// The record went: cleared, or ended by the shell's next prompt.
    pub const GONE: u32 = 0;
    /// At rest, waiting for the user's next instruction.
    pub const IDLE: u32 = 1;
    pub const WORKING: u32 = 2;
    /// It cannot go on until the user acts.
    pub const BLOCKED: u32 = 3;
    /// Finished, with a result the user has not seen.
    pub const DONE: u32 = 4;
    /// Failed and stopped.
    pub const ERROR: u32 = 5;
}

fn program_code(state: ProgramState) -> u32 {
    match state {
        ProgramState::Idle => program::IDLE,
        ProgramState::Working => program::WORKING,
        ProgramState::Blocked => program::BLOCKED,
        ProgramState::Done => program::DONE,
        ProgramState::Error => program::ERROR,
    }
}

/// What the shell is doing ([`bt_pane_phase`]); 0 without the shell integration.
pub mod phase {
    /// The prompt is being drawn.
    pub const PROMPT: u32 = 1;
    /// The user is typing a command.
    pub const INPUT: u32 = 2;
    /// A command runs.
    pub const RUNNING: u32 = 3;
    /// A command ended; the next prompt has not come yet.
    pub const FINISHED: u32 = 4;
}

/// What the parse calls answer ([`bt_pane_config_set_settings_toml`] and the theme's).
pub mod parse {
    /// Every key was accepted.
    pub const CLEAN: i32 = 0;
    /// The text was read; some keys were not accepted and took their defaults.
    pub const WITH_NOTES: i32 = 1;
    /// The text is not TOML; nothing changed.
    pub const FAILED: i32 = -1;
    /// A NULL or non-UTF-8 argument, or the wrong thread; nothing changed.
    pub const INVALID: i32 = -2;
}

/// A notice's source as [`bt_event_number`] gives it ([`kind::NOTICES`]).
pub mod notice_source {
    /// A write to the settings file failed (bateri's own menus; not a pane's).
    pub const WRITE: i64 = 0;
    /// The settings file (bateri's own; not a pane's).
    pub const SETTINGS: i64 = 1;
    /// The theme (bateri's own; not a pane's).
    pub const THEME: i64 = 2;
    /// The pane's font: the family was not found or is not monospaced.
    pub const FONT: i64 = 3;
}

/// `source` as [`bt_event_number`] gives it.
fn source_number(source: Source) -> i64 {
    match source {
        Source::Write => notice_source::WRITE,
        Source::Settings => notice_source::SETTINGS,
        Source::Theme => notice_source::THEME,
        Source::Font => notice_source::FONT,
    }
}

/// Runs `body`, or answers `failed` if it panics: an unwind must not cross into the host.
fn guarded<T>(failed: T, body: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(body)).unwrap_or(failed)
}

/// `text` as UTF-8, if it is a string at all.
///
/// # Safety
/// `text` is NULL or a NUL-terminated string alive for the call.
unsafe fn text<'a>(text: *const c_char) -> Option<&'a str> {
    if text.is_null() {
        return None;
    }
    // SAFETY: the caller's promise.
    unsafe { CStr::from_ptr(text) }.to_str().ok()
}

/// `path` as a path — any bytes but NUL, as the file system takes them; `None` if NULL or empty.
///
/// # Safety
/// `path` is NULL or a NUL-terminated string alive for the call.
unsafe fn path(path: *const c_char) -> Option<PathBuf> {
    if path.is_null() {
        return None;
    }
    // SAFETY: the caller's promise.
    let bytes = unsafe { CStr::from_ptr(path) }.to_bytes();
    (!bytes.is_empty()).then(|| PathBuf::from(OsStr::from_bytes(bytes)))
}

/// `text` handed to the caller, who frees it with [`bt_string_free`]. A NUL inside is dropped —
/// a C string cannot hold one.
fn handed(text: &[u8]) -> *mut c_char {
    c_text(text).map_or(null_mut(), CString::into_raw)
}

/// `text` as a C string, any NUL inside dropped.
fn c_text(text: &[u8]) -> Option<CString> {
    let bytes: Vec<u8> = text.iter().copied().filter(|&byte| byte != 0).collect();
    CString::new(bytes).ok()
}

/// A path's bytes handed to the caller ([`handed`]).
fn handed_path(path: &Path) -> *mut c_char {
    handed(path.as_os_str().as_bytes())
}

/// Writes `notes` — one per line — where `out` points, if it points anywhere and there are notes.
///
/// # Safety
/// `out` is NULL or points at a `char *` the caller can write.
unsafe fn write_notes(out: *mut *mut c_char, notes: &[String]) {
    if out.is_null() {
        return;
    }
    let text = (!notes.is_empty()).then(|| handed(notes.join("\n").as_bytes()));
    // SAFETY: the caller's promise.
    unsafe { out.write(text.unwrap_or(null_mut())) };
}

/// The interface's version: 1. A host checks it once, before anything else.
#[unsafe(no_mangle)]
pub extern "C" fn bt_embed_abi_version() -> u32 {
    ABI_VERSION
}

/// Frees a string this library returned. NULL is ignored.
///
/// # Safety
/// `text` is NULL or a string this library returned and not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_string_free(text: *mut c_char) {
    if !text.is_null() {
        // SAFETY: the caller's promise: it came from `CString::into_raw` here.
        drop(unsafe { CString::from_raw(text) });
    }
}

/// The function a host's events arrive at, on the main thread: the context it gave and the event,
/// lent for the call.
pub type BtEventHandler = unsafe extern "C" fn(context: *mut c_void, event: *const BtEvent);

/// What a pane is opened with, built by the host before [`bt_pane_open`]: a new shell in the
/// user's home, bateri's default settings and theme, no shell integration, and no handler until
/// one is set.
pub struct BtPaneConfig {
    id: u64,
    identity: Identity,
    settings: Settings,
    theme: Theme,
    working_directory: Option<PathBuf>,
    env: Vec<(String, String)>,
    command: Option<String>,
    handler: Option<BtEventHandler>,
    context: *mut c_void,
}

/// A configuration for the pane `id` — the number its events carry, unique for the process's
/// life — of the application `app_name` (named where a pane says whose it is: a downloaded file's
/// quarantine record). NULL if `app_name` is NULL or not UTF-8.
///
/// # Safety
/// `app_name` is NULL or a NUL-terminated string alive for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_pane_config_new(id: u64, app_name: *const c_char) -> *mut BtPaneConfig {
    guarded(null_mut(), || {
        // SAFETY: the caller's promise.
        let Some(app_name) = (unsafe { text(app_name) }) else {
            return null_mut();
        };
        Box::into_raw(Box::new(BtPaneConfig {
            id,
            identity: Identity {
                app_name: app_name.to_owned(),
                helper: None,
                zsh_wrapper_dir: None,
            },
            settings: Settings::default(),
            theme: Theme::BATERI,
            working_directory: None,
            env: Vec::new(),
            command: None,
            handler: None,
            context: null_mut(),
        }))
    })
}

/// Frees a configuration that was never opened. NULL is ignored.
///
/// # Safety
/// `config` is NULL or a configuration from [`bt_pane_config_new`], not yet consumed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_pane_config_free(config: *mut BtPaneConfig) {
    guarded((), || {
        if !config.is_null() {
            // SAFETY: the caller's promise.
            drop(unsafe { Box::from_raw(config) });
        }
    });
}

/// Runs `body` on the configuration `config` points at; `failed` if it is NULL or `body` panics.
///
/// # Safety
/// `config` is NULL or a configuration from [`bt_pane_config_new`], not yet consumed.
unsafe fn with_config<T: Copy>(
    config: *mut BtPaneConfig,
    failed: T,
    body: impl FnOnce(&mut BtPaneConfig) -> T,
) -> T {
    guarded(failed, || {
        // SAFETY: the caller's promise.
        match unsafe { config.as_mut() } {
            Some(config) => body(config),
            None => failed,
        }
    })
}

/// The program that answers the shell integration's helper calls (bateri's executable answers
/// `ssh-argv` and `ssh-fell-back`). NULL or empty: none — the integration's `ssh` runs plain ssh.
///
/// # Safety
/// `config` as in [`bt_pane_config_free`]; `path` is NULL or a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_pane_config_set_helper(
    config: *mut BtPaneConfig,
    path: *const c_char,
) -> bool {
    // SAFETY: the caller's promises.
    unsafe {
        with_config(config, false, |config| {
            config.identity.helper = path_arg(path);
            true
        })
    }
}

/// The shell integration's zsh scripts (bateri's `Resources/shell/zsh`). NULL or empty: no
/// integration — a plain terminal, without the dock and the command blocks.
///
/// # Safety
/// As [`bt_pane_config_set_helper`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_pane_config_set_zsh_scripts(
    config: *mut BtPaneConfig,
    dir: *const c_char,
) -> bool {
    // SAFETY: the caller's promises.
    unsafe {
        with_config(config, false, |config| {
            config.identity.zsh_wrapper_dir = path_arg(dir);
            true
        })
    }
}

/// Where the shell starts. NULL or empty: the user's home.
///
/// # Safety
/// As [`bt_pane_config_set_helper`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_pane_config_set_working_directory(
    config: *mut BtPaneConfig,
    dir: *const c_char,
) -> bool {
    // SAFETY: the caller's promises.
    unsafe {
        with_config(config, false, |config| {
            config.working_directory = path_arg(dir);
            true
        })
    }
}

/// [`path`] for an argument of a call whose caller promised it.
fn path_arg(arg: *const c_char) -> Option<PathBuf> {
    // SAFETY: every caller passes an argument its own caller promised to be NULL or a
    // NUL-terminated string alive for the call.
    unsafe { path(arg) }
}

/// Adds `name=value` to the shell's environment. The locale's and the shell integration's own
/// variables win on a clash; `TERM` and the pane's identity (`TERM_SESSION_ID`, …) are never
/// overridden. `false` for a NULL or non-UTF-8 argument, an empty name or one holding `=`.
///
/// # Safety
/// `config` as in [`bt_pane_config_free`]; `name` and `value` NULL or NUL-terminated strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_pane_config_add_env(
    config: *mut BtPaneConfig,
    name: *const c_char,
    value: *const c_char,
) -> bool {
    // SAFETY: the caller's promises.
    unsafe {
        with_config(config, false, |config| {
            let (Some(name), Some(value)) = (text(name), text(value)) else {
                return false;
            };
            if name.is_empty() || name.contains('=') {
                return false;
            }
            config.env.push((name.to_owned(), value.to_owned()));
            true
        })
    }
}

/// A line the shell runs once it is ready. NULL clears it. `false` if not UTF-8.
///
/// # Safety
/// `config` as in [`bt_pane_config_free`]; `line` NULL or a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_pane_config_set_command(
    config: *mut BtPaneConfig,
    line: *const c_char,
) -> bool {
    // SAFETY: the caller's promises.
    unsafe {
        with_config(config, false, |config| {
            if line.is_null() {
                config.command = None;
                return true;
            }
            let Some(line) = text(line) else {
                return false;
            };
            config.command = Some(line.to_owned());
            true
        })
    }
}

/// Where the pane's events go: `handler`, called with `context`. NULL: nowhere.
///
/// # Safety
/// `config` as in [`bt_pane_config_free`]; `handler`, if set, takes `context` and the event for
/// as long as the pane lives.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_pane_config_set_event_handler(
    config: *mut BtPaneConfig,
    handler: Option<BtEventHandler>,
    context: *mut c_void,
) -> bool {
    // SAFETY: the caller's promise.
    unsafe {
        with_config(config, false, |config| {
            config.handler = handler;
            config.context = context;
            true
        })
    }
}

/// The pane's settings from `settings.toml`'s text — any key not given takes its default, so the
/// host need not keep bateri's file. Answers a [`parse`] code; with notes (or the reason it was not
/// read), `notes` — if not NULL — gets them one per line, the caller's to free, else NULL.
///
/// # Safety
/// `config` as in [`bt_pane_config_free`]; `toml` NULL or a NUL-terminated string; `notes` NULL or
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_pane_config_set_settings_toml(
    config: *mut BtPaneConfig,
    toml: *const c_char,
    notes: *mut *mut c_char,
) -> i32 {
    // SAFETY: the caller's promises.
    unsafe {
        with_config(config, parse::INVALID, |config| {
            let Some(toml) = text(toml) else {
                return parse::INVALID;
            };
            match Settings::parse(toml) {
                Ok(parsed) => {
                    let said: Vec<String> =
                        parsed.diagnostics.iter().map(ToString::to_string).collect();
                    write_notes(notes, &said);
                    config.settings = parsed.settings;
                    if said.is_empty() {
                        parse::CLEAN
                    } else {
                        parse::WITH_NOTES
                    }
                }
                Err(reason) => {
                    write_notes(notes, &[reason.to_string()]);
                    parse::FAILED
                }
            }
        })
    }
}

/// The pane's colours: one of bateri's built-in themes by name (`bateri`, `bateri-light`, …).
/// `false` for a name it does not have.
///
/// # Safety
/// `config` as in [`bt_pane_config_free`]; `name` NULL or a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_pane_config_set_theme_named(
    config: *mut BtPaneConfig,
    name: *const c_char,
) -> bool {
    // SAFETY: the caller's promises.
    unsafe {
        with_config(config, false, |config| {
            let Some(theme) = text(name).and_then(Theme::embedded) else {
                return false;
            };
            config.theme = theme;
            true
        })
    }
}

/// The pane's colours from a theme file's text (bateri's theme format; a key not given comes from
/// the built-in `bateri`). Answers as [`bt_pane_config_set_settings_toml`].
///
/// # Safety
/// As [`bt_pane_config_set_settings_toml`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_pane_config_set_theme_toml(
    config: *mut BtPaneConfig,
    toml: *const c_char,
    notes: *mut *mut c_char,
) -> i32 {
    // SAFETY: the caller's promises.
    unsafe {
        with_config(config, parse::INVALID, |config| {
            let Some(toml) = text(toml) else {
                return parse::INVALID;
            };
            let (code, theme) = parse_theme(toml, notes);
            if let Some(theme) = theme {
                config.theme = theme;
            }
            code
        })
    }
}

/// A theme file's text over the built-in `bateri`: the [`parse`] code and the theme, if read.
///
/// # Safety
/// `notes` NULL or writable.
unsafe fn parse_theme(toml: &str, notes: *mut *mut c_char) -> (i32, Option<Theme>) {
    match Theme::parse(toml, &Theme::BATERI) {
        Ok((theme, diagnostics)) => {
            let said: Vec<String> = diagnostics.iter().map(ToString::to_string).collect();
            // SAFETY: the caller's promise.
            unsafe { write_notes(notes, &said) };
            let code = if said.is_empty() {
                parse::CLEAN
            } else {
                parse::WITH_NOTES
            };
            (code, Some(theme))
        }
        Err(reason) => {
            // SAFETY: the caller's promise.
            unsafe { write_notes(notes, &[reason.to_string()]) };
            (parse::FAILED, None)
        }
    }
}

/// An open pane, as the host holds it: the pane, and its events' way to the host — cut when the
/// host closes it.
pub struct BtPane {
    pane: Retained<TerminalPane>,
    host: Rc<CHost>,
}

/// Opens a pane filling `parent` — an `NSView *` — and following its size; consumes `config`,
/// whatever the outcome. The shell starts with [`bt_pane_start`], once `parent` is in a window.
/// NULL if `parent` or `config` is NULL, off the main thread, or if the GPU could not be set up
/// (the reason goes to stderr).
///
/// # Safety
/// `parent` is NULL or an `NSView` alive for the call; `config` as in [`bt_pane_config_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_pane_open(
    parent: *mut c_void,
    config: *mut BtPaneConfig,
) -> *mut BtPane {
    guarded(null_mut(), || {
        // SAFETY: the caller's promise; consumed here whatever comes.
        let config = (!config.is_null()).then(|| unsafe { Box::from_raw(config) });
        let (Some(mtm), Some(config)) = (MainThreadMarker::new(), config) else {
            return null_mut();
        };
        // SAFETY: the caller's promise.
        let Some(parent) = (unsafe { parent.cast::<NSView>().as_ref() }) else {
            return null_mut();
        };
        let BtPaneConfig {
            id,
            identity,
            settings,
            theme,
            working_directory,
            env,
            command,
            handler,
            context,
        } = *config;
        let host = Rc::new(CHost {
            handler,
            context,
            closed: Cell::new(false),
            command: Cell::new(None),
            place: RefCell::new(None),
            programs: RefCell::new(Vec::new()),
        });
        let mut config =
            embed::Config::new(mtm, id, host.clone(), Rc::new(identity), settings, theme);
        if working_directory.is_some() {
            config.working_directory = working_directory;
        }
        config.initial_input = command.map(InitialInput::run);
        config.env = env;
        let pane = match embed::open(mtm, parent.bounds(), config) {
            Ok(pane) => pane,
            Err(error) => {
                eprintln!("bt-embed: the pane could not open: {error}");
                return null_mut();
            }
        };
        pane.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        parent.addSubview(&pane);
        pane.observe_frame();
        Box::into_raw(Box::new(BtPane { pane, host }))
    })
}

/// Runs `body` on the pane `pane` points at, on the main thread; `failed` if it is NULL, off the
/// main thread, or `body` panics.
///
/// # Safety
/// `pane` is NULL or a handle from [`bt_pane_open`], not yet closed.
unsafe fn with_pane<T: Copy>(
    pane: *const BtPane,
    failed: T,
    body: impl FnOnce(&TerminalPane, MainThreadMarker) -> T,
) -> T {
    guarded(failed, || {
        // SAFETY: the caller's promise.
        match (unsafe { pane.as_ref() }, MainThreadMarker::new()) {
            (Some(pane), Some(mtm)) => body(&pane.pane, mtm),
            _ => failed,
        }
    })
}

/// Starts the pane's shell. `false` if it could not start (the reason goes to stderr).
///
/// # Safety
/// `pane` is NULL or a handle from [`bt_pane_open`], not yet closed — for every `bt_pane_*` call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_pane_start(pane: *mut BtPane) -> bool {
    // SAFETY: the caller's promise.
    unsafe {
        with_pane(pane, false, |pane, mtm| match pane.start(mtm) {
            Ok(()) => true,
            Err(error) => {
                eprintln!("bt-embed: the shell could not start: {error}");
                false
            }
        })
    }
}

/// Gives the keyboard to the pane. `false` if it is in no window or the window refused.
///
/// # Safety
/// As [`bt_pane_start`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_pane_focus(pane: *mut BtPane) -> bool {
    // SAFETY: the caller's promise.
    unsafe { with_pane(pane, false, |pane, _| pane.focus()) }
}

/// Hides or shows the pane. Hidden, it draws nothing and lets the focus go; shown, it draws again
/// and takes the focus if its window is key. The pane follows its window by itself (key state,
/// occlusion, scale); this is for the host's own hiding.
///
/// # Safety
/// As [`bt_pane_start`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_pane_set_hidden(pane: *mut BtPane, hidden: bool) -> bool {
    // SAFETY: the caller's promise.
    unsafe {
        with_pane(pane, false, |pane, _| {
            pane.setHidden(hidden);
            pane.refresh_visibility();
            true
        })
    }
}

/// The host hid or showed a view above the pane: the pane reads where it stands again — no
/// notification tells a view that.
///
/// # Safety
/// As [`bt_pane_start`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_pane_visibility_changed(pane: *mut BtPane) -> bool {
    // SAFETY: the caller's promise.
    unsafe {
        with_pane(pane, false, |pane, _| {
            pane.refresh_visibility();
            true
        })
    }
}

/// Writes `len` bytes to the shell as if typed. `false` before the shell started.
///
/// # Safety
/// As [`bt_pane_start`]; `bytes` points at `len` readable bytes (or is NULL with `len` 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_pane_write(pane: *mut BtPane, bytes: *const u8, len: usize) -> bool {
    // SAFETY: the caller's promise.
    unsafe {
        with_pane(pane, false, |pane, _| {
            let Some(session) = pane.session() else {
                return false;
            };
            if len == 0 {
                return true;
            }
            if bytes.is_null() {
                return false;
            }
            // SAFETY: the caller's promise.
            session.write(std::slice::from_raw_parts(bytes, len));
            true
        })
    }
}

/// Pastes `text` into the shell as the user's paste would (bracketed if the program asked).
/// `false` before the shell started or for a NULL argument.
///
/// # Safety
/// As [`bt_pane_start`]; `text` NULL or a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_pane_paste(pane: *mut BtPane, text: *const c_char) -> bool {
    // SAFETY: the caller's promise.
    unsafe {
        with_pane(pane, false, |pane, _| {
            let Some(session) = pane.session() else {
                return false;
            };
            if text.is_null() {
                return false;
            }
            // SAFETY: the caller's promise.
            session.paste(CStr::from_ptr(text).to_bytes().to_vec());
            true
        })
    }
}

/// Changes the pane's colours to a built-in theme. `false` for a name it does not have.
///
/// # Safety
/// As [`bt_pane_start`]; `name` NULL or a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_pane_set_theme_named(pane: *mut BtPane, name: *const c_char) -> bool {
    // SAFETY: the caller's promise.
    unsafe {
        with_pane(pane, false, |pane, _| {
            let Some(theme) = text(name).and_then(Theme::embedded) else {
                return false;
            };
            pane.set_theme(theme);
            true
        })
    }
}

/// Changes the pane's colours to a theme file's text. Answers as
/// [`bt_pane_config_set_theme_toml`].
///
/// # Safety
/// As [`bt_pane_start`]; `toml` and `notes` as in [`bt_pane_config_set_settings_toml`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_pane_set_theme_toml(
    pane: *mut BtPane,
    toml: *const c_char,
    notes: *mut *mut c_char,
) -> i32 {
    // SAFETY: the caller's promise.
    unsafe {
        with_pane(pane, parse::INVALID, |pane, _| {
            let Some(toml) = text(toml) else {
                return parse::INVALID;
            };
            let (code, theme) = parse_theme(toml, notes);
            if let Some(theme) = theme {
                pane.set_theme(theme);
            }
            code
        })
    }
}

/// The pane's title — the program's own (OSC 0/2), else its directory's last part — the caller's
/// to free. NULL before the shell started.
///
/// # Safety
/// As [`bt_pane_start`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_pane_title(pane: *mut BtPane) -> *mut c_char {
    // SAFETY: the caller's promise.
    unsafe {
        with_pane(pane, null_mut(), |pane, _| {
            pane.session()
                .map_or(null_mut(), |session| handed(session.title().as_bytes()))
        })
    }
}

/// The shell's working directory as the shell last reported it, the caller's to free. NULL
/// before it reported one.
///
/// # Safety
/// As [`bt_pane_start`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_pane_working_directory(pane: *mut BtPane) -> *mut c_char {
    // SAFETY: the caller's promise.
    unsafe {
        with_pane(pane, null_mut(), |pane, _| {
            pane.session()
                .and_then(|session| session.working_directory())
                .map_or(null_mut(), |dir| handed_path(&dir))
        })
    }
}

/// The pane's persistent identity — the uppercase UUID its shell sees as `TERM_SESSION_ID` — the
/// caller's to free.
///
/// # Safety
/// As [`bt_pane_start`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_pane_uuid(pane: *mut BtPane) -> *mut c_char {
    // SAFETY: the caller's promise.
    unsafe {
        with_pane(pane, null_mut(), |pane, _| {
            handed(pane.uuid().as_str().as_bytes())
        })
    }
}

/// The smallest size the pane can be laid out at, in points — the columns and rows a terminal
/// needs at its font and screen, for the layout engine (`bt_world_set_minimum`). `false` while it
/// is in no window.
///
/// # Safety
/// As [`bt_pane_start`]; `width` and `height` NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_pane_min_size(
    pane: *mut BtPane,
    width: *mut f64,
    height: *mut f64,
) -> bool {
    // SAFETY: the caller's promise.
    unsafe {
        with_pane(pane, false, |pane, _| {
            let Some(size) = pane.min_size() else {
                return false;
            };
            if !width.is_null() {
                width.write(size.width);
            }
            if !height.is_null() {
                height.write(size.height);
            }
            true
        })
    }
}

/// What the shell is doing (a `BT_PHASE_*`); 0 before the shell started and without the shell
/// integration, which is what reports it.
///
/// # Safety
/// As [`bt_pane_start`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_pane_phase(pane: *mut BtPane) -> u32 {
    // SAFETY: the caller's promise.
    unsafe {
        with_pane(pane, 0, |pane, _| {
            match pane
                .session()
                .and_then(|session| session.shell_state())
                .map(|state| state.phase)
            {
                Some(bt_core::ShellPhase::Prompt) => phase::PROMPT,
                Some(bt_core::ShellPhase::Input) => phase::INPUT,
                Some(bt_core::ShellPhase::Running) => phase::RUNNING,
                Some(bt_core::ShellPhase::Finished) => phase::FINISHED,
                None => 0,
            }
        })
    }
}

/// The programs in the foreground in place of the shell, one name per line in process order, the
/// caller's to free; NULL while the shell itself is in the foreground. An empty string: something
/// runs whose name could not be read. Asks the process table — for a question, not a loop.
///
/// # Safety
/// As [`bt_pane_start`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_pane_foreground(pane: *mut BtPane) -> *mut c_char {
    // SAFETY: the caller's promise.
    unsafe {
        with_pane(pane, null_mut(), |pane, _| match pane.foreground() {
            Foreground::Idle => null_mut(),
            Foreground::Running(names) => handed(names.join("\n").as_bytes()),
        })
    }
}

/// The host of the remote session the pane is in, as the user wrote it (`user@db1`), the
/// caller's to free; NULL locally.
///
/// # Safety
/// As [`bt_pane_start`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_pane_remote_host(pane: *mut BtPane) -> *mut c_char {
    // SAFETY: the caller's promise.
    unsafe {
        with_pane(pane, null_mut(), |pane, _| {
            pane.session()
                .and_then(|session| session.remote_target())
                .map_or(null_mut(), |(_, target, _)| handed(target.host.as_bytes()))
        })
    }
}

/// The remote shell's directory as it last reported it, the caller's to free; NULL locally and
/// before it reported one.
///
/// # Safety
/// As [`bt_pane_start`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_pane_remote_directory(pane: *mut BtPane) -> *mut c_char {
    // SAFETY: the caller's promise.
    unsafe {
        with_pane(pane, null_mut(), |pane, _| {
            pane.session()
                .and_then(|session| session.remote_target())
                .filter(|(_, _, cwd)| !cwd.is_empty())
                .map_or(null_mut(), |(_, _, cwd)| handed(cwd.as_bytes()))
        })
    }
}

/// The program status record at `index` (by id), if there is one.
fn program_at(pane: &TerminalPane, index: usize) -> Option<ProgramRecord> {
    pane.session()?.program_records().into_iter().nth(index)
}

/// How many program status records the pane holds — what programs that report their status
/// (`OSC 7501`, `OSC 9;4`) say they are doing.
///
/// # Safety
/// As [`bt_pane_start`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_pane_program_count(pane: *mut BtPane) -> usize {
    // SAFETY: the caller's promise.
    unsafe {
        with_pane(pane, 0, |pane, _| {
            pane.session()
                .map_or(0, |session| session.program_records().len())
        })
    }
}

/// The `index`th record's id (by id order; the root record's is empty), the caller's to free;
/// NULL past the end.
///
/// # Safety
/// As [`bt_pane_start`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_pane_program_id(pane: *mut BtPane, index: usize) -> *mut c_char {
    // SAFETY: the caller's promise.
    unsafe {
        with_pane(pane, null_mut(), |pane, _| {
            program_at(pane, index).map_or(null_mut(), |record| handed(record.id.as_bytes()))
        })
    }
}

/// The `index`th record's state (a `BT_PROGRAM_*`); 0 past the end.
///
/// # Safety
/// As [`bt_pane_start`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_pane_program_state(pane: *mut BtPane, index: usize) -> u32 {
    // SAFETY: the caller's promise.
    unsafe {
        with_pane(pane, program::GONE, |pane, _| {
            program_at(pane, index).map_or(program::GONE, |record| program_code(record.state))
        })
    }
}

/// The `index`th record's progress, from 0 to 100; -1 when it gives none or past the end.
///
/// # Safety
/// As [`bt_pane_start`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_pane_program_progress(pane: *mut BtPane, index: usize) -> i32 {
    // SAFETY: the caller's promise.
    unsafe {
        with_pane(pane, -1, |pane, _| {
            program_at(pane, index)
                .and_then(|record| record.progress)
                .map_or(-1, i32::from)
        })
    }
}

/// How many listening ports the pane knows: its programs' and, in a remote session, its
/// server's. Kept up to date while `[shell] ports` is on; a PORTS event says when they changed.
///
/// # Safety
/// As [`bt_pane_start`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_pane_port_count(pane: *mut BtPane) -> usize {
    // SAFETY: the caller's promise.
    unsafe { with_pane(pane, 0, |pane, _| pane.listening_ports().len()) }
}

/// The `index`th listening port: `true` and its number in `port`, and in `remote` whether it is
/// the server's; `false` past the end.
///
/// # Safety
/// As [`bt_pane_start`]; `port` and `remote` NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_pane_port_at(
    pane: *mut BtPane,
    index: usize,
    port: *mut u16,
    remote: *mut bool,
) -> bool {
    // SAFETY: the caller's promises.
    unsafe {
        with_pane(pane, false, |pane, _| {
            let Some(found) = pane.listening_ports().get(index).copied() else {
                return false;
            };
            if !port.is_null() {
                port.write(found.port);
            }
            if !remote.is_null() {
                remote.write(found.remote);
            }
            true
        })
    }
}

/// The id the pane was configured with — the one its events carry. 0 for NULL.
///
/// # Safety
/// As [`bt_pane_start`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_pane_id(pane: *mut BtPane) -> u64 {
    // SAFETY: the caller's promise.
    unsafe { with_pane(pane, 0, |pane, _| pane.id()) }
}

/// The pane's `NSView *`, owned by the pane: the host may move it to another view or resize it.
///
/// # Safety
/// As [`bt_pane_start`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_pane_view(pane: *mut BtPane) -> *mut c_void {
    // SAFETY: the caller's promise.
    unsafe {
        with_pane(pane, null_mut(), |pane, _| {
            std::ptr::from_ref(pane).cast_mut().cast::<c_void>()
        })
    }
}

/// Closes the pane: its shell is told to end, nothing waits for it, its view leaves its parent and
/// the handle is freed. No event of the pane reaches the host from here on — closing itself would
/// send some (its transfers end), to a host whose handle is already gone. Safe to call from the
/// pane's own event handler. Off the main thread it does nothing — the handle stays the caller's.
///
/// # Safety
/// As [`bt_pane_start`]; the handle is not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_pane_close(pane: *mut BtPane) {
    guarded((), || {
        if pane.is_null() || MainThreadMarker::new().is_none() {
            return;
        }
        // SAFETY: the caller's promise.
        let BtPane { pane, host } = *unsafe { Box::from_raw(pane) };
        host.closed.set(true);
        pane.close();
        pane.removeFromSuperview();
        // The handler may be closing the pane from inside one of its events, with the pane's own
        // code still on the stack: the last release waits for the autorelease pool to drain.
        let _ = Retained::autorelease_ptr(pane);
    });
}

/// An event as a host's handler reads it, lent for the handler's call ([`bt_event_kind`] and the
/// other getters). Which fields an event fills is its kind's ([`kind`]); the others read as empty.
pub struct BtEvent {
    kind: u32,
    pane: u64,
    text: Option<CString>,
    detail: Option<CString>,
    flag: bool,
    number: i64,
    x: f64,
    y: f64,
    exit: Option<i32>,
    duration_ms: Option<u64>,
    started: Option<u32>,
    progress: Option<u8>,
    line: Option<u32>,
    column: Option<u32>,
    /// The handler took the event's request ([`bt_event_set_handled`]).
    handled: Cell<bool>,
}

impl BtEvent {
    fn new(kind: u32, pane: u64) -> Self {
        Self {
            kind,
            pane,
            text: None,
            detail: None,
            flag: false,
            number: 0,
            x: 0.0,
            y: 0.0,
            exit: None,
            duration_ms: None,
            started: None,
            progress: None,
            line: None,
            column: None,
            handled: Cell::new(false),
        }
    }

    /// A command's news, its line read from `session` while the screen still shows it.
    fn command(kind: u32, pane: u64, state: CommandState, session: &Session) -> Self {
        Self {
            text: session
                .command_line(state.id)
                .and_then(|line| c_text(line.as_bytes())),
            flag: state.remote,
            exit: state.exit,
            duration_ms: state
                .elapsed
                .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)),
            started: state.started,
            ..Self::new(kind, pane)
        }
    }
}

/// Runs `body` on the event `event` points at; `failed` if it is NULL.
///
/// # Safety
/// `event` is NULL or the event a handler was lent, during that call.
unsafe fn with_event<T: Copy>(
    event: *const BtEvent,
    failed: T,
    body: impl FnOnce(&BtEvent) -> T,
) -> T {
    guarded(failed, || {
        // SAFETY: the caller's promise.
        unsafe { event.as_ref() }.map_or(failed, body)
    })
}

/// The event's kind ([`kind`]); 0 for NULL.
///
/// # Safety
/// `event` is NULL or the event a handler was lent, during that call — for every `bt_event_*` call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_event_kind(event: *const BtEvent) -> u32 {
    // SAFETY: the caller's promise.
    unsafe { with_event(event, 0, |event| event.kind) }
}

/// The id of the pane the event is about.
///
/// # Safety
/// As [`bt_event_kind`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_event_pane(event: *const BtEvent) -> u64 {
    // SAFETY: the caller's promise.
    unsafe { with_event(event, 0, |event| event.pane) }
}

/// The event's text, lent for the handler's call; NULL if its kind has none.
///
/// # Safety
/// As [`bt_event_kind`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_event_text(event: *const BtEvent) -> *const c_char {
    // SAFETY: the caller's promise.
    unsafe {
        with_event(event, std::ptr::null(), |event| {
            event.text.as_deref().map_or(std::ptr::null(), CStr::as_ptr)
        })
    }
}

/// The event's second text (a notification's body), lent for the handler's call; NULL if none.
///
/// # Safety
/// As [`bt_event_kind`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_event_detail(event: *const BtEvent) -> *const c_char {
    // SAFETY: the caller's promise.
    unsafe {
        with_event(event, std::ptr::null(), |event| {
            event
                .detail
                .as_deref()
                .map_or(std::ptr::null(), CStr::as_ptr)
        })
    }
}

/// The event's yes/no (files dragged over the pane or not).
///
/// # Safety
/// As [`bt_event_kind`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_event_flag(event: *const BtEvent) -> bool {
    // SAFETY: the caller's promise.
    unsafe { with_event(event, false, |event| event.flag) }
}

/// The event's number (a notice's source).
///
/// # Safety
/// As [`bt_event_kind`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_event_number(event: *const BtEvent) -> i64 {
    // SAFETY: the caller's promise.
    unsafe { with_event(event, 0, |event| event.number) }
}

/// The event's point's x, in the window's points.
///
/// # Safety
/// As [`bt_event_kind`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_event_x(event: *const BtEvent) -> f64 {
    // SAFETY: the caller's promise.
    unsafe { with_event(event, 0.0, |event| event.x) }
}

/// The event's point's y, in the window's points.
///
/// # Safety
/// As [`bt_event_kind`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_event_y(event: *const BtEvent) -> f64 {
    // SAFETY: the caller's promise.
    unsafe { with_event(event, 0.0, |event| event.y) }
}

/// The exit code of a COMMAND_FINISHED or SHELL_EXITED event: `true` and written to `code`;
/// `false` when it is not known (the shell printed it unreadably, a signal ended the shell) or the
/// kind has none. A SHELL_EXITED code is the code of the process bateri started the shell with: on
/// macOS that is `login`, which answers 0 whatever the shell's own exit code was — it tells a
/// normal exit from a killed one, not the shell's status.
///
/// # Safety
/// As [`bt_event_kind`]; `code` NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_event_exit_code(event: *const BtEvent, code: *mut i32) -> bool {
    // SAFETY: the caller's promises.
    unsafe {
        with_event(event, false, |event| {
            let Some(exit) = event.exit else {
                return false;
            };
            if !code.is_null() {
                code.write(exit);
            }
            true
        })
    }
}

/// How long a COMMAND_FINISHED event's command ran, in milliseconds; -1 when its start was never
/// seen or the kind has none.
///
/// # Safety
/// As [`bt_event_kind`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_event_duration_ms(event: *const BtEvent) -> i64 {
    // SAFETY: the caller's promise.
    unsafe {
        with_event(event, -1, |event| {
            event
                .duration_ms
                .map_or(-1, |ms| i64::try_from(ms).unwrap_or(i64::MAX))
        })
    }
}

/// An OPEN_LINK event's line (1-based, as `path:12` names it); 0 when it names none.
///
/// # Safety
/// As [`bt_event_kind`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_event_line(event: *const BtEvent) -> u32 {
    // SAFETY: the caller's promise.
    unsafe { with_event(event, 0, |event| event.line.unwrap_or(0)) }
}

/// An OPEN_LINK event's column (1-based, as `path:12:5` names it); 0 when it names none.
///
/// # Safety
/// As [`bt_event_kind`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_event_column(event: *const BtEvent) -> u32 {
    // SAFETY: the caller's promise.
    unsafe { with_event(event, 0, |event| event.column.unwrap_or(0)) }
}

/// The handler took the event's request: an OPEN_LINK it opened itself — the pane does nothing
/// more. Only during the handler's call; for other kinds it changes nothing.
///
/// # Safety
/// As [`bt_event_kind`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_event_set_handled(event: *const BtEvent) {
    // SAFETY: the caller's promise.
    unsafe { with_event(event, (), |event| event.handled.set(true)) }
}

/// A PROGRAM_STATUS event's progress, from 0 to 100; -1 when the record gives none.
///
/// # Safety
/// As [`bt_event_kind`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_event_progress(event: *const BtEvent) -> i32 {
    // SAFETY: the caller's promise.
    unsafe { with_event(event, -1, |event| event.progress.map_or(-1, i32::from)) }
}

/// When a command event's command started, seconds since the Unix epoch; 0 when unknown.
///
/// # Safety
/// As [`bt_event_kind`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_event_started(event: *const BtEvent) -> i64 {
    // SAFETY: the caller's promise.
    unsafe { with_event(event, 0, |event| event.started.map_or(0, i64::from)) }
}

/// Where the shell stands: a remote session's host, the directory as the shell last reported it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Place {
    remote: bool,
    host: Option<String>,
    path: Option<Vec<u8>>,
}

impl Place {
    fn of(session: &Session) -> Self {
        match session.remote_target() {
            Some((_, target, cwd)) => Self {
                remote: true,
                host: Some(target.host),
                path: (!cwd.is_empty()).then(|| cwd.into_bytes()),
            },
            None => Self {
                remote: false,
                host: None,
                path: session
                    .working_directory()
                    .map(|dir| dir.into_os_string().into_vec()),
            },
        }
    }
}

/// The pane's owner on the C side: every event becomes a [`BtEvent`] handed to the host's handler,
/// until the host closes the pane.
struct CHost {
    handler: Option<BtEventHandler>,
    context: *mut c_void,
    /// The host closed the pane ([`bt_pane_close`]): its handle is gone, and so are its events.
    closed: Cell<bool>,
    /// The newest command the last look saw ([`bt_core::command_news`]).
    command: Cell<Option<CommandState>>,
    /// Where the shell stood at the last look.
    place: RefCell<Option<Place>>,
    /// The program status records at the last look.
    programs: RefCell<Vec<ProgramRecord>>,
}

impl CHost {
    fn send(&self, event: BtEvent) {
        self.send_ref(&event);
    }

    fn send_ref(&self, event: &BtEvent) {
        if self.closed.get() {
            return;
        }
        if let Some(handler) = self.handler {
            // SAFETY: the host's promise at `bt_pane_config_set_event_handler`: the handler takes
            // the context it gave and an event lent for the call.
            unsafe { handler(self.context, event) };
        }
    }

    fn plain(&self, kind: u32, pane: u64) {
        self.send(BtEvent::new(kind, pane));
    }

    /// Sends `event` and reads `answer` off it once the handler is done; `false` when nothing
    /// heard it (the pane closed, no handler).
    fn send_and(&self, event: BtEvent, answer: impl Fn(&BtEvent) -> bool) -> bool {
        if self.closed.get() || self.handler.is_none() {
            return false;
        }
        self.send_ref(&event);
        answer(&event)
    }

    /// The session of pane `pane`, if it is open and started.
    fn session(pane: u64) -> Option<std::sync::Arc<Session>> {
        let mtm = MainThreadMarker::new()?;
        embed::pane(mtm, pane).and_then(|pane| pane.session().cloned())
    }

    /// The commands' news since the last look: each start and end the look did not see.
    fn commands(&self, pane: u64) {
        let Some(session) = Self::session(pane) else {
            return;
        };
        let now = session.last_command();
        let before = self.command.get();
        let before_now = before
            .filter(|before| now.is_some_and(|now| now.id != before.id))
            .and_then(|before| session.command(before.id));
        for news in bt_core::command_news(before, before_now, now) {
            let event = match news {
                CommandNews::Started(state) => {
                    BtEvent::command(kind::COMMAND_STARTED, pane, state, &session)
                }
                CommandNews::Finished(state) => {
                    BtEvent::command(kind::COMMAND_FINISHED, pane, state, &session)
                }
            };
            self.send(event);
        }
        if now.is_some() {
            self.command.set(now);
        }
    }

    /// The program status records' news: each record that changed or came, and each that went.
    fn programs(&self, pane: u64) {
        let Some(session) = Self::session(pane) else {
            return;
        };
        let now = session.program_records();
        let before = self.programs.replace(now.clone());
        for record in &now {
            if !before.contains(record) {
                self.send(BtEvent {
                    text: c_text(record.id.as_bytes()),
                    number: i64::from(program_code(record.state)),
                    progress: record.progress,
                    ..BtEvent::new(kind::PROGRAM_STATUS, pane)
                });
            }
        }
        for gone in before
            .iter()
            .filter(|old| !now.iter().any(|record| record.id == old.id))
        {
            self.send(BtEvent {
                text: c_text(gone.id.as_bytes()),
                number: i64::from(program::GONE),
                ..BtEvent::new(kind::PROGRAM_STATUS, pane)
            });
        }
    }

    /// The directory's news: where the shell stands, if it moved since the last look.
    fn place(&self, pane: u64) {
        let Some(session) = Self::session(pane) else {
            return;
        };
        let now = Place::of(&session);
        if self.place.borrow().as_ref() == Some(&now) {
            return;
        }
        self.place.replace(Some(now.clone()));
        if now.path.is_none() && now.host.is_none() {
            return;
        }
        self.send(BtEvent {
            text: now.path.as_deref().and_then(c_text),
            detail: now.host.as_deref().and_then(|host| c_text(host.as_bytes())),
            flag: now.remote,
            ..BtEvent::new(kind::DIRECTORY, pane)
        });
    }
}

impl Host for CHost {
    fn title_changed(&self, pane: u64) {
        self.plain(kind::TITLE, pane);
        self.place(pane);
    }
    fn shell_exited(&self, pane: u64) {
        let exit = MainThreadMarker::new()
            .and_then(|mtm| embed::pane(mtm, pane))
            .and_then(|pane| pane.exit_status());
        self.send(BtEvent {
            exit,
            ..BtEvent::new(kind::SHELL_EXITED, pane)
        });
    }
    fn focused(&self, pane: u64) {
        self.plain(kind::FOCUSED, pane);
    }
    fn uploads_changed(&self, pane: u64) {
        self.plain(kind::UPLOADS, pane);
    }
    fn activity_changed(&self, pane: u64) {
        self.commands(pane);
        self.programs(pane);
        self.plain(kind::ACTIVITY, pane);
    }
    fn ports_changed(&self, pane: u64) {
        self.plain(kind::PORTS, pane);
    }
    fn open_link(&self, pane: u64, request: &LinkRequest) -> bool {
        let base = BtEvent::new(kind::OPEN_LINK, pane);
        let event = match request {
            LinkRequest::Url(url) => BtEvent {
                text: c_text(url.as_bytes()),
                number: i64::from(link::URL),
                ..base
            },
            LinkRequest::Path {
                path,
                directory,
                line,
                col,
            } => BtEvent {
                text: c_text(path.as_os_str().as_bytes()),
                number: i64::from(if *directory {
                    link::DIRECTORY
                } else {
                    link::FILE
                }),
                line: *line,
                column: *col,
                ..base
            },
            LinkRequest::RemotePath {
                path,
                directory,
                line,
                col,
            } => BtEvent {
                text: c_text(path.as_bytes()),
                detail: Self::session(pane)
                    .and_then(|session| session.remote_target())
                    .and_then(|(_, target, _)| c_text(target.host.as_bytes())),
                number: i64::from(if *directory {
                    link::DIRECTORY
                } else {
                    link::FILE
                }),
                flag: true,
                line: *line,
                column: *col,
                ..base
            },
        };
        let handled = |event: &BtEvent| event.handled.get();
        self.send_and(event, handled)
    }
    fn notify(&self, pane: u64, title: &str, body: &str) {
        self.send(BtEvent {
            text: c_text(title.as_bytes()),
            detail: c_text(body.as_bytes()),
            ..BtEvent::new(kind::NOTIFY, pane)
        });
    }
    fn post_notices(&self, pane: u64, source: Source, messages: Vec<String>) {
        self.send(BtEvent {
            text: c_text(messages.join("\n").as_bytes()),
            number: source_number(source),
            ..BtEvent::new(kind::NOTICES, pane)
        });
    }
    fn files_dragged(&self, pane: u64, over: bool) {
        self.send(BtEvent {
            flag: over,
            ..BtEvent::new(kind::FILES_DRAGGED, pane)
        });
    }
    fn carry_press(&self, pane: u64, at: (f64, f64)) {
        self.send(BtEvent {
            x: at.0,
            y: at.1,
            ..BtEvent::new(kind::CARRY_PRESS, pane)
        });
    }
    fn questions_changed(&self, pane: u64) {
        self.plain(kind::QUESTIONS, pane);
    }
    /// A pane's questions sit on its window: a C host has no cover of its own to give yet.
    fn cover(&self, _pane: &TerminalPane) -> Option<Rc<dyn Cover>> {
        None
    }
}

#[cfg(test)]
mod tests;
