//! `bateri help`: a short guide to bateri printed in the terminal.
//!
//! ```text
//! bateri help              the guide: shortcuts, tabs and splits, the palette, settings, docs
//! bateri help --welcome    the welcome window, in the running bateri (the guide without one)
//! bateri help config       `bateri config`'s usage
//! ```
//!
//! **The shortcuts are the menu's.** [`SHORTCUTS`] names each item by its menu, title,
//! selector, key and modifiers exactly as `bt-shell-macos`'s `menu.rs` builds it, and a test
//! there reads `menu.rs` itself and fails when an entry here no longer matches an item there
//! — a moved shortcut can't leave the guide behind. The glyphs are drawn by the palette's own
//! formatter ([`crate::palette::shortcut`]), so the guide writes `⇧⌘D` as the palette and the
//! menu do. The table lives here, not read from the live `NSMenu`, because `bateri help` must
//! work where there is no window server (ssh into the Mac) and building AppKit's menu needs
//! one. The settings lines come from the settings model ([`bt_core::template_key`]) and the
//! settings path from [`settings::config_root`], so they can't drift either.
//!
//!
//! The welcome itself is a window of the application (`bt-shell-macos`'s `welcome`, its
//! model [`crate::welcome`]); `--welcome` asks the running bateri to show it, and only where
//! no bateri can be reached — over ssh, with none running — prints this guide instead.

use std::io::Write;
use std::path::Path;

use crate::cli::Style;
use crate::palette::{Modifiers, shortcut};
use crate::settings::{self, FILE_NAME};

/// The subcommand's word: `bateri help`.
pub const SUBCOMMAND: &str = "help";

/// Bad usage — `EX_USAGE`, as for `bateri config`.
pub const EXIT_USAGE: i32 = 64;

/// The usage text.
pub const USAGE: &str = "\
usage: bateri help              a short guide: shortcuts, tabs, splits, settings, docs
       bateri help --welcome    the welcome window again (in the running bateri)
       bateri help config       how to change settings from the terminal";

/// Where the settings are documented in full.
pub const SETTINGS_DOC: &str = "https://github.com/bateri/bateri/blob/main/docs/SETTINGS.md";

/// The project's site.
pub const WEBSITE: &str = "https://bateri.dev";

/// One menu item the guide shows, spelled as `menu.rs` builds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shortcut {
    /// The menu bar's menu (`Shell`) and, for a nested item, its submenu (`Edit ▸ Find`).
    pub menu: &'static str,
    /// The item's title, as the menu shows it.
    pub title: &'static str,
    /// The item's action.
    pub selector: &'static str,
    /// The key equivalent as the menu stores it (`d`, `}`, `\r`).
    pub key: &'static str,
    pub modifiers: Modifiers,
    /// What it does, when the title doesn't say. Never a shortcut: every glyph the guide
    /// prints is an entry's own, checked against the menu.
    pub note: &'static str,
}

impl Shortcut {
    /// The glyphs the menu shows (`⇧⌘D`).
    pub fn glyphs(&self) -> String {
        shortcut(self.key, self.modifiers).unwrap_or_default()
    }
}

const fn modifiers(control: bool, option: bool, shift: bool) -> Modifiers {
    Modifiers {
        control,
        option,
        shift,
        command: true,
    }
}

/// ⌘ alone.
const CMD: Modifiers = modifiers(false, false, false);
/// ⇧⌘.
const SHIFT_CMD: Modifiers = modifiers(false, false, true);
/// ⌃⌘.
const CONTROL_CMD: Modifiers = modifiers(true, false, false);

const fn item(
    menu: &'static str,
    title: &'static str,
    selector: &'static str,
    key: &'static str,
    modifiers: Modifiers,
    note: &'static str,
) -> Shortcut {
    Shortcut {
        menu,
        title,
        selector,
        key,
        modifiers,
        note,
    }
}

