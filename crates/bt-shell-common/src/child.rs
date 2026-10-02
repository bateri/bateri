//! The shell's starting conditions: in which directory and with which locale it opens.
//!
//! A bundle opened from the Dock gets `cwd=/` and launchd's environment from LaunchServices;
//! launchd's user domain has no `LANG` (`launchctl getenv LANG` is empty). Had the shell
//! inherited as is, it would start in `/` and without UTF-8. `cargo run` shows neither: it
//! inherits the caller's directory and environment.
//!
//! The policy lives **here**, not in `bt-core`: "home directory" and "which locale" are the
//! application's decision, `bt-core` only passes what it is given on to the child
//! (`SessionOptions`). Both go **only to the child** — our own process's directory and
//! environment never change (no `set_current_dir`, `set_var` or `setlocale`; `CLAUDE.md` →
//! `tty::setup_env()` çağrılmaz). alacritty does the same two jobs in its own process; that is
//! why it is not followed.
//!
//! Which shell runs ([`shell`]) and where the shell integration script lives
//! ([`zsh_wrapper_dir`]) are here too: both belong to the shell's birth and both must be
//! answered **before the spawn**.
//!
//! The decisions are in pure functions ([`home_directory`], [`decide_locale`], [`is_zsh`]):
//! the only places where the system is read are a few thin wrappers, the rest is tested.

use std::ffi::{CStr, OsString};
use std::path::{Path, PathBuf};

pub use crate::jobs::ShellParent;

/// The shell's starting directory: the user's home directory — on **every** launch, `cargo
/// run` included (same as alacritty and Terminal.app). "Home directory only if `/` comes in"
/// was rejected: the rule would have two branches (`discussion.md` → Karar 6 eki).
///
/// The source is `std::env::home_dir`: `HOME`, or if that is missing the user's passwd entry
/// (`getpwuid_r`) — the order alacritty follows when writing `HOME` for the child
/// (`ShellUser::from_env`), so in the ordinary case the shell's `pwd` and its `$HOME` are the
/// same directory. It needs no new dependency. The only edge where the two can diverge is a
/// non-UTF-8 `HOME`: `std` takes it as is, alacritty (`env::var`) falls back to passwd.
pub fn working_directory() -> Option<PathBuf> {
    home()
}

/// The user's home directory, with **the same resolution** as [`working_directory`]: the
/// settings directory (`~/.config/bateri/`) is derived from here too, so the shell's `$HOME`
/// and the home the settings are read from do not diverge through two separate rules.
pub fn home() -> Option<PathBuf> {
    home_directory(std::env::home_dir())
}

/// Home directory → starting directory; `None` **if it is not absolute**, i.e. the child
/// inherits our directory.
///
/// Two edges in a single condition: `HOME=""` does not fall back to passwd, `std` gives
/// `Some("")`; a relative `HOME`, on the other hand, would be resolved in `chdir` relative to
/// **our** directory (`/` from the Dock). There is no point in giving either to `chdir`;
/// inheriting is the honest choice.
fn home_directory(home: Option<PathBuf>) -> Option<PathBuf> {
    home.filter(|home| home.is_absolute())
}

/// The locale variable to add to the shell, if needed: from the system's language/region
/// setting when the environment has no locale.
///
/// `system` is the system's `(language, region)` pair, read by the platform shell — on macOS
/// from `NSLocale` (`bt-shell-macos`'s `locale::system_locale`, where the rationale for which
/// `NSLocale` answer is read lives); this crate sees no Foundation (043 Karar 2). The
/// decision itself is [`decide_locale`].
pub fn locale_env(system: Option<(String, String)>) -> Option<(String, String)> {
    decide_locale(|name| std::env::var_os(name), system, locale_installed)
}

/// The locale decision: `env` reads the environment, `system` is the system's `(language,
/// region)` code, `installed` is the question "is a locale with this name installed".
///
/// 1. If **one** of `LC_ALL`, `LC_CTYPE` or `LANG` is defined with a non-empty value →
///    nothing; the user's environment is not touched (`cargo run` takes this path). An empty
///    value counts as undefined: POSIX says so too. The value's validity is not questioned —
///    `LANG=C` is left alone as well.
/// 2. Otherwise, if `{language}_{region}.UTF-8` is installed → `LANG` is that name.
/// 3. Otherwise (no region, or e.g. English language + Turkey region → there is no
///    `en_TR.UTF-8`) → `LANG=en_US.UTF-8`: messages in English; date, number format and
///    collation are `en_US`'s, but UTF-8 input works.
///
/// **`LANG`, not `LC_ALL`**, in both writing branches: the weakest variable (what
/// Terminal.app does), so the shell's rc file can write its own `LC_*` on top. alacritty
/// writes `LC_ALL`, and that overrides every `LC_*` in the rc.
///
/// **The fallback is not `LC_CTYPE=UTF-8`** — we deliberately depart from alacritty's and
/// iTerm2's fallback. `UTF-8` is a valid `LC_CTYPE` on macOS but not a locale name on Linux,
/// and macOS's `ssh_config` carries `LANG` and `LC_*` to the remote machine (`SendEnv LANG
/// LC_*`): the tools there would print a `setlocale` warning and fall back to `C`.
/// `en_US.UTF-8` is a locale name on Linux too and is installed on most servers; where it is
/// not (e.g. an image carrying only `C.UTF-8`) the warning still appears — the cost for every
/// terminal that sets `LANG=en_US.UTF-8`. Moreover `LC_CTYPE` is stronger than `LANG`: it
/// would also lock the character class of a user who changes only `LANG` in their rc. User
/// decision (`discussion.md` → Karar 6 eki, son madde).
///
/// Whether `en_US.UTF-8` is installed is **not asked**: on macOS that locale ships with the
/// system (`/usr/share/locale` is on the read-only system volume). A last resort of "if that
/// is missing too, `LC_CTYPE=UTF-8`" would be a branch that never runs. The two share the
/// same character class file anyway: `en_US.UTF-8/LC_CTYPE` → `../C.UTF-8/LC_CTYPE`, which is
/// the same inode as `UTF-8/LC_CTYPE`.
///
/// **On Linux** there is no system pair (`system` is `None`): the session's environment
/// normally carries `LANG`, and without it the same `en_US.UTF-8` fallback applies.
fn decide_locale(
    env: impl Fn(&str) -> Option<OsString>,
    system: Option<(String, String)>,
    installed: impl Fn(&str) -> bool,
) -> Option<(String, String)> {
    let defined = ["LC_ALL", "LC_CTYPE", "LANG"]
        .into_iter()
        .any(|name| env(name).is_some_and(|value| !value.is_empty()));
    if defined {
        return None;
    }
    let name = system
        .map(|(language, region)| format!("{language}_{region}.UTF-8"))
        .filter(|name| installed(name))
        .unwrap_or_else(|| "en_US.UTF-8".to_owned());
    Some(("LANG".to_owned(), name))
}

/// Whether the locale is installed: does the `/usr/share/locale/{name}` directory exist.
///
/// macOS's layout. On Linux `/usr/share/locale` holds message catalogues, not locales
/// (those are compiled into `locale-archive`), so the answer would be wrong there — but the
/// branch does not run on Linux: without a system pair [`decide_locale`] never asks.
///
/// **Not tested** with `setlocale`: that changes our own process's global locale. A name
/// carrying `/` is rejected, so it cannot escape the directory.
fn locale_installed(name: &str) -> bool {
    !name.contains('/') && Path::new("/usr/share/locale").join(name).is_dir()
}

