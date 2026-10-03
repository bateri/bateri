//! Loader for the settings and theme files: reads `settings.toml` and `themes/{name}.toml` in
//! the root directory and hands them to `bt-core`'s pure parsers; resolving a theme **name**
//! to a theme also happens here ([`load_theme`]).
//!
//! The root is a **parameter**: `$HOME/.config/bateri/` in production ([`config_root`]), a
//! temporary directory in tests — no test reads the real `HOME`. The timed run never calls the
//! loader; the only place of that branch is `app::Inputs`.
//!
//! The result carries four distinct states and is **not collapsed** into
//! [`Settings::default`]: "no file" is a silent and correct state, while "could not be parsed"
//! requires applying nothing on a live reload and turning OSC 52 off at launch. What
//! to do in which state is the caller's rule; the launch rule is [`Loaded::at_launch`], the
//! save-time rule [`Loaded::live`]. The two rules for a theme name are the same pair:
//! [`ThemeLoaded::or_embedded`], [`ThemeLoaded::or_current`]. The watched paths also come from
//! here ([`watched_paths`], [`theme_path`]): so that the path read and the path watched do not
//! diverge.
//!
//! **Known limit — the read happens on the main thread and waits without a bound.** A path
//! that is not a regular file (a FIFO, a link to `/dev/zero`, a directory) is filtered out
//! before reading; but a link to a file evicted from iCloud Drive or a hung network home
//! directory can stall the launch. The settings file is small and local; moving this to a
//! separate thread would make the launch order (settings → geometry → session) asynchronous
//! and was not worth the cost.

use std::io;
use std::path::{Path, PathBuf};

use bt_core::{Diagnostic, Parsed, Settings, SettingsEdit, Theme};

/// The settings file's name; diagnostic texts also refer to it by this name for the user.
pub const FILE_NAME: &str = "settings.toml";

/// The directory of user themes, under the root.
const THEMES_DIR: &str = "themes";

/// The root in production: `{home}/.config/bateri/`.
///
/// Not macOS's `~/Library/Application Support`: the file is edited by hand and the documented
/// contract names this path explicitly.
pub fn config_root(home: &Path) -> PathBuf {
    home.join(".config").join("bateri")
}

/// The root's watched paths (`watch`): the root itself, `themes/` and `settings.toml`. The
/// active theme file is separate ([`theme_path`]): its name derives from the settings and the
/// appearance, it is set up and refreshed separately.
///
/// The root and `themes/`, as directories, report a file being created, deleted or moved over;
/// `settings.toml`, as a file, reports an in-place write and a save at the link's target. A
/// path that does not exist creates no source.
pub fn watched_paths(root: &Path) -> [PathBuf; 3] {
    [
        root.to_path_buf(),
        root.join(THEMES_DIR),
        root.join(FILE_NAME),
    ]
}

/// A user theme's path relative to the root — the diagnostic text also refers to the file by
/// this name.
fn theme_file(name: &str) -> String {
    format!("{THEMES_DIR}/{name}.toml")
}

/// `{root}/themes/{name}.toml`: the file [`load_theme`] reads and the watcher sets up for the
/// active theme. If an embedded theme is selected there is no file and no source is set up.
pub fn theme_path(root: &Path, name: &str) -> PathBuf {
    root.join(theme_file(name))
}

/// The first half of "Settings…": creates the root directory and `settings.toml` with
/// [`Settings::TEMPLATE`] if they are missing, returns the file's path.
///
/// **Never overwrites what exists** — not a broken file, not a symbolic link, not a link
/// without a target (`create_new`, `O_EXCL`: does not follow the link). A broken file's
/// content is the user's unfinished work; creating a dangling link's target would leave a
/// stray file where the dotfile repository moved from, and the settings slot already reports
/// that link.
pub fn create_if_missing(root: &Path) -> io::Result<PathBuf> {
    std::fs::create_dir_all(root)?;
    let path = root.join(FILE_NAME);
    let created = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path);
    match created {
        Ok(mut file) => {
            use std::io::Write as _;
            file.write_all(Settings::TEMPLATE.as_bytes())?;
        }
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
        Err(err) => return Err(err),
    }
    Ok(path)
}

/// Writing a single key — View ▸ Theme ▸ and the settings window: changes `edit`'s key in
/// `{root}/settings.toml` ([`Settings::with_edit`]); the error is the write slot's message.
/// The message says what was not saved: the theme choice by its name in the menu ("the
/// theme"), every other key "the setting".
///
/// **Only writes, does not apply** — what applies is the path that reads the file, the
/// watcher's path (`app`).
///
/// - The file is read **at that moment**: a copy held on hand would overwrite a save made in
///   the editor.
/// - If the file does not exist, the "Settings…" path ([`create_if_missing`]): the template,
///   with the key on top. A dangling link's target is not created; the read reports it.
/// - Written **in place** (`O_TRUNC`): the symbolic link is followed, the target is updated. A
///   temporary file + rename would turn the link into a regular file; and there is no race it
///   would guard against, reading and writing are on the same main queue. The watcher treats
///   the empty file between truncation and write as no file ([`load_keeping`]).
/// - A file that cannot be read or parsed is **not written**: its content is the user's
///   unfinished work.
///
/// **Known limit — between truncation and write.** `write` does a single (under 1 KB) write
/// to the truncated file; if that write fails or the process dies at exactly that moment the
/// file is left empty or half written. A temporary file + rename next to the resolved target
/// would close this window, but it breaks hard links, drops permissions and extended
/// attributes and fails in an unwritable directory; writing in place is the deliberate choice.
pub fn write_edit(root: &Path, edit: &SettingsEdit) -> Result<(), String> {
    let subject = match edit {
        SettingsEdit::Theme(_) => "theme",
        _ => "setting",
    };
    let not_saved = |reason: String| format!("{reason}; the {subject} was not saved");
    let path = create_if_missing(root)
        .map_err(|err| format!("{FILE_NAME} could not be created: {err}"))?;
    let text = match read_text(&path) {
        Text::Read(text) => text,
        Text::Unreadable(err) => {
            return Err(not_saved(format!("{FILE_NAME} could not be read: {err}")));
        }
        // Deleted between creation and reading.
        Text::Missing => return Err(not_saved(format!("{FILE_NAME} was removed"))),
    };
    let written = Settings::with_edit(&text, edit).map_err(|d| not_saved(notice(&d)))?;
    std::fs::write(&path, written).map_err(|err| format!("{FILE_NAME} could not be written: {err}"))
}