/// The shortcuts the guide shows, by topic — a selection of the menu, not all of it (the
/// command palette lists every command with its shortcut).
pub const SHORTCUTS: &[(&str, &[Shortcut])] = &[
    (
        "Tabs and windows",
        &[
            item("Shell", "New Tab", "newTab:", "t", CMD, ""),
            item("Shell", "New Window", "newWindow:", "n", CMD, ""),
            item("Shell", "Close Tab", "closeTab:", "w", CMD, ""),
            item("Window", "Show Next Tab", "showNextTab:", "}", CMD, ""),
            item(
                "Window",
                "Show All Tabs",
                "showTabList:",
                "|",
                CMD,
                "every tab of the window at once",
            ),
        ],
    ),
    (
        "Splits",
        &[
            item("Shell", "Split Right", "splitRight:", "d", CMD, ""),
            item("Shell", "Split Down", "splitDown:", "d", SHIFT_CMD, ""),
            item(
                "Window",
                "Select Next Split",
                "selectNextSplit:",
                "]",
                CMD,
                "",
            ),
            item(
                "Window",
                "Zoom Split",
                "toggleSplitZoom:",
                "\r",
                SHIFT_CMD,
                "one pane fills the tab, and back",
            ),
            item(
                "Window",
                "Equalize Splits",
                "equalizeSplits:",
                "=",
                CONTROL_CMD,
                "",
            ),
        ],
    ),
    (
        "Find your way",
        &[
            item(
                "View",
                "Command Palette…",
                "toggleCommandPalette:",
                "p",
                SHIFT_CMD,
                "every command, setting, theme, tab and folder, by a few letters",
            ),
            item("Window", "Go to Tab…", "goToTab:", "p", CMD, ""),
            item("Shell", "Go to Folder…", "goToFolder:", "o", CMD, ""),
            item(
                "Edit ▸ Find",
                "Find…",
                "findInScrollback:",
                "f",
                CMD,
                "search the scrollback",
            ),
            item("Edit", "Clear to Start", "clearToStart:", "k", CMD, ""),
        ],
    ),
    (
        "Text and settings",
        &[
            item("View", "Bigger", "makeFontBigger:", "+", CMD, ""),
            item("View", "Smaller", "makeFontSmaller:", "-", CMD, ""),
            item(
                "bateri",
                "Settings…",
                "openSettings:",
                ",",
                CMD,
                "the settings window",
            ),
        ],
    ),
];

/// The entry of [`SHORTCUTS`] whose action is `selector`.
pub fn shortcut_for(selector: &str) -> Option<&'static Shortcut> {
    SHORTCUTS
        .iter()
        .flat_map(|(_, items)| items.iter())
        .find(|entry| entry.selector == selector)
}

/// The glyphs of the guide's shortcut for `selector`, empty when there is none.
fn keys(selector: &str) -> String {
    shortcut_for(selector)
        .map(Shortcut::glyphs)
        .unwrap_or_default()
}

/// The `[appearance]` key and value the guide's side-column example sets; its sentence and
/// values are read off the settings model, and a test runs the value through the parser, so the
/// example can't name a value the key doesn't take.
pub const SIDE_EXAMPLE: (&str, &str) = ("tab_position", "side");

/// What `bateri help` needs from outside.
pub struct Io<'a> {
    /// The home directory: the settings root is under it.
    pub home: &'a Path,
    pub out: &'a mut dyn Write,
    pub err: &'a mut dyn Write,
    pub style: Style,
}

/// `bateri help ARGS…` (after `help`) → the exit code.
pub fn help_main(argv: &[String], io: &mut Io<'_>) -> i32 {
    let args: Vec<&str> = argv.iter().map(String::as_str).collect();
    match args.as_slice() {
        [] | ["--help" | "-h"] => {
            let text = guide(io.home, io.style);
            let _ = io.out.write_all(text.as_bytes());
            0
        }
        // What `--welcome` falls back to where no running bateri can show the window (the
        // platform shell asked first): the guide, which carries the welcome's tips and more.
        ["--welcome"] => {
            let text = guide(io.home, io.style);
            let _ = io.out.write_all(text.as_bytes());
            0
        }
        ["config"] => {
            let _ = writeln!(io.out, "{}", crate::config_command::USAGE);
            0
        }
        _ => {
            let _ = writeln!(io.err, "{USAGE}");
            EXIT_USAGE
        }
    }
}

