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
//! environment never change (no `set_current_dir`, `set_var` or `setlocale`;
//! `tty::setup_env()` is never called). alacritty does the same two jobs in its own process; that is
//! why it is not followed.
//!
//! Which shell runs ([`shell`]) and where the shell integration script lives
//! ([`zsh_wrapper_dir`], [`fish_wrapper_dir`], [`bash_wrapper_dir`]) are here too: both belong to
//! the shell's birth and both must be answered **before the spawn** — for bash, which version it
//! is too ([`bash_version`]).
//!
//! The decisions are in pure functions ([`home_directory`], [`decide_locale`], [`is_zsh`]):
//! the only places where the system is read are a few thin wrappers, the rest is tested.

use std::ffi::{CStr, OsString};
use std::path::{Path, PathBuf};

pub use crate::jobs::ShellParent;

/// The shell's starting directory: the user's home directory — on **every** launch, `cargo
/// run` included (same as alacritty and Terminal.app). "Home directory only if `/` comes in"
/// was rejected: the rule would have two branches.
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
/// `NSLocale` answer is read lives); this crate sees no Foundation. The
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
/// decision.
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
///   the login session on macOS. An unresolvable shell gives
///   `None`, and alacritty's own Linux path spawns the shell directly, so the
///   parent stays `Direct`.
///
/// `shell_args` are the arguments the shell integration needs the shell started with
/// (bash's `--posix`; empty for every other case): they follow the shell's own name, before the
/// `-l` on Linux (bash takes its long options first) and inside the `exec` on macOS.
pub fn shell_command(shell_args: &[String]) -> (Option<(String, Vec<String>)>, ShellParent) {
    #[cfg(target_os = "macos")]
    {
        (login_command(shell_args), ShellParent::Login)
    }
    #[cfg(not(target_os = "macos"))]
    {
        (
            login_shell_command(shell(), shell_args),
            ShellParent::Direct,
        )
    }
}