/// The user themes in View ▸ Theme ▸: the names of `{root}/themes/*.toml`, in
/// case-insensitive order.
///
/// What cannot be selected is not listed: anything that is not a regular file (a directory, a
/// dangling link), a non-UTF-8 name and a name starting with a dot (a hidden file, an
/// AppleDouble `._x.toml` copied from another file system), the reserved
/// [`SYSTEM_THEME`](bt_core::SYSTEM_THEME) and the name of an embedded theme — that file
/// shadows the embedded one and selecting it is the same as the embedded name's item.
///
/// If the directory does not exist or cannot be read the list is empty: the menu has no place
/// to show an error, and a user without a themes directory is the ordinary case.
pub fn user_theme_names(root: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(root.join(THEMES_DIR)) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let stem = name.strip_suffix(".toml")?;
            // The name is checked, not the stem: a file named exactly `.toml` has an empty stem.
            let selectable = !name.starts_with('.')
                && stem != bt_core::SYSTEM_THEME
                && Theme::embedded(stem).is_none()
                // Follows the link: a linked theme file is selectable too.
                && std::fs::metadata(entry.path()).is_ok_and(|meta| meta.is_file());
            selectable.then(|| stem.to_owned())
        })
        .collect();
    names.sort_by_cached_key(|name| name.to_lowercase());
    names
}

/// The result of a read.
#[derive(Debug)]
pub enum Loaded {
    /// The file is missing or empty: the user never wrote any settings. **No** diagnostic.
    Missing,
    /// The file exists but could not be read: permissions, not a regular file (a directory, a
    /// FIFO), a broken symbolic link, non-UTF-8 content.
    Unreadable(io::Error),
    /// Read, but could not be parsed as TOML.
    Unparseable(Diagnostic),
    /// Parsed; the diagnostics are the rejected keys. Boxed: the settings
    /// record dwarfs the other variants (the remote file keys tipped
    /// `clippy::large_enum_variant`).
    Parsed(Box<Parsed>),
}

/// The reading of a text file — the shared gate of the settings and theme files.
enum Text {
    Missing,
    Unreadable(io::Error),
    Read(String),
}

/// Reads `path`; tells "no file" apart from "could not be read".
fn read_text(path: &Path) -> Text {
    // `metadata` follows the link: the target's type is asked, not the link's.
    match std::fs::metadata(path) {
        // A broken link also yields `NotFound`; but the file shows up in `ls`, and staying
        // silent as "no settings at all" would leave the user searching for why it has no
        // effect (a dotfile manager's moved repository).
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return match std::fs::symlink_metadata(path) {
                Ok(_) => {
                    Text::Unreadable(io::Error::other("symbolic link points to a missing file"))
                }
                Err(_) => Text::Missing,
            };
        }
        Err(err) => return Text::Unreadable(err),
        // A FIFO blocks the read forever, `/dev/zero` exhausts memory.
        Ok(meta) if !meta.is_file() => {
            return Text::Unreadable(io::Error::other("not a regular file"));
        }
        Ok(_) => {}
    }
    match std::fs::read_to_string(path) {
        // Deleted between the check and the read: the no-file state.
        Err(err) if err.kind() == io::ErrorKind::NotFound => Text::Missing,
        Err(err) => Text::Unreadable(err),
        Ok(text) => Text::Read(text),
    }
}

/// Reads `{root}/settings.toml` — the launch read: a rejected value takes its default
/// (except `osc52`, which falls back to off).
pub fn load(root: &Path) -> Loaded {
    load_keeping(root, &Settings::default())
}

/// The save-time read: a rejected value takes `current`'s ([`Settings::parse_keeping`];
/// except `osc52`, which falls back to off). If a `scrollback` saved with the wrong type fell
/// back to the default, it would trim the history irreversibly.
///
/// An empty file (whitespace only) counts as no file. An editor that saves in place first
/// truncates (`O_TRUNC`) and then writes, and the truncation also fires an event: if the empty
/// file read in between applied the default `scrollback`, the history would be trimmed
/// irreversibly. At launch the two were already the same (defaults, no diagnostics).
pub fn load_keeping(root: &Path, current: &Settings) -> Loaded {
    match read_text(&root.join(FILE_NAME)) {
        Text::Missing => Loaded::Missing,
        Text::Read(text) if text.trim().is_empty() => Loaded::Missing,
        Text::Unreadable(err) => Loaded::Unreadable(err),
        Text::Read(text) => match Settings::parse_keeping(&text, current) {
            Ok(parsed) => Loaded::Parsed(Box::new(parsed)),
            Err(diagnostic) => Loaded::Unparseable(diagnostic),
        },
    }
}

/// The resolution of a theme name.
#[derive(Debug)]
pub enum ThemeLoaded {
    /// Found in the user file or among the embedded themes; the messages are the colors rejected
    /// in the file (formatted ready for the subtitle).
    Found(Theme, Vec<String>),
    /// Unusable: the file could not be read or parsed, or the name exists nowhere. A single
    /// message; which theme remains in effect is the caller's rule.
    Failed(String),
}