/// `~/…` for a path under `home`, the path itself otherwise.
fn tilde(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

/// `text` padded with spaces to `width` characters (glyphs count one each).
fn pad(text: &str, width: usize) -> String {
    let count = text.chars().count();
    format!("{text}{}", " ".repeat(width.saturating_sub(count)))
}

/// The guide `bateri help` prints.
pub fn guide(home: &Path, style: Style) -> String {
    let mut out = String::new();
    let heading = |out: &mut String, text: &str| {
        out.push('\n');
        out.push_str(&style.strong(text));
        out.push('\n');
    };
    let width = SHORTCUTS
        .iter()
        .flat_map(|(_, items)| items.iter())
        .map(|entry| entry.glyphs().chars().count())
        .max()
        .unwrap_or(0)
        + 2;
    let title_width = SHORTCUTS
        .iter()
        .flat_map(|(_, items)| items.iter())
        .map(|entry| entry.title.chars().count())
        .max()
        .unwrap_or(0)
        + 2;
    out.push_str(&format!(
        "{} {}\n",
        style.strong(&format!("bateri {}", env!("CARGO_PKG_VERSION"))),
        style.quiet(
            "— a GPU-rendered terminal for macOS: command blocks, a dock for the line you \
             type, tabs and splits."
        )
    ));
    for (topic, items) in SHORTCUTS {
        heading(&mut out, topic);
        for entry in *items {
            let note = if entry.note.is_empty() {
                format!("{} ▸ {}", entry.menu, entry.title)
            } else {
                entry.note.to_owned()
            };
            out.push_str(&format!(
                "  {}{}{}\n",
                style.mark(&pad(&entry.glyphs(), width)),
                pad(entry.title, title_width),
                style.quiet(&note)
            ));
        }
    }

    heading(&mut out, "The side column");
    let (key, value) = SIDE_EXAMPLE;
    let position = bt_core::template_key(&format!("appearance.{key}"));
    out.push_str(&format!(
        "  {}\n  View ▸ Tabs ▸ Side, or {}\n",
        position.map_or_else(
            || "Tabs can sit in a column at the window's left edge.".to_owned(),
            |found| format!("{} ({}).", found.summary(), found.allowed()),
        ),
        style.accent(&format!("bateri config set {key} {value}"))
    ));

    heading(&mut out, "Settings");
    let file = settings::config_root(home).join(FILE_NAME);
    out.push_str(&format!(
        "  {} — changes apply the moment the file is saved, in every window.\n",
        style.accent(&tilde(&file, home))
    ));
    let commands: [(&str, &str); 4] = [
        (
            "bateri config list",
            "every setting: its value, the values it takes, what it does",
        ),
        (
            "bateri config set KEY VALUE",
            "change one, checked; your comments stay",
        ),
        ("bateri config reset KEY", "back to its default"),
        ("bateri config edit", "open the file in $EDITOR"),
    ];
    let command_width = commands
        .iter()
        .map(|(command, _)| command.len())
        .max()
        .unwrap_or(0)
        + 2;
    for (command, text) in commands {
        out.push_str(&format!(
            "  {}{}\n",
            style.accent(&pad(command, command_width)),
            style.quiet(text)
        ));
    }
    out.push_str(&format!(
        "  {}{}\n",
        pad(
            &format!("{} Settings…", keys("openSettings:")),
            command_width
        ),
        style.quiet("the same settings, in a window")
    ));

    heading(&mut out, "More");
    out.push_str(&format!(
        "  {}  {}\n  {}  {}\n  {}  {} {}\n",
        pad("Every setting", 14),
        SETTINGS_DOC,
        pad("Website", 14),
        WEBSITE,
        pad("Welcome", 14),
        style.accent("bateri help --welcome"),
        style.quiet("(or Help ▸ Welcome to bateri)"),
    ));
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::TempRoot;

    fn run(home: &Path, args: &[&str]) -> (i32, String, String) {
        let argv: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = help_main(
            &argv,
            &mut Io {
                home,
                out: &mut out,
                err: &mut err,
                style: Style::plain(),
            },
        );
        (
            code,
            String::from_utf8(out).expect("utf-8"),
            String::from_utf8(err).expect("utf-8"),
        )
    }

    #[test]
    fn the_guide_shows_the_menus_shortcuts_with_the_palettes_glyphs() {
        let home = TempRoot::new("help-guide");
        let (code, out, _) = run(&home.0, &[]);
        assert_eq!(code, 0);
        for line in [
            "⌘T",
            "⌘D",
            "⇧⌘D",
            "⇧⌘P",
            "⇧⌘]",
            "⇧⌘↩",
            "⌃⌘=",
            "⌘,",
            "~/.config/bateri/settings.toml",
            "bateri config set tab_position side",
            SETTINGS_DOC,
        ] {
            assert!(out.contains(line), "{line} missing:\n{out}");
        }
        assert!(!out.contains('\x1b'), "plain output carries no escape");
        assert_eq!(run(&home.0, &["--help"]).1, out);
        // The guide writes nothing.
        assert!(std::fs::read_dir(&home.0).expect("home").next().is_none());
    }

    #[test]
    fn the_side_example_is_a_value_the_setting_takes() {
        let (key, value) = SIDE_EXAMPLE;
        let found = bt_core::template_key(&format!("appearance.{key}")).expect("tab_position");
        assert!(
            bt_core::Settings::with_checked_value(
                bt_core::Settings::TEMPLATE,
                found.path,
                &found.toml_value(value)
            )
            .is_ok()
        );
    }

    #[test]
    fn every_shortcut_has_glyphs_and_a_unique_action() {
        let mut selectors = Vec::new();
        for (_, items) in SHORTCUTS {
            for entry in *items {
                assert!(!entry.glyphs().is_empty(), "{} has no glyphs", entry.title);
                selectors.push(entry.selector);
            }
        }
        let count = selectors.len();
        selectors.sort_unstable();
        selectors.dedup();
        assert_eq!(selectors.len(), count, "an action listed twice");
    }

    #[test]
    fn welcome_falls_back_to_the_guide_and_no_shell_greets() {
        let home = TempRoot::new("help-welcome");
        assert_eq!(run(&home.0, &["--welcome"]), run(&home.0, &[]));
        // The welcome is the application's window: no shell wrapper prints one any more.
        for wrapper in [
            include_str!("../../../assets/shell/zsh/bateri.zsh"),
            include_str!("../../../assets/shell/bash/bateri.bash"),
            include_str!("../../../assets/shell/fish/vendor_conf.d/bateri.fish"),
        ] {
            assert!(!wrapper.contains("--first-run") && !wrapper.contains("welcomed"));
        }
    }

    #[test]
    fn a_wrong_argument_prints_the_usage() {
        let home = TempRoot::new("help-usage");
        let (code, _, err) = run(&home.0, &["--nope"]);
        assert_eq!(code, EXIT_USAGE);
        assert!(err.starts_with("usage: bateri help"));
        assert!(
            run(&home.0, &["config"])
                .1
                .starts_with("usage: bateri config")
        );
    }

    #[test]
    fn color_goes_out_only_when_asked() {
        let home = Path::new("/Users/someone");
        assert!(guide(home, Style::colored()).contains("\x1b[1m"));
        assert!(!guide(home, Style::plain()).contains('\x1b'));
    }
}