/// The path of the shell that will run as the child — **the same** resolution alacritty does
/// inside `tty::new`, but **before** the spawn.
///
/// A second resolution is born and that is deliberate: shell integration wants the answer to
/// "is this zsh" while `SessionOptions` is being built, whereas alacritty answers the same
/// question inside `tty::new`, when we can no longer intervene. The precedent is [`home`] in
/// the same file: there too the policy is ours and the resolution is in **parity** with
/// alacritty's order.
///
/// **Parity:** `$SHELL`, otherwise `pw_shell` from the user's passwd entry
/// (`ShellUser::from_env`). On two edges we behave **the same** as alacritty, because both
/// arise from the same `env::var` call: a non-UTF-8 `$SHELL` falls back to passwd, while an
/// empty `SHELL=""` is taken as is — which for us means "not zsh" and for alacritty a program
/// that cannot be executed. The only place we diverge is passwd being **unreadable**:
/// alacritty does not open the session at all there, we merely do not install the
/// integration. It is still alacritty that chooses the shell; we only compute the same answer
/// in advance.
///
/// On a Dock launch the second half of this path is **mandatory**: launchd's environment has
/// no `SHELL` (`launchctl getenv SHELL` is empty), so a resolution that only looked at
/// `$SHELL` would disable the integration precisely in the shipped bundle. The `LANG` form of
/// the same trap is in [`decide_locale`]'s doc.
pub fn shell() -> Option<PathBuf> {
    std::env::var("SHELL")
        .ok()
        .or_else(passwd_shell)
        .map(PathBuf::from)
}

/// The shell in the user's passwd entry (`pw_shell`); `None` if it cannot be read.
fn passwd_shell() -> Option<String> {
    passwd_field(|entry| entry.pw_shell)
}

/// The name in the user's passwd entry (`pw_name`); `None` if it cannot be read.
#[cfg(target_os = "macos")]
fn passwd_name() -> Option<String> {
    passwd_field(|entry| entry.pw_name)
}

/// A **single** field from the passwd entry; `None` if it cannot be read.
///
/// `getpwuid_r`, not `getpwuid`: the latter returns a static buffer shared process-wide, and
/// another thread's call refreshes it. The buffer is 1024 bytes, the same as alacritty's
/// `ShellUser::from_env`; an entry that does not fit fails with `ERANGE`.
///
/// The caller picks the field so that the `unsafe` reasoning stays in **one** place: two
/// copies would have meant two blocks, each calling its own `getpwuid_r`.
fn passwd_field(pick: impl Fn(&libc::passwd) -> *mut std::ffi::c_char) -> Option<String> {
    let mut buf = [0; 1024];
    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut found: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: `entry` and `found` are valid writable slots living in this frame; `buf` is
    // `buf.len()` bytes. `getpwuid_r` writes the entry into `entry`, the strings into `buf`,
    // and sets `found` to `entry` or to null.
    let status = unsafe {
        libc::getpwuid_r(
            libc::getuid(),
            &mut entry,
            buf.as_mut_ptr(),
            buf.len(),
            &mut found,
        )
    };
    if status != 0 || found.is_null() {
        return None;
    }
    let field = pick(&entry);
    if field.is_null() {
        return None;
    }
    // SAFETY: `found` is not null, so `entry` was filled and the chosen field points to a
    // NUL-terminated string inside `buf`. The slice is read while `buf` is alive and is copied
    // into an owned `String` right away.
    let value = unsafe { CStr::from_ptr(field) };
    // A non-UTF-8 value is `None`: the other side of the boundary (`SessionOptions`) wants a
    // `String`, and the fallbacks are already in place.
    value.to_str().ok().map(str::to_owned)
}

/// The command that spawns an untimed session's shell and the shell's position
/// relative to the PTY child — **from one call**, because the process table
/// (`jobs::foreground`) reads the command's shape through the parent: a Linux
/// shell recorded as `Login` would make the shell's first child count as the
/// shell.
///
/// - **macOS:** [`login_command`] + [`ShellParent::Login`]; an unresolvable
///   user or shell gives `None`, and alacritty's own macOS path is `login(1)`
///   too, so the parent stays `Login`.
/// - **Linux:** `$SHELL -l` (passwd if `$SHELL` is missing) +
///   [`ShellParent::Direct`]. **Not** alacritty parity — 0.26.0 spawns the
///   shell without arguments outside macOS — but the same startup-file chain as
///   the login session on macOS (043 Karar 1). An unresolvable shell gives
///   `None`, and alacritty's own Linux path spawns the shell directly, so the
///   parent stays `Direct`.
pub fn shell_command() -> (Option<(String, Vec<String>)>, ShellParent) {
    #[cfg(target_os = "macos")]
    {
        (login_command(), ShellParent::Login)
    }
    #[cfg(not(target_os = "macos"))]
    {
        (login_shell_command(shell()), ShellParent::Direct)
    }
}

/// The **pure** half of [`shell_command`]'s Linux branch: the resolved shell
/// with `-l`; `None` if there is no shell or its path is not UTF-8 (the other
/// side of the boundary wants a `String`).
#[cfg(not(target_os = "macos"))]
fn login_shell_command(shell: Option<PathBuf>) -> Option<(String, Vec<String>)> {
    Some((shell?.to_str()?.to_owned(), vec!["-l".to_owned()]))
}

/// The command that spawns the shell on macOS — the **`-q`** counterpart of alacritty's
/// `default_shell_command`; `None` → alacritty's own path.
///
/// The only difference is `-q` and that is its only purpose: `login(1)` prints the `Last
/// login: …` banner in every session and that line sits on the grid's first row. In bateri the
/// prompt belongs to the terminal and the grid to the commands; a system line landing there at
/// launch is a block nobody wrote.
///
/// **Why we do not write `~/.hushlogin`:** writing to files in the user's home directory is
/// forbidden in this repo (`make audit`), and leaving a permanent trace on the user's
/// machine to silence a banner goes far beyond what a terminal may ask for its own window.
/// alacritty's reason for adding `-q` conditionally is precisely to **look for** that file;
/// we remove the condition, not the mechanism.
///
/// Everything else is **parity** and deliberate: the `-flp` flags, the `exec -a` that makes
/// argv[0] `-zsh`, and the `/bin/zsh` that runs that `exec -a` (alacritty's note: `sh` has no
/// `exec -a`). The policy is ours, the resolution in parity — the same pattern as [`home`]
/// and [`shell`].
///
/// **An unresolvable user or shell falls back to `None`** and the session opens through
/// alacritty's own path: the banner comes back, the window works. The opposite direction —
/// building the command halfway and handing it over anyway — meant a terminal that does not
/// open.
#[cfg(target_os = "macos")]
fn login_command() -> Option<(String, Vec<String>)> {
    login_command_from(shell(), std::env::var("USER").ok().or_else(passwd_name))
}

/// The **pure** half of [`login_command`]: the command from resolved inputs.
///
/// A separate function, because this is the testable part — the other's answer depends on
/// the test process's `$USER` and `$SHELL`, and those two cannot be injected. The fallback
/// has **two** branches (user and shell) and both are tested here: the `?` chain gets them
/// right, but untested correctness could silently get lost in the next edit.
#[cfg(target_os = "macos")]
fn login_command_from(
    shell: Option<PathBuf>,
    user: Option<String>,
) -> Option<(String, Vec<String>)> {
    let shell = shell?;
    Some(login_argv(shell.to_str()?, &user?))
}