/// Resolves a theme name: first `{root}/themes/{name}.toml`, then the embedded themes.
///
/// `root` `None` → the home directory could not be resolved, only the embedded ones. A key
/// missing from the file comes from the **base**: in a file shadowing an embedded theme, that
/// theme itself; under another name, `Theme::BATERI` (`docs/SETTINGS.md` → Themes). A user who
/// writes only `accent` into a shadowing `themes/bateri-light.toml` expects the light theme
/// with a red cursor; read from the dark base, it would get a dark background in light mode.
///
/// **If the file exists but is unusable, there is no fallback to the embedded one**: if a
/// broken `themes/bateri.toml` silently opened the embedded `bateri`, the user could not see
/// why their file has no effect. If the file **does not exist**, the embedded one is looked up
/// — that is the unshadowed name.
///
/// **An empty file counts as unusable**, by `settings.toml`'s rule ([`load_keeping`]): an
/// editor that saves in place first truncates the file, and an empty theme read in the middle
/// of the save would be the base itself — the live reload would slam the window to the dark
/// base at that moment. At save time the theme on screen stays ([`ThemeLoaded::or_current`]);
/// at launch the embedded theme matching the appearance comes.
///
/// The name's form (no `/`, not empty) was already checked in `bt-core`; it is not rechecked
/// here, a name that does not come from `Settings` is never passed to this function.
pub fn load_theme(root: Option<&Path>, name: &str) -> ThemeLoaded {
    if let Some(root) = root {
        let file = theme_file(name);
        match read_text(&root.join(&file)) {
            Text::Missing => {}
            Text::Unreadable(err) => {
                return ThemeLoaded::Failed(format!("{file} could not be read: {err}"));
            }
            Text::Read(text) if text.trim().is_empty() => {
                return ThemeLoaded::Failed(format!("{file} is empty"));
            }
            Text::Read(text) => {
                let base = Theme::embedded(name).unwrap_or(Theme::BATERI);
                return match Theme::parse(&text, &base) {
                    Ok((theme, diagnostics)) => ThemeLoaded::Found(
                        theme,
                        diagnostics.iter().map(|d| format!("{file}: {d}")).collect(),
                    ),
                    Err(diagnostic) => ThemeLoaded::Failed(format!("{file}: {diagnostic}")),
                };
            }
        }
    }
    match Theme::embedded(name) {
        Some(theme) => ThemeLoaded::Found(theme, Vec::new()),
        None => ThemeLoaded::Failed(format!("theme \"{name}\" not found")),
    }
}

impl ThemeLoaded {
    /// The rule for the moment a theme is chosen **for an appearance** — launch and appearance
    /// change: in place of an unusable theme, the embedded theme matching the appearance
    /// (`bateri` in dark, `bateri-light` in light) and a message saying so.
    ///
    /// The fallback is the theme a user without a file would see in that appearance
    /// (`Settings::default().theme_for(dark)`): opening a dark window for a user who mistyped
    /// their `light_theme` in light mode would compound the error with a second surprise. On an
    /// appearance change one cannot say "the theme on screen stays" — the theme on screen is
    /// the other appearance's theme (with `dark_theme` broken, a window switching from light to
    /// dark would stay light). The "theme on screen stays" rule belongs to the live reload,
    /// where a file was saved while the appearance is unchanged ([`ThemeLoaded::or_current`]).
    pub fn or_embedded(self, dark: bool) -> (Theme, Vec<String>) {
        match self {
            ThemeLoaded::Found(theme, messages) => (theme, messages),
            ThemeLoaded::Failed(message) => {
                let defaults = Settings::default();
                let name = defaults.theme_for(dark);
                // Both default names are embedded (`Theme::embedded`'s
                // test); `BATERI` only if that table breaks.
                let theme = Theme::embedded(name).unwrap_or(Theme::BATERI);
                (theme, vec![format!("{message}; using {name}")])
            }
        }
    }

    /// The live reload's rule — the appearance is the same, a file was saved: an unusable
    /// theme is **not swapped in** (`None`), the theme on screen stays and the message says so.
    ///
    /// Falling back to the embedded theme matching the appearance ([`ThemeLoaded::or_embedded`])
    /// would punish editing here: a half-saved theme file, or a name still incomplete while
    /// being typed, would slam the window to the embedded theme and back on every save. The
    /// rule is the same when the name changes in the settings file — both are moments of
    /// editing. On the next launch the appearance's rule applies.
    pub fn or_current(self) -> (Option<Theme>, Vec<String>) {
        match self {
            ThemeLoaded::Found(theme, messages) => (Some(theme), messages),
            ThemeLoaded::Failed(message) => {
                (None, vec![format!("{message}; keeping the current theme")])
            }
        }
    }
}

/// A diagnostic's form in the subtitle and on stderr; in one place so that both branches and
/// the live reload use the same form.
pub fn notice(diagnostic: &Diagnostic) -> String {
    format!("{FILE_NAME}: {diagnostic}")
}

/// The file's state as the settings window sees it — and the **source** of the
/// text in the subtitle's settings slot ([`FileState::notices`]): the window's banner and the
/// subtitle say the same sentence, two texts are not produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileState {
    /// The file is missing or empty: controls enabled, the first change creates the file.
    Missing,
    /// Could not be read or parsed: writing will be refused, the window is locked.
    /// The text is exactly the subtitle's.
    Locked(String),
    /// Parsed; the diagnostics are the rejected keys (with their keys — the
    /// window maps them to their own rows).
    Usable(Vec<Diagnostic>),
}

impl FileState {
    /// The messages to go into the subtitle's settings slot.
    pub fn notices(&self) -> Vec<String> {
        match self {
            FileState::Missing => Vec::new(),
            FileState::Locked(reason) => vec![reason.clone()],
            FileState::Usable(diagnostics) => diagnostics.iter().map(notice).collect(),
        }
    }
}

impl Loaded {
    /// The read's state as it goes to the window and the subtitle; which settings get applied
    /// is a separate question ([`Loaded::at_launch`], [`Loaded::live`]).
    pub fn state(&self) -> FileState {
        match self {
            Loaded::Missing => FileState::Missing,
            Loaded::Unreadable(err) => {
                FileState::Locked(format!("{FILE_NAME} could not be read: {err}"))
            }
            Loaded::Unparseable(diagnostic) => FileState::Locked(notice(diagnostic)),
            Loaded::Parsed(parsed) => FileState::Usable(parsed.diagnostics.clone()),
        }
    }