/// The **pure** half of [`shell_command`]'s Linux branch: the resolved shell
/// with `shell_args` and `-l`, in that order — bash only recognizes `--posix` before the
/// single-character options; `None` if there is no shell or its path is not UTF-8 (the other
/// side of the boundary wants a `String`).
#[cfg(not(target_os = "macos"))]
fn login_shell_command(
    shell: Option<PathBuf>,
    shell_args: &[String],
) -> Option<(String, Vec<String>)> {
    let mut args = shell_args.to_vec();
    args.push("-l".to_owned());
    Some((shell?.to_str()?.to_owned(), args))
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
fn login_command(shell_args: &[String]) -> Option<(String, Vec<String>)> {
    login_command_from(
        shell(),
        std::env::var("USER").ok().or_else(passwd_name),
        shell_args,
    )
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
    shell_args: &[String],
) -> Option<(String, Vec<String>)> {
    let shell = shell?;
    Some(login_argv(shell.to_str()?, &user?, shell_args))
}

/// argv from the resolved user and shell.
///
/// `shell_args` ride inside the `exec` string, because the string goes to `/bin/zsh -fc` and
/// that is the only place the shell's own arguments can be written: `login(1)` takes a command,
/// not an argv. They are quoted (`quote::command_line`) for the same reason.
#[cfg(target_os = "macos")]
fn login_argv(shell: &str, user: &str, shell_args: &[String]) -> (String, Vec<String>) {
    // `rsplit` always yields at least one piece; with an empty `$SHELL` that piece is empty
    // too, and a session opened with `exec -a -` was broken in alacritty as well.
    let name = shell.rsplit('/').next().unwrap_or(shell);
    let tail = if shell_args.is_empty() {
        String::new()
    } else {
        format!(" {}", crate::quote::command_line(shell_args))
    };
    (
        "/usr/bin/login".to_owned(),
        vec![
            "-qflp".to_owned(),
            user.to_owned(),
            "/bin/zsh".to_owned(),
            "-fc".to_owned(),
            format!("exec -a -{name} {shell}{tail}"),
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

/// Whether the shell is fish: the last component of the path is exactly `fish`, for the
/// reasons [`is_zsh`] gives for zsh.
pub fn is_fish(shell: &Path) -> bool {
    shell.file_name().is_some_and(|name| name == "fish")
}

/// Whether the shell is bash: the last component of the path is exactly `bash`, for the
/// reasons [`is_zsh`] gives for zsh. (`sh` is not bash even where it is a link to it: it
/// starts in POSIX mode by its own name, which is not the start the script is written for.)
pub fn is_bash(shell: &Path) -> bool {
    shell.file_name().is_some_and(|name| name == "bash")
}

/// A shell bateri has an integration script for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellKind {
    Zsh,
    Fish,
    Bash,
}

impl ShellKind {
    /// The kind of shell at `path`; `None` for one without a script (the terminal then works as
    /// it always did, only the marks do not arrive).
    pub fn of(shell: &Path) -> Option<Self> {
        if is_zsh(shell) {
            Some(Self::Zsh)
        } else if is_fish(shell) {
            Some(Self::Fish)
        } else if is_bash(shell) {
            Some(Self::Bash)
        } else {
            None
        }
    }
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
/// the path we run most. In release that branch is not compiled at all — the
/// shipped binary falling back to a path on a development machine would tie the product to
/// that machine.
///
/// **Known limit (Linux):** there is no bundle, so a Linux release build finds no script and
/// the integration is not installed; where a Linux package puts the script is the packaging
/// set's question (out of scope here). The repo branch works on both platforms.
pub fn zsh_wrapper_dir() -> Option<PathBuf> {
    script_dir("zsh", "bateri.zsh")
}

/// The fish wrapper's directory: `Contents/Resources/shell/fish` in the bundle, the repo's
/// `assets/shell/fish` in a debug build — resolved like [`zsh_wrapper_dir`], and verified by the
/// presence of the body (`vendor_conf.d/bateri.fish`).
///
/// What fish is pointed at is this directory's **parent**: fish reads
/// `<entry of XDG_DATA_DIRS>/fish/vendor_conf.d`, so the entry that makes it find the body is the
/// directory that holds `fish/` ([`fish_data_dir`]).
pub fn fish_wrapper_dir() -> Option<PathBuf> {
    script_dir("fish", "vendor_conf.d/bateri.fish")
}

/// The bash wrapper's directory: `Contents/Resources/shell/bash` in the bundle, the repo's
/// `assets/shell/bash` in a debug build — resolved like [`zsh_wrapper_dir`], and verified by the
/// presence of the body (`bateri.bash`), the file `ENV` is pointed at.
pub fn bash_wrapper_dir() -> Option<PathBuf> {
    script_dir("bash", "bateri.bash")
}

/// A bash version as `BASH_VERSINFO` reports it: major and minor.
///
/// Ordered by major and then minor, so `>=` asks "this version or newer".
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct BashVersion {
    pub major: u32,
    pub minor: u32,
}

impl BashVersion {
    /// The oldest bash the integration runs on: 4.4 added `PS0`, the only hook between reading
    /// a command and running it that is not the user's `DEBUG` trap. Below it a block would
    /// open at the prompt and never close, so the integration is not installed at all (macOS's
    /// own `/bin/bash` is 3.2).
    pub const MIN_INTEGRATION: Self = Self { major: 4, minor: 4 };

    pub const fn new(major: u32, minor: u32) -> Self {
        Self { major, minor }
    }

    /// Whether the integration runs on this version.
    pub fn integrates(self) -> bool {
        self >= Self::MIN_INTEGRATION
    }
}

/// How long [`bash_version`] waits for the shell to answer. A feel threshold, not a measured
/// number: bash answers in a few milliseconds, and the wait happens on the thread that opens
/// the pane, once per path, so a bash that hangs costs the user this long and no more.
const BASH_QUERY_LIMIT: std::time::Duration = std::time::Duration::from_secs(1);

/// The version of the bash at `bash`, asked of the shell itself — once per path for the life of
/// the process (a failure is remembered too: a bash that did not answer once is not asked again
/// at every new pane).
///
/// The version has to be known **before** the shell starts: the dock's rows are reserved when
/// the session is born and are not given back if the shell never prints a mark
/// (`grid::dock_rows_for`), and below 4.4 the shell never will. Guessing from the path was
/// rejected (`/bin/bash` is 3.2 on a stock Mac, but a hand-installed old bash would be
/// misjudged), and so was branching inside the shell like the remote bootstrap does: it starts
/// the shell right, but the parent could not know the dock share.
///
/// `None`: the shell did not answer, answered with something that is not a version, or ran
/// out of time — the integration is not installed (the terminal works as it did before).
pub fn bash_version(bash: &Path) -> Option<BashVersion> {
    remembered_bash_version(bash, BASH_QUERY_LIMIT)
}

/// [`bash_version`]'s memory with the question's time limit as a parameter, so a test of
/// the memory does not depend on how fast this machine runs a program.
fn remembered_bash_version(bash: &Path, limit: std::time::Duration) -> Option<BashVersion> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock, PoisonError};
    static ASKED: OnceLock<Mutex<HashMap<PathBuf, Option<BashVersion>>>> = OnceLock::new();
    let asked = ASKED.get_or_init(Mutex::default);
    if let Some(known) = asked
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(bash)
    {
        return *known;
    }
    let answer = query_bash_version(bash, limit);
    asked
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(bash.to_owned(), answer);
    answer
}

/// The uncached question of [`bash_version`]: `bash -c` prints `BASH_VERSINFO`'s first two
/// elements. `BASH_ENV` is taken out of the environment — a non-interactive bash runs the file
/// it names before `-c`, and the user's file has no business answering (or printing) here —
/// and the shell gets no stdin and no stderr. A shell that outlives `limit` is killed.
fn query_bash_version(bash: &Path, limit: std::time::Duration) -> Option<BashVersion> {
    use std::io::Read as _;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    let mut child = Command::new(bash)
        .args([
            "-c",
            r#"printf '%s.%s' "${BASH_VERSINFO[0]}" "${BASH_VERSINFO[1]}""#,
        ])
        .env_remove("BASH_ENV")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + limit;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(2)),
            Ok(Some(_)) => return None,
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut text = String::new();
    child.stdout.take()?.read_to_string(&mut text).ok()?;
    parse_bash_version(&text)
}

/// `5.3` → 5.3; anything that is not two dot-separated numbers is `None`.
fn parse_bash_version(text: &str) -> Option<BashVersion> {
    let (major, minor) = text.trim().split_once('.')?;
    Some(BashVersion::new(major.parse().ok()?, minor.parse().ok()?))
}

/// The `XDG_DATA_DIRS` entry that makes fish read the wrapper in `wrapper` (the directory
/// [`fish_wrapper_dir`] returns): its parent.
pub fn fish_data_dir(wrapper: &Path) -> Option<&Path> {
    wrapper
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
}

/// `{shell}/{name}` from the bundle or, in a debug build, the repo.
fn script_dir(name: &str, body: &str) -> Option<PathBuf> {
    bundle_shell_dir()
        .and_then(|shell| wrapper_dir(shell, name, body))
        .or_else(|| repo_wrapper_dir(name, body))
}

/// The repo's `assets/shell/{name}` — **exists only in a debug build**.
///
/// `#[cfg]`, not `cfg!` (found in code review): the latter is a runtime `bool`, so the
/// development machine's absolute path embedded via `env!("CARGO_MANIFEST_DIR")` went through
/// type checking and code generation in the release binary too; what kept it out of the
/// product was not a language guarantee but LLVM's dead code elimination. The sentence "in
/// release that branch is not compiled at all" in the doc above is true only with this
/// distinction.
#[cfg(debug_assertions)]
fn repo_wrapper_dir(name: &str, body: &str) -> Option<PathBuf> {
    wrapper_dir(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/shell"),
        name,
        body,
    )
}

#[cfg(not(debug_assertions))]
fn repo_wrapper_dir(_name: &str, _body: &str) -> Option<PathBuf> {
    None
}

/// `{shell}/{name}`, only if `body` is a readable file in it.
fn wrapper_dir(shell: PathBuf, name: &str, body: &str) -> Option<PathBuf> {
    let dir = shell.join(name);
    dir.join(body).is_file().then_some(dir)
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
/// packaging set's question (out of scope here).
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
    // The tests do not probe for a remote session; the timed run's `ShellWake` does not
    // probe either (the `timed` branch).
    fn command_started(&self) {}
    fn remote_up(&self) {}
    fn remote_typed(&self) {}
    fn link_hover_lost(&self) {}
    fn pointer_changed(&self) {}
    fn phase_edge(&self) {}
    fn remote_command_edge(&self) {}
    fn mirror_changed(&self) {}
    fn environment_changed(&self) {}
    fn blocks_changed(&self) {}
    fn unseen_changed(&self) {}
    fn program_status_changed(&self) {}

    fn bell(&self) {}

    fn notification(&self) {}
}

/// Waits until `ready` says true; if time runs out, fails with `message`.
#[cfg(any(test, feature = "test-support"))]
pub fn wait_until(message: &str, mut ready: impl FnMut() -> bool) {
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
        Blocks, CaretShape, CellHalf, Click, CursorBlink, DockState, DockStatus, MouseModifiers,
        Osc52, ScrollGlide, SelectionPoint, Session, SessionOptions, ShellPhase, ShellState,
        TerminalOptions, Theme,
    };

    use super::*;
    use crate::settings::TempRoot;

    /// Writes `body` (a `#!/bin/sh` script) to `path` as a program (mode 755) that a test is
    /// about to run, ready to run at once.
    ///
    /// **Through `cp`, not from this process.** The tests fork from many threads at once, and
    /// a child forked while this process held the file open for writing keeps that handle
    /// until it execs; Linux refuses to run a file anyone holds open for writing (`ETXTBSY`).
    /// Written by a `cp` that has exited, the file has no writer to hand on (measured in the
    /// Linux image with four threads forking: 86 refusals in 3000 runs written in-process,
    /// none through `cp`).
    ///
    /// **Run once here, doing nothing.** macOS checks a new file the first time it runs —
    /// every new file, a copy or a rewrite included — before the program starts (measured:
    /// a tenth of a second to a second and a half on an idle machine, up to three seconds
    /// while this crate's tests run; the second run takes milliseconds). Paid inside a test's
    /// wait, two fresh programs used up most of a ten-second one. The line after the shebang
    /// ends the warm-up run before the body, so the body's side effects (the logs the tests
    /// read) count only the test's own runs.
    pub(crate) fn write_program(path: &Path, body: &str) {
        let rest = body
            .strip_prefix("#!/bin/sh\n")
            .expect("a stand-in program is a `#!/bin/sh` script");
        let body = format!("#!/bin/sh\n[ -n \"${{BT_TEST_WARM_UP-}}\" ] && exit 0\n{rest}");
        let name = path.file_name().expect("a file name").to_string_lossy();
        let source = path.with_file_name(format!(".{name}.source"));
        std::fs::write(&source, body).expect("the program's source");
        let copied = std::process::Command::new("cp")
            .arg(&source)
            .arg(path)
            .status()
            .expect("cp");
        assert!(copied.success(), "cp {}", path.display());
        std::fs::remove_file(&source).expect("the program's source");
        std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .expect("chmod");
        let warmed = std::process::Command::new(path)
            .env("BT_TEST_WARM_UP", "1")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("the warm-up run");
        assert!(warmed.success(), "the warm-up run of {}", path.display());
    }

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
        let (program, args) = login_argv("/bin/zsh", "someone", &[]);
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
        let (_, args) = login_argv("/opt/homebrew/bin/zsh", "someone", &[]);
        assert_eq!(args[4], "exec -a -zsh /opt/homebrew/bin/zsh");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn an_unresolved_user_or_shell_falls_back_to_the_default_command() {
        // **The direction of the fallback:** building the command halfway and handing it over
        // anyway meant a terminal that does not open. `None` brings back alacritty's own path
        // — the banner returns but the window works, and that trade-off is the right way.
        assert!(login_command_from(Some("/bin/zsh".into()), Some("someone".into()), &[]).is_some());
        assert!(login_command_from(None, Some("someone".into()), &[]).is_none());
        assert!(login_command_from(Some("/bin/zsh".into()), None, &[]).is_none());
        // A non-UTF-8 shell path takes the same branch: the other side of the boundary wants a
        // `String`, and guessing would mean spawning the wrong shell.
        let raw = PathBuf::from(OsString::from_vec(vec![0x2f, 0x62, 0xff]));
        assert!(login_command_from(Some(raw), Some("someone".into()), &[]).is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_shell_command_on_macos_is_login_with_a_login_parent() {
        // The pin on the macOS branch: the test process has `$USER` and `$SHELL` (or a
        // passwd entry), so the command resolves, goes through `login(1)`, and comes with
        // the parent the process table needs for it.
        let (command, parent) = shell_command(&[]);
        assert_eq!(parent, ShellParent::Login);
        let (program, args) = command.expect("login command did not resolve");
        assert_eq!(program, "/usr/bin/login");
        assert_eq!(args[0], "-qflp");
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn the_shell_command_on_linux_is_a_direct_login_shell() {
        let (command, parent) = shell_command(&[]);
        assert_eq!(parent, ShellParent::Direct);
        let (program, args) = command.expect("shell did not resolve");
        assert!(
            program.starts_with('/'),
            "shell path is not absolute: {program}"
        );
        assert_eq!(args, vec!["-l".to_owned()]);
        // The pure half: an unresolved or non-UTF-8 shell falls back to alacritty's path.
        assert_eq!(
            login_shell_command(Some("/bin/zsh".into()), &[]),
            Some(("/bin/zsh".to_owned(), vec!["-l".to_owned()]))
        );
        assert_eq!(login_shell_command(None, &[]), None);
        let raw = PathBuf::from(OsString::from_vec(vec![0x2f, 0x62, 0xff]));
        assert_eq!(login_shell_command(Some(raw), &[]), None);
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
    fn fish_is_recognized_by_name_not_by_path() {
        assert!(is_fish(Path::new("/opt/homebrew/bin/fish")));
        assert!(is_fish(Path::new("fish")));
        assert!(!is_fish(Path::new("/bin/zsh")));
        assert!(!is_fish(Path::new("/usr/local/bin/fish-3")));
        assert!(!is_fish(Path::new("")));
        assert_eq!(
            ShellKind::of(Path::new("/opt/homebrew/bin/fish")),
            Some(ShellKind::Fish)
        );
        assert_eq!(ShellKind::of(Path::new("/bin/zsh")), Some(ShellKind::Zsh));
        assert_eq!(ShellKind::of(Path::new("/usr/bin/sh")), None);
    }

    #[test]
    fn bash_is_recognized_by_name_not_by_path() {
        assert!(is_bash(Path::new("/opt/homebrew/bin/bash")));
        assert!(is_bash(Path::new("/bin/bash")));
        assert!(is_bash(Path::new("bash")));
        // `sh` starts in POSIX mode by its own name, whatever it links to; a versioned name is
        // not how bash is installed.
        assert!(!is_bash(Path::new("/bin/sh")));
        assert!(!is_bash(Path::new("/usr/local/bin/bash-5.2")));
        assert!(!is_bash(Path::new("/bin/zsh")));
        assert!(!is_bash(Path::new("")));
        assert_eq!(
            ShellKind::of(Path::new("/opt/homebrew/bin/bash")),
            Some(ShellKind::Bash)
        );
    }

    #[test]
    fn the_bash_wrapper_ships_with_the_crate() {
        // The repo branch (tests run in a debug build): the file `ENV` is pointed at is there.
        let dir = bash_wrapper_dir().expect("bash wrapper not found in the repo branch");
        assert!(dir.ends_with("bash"));
        assert!(dir.join("bateri.bash").is_file());
    }

    #[test]
    fn a_bash_version_is_two_numbers_and_ordered() {
        assert_eq!(parse_bash_version("5.3"), Some(BashVersion::new(5, 3)));
        assert_eq!(parse_bash_version("3.2\n"), Some(BashVersion::new(3, 2)));
        assert_eq!(parse_bash_version("4.10"), Some(BashVersion::new(4, 10)));
        for junk in ["", "5", "5.", ".3", "five.three", "5.3.1", "5,3", "-1.2"] {
            assert_eq!(parse_bash_version(junk), None, "{junk:?}");
        }
        // 4.10 is newer than 4.4: the comparison is by number, not by text.
        assert!(BashVersion::new(4, 10) > BashVersion::new(4, 4));
        assert!(BashVersion::new(5, 0) > BashVersion::new(4, 99));
        // `PS0` came in 4.4.
        assert!(!BashVersion::new(3, 2).integrates());
        assert!(!BashVersion::new(4, 3).integrates());
        assert!(BashVersion::new(4, 4).integrates());
        assert!(BashVersion::new(5, 3).integrates());
    }

    /// A stand-in "bash" in `dir`: an executable script with `body`.
    fn stand_in(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        write_program(&path, &format!("#!/bin/sh\n{body}"));
        path
    }

    #[test]
    fn the_version_is_what_the_shell_answers_in_time() {
        let root = TempRoot::new("bash-version");
        let limit = std::time::Duration::from_secs(5);
        let answer = |body: &str| {
            let path = stand_in(&root.0, "bash", body);
            query_bash_version(&path, limit)
        };
        assert_eq!(answer("printf 5.3"), Some(BashVersion::new(5, 3)));
        assert_eq!(answer("printf 3.2"), Some(BashVersion::new(3, 2)));
        // Not a version, a failure, and a shell that prints and then fails are all "no answer".
        assert_eq!(answer("printf hello"), None);
        assert_eq!(answer("exit 1"), None);
        assert_eq!(answer("printf 5.3; exit 1"), None);
        assert_eq!(answer(""), None);
        assert_eq!(
            query_bash_version(&root.0.join("missing"), limit),
            None,
            "a path with no program"
        );
        // A shell that does not answer is killed when the limit passes; the pane is not kept
        // waiting for it.
        let hang = stand_in(&root.0, "hang", "exec sleep 30");
        let started = std::time::Instant::now();
        assert_eq!(
            query_bash_version(&hang, std::time::Duration::from_millis(200)),
            None
        );
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "the question outlived its limit: {:?}",
            started.elapsed()
        );
    }

    /// The memory, not the time limit: the stand-ins here are asked with a limit far above
    /// [`BASH_QUERY_LIMIT`], which a busy test run can stretch a shell's answer past.
    #[test]
    fn a_version_is_asked_once_per_path_failures_included() {
        let limit = std::time::Duration::from_secs(30);
        let root = TempRoot::new("bash-version-cache");
        let log = root.0.join("asked");
        for (name, body) in [("good", "printf 5.3"), ("bad", "exit 1")] {
            let path = stand_in(
                &root.0,
                name,
                &format!("echo asked >> '{}.{name}'\n{body}", log.display()),
            );
            let first = remembered_bash_version(&path, limit);
            let second = remembered_bash_version(&path, limit);
            assert_eq!(first, second, "{name}");
            assert_eq!(first.is_some(), name == "good", "{name}");
            let asked = std::fs::read_to_string(format!("{}.{name}", log.display()))
                .expect("the stand-in was run");
            assert_eq!(asked, "asked\n", "{name}: the shell was asked again");
        }
    }

    #[test]
    fn the_argv_carries_the_shells_own_arguments() {
        #[cfg(target_os = "macos")]
        {
            // After the shell's name inside the `exec` string (the only place `login(1)` lets
            // an argument go), quoted for `/bin/zsh -fc`.
            let args = ["--posix".to_owned()];
            let (_, argv) = login_argv("/opt/homebrew/bin/bash", "someone", &args);
            assert_eq!(argv[4], "exec -a -bash /opt/homebrew/bin/bash --posix");
            let args = ["--init-file".to_owned(), "a b; c".to_owned()];
            let (_, argv) = login_argv("/opt/homebrew/bin/bash", "someone", &args);
            assert_eq!(
                argv[4],
                "exec -a -bash /opt/homebrew/bin/bash --init-file a\\ b\\;\\ c"
            );
            // No arguments: the string is what it was, byte for byte.
            let (_, argv) = login_argv("/bin/zsh", "someone", &[]);
            assert_eq!(argv[4], "exec -a -zsh /bin/zsh");
        }
        #[cfg(not(target_os = "macos"))]
        {
            assert_eq!(
                login_shell_command(Some("/bin/bash".into()), &["--posix".to_owned()]),
                Some((
                    "/bin/bash".to_owned(),
                    vec!["--posix".to_owned(), "-l".to_owned()]
                ))
            );
        }
    }

    #[test]
    fn the_fish_wrapper_ships_with_the_crate() {
        // The repo branch (tests run in a debug build): the body is readable, and
        // the entry fish is pointed at is the directory that holds `fish/`.
        let dir = fish_wrapper_dir().expect("fish wrapper not found in the repo branch");
        assert!(dir.ends_with("fish"));
        assert!(dir.join("vendor_conf.d/bateri.fish").is_file());
        let entry = fish_data_dir(&dir).expect("no parent");
        assert!(entry.join("fish/vendor_conf.d/bateri.fish").is_file());
        assert_eq!(fish_data_dir(Path::new("fish")), None);
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
    /// Rationale (found in code review): `ZDOTDIR` points at us for a while during the
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
    /// drawn" claim would stay green in every case (a measured trap).
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
            &mut bt_core::TrackMarks::default(),
            &mut bt_core::Clusters::default(),
            &mut bt_core::Pictures::default(),
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
        // **The guard of the set's most silent defect**: a zero-width `PS1`
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
                pane_uuid: None,
                hostname: None,
                replay: None,
                journal: None,
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
        // The wire changed — the anchor is open throughout `Input`, so **every** cell ZLE
        // writes carries the id. All of the suppression's unit guards build the anchor in the **old**
        // format that closes at the end of the prompt (`anchored_prompt`), so none of them can
        // see a regression specific to the new format: if suppression died while typing, the
        // grid and the dock would show the same line at once — the double image suppression came
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
        // the suppression **already** hides the user's prompt (the range runs from the
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

    /// **`👍🏽` at zsh's wrap edge** (a known cost of clustering): zsh
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
                    pane_uuid: None,
                    hostname: None,
                    replay: None,
                    journal: None,
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
                pane_uuid: None,
                hostname: None,
                replay: None,
                journal: None,
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
        // The pin. `typeset` is **local** inside a function: if the user's file is
        // `source`d from a function, these two lines are erased on return and the symptom is
        // silent. The chosen idiom is not made up — Homebrew, asdf, pyenv and nvm set up PATH
        // exactly like this, so the defect meant those tools vanishing in bateri
        // (measured).
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
                pane_uuid: None,
                hostname: None,
                replay: None,
                journal: None,
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
        // The last three fields are the pin: the directory added with `typeset -U path` is in
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
        // WAITING for shutdown is mandatory (found in code review): zsh writes
        // `$HISTFILE` **at exit** (nothing in the chain sets `inc_append_history` or
        // `share_history`). A claim that looks microseconds after `exit` wins the race every
        // time and would stay green even if the defect regressed — the trap's guard would fall
        // into the trap itself.
        //
        // The synchronization point is **the event itself**: the history file appearing in the
        // user's directory. Two candidates were rejected — `reader_alive()` is already `false`
        // on return because `shutdown()` `take`s the reader, and `Teardown::Clean` is not
        // guaranteed here (measured: `Abandoned` arrives; a child stuck in its exit cannot hang
        // the shutdown, a recorded debt). The awaited event is also a
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

    /// The wrapper's `ssh` function in a real zsh: it asks `$BATERI_BIN
    /// ssh-argv` (`--tty` only when stdin and stdout are terminals), runs
    /// `command ssh` with the NUL-separated answer, takes `BATERI_BIN` out of the
    /// environment — and a user's own `ssh` function is left alone. The
    /// fallback: after a wrapped `ssh` that did not end with 255 it
    /// asks `ssh-fell-back` with the **wrapped** arguments and reruns a
    /// non-empty answer, returning the last `ssh`'s code — 255 asks too
    /// (the binary decides); inside tmux or screen nothing is wrapped. The
    /// inherited remote folder (`BATERI_SSH_CWD`) goes to the first `ssh`
    /// only, and out of the environment.
    #[test]
    fn the_wrappers_ssh_function_asks_the_binary() {
        let root = TempRoot::new("ssh-function");
        let home = root.0.join("home");
        let bin = root.0.join("bin");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::create_dir_all(&bin).expect("bin");
        let log = root.0.join("asked");
        let script = |path: PathBuf, body: String| write_program(&path, &body);
        // The stand-in binary records how it was asked; `ssh-argv --tty`
        // answers a wrapped argv, `ssh-fell-back` a plain rerun while `fall`
        // exists.
        let fall = root.0.join("fall");
        script(
            bin.join("bateri"),
            format!(
                "#!/bin/sh\necho \"$*\" >> '{}'\ncase \"$1 $2\" in\n\
                 'ssh-argv --tty') printf -- '-t\\0x\\0y z\\0BOOT\\0' ;;\n\
                 ssh-fell-back*) [ -f '{}' ] && printf -- 'PLAIN\\0x\\0' ;;\nesac\nexit 0\n",
                log.display(),
                fall.display()
            ),
        );
        // The stand-in ssh prints its arguments, one bracket each, and exits
        // with the code in `code` (zero without it).
        let code = root.0.join("code");
        script(
            bin.join("ssh"),
            format!(
                "#!/bin/sh\nfor a in \"$@\"; do printf '[%s]' \"$a\"; done; echo\n\
                 [ -f '{0}' ] && exit \"$(cat '{0}')\"\nexit 0\n",
                code.display()
            ),
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
                        ("BATERI_SSH_INSTANCE".to_owned(), "0a1b2c3d".to_owned()),
                        ("BATERI_SSH_CWD".to_owned(), "/srv/my app".to_owned()),
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
                    pane_uuid: None,
                    hostname: None,
                    replay: None,
                    journal: None,
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
        session.write(
            b"ssh x 'y z'; ssh x | cat; echo \"[${BATERI_BIN-unset}${BATERI_SSH_INSTANCE-unset}${BATERI_SSH_CWD-unset}]\"\n",
        );
        wait_until("the wrapped call did not run", || {
            shown(&session, "[-t][x][y z][BOOT]") && shown(&session, "[unsetunsetunset]")
        });
        let asked = std::fs::read_to_string(&log).expect("the binary was asked");
        // `--block` is the command's own block: the first prompt's; the
        // instance is the masters' directory, out of the environment.
        // The wrapped call that ended with 0 asked the fallback with the
        // wrapped arguments (nothing came back: no rerun). The folder went
        // with the first call and not the second.
        assert_eq!(
            asked,
            "ssh-argv --tty --block 1 --instance 0a1b2c3d --cwd /srv/my app -- x y z\n\
             ssh-fell-back --rc 0 --instance 0a1b2c3d -- -t x y z BOOT\n\
             ssh-argv --block 1 --instance 0a1b2c3d -- x\n"
        );
        std::fs::remove_file(&log).expect("log");

        // A fallback answer is rerun plain; the code is the rerun's.
        std::fs::write(&fall, "").expect("fall");
        std::fs::write(&code, "7").expect("code");
        session.write(b"ssh x; echo \"rc=$?\"\n");
        wait_until("the plain rerun did not run", || {
            shown(&session, "[PLAIN][x]") && shown(&session, "rc=7")
        });
        // 255 asks too (the binary tells ssh's own error from a
        // refusal after the login) — here it answers nothing, so no rerun;
        // inside tmux nothing is wrapped.
        std::fs::remove_file(&fall).expect("fall");
        std::fs::write(&code, "255").expect("code");
        session.write(b"ssh x; echo \"r1=$?\"; TMUX=t ssh x; echo \"r2=$?\"\n");
        wait_until("the 255 and tmux calls did not run", || {
            shown(&session, "r1=255") && shown(&session, "r2=255")
        });
        let asked = std::fs::read_to_string(&log).expect("the binary was asked");
        assert_eq!(
            asked,
            "ssh-argv --tty --block 2 --instance 0a1b2c3d -- x\n\
             ssh-fell-back --rc 7 --instance 0a1b2c3d -- -t x y z BOOT\n\
             ssh-argv --tty --block 3 --instance 0a1b2c3d -- x\n\
             ssh-fell-back --rc 255 --instance 0a1b2c3d -- -t x y z BOOT\n"
        );
        session.shutdown();

        // A user's own `ssh` function wins: ours is not defined over it.
        let session = spawn("PS1='$ '\nssh() { echo USER-SSH }\n");
        session.write(b"ssh x\n");
        wait_until("the user's ssh did not run", || shown(&session, "USER-SSH"));
        session.shutdown();
    }

    /// The acceptance scenario end to end: the real wrapper, the real `bateri`
    /// binary (`ssh-argv`, `ssh-fell-back`; `target/debug/bateri`, built
    /// first) and a real password sshd in Docker on `127.0.0.1:2249` (the
    /// set's own container; skipped when it does not answer) with three users:
    /// `deneme` (oh-my-zsh), `kapi` (a login shell that runs no `-c`, Windows
    /// cmd's answer) and `router` (`ForceCommand` of an interactive CLI). The
    /// pane's half — the bootstrap's `up` → `mark_up` + `record_posix` — is
    /// played by the test thread polling `Session::remote_up` (the app's wake →
    /// main queue → thread latency is not covered here). `HOME` is a
    /// temporary directory, so the state file is too; every `ssh` carries
    /// `-F /dev/null`, no agent, no keys and a temporary `known_hosts`:
    /// `~/.ssh` is never read. Prints what it measured (`--nocapture`).
    #[test]
    #[ignore = "needs bateri's Docker sshd on 127.0.0.1:2249 and target/debug/bateri: cargo test -p bt-shell-common e2e_ -- --ignored --nocapture"]
    fn e2e_first_connection_and_the_silent_fallback() {
        use crate::ssh_wrap::{Fact, load};
        use std::time::{Duration, Instant};
        if std::net::TcpStream::connect(("127.0.0.1", 2249)).is_err() {
            eprintln!("SKIPPED: no sshd on 127.0.0.1:2249");
            return;
        }
        let binary = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/bateri");
        assert!(
            binary.is_file(),
            "build bateri first: cargo build -p bateri"
        );
        // A short root: the session socket must fit `sun_path`.
        let root = PathBuf::from(format!("/tmp/bt049-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let home = root.join("home");
        std::fs::create_dir_all(&home).expect("home");
        let state = home.join("Library/Application Support/bateri/remote-hosts");
        let instance = format!("{:08x}", std::process::id());
        // SAFETY: `getuid` has no preconditions and cannot fail.
        let uid = unsafe { libc::getuid() };
        let bases = crate::ssh_route::socket_bases(Some(&home), uid);
        crate::ssh_route::prepare_instance(&bases[0], &instance).expect("instance");
        let wrapper = copy_wrapper(&root);
        std::fs::write(home.join(".zshrc"), "PS1='$ '\n").expect(".zshrc");
        let tab = bt_core::PaneUuid::parse("0F1E2D3C-4B5A-6978-8796-A5B4C3D2E1F0").expect("tab");
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
                    ("BATERI_DOCK".to_owned(), "off".to_owned()),
                    ("BATERI_BIN".to_owned(), binary.display().to_string()),
                    ("BATERI_SSH_INSTANCE".to_owned(), instance.clone()),
                ]),
                cols: 250,
                rows: 50,
                cell_px: (9, 18),
                terminal: TerminalOptions {
                    scrollback: 2000,
                    osc52: Osc52::Off,
                    cursor: CaretShape::default(),
                    blink: CursorBlink::default(),
                },
                theme: Theme::BATERI,
                dock: false,
                cluster: false,
                initial_input: None,
                shell_marks: false,
                // The pane's identity: `BATERI_TAB_URL` and the `LC_` trio.
                pane_uuid: Some(tab.clone()),
                hostname: None,
                replay: None,
                journal: None,
            },
            Arc::new(SilentWake),
        )
        .expect("could not open the session");
        let known = root.join("known_hosts").display().to_string();
        let ssh_args = |user: &str, port: u16| -> Vec<String> {
            [
                "-F",
                "/dev/null",
                "-o",
                &format!("UserKnownHostsFile={known}"),
                "-o",
                "StrictHostKeyChecking=accept-new",
                "-o",
                "IdentityAgent=none",
                "-o",
                "PubkeyAuthentication=no",
                "-p",
                &port.to_string(),
                &format!("{user}@127.0.0.1"),
            ]
            .map(str::to_owned)
            .to_vec()
        };
        let line = |args: &[String]| format!("ssh {}\r", args.join(" "));
        let key = |args: &[String]| {
            let mut argv = vec!["ssh".to_owned(), "-G".to_owned()];
            argv.extend(args.iter().cloned());
            let out = std::process::Command::new(&argv[0])
                .args(&argv[1..])
                .output()
                .expect("ssh -G");
            crate::ssh_wrap::host_key(&String::from_utf8_lossy(&out.stdout)).expect("key")
        };
        // The pane's half: the remote probe (`set_remote`, the wrapped call's
        // nonce), the login probe and the first input after it
        // (`mark_used`), the `up` marked at once, then `posix`.
        let seen = std::cell::RefCell::new(None::<(u64, String)>);
        let probed = std::cell::RefCell::new(None::<(u64, Option<String>)>);
        let used = std::cell::RefCell::new(None::<u64>);
        let logged = std::cell::RefCell::new(None::<u64>);
        let pane = |user_args: &[String]| {
            if let Some(command) = session.running_command()
                && probed.borrow().as_ref().map(|(probed, _)| *probed) != Some(command)
                && let crate::jobs::Probe::Remote(target) = crate::jobs::remote(
                    ShellParent::Direct,
                    session.child_pid(),
                    &crate::jobs::SystemTable,
                )
            {
                let remote = bt_core::RemoteTarget {
                    host: target.host.clone(),
                    port: target.port,
                    kind: target.kind,
                    argv: target.argv.clone(),
                    line: target.argv.join(" "),
                };
                session.set_remote(command, Some(&remote));
                *probed.borrow_mut() = Some((command, target.nonce.clone()));
            }
            if let Some((command, nonce)) = probed.borrow().clone() {
                if crate::jobs::remote_login(&session) == Some(command)
                    && *logged.borrow() != Some(command)
                    && let Some(nonce) = &nonce
                {
                    crate::ssh_wrap::mark_login(&state, nonce).expect("login");
                    *logged.borrow_mut() = Some(command);
                }
                if session.remote_typed() == Some(command)
                    && *used.borrow() != Some(command)
                    && let Some(nonce) = nonce
                {
                    crate::ssh_wrap::mark_used(&state, &nonce).expect("used");
                    *used.borrow_mut() = Some(command);
                }
            }
            let up = session.remote_up();
            if up.is_some() && *seen.borrow() != up {
                let (_, nonce) = up.clone().unwrap();
                crate::ssh_wrap::mark_up(&state, &nonce).expect("mark");
                let mut argv = vec!["ssh".to_owned()];
                argv.extend(user_args.iter().cloned());
                crate::ssh_wrap::record_posix(&crate::ssh_route::SystemSsh, &argv, &state)
                    .expect("posix");
                *seen.borrow_mut() = up;
            }
        };
        let text = || screen(&session, &mut Blocks::default()).join("\n");
        let count = |needle: &str| text().matches(needle).count();
        let wait = |what: &str, secs: u64, args: &[String], ready: &dyn Fn() -> bool| {
            let deadline = Instant::now() + Duration::from_secs(secs);
            while Instant::now() < deadline {
                pane(args);
                if ready() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!("{what}\n{}", text());
        };
        let stripes = || -> Vec<bt_core::LinearRgba> {
            let mut blocks = Blocks::default();
            screen(&session, &mut blocks);
            blocks.as_slice().iter().map(|block| block.stripe).collect()
        };
        wait("no local prompt", 10, &[], &|| text().contains("$"));

        // (a) oh-my-zsh, clean state: the first connection is integrated.
        let deneme = ssh_args("deneme", 2249);
        let prompts = count("(deneme@127.0.0.1) Password:");
        session.write(line(&deneme).as_bytes());
        wait("no password prompt", 15, &deneme, &|| {
            count("(deneme@127.0.0.1) Password:") > prompts
        });
        session.write(b"parola123\r");
        wait("no remote directory", 20, &deneme, &|| {
            session.remote_link_directory() == "/home/deneme"
        });
        assert!(load(&state).knows(Fact::Posix, &key(&deneme)));
        session.write(b"false\r");
        wait("no remote error stripe", 15, &deneme, &|| {
            stripes().contains(&Theme::BATERI.error_linear())
        });
        eprintln!("(a) first connection: remote dir /home/deneme, remote block stripe, posix row");
        // The wrapped session carries the identity (no `SendEnv`
        // here — `-F /dev/null` — so it is the bootstrap's export).
        let identity = format!("lc=bateri|{}|{}.", bt_core::TERM_PROGRAM_VERSION, tab.url());
        let show_lc = b"printf 'lc=%s|%s|%s.\\n' \"${LC_TERMINAL-unset}\" \
                        \"${LC_TERMINAL_VERSION-unset}\" \"${LC_BATERI_TAB_URL-unset}\"\r";
        let seen_lc = count(&identity);
        session.write(show_lc);
        wait("no identity (wrapped)", 10, &deneme, &|| {
            count(&identity) > seen_lc
        });
        eprintln!("(identity) wrapped, AcceptEnv LANG LC_*: {identity}");
        session.write(b"exit\r");
        std::thread::sleep(Duration::from_secs(3));
        session.write(b"echo done-a\r");
        wait("exit did not return", 10, &deneme, &|| {
            text().contains("\ndone-a")
        });
        std::thread::sleep(Duration::from_secs(3));
        assert_eq!(
            count("(deneme@127.0.0.1) Password:"),
            prompts + 1,
            "reconnected after exit\n{}",
            text()
        );
        assert!(!load(&state).knows(Fact::Plain, &key(&deneme)));
        eprintln!("(a) exit: no reconnection, no plain row");

        // (a') inside a local multiplexer (`$TMUX` set — what tmux exports to
        // its panes): never wrapped, no fallback.
        let before = std::fs::read_to_string(&state).unwrap_or_default();
        session.write(format!("TMUX=x ssh {} ; echo done-t\r", deneme.join(" ")).as_bytes());
        wait("no password prompt (tmux)", 15, &[], &|| {
            count("(deneme@127.0.0.1) Password:") > prompts + 1
        });
        session.write(b"parola123\r");
        std::thread::sleep(Duration::from_secs(3));
        session.write(b"exit\r");
        wait("tmux ssh did not end", 10, &[], &|| {
            text().contains("\ndone-t")
        });
        assert_eq!(std::fs::read_to_string(&state).unwrap_or_default(), before);
        eprintln!("(a') TMUX set: not wrapped, state untouched, no rerun");

        // The identity across the carriers: a plain ssh (`TMUX` set — never wrapped)
        // with the stock `SendEnv LC_*` shows the identity on a server with
        // `AcceptEnv LANG LC_*`; on a server with `AcceptEnv LANG` only
        // (127.0.0.1:2251) the wrapped session still shows it, the plain one
        // does not (the known limit).
        let unset = "lc=unset|unset|unset.";
        let remote_lc = |port: u16, wrapped: bool, expect: &str| {
            let mut args = ssh_args("deneme", port);
            args.splice(2..2, ["-o".to_owned(), "'SendEnv=LC_*'".to_owned()]);
            let asked = count("(deneme@127.0.0.1) Password:");
            let prefix = if wrapped { "" } else { "TMUX=x " };
            session.write(format!("{prefix}ssh {}\r", args.join(" ")).as_bytes());
            wait("no password prompt (identity)", 15, &args, &|| {
                count("(deneme@127.0.0.1) Password:") > asked
            });
            session.write(b"parola123\r");
            std::thread::sleep(Duration::from_secs(3));
            let before = count(expect);
            session.write(show_lc);
            wait("no identity line", 10, &args, &|| count(expect) > before);
            session.write(b"exit\r");
            std::thread::sleep(Duration::from_secs(3));
            session.write(b"echo done-lc\r");
            let done = count("\ndone-lc");
            wait("identity session did not end", 10, &args, &|| {
                count("\ndone-lc") > done
            });
        };
        remote_lc(2249, false, &identity);
        eprintln!("(identity) plain, SendEnv LC_* + AcceptEnv LANG LC_*: {identity}");
        if std::net::TcpStream::connect(("127.0.0.1", 2251)).is_ok() {
            remote_lc(2251, true, &identity);
            eprintln!("(identity) wrapped, AcceptEnv LANG only: {identity}");
            remote_lc(2251, false, unset);
            eprintln!("(identity) plain, AcceptEnv LANG only: {unset} (known limit)");
        } else {
            eprintln!("SKIPPED: no AcceptEnv-restricted sshd on 127.0.0.1:2251");
        }

        // (b) the shell-less endpoint: plain rerun, and is a second password asked?
        let kapi = ssh_args("kapi", 2249);
        let asked = count("(kapi@127.0.0.1) Password:");
        session.write(line(&kapi).as_bytes());
        wait("no password prompt (kapi)", 15, &kapi, &|| {
            count("(kapi@127.0.0.1) Password:") > asked
        });
        session.write(b"parola123\r");
        wait("no error line", 15, &kapi, &|| {
            text().contains("'exec' is not recognized")
        });
        let ended = Instant::now();
        wait("no rerun", 15, &kapi, &|| {
            text().contains("C:\\>") || count("(kapi@127.0.0.1) Password:") > asked + 1
        });
        let second = count("(kapi@127.0.0.1) Password:") > asked + 1;
        eprintln!(
            "(b) rerun after {:?}; second password asked: {second}",
            ended.elapsed()
        );
        if second {
            session.write(b"parola123\r");
            wait("no plain shell", 15, &kapi, &|| text().contains("C:\\>"));
        }
        assert!(load(&state).knows(Fact::Plain, &key(&kapi)));
        session.write(b"exit\r");
        std::thread::sleep(Duration::from_secs(3));
        session.write(b"echo done-b\r");
        wait("plain exit did not return", 10, &kapi, &|| {
            text().contains("\ndone-b")
        });
        // The next connection is plain from the start: no error line again.
        let errors = count("'exec' is not recognized");
        let asked = count("(kapi@127.0.0.1) Password:");
        session.write(line(&kapi).as_bytes());
        wait("no password prompt (kapi again)", 15, &kapi, &|| {
            count("(kapi@127.0.0.1) Password:") > asked
        });
        session.write(b"parola123\r");
        wait("no plain shell (again)", 15, &kapi, &|| {
            text().matches("C:\\>").count() >= 2
        });
        assert_eq!(count("'exec' is not recognized"), errors);
        session.write(b"exit\r");
        std::thread::sleep(Duration::from_secs(3));
        session.write(b"echo done-b2\r");
        wait("second plain exit", 10, &kapi, &|| {
            text().contains("\ndone-b2")
        });
        eprintln!("(b) second connection plain from the start");

        // 255: ssh's own error records nothing and reruns nothing.
        let nowhere = ssh_args("nobody", 2250);
        session.write(format!("ssh {}; echo rc=$?\r", nowhere.join(" ")).as_bytes());
        wait("no 255", 15, &nowhere, &|| text().contains("rc=255"));
        assert!(!load(&state).knows(Fact::Plain, &key(&nowhere)));
        eprintln!("(255) no plain row, no rerun");

        // (c) ForceCommand of an interactive CLI: the user works, then exits.
        let router = ssh_args("router", 2249);
        let asked = count("(router@127.0.0.1) Password:");
        session.write(line(&router).as_bytes());
        wait("no password prompt (router)", 15, &router, &|| {
            count("(router@127.0.0.1) Password:") > asked
        });
        session.write(b"parola123\r");
        wait("no cli", 15, &router, &|| text().contains("router>"));
        session.write(b"show\r");
        wait("cli did not answer", 10, &router, &|| {
            text().contains("cli: show")
        });
        // The pane noticed the input after the login.
        wait(
            "the input after the login was not marked",
            10,
            &router,
            &|| {
                used.borrow()
                    .is_some_and(|command| Some(command) == session.running_command())
            },
        );
        session.write(b"exit\r");
        std::thread::sleep(Duration::from_secs(4));
        let rerun = count("(router@127.0.0.1) Password:") > asked + 1
            || text().matches("router>").count() > 2;
        let plain = load(&state).knows(Fact::Plain, &key(&router));
        eprintln!("(c) ForceCommand CLI: reconnected after exit: {rerun}; plain row: {plain}");
        assert!(
            !rerun && !plain,
            "a session the user worked in does not fall back"
        );
        // (d) Ctrl-C at a fresh server's password prompt (rc 130): no rerun,
        // no `plain` row (found in code review; an interactive zsh aborts
        // the function on the child's SIGINT anyway, `says_nothing` is the
        // binary's half for the other signals).
        let cancelled = ssh_args("root", 2249);
        let asked = count("(root@127.0.0.1) Password:");
        session.write(format!("ssh {}\r", cancelled.join(" ")).as_bytes());
        wait("no password prompt (root)", 15, &cancelled, &|| {
            count("(root@127.0.0.1) Password:") > asked
        });
        session.write(b"\x03");
        std::thread::sleep(Duration::from_secs(2));
        session.write(b"echo rc=$?\r");
        wait("no 130", 15, &cancelled, &|| text().contains("\nrc=130"));
        assert_eq!(count("(root@127.0.0.1) Password:"), asked + 1);
        assert!(!load(&state).knows(Fact::Plain, &key(&cancelled)));
        eprintln!("(d) Ctrl-C at the password prompt: rc=130, no rerun, no plain row");

        // (e) an endpoint that refuses our command with 255 after the login
        // (found in code review; `refuse`'s login shell answers `-c` with
        // "exec request failed" and 255, a shell request with a prompt): the
        // pane's login proof turns the 255 into a fallback — plain rerun,
        // `plain` row, the next connection plain from the start.
        let refuse = ssh_args("refuse", 2249);
        let asked = count("(refuse@127.0.0.1) Password:");
        session.write(line(&refuse).as_bytes());
        wait("no password prompt (refuse)", 15, &refuse, &|| {
            count("(refuse@127.0.0.1) Password:") > asked
        });
        session.write(b"parola123\r");
        wait("no refusal", 15, &refuse, &|| {
            text().contains("exec request failed on channel 0")
        });
        let ended = Instant::now();
        wait("no rerun (refuse)", 15, &refuse, &|| {
            text().contains("sw>") || count("(refuse@127.0.0.1) Password:") > asked + 1
        });
        let second = count("(refuse@127.0.0.1) Password:") > asked + 1;
        if second {
            session.write(b"parola123\r");
            wait("no plain prompt (refuse)", 15, &refuse, &|| {
                text().contains("sw>")
            });
        }
        assert!(load(&state).knows(Fact::Plain, &key(&refuse)));
        eprintln!(
            "(e) 255 after the login: rerun after {:?}; second password asked: {second}; plain row",
            ended.elapsed()
        );
        session.write(b"exit\r");
        std::thread::sleep(Duration::from_secs(3));
        session.write(b"echo done-e\r");
        wait("refuse exit did not return", 10, &refuse, &|| {
            text().contains("\ndone-e")
        });
        session.shutdown();
        let _ = std::fs::remove_dir_all(&root);
    }

    /// **Does the command's duration reach the screen in real zsh**.
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
                pane_uuid: None,
                hostname: None,
                replay: None,
                journal: None,
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

    // ── fish ─────────────────────────────────────────────────────────────

    /// The fish on this machine, held for the test's length.
    struct Fish {
        path: PathBuf,
        /// `(major, minor)` of `fish --version`; `(0, 0)` if it could not be read.
        version: (u32, u32),
        /// One fish session at a time: each starts a fish that asks the terminal about itself
        /// and waits for the answers, beside the rest of the suite's shells; in turn their
        /// load on the machine stays what one test makes of it.
        _serial: std::sync::MutexGuard<'static, ()>,
    }

    /// The fish on this machine, or `None` after the line the other real-shell tests print for
    /// a missing tool. The version is printed too: the wrapper is checked on whichever fish runs
    /// the test, and a green run says which one that was.
    fn fish() -> Option<Fish> {
        static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let on_path = std::env::var_os("PATH").and_then(|path| {
            std::env::split_paths(&path)
                .map(|dir| dir.join("fish"))
                .find(|candidate| candidate.is_file())
        });
        let found = on_path.or_else(|| {
            [
                "/opt/homebrew/bin/fish",
                "/usr/local/bin/fish",
                "/usr/bin/fish",
            ]
            .into_iter()
            .map(PathBuf::from)
            .find(|candidate| candidate.is_file())
        });
        let Some(path) = found else {
            println!("SKIPPED: no fish on this machine");
            return None;
        };
        let version = std::process::Command::new(&path)
            .arg("--version")
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
            .unwrap_or_default();
        println!("fish: {version} ({})", path.display());
        // `fish, version 4.9.3` (a development build adds `-12-gabcdef`).
        let mut numbers = version
            .rsplit(' ')
            .next()
            .unwrap_or_default()
            .split(['.', '-'])
            .map(|part| part.parse::<u32>().unwrap_or(0));
        let version = (numbers.next().unwrap_or(0), numbers.next().unwrap_or(0));
        Some(Fish {
            path,
            version,
            _serial: SERIAL
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        })
    }

    /// A real fish session with the wrapper installed the way the application installs it
    /// (`launch::integration_env`'s fish arm): our directory first in `XDG_DATA_DIRS`, with
    /// `data_dirs` after it, or with fish's defaults and `BATERI_XDG_UNSET` when `None`.
    ///
    /// `HOME` and the XDG directories are the test's own: fish writes its history and universal
    /// variables there, and the user's real configuration is never read.
    ///
    /// The session has a dock unless `extra` says `BATERI_DOCK=off`, the way the application
    /// reserves the share (`launch::dock_rows_at_birth`): one decision, the script's and the
    /// session's.
    fn spawn_fish(
        fish: &Fish,
        home: &Path,
        data_dirs: Option<&str>,
        extra: &[(&str, String)],
    ) -> Session {
        let wrapper = fish_wrapper_dir().expect("fish wrapper not found");
        let entry = fish_data_dir(&wrapper)
            .and_then(Path::to_str)
            .expect("the wrapper's entry")
            .to_owned();
        let mut env = HashMap::from([
            ("HOME".to_owned(), home.display().to_string()),
            (
                "XDG_CONFIG_HOME".to_owned(),
                home.join(".config").display().to_string(),
            ),
            (
                "XDG_DATA_HOME".to_owned(),
                home.join(".local/share").display().to_string(),
            ),
            ("BATERI_FISH_DIR".to_owned(), entry.clone()),
        ]);
        match data_dirs {
            Some(theirs) => {
                env.insert("XDG_DATA_DIRS".to_owned(), format!("{entry}:{theirs}"));
            }
            None => {
                env.insert(
                    "XDG_DATA_DIRS".to_owned(),
                    format!("{entry}:/usr/local/share:/usr/share"),
                );
                env.insert("BATERI_XDG_UNSET".to_owned(), "1".to_owned());
            }
        }
        for (key, value) in extra {
            env.insert((*key).to_owned(), value.clone());
        }
        let dock = env.get("BATERI_DOCK").is_none_or(|value| value != "off");
        Session::spawn(
            // `-l -i`: the shape of our session (`login` makes argv[0] `-fish`).
            shell_options(
                (
                    fish.path.display().to_string(),
                    vec!["-l".to_owned(), "-i".to_owned()],
                ),
                home,
                env,
                dock,
            ),
            Arc::new(SilentWake),
        )
        .expect("could not open the session")
    }

    /// The options of a real-shell session: `command` in `home`, with `env` added, 80 × 20
    /// cells and the default theme. `dock` is the share the application would reserve.
    fn shell_options(
        command: (String, Vec<String>),
        home: &Path,
        env: HashMap<String, String>,
        dock: bool,
    ) -> SessionOptions {
        SessionOptions {
            command: Some(command),
            working_directory: Some(home.to_owned()),
            home: Some(home.to_owned()),
            env,
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
            dock,
            cluster: false,
            initial_input: None,
            shell_marks: false,
            pane_uuid: None,
            hostname: None,
            replay: None,
            journal: None,
        }
    }

    /// `config.fish` of a fake home.
    fn write_fish_config(home: &Path, text: &str) -> PathBuf {
        let dir = home.join(".config/fish");
        std::fs::create_dir_all(&dir).expect("could not set up the fake home directory");
        let path = dir.join("config.fish");
        std::fs::write(&path, text).expect("config.fish not written");
        path
    }

    fn shown(session: &Session, text: &str) -> bool {
        screen(session, &mut Blocks::default())
            .iter()
            .any(|row| row.contains(text))
    }

    #[test]
    fn the_fish_wrapper_loads_the_users_files_and_reports_marks() {
        // The real test of the fish wrapper: a real fish, a real PTY and a real user
        // configuration. In one round:
        //
        // 1. The user's `config.fish` was read — after ours, with a broken line in it —
        //    and the prompt it defines is the one drawn: the integration adds marks to it,
        //    it does not replace it.
        // 2. Our entry is out of `XDG_DATA_DIRS` by the time `config.fish` runs, and the
        //    user's own list is what is left.
        // 3. The marks arrive: a finished command's code, the block's duration (fish prints
        //    its own, identity-less marks beside ours, and neither may steal the clock), the
        //    block anchors on the grid, the working directory.
        // 4. Nothing of the user's was written: `config.fish` is as it was and the universal
        //    variables hold nothing of ours.
        let Some(fish) = fish() else { return };
        let root = TempRoot::new("fish-wrapper");
        let home = root.0.join("home");
        let config = "set -g SEEN_CONFIG 1\n\
                      function fish_prompt\n  echo -n '$ '\nend\n\
                      bateri_missing_command\n\
                      echo \"$SEEN_CONFIG [$XDG_DATA_DIRS] [$BATERI_FISH_DIR] [$BATERI_DOCK]\" \
                      > $HOME/config_seen\n";
        let config_path = write_fish_config(&home, config);
        std::fs::create_dir_all(home.join("sub dir")).expect("sub dir");

        let session = spawn_fish(
            &fish,
            &home,
            Some("/usr/local/share:/opt/user/share"),
            &[("BATERI_DOCK", "off".to_owned())],
        );
        wait_until("prompt marks did not arrive", || {
            session
                .shell_state()
                .is_some_and(|state| state.phase == ShellPhase::Input)
        });
        assert!(shown(&session, "$"), "the user's prompt is not drawn");

        // 2: ours is out, the user's list stays; the dock decision and the directory
        // variable are ours and out of the environment too.
        let seen = home.join("config_seen");
        wait_until("config.fish did not run", || seen.is_file());
        assert_eq!(
            std::fs::read_to_string(&seen).expect("config_seen").trim(),
            "1 [/usr/local/share:/opt/user/share] [] []",
            "our entry was not taken out of XDG_DATA_DIRS or a variable of ours leaked"
        );

        // 3: the code, the directory, the anchors and the duration.
        session.write(b"false\n");
        wait_until("the command's exit code did not reach the state", || {
            session
                .shell_state()
                .is_some_and(|state| state.last_exit == Some(1))
        });
        session.write(b"cd 'sub dir'\n");
        wait_until("the directory did not reach the session", || {
            session
                .working_directory()
                .is_some_and(|dir| dir.ends_with("sub dir"))
        });
        // The two finished commands' anchors are on the grid, each with its stripe (the failed
        // one's differs from the other's); the prompt waiting for input has none yet.
        let mut blocks = Blocks::default();
        screen(&session, &mut blocks);
        assert_eq!(
            blocks.as_slice().len(),
            2,
            "the prompts' anchors were not on the grid: {:?}",
            blocks.as_slice()
        );
        assert_ne!(blocks.as_slice()[0].stripe, blocks.as_slice()[1].stripe);
        // A command that exceeds the threshold: the counter appears only when our `D` found the
        // clock the first `C` planted.
        session.write(b"sleep 2\n");
        wait_until("finished command's duration not shown", || {
            let drawn = screen(&session, &mut Blocks::default()).join("\n");
            drawn.contains("2.0s") || drawn.contains("2.1s")
        });

        // 4.
        session.write(b"exit\n");
        wait_until("history was not written to the fake home", || {
            home.join(".local/share/fish/fish_history").is_file()
        });
        assert_eq!(
            std::fs::read_to_string(&config_path).expect("config.fish"),
            config,
            "config.fish was written to"
        );
        let variables = home.join(".config/fish/fish_variables");
        if variables.is_file() {
            let text = std::fs::read_to_string(&variables).expect("fish_variables");
            assert!(
                !text.to_lowercase().contains("bateri"),
                "a universal variable of ours was written: {text}"
            );
        }
        session.shutdown();
    }

    #[test]
    fn fish_gets_the_data_dirs_it_would_have_had() {
        // `XDG_DATA_DIRS` unset is not "just ours": fish computes its `vendor_conf.d`,
        // `functions` and `completions` paths from the variable at startup, and ours alone would
        // drop the two the specification names as the default for the whole session. The
        // terminal writes them after ours; the script takes ours out and unsets the variable
        // again, because the user had none.
        let Some(fish) = fish() else { return };
        for (name, theirs, expected) in
            [("unset", None, "U"), ("set", Some("/opt/user/share"), "S")]
        {
            let root = TempRoot::new(&format!("fish-data-dirs-{name}"));
            let home = root.0.join("home");
            write_fish_config(&home, "");
            let session = spawn_fish(&fish, &home, theirs, &[]);
            wait_until("prompt marks did not arrive", || {
                session
                    .shell_state()
                    .is_some_and(|state| state.phase == ShellPhase::Input)
            });
            // The variable's state, and whether fish still has the directory the
            // specification names as the default.
            session.write(
                b"echo (set -q XDG_DATA_DIRS; and echo S; or echo U)(contains /usr/share/fish/vendor_conf.d $__fish_vendor_confdirs; and echo Y; or echo N)\n",
            );
            // Whether fish still has the default directory matters when the variable was unset;
            // with the user's own list it depends on the install (a distribution's fish also
            // compiles its vendor directories in).
            wait_until(
                &format!("{name}: the variable's state was not shown"),
                || {
                    shown(&session, &format!("{expected}Y"))
                        || shown(&session, &format!("{expected}N"))
                },
            );
            if theirs.is_none() {
                assert!(
                    shown(&session, "UY"),
                    "{name}: fish lost /usr/share/fish/vendor_conf.d"
                );
            }
            if let Some(theirs) = theirs {
                session.write(b"echo \"[$XDG_DATA_DIRS]\"\n");
                wait_until("the user's list was not shown", || {
                    shown(&session, &format!("[{theirs}]"))
                });
            }
            session.shutdown();
        }
    }

    #[test]
    fn the_fish_prompt_keeps_its_status_and_follows_a_redefinition() {
        // The wrapper runs the user's prompt, it does not replace it, and it runs it FIRST:
        // `$status` and `$pipestatus` are what fish handed the prompt. Any command before the
        // user's function would overwrite them. And a prompt the user (or a theme) defines
        // later is taken back at the next prompt.
        let Some(fish) = fish() else { return };
        let root = TempRoot::new("fish-prompt");
        let home = root.0.join("home");
        write_fish_config(
            &home,
            "function fish_prompt\n  echo \"top [$status|$pipestatus]\"\n  echo -n '> '\nend\n",
        );
        let session = spawn_fish(
            &fish,
            &home,
            Some("/a"),
            &[("BATERI_DOCK", "off".to_owned())],
        );
        wait_until("prompt marks did not arrive", || {
            session
                .shell_state()
                .is_some_and(|state| state.phase == ShellPhase::Input)
        });
        session.write(b"true | false | true; false\n");
        wait_until("the status was not in the prompt", || {
            shown(&session, "top [1|1]")
        });
        session.write(b"true | false\n");
        wait_until("the pipe status was not in the prompt", || {
            shown(&session, "top [1|0 1]")
        });

        // A new definition: wrapped again at the next prompt, so the block it opens is still
        // anchored (a finished command's stripe is what shows it).
        session.write(b"function fish_prompt; echo -n 'NEW> '; end\n");
        wait_until("the new prompt was not drawn", || shown(&session, "NEW>"));
        session.write(b"true\n");
        wait_until("the command did not finish", || {
            shown(&session, "NEW> true")
        });
        wait_until("the command's code did not arrive", || {
            session
                .shell_state()
                .is_some_and(|state| state.last_exit == Some(0))
        });
        let mut blocks = Blocks::default();
        let rows = screen(&session, &mut blocks);
        let typed = rows
            .iter()
            .position(|row| row.starts_with("NEW> true"))
            .expect("the typed line");
        assert!(
            blocks
                .as_slice()
                .iter()
                .any(|block| usize::from(block.row) == typed),
            "a prompt defined after the wrapper has no anchor: row {typed} vs {:?}",
            blocks.as_slice()
        );
        session.shutdown();
    }

    /// The line the dock shows, as the frame reads it from the grid and publishes it. The frame
    /// is what publishes, so every look is a frame; `blocks` is the caller's so the scratch the
    /// frame fills lives as long as the test does.
    fn read_dock(session: &Session, blocks: &std::cell::RefCell<Blocks>) -> DockState {
        screen(session, &mut blocks.borrow_mut());
        let mut state = DockState::default();
        session.dock_state(&mut state);
        state
    }

    /// A fish session with a dock, a prompt of the user's and a right prompt of the user's,
    /// both unique so that a claim "it is not drawn" cannot be fooled by other text.
    fn spawn_fish_dock(fish: &Fish, root: &TempRoot) -> Session {
        let home = root.0.join("home");
        write_fish_config(
            &home,
            "function fish_prompt\n  echo -n 'FISHPROMPTXY> '\nend\n\
             function fish_right_prompt\n  echo -n 'RIGHTXY'\nend\n",
        );
        let session = spawn_fish(fish, &home, Some("/a"), &[]);
        wait_until("prompt marks did not arrive", || {
            session
                .shell_state()
                .is_some_and(|state| state.phase == ShellPhase::Input)
        });
        session
    }

    #[test]
    fn the_fish_dock_shows_the_line_fish_draws() {
        // The real test of the fish dock: a real fish draws its prompt, colors and suggestion
        // and the terminal reads the line back from the grid. In one round:
        //
        // 1. The user's prompt and right prompt are not drawn: the dock's prompt is the
        //    terminal's (the same decision as zsh's), and the typed line is in the dock, in
        //    fish's own colors, the suggestion apart from what was typed, the caret after it.
        // 2. The grid does not show it a second time — a line that wraps included.
        // 3. After Enter the command is a block on the grid, and the next prompt is a dock
        //    again.
        let Some(fish) = fish() else { return };
        let root = TempRoot::new("fish-dock");
        let session = spawn_fish_dock(&fish, &root);
        let blocks = std::cell::RefCell::new(Blocks::default());
        let rows = || screen(&session, &mut blocks.borrow_mut());

        // A command the suggestion can come from.
        session.write(b"echo sugarcane\n");
        wait_until("the first command did not finish", || {
            session
                .shell_state()
                .is_some_and(|state| state.phase == ShellPhase::Input && state.last_exit == Some(0))
        });
        let before = rows().iter().filter(|row| row.contains("echo su")).count();
        assert_eq!(before, 1, "the finished command's line: {:?}", rows());

        // 1.
        session.write(b"echo su");
        wait_until("the dock did not show the typed line", || {
            let dock = read_dock(&session, &blocks);
            dock.status == DockStatus::Live && dock.buffer == "echo su"
        });
        let dock = read_dock(&session, &blocks);
        assert_eq!(dock.postdisplay, "garcane", "the suggestion: {dock:?}");
        assert_eq!(
            dock.cursor, 7,
            "the caret is after what was typed: {dock:?}"
        );
        // The suggestion is a run of its own, drawn unlike what was typed (fish's colors).
        let (typed, suggestion) = match dock.highlights.as_slice() {
            [.., typed, suggestion] => (typed, suggestion),
            other => panic!("fish's colors did not reach the dock: {other:?}"),
        };
        assert_eq!((suggestion.start, suggestion.end), (7, 14), "{dock:?}");
        assert_ne!(typed.style, suggestion.style, "{dock:?}");
        let drawn = rows().join("\n");
        assert!(
            !drawn.contains("FISHPROMPTXY") && !drawn.contains("RIGHTXY"),
            "the user's prompt was drawn on the grid:\n{drawn}"
        );
        // 2.
        assert_eq!(
            rows().iter().filter(|row| row.contains("echo su")).count(),
            before,
            "the typed line was drawn on the grid too:\n{drawn}"
        );
        session.write("x".repeat(100).as_bytes());
        wait_until("the dock did not show the wrapped line", || {
            let dock = read_dock(&session, &blocks);
            dock.status == DockStatus::Live
                && dock.buffer.ends_with("xxxxxxxxxx")
                && dock.buffer.len() == 107
        });
        let drawn = rows().join("\n");
        assert!(
            !drawn.contains("xxxxxxxxxx"),
            "the wrapped line was drawn on the grid too:\n{drawn}"
        );
        // A resize repaints the prompt under the same anchor and reflows the line: it stays in
        // the dock, whole, and off the grid at either width.
        for cols in [50, 80] {
            let _ = session.resize(cols, 20, (9, 18));
            wait_until("the dock lost the line in a resize", || {
                let dock = read_dock(&session, &blocks);
                dock.status == DockStatus::Live && dock.buffer.len() == 107
            });
            let drawn = rows().join("\n");
            assert!(
                !drawn.contains("xxxxxxxxxx"),
                "the line was drawn on the grid at {cols} columns:\n{drawn}"
            );
        }
        // Back to what was typed first: erase the x's.
        session.write("\u{8}".repeat(100).as_bytes());
        wait_until("the dock did not show the shortened line", || {
            read_dock(&session, &blocks).buffer == "echo su"
        });

        // 3.
        session.write(b"\n");
        wait_until("the command did not finish", || {
            shown(&session, "su")
                && session.shell_state().is_some_and(|state| {
                    state.phase == ShellPhase::Input && state.last_exit == Some(0)
                })
                && rows().iter().filter(|row| row.contains("echo su")).count() == 2
        });
        let mut now = Blocks::default();
        screen(&session, &mut now);
        assert_eq!(
            now.as_slice().len(),
            2,
            "the commands' anchors were not on the grid: {:?}",
            now.as_slice()
        );
        wait_until("the next prompt was not a dock", || {
            let dock = read_dock(&session, &blocks);
            dock.status == DockStatus::Live && dock.buffer.is_empty()
        });
        session.shutdown();
    }

    /// One frame and one drawing of the dock, as the window does: the drawing is what leaves
    /// the dock's last drawn window behind, and a click in the dock is resolved against it.
    fn draw_dock(session: &Session, blocks: &std::cell::RefCell<Blocks>) {
        let cursor = session.frame(
            |_| (),
            |_| (),
            &mut blocks.borrow_mut(),
            &mut bt_core::SelectionRuns::default(),
            &mut bt_core::SearchRuns::default(),
            &mut bt_core::TrackMarks::default(),
            &mut bt_core::Clusters::default(),
            &mut bt_core::Pictures::default(),
            ScrollGlide::default(),
            bt_core::DockBudget {
                share: 0.5,
                cols: 80,
            },
        );
        session.dock(
            bt_core::DockCols {
                input: 80,
                context: 80,
            },
            cursor.band_rows(),
            &mut DockState::default(),
            &mut bt_core::DockContext::default(),
            cursor.caret_in_dock,
            &mut Vec::new(),
            &mut bt_core::Clusters::default(),
            |_| (),
            |_| (),
        );
    }

    /// A click in the dock the way the window ends one: press on the character's left half,
    /// release without a drag.
    fn click_dock(session: &Session, blocks: &std::cell::RefCell<Blocks>, index: u16) {
        draw_dock(session, blocks);
        session.dock_select(
            bt_core::SelectKind::Simple,
            bt_core::SelectionPoint {
                col: bt_core::DOCK_TEXT_COL + index % 78,
                row: index / 78,
                half: bt_core::CellHalf::Left,
            },
        );
        session.dock_click();
    }

    #[test]
    fn a_click_in_the_fish_dock_moves_fishs_caret() {
        // The dock cannot edit fish's line: fish owns it. What it can do is click fish where
        // the character was drawn (fish 4.1's `click_events`, announced on its own `A`), and
        // fish moves its caret the way it does for a click in its own prompt. The whole chain
        // is real: the script's `g`, fish's `A;click_events=1`, the cells the line was read
        // from, the SGR press and release the click sends, fish's reading of them (it asks the
        // terminal where its prompt is, and the session answers) and the dock reading the
        // moved caret back.
        let Some(fish) = fish() else { return };
        let root = TempRoot::new("fish-click");
        let session = spawn_fish_dock(&fish, &root);
        let blocks = std::cell::RefCell::new(Blocks::default());
        let caret = || read_dock(&session, &blocks).cursor;

        session.write(b"echo hello world");
        wait_until("the dock did not show the typed line", || {
            let dock = read_dock(&session, &blocks);
            dock.status == DockStatus::Live && dock.buffer == "echo hello world"
        });
        wait_until("the caret was not at the end", || caret() == 16);

        // Clicks are fish 4.1's: an older fish never announces them, and the dock's click
        // must then do what it did before — nothing reaches fish.
        if fish.version < (4, 1) {
            println!(
                "fish {:?} takes no clicks: checking that none is sent",
                fish.version
            );
            click_dock(&session, &blocks, 5);
            std::thread::sleep(std::time::Duration::from_millis(500));
            assert_eq!(
                caret(),
                16,
                "a click moved a fish that does not take clicks"
            );
            session.shutdown();
            return;
        }

        // The middle of the line, then its start, then a place back to the right: fish's caret
        // lands on the character clicked, which the dock shows as the caret's index.
        for index in [5u16, 0, 11] {
            click_dock(&session, &blocks, index);
            wait_until("the caret did not follow the click", || {
                caret() == usize::from(index)
            });
        }

        // Past the last character: the end of the line, however the click is aimed at it.
        click_dock(&session, &blocks, 16);
        wait_until("a click at the end did not reach the end", || caret() == 16);
        click_dock(&session, &blocks, 40);
        wait_until("a click past the line did not reach the end", || {
            caret() == 16
        });

        // The caret moved while the text did not: the line is still typed text, not a
        // new line.
        let dock = read_dock(&session, &blocks);
        assert_eq!(dock.buffer, "echo hello world", "{dock:?}");

        // With a suggestion after the typed text: a click on the suggestion is a click at the
        // end of what was typed (the dock's hit test), and fish does not take the suggestion.
        session.write(b"\n");
        wait_until("the command did not finish", || {
            session
                .shell_state()
                .is_some_and(|state| state.phase == ShellPhase::Input && state.last_exit == Some(0))
                && read_dock(&session, &blocks).buffer.is_empty()
        });
        session.write(b"echo he");
        wait_until("the dock did not show the suggestion", || {
            let dock = read_dock(&session, &blocks);
            dock.buffer == "echo he" && dock.postdisplay == "llo world"
        });
        click_dock(&session, &blocks, 2);
        wait_until("the caret did not move into the typed text", || {
            caret() == 2
        });
        click_dock(&session, &blocks, 12);
        wait_until(
            "a click on the suggestion did not reach the end of the text",
            || caret() == 7,
        );
        let dock = read_dock(&session, &blocks);
        assert_eq!(dock.buffer, "echo he", "the suggestion was taken: {dock:?}");
        session.shutdown();
    }

    #[test]
    fn a_job_notice_stays_on_the_grid_under_a_fish_dock() {
        // fish prints a background job's end under the open link of the prompt that is on
        // screen, then draws the prompt again from its cache — the same anchor URI, a fresh
        // link id. The notice stays in the grid (it is output) and the dock holds the new
        // prompt's line only. Typed BEFORE the job ends: the harder order, as fish redraws the
        // line it holds.
        let Some(fish) = fish() else { return };
        let root = TempRoot::new("fish-dock-job");
        let session = spawn_fish_dock(&fish, &root);
        let blocks = std::cell::RefCell::new(Blocks::default());
        session.write(b"sleep 2 &\n");
        wait_until("the job did not start", || {
            session
                .shell_state()
                .is_some_and(|state| state.phase == ShellPhase::Input && state.last_exit == Some(0))
        });
        session.write(b"xy");
        wait_until("the dock did not show the typed line", || {
            let dock = read_dock(&session, &blocks);
            dock.status == DockStatus::Live && dock.buffer == "xy"
        });
        wait_until("the job's notice was not on the grid", || {
            shown(&session, "has ended")
        });
        let dock = read_dock(&session, &blocks);
        assert_eq!(
            (dock.status, dock.buffer.as_str()),
            (DockStatus::Live, "xy"),
            "the dock lost the line, or took the notice into it: {dock:?}"
        );
        let rows = screen(&session, &mut blocks.borrow_mut());
        assert!(
            rows.iter().any(|row| row.contains("has ended")),
            "the notice went with the dock's line:\n{}",
            rows.join("\n")
        );
        // fish abandons the old prompt's row where it stood (the notice goes under it), so the
        // grid keeps one `xy`: the old, outside the dock's range. The new prompt's is in the dock
        // only; a second one on the grid would be the double image.
        assert_eq!(
            rows.iter().filter(|row| row.trim() == "xy").count(),
            1,
            "the typed line was drawn on the grid as well as in the dock:\n{}",
            rows.join("\n")
        );
        session.shutdown();
    }

    #[test]
    fn the_fish_dock_takes_back_a_prompt_defined_later() {
        // A prompt the user (or a theme) defines after the wrapper is the terminal's again at
        // the next prompt: the event defines it at every prompt.
        let Some(fish) = fish() else { return };
        let root = TempRoot::new("fish-dock-redefine");
        let session = spawn_fish_dock(&fish, &root);
        session.write(b"function fish_prompt; echo -n 'NEWXY> '; end\n");
        session.write(b"true\n");
        wait_until("the command did not finish", || {
            session
                .shell_state()
                .is_some_and(|state| state.phase == ShellPhase::Input && state.last_exit == Some(0))
        });
        let blocks = std::cell::RefCell::new(Blocks::default());
        wait_until("the next prompt was not a dock", || {
            let dock = read_dock(&session, &blocks);
            dock.status == DockStatus::Live && dock.buffer.is_empty()
        });
        let rows = screen(&session, &mut blocks.borrow_mut());
        assert!(
            !rows.iter().any(|row| row.starts_with("NEWXY>")),
            "the later prompt was drawn:\n{}",
            rows.join("\n")
        );
        session.shutdown();
    }

    #[test]
    fn the_fish_blocks_setting_keeps_the_prompt_and_opens_no_dock() {
        // `integration = "blocks"` (`BATERI_DOCK=off`): the user's prompt and the line stay in
        // the grid, and nothing is read back from it.
        let Some(fish) = fish() else { return };
        let root = TempRoot::new("fish-blocks");
        let home = root.0.join("home");
        write_fish_config(
            &home,
            "function fish_prompt\n  echo -n 'FISHPROMPTXY> '\nend\n",
        );
        let session = spawn_fish(
            &fish,
            &home,
            Some("/a"),
            &[("BATERI_DOCK", "off".to_owned())],
        );
        wait_until("prompt marks did not arrive", || {
            session
                .shell_state()
                .is_some_and(|state| state.phase == ShellPhase::Input)
        });
        session.write(b"echo hi");
        wait_until("the line was not on the grid", || {
            shown(&session, "FISHPROMPTXY> echo hi")
        });
        let blocks = std::cell::RefCell::new(Blocks::default());
        let dock = read_dock(&session, &blocks);
        assert_ne!(dock.status, DockStatus::Live, "{dock:?}");
        assert!(dock.buffer.is_empty(), "{dock:?}");
        session.shutdown();
    }

    #[test]
    fn the_fish_script_finishes_the_line_before_it_starts_the_command() {
        // `8133;e` hands the line from the dock to the grid in one step: Enter has moved the
        // cursor down a row and, without it, the dock would show the empty row until `C`. The
        // order is the script's whole point and a real fish cannot show it (the gap is a
        // frame); the text of the handler can.
        let path = fish_wrapper_dir()
            .expect("fish wrapper not found")
            .join("vendor_conf.d/bateri.fish");
        let script = std::fs::read_to_string(path).expect("the script");
        let start = script
            .find("function __bateri_start --on-event fish_preexec")
            .expect("the preexec handler");
        let body = &script[start..start + script[start..].find("\nend\n").expect("its end")];
        let end = body.find("8133;e").expect("`e` in the handler");
        let command = body.find("133;C").expect("`C` in the handler");
        assert!(end < command, "{body}");
    }

    #[test]
    fn the_fish_wrappers_ssh_function_asks_the_binary() {
        // `ssh` is the user's until the first prompt: a function of their own wins, and
        // otherwise ours asks the binary (`bateri ssh-argv`, one parser) for the wrapped
        // arguments, NUL-separated, an empty one included.
        let Some(fish) = fish() else { return };
        let root = TempRoot::new("fish-ssh");
        let home = root.0.join("home");
        let bin = root.0.join("bin");
        std::fs::create_dir_all(&bin).expect("bin");
        let log = root.0.join("asked");
        let script = |path: PathBuf, body: String| write_program(&path, &body);
        let fall = root.0.join("fall");
        script(
            bin.join("bateri"),
            format!(
                "#!/bin/sh\necho \"$*\" >> '{}'\ncase \"$1 $2\" in\n\
                 'ssh-argv --tty') printf -- '-t\\0x\\0y z\\0\\0BOOT\\0' ;;\n\
                 ssh-fell-back*) [ -f '{}' ] && printf -- 'PLAIN\\0x\\0' ;;\nesac\nexit 0\n",
                log.display(),
                fall.display()
            ),
        );
        let code = root.0.join("code");
        script(
            bin.join("ssh"),
            format!(
                "#!/bin/sh\nfor a in \"$@\"; do printf '[%s]' \"$a\"; done; echo\n\
                 [ -f '{0}' ] && exit \"$(cat '{0}')\"\nexit 0\n",
                code.display()
            ),
        );
        let spawn = |rc: &str| {
            // The stand-ins first on `PATH`, after the system's login files (macOS's
            // `path_helper` rewrites it for a login shell).
            write_fish_config(
                &home,
                &format!(
                    "set -gx PATH {} $PATH\nfunction fish_prompt; echo -n '$ '; end\n{rc}",
                    bin.display()
                ),
            );
            spawn_fish(
                &fish,
                &home,
                Some("/a"),
                &[
                    ("BATERI_DOCK", "off".to_owned()),
                    ("BATERI_BIN", bin.join("bateri").display().to_string()),
                    ("BATERI_SSH_INSTANCE", "0a1b2c3d".to_owned()),
                    ("BATERI_SSH_CWD", "/srv/my app".to_owned()),
                ],
            )
        };

        let session = spawn("");
        wait_until("prompt marks did not arrive", || {
            session
                .shell_state()
                .is_some_and(|state| state.phase == ShellPhase::Input)
        });
        session.write(
            b"ssh x 'y z'; ssh x | cat; echo \"[\"(set -q BATERI_BIN BATERI_SSH_INSTANCE BATERI_SSH_CWD; echo $status)\"]\"\n",
        );
        wait_until("the wrapped call did not run", || {
            shown(&session, "[-t][x][y z][][BOOT]") && shown(&session, "[3]")
        });
        let asked = std::fs::read_to_string(&log).expect("the binary was asked");
        // `--block` is the command's own block: the first prompt's; the folder went with the
        // first call and not the second; the wrapped call asked the fallback with its arguments.
        assert_eq!(
            asked,
            "ssh-argv --tty --block 1 --instance 0a1b2c3d --cwd /srv/my app -- x y z\n\
             ssh-fell-back --rc 0 --instance 0a1b2c3d -- -t x y z  BOOT\n\
             ssh-argv --block 1 --instance 0a1b2c3d -- x\n"
        );
        std::fs::remove_file(&log).expect("log");

        // A fallback answer is rerun plain; the code is the rerun's.
        std::fs::write(&fall, "").expect("fall");
        std::fs::write(&code, "7").expect("code");
        session.write(b"ssh x; echo \"rc=$status\"\n");
        wait_until("the plain rerun did not run", || {
            shown(&session, "[PLAIN][x]") && shown(&session, "rc=7")
        });
        // Inside a multiplexer nothing is wrapped.
        std::fs::remove_file(&fall).expect("fall");
        std::fs::write(&code, "255").expect("code");
        session.write(b"env TMUX=t fish -c 'true'; set -x TMUX t; ssh x; echo \"r2=$status\"\n");
        wait_until("the tmux call did not run", || shown(&session, "r2=255"));
        session.shutdown();

        // A user's own `ssh` function wins: ours is not defined over it.
        let session = spawn("function ssh; echo USER-SSH; end\n");
        wait_until("prompt marks did not arrive", || {
            session
                .shell_state()
                .is_some_and(|state| state.phase == ShellPhase::Input)
        });
        session.write(b"ssh x\n");
        wait_until("the user's ssh did not run", || shown(&session, "USER-SSH"));
        session.shutdown();
    }

    #[test]
    fn the_fish_wrapper_forks_git_only_for_a_dock() {
        // The branch is the dock's context line and nothing else reads it: without a dock
        // (`BATERI_DOCK=off`) it is a `git` fork per prompt for no consumer. The stand-in `git`
        // records that it was asked.
        let Some(fish) = fish() else { return };
        let root = TempRoot::new("fish-git");
        let home = root.0.join("home");
        let bin = root.0.join("bin");
        std::fs::create_dir_all(&bin).expect("bin");
        let log = root.0.join("git_asked");
        let git = bin.join("git");
        write_program(
            &git,
            &format!("#!/bin/sh\necho \"$*\" >> '{}'\necho main\n", log.display()),
        );
        for (dock, extra) in [(false, "off"), (true, "")] {
            let _ = std::fs::remove_file(&log);
            write_fish_config(
                &home,
                &format!(
                    "set -gx PATH {} $PATH\nfunction fish_prompt; echo -n '$ '; end\n",
                    bin.display()
                ),
            );
            let env: Vec<(&str, String)> = if dock {
                Vec::new()
            } else {
                vec![("BATERI_DOCK", extra.to_owned())]
            };
            let session = spawn_fish(&fish, &home, Some("/a"), &env);
            wait_until("prompt marks did not arrive", || {
                session
                    .shell_state()
                    .is_some_and(|state| state.phase == ShellPhase::Input)
            });
            session.write(b"true\n");
            wait_until("the second prompt did not arrive", || {
                session
                    .shell_state()
                    .is_some_and(|state| state.last_exit == Some(0))
            });
            std::thread::sleep(std::time::Duration::from_millis(300));
            let asked = std::fs::read_to_string(&log).unwrap_or_default();
            if dock {
                assert!(
                    asked.starts_with("rev-parse --abbrev-ref HEAD"),
                    "the branch was not asked at a dock's prompt: {asked:?}"
                );
            } else {
                assert!(asked.is_empty(), "git was forked without a dock: {asked:?}");
            }
            session.shutdown();
        }
    }

    /// The report a dock prompt sends with the branch, once it changed from `since`.
    fn environment_after(session: &Session, since: u64) -> bt_core::ShellEnvironment {
        wait_until("no environment report arrived", || {
            session.environment_request().generation > since
        });
        session.environment_request().reported
    }

    #[test]
    fn the_fish_wrapper_reports_the_environment_after_the_branch() {
        // What `activate.fish` and conda leave in the variables, and the `node` on fish's
        // `PATH`, reach the terminal at the dock's prompt; `set -e` (what `deactivate`
        // does) is seen at the next one.
        let Some(fish) = fish() else { return };
        let root = TempRoot::new("fish-env");
        let home = root.0.join("home");
        let bin = root.0.join("bin");
        std::fs::create_dir_all(&bin).expect("bin");
        write_program(&bin.join("node"), "#!/bin/sh\n");
        write_fish_config(
            &home,
            &format!(
                "set -gx PATH {} $PATH\nset -gx VIRTUAL_ENV /p/.venv\n\
                 set -gx VIRTUAL_ENV_PROMPT .venv\nset -gx CONDA_PROMPT_MODIFIER '(base) '\n",
                bin.display()
            ),
        );
        let session = spawn_fish(&fish, &home, Some("/a"), &[]);
        let reported = environment_after(&session, 0);
        assert_eq!(reported.virtual_env, "/p/.venv");
        assert_eq!(reported.virtual_env_prompt, ".venv");
        assert_eq!(reported.conda_prefix, "");
        assert_eq!(reported.conda_prompt, "(base) ");
        assert_eq!(Path::new(&reported.node), bin.join("node"));
        let generation = session.environment_request().generation;
        session.write(b"set -e VIRTUAL_ENV\n");
        let reported = environment_after(&session, generation);
        assert_eq!(reported.virtual_env, "");
        assert_eq!(
            reported.conda_prompt, "(base) ",
            "the other fields keep their places"
        );
        session.shutdown();
    }

    #[test]
    fn the_bash_wrapper_reports_the_environment_after_the_branch() {
        // The fish test's claim for bash: `node` is found by walking `PATH`, no fork.
        let Some(bash) = bash() else { return };
        let root = TempRoot::new("bash-env");
        let home = root.0.join("home");
        let bin = root.0.join("bin");
        std::fs::create_dir_all(&bin).expect("bin");
        write_program(&bin.join("node"), "#!/bin/sh\n");
        write_home_file(
            &home,
            ".bash_profile",
            &format!(
                "PS1='$ '\nexport PATH='{}':\"$PATH\"\nexport VIRTUAL_ENV=/p/.venv\n\
                 export VIRTUAL_ENV_PROMPT='(.venv) '\nexport CONDA_PREFIX=/opt/conda\n",
                bin.display()
            ),
        );
        let session = spawn_bash_dock(&bash, &home, &[]);
        let reported = environment_after(&session, 0);
        assert_eq!(reported.virtual_env, "/p/.venv");
        assert_eq!(reported.virtual_env_prompt, "(.venv) ");
        assert_eq!(reported.conda_prefix, "/opt/conda");
        assert_eq!(reported.conda_prompt, "");
        assert_eq!(Path::new(&reported.node), bin.join("node"));
        let generation = session.environment_request().generation;
        session.write(b"unset VIRTUAL_ENV\n");
        let reported = environment_after(&session, generation);
        assert_eq!(reported.virtual_env, "");
        assert_eq!(
            reported.conda_prefix, "/opt/conda",
            "the other fields keep their places"
        );
        session.shutdown();
    }

    // ── bash ─────────────────────────────────────────────────────────────

    /// A bash on this machine the wrapper runs on (4.4 or newer), held for the test's length.
    struct Bash {
        path: PathBuf,
        version: BashVersion,
    }

    /// The first bash on `PATH` or in the usual places that is new enough, or `None` after the
    /// line the other real-shell tests print for a missing tool. macOS's own `/bin/bash` is 3.2
    /// and is passed over, as the terminal passes over it. The version is printed: the wrapper is
    /// checked on whichever bash runs the test, and a green run says which one that was.
    fn bash() -> Option<Bash> {
        let on_path = std::env::var_os("PATH")
            .map(|path| {
                std::env::split_paths(&path)
                    .map(|dir| dir.join("bash"))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let usual = [
            "/opt/homebrew/bin/bash",
            "/usr/local/bin/bash",
            "/usr/bin/bash",
            "/bin/bash",
        ]
        .map(PathBuf::from);
        for path in on_path.into_iter().chain(usual) {
            if !path.is_file() {
                continue;
            }
            let Some(version) = query_bash_version(&path, std::time::Duration::from_secs(5)) else {
                continue;
            };
            if version.integrates() {
                println!(
                    "bash: {}.{} ({})",
                    version.major,
                    version.minor,
                    path.display()
                );
                return Some(Bash { path, version });
            }
        }
        println!("SKIPPED: no bash 4.4 or newer on this machine");
        None
    }

    /// A real bash session with the wrapper installed the way the application installs it
    /// (`launch::integration_env`'s bash arm): `ENV` names the file and the shell is started
    /// `--posix -l` (the Linux shape of the command; on macOS `login(1)` gives the same shell
    /// its login name).
    ///
    /// `HOME` is the test's own: bash writes its history there, and the user's real files are
    /// never read. `extra` is the rest of the contract (`BATERI_ENV`, `BATERI_BIN`, …).
    fn spawn_bash(bash: &Bash, home: &Path, extra: &[(&str, String)]) -> Session {
        spawn_bash_with(bash, home, extra, false)
    }

    /// [`spawn_bash`] with the dock: `BATERI_DOCK` is not sent, which is what `"auto"` does.
    fn spawn_bash_dock(bash: &Bash, home: &Path, extra: &[(&str, String)]) -> Session {
        spawn_bash_with(bash, home, extra, true)
    }

    fn spawn_bash_with(
        bash: &Bash,
        home: &Path,
        extra: &[(&str, String)],
        with_dock: bool,
    ) -> Session {
        let wrapper = bash_wrapper_dir().expect("bash wrapper not found");
        let mut env = HashMap::from([
            ("HOME".to_owned(), home.display().to_string()),
            (
                "ENV".to_owned(),
                wrapper.join("bateri.bash").display().to_string(),
            ),
        ]);
        if !with_dock {
            env.insert("BATERI_DOCK".to_owned(), "off".to_owned());
        }
        for (key, value) in extra {
            env.insert((*key).to_owned(), value.clone());
        }
        let dock = env.get("BATERI_DOCK").is_none_or(|value| value != "off");
        Session::spawn(
            shell_options(
                (
                    bash.path.display().to_string(),
                    vec!["--posix".to_owned(), "-l".to_owned()],
                ),
                home,
                env,
                dock,
            ),
            Arc::new(SilentWake),
        )
        .expect("could not open the session")
    }

    /// A file of a fake home.
    fn write_home_file(home: &Path, name: &str, text: &str) -> PathBuf {
        std::fs::create_dir_all(home).expect("could not set up the fake home directory");
        let path = home.join(name);
        std::fs::write(&path, text).expect("home file not written");
        path
    }

    fn at_the_prompt(session: &Session) {
        wait_until("prompt marks did not arrive", || {
            session
                .shell_state()
                .is_some_and(|state| state.phase == ShellPhase::Input)
        });
    }

    #[test]
    fn the_bash_wrapper_loads_the_login_files_and_reports_marks() {
        // The real test of the bash wrapper: a real bash, a real PTY and a real user
        // configuration. In one round:
        //
        // 1. The login files were read in bash's order, from our file, with a broken line in
        //    the first and a `PROMPT_COMMAND` that rewrites `PS1` at every prompt: only
        //    `~/.bash_profile` ran (not `~/.profile`, not `~/.bashrc`), the prompt it
        //    defines is the one drawn, and the blocks still have their anchors.
        // 2. What is ours is out of the environment by then: `ENV` (the user's own is back, or
        //    gone), `BATERI_*`; the shell is out of POSIX mode and says so in no variable;
        //    the history file is `~/.bash_history`, not POSIX mode's `~/.sh_history`, unless
        //    the environment named one.
        // 3. The marks arrive: a finished command's code, the working directory, the block
        //    anchors on the grid, the duration.
        // 4. It is a login shell (`logout` works and `~/.bash_logout` is read) and nothing of
        //    the user's was written.
        let Some(bash) = bash() else { return };
        let profile = "PS1='$ '\n\
                       PROMPT_COMMAND='PS1=\"$ \"'\n\
                       export FROM_PROFILE=1\n\
                       bateri_missing_command\n\
                       echo \"$FROM_PROFILE [${ENV-unset}] [${BATERI_ENV-unset}] [$HISTFILE] \
                       [${POSIXLY_CORRECT-unset}] [${BATERI_DOCK-unset}] [${BATERI_BIN-unset}]\" \
                       > \"$HOME/profile_seen\"\n";
        for (name, extra, user_env, history) in [
            ("none", vec![], "unset", ".bash_history"),
            (
                "env",
                vec![("BATERI_ENV", "/home/me/.shrc".to_owned())],
                "/home/me/.shrc",
                ".bash_history",
            ),
            ("histfile", Vec::new(), "unset", ".sh_history"),
        ] {
            let root = TempRoot::new(&format!("bash-wrapper-{name}"));
            let home = root.0.join("home");
            let profile_path = write_home_file(&home, ".bash_profile", profile);
            write_home_file(&home, ".profile", "touch \"$HOME/profile_read\"\n");
            write_home_file(&home, ".bashrc", "touch \"$HOME/bashrc_read\"\n");
            write_home_file(&home, ".bash_logout", "touch \"$HOME/logout_ran\"\n");
            std::fs::create_dir_all(home.join("sub dir")).expect("sub dir");
            let mut extra = extra;
            if name == "histfile" {
                // An exported `HISTFILE` is the user's, whatever it is.
                extra.push(("HISTFILE", home.join(".sh_history").display().to_string()));
            }

            let session = spawn_bash(&bash, &home, &extra);
            at_the_prompt(&session);
            assert!(
                shown(&session, "$"),
                "{name}: the user's prompt is not drawn"
            );
            let seen = home.join("profile_seen");
            wait_until("~/.bash_profile did not run", || seen.is_file());
            assert_eq!(
                std::fs::read_to_string(&seen).expect("profile_seen").trim(),
                format!(
                    "1 [{user_env}] [unset] [{}] [unset] [unset] [unset]",
                    home.join(history).display()
                ),
                "{name}: a variable of ours leaked, or the history file is wrong"
            );
            assert!(
                !home.join("profile_read").exists(),
                "{name}: ~/.profile ran"
            );
            assert!(!home.join("bashrc_read").exists(), "{name}: ~/.bashrc ran");
            if name != "none" {
                session.write(b"exit\n");
                wait_until("logout did not run", || home.join("logout_ran").is_file());
                session.shutdown();
                continue;
            }

            // 3: the code, the directory, the anchors and the duration.
            session.write(b"false\n");
            wait_until("the command's exit code did not reach the state", || {
                session
                    .shell_state()
                    .is_some_and(|state| state.last_exit == Some(1))
            });
            session.write(b"cd 'sub dir'\n");
            wait_until("the directory did not reach the session", || {
                session
                    .working_directory()
                    .is_some_and(|dir| dir.ends_with("sub dir"))
            });
            // The two finished commands' anchors are on the grid, each with its stripe (the
            // failed one's differs from the other's); the prompt waiting for input has none yet.
            // The user's `PROMPT_COMMAND` wrote `PS1` again before each of them.
            let mut blocks = Blocks::default();
            screen(&session, &mut blocks);
            assert_eq!(
                blocks.as_slice().len(),
                2,
                "the prompts' anchors were not on the grid: {:?}",
                blocks.as_slice()
            );
            assert_ne!(blocks.as_slice()[0].stripe, blocks.as_slice()[1].stripe);
            // A command that exceeds the threshold: the counter appears only when our `D`
            // found the clock the `C` planted.
            session.write(b"sleep 2\n");
            wait_until("finished command's duration not shown", || {
                let drawn = screen(&session, &mut Blocks::default()).join("\n");
                drawn.contains("2.0s") || drawn.contains("2.1s")
            });

            // 4: `logout` only works in a login shell; its file and the history were written.
            session.write(b"logout\n");
            wait_until("~/.bash_logout was not read", || {
                home.join("logout_ran").is_file()
            });
            wait_until("history was not written", || {
                home.join(".bash_history").is_file()
            });
            assert!(
                !home.join(".sh_history").exists(),
                "POSIX mode's history file was written"
            );
            assert_eq!(
                std::fs::read_to_string(&profile_path).expect("~/.bash_profile"),
                profile,
                "~/.bash_profile was written to"
            );
            session.shutdown();
        }
    }

    #[test]
    fn the_bash_prompt_is_wrapped_once_and_again_after_a_rewrite() {
        // The user's `PS1` is wrapped (anchor in front, end mark behind) and what is wrapped
        // is remembered: a prompt that comes back unchanged is wrapped again from the user's
        // own text with the new block's id, never the wrapper's own string wrapped twice. One
        // that a hook rewrote is the user's new text.
        let Some(bash) = bash() else { return };
        let root = TempRoot::new("bash-ps1");
        let home = root.0.join("home");
        write_home_file(&home, ".bash_profile", "PS1='\\[\\e[1m\\]$ \\[\\e[0m\\]'\n");
        let session = spawn_bash(&bash, &home, &[]);
        at_the_prompt(&session);
        let seen = home.join("ps1_seen");
        let ask = |session: &Session| {
            let _ = std::fs::remove_file(&seen);
            session.write(
                b"printf '%s|%s|%s' \"$PS1\" \"$__bateri_user_ps1\" \"$__bateri_block\" > \"$HOME/ps1_seen\"\n",
            );
            wait_until("the prompt was not read", || seen.is_file());
            std::fs::read_to_string(&seen).expect("ps1_seen")
        };
        for _ in 0..3 {
            session.write(b"true\n");
            wait_until("the next prompt did not arrive", || {
                session
                    .shell_state()
                    .is_some_and(|state| state.last_exit == Some(0))
            });
        }
        let user = "\\[\\e[1m\\]$ \\[\\e[0m\\]";
        let got = ask(&session);
        // Ours was set before this prompt's command ran: the block is the one of the prompt
        // the printf was typed at (four prompts so far, this is the fifth).
        let block = got.rsplit('|').next().expect("block").to_owned();
        assert_eq!(
            got,
            format!(
                "\\[\\e]8;;bateri://block/{block}\\a\\]{user}\\[\\e]133;B\\a\\]|{user}|{block}"
            ),
            "the prompt was wrapped twice or lost the user's text"
        );
        // A hook rewrites it: the new text is the user's from now on.
        session.write(b"PS1='% '\n");
        wait_until("the rewritten prompt did not come", || {
            shown(&session, "% ")
        });
        let got = ask(&session);
        let block = got.rsplit('|').next().expect("block").to_owned();
        assert_eq!(
            got,
            format!("\\[\\e]8;;bateri://block/{block}\\a\\]% \\[\\e]133;B\\a\\]|% |{block}")
        );
        session.shutdown();
    }

    #[test]
    fn a_users_prompt_command_keeps_running_in_every_form() {
        // Ours go around the user's, in whatever form it is: a string, one that ends in a
        // comment (the joiner is a newline, not `;`), an array (bash 5.1 and newer keep its
        // other elements; before, only the first one runs and ours must still be there), and a
        // shell with `set -u`.
        let Some(bash) = bash() else { return };
        let mut forms = vec![
            ("string", "PROMPT_COMMAND='touch \"$HOME/ran_one\"'\n"),
            (
                "comment",
                "PROMPT_COMMAND='touch \"$HOME/ran_one\" # a trailing comment'\n",
            ),
            ("none", "unset PROMPT_COMMAND\n"),
            // Under `set -u` the attribute query of an unset variable is an error.
            ("nounset", "unset PROMPT_COMMAND\nset -u\n"),
        ];
        // Before 5.1 only the array's first element runs, and ours must still be there.
        forms.push((
            "array",
            "PROMPT_COMMAND=('touch \"$HOME/ran_one\"' 'touch \"$HOME/ran_two\"')\n",
        ));
        for (name, profile) in forms {
            let root = TempRoot::new(&format!("bash-pc-{name}"));
            let home = root.0.join("home");
            write_home_file(&home, ".bash_profile", profile);
            let session = spawn_bash(&bash, &home, &[]);
            at_the_prompt(&session);
            session.write(b"true\n");
            wait_until("the second prompt did not arrive", || {
                session
                    .shell_state()
                    .is_some_and(|state| state.last_exit == Some(0))
            });
            if name != "none" && name != "nounset" {
                wait_until(&format!("{name}: the user's hook did not run"), || {
                    home.join("ran_one").is_file()
                });
            }
            if name == "array" && bash.version >= BashVersion::new(5, 1) {
                wait_until("array: the second element did not run", || {
                    home.join("ran_two").is_file()
                });
            }
            // And ours did: the finished command's anchor is on the grid.
            let mut blocks = Blocks::default();
            screen(&session, &mut blocks);
            assert_eq!(
                blocks.as_slice().len(),
                1,
                "{name}: {:?}",
                blocks.as_slice()
            );
            session.shutdown();
        }
    }

    #[test]
    fn a_bash_prompt_waiting_for_a_command_closes_no_block_without_one() {
        // An empty line and a line cancelled with Ctrl-C read no command: `PS0` never runs, so
        // no `D` is printed for the block they leave — zsh's rule — and the last exit code is
        // still the last command's, not the cancelled line's 130.
        let Some(bash) = bash() else { return };
        let root = TempRoot::new("bash-empty");
        let home = root.0.join("home");
        write_home_file(&home, ".bash_profile", "PS1='$ '\n");
        let session = spawn_bash(&bash, &home, &[]);
        at_the_prompt(&session);
        session.write(b"false\n");
        wait_until("the exit code did not arrive", || {
            session
                .shell_state()
                .is_some_and(|state| state.last_exit == Some(1))
        });
        session.write(b"\n");
        session.write(b"abc\x03");
        // The interrupt flushes what the terminal had queued for the shell, so the next line is
        // typed once the prompt after the cancelled one is on the screen.
        wait_until("the prompt after the cancelled line did not come", || {
            let rows = screen(&session, &mut Blocks::default());
            rows.iter()
                .rposition(|row| row.contains("^C"))
                .is_some_and(|at| rows[at + 1..].iter().any(|row| row.starts_with('$')))
        });
        session.write(b"echo marker-after\n");
        wait_until("the command after the cancelled line did not run", || {
            shown(&session, "marker-after")
                && session
                    .shell_state()
                    .is_some_and(|s| s.last_exit == Some(0))
        });
        // Four prompts have come and gone (`false`, empty, cancelled, echo) but only the two
        // commands that ran have a stripe: the other two blocks never got their `D`.
        let mut blocks = Blocks::default();
        screen(&session, &mut blocks);
        assert_eq!(blocks.as_slice().len(), 2, "{:?}", blocks.as_slice());
        assert_ne!(blocks.as_slice()[0].stripe, blocks.as_slice()[1].stripe);
        session.shutdown();
    }

    #[test]
    fn the_bash_wrappers_ssh_function_asks_the_binary() {
        // `ssh` is the user's until the wrapper is read: a function or an alias of their own
        // wins, and otherwise ours asks the binary (`bateri ssh-argv`, one parser) for the
        // wrapped arguments, NUL-separated, an empty one included.
        let Some(bash) = bash() else { return };
        let root = TempRoot::new("bash-ssh");
        let home = root.0.join("home");
        let bin = root.0.join("bin");
        std::fs::create_dir_all(&bin).expect("bin");
        let log = root.0.join("asked");
        let fall = root.0.join("fall");
        stand_in(
            &bin,
            "bateri",
            &format!(
                "echo \"$*\" >> '{}'\ncase \"$1 $2\" in\n\
                 'ssh-argv --tty') printf -- '-t\\0x\\0y z\\0\\0BOOT\\0' ;;\n\
                 ssh-fell-back*) [ -f '{}' ] && printf -- 'PLAIN\\0x\\0' ;;\nesac\nexit 0\n",
                log.display(),
                fall.display()
            ),
        );
        let code = root.0.join("code");
        stand_in(
            &bin,
            "ssh",
            &format!(
                "for a in \"$@\"; do printf '[%s]' \"$a\"; done; echo\n\
                 [ -f '{0}' ] && exit \"$(cat '{0}')\"\nexit 0\n",
                code.display()
            ),
        );
        let spawn = |rc: &str| {
            // The stand-ins first on `PATH`, after the system's login files (macOS's
            // `path_helper` rewrites it for a login shell).
            write_home_file(
                &home,
                ".bash_profile",
                &format!("PATH={}:$PATH\nPS1='$ '\n{rc}", bin.display()),
            );
            spawn_bash(
                &bash,
                &home,
                &[
                    ("BATERI_BIN", bin.join("bateri").display().to_string()),
                    ("BATERI_SSH_INSTANCE", "0a1b2c3d".to_owned()),
                    ("BATERI_SSH_CWD", "/srv/my app".to_owned()),
                ],
            )
        };

        let session = spawn("");
        at_the_prompt(&session);
        session.write(
            b"ssh x 'y z'; ssh x | cat; echo \"leak<${BATERI_BIN-}${BATERI_SSH_INSTANCE-}${BATERI_SSH_CWD-}>\"\n",
        );
        wait_until("the wrapped call did not run", || {
            shown(&session, "[-t][x][y z][][BOOT]") && shown(&session, "leak<>")
        });
        let asked = std::fs::read_to_string(&log).expect("the binary was asked");
        // `--block` is the command's own block: the first prompt's; the folder went with the
        // first call and not the second; the wrapped call asked the fallback with its arguments.
        assert_eq!(
            asked,
            "ssh-argv --tty --block 1 --instance 0a1b2c3d --cwd /srv/my app -- x y z\n\
             ssh-fell-back --rc 0 --instance 0a1b2c3d -- -t x y z  BOOT\n\
             ssh-argv --block 1 --instance 0a1b2c3d -- x\n"
        );
        std::fs::remove_file(&log).expect("log");

        // A fallback answer is rerun plain; the code is the rerun's.
        std::fs::write(&fall, "").expect("fall");
        std::fs::write(&code, "7").expect("code");
        session.write(b"ssh x; echo \"rc=$?\"\n");
        wait_until("the plain rerun did not run", || {
            shown(&session, "[PLAIN][x]") && shown(&session, "rc=7")
        });
        // Inside a multiplexer nothing is wrapped.
        std::fs::remove_file(&fall).expect("fall");
        std::fs::write(&code, "255").expect("code");
        session.write(b"export TMUX=t; ssh x; echo \"r2=$?\"\n");
        wait_until("the tmux call did not run", || shown(&session, "r2=255"));
        session.shutdown();

        // A user's own function or alias wins: ours is not defined over it.
        for (rc, shown_text) in [
            ("ssh() { echo USER-SSH; }\n", "USER-SSH"),
            ("alias ssh='echo ALIAS-SSH'\n", "ALIAS-SSH"),
        ] {
            let session = spawn(rc);
            at_the_prompt(&session);
            session.write(b"ssh x\n");
            wait_until("the user's ssh did not run", || shown(&session, shown_text));
            session.shutdown();
        }

        // Without the binary's path there is no function at all.
        write_home_file(&home, ".bash_profile", "PS1='$ '\n");
        let session = spawn_bash(&bash, &home, &[]);
        at_the_prompt(&session);
        session.write(b"echo \"kind=$(type -t ssh)\"\n");
        wait_until("the kind was not shown", || {
            shown(&session, "kind=file") || shown(&session, "kind=\n")
        });
        assert!(
            !shown(&session, "kind=function"),
            "ssh was wrapped without the binary"
        );
        session.shutdown();
    }

    /// A bash session with the dock and a prompt the user's files write: `PS1` set once and
    /// rewritten by a `PROMPT_COMMAND` of the kind a prompt theme installs. Both strings are
    /// unique, so that a claim "it is not drawn" cannot be fooled by other text.
    fn spawn_bash_dock_with_a_theme(bash: &Bash, root: &TempRoot) -> Session {
        let home = root.0.join("home");
        write_home_file(
            &home,
            ".bash_profile",
            "PS1='USERPS1XY> '\nPROMPT_COMMAND='PS1=\"THEMEXY> \"'\n",
        );
        let session = spawn_bash_dock(bash, &home, &[]);
        at_the_prompt(&session);
        session
    }

    #[test]
    fn the_bash_dock_shows_the_line_readline_draws() {
        // The real test of the bash dock: a real bash draws the prompt the script gave it and
        // the terminal reads the line back from the grid. In one round:
        //
        // 1. Neither the user's prompt nor the one their theme writes at every prompt is drawn:
        //    the prompt is the terminal's, and the typed line is in the dock with the caret
        //    after it.
        // 2. The grid does not show the line a second time — a line that wraps included, and
        //    after a resize that repaints the prompt.
        // 3. After Enter the command is a block on the grid and the next prompt is a dock again.
        // 4. A line cancelled with Ctrl-C stays on the grid as bash leaves it (readline echoes
        //    `^C`, it does not erase the line as zsh does) and the next prompt is a dock again.
        let Some(bash) = bash() else { return };
        let root = TempRoot::new("bash-dock");
        let session = spawn_bash_dock_with_a_theme(&bash, &root);
        let blocks = std::cell::RefCell::new(Blocks::default());
        let rows = || screen(&session, &mut blocks.borrow_mut());

        wait_until("the first prompt was not a dock", || {
            let dock = read_dock(&session, &blocks);
            dock.status == DockStatus::Live && dock.buffer.is_empty()
        });
        // 1.
        session.write(b"echo su");
        wait_until("the dock did not show the typed line", || {
            let dock = read_dock(&session, &blocks);
            dock.status == DockStatus::Live && dock.buffer == "echo su"
        });
        let dock = read_dock(&session, &blocks);
        assert_eq!(dock.cursor, 7, "the caret is after the line: {dock:?}");
        assert!(
            dock.postdisplay.is_empty(),
            "bash has no suggestion: {dock:?}"
        );
        let drawn = rows().join("\n");
        assert!(
            !drawn.contains("USERPS1XY") && !drawn.contains("THEMEXY"),
            "the user's prompt was drawn on the grid:\n{drawn}"
        );
        // 2.
        assert!(
            !drawn.contains("echo su"),
            "the typed line was drawn on the grid too:\n{drawn}"
        );
        session.write("x".repeat(100).as_bytes());
        wait_until("the dock did not show the wrapped line", || {
            let dock = read_dock(&session, &blocks);
            dock.status == DockStatus::Live
                && dock.buffer.ends_with("xxxxxxxxxx")
                && dock.buffer.len() == 107
        });
        for cols in [50, 80] {
            let _ = session.resize(cols, 20, (9, 18));
            wait_until("the dock lost the line in a resize", || {
                let dock = read_dock(&session, &blocks);
                dock.status == DockStatus::Live && dock.buffer.len() == 107
            });
            let drawn = rows().join("\n");
            assert!(
                !drawn.contains("xxxxxxxxxx") && !drawn.contains("THEMEXY"),
                "the line was drawn on the grid at {cols} columns:\n{drawn}"
            );
        }
        session.write("\u{8}".repeat(100).as_bytes());
        wait_until("the dock did not show the shortened line", || {
            read_dock(&session, &blocks).buffer == "echo su"
        });

        // 3.
        session.write(b"\n");
        wait_until("the command did not finish", || {
            session
                .shell_state()
                .is_some_and(|state| state.phase == ShellPhase::Input && state.last_exit == Some(0))
                && rows().iter().filter(|row| row.contains("echo su")).count() == 1
                && rows().iter().any(|row| row == "su")
        });
        let mut now = Blocks::default();
        screen(&session, &mut now);
        assert_eq!(
            now.as_slice().len(),
            1,
            "the command's anchor was not on the grid: {:?}",
            now.as_slice()
        );
        wait_until("the next prompt was not a dock", || {
            let dock = read_dock(&session, &blocks);
            dock.status == DockStatus::Live && dock.buffer.is_empty()
        });

        // 4.
        session.write(b"abc");
        wait_until("the dock did not show the line to cancel", || {
            read_dock(&session, &blocks).buffer == "abc"
        });
        session.write(b"\x03");
        wait_until("the prompt after the cancelled line was not a dock", || {
            let dock = read_dock(&session, &blocks);
            dock.status == DockStatus::Live
                && dock.buffer.is_empty()
                && rows().iter().any(|row| row.contains("abc^C"))
        });
        session.write(b"echo after\n");
        wait_until("the command after the cancelled line did not run", || {
            rows().iter().any(|row| row == "after")
        });
        session.shutdown();
    }

    #[test]
    fn the_bash_dock_is_edited_by_clicks_and_selections() {
        // The widget end to end: the terminal's edit command goes to a real readline, which
        // moves its caret or deletes a range, and the dock reads the result back from the grid.
        // The line has a letter of two bytes, so that a unit mistake (bytes for characters)
        // moves the caret to the wrong place.
        let Some(bash) = bash() else { return };
        let root = TempRoot::new("bash-dock-edit");
        let session = spawn_bash_dock_with_a_theme(&bash, &root);
        let blocks = std::cell::RefCell::new(Blocks::default());
        let rows = || screen(&session, &mut blocks.borrow_mut());
        let dock = || read_dock(&session, &blocks);
        let caret = || dock().cursor;

        // A command first, so that `$?` has a value to lose.
        session.write(b"false\n");
        wait_until("the first command did not finish", || {
            session
                .shell_state()
                .is_some_and(|state| state.phase == ShellPhase::Input && state.last_exit == Some(1))
        });
        session.write("echo \"rc=$?\" ğüş end".as_bytes());
        wait_until("the dock did not show the typed line", || {
            let dock = dock();
            dock.status == DockStatus::Live && dock.buffer == "echo \"rc=$?\" ğüş end"
        });
        let len = dock().buffer.chars().count();
        assert_eq!(caret(), len);
        // The widget is bound: a click moves the caret to the character clicked, in characters.
        for index in [3usize, 14, 0, 15] {
            click_dock(&session, &blocks, index as u16);
            wait_until("the caret did not follow the click", || caret() == index);
        }
        assert_eq!(dock().buffer, "echo \"rc=$?\" ğüş end", "the click edited");
        let drawn = rows().join("\n");
        assert!(
            !drawn.contains("ğüş") && !drawn.contains("THEMEXY"),
            "the widget's redraw left the line or the theme's prompt on the grid:\n{drawn}"
        );

        // Typing goes where the caret is.
        session.write(b"Z");
        wait_until("the typed character did not land at the caret", || {
            dock().buffer == "echo \"rc=$?\" ğüZş end"
        });
        // A selection is deleted by the key, and the caret lands where it started.
        draw_dock(&session, &blocks);
        let point = |index: u16| bt_core::SelectionPoint {
            col: bt_core::DOCK_TEXT_COL + index,
            row: 0,
            half: bt_core::CellHalf::Left,
        };
        session.dock_select(bt_core::SelectKind::Simple, point(1));
        session.dock_drag(point(5));
        assert!(
            session.dock_key(bt_core::DockKey::Backspace),
            "the gate was shut"
        );
        wait_until("the selection was not deleted", || {
            dock().buffer == "e\"rc=$?\" ğüZş end"
        });
        wait_until("the caret did not land at the selection's start", || {
            caret() == 1
        });
        // Typing over a selection replaces it.
        draw_dock(&session, &blocks);
        session.dock_select(bt_core::SelectKind::Simple, point(0));
        session.dock_drag(point(1));
        // The window deletes the selection first and then types by the usual path.
        assert!(session.dock_delete_selection(), "the gate was shut");
        session.write(b"X");
        wait_until("the selection was not replaced", || {
            dock().buffer == "X\"rc=$?\" ğüZş end"
        });

        // A payload that is stale (`L` is not the line's length), a refresh and a corrupt one
        // change nothing.
        let before = dock();
        for payload in [
            &b"\x1b[8133~d;0;0;3\x07"[..],
            b"\x1b[8133~r\x07",
            b"\x1b[8133~d;x;1;2\x07",
        ] {
            session.write(payload);
        }
        std::thread::sleep(std::time::Duration::from_millis(700));
        let after = dock();
        assert_eq!(
            (after.buffer.as_str(), after.cursor),
            (before.buffer.as_str(), before.cursor),
            "a payload the widget should refuse changed the line"
        );
        // And the line still runs with the status of the command before it (the widget
        // hands `$?` back).
        session.write(b"\x01\x0b");
        session.write(b"echo \"rc=$?\"\n");
        wait_until("the command did not run", || {
            rows().iter().any(|row| row == "rc=1")
        });
        session.shutdown();
    }

    #[test]
    fn the_bash_widget_only_follows_clicks_in_vi_command_mode() {
        // A deletion the terminal sends is followed by the key the user pressed, and in vi's
        // command mode that key is a command: it would run on a line that had changed under
        // it. There the widget moves the caret and leaves the text alone. (An unbound sequence
        // would be worse: `8133~` is a count and a case toggle.)
        let Some(bash) = bash() else { return };
        let root = TempRoot::new("bash-dock-vi");
        let home = root.0.join("home");
        write_home_file(&home, ".bash_profile", "set -o vi\n");
        let session = spawn_bash_dock(&bash, &home, &[]);
        at_the_prompt(&session);
        let blocks = std::cell::RefCell::new(Blocks::default());
        let dock = || read_dock(&session, &blocks);

        session.write(b"echo Hello World");
        wait_until("the dock did not show the typed line", || {
            dock().buffer == "echo Hello World"
        });
        // Insert mode: a selection is deleted.
        draw_dock(&session, &blocks);
        let point = |index: u16| bt_core::SelectionPoint {
            col: bt_core::DOCK_TEXT_COL + index,
            row: 0,
            half: bt_core::CellHalf::Left,
        };
        session.dock_select(bt_core::SelectKind::Simple, point(10));
        session.dock_drag(point(16));
        assert!(session.dock_delete_selection());
        wait_until("insert mode did not delete", || {
            dock().buffer == "echo Hello"
        });

        // Command mode (Escape, then readline waits out its key sequence timeout).
        session.write(b"\x1b");
        std::thread::sleep(std::time::Duration::from_millis(900));
        click_dock(&session, &blocks, 2);
        wait_until("the caret did not follow the click", || dock().cursor == 2);
        draw_dock(&session, &blocks);
        session.dock_select(bt_core::SelectKind::Simple, point(0));
        session.dock_drag(point(4));
        assert!(session.dock_delete_selection());
        std::thread::sleep(std::time::Duration::from_millis(700));
        assert_eq!(
            dock().buffer,
            "echo Hello",
            "command mode deleted a selection"
        );
        session.shutdown();
    }

    #[test]
    fn the_bash_marks_do_not_depend_on_promptvars() {
        // `PS0`'s flag is a parameter expansion, and a user may turn `promptvars` off: the text
        // must not be printed before every command, and the block still closes.
        let Some(bash) = bash() else { return };
        let root = TempRoot::new("bash-promptvars");
        let home = root.0.join("home");
        write_home_file(&home, ".bash_profile", "PS1='$ '\nshopt -u promptvars\n");
        let session = spawn_bash(&bash, &home, &[]);
        at_the_prompt(&session);
        session.write(b"echo ran\n");
        wait_until("the command did not finish", || {
            shown(&session, "ran")
                && session.shell_state().is_some_and(|state| {
                    state.phase == ShellPhase::Input && state.last_exit == Some(0)
                })
        });
        let rows = screen(&session, &mut Blocks::default());
        assert!(
            !rows.iter().any(|row| row.contains("__bateri")),
            "the flag was printed: {rows:?}"
        );
        let mut blocks = Blocks::default();
        screen(&session, &mut blocks);
        assert_eq!(blocks.as_slice().len(), 1, "{:?}", blocks.as_slice());
        session.shutdown();
    }

    #[test]
    fn the_bash_blocks_setting_keeps_the_prompt_and_opens_no_dock() {
        // `BATERI_DOCK=off`: the user's prompt is drawn with the marks around it, the line stays
        // on the grid and the widget is never bound (no `w`, nothing to answer a click).
        let Some(bash) = bash() else { return };
        let root = TempRoot::new("bash-blocks");
        let home = root.0.join("home");
        write_home_file(&home, ".bash_profile", "PS1='USERPS1XY> '\n");
        let session = spawn_bash(&bash, &home, &[]);
        at_the_prompt(&session);
        let blocks = std::cell::RefCell::new(Blocks::default());
        session.write(b"echo su");
        wait_until("the line was not drawn on the grid", || {
            screen(&session, &mut blocks.borrow_mut())
                .iter()
                .any(|row| row.contains("USERPS1XY> echo su"))
        });
        assert_eq!(read_dock(&session, &blocks).status, DockStatus::Idle);
        assert!(!session.can_edit_dock());
        session.write(b"\x03");
        session.shutdown();
    }

    // ── every shell ──────────────────────────────────────────────────────

    /// A command turns on what a TUI asks for — mouse reports, in pixels, focus reports, kitty's
    /// key forms — and ends without turning it off, the way a program killed or cut off by a
    /// dropped ssh does. At the next prompt the script has turned it off: the pointer is the
    /// terminal's again, and no report reaches a line editor that would take it for typed
    /// text.
    fn a_command_takes_its_input_modes_with_it(session: &Session) {
        at_the_prompt(session);
        session.write(
            b"printf '\\033[?1003h\\033[?1006h\\033[?1004h\\033[?1016h\\033[>1u'; sleep 1\r",
        );
        // Pixel reports are the one mode read without the terminal's lock, so they say the
        // command ran without a report being sent to anyone.
        wait_until("the command did not turn the modes on", || {
            session.pixel_mouse()
        });
        wait_until("the command did not end", || {
            session
                .shell_state()
                .is_some_and(|state| state.phase == ShellPhase::Input && state.last_exit == Some(0))
        });
        // The report's route is asked under the terminal's lock, which waits for the bytes
        // the marks came with to be parsed; the pixel flag only after it.
        let at = SelectionPoint {
            col: 0,
            row: 0,
            half: CellHalf::Left,
        };
        assert_eq!(
            session.mouse_motion(None, at, MouseModifiers::default()),
            Click::Ignored
        );
        assert!(!session.pixel_mouse());
    }

    #[test]
    fn a_zsh_command_takes_its_input_modes_with_it() {
        let root = TempRoot::new("zsh-modes");
        let home = root.0.join("home");
        std::fs::create_dir_all(&home).expect("could not set up the fake home directory");
        let wrapper = copy_wrapper(&root.0);
        let env = HashMap::from([
            ("HOME".to_owned(), home.display().to_string()),
            ("ZDOTDIR".to_owned(), wrapper.display().to_string()),
            ("BATERI_DOCK".to_owned(), "off".to_owned()),
        ]);
        let session = Session::spawn(
            shell_options(
                (
                    "/bin/zsh".to_owned(),
                    vec!["-l".to_owned(), "-i".to_owned()],
                ),
                &home,
                env,
                false,
            ),
            Arc::new(SilentWake),
        )
        .expect("could not open the session");
        a_command_takes_its_input_modes_with_it(&session);
        session.shutdown();
    }

    #[test]
    fn a_bash_command_takes_its_input_modes_with_it() {
        let Some(bash) = bash() else { return };
        let root = TempRoot::new("bash-modes");
        let home = root.0.join("home");
        std::fs::create_dir_all(&home).expect("could not set up the fake home directory");
        let session = spawn_bash(&bash, &home, &[]);
        a_command_takes_its_input_modes_with_it(&session);
        session.shutdown();
    }

    #[test]
    fn a_fish_command_takes_its_input_modes_with_it() {
        let Some(fish) = fish() else { return };
        let root = TempRoot::new("fish-modes");
        let home = root.0.join("home");
        let session = spawn_fish(&fish, &home, None, &[("BATERI_DOCK", "off".to_owned())]);
        a_command_takes_its_input_modes_with_it(&session);
        session.shutdown();
    }

    /// A command names the window and ends without naming it back, the way a program drawn in
    /// the terminal or a killed TUI does. At the next prompt the tab has the name it had before
    /// the command, not the program's.
    fn a_command_takes_its_title_with_it(session: &Session) {
        at_the_prompt(session);
        let before = session.title();
        session.write(b"printf '\\033]2;Todo\\007'; sleep 1\r");
        wait_until("the command did not name the window", || {
            session.title() == "Todo"
        });
        wait_until("the command did not end", || {
            session
                .shell_state()
                .is_some_and(|state| state.phase == ShellPhase::Input && state.last_exit == Some(0))
        });
        wait_until("the command's title stayed on the tab", || {
            session.title() == before
        });
    }

    /// zsh with our wrapper and the user's files in `home`, no dock.
    fn spawn_zsh(root: &TempRoot, home: &Path) -> Session {
        let wrapper = copy_wrapper(&root.0);
        let env = HashMap::from([
            ("HOME".to_owned(), home.display().to_string()),
            ("ZDOTDIR".to_owned(), wrapper.display().to_string()),
            ("BATERI_DOCK".to_owned(), "off".to_owned()),
        ]);
        Session::spawn(
            shell_options(
                (
                    "/bin/zsh".to_owned(),
                    vec!["-l".to_owned(), "-i".to_owned()],
                ),
                home,
                env,
                false,
            ),
            Arc::new(SilentWake),
        )
        .expect("could not open the session")
    }

    #[test]
    fn a_zsh_command_takes_its_title_with_it() {
        let root = TempRoot::new("zsh-title");
        let home = root.0.join("home");
        std::fs::create_dir_all(&home).expect("could not set up the fake home directory");
        let session = spawn_zsh(&root, &home);
        a_command_takes_its_title_with_it(&session);
        session.shutdown();
    }

    #[test]
    fn a_zsh_prompt_that_names_the_window_keeps_the_last_word() {
        // oh-my-zsh's shape: a precmd hook of the user's names the window after the directory.
        // The command moves to another directory and names the window itself; at the prompt
        // the user's hook has the last word — the new directory's name, not the name from
        // before the command, which is what putting the old title back behind the user's
        // hooks would leave.
        let root = TempRoot::new("zsh-title-hook");
        let home = root.0.join("home");
        std::fs::create_dir_all(home.join("sub")).expect("could not set up the fake home");
        std::fs::write(
            home.join(".zshrc"),
            "autoload -Uz add-zsh-hook\n\
             __user_title() { print -nr -- $'\\e]2;user:'${PWD:t}$'\\a' }\n\
             add-zsh-hook precmd __user_title\n",
        )
        .expect(".zshrc not written");
        let session = spawn_zsh(&root, &home);
        at_the_prompt(&session);
        wait_until("the user's hook did not name the window", || {
            session.title() == "user:home"
        });
        session.write(b"cd sub; printf '\\033]2;Todo\\007'; sleep 1\r");
        wait_until("the command did not name the window", || {
            session.title() == "Todo"
        });
        wait_until("the user's hook did not have the last word", || {
            session.title() == "user:sub"
        });
        // And it stays: nothing after the hook puts an older name back.
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert_eq!(session.title(), "user:sub");
        session.shutdown();
    }

    #[test]
    fn a_bash_command_takes_its_title_with_it() {
        let Some(bash) = bash() else { return };
        let root = TempRoot::new("bash-title");
        let home = root.0.join("home");
        std::fs::create_dir_all(&home).expect("could not set up the fake home directory");
        let session = spawn_bash(&bash, &home, &[]);
        a_command_takes_its_title_with_it(&session);
        session.shutdown();
    }

    #[test]
    fn a_fish_command_takes_its_title_with_it() {
        let Some(fish) = fish() else { return };
        let root = TempRoot::new("fish-title");
        let home = root.0.join("home");
        let session = spawn_fish(&fish, &home, None, &[("BATERI_DOCK", "off".to_owned())]);
        a_command_takes_its_title_with_it(&session);
        session.shutdown();
    }
}
