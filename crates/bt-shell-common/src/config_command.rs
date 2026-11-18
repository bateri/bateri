//! `bateri config`: the settings read and changed from a terminal of this computer.
//!
//! ```text
//! bateri config list [--changed] [WORD]   each setting and its value
//! bateri config get KEY                   one value, plain
//! bateri config set KEY VALUE             checked, then written
//! bateri config reset KEY                 back to the default
//! bateri config path                      where the file is
//! ```
//!
//! The command is kablo's; its words come to the app that keeps the settings (`agent`), which
//! runs [`run`] on its own settings root — a development copy's command changes the development
//! copy's settings — and sends back what to print and the status.
//!
//! **No second list, writer or reader.** The keys are the template's plain keys
//! ([`bt_core::plain_keys`]); `set` and `reset` write through the settings window's writer
//! ([`settings::write_plain`]) and a value the app's reading refuses is not written: its sentence
//! is printed and the status is 64. A list (`remote.hosts`, `apps.sources`), the shortcuts of
//! `[keys]` and a font's features are no keys here: Settings and the file change them.
//!
//! **What widens bateri's reach is read, never written here** ([`SETTINGS_ONLY`]): the reach of
//! the `bateri` command itself, setting bateri up on a server without asking, and teaching Claude
//! Code the command. Settings changes them, where a person does. This is not a boundary — a
//! program in a terminal can write the file — only the command not doing it. A terminal on a
//! server never gets here: the app refuses it before.

use std::fmt::Write as _;
use std::path::Path;

use bt_core::{PlainKey, PlainValue, Settings, plain_key, plain_keys};

use crate::settings::{self, FILE_NAME, Unwritten};

/// The keys `bateri config` reads and does not write: each widens what bateri, or a program in
/// it, reaches.
const SETTINGS_ONLY: [&str; 3] = [
    "commands.level",
    "remote.integration",
    "agents.claude_skill",
];

/// What a `bateri config` did: what to print, how it ends, and whether the file was written (the
/// app then reads it at once, as after a change in Settings).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ran {
    pub status: u8,
    pub stdout: String,
    pub stderr: String,
    pub wrote: bool,
}

impl Ran {
    fn say(&mut self, line: &str) {
        let _ = writeln!(self.stdout, "{line}");
    }

    fn fail(&mut self, status: u8, line: &str) {
        self.status = status;
        let _ = writeln!(self.stderr, "bateri: {line}");
    }
}

/// `bateri config WORDS` on the settings of `root`.
pub fn run(root: &Path, words: &[String]) -> Ran {
    let mut ran = Ran::default();
    let Some((verb, rest)) = words.split_first() else {
        ran.fail(64, "config takes list, get, set, reset or path");
        return ran;
    };
    match verb.as_str() {
        "list" => list(root, rest, &mut ran),
        "get" => get(root, rest, &mut ran),
        "set" => set(root, rest, &mut ran),
        "reset" => reset(root, rest, &mut ran),
        "path" if rest.is_empty() => ran.say(&root.join(FILE_NAME).display().to_string()),
        "path" => ran.fail(64, "config path takes nothing more"),
        other => ran.fail(
            64,
            &format!("config takes list, get, set, reset or path, not {other:?}"),
        ),
    }
    ran
}

/// The file's text, `""` when there is none; `None` with the reason said when it cannot be read
/// or is no TOML the app reads.
fn text(root: &Path, ran: &mut Ran) -> Option<String> {
    let text = match settings::file_text(root) {
        Ok(text) => text.unwrap_or_default(),
        Err(reason) => {
            ran.fail(1, &reason);
            return None;
        }
    };
    if let Err(diagnostic) = Settings::parse(&text) {
        ran.fail(1, &settings::notice(&diagnostic));
        return None;
    }
    Some(text)
}

/// The key `name` stands for, or `None` with why said.
fn key_of(name: &str, ran: &mut Ran) -> Option<&'static PlainKey> {
    match plain_key(name) {
        Ok(key) => Some(key),
        Err(paths) if paths.is_empty() => {
            ran.fail(
                64,
                &format!("no setting is called {name:?}; bateri config list shows them"),
            );
            None
        }
        Err(paths) => {
            ran.fail(
                64,
                &format!("{name:?} is in more than one section: {}", paths.join(", ")),
            );
            None
        }
    }
}

/// What the file gives `key`, and whether the app's reading accepts it: `Ok(None)` when the file
/// does not write it.
fn in_file(text: &str, key: &PlainKey) -> Result<Option<PlainValue>, String> {
    let value = key.read(text).map_err(|diagnostic| diagnostic.message)?;
    match refusal(text, key) {
        Some(reason) if value.is_some() => Err(reason),
        _ => Ok(value),
    }
}