/// argv from the resolved user and shell.
#[cfg(target_os = "macos")]
fn login_argv(shell: &str, user: &str) -> (String, Vec<String>) {
    // `rsplit` always yields at least one piece; with an empty `$SHELL` that piece is empty
    // too, and a session opened with `exec -a -` was broken in alacritty as well.
    let name = shell.rsplit('/').next().unwrap_or(shell);
    (
        "/usr/bin/login".to_owned(),
        vec![
            "-qflp".to_owned(),
            user.to_owned(),
            "/bin/zsh".to_owned(),
            "-fc".to_owned(),
            format!("exec -a -{name} {shell}"),
        ],
    )
}

/// Whether the shell is zsh: the last component of the path is exactly `zsh`.
///
/// The **name** is asked, not the path: Homebrew's `/opt/homebrew/bin/zsh` and the system's
/// `/bin/zsh` are the same shell. A name like `zsh-5.9` is not recognized — zsh is not
/// installed under that name, and installing the wrapper for a shell we do not recognize
/// would mean a session that opens with files it cannot load.
pub fn is_zsh(shell: &Path) -> bool {
    shell.file_name().is_some_and(|name| name == "zsh")
}

/// The zsh wrapper's directory: `Contents/Resources/shell/zsh` in the bundle, the repo's
/// `assets/shell/zsh` in a debug build.
///
/// Both branches are verified by **the presence of the body** (`bateri.zsh`): a directory
/// name alone is no proof, and a `ZDOTDIR` set up with a missing script would leave the
/// user's entire configuration unloaded.
///
/// The repo branch exists **only in debug**, because of `cargo run`: development runs
/// unbundled, and a resolution that only looked at the bundle would disable the feature on
/// the path we run most (009 Karar 4). In release that branch is not compiled at all — the
/// shipped binary falling back to a path on a development machine would tie the product to
/// that machine.
///
/// **Known limit (Linux):** there is no bundle, so a Linux release build finds no script and
/// the integration is not installed; where a Linux package puts the script is the packaging
/// set's question (043 plan → Kapsam Dışı). The repo branch works on both platforms.
pub fn zsh_wrapper_dir() -> Option<PathBuf> {
    bundle_shell_dir()
        .and_then(wrapper_dir)
        .or_else(repo_wrapper_dir)
}

/// The repo's `assets/shell/zsh` — **exists only in a debug build**.
///
/// `#[cfg]`, not `cfg!` (`/code-review`, 009 gate): the latter is a runtime `bool`, so the
/// development machine's absolute path embedded via `env!("CARGO_MANIFEST_DIR")` went through
/// type checking and code generation in the release binary too; what kept it out of the
/// product was not a language guarantee but LLVM's dead code elimination. The sentence "in
/// release that branch is not compiled at all" in the doc above is true only with this
/// distinction.
#[cfg(debug_assertions)]
fn repo_wrapper_dir() -> Option<PathBuf> {
    wrapper_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/shell"))
}

#[cfg(not(debug_assertions))]
fn repo_wrapper_dir() -> Option<PathBuf> {
    None
}

/// `{shell}/zsh`, only if the body is a readable file.
fn wrapper_dir(shell: PathBuf) -> Option<PathBuf> {
    let dir = shell.join("zsh");
    dir.join("bateri.zsh").is_file().then_some(dir)
}

/// The bundle's `Contents/Resources/shell`: `…/bateri.app/Contents/MacOS/bateri` → two
/// parent directories up → `Resources/shell`.
///
/// Whether it is a bundle is **not asked**; the caller's body check already gives the answer,
/// and there is no such file two levels above `target/debug/bateri`.
#[cfg(target_os = "macos")]
fn bundle_shell_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let contents = exe.parent()?.parent()?;
    Some(contents.join("Resources/shell"))
}

/// No bundle outside macOS; where a Linux release finds the script is the
/// packaging set's question (043 plan → Kapsam Dışı).
#[cfg(not(target_os = "macos"))]
fn bundle_shell_dir() -> Option<PathBuf> {
    None
}

/// The language subtag of a BCP 47 language tag: `tr-TR` → `tr`, `zh-Hans-CN` → `zh`.
/// `preferredLanguages` uses `-`; `_` is for old locale names.
pub fn primary_language(tag: &str) -> Option<&str> {
    tag.split(['-', '_'])
        .next()
        .filter(|language| !language.is_empty())
}

/// The tests' `Wake`: does nothing.
///
/// No need to request frames — the only thing asked is `shell_state()`, and that is a
/// separate query that does not touch the `Term` lock (`Session::shell_state`'s doc). At
/// module level, because the process table's real PTY test (`jobs`) also spawns sessions
/// (precedent: [`crate::settings::TempRoot`]).
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Default)]
pub struct SilentWake;

#[cfg(any(test, feature = "test-support"))]
impl bt_core::Wake for SilentWake {
    fn wake(&self) {}
    fn child_exit(&self, _code: Option<i32>) {}
    fn copy_to_clipboard(&self, _text: String) {}
    fn title_changed(&self) {}
    fn search_changed(&self) {}
    // The tests do not probe for a remote session (036); the timed run's `ShellWake` does not
    // probe either (the `timed` branch).
    fn command_started(&self) {}
    fn link_hover_lost(&self) {}
}