    /// The launch rule: the settings to use and the diagnostics to go to the subtitle.
    ///
    /// For a file that cannot be read or parsed, **the defaults** with OSC 52 off
    /// ([`Settings::for_unusable_file`]): the window must still open, a broken file must not
    /// lock up the terminal, but the clipboard must not fall open just because the file's
    /// `osc52 = "off"` could not be read. If there is no file, the plain defaults. In a parsed
    /// file every key already got its own value or its default.
    pub fn at_launch(self) -> (Settings, Vec<String>) {
        let notices = self.state().notices();
        let settings = match self {
            Loaded::Missing => Settings::default(),
            Loaded::Unreadable(_) | Loaded::Unparseable(_) => Settings::for_unusable_file(),
            Loaded::Parsed(parsed) => parsed.settings,
        };
        (settings, notices)
    }

    /// The live reload's rule: the settings to apply (`None` → **nothing** is applied, the
    /// current settings stay) and the settings slot's diagnostics.
    ///
    /// - **A file that cannot be parsed or read** → `None` + diagnostic. A half-finished save
    ///   (a missing quote) does not break the screen; the corrected save is applied. This is the
    ///   difference from launch: there, opening with the defaults is mandatory.
    /// - **No file** → `None`, no diagnostic. Editors often save by "move the old one aside,
    ///   write the new one" (vim's backup) and the path is briefly absent in between; applying
    ///   the defaults would slam the window on every save. An empty file is also this branch
    ///   ([`load_keeping`]). The cost: a user who really deletes or empties the file sees the
    ///   defaults on the next launch (`docs/SETTINGS.md`).
    pub fn live(self) -> (Option<Settings>, Vec<String>) {
        let notices = self.state().notices();
        let settings = match self {
            Loaded::Missing | Loaded::Unreadable(_) | Loaded::Unparseable(_) => None,
            Loaded::Parsed(parsed) => Some(parsed.settings),
        };
        (settings, notices)
    }
}

/// A test-only temporary root (behind `test-support` for other crates' tests); the process id separates two `cargo test` runs going in
/// parallel. `tempfile` would be a dependency decision. The watch tests (`watch`) use it too;
/// the name prefixes must not collide.
#[cfg(any(test, feature = "test-support"))]
pub struct TempRoot(pub PathBuf);

#[cfg(any(test, feature = "test-support"))]
impl TempRoot {
    pub fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("bateri-settings-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("temp root setup failed");
        Self(path)
    }
}