/// The app's reading's sentence about `key` in `text`, when it refuses what the file says.
fn refusal(text: &str, key: &PlainKey) -> Option<String> {
    Settings::parse(text)
        .ok()?
        .diagnostics
        .into_iter()
        .find(|diagnostic| diagnostic.key == Some(key.path))
        .map(|diagnostic| diagnostic.message)
}

fn list(root: &Path, words: &[String], ran: &mut Ran) {
    let mut changed = false;
    let mut word = None;
    for given in words {
        match given.as_str() {
            "--changed" => changed = true,
            option if option.starts_with('-') => {
                ran.fail(64, &format!("config list takes --changed, not {option:?}"));
                return;
            }
            _ if word.is_some() => {
                ran.fail(64, "config list takes one word to look for");
                return;
            }
            name => word = Some(name),
        }
    }
    let Some(text) = text(root, ran) else {
        return;
    };
    let mut lines = Vec::new();
    for key in plain_keys() {
        if word.is_some_and(|word| !key.path.contains(word)) {
            continue;
        }
        let (shown, note) = match key.read(&text) {
            Err(diagnostic) => (None, format!("not accepted: {}", diagnostic.message)),
            Ok(Some(value)) => match refusal(&text, key) {
                Some(reason) => (Some(value), format!("not accepted: {reason}")),
                None if key.default.as_ref() == Some(&value) => (Some(value), String::new()),
                None => (
                    Some(value),
                    key.default.as_ref().map_or_else(
                        || "default: not set".to_owned(),
                        |default| format!("default {}", default.toml()),
                    ),
                ),
            },
            Ok(None) => (key.default.clone(), String::new()),
        };
        if changed && note.is_empty() {
            continue;
        }
        lines.push((key.path, shown, note));
    }
    if lines.is_empty() {
        match word {
            Some(word) if !changed => {
                ran.fail(1, &format!("no setting has {word:?} in its name"));
            }
            _ => {}
        }
        return;
    }
    let width = lines.iter().map(|(path, ..)| path.len()).max().unwrap_or(0);
    for (path, shown, note) in lines {
        let mut line = match shown {
            Some(value) => format!("{path:<width$} = {}", value.toml()),
            None if note.is_empty() => format!("{path:<width$}   (not set)"),
            None => format!("{path:<width$}"),
        };
        if !note.is_empty() {
            let _ = write!(line, "  ({note})");
        }
        ran.say(&line);
    }
}

fn get(root: &Path, words: &[String], ran: &mut Ran) {
    let [name] = words else {
        ran.fail(64, "config get takes one KEY");
        return;
    };
    let Some(key) = key_of(name, ran) else {
        return;
    };
    let Some(text) = text(root, ran) else {
        return;
    };
    let value = match in_file(&text, key) {
        Ok(value) => value.or_else(|| key.default.clone()),
        Err(reason) => {
            ran.fail(
                0,
                &format!("{FILE_NAME}'s {} is not accepted: {reason}", key.path),
            );
            key.default.clone()
        }
    };
    match value {
        Some(value) => ran.say(&value.plain()),
        None => ran.fail(1, &format!("{} is not set", key.path)),
    }
}

fn set(root: &Path, words: &[String], ran: &mut Ran) {
    let [name, value @ ..] = words else {
        ran.fail(64, "config set takes a KEY and a VALUE");
        return;
    };
    if value.is_empty() {
        ran.fail(64, "config set takes a KEY and a VALUE");
        return;
    }
    let Some(key) = key_of(name, ran) else {
        return;
    };
    if settings_only(key, ran) {
        return;
    }
    let value = match key.value_of(&value.join(" ")) {
        Ok(value) => value,
        Err(reason) => {
            ran.fail(64, &reason);
            return;
        }
    };
    let Some(text) = text(root, ran) else {
        return;
    };
    let before = in_file(&text, key).ok().flatten();
    if !write(root, key, Some(&value), ran) {
        return;
    }
    let was = match before.as_ref().or(key.default.as_ref()) {
        Some(old) if *old == value => "unchanged".to_owned(),
        Some(old) => format!("was {}", old.toml()),
        None => "was not set".to_owned(),
    };
    ran.say(&format!("{} = {}  ({was})", key.path, value.toml()));
}

fn reset(root: &Path, words: &[String], ran: &mut Ran) {
    let [name] = words else {
        ran.fail(64, "config reset takes one KEY");
        return;
    };
    let Some(key) = key_of(name, ran) else {
        return;
    };
    if settings_only(key, ran) {
        return;
    }
    let Some(text) = text(root, ran) else {
        return;
    };
    // A file that does not write the key, or none at all, is at the default already: nothing is
    // written, and no file is made for it.
    let written = key.read(&text).ok().flatten().is_some();
    if written && !write(root, key, None, ran) {
        return;
    }
    let state = if written {
        "the default"
    } else {
        "already the default"
    };
    match &key.default {
        Some(default) => ran.say(&format!("{} = {}  ({state})", key.path, default.toml())),
        None => ran.say(&format!("{} is not set  ({state})", key.path)),
    }
}