/// Waits until `ready` says true; if time runs out, fails with `message`.
#[cfg(any(test, feature = "test-support"))]
pub fn wait_until(message: &str, ready: impl Fn() -> bool) {
    use std::time::{Duration, Instant};
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if ready() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("{message}");
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::HashMap;
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    use std::sync::Arc;

    use bt_core::{
        Blocks, CaretShape, CursorBlink, DockState, DockStatus, Osc52, ScrollGlide, Session,
        SessionOptions, ShellPhase, ShellState, TerminalOptions, Theme,
    };

    use super::*;
    use crate::settings::TempRoot;

    /// A reader that reads the environment from a fixed list; anything not in the list is
    /// undefined.
    fn env_of(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let vars: HashMap<String, OsString> = vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), OsString::from(v)))
            .collect();
        move |name| vars.get(name).cloned()
    }

    fn system(language: &str, region: &str) -> Option<(String, String)> {
        Some((language.to_owned(), region.to_owned()))
    }

    fn pair(key: &str, value: &str) -> Option<(String, String)> {
        Some((key.to_owned(), value.to_owned()))
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_login_command_always_silences_the_banner() {
        // The whole change is in this single letter: without `-q`, `login(1)` prints `Last
        // login: …` in every session and that line stays on the grid's first row. alacritty
        // adds the same flag only **if** `~/.hushlogin` exists; we removed the condition,
        // because the alternative was writing a file into the user's home directory, and that
        // is forbidden in this repo.
        let (program, args) = login_argv("/bin/zsh", "someone");
        assert_eq!(program, "/usr/bin/login");
        assert_eq!(args[0], "-qflp", "banner not silenced");

        // The rest is **parity** and that is the test's second half: the `exec -a` that makes
        // argv[0] `-zsh`, the `/bin/zsh` that runs it (`sh` has no `exec -a`) and the user
        // name in the order alacritty writes it.
        assert_eq!(args[1], "someone");
        assert_eq!(args[2], "/bin/zsh");
        assert_eq!(args[3], "-fc");
        assert_eq!(args[4], "exec -a -zsh /bin/zsh");

        // The shell's **name** is the last component of the path: Homebrew's zsh is the same
        // shell and argv[0] must still be `-zsh`, otherwise the login shell would not count
        // itself as a login shell.
        let (_, args) = login_argv("/opt/homebrew/bin/zsh", "someone");
        assert_eq!(args[4], "exec -a -zsh /opt/homebrew/bin/zsh");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn an_unresolved_user_or_shell_falls_back_to_the_default_command() {
        // **The direction of the fallback:** building the command halfway and handing it over
        // anyway meant a terminal that does not open. `None` brings back alacritty's own path
        // — the banner returns but the window works, and that trade-off is the right way.
        assert!(login_command_from(Some("/bin/zsh".into()), Some("someone".into())).is_some());
        assert!(login_command_from(None, Some("someone".into())).is_none());
        assert!(login_command_from(Some("/bin/zsh".into()), None).is_none());
        // A non-UTF-8 shell path takes the same branch: the other side of the boundary wants a
        // `String`, and guessing would mean spawning the wrong shell.
        let raw = PathBuf::from(OsString::from_vec(vec![0x2f, 0x62, 0xff]));
        assert!(login_command_from(Some(raw), Some("someone".into())).is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_shell_command_on_macos_is_login_with_a_login_parent() {
        // The pin on the macOS branch: the test process has `$USER` and `$SHELL` (or a
        // passwd entry), so the command resolves, goes through `login(1)`, and comes with
        // the parent the process table needs for it.
        let (command, parent) = shell_command();
        assert_eq!(parent, ShellParent::Login);
        let (program, args) = command.expect("login command did not resolve");
        assert_eq!(program, "/usr/bin/login");
        assert_eq!(args[0], "-qflp");
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn the_shell_command_on_linux_is_a_direct_login_shell() {
        let (command, parent) = shell_command();
        assert_eq!(parent, ShellParent::Direct);
        let (program, args) = command.expect("shell did not resolve");
        assert!(
            program.starts_with('/'),
            "shell path is not absolute: {program}"
        );
        assert_eq!(args, vec!["-l".to_owned()]);
        // The pure half: an unresolved or non-UTF-8 shell falls back to alacritty's path.
        assert_eq!(
            login_shell_command(Some("/bin/zsh".into())),
            Some(("/bin/zsh".to_owned(), vec!["-l".to_owned()]))
        );
        assert_eq!(login_shell_command(None), None);
        let raw = PathBuf::from(OsString::from_vec(vec![0x2f, 0x62, 0xff]));
        assert_eq!(login_shell_command(Some(raw)), None);
    }

    #[test]
    fn home_directory_is_home_unless_missing_or_empty() {
        assert_eq!(
            home_directory(Some("/Users/someone".into())),
            Some(PathBuf::from("/Users/someone"))
        );
        assert_eq!(home_directory(Some(PathBuf::new())), None);
        assert_eq!(home_directory(Some("relative/home".into())), None);
        assert_eq!(home_directory(None), None);
    }

    #[test]
    fn primary_language_is_the_first_subtag() {
        assert_eq!(primary_language("tr-TR"), Some("tr"));
        assert_eq!(primary_language("zh-Hans-CN"), Some("zh"));
        assert_eq!(primary_language("en"), Some("en"));
        assert_eq!(primary_language("pt_BR"), Some("pt"));
        assert_eq!(primary_language(""), None);
        assert_eq!(primary_language("-TR"), None);
    }

    #[test]
    fn locale_in_env_is_left_alone() {
        // **One** of the three is enough; which one does not matter. An installed system
        // locale is even given: the decision must not look at it at all.
        for name in ["LC_ALL", "LC_CTYPE", "LANG"] {
            let decided = decide_locale(env_of(&[(name, "C")]), system("tr", "TR"), |_| true);
            assert_eq!(decided, None, "locale added while {name} was defined");
        }
    }

    #[test]
    fn empty_locale_var_counts_as_unset() {
        // In POSIX an empty `LC_ALL`/`LC_CTYPE`/`LANG` is the same as undefined: the shell
        // ignores them and falls back to `C`.
        let env = env_of(&[("LC_ALL", ""), ("LC_CTYPE", ""), ("LANG", "")]);
        let decided = decide_locale(env, system("tr", "TR"), |_| true);
        assert_eq!(decided, pair("LANG", "tr_TR.UTF-8"));
    }

    #[test]
    fn installed_system_locale_becomes_lang() {
        let decided = decide_locale(env_of(&[]), system("tr", "TR"), |name| {
            name == "tr_TR.UTF-8"
        });
        assert_eq!(decided, pair("LANG", "tr_TR.UTF-8"));
    }

    #[test]
    fn missing_system_locale_falls_back_to_en_us_lang() {
        // This machine's situation: English language + Turkey region, no `en_TR.UTF-8`
        // (`ls /usr/share/locale`).
        let decided = decide_locale(env_of(&[]), system("en", "TR"), |name| {
            name == "tr_TR.UTF-8"
        });
        assert_eq!(decided, pair("LANG", "en_US.UTF-8"));
    }

    #[test]
    fn locale_without_region_falls_back_to_en_us_lang() {
        // `{language}_{country}` cannot be built from a region-less locale (`en`). A query that
        // says "installed" for every name shows this branch does not go through the
        // existence query.
        let decided = decide_locale(env_of(&[]), None, |_| true);
        assert_eq!(decided, pair("LANG", "en_US.UTF-8"));
    }

    #[test]
    fn zsh_is_recognized_by_name_not_by_path() {
        // Homebrew's zsh and the system's zsh are the same shell: the name is asked, not the
        // path.
        assert!(is_zsh(Path::new("/bin/zsh")));
        assert!(is_zsh(Path::new("/opt/homebrew/bin/zsh")));
        assert!(is_zsh(Path::new("zsh")));
        // Installing the wrapper for a shell we do not recognize would mean a session that
        // opens with files it cannot load.
        assert!(!is_zsh(Path::new("/bin/bash")));
        assert!(!is_zsh(Path::new("/usr/local/bin/fish")));
        assert!(!is_zsh(Path::new("/usr/local/bin/zsh-5.9")));
        // `SHELL=""`: alacritty takes it as is and cannot execute it, for us it means "not
        // zsh" (parity, [`shell`]'s doc).
        assert!(!is_zsh(Path::new("")));
        // A trailing slash does not change the name (`Path::file_name`) and this is not
        // questioned: `/bin/zsh/` is a directory, so exec fails and the session never opens —
        // whether the integration was installed stops mattering.
        assert!(is_zsh(Path::new("/bin/zsh/")));
    }

    #[test]
    fn this_user_has_a_resolvable_shell() {
        // Both halves of the resolution are real: `$SHELL` is defined in the test process,
        // and the passwd entry must be readable too. The latter is the only path on a Dock
        // launch (`launchctl getenv SHELL` is empty) and **only** this test guards it — since
        // `$SHELL` always takes precedence, the defect would stay silent.
        assert!(passwd_shell().is_some_and(|shell| shell.starts_with('/')));
        assert!(shell().is_some_and(|shell| shell.is_absolute()));
    }

    #[test]
    fn the_zsh_wrapper_ships_with_the_crate() {
        // Tests run in a debug build, so this tests the repo branch: is `assets/shell/zsh` in
        // place and is its body readable. The directory name alone is not enough — a
        // `ZDOTDIR` without a body would leave the user's entire configuration unloaded.
        let dir = zsh_wrapper_dir().expect("wrapper not found in the repo branch");
        assert!(dir.ends_with("zsh"));
        // All four of zsh's startup files are in place. `.zlogout` is deliberately absent:
        // `ZDOTDIR` is restored to the user's at `.zlogin` at the latest, so on exit zsh
        // already reads the user's own `.zlogout` (`bateri.zsh`'s header).
        for file in [
            ".zshenv",
            ".zprofile",
            ".zshrc",
            ".zlogin",
            "bateri.zsh",
            "zdotdir.zsh",
        ] {
            assert!(dir.join(file).is_file(), "{file} missing from the wrapper");
        }
        assert!(!dir.join(".zlogout").exists(), ".zlogout was not expected");
    }

    /// A copy of the wrapper taken outside the repo; this is what is given as `ZDOTDIR`,
    /// **not** the repo directory.
    ///
    /// Rationale (`/code-review`, 009 gate): `ZDOTDIR` points at us for a while during the
    /// session, and if the `HISTFILE` fix regresses zsh leaves a `.zsh_history` there. On the
    /// repo path this, beyond dirtying the working copy, would turn **another crate's** test
    /// (`zsh_wrapper_inventory_is_exactly_what_the_bundle_copies`, `bateri`) permanently red —
    /// in a separate test binary to boot, so the symptom would appear on a random run.
    fn copy_wrapper(into: &Path) -> PathBuf {
        let source = zsh_wrapper_dir().expect("wrapper not found");
        let wrapper = into.join("wrapper");
        std::fs::create_dir_all(&wrapper).expect("could not set up the wrapper copy");
        for file in [
            ".zshenv",
            ".zprofile",
            ".zshrc",
            ".zlogin",
            "bateri.zsh",
            "zdotdir.zsh",
        ] {
            std::fs::copy(source.join(file), wrapper.join(file))
                .unwrap_or_else(|e| panic!("could not copy {file}: {e}"));
        }
        wrapper
    }

    /// The text drawn in this frame, row by row — an inkless column is a space.
    ///
    /// Lining up the `Cell`s in order **would not be enough**: a space cell never reaches the
    /// sink, so `"$ ls"` and `"$ls"` would reduce to the same string and the "prompt was not
    /// drawn" claim would stay green in every case (the trap 012 phase-4 measured).
    pub(crate) fn screen(session: &Session, blocks: &mut Blocks) -> Vec<String> {
        let mut rows: Vec<Vec<char>> = Vec::new();
        session.frame(
            |cell| {
                let row = usize::from(cell.row);
                let col = usize::from(cell.col);
                if rows.len() <= row {
                    rows.resize(row + 1, Vec::new());
                }
                if rows[row].len() <= col {
                    rows[row].resize(col + 1, ' ');
                }
                rows[row][col] = cell.ch.unwrap_or(' ');
            },
            // What is asked is the text drawn **on the grid**; the fill is a separate channel
            // with fill-local rows, so if it were poured into the same buffer it would
            // overwrite the grid's first rows.
            |_| (),
            blocks,
            &mut bt_core::SelectionRuns::default(),
            &mut bt_core::SearchRuns::default(),
            &mut bt_core::Clusters::default(),
            ScrollGlide::default(),
            bt_core::DockBudget {
                share: 0.5,
                cols: 80,
            },
        );
        rows.into_iter()
            .map(|row| row.into_iter().collect())
            .collect()
    }

    #[test]
    fn the_terminal_takes_the_prompt_and_the_block_survives_it() {
        // **The guard of the set's most silent defect** (012 phase-5): a zero-width `PS1`
        // writes no cell, so had the anchor's close stayed at the end of `PS1`, the cell
        // carrying the anchor would **never be born** — both the block stripe and the
        // suppression of the input line derive from that cell, and both would silently die
        // together. On top of that `make check`, `make smoke` and `make bundle` would all three
        // stay green: the smoke run runs `/bin/sh`, the other tests print the anchor by hand.
        // It has no witness other than real zsh.
        //
        // Two claims in one round: the user's prompt is **not** on the grid, and the typed
        // command still gives birth to a block.
        let root = TempRoot::new("prompt-terminal");
        let home = root.0.join("home");
        std::fs::create_dir_all(&home).expect("could not set up the fake home directory");
        // The prompt is long and **unique**: a short `$ ` could appear on screen for other
        // reasons too, so the claim would fool itself.
        std::fs::write(home.join(".zshrc"), "PS1='ZSHPROMPTXY> '\nRPS1='RIGHTXY'\n")
            .expect(".zshrc not written");
        let wrapper = copy_wrapper(&root.0);

        let session = Session::spawn(
            SessionOptions {
                command: Some((
                    "/bin/zsh".to_owned(),
                    vec!["-l".to_owned(), "-i".to_owned()],
                )),
                working_directory: Some(home.clone()),
                home: Some(home.clone()),
                env: HashMap::from([
                    ("HOME".to_owned(), home.display().to_string()),
                    ("ZDOTDIR".to_owned(), wrapper.display().to_string()),
                ]),
                cols: 40,
                rows: 10,
                cell_px: (9, 18),
                terminal: TerminalOptions {
                    scrollback: 100,
                    osc52: Osc52::Off,
                    cursor: CaretShape::default(),
                    blink: CursorBlink::default(),
                },
                theme: Theme::BATERI,
                // Real zsh, real wrapper: in the app this session would get
                // a dock.
                dock: true,
                cluster: false,
                initial_input: None,
                shell_marks: false,
                tab_id: None,
                hostname: None,
            },
            Arc::new(SilentWake),
        )
        .expect("could not open the session");

        wait_until("prompt marks did not arrive", || {
            session.shell_state()
                == Some(ShellState {
                    phase: ShellPhase::Input,
                    last_exit: None,
                })
        });

        // **The moment of typing: does suppression still work under the new anchor format.**
        // R4.2 changed the wire — the anchor is open throughout `Input`, so **every** cell ZLE
        // writes carries the id. All of phase-4's unit guards build the anchor in the **old**
        // format that closes at the end of the prompt (`anchored_prompt`), so none of them can
        // see a regression specific to the new format: if suppression died while typing, the
        // grid and the dock would show the same line at once — the double image phase-4 came
        // to close — and all three gates would stay green. Its only witness is real zsh.
        session.write(b"true");
        wait_until("the mirror did not show the typed line", || {
            let mut mirror = DockState::default();
            session.dock_state(&mut mirror);
            mirror.status == DockStatus::Live && mirror.buffer == "true"
        });
        let typing = screen(&session, &mut Blocks::default()).join("\n");
        assert!(
            !typing.contains("true"),
            "the line being typed was drawn on the grid too (double image):\n{typing}"
        );

        // **The claims come AFTER the command runs, and this order is mandatory** — measured:
        // saying "not drawn" at an idle prompt is a claim that carries no weight, because
        // phase-4's suppression **already** hides the user's prompt (the range runs from the
        // anchor row to the cursor's row and the prompt is in that range). It stayed green
        // even under a regression that undid the handover, and it was timing-sensitive on top:
        // a frame taken before the mirror was `Live` would see the prompt and the claim would
        // turn red **at random**.
        //
        // The line of a command that has run is outside the suppression (suppression covers
        // only the block **being typed**), so the answer is definite: with the handover the
        // line is `true`, without it `ZSHPROMPTXY> true`.
        session.write(b"\n");
        wait_until("the command's exit code did not reach the state", || {
            session
                .shell_state()
                .is_some_and(|state| state.last_exit == Some(0))
        });

        let mut blocks = Blocks::default();
        let drawn = screen(&session, &mut blocks).join("\n");
        // `RPS1` is asked separately: resetting `PS1` and forgetting the right prompt would
        // leave a piece of the theme hanging on the right of the screen, and the `PS1` claim
        // would not see it.
        assert!(
            !drawn.contains("ZSHPROMPTXY"),
            "the user's prompt was drawn on the grid:\n{drawn}"
        );
        assert!(
            !drawn.contains("RIGHTXY"),
            "the user's right prompt was drawn on the grid:\n{drawn}"
        );
        // The command's line is there: the **screen really was drawn** half of the "prompt is
        // not visible" claim. Without it an empty grid would also pass the two claims above.
        assert!(drawn.contains("true"), "command line not drawn:\n{drawn}");
        // The mark of the block whose anchor closes in `preexec` is on the command's line.
        assert!(
            !blocks.as_slice().is_empty(),
            "no block was born with the zero-width prompt — no cell carries \
             the anchor. Is `anchor_close` in `preexec`?\ngrid:\n{drawn}"
        );
        session.shutdown();
    }

    /// **`👍🏽` at zsh's wrap edge** (035 phase-3, `discussion.md` → Karar, bedel 3): zsh
    /// counts the sequence as four columns with wcwidth, the clustered grid and the dock as
    /// two. At ten columns the prompt's two spaces + `abcdef` put `👍` in the last two
    /// columns — by zsh's count `🏽` is on the next row, on the grid in the same cell; at nine
    /// columns `👍` does not fit and wraps down. In both cases the suppression range must
    /// cover **all** rows of the input and must not spill into the output above: the input is
    /// not visible on the grid at all, `TOP3` is. It has no witness other than real zsh — the
    /// suppression's unit guards print the grid by hand, so they cannot see zsh's own
    /// arithmetic.
    #[test]
    fn a_clustered_emoji_at_the_wrap_edge_stays_suppressed() {
        for cols in [9, 10] {
            let root = TempRoot::new("cluster-wrap");
            let home = root.0.join("home");
            std::fs::create_dir_all(&home).expect("could not set up the fake home directory");
            std::fs::write(home.join(".zshrc"), "").expect(".zshrc not written");
            let wrapper = copy_wrapper(&root.0);
            let session = Session::spawn(
                SessionOptions {
                    command: Some((
                        "/bin/zsh".to_owned(),
                        vec!["-l".to_owned(), "-i".to_owned()],
                    )),
                    working_directory: Some(home.clone()),
                    home: Some(home.clone()),
                    env: HashMap::from([
                        ("HOME".to_owned(), home.display().to_string()),
                        ("ZDOTDIR".to_owned(), wrapper.display().to_string()),
                        // zsh counts a multibyte character as a single
                        // character only in a UTF-8 locale (installed in
                        // `make linux`'s image too: `tools/linux/Dockerfile`).
                        ("LANG".to_owned(), "en_US.UTF-8".to_owned()),
                    ]),
                    cols,
                    rows: 10,
                    cell_px: (9, 18),
                    terminal: TerminalOptions {
                        scrollback: 100,
                        osc52: Osc52::Off,
                        cursor: CaretShape::default(),
                        blink: CursorBlink::default(),
                    },
                    theme: Theme::BATERI,
                    dock: true,
                    cluster: true,
                    initial_input: None,
                    shell_marks: false,
                    tab_id: None,
                    hostname: None,
                },
                Arc::new(SilentWake),
            )
            .expect("could not open the session");
            session.write(b"echo TOP3\n");
            wait_until("the command did not finish", || {
                session.shell_state()
                    == Some(ShellState {
                        phase: ShellPhase::Input,
                        last_exit: Some(0),
                    })
            });
            // Two stops: while the line ends at `👍🏽` (by zsh's count `🏽` is on the next
            // row) and after plain letters follow it. This guard does not see the content
            // half of the freshness gate — while the mirror is the key's answer, the gate
            // says "fresh" from time (measured: green under a mutation that reads the mirror
            // without clusters); the guard of that half is
            // `the_clustered_last_ink_is_the_head_of_the_last_cluster`.
            let mut typed = String::new();
            for (piece, hidden) in [
                ("abcdef\u{1F44D}\u{1F3FD}", ["abc", "\u{1F44D}"]),
                ("xy", ["xy", "\u{1F44D}"]),
            ] {
                session.write(piece.as_bytes());
                typed.push_str(piece);
                wait_until("the mirror did not show the typed line", || {
                    let mut mirror = DockState::default();
                    session.dock_state(&mut mirror);
                    mirror.status == DockStatus::Live && mirror.buffer == typed
                });
                // One round for the grid to settle after the mirror too: zsh prints the
                // line before the mirror, so this is a safety margin.
                std::thread::sleep(std::time::Duration::from_millis(200));
                let drawn = screen(&session, &mut Blocks::default()).join("\n");
                assert!(
                    drawn.contains("TOP3"),
                    "suppression spilled into the output above ({cols} cols, {typed:?}):\n{drawn}"
                );
                for piece in hidden {
                    assert!(
                        !drawn.contains(piece),
                        "input drawn on the grid too ({cols} cols, {typed:?}, {piece:?}):\n{drawn}"
                    );
                }
            }
            session.shutdown();
        }
    }

    #[test]
    fn the_shell_keeps_the_prompt_when_the_user_asks_for_it() {
        // The other end of `integration = "blocks"`: once `BATERI_DOCK=off` lands in the
        // environment, the user's prompt stays **in place**. That is the tier's whole reason
        // for existing and its only witness is real zsh — `shell_integration_env` only sees
        // that the pair is sent, not that the script reads it.
        let root = TempRoot::new("prompt-shell");
        let home = root.0.join("home");
        std::fs::create_dir_all(&home).expect("could not set up the fake home directory");
        std::fs::write(home.join(".zshrc"), "PS1='ZSHPROMPTXY> '\n").expect(".zshrc not written");
        let wrapper = copy_wrapper(&root.0);

        let session = Session::spawn(
            SessionOptions {
                command: Some((
                    "/bin/zsh".to_owned(),
                    vec!["-l".to_owned(), "-i".to_owned()],
                )),
                working_directory: Some(home.clone()),
                home: Some(home.clone()),
                env: HashMap::from([
                    ("HOME".to_owned(), home.display().to_string()),
                    ("ZDOTDIR".to_owned(), wrapper.display().to_string()),
                    // The `blocks` tier's wire: no dock, so the script neither
                    // resets `PS1` nor sets up the mirror nor prints the branch.
                    ("BATERI_DOCK".to_owned(), "off".to_owned()),
                ]),
                cols: 40,
                rows: 10,
                cell_px: (9, 18),
                terminal: TerminalOptions {
                    scrollback: 100,
                    osc52: Osc52::Off,
                    cursor: CaretShape::default(),
                    blink: CursorBlink::default(),
                },
                theme: Theme::BATERI,
                // Real zsh, real wrapper: in the app this session would get
                // a dock.
                dock: true,
                cluster: false,
                initial_input: None,
                shell_marks: false,
                tab_id: None,
                hostname: None,
            },
            Arc::new(SilentWake),
        )
        .expect("could not open the session");

        wait_until("prompt marks did not arrive", || {
            session.shell_state()
                == Some(ShellState {
                    phase: ShellPhase::Input,
                    last_exit: None,
                })
        });
        // The ledger is fresh **every round**: `wait_until` wants an `Fn`, and in a test a
        // per-frame allocation costs nothing (the production rationale is in `Blocks`'s
        // doc).
        wait_until("the user's prompt was not drawn on the grid", || {
            screen(&session, &mut Blocks::default())
                .join("\n")
                .contains("ZSHPROMPTXY")
        });
        // **AND THE MIRROR WAS NEVER SET UP.** This tier has no dock, so no reader for the
        // mirror either; had the hooks been installed anyway, on every keystroke five
        // variables would be base64-encoded and written to the stream and nothing would be
        // drawn in return. The witness is `DockState`: the prompt was printed long ago (the
        // two waits above passed), so if the mirror were coming it would have come.
        //
        // The criterion is **not** being `Live`: if the channel never spoke, the state stays
        // as it was born. `Unavailable` is not accepted either — that would mean "there is a
        // mirror but we could not show it".
        let mut dock = DockState::default();
        session.dock_state(&mut dock);
        assert_eq!(
            dock.status,
            DockStatus::Idle,
            "mirror set up in the dockless tier: a per-key cost with no return"
        );
        session.shutdown();
    }

    #[test]
    fn the_zsh_wrapper_loads_the_users_files_and_reports_marks() {
        // The set's **real** test: a real zsh, a real PTY and a real user configuration. Four
        // claims at once in a single round:
        //
        // 1. The user's `.zshenv` was read **and** the `ZDOTDIR` assignment in it was read
        //    back — that is the most common way for a user to have a `ZDOTDIR`, and had we not
        //    read it we would look for the remaining files in the old directory.
        // 2. The user's `.zprofile` was read: the login shell's PATH is born there, and a
        //    wrapper that handed over only `.zshrc` would silently drop it.
        // 3. A **broken** `.zshrc` did not bring the shell down and the marks still arrived.
        // 4. `ZDOTDIR` was restored to the user's: `.zlogin` is now read from their directory,
        //    not ours, and the value it sees inside is its own directory.
        // 5. `HISTFILE` points to the user's directory. The system's `/etc/zshrc` is read
        //    **before** our files and sets it to `${ZDOTDIR:-$HOME}/.zsh_history`: if not
        //    fixed, the user's history is written into the app's bundle, their own file
        //    freezes and no warning appears anywhere. This is the only place the test sees it
        //    — the defect really happened in the first draft and left its trace as
        //    `assets/shell/zsh/.zsh_history`. Debian's system zshrc sets no history,
        //    so `make linux`'s image appends macOS's lines (`tools/linux/Dockerfile`).
        let root = TempRoot::new("shell-wrapper");
        let home = root.0.join("home");
        let cfg = home.join("cfg");
        std::fs::create_dir_all(&cfg).expect("could not set up the fake home directory");
        let write = |path: PathBuf, text: &str| {
            std::fs::write(&path, text)
                .unwrap_or_else(|e| panic!("could not write {}: {e}", path.display()))
        };
        write(
            home.join(".zshenv"),
            "export ZDOTDIR=$HOME/cfg\nexport SEEN_ZSHENV=1\n",
        );
        // R3.5's pin. `typeset` is **local** inside a function: if the user's file is
        // `source`d from a function, these two lines are erased on return and the symptom is
        // silent. The chosen idiom is not made up — Homebrew, asdf, pyenv and nvm set up PATH
        // exactly like this, so the defect meant those tools vanishing in bateri (009
        // phase-5, the measurement is in that file).
        write(
            cfg.join(".zprofile"),
            "export SEEN_ZPROFILE=1\n\
             typeset -U path\n\
             path+=(/opt/probe)\n\
             typeset -A probe_map=(k v)\n\
             export SEEN_ARGC=$#\n",
        );
        // Deliberately broken: a nonexistent command **and** a syntax error. Both abort the
        // `source` halfway; they must not bring the shell down.
        write(
            cfg.join(".zshrc"),
            "PS1='$ '\nbateri_missing_command\nif then fi\n",
        );
        // The index in `path` depends on the machine (the length of the inherited PATH), so
        // **presence** is printed: `> 0` is deterministic.
        write(
            cfg.join(".zlogin"),
            "print -r -- \"$ZDOTDIR $SEEN_ZSHENV $SEEN_ZPROFILE $HISTFILE \
             $((${path[(I)/opt/probe]} > 0)) ${${(t)probe_map}:-yok} $SEEN_ARGC\" \
             >| $HOME/zlogin\n",
        );

        let wrapper = copy_wrapper(&root.0);
        let session = Session::spawn(
            SessionOptions {
                // `-l -i`: the shape of our session (alacritty makes argv[0]
                // `-zsh` via `login`). Which of the five files are read depends on
                // this, so the chain the test checks is these two flags.
                command: Some((
                    "/bin/zsh".to_owned(),
                    vec!["-l".to_owned(), "-i".to_owned()],
                )),
                working_directory: Some(home.clone()),
                home: Some(home.clone()),
                env: HashMap::from([
                    ("HOME".to_owned(), home.display().to_string()),
                    ("ZDOTDIR".to_owned(), wrapper.display().to_string()),
                ]),
                cols: 40,
                rows: 10,
                cell_px: (9, 18),
                terminal: TerminalOptions {
                    scrollback: 100,
                    osc52: Osc52::Off,
                    cursor: CaretShape::default(),
                    blink: CursorBlink::default(),
                },
                theme: Theme::BATERI,
                // Real zsh, real wrapper: in the app this session would get
                // a dock.
                dock: true,
                cluster: false,
                initial_input: None,
                shell_marks: false,
                tab_id: None,
                hostname: None,
            },
            Arc::new(SilentWake),
        )
        .expect("could not open the session");

        // `A` then `B`: the prompt was drawn and finished. Getting here proves 1–3 at once —
        // the hook was loaded, the addition went into PS1 and the broken file did not bring
        // the shell down.
        wait_until("prompt marks did not arrive", || {
            session.shell_state()
                == Some(ShellState {
                    phase: ShellPhase::Input,
                    last_exit: None,
                })
        });

        // Run a command: `C` brings the running state, the next prompt's `D` the exit code.
        // `false` was chosen so the code is non-zero — `0` would be confused with "could not
        // read the code".
        session.write(b"false\n");
        wait_until("the command's exit code did not reach the state", || {
            session
                .shell_state()
                .is_some_and(|state| state.last_exit == Some(1))
        });

        session.write(b"exit\n");
        // 4 and 5: `.zlogin` was read **at startup**, not at exit (login shell), but the line
        // writing the file may not have reached the disk until the shell exits; instead of
        // waiting blindly we wait for the file to exist. Since `.zlogin` runs **after** the
        // restore, the `HISTFILE` it sees is the corrected one.
        let seen = home.join("zlogin");
        wait_until("the user's .zlogin was not read", || seen.is_file());
        let seen = std::fs::read_to_string(&seen).expect("could not read zlogin trace");
        // The last three fields are R3.5: the directory added with `typeset -U path` is in
        // `path`, the `typeset -A` array is still an association and the user's file sees no
        // positional parameters. All three fail in a file `source`d from inside a function.
        assert_eq!(
            seen.trim(),
            format!("{0} 1 1 {0}/.zsh_history 1 association 0", cfg.display()),
            "ZDOTDIR/HISTFILE not restored, the user's files were not loaded \
             or the file was not read in zsh's own context (`typeset` stayed \
             local / positional parameters leaked)"
        );
        // The defect's own trace: the history must not have been written into **our**
        // directory.
        //
        // WAITING for shutdown is mandatory (`/code-review`, 009 gate): zsh writes
        // `$HISTFILE` **at exit** (nothing in the chain sets `inc_append_history` or
        // `share_history`). A claim that looks microseconds after `exit` wins the race every
        // time and would stay green even if the defect regressed — the trap's guard would fall
        // into the trap itself.
        //
        // The synchronization point is **the event itself**: the history file appearing in the
        // user's directory. Two candidates were rejected — `reader_alive()` is already `false`
        // on return because `shutdown()` `take`s the reader, and `Teardown::Clean` is not
        // guaranteed here (measured: `Abandoned` arrives; a child stuck in its exit cannot hang
        // the shutdown, a recorded debt — `CLAUDE.md` → Kapanış). The awaited event is also a
        // **positive** claim: a negative claim alone could not tell "written to the right
        // place" from "never written", both leave the wrapper's directory empty.
        wait_until(
            "command history was not written to the user's directory",
            || cfg.join(".zsh_history").is_file(),
        );
        assert!(
            !wrapper.join(".zsh_history").exists(),
            "command history was written to the wrapper's directory"
        );
        session.shutdown();
    }

    #[test]
    fn locale_name_with_slash_is_not_installed() {
        // `/usr/share/locale/../../../usr` is a real directory (`/usr`): without the check it
        // would say "installed".
        assert!(!locale_installed("../../../usr"));
    }

    /// The wrapper's `ssh` function (048) in a real zsh: it asks `$BATERI_BIN
    /// ssh-argv` (`--tty` only when stdin and stdout are terminals), runs
    /// `command ssh` with the NUL-separated answer, takes `BATERI_BIN` out of the
    /// environment — and a user's own `ssh` function is left alone.
    #[test]
    fn the_wrappers_ssh_function_asks_the_binary() {
        let root = TempRoot::new("ssh-function");
        let home = root.0.join("home");
        let bin = root.0.join("bin");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::create_dir_all(&bin).expect("bin");
        let log = root.0.join("asked");
        let script = |path: PathBuf, body: String| {
            std::fs::write(&path, body).expect("script");
            std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o755))
                .expect("chmod");
        };
        // The stand-in binary records how it was asked and answers a wrapped argv.
        script(
            bin.join("bateri"),
            format!(
                "#!/bin/sh\necho \"$*\" >> '{}'\nprintf -- '-t\\0x\\0y z\\0BOOT\\0'\n",
                log.display()
            ),
        );
        // The stand-in ssh prints its arguments, one bracket each.
        script(
            bin.join("ssh"),
            "#!/bin/sh\nfor a in \"$@\"; do printf '[%s]' \"$a\"; done; echo\n".to_owned(),
        );
        let wrapper = copy_wrapper(&root.0);
        // The stand-ins first on `PATH`, after the system's login files
        // (macOS's `path_helper` rewrites it in `/etc/zprofile`).
        let spawn = |rc: &str| {
            std::fs::write(
                home.join(".zshrc"),
                format!("PATH={}:$PATH\n{rc}", bin.display()),
            )
            .expect(".zshrc");
            Session::spawn(
                SessionOptions {
                    command: Some((
                        "/bin/zsh".to_owned(),
                        vec!["-l".to_owned(), "-i".to_owned()],
                    )),
                    working_directory: Some(home.clone()),
                    home: Some(home.clone()),
                    env: HashMap::from([
                        ("HOME".to_owned(), home.display().to_string()),
                        ("ZDOTDIR".to_owned(), wrapper.display().to_string()),
                        ("BATERI_DOCK".to_owned(), "off".to_owned()),
                        (
                            "BATERI_BIN".to_owned(),
                            bin.join("bateri").display().to_string(),
                        ),
                    ]),
                    cols: 80,
                    rows: 20,
                    cell_px: (9, 18),
                    terminal: TerminalOptions {
                        scrollback: 100,
                        osc52: Osc52::Off,
                        cursor: CaretShape::default(),
                        blink: CursorBlink::default(),
                    },
                    theme: Theme::BATERI,
                    dock: false,
                    cluster: false,
                    initial_input: None,
                    shell_marks: false,
                    tab_id: None,
                    hostname: None,
                },
                Arc::new(SilentWake),
            )
            .expect("could not open the session")
        };
        let shown = |session: &Session, text: &str| {
            screen(session, &mut Blocks::default())
                .iter()
                .any(|row| row.contains(text))
        };

        let session = spawn("PS1='$ '\n");
        session.write(b"ssh x 'y z'; ssh x | cat; echo \"[${BATERI_BIN-unset}]\"\n");
        wait_until("the wrapped call did not run", || {
            shown(&session, "[-t][x][y z][BOOT]") && shown(&session, "[unset]")
        });
        let asked = std::fs::read_to_string(&log).expect("the binary was asked");
        assert_eq!(asked, "ssh-argv --tty -- x y z\nssh-argv -- x\n");
        session.shutdown();

        // A user's own `ssh` function wins: ours is not defined over it.
        let session = spawn("PS1='$ '\nssh() { echo USER-SSH }\n");
        session.write(b"ssh x\n");
        wait_until("the user's ssh did not run", || shown(&session, "USER-SSH"));
        session.shutdown();
    }

    /// **Does the command's duration reach the screen in real zsh** (013).
    ///
    /// All of `bt-core`'s counter tests print OSC 133 **by hand**; the real script's order
    /// (the anchor closes in `preexec`, `D` and the next `A` in the same `precmd`) is tried in
    /// none of them. The user said "the duration does not show when it finishes" and no gate
    /// turned red — this is its only witness.
    #[test]
    fn a_real_zsh_command_shows_its_duration() {
        let root = TempRoot::new("duration-terminal");
        let home = root.0.join("home");
        std::fs::create_dir_all(&home).expect("could not set up the fake home directory");
        // **A SECOND OSC 133 SOURCE** — an imitation of iTerm2's
        // `~/.iterm2_shell_integration.zsh`. Not fake but representative: it produces exactly
        // the sequence measured on the user's machine (id-less `C` and `D`, before ours). VS
        // Code and Ghostty print the same protocol too, so this setup is **common**.
        //
        // Testing with an empty `.zshrc` was testing a world most users do not live in: the
        // duration dropped to zero and no gate saw it.
        std::fs::write(
            home.join(".zshrc"),
            "autoload -Uz add-zsh-hook\n\
             foreign_preexec() { printf '\\033]133;C;\\007' }\n\
             foreign_precmd() { printf '\\033]133;D;%s\\007' \"$?\" }\n\
             add-zsh-hook preexec foreign_preexec\n\
             add-zsh-hook precmd foreign_precmd\n",
        )
        .expect(".zshrc not written");
        let wrapper = copy_wrapper(&root.0);

        let session = Session::spawn(
            SessionOptions {
                command: Some((
                    "/bin/zsh".to_owned(),
                    vec!["-l".to_owned(), "-i".to_owned()],
                )),
                working_directory: Some(home.clone()),
                home: Some(home.clone()),
                env: HashMap::from([
                    ("HOME".to_owned(), home.display().to_string()),
                    ("ZDOTDIR".to_owned(), wrapper.display().to_string()),
                ]),
                cols: 40,
                rows: 10,
                cell_px: (9, 18),
                terminal: TerminalOptions {
                    scrollback: 100,
                    osc52: Osc52::Off,
                    cursor: CaretShape::default(),
                    blink: CursorBlink::default(),
                },
                theme: Theme::BATERI,
                dock: true,
                cluster: false,
                initial_input: None,
                shell_marks: false,
                tab_id: None,
                hostname: None,
            },
            Arc::new(SilentWake),
        )
        .expect("could not open the session");

        // **Only the phase is asked, not `last_exit`:** the foreign source prints a `D;0`
        // already at the first prompt, so `last_exit` is filled at startup too. Tying the
        // precondition to it would never start the test in the real world.
        wait_until("prompt marks did not arrive", || {
            session
                .shell_state()
                .is_some_and(|state| state.phase == ShellPhase::Input)
        });

        // A command that **exceeds** the threshold: below one second no counter is born
        // anyway, and the test would mistake that for a defect.
        //
        // We do not wait for the end via `last_exit` (reason above): the criterion is directly
        // **the thing being looked for itself**, i.e. is the duration on screen. The counter
        // has a decimal in its finished value (`2.0s`); we search the whole screen, because
        // the line's position shifts with the bottom alignment.
        session.write(b"sleep 2\n");
        wait_until("finished command's duration not shown", || {
            let drawn = screen(&session, &mut Blocks::default()).join("\n");
            drawn.contains("2.0s") || drawn.contains("2.1s")
        });
        session.shutdown();
    }
}
