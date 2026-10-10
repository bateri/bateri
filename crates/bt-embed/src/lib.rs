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

use std::cell::Cell;
use std::ffi::{CStr, CString, OsStr, c_char, c_void};
use std::os::unix::ffi::OsStrExt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::rc::Rc;

use bt_core::{InitialInput, Settings, Theme};
use bt_shell_macos::embed::{self, Cover, Host, Identity, Source, TerminalPane};
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

/// The pane's owner on the C side: every event becomes a [`BtEvent`] handed to the host's handler,
/// until the host closes the pane.
struct CHost {
    handler: Option<BtEventHandler>,
    context: *mut c_void,
    /// The host closed the pane ([`bt_pane_close`]): its handle is gone, and so are its events.
    closed: Cell<bool>,
}

impl CHost {
    fn send(&self, event: BtEvent) {
        if self.closed.get() {
            return;
        }
        if let Some(handler) = self.handler {
            // SAFETY: the host's promise at `bt_pane_config_set_event_handler`: the handler takes
            // the context it gave and an event lent for the call.
            unsafe { handler(self.context, &event) };
        }
    }

    fn plain(&self, kind: u32, pane: u64) {
        self.send(BtEvent::new(kind, pane));
    }
}

impl Host for CHost {
    fn title_changed(&self, pane: u64) {
        self.plain(kind::TITLE, pane);
    }
    fn shell_exited(&self, pane: u64) {
        self.plain(kind::SHELL_EXITED, pane);
    }
    fn focused(&self, pane: u64) {
        self.plain(kind::FOCUSED, pane);
    }
    fn uploads_changed(&self, pane: u64) {
        self.plain(kind::UPLOADS, pane);
    }
    fn activity_changed(&self, pane: u64) {
        self.plain(kind::ACTIVITY, pane);
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