/// A key of [`SETTINGS_ONLY`] is not the command's to change: said, and `true`.
fn settings_only(key: &PlainKey, ran: &mut Ran) -> bool {
    if !SETTINGS_ONLY.contains(&key.path) {
        return false;
    }
    ran.fail(
        77,
        &format!(
            "{} widens what bateri reaches, so it is changed in Settings, not by this command",
            key.path
        ),
    );
    true
}

/// Writes `value` (or takes the key out); `false` with the reason said when it was not written.
fn write(root: &Path, key: &PlainKey, value: Option<&PlainValue>, ran: &mut Ran) -> bool {
    match settings::write_plain(root, key, value) {
        Ok(()) => {
            ran.wrote = true;
            true
        }
        // The reading's sentence without what it would fall back to (`; using "cards"`):
        // nothing was written, so nothing falls back.
        Err(Unwritten::Refused(diagnostic)) => {
            let reason = diagnostic
                .message
                .split("; using ")
                .next()
                .unwrap_or_default();
            ran.fail(64, reason);
            false
        }
        Err(Unwritten::Unread(reason) | Unwritten::Failed(reason)) => {
            ran.fail(1, &reason);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Root(std::path::PathBuf);

    impl Root {
        fn new(name: &str) -> Root {
            let path = std::env::temp_dir().join(format!(
                "bateri-config-{name}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("root");
            Root(path)
        }

        fn run(&self, words: &[&str]) -> Ran {
            let words: Vec<String> = words.iter().map(|word| (*word).to_owned()).collect();
            run(&self.0, &words)
        }

        fn file(&self) -> Option<String> {
            std::fs::read_to_string(self.0.join(FILE_NAME)).ok()
        }

        fn write(&self, text: &str) {
            std::fs::write(self.0.join(FILE_NAME), text).expect("written");
        }
    }

    impl Drop for Root {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn get_reads_the_file_and_the_default_and_makes_no_file() {
        let root = Root::new("get");
        let ran = root.run(&["get", "split_style"]);
        assert_eq!((ran.status, ran.stdout.as_str()), (0, "cards\n"));
        assert_eq!(root.file(), None, "a read makes no file");
        root.write("[appearance]\nsplit_style = \"lines\"\n");
        assert_eq!(
            root.run(&["get", "appearance.split_style"]).stdout,
            "lines\n"
        );
        let unset = root.run(&["get", "family"]);
        assert_eq!(unset.status, 1);
        assert_eq!(unset.stderr, "bateri: font.family is not set\n");
        assert!(!root.run(&["get", "scrollback"]).wrote);
    }

    #[test]
    fn set_writes_one_line_through_the_settings_writer_and_says_what_it_was() {
        let root = Root::new("set");
        let ran = root.run(&["set", "split_style", "lines"]);
        assert_eq!(ran.status, 0, "{}", ran.stderr);
        assert_eq!(
            ran.stdout,
            "appearance.split_style = \"lines\"  (was \"cards\")\n"
        );
        assert!(ran.wrote);
        // No file: the template is made, as Settings makes it, with the one line changed.
        let file = root.file().expect("made");
        assert_eq!(
            file,
            Settings::TEMPLATE.replace("split_style = \"cards\"", "split_style = \"lines\"")
        );
        let ran = root.run(&["set", "font.family", "Kode", "Mono"]);
        assert_eq!(ran.stdout, "font.family = \"Kode Mono\"  (was not set)\n");
        assert_eq!(root.run(&["get", "family"]).stdout, "Kode Mono\n");
        let again = root.run(&["set", "split_style", "lines"]);
        assert_eq!(
            again.stdout,
            "appearance.split_style = \"lines\"  (unchanged)\n"
        );
    }

    #[test]
    fn a_value_the_app_refuses_is_said_and_not_written() {
        let root = Root::new("refused");
        root.write("# mine\n[appearance]\nsplit_style = \"cards\"\n");
        let ran = root.run(&["set", "split_style", "dots"]);
        assert_eq!(ran.status, 64);
        assert!(ran.stderr.starts_with("bateri: "), "{}", ran.stderr);
        assert_eq!(
            ran.stderr,
            "bateri: `appearance.split_style` must be \"cards\" or \"lines\", found \"dots\"\n"
        );
        assert!(!ran.wrote);
        assert_eq!(
            root.file().as_deref(),
            Some("# mine\n[appearance]\nsplit_style = \"cards\"\n")
        );
        let ran = root.run(&["set", "scrollback", "many"]);
        assert_eq!(ran.status, 64);
        assert_eq!(
            ran.stderr,
            "bateri: terminal.scrollback takes a whole number, not \"many\"\n"
        );
        // A file the app cannot read is not written over.
        root.write("[font\n");
        let ran = root.run(&["set", "size", "14"]);
        assert_eq!(ran.status, 1);
        assert_eq!(root.file().as_deref(), Some("[font\n"));
    }

    #[test]
    fn reset_takes_the_line_out_and_makes_no_file() {
        let root = Root::new("reset");
        let ran = root.run(&["reset", "theme"]);
        assert_eq!(
            ran.stdout,
            "appearance.theme = \"system\"  (already the default)\n"
        );
        assert_eq!(root.file(), None);
        root.write("[appearance]\n# mine\ntheme = \"nord\"\nfocus_frame = true\n");
        let ran = root.run(&["reset", "theme"]);
        assert_eq!(ran.stdout, "appearance.theme = \"system\"  (the default)\n");
        assert!(ran.wrote);
        assert_eq!(
            root.file().as_deref(),
            Some("[appearance]\n# mine\nfocus_frame = true\n")
        );
    }

    #[test]
    fn what_widens_bateris_reach_is_read_and_never_written() {
        let root = Root::new("level");
        assert_eq!(root.run(&["get", "commands.level"]).stdout, "own\n");
        assert_eq!(root.run(&["get", "remote.integration"]).stdout, "ask\n");
        assert_eq!(root.run(&["get", "claude_skill"]).stdout, "false\n");
        for words in [
            &["set", "level", "full"][..],
            &["reset", "commands.level"],
            &["set", "remote.integration", "auto"],
            &["reset", "remote.integration"],
            &["set", "claude_skill", "true"],
            &["reset", "agents.claude_skill"],
        ] {
            let ran = root.run(words);
            assert_eq!(ran.status, 77, "{words:?}");
            assert!(ran.stderr.contains("changed in Settings"), "{}", ran.stderr);
            assert!(!ran.wrote);
        }
        assert_eq!(root.file(), None);
        // Each is a key of the template: a renamed one would leave the command free to write it.
        for path in SETTINGS_ONLY {
            assert!(plain_key(path).is_ok(), "{path} is no plain key");
        }
    }

    #[test]
    fn list_shows_each_setting_and_what_differs_from_its_default() {
        let root = Root::new("list");
        root.write(
            "[appearance]\ntheme = \"nord\"\nsplit_style = \"cards\"\n[font]\nsize = \"big\"\n",
        );
        let all = root.run(&["list"]);
        assert_eq!(all.status, 0, "{}", all.stderr);
        assert_eq!(all.stdout.lines().count(), plain_keys().len());
        assert!(all.stdout.contains("font.family"), "{}", all.stdout);
        let changed = root.run(&["list", "--changed"]);
        let lines: Vec<&str> = changed.stdout.lines().collect();
        assert_eq!(lines.len(), 2, "{}", changed.stdout);
        assert_eq!(
            lines[0],
            "appearance.theme = \"nord\"  (default \"system\")"
        );
        assert!(
            lines[1].starts_with("font.size        = \"big\"  (not accepted: "),
            "{}",
            lines[1]
        );
        let theme = root.run(&["list", "theme"]);
        assert_eq!(
            theme.stdout.lines().collect::<Vec<_>>(),
            [
                "appearance.theme       = \"nord\"  (default \"system\")",
                "appearance.light_theme = \"bateri-light\"",
                "appearance.dark_theme  = \"bateri\"",
            ]
        );
        assert_eq!(root.run(&["list", "nothing_like_it"]).status, 1);
    }

    #[test]
    fn a_mistake_is_64_and_says_what_to_write() {
        let root = Root::new("usage");
        for (words, said) in [
            (&[][..], "config takes list, get, set, reset or path"),
            (
                &["remove", "theme"],
                "config takes list, get, set, reset or path, not \"remove\"",
            ),
            (&["get"], "config get takes one KEY"),
            (&["set", "theme"], "config set takes a KEY and a VALUE"),
            (
                &["get", "nothing"],
                "no setting is called \"nothing\"; bateri config list shows them",
            ),
            (
                &["get", "integration"],
                "\"integration\" is in more than one section: shell.integration, remote.integration",
            ),
            (
                &["get", "remote.hosts"],
                "no setting is called \"remote.hosts\"; bateri config list shows them",
            ),
            (
                &["list", "--all"],
                "config list takes --changed, not \"--all\"",
            ),
        ] {
            let ran = root.run(words);
            assert_eq!(ran.status, 64, "{words:?}");
            assert_eq!(ran.stderr, format!("bateri: {said}\n"), "{words:?}");
        }
        let path = root.run(&["path"]);
        assert_eq!(
            path.stdout,
            format!("{}\n", root.0.join(FILE_NAME).display())
        );
    }
}