#[cfg(any(test, feature = "test-support"))]
impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_root_is_under_dot_config() {
        assert_eq!(
            config_root(Path::new("/Users/someone")),
            PathBuf::from("/Users/someone/.config/bateri")
        );
    }

    #[test]
    fn settings_command_creates_the_template_once() {
        // Neither directory nor file exists: both are created, the template yields the defaults
        // without diagnostics at launch — "Settings…" does not change behavior.
        let root = TempRoot::new("create");
        let config = root.0.join("nested").join("bateri");
        let path = create_if_missing(&config).expect("template not created");
        assert_eq!(path, config.join(FILE_NAME));
        assert_eq!(
            std::fs::read_to_string(&path).expect("read failed"),
            Settings::TEMPLATE
        );
        assert_eq!(load(&config).at_launch(), (Settings::default(), Vec::new()));

        // An existing file is not overwritten, even a broken one: an unfinished edit.
        std::fs::write(&path, "[terminal\n").expect("write failed");
        assert_eq!(create_if_missing(&config).expect("second call"), path);
        assert_eq!(
            std::fs::read_to_string(&path).expect("read failed"),
            "[terminal\n"
        );
    }

    #[test]
    fn settings_command_does_not_write_through_a_dangling_link() {
        // A link pointing at a moved dotfile repository: the target is not created, the link
        // stays and the settings slot keeps reporting it.
        let root = TempRoot::new("create-dangling");
        let target = root.0.join("moved-away.toml");
        std::os::unix::fs::symlink(&target, root.0.join(FILE_NAME)).expect("symlink failed");
        assert!(create_if_missing(&root.0).is_ok());
        assert!(!target.exists(), "link target was created");
        assert!(matches!(load(&root.0), Loaded::Unreadable(_)));
    }

    #[test]
    fn settings_command_reports_an_uncreatable_root() {
        // A file in the root's place: the directory cannot be created, the error goes back to the
        // caller.
        let root = TempRoot::new("create-blocked");
        let config = root.0.join("bateri");
        std::fs::write(&config, "").expect("write failed");
        assert!(create_if_missing(&config).is_err());
    }

    /// The menu's theme choice: the edit the tests write.
    fn paper() -> SettingsEdit {
        SettingsEdit::Theme("paper".to_owned())
    }

    #[test]
    fn a_setting_write_names_what_was_not_saved() {
        // The window's write takes the same path: in place, without touching the remaining
        // lines; when refused the message says "setting", not "theme".
        let root = TempRoot::new("write-setting");
        std::fs::write(root.0.join(FILE_NAME), "[terminal]\nscrollback = 7 # few\n")
            .expect("write failed");
        assert_eq!(write_edit(&root.0, &SettingsEdit::Scrollback(2500)), Ok(()));
        assert_eq!(
            std::fs::read_to_string(root.0.join(FILE_NAME)).expect("read failed"),
            "[terminal]\nscrollback = 2500 # few\n"
        );
        std::fs::write(root.0.join(FILE_NAME), "terminal = 1\n").expect("write failed");
        assert_eq!(
            write_edit(&root.0, &SettingsEdit::Scrollback(2500)),
            Err("settings.toml: line 1: `terminal` must be a section, found an integer; the setting was not saved".to_owned())
        );
    }

    #[test]
    fn theme_write_updates_the_file_in_place() {
        // The comment and the unknown key stay, the pair is in place; the read sees the new
        // theme.
        let root = TempRoot::new("write");
        let text =
            "# mine\n[appearance]\ntheme = \"system\" # os\ndark_theme = \"ink\"\n[x]\ny = 1\n";
        std::fs::write(root.0.join(FILE_NAME), text).expect("write failed");
        assert_eq!(write_edit(&root.0, &paper()), Ok(()));
        assert_eq!(
            std::fs::read_to_string(root.0.join(FILE_NAME)).expect("read failed"),
            text.replace("\"system\"", "\"paper\"")
        );
        let (settings, notices) = load(&root.0).at_launch();
        assert_eq!(
            (settings.theme.as_str(), settings.dark_theme.as_str()),
            ("paper", "ink")
        );
        assert!(notices.is_empty(), "{notices:?}");
    }

    #[test]
    fn theme_write_goes_through_a_symlink() {
        // A dotfile repository: the target is updated, the link stays a link — a temporary
        // file + rename would turn it into a regular file.
        let root = TempRoot::new("write-symlink");
        let target = root.0.join("dotfiles.toml");
        std::fs::write(&target, "[terminal]\nscrollback = 7\n").expect("write failed");
        let link = root.0.join(FILE_NAME);
        std::os::unix::fs::symlink(&target, &link).expect("symlink failed");
        assert_eq!(write_edit(&root.0, &paper()), Ok(()));
        assert!(
            std::fs::symlink_metadata(&link)
                .expect("no link")
                .file_type()
                .is_symlink(),
            "link became a regular file"
        );
        assert_eq!(
            std::fs::read_to_string(&target).expect("read failed"),
            "[terminal]\nscrollback = 7\n\n[appearance]\ntheme = \"paper\"\n"
        );
    }

    #[test]
    fn theme_write_refuses_a_file_it_cannot_parse() {
        // The user's unfinished work is not overwritten; the message goes to the write slot.
        let root = TempRoot::new("write-unparseable");
        let text = "[appearance\ntheme = \"ink\"\n";
        std::fs::write(root.0.join(FILE_NAME), text).expect("write failed");
        let err = write_edit(&root.0, &paper()).expect_err("must not write");
        assert!(
            err.starts_with("settings.toml: line 1: invalid TOML: ")
                && err.ends_with("; the theme was not saved"),
            "{err}"
        );
        assert_eq!(
            std::fs::read_to_string(root.0.join(FILE_NAME)).expect("read failed"),
            text
        );

        // An `appearance` that is not a section is not overwritten either.
        std::fs::write(root.0.join(FILE_NAME), "appearance = 1\n").expect("write failed");
        assert_eq!(
            write_edit(&root.0, &paper()),
            Err("settings.toml: line 1: `appearance` must be a section, found an integer; the theme was not saved".to_owned())
        );

        // A dangling link: the target is not created.
        std::fs::remove_file(root.0.join(FILE_NAME)).expect("remove failed");
        let moved = root.0.join("moved.toml");
        std::os::unix::fs::symlink(&moved, root.0.join(FILE_NAME)).expect("symlink failed");
        assert_eq!(
            write_edit(&root.0, &paper()),
            Err("settings.toml could not be read: symbolic link points to a missing file; the theme was not saved".to_owned())
        );
        assert!(!moved.exists(), "link target was created");
    }

    #[test]
    fn theme_write_without_a_file_starts_from_the_template() {
        // Neither directory nor file: the "Settings…" path, with the key on top.
        let root = TempRoot::new("write-missing");
        let config = root.0.join("nested").join("bateri");
        assert_eq!(write_edit(&config, &paper()), Ok(()));
        assert_eq!(
            std::fs::read_to_string(config.join(FILE_NAME)).expect("read failed"),
            // With the line start: the template's comment also says `theme = "system"` and
            // stays in place.
            Settings::TEMPLATE.replace("\ntheme = \"system\"\n", "\ntheme = \"paper\"\n")
        );
        assert_eq!(
            load(&config).at_launch(),
            (
                Settings {
                    theme: "paper".to_owned(),
                    ..Settings::default()
                },
                Vec::new()
            )
        );
    }

    #[test]
    fn user_theme_names_are_the_selectable_files() {
        let root = TempRoot::new("theme-names");
        // Without a themes directory the list is empty, not an error.
        assert_eq!(user_theme_names(&root.0), Vec::<String>::new());
        // `""`: a file named exactly `.toml` — a review finding, an item with an empty
        // title would write `theme = ""`.
        for name in ["paper", "Ink", "bateri", "system", ".hidden", "._paper", ""] {
            write_theme_file(&root, name, "");
        }
        let dir = root.0.join(THEMES_DIR);
        std::fs::write(dir.join("notes.txt"), "").expect("write failed");
        std::fs::create_dir(dir.join("folder.toml")).expect("mkdir failed");
        std::os::unix::fs::symlink(dir.join("paper.toml"), dir.join("linked.toml"))
            .expect("symlink failed");
        std::os::unix::fs::symlink(dir.join("gone.toml"), dir.join("dangling.toml"))
            .expect("symlink failed");
        // A file shadowing an embedded name (`bateri`) is the same choice as the embedded name's
        // item and is not listed separately; `system` is not a theme name; entries starting
        // with a dot (AppleDouble `._x`, hidden) and entries that are not regular files cannot
        // be selected. The order is case-insensitive.
        assert_eq!(user_theme_names(&root.0), ["Ink", "linked", "paper"]);
    }

    #[test]
    fn missing_file_is_silent_default() {
        let root = TempRoot::new("missing");
        let loaded = load(&root.0);
        assert!(matches!(loaded, Loaded::Missing), "{loaded:?}");
        assert_eq!(loaded.at_launch(), (Settings::default(), Vec::new()));

        // Same state if the root itself is missing: the user never set up the directory.
        let loaded = load(&root.0.join("absent"));
        assert!(matches!(loaded, Loaded::Missing), "{loaded:?}");
    }

    #[test]
    fn unreadable_file_is_reported_and_defaults_at_launch() {
        // With a directory, not with permissions: a test running as root would still read a
        // `chmod 000` file.
        let root = TempRoot::new("unreadable");
        std::fs::create_dir(root.0.join(FILE_NAME)).expect("mkdir failed");
        let loaded = load(&root.0);
        assert!(matches!(loaded, Loaded::Unreadable(_)), "{loaded:?}");
        let (settings, notices) = loaded.at_launch();
        // The unreadable file may hold `osc52 = "off"`: the clipboard falls back to off.
        assert_eq!(settings, Settings::for_unusable_file());
        assert_eq!(notices.len(), 1);
        assert_eq!(
            notices,
            ["settings.toml could not be read: not a regular file"]
        );
    }

    #[test]
    fn dangling_symlink_is_not_missing() {
        let root = TempRoot::new("dangling");
        std::os::unix::fs::symlink(root.0.join("moved-away.toml"), root.0.join(FILE_NAME))
            .expect("symlink failed");
        let loaded = load(&root.0);
        assert!(matches!(loaded, Loaded::Unreadable(_)), "{loaded:?}");
        let (settings, notices) = loaded.at_launch();
        assert_eq!(settings, Settings::for_unusable_file());
        assert_eq!(
            notices,
            ["settings.toml could not be read: symbolic link points to a missing file"]
        );
    }

    #[test]
    fn symlink_to_a_file_is_read() {
        // A dotfile manager's ordinary case: a link to the file in the repository.
        let root = TempRoot::new("symlink");
        std::fs::write(root.0.join("real.toml"), "[terminal]\nscrollback = 7\n")
            .expect("write failed");
        std::os::unix::fs::symlink(root.0.join("real.toml"), root.0.join(FILE_NAME))
            .expect("symlink failed");
        assert_eq!(load(&root.0).at_launch().0.scrollback, 7);
    }

    #[test]
    fn unparseable_file_is_its_own_result() {
        let root = TempRoot::new("unparseable");
        std::fs::write(root.0.join(FILE_NAME), "[terminal\n").expect("write failed");
        let loaded = load(&root.0);
        assert!(matches!(loaded, Loaded::Unparseable(_)), "{loaded:?}");
        let (settings, notices) = loaded.at_launch();
        // All settings are defaults, except OSC 52: it falls back to off.
        assert_eq!(settings, Settings::for_unusable_file());
        assert_eq!(notices.len(), 1);
        assert!(
            notices[0].starts_with("settings.toml: line 1: invalid TOML: "),
            "{notices:?}"
        );
    }

    #[test]
    fn valid_file_is_read_with_its_diagnostics() {
        let root = TempRoot::new("valid");
        std::fs::write(root.0.join(FILE_NAME), "[terminal]\nscrollback = 2500\n")
            .expect("write failed");
        assert_eq!(
            load(&root.0).at_launch(),
            (
                Settings {
                    scrollback: 2500,
                    ..Settings::default()
                },
                Vec::new()
            )
        );

        std::fs::write(root.0.join(FILE_NAME), "[terminal]\nscrollback = true\n")
            .expect("write failed");
        let (settings, notices) = load(&root.0).at_launch();
        assert_eq!(settings, Settings::default());
        assert_eq!(notices.len(), 1);
        assert!(
            notices[0].starts_with("settings.toml: line 2: `terminal.scrollback`"),
            "{notices:?}"
        );
    }

    /// The appearance of the theme tests: dark. A test whose fallback depends on the appearance
    /// passes both explicitly.
    const DARK: bool = true;

    /// Writes `themes/{name}.toml` under the root.
    fn write_theme_file(root: &TempRoot, name: &str, text: &str) {
        let dir = root.0.join(THEMES_DIR);
        std::fs::create_dir_all(&dir).expect("themes dir setup failed");
        std::fs::write(dir.join(format!("{name}.toml")), text).expect("write failed");
    }

    #[test]
    fn embedded_theme_without_user_file() {
        let root = TempRoot::new("theme-embedded");
        let (theme, notices) = load_theme(Some(&root.0), "bateri").or_embedded(DARK);
        assert_eq!((theme, notices), (Theme::BATERI, Vec::new()));
        // The embedded ones resolve even without a home directory.
        assert_eq!(
            load_theme(None, "bateri").or_embedded(DARK),
            (Theme::BATERI, Vec::new())
        );
    }

    #[test]
    fn user_theme_shadows_the_embedded_one() {
        let root = TempRoot::new("theme-shadow");
        write_theme_file(&root, "bateri", "background = \"#ffffff\"\n");
        let (theme, notices) = load_theme(Some(&root.0), "bateri").or_embedded(DARK);
        assert_eq!(notices, Vec::<String>::new());
        assert_eq!(
            theme,
            Theme {
                background: 0xffffff,
                ..Theme::BATERI
            }
        );

        // The base of a file shadowing the light embedded theme is the light theme
        // (a review finding): a user who changes only the cursor must not get a dark
        // background in light mode. The base of a non-shadowing name stays `bateri`.
        write_theme_file(&root, "bateri-light", "accent = \"#ff0000\"\n");
        write_theme_file(&root, "paper", "accent = \"#ff0000\"\n");
        let light = load_theme(Some(&root.0), "bateri-light").or_embedded(false);
        assert_eq!(
            light,
            (
                Theme {
                    accent: 0xff0000,
                    ..Theme::BATERI_LIGHT
                },
                Vec::new()
            )
        );
        let paper = load_theme(Some(&root.0), "paper").or_embedded(false);
        assert_eq!(
            paper.0,
            Theme {
                accent: 0xff0000,
                ..Theme::BATERI
            }
        );
    }

    #[test]
    fn empty_user_theme_is_unusable() {
        // An editor saving in place first truncates the file: an empty theme read in the middle
        // of the save would be the dark base itself and the live reload would slam the window
        // to it (a review finding). `settings.toml`'s rule: an empty file is unusable.
        let root = TempRoot::new("theme-empty");
        write_theme_file(&root, "paper", " \n");
        assert_eq!(
            load_theme(Some(&root.0), "paper").or_current(),
            (
                None,
                vec!["themes/paper.toml is empty; keeping the current theme".to_owned()]
            )
        );
        // At launch the embedded theme matching the appearance; no silent fallback to the
        // shadowed name's embedded theme (the broken file's rule).
        write_theme_file(&root, "bateri", "");
        let (theme, notices) = load_theme(Some(&root.0), "bateri").or_embedded(false);
        assert_eq!(theme, Theme::BATERI_LIGHT);
        assert_eq!(notices, ["themes/bateri.toml is empty; using bateri-light"]);
    }

    #[test]
    fn user_theme_reports_its_bad_colors_with_the_file_name() {
        let root = TempRoot::new("theme-diagnostics");
        write_theme_file(
            &root,
            "paper",
            "[ansi]\nred = \"red\"\nblue = \"#0000ff\"\n",
        );
        let (theme, notices) = load_theme(Some(&root.0), "paper").or_embedded(DARK);
        assert_eq!(theme.ansi[4], 0x0000ff);
        assert_eq!(theme.ansi[1], Theme::BATERI.ansi[1]);
        assert_eq!(
            notices,
            [
                "themes/paper.toml: line 2: `ansi.red` must be a color like \"#rrggbb\", found \"red\"; using #d16d6a"
            ]
        );
    }

    #[test]
    fn missing_theme_falls_back_to_bateri_with_notice() {
        let root = TempRoot::new("theme-missing");
        let (theme, notices) = load_theme(Some(&root.0), "paper").or_embedded(DARK);
        assert_eq!(theme, Theme::BATERI);
        assert_eq!(notices, ["theme \"paper\" not found; using bateri"]);
    }

    #[test]
    fn fallback_follows_the_appearance() {
        // A theme not found in light mode does not open a dark window: the fallback is the
        // embedded theme a user without a file would see in that appearance.
        let root = TempRoot::new("theme-fallback-light");
        let (theme, notices) = load_theme(Some(&root.0), "paper").or_embedded(false);
        assert_eq!(theme, Theme::BATERI_LIGHT);
        assert_eq!(notices, ["theme \"paper\" not found; using bateri-light"]);
        assert_eq!(
            load_theme(Some(&root.0), "bateri-light").or_embedded(false),
            (Theme::BATERI_LIGHT, Vec::new())
        );
    }

    #[test]
    fn appearance_switches_never_leave_the_other_appearances_theme() {
        // A review scenario: `dark_theme` is not found, the window goes back and forth
        // dark → light → dark. Every switch chooses the theme again for that appearance; the
        // broken dark theme does **not leave** the light theme on screen, it falls back to the
        // embedded dark one. The light switch empties the theme slot.
        let root = TempRoot::new("theme-switches");
        let settings = Settings {
            dark_theme: "ink".to_owned(),
            ..Settings::default()
        };
        let pick = |dark| load_theme(Some(&root.0), settings.theme_for(dark)).or_embedded(dark);
        for _ in 0..2 {
            assert_eq!(
                pick(true),
                (
                    Theme::BATERI,
                    vec!["theme \"ink\" not found; using bateri".to_owned()]
                )
            );
            assert_eq!(pick(false), (Theme::BATERI_LIGHT, Vec::new()));
        }
    }

    #[test]
    fn broken_user_theme_does_not_fall_to_the_embedded_one() {
        // An embedded theme with the same name exists, but the broken file does not open it:
        // even though the result is still `bateri`, the **message** names the file. The same
        // rule as for a theme without an embedded name (second half).
        let root = TempRoot::new("theme-broken");
        write_theme_file(&root, "bateri", "background = \"#ffffff\n");
        let loaded = load_theme(Some(&root.0), "bateri");
        assert!(matches!(loaded, ThemeLoaded::Failed(_)), "{loaded:?}");
        let (theme, notices) = loaded.or_embedded(DARK);
        assert_eq!(theme, Theme::BATERI);
        assert_eq!(notices.len(), 1);
        assert!(
            notices[0].starts_with("themes/bateri.toml: line 1: invalid TOML: ")
                && notices[0].ends_with("; using bateri"),
            "{notices:?}"
        );

        std::fs::create_dir_all(root.0.join(THEMES_DIR).join("paper.toml"))
            .expect("theme dir not created");
        assert_eq!(
            load_theme(Some(&root.0), "paper").or_embedded(DARK).1,
            ["themes/paper.toml could not be read: not a regular file; using bateri"]
        );
    }

    #[test]
    fn live_reload_applies_nothing_from_a_broken_or_missing_file() {
        let root = TempRoot::new("live");
        // No file (also in the middle of an editor's save): nothing to apply, and
        // this is not an error.
        assert_eq!(load(&root.0).live(), (None, Vec::new()));

        // A half-finished save: nothing is applied, the slot reports the line.
        std::fs::write(root.0.join(FILE_NAME), "[appearance]\ntheme = \"paper\n")
            .expect("write failed");
        let (settings, notices) = load(&root.0).live();
        assert_eq!(settings, None);
        assert_eq!(notices.len(), 1);
        assert!(
            notices[0].starts_with("settings.toml: line 2: invalid TOML: "),
            "{notices:?}"
        );

        // An unreadable file (the link's target moved) applies nothing either.
        std::fs::remove_file(root.0.join(FILE_NAME)).expect("remove failed");
        std::os::unix::fs::symlink(root.0.join("moved.toml"), root.0.join(FILE_NAME))
            .expect("symlink failed");
        assert_eq!(
            load(&root.0).live(),
            (
                None,
                vec![
                    "settings.toml could not be read: symbolic link points to a missing file"
                        .to_owned()
                ]
            )
        );

        // The corrected save is applied along with its diagnostics.
        std::fs::remove_file(root.0.join(FILE_NAME)).expect("remove failed");
        std::fs::write(
            root.0.join(FILE_NAME),
            "[terminal]\nscrollback = 5\n[appearance]\ntheme = 3\n",
        )
        .expect("write failed");
        let (settings, notices) = load(&root.0).live();
        assert_eq!(settings.map(|s| s.scrollback), Some(5));
        assert_eq!(notices.len(), 1);
        assert!(
            notices[0].starts_with("settings.toml: line 4: `appearance.theme`"),
            "{notices:?}"
        );
    }

    #[test]
    fn file_state_is_what_the_subtitle_says() {
        // Three states and no file; the subtitle's text comes from the state itself, so the
        // window's banner and the subtitle say the same sentence.
        let root = TempRoot::new("state");
        let path = root.0.join(FILE_NAME);
        assert_eq!(load(&root.0).state(), FileState::Missing);
        assert_eq!(FileState::Missing.notices(), Vec::<String>::new());

        std::fs::write(&path, "[terminal]\ncursor = \"bar\"\n").expect("write failed");
        let loaded = load(&root.0);
        let state = loaded.state();
        let FileState::Usable(diagnostics) = &state else {
            panic!("parsed file is usable: {state:?}");
        };
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].key, Some("terminal.cursor"));
        assert_eq!(state.notices(), loaded.live().1);

        std::fs::write(&path, "[terminal\n").expect("write failed");
        let loaded = load(&root.0);
        let state = loaded.state();
        let FileState::Locked(reason) = &state else {
            panic!("unparseable file is locked: {state:?}");
        };
        assert!(reason.starts_with("settings.toml: line 1: "), "{reason}");
        assert_eq!(state.notices(), loaded.at_launch().1);

        std::fs::remove_file(&path).expect("remove failed");
        std::os::unix::fs::symlink(root.0.join("moved.toml"), &path).expect("symlink failed");
        let loaded = load(&root.0);
        assert_eq!(
            loaded.state(),
            FileState::Locked(
                "settings.toml could not be read: symbolic link points to a missing file"
                    .to_owned()
            )
        );
        assert_eq!(loaded.state().notices(), loaded.live().1);
    }

    #[test]
    fn live_reload_keeps_current_values_for_rejected_keys() {
        // `scrollback` was saved with the wrong type while a hundred-thousand-line history is
        // open: at save time the value comes from the current setting, the diff stays empty and
        // the history is not trimmed. The launch read falls back to the default on the same file.
        let root = TempRoot::new("live-rejected");
        std::fs::write(
            root.0.join(FILE_NAME),
            "[terminal]\nscrollback = \"100000\"\n",
        )
        .expect("write failed");
        let current = Settings {
            scrollback: 100_000,
            ..Settings::default()
        };
        let (settings, notices) = load_keeping(&root.0, &current).live();
        let settings = settings.expect("parsed file is applied");
        assert_eq!(current.changes(&settings), bt_core::Changes::default());
        assert_eq!(
            notices,
            [
                "settings.toml: line 2: `terminal.scrollback` must be an integer, found a string; using 100000"
            ]
        );
        assert_eq!(load(&root.0).at_launch().0.scrollback, 10_000);
    }

    #[test]
    fn live_reload_applies_nothing_from_an_empty_file() {
        // An editor saving in place first truncates the file (`O_TRUNC`), then writes; the
        // truncation fires an event. If the empty file read in between applied the default
        // `scrollback`, the excess history would be deleted irreversibly.
        let root = TempRoot::new("live-empty");
        for text in ["", "\n  \n"] {
            std::fs::write(root.0.join(FILE_NAME), text).expect("write failed");
            let loaded = load_keeping(&root.0, &Settings::default());
            assert!(matches!(loaded, Loaded::Missing), "{text:?}: {loaded:?}");
            assert_eq!(loaded.live(), (None, Vec::new()), "{text:?}");
            // At launch an empty file was already the same as no file.
            assert_eq!(
                load(&root.0).at_launch(),
                (Settings::default(), Vec::new()),
                "{text:?}"
            );
        }
    }

    #[test]
    fn live_reload_keeps_the_current_theme_when_unusable() {
        // On a live reload an unusable theme is not swapped in: a half-saved
        // theme file does not slam the window to the embedded theme.
        let root = TempRoot::new("live-theme");
        write_theme_file(&root, "paper", "background = \"#ffffff\n");
        let (theme, notices) = load_theme(Some(&root.0), "paper").or_current();
        assert_eq!(theme, None);
        assert_eq!(notices.len(), 1);
        assert!(
            notices[0].starts_with("themes/paper.toml: line 1: invalid TOML: ")
                && notices[0].ends_with("; keeping the current theme"),
            "{notices:?}"
        );
        assert_eq!(
            load_theme(Some(&root.0), "ink").or_current(),
            (
                None,
                vec!["theme \"ink\" not found; keeping the current theme".to_owned()]
            )
        );

        // The corrected file is swapped in, the slot empties.
        write_theme_file(&root, "paper", "background = \"#ffffff\"\n");
        assert_eq!(
            load_theme(Some(&root.0), "paper").or_current(),
            (
                Some(Theme {
                    background: 0xffffff,
                    ..Theme::BATERI
                }),
                Vec::new()
            )
        );
    }

    #[test]
    fn watched_paths_follow_the_layout() {
        // The watcher's paths are the same as the paths the reader reads: if one changes and the
        // other stays, no source sees the save and the symptom is silent.
        let root = Path::new("/r");
        assert_eq!(
            watched_paths(root),
            [
                PathBuf::from("/r"),
                PathBuf::from("/r/themes"),
                PathBuf::from("/r/settings.toml")
            ]
        );
        assert_eq!(
            theme_path(root, "paper"),
            PathBuf::from("/r/themes/paper.toml")
        );
    }
}
