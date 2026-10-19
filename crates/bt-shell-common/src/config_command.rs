//! `bateri config`: every setting read and changed from a shell, without the settings window —
//! over ssh, from a script, in a dotfiles bootstrap.
//!
//! ```text
//! bateri config list [--changed] [FILTER]   every key: value, default, values, meaning
//! bateri config get KEY                     the value in effect, plain (for scripts)
//! bateri config set KEY VALUE               checked by the app's own parser, then written
//! bateri config reset KEY                   back to the default
//! bateri config path                        the settings file's path
//! bateri config edit                        the file in $VISUAL / $EDITOR, or `open -t`
//! ```
//!
//! **Nothing here is a second list of settings.** The keys, their sections, their defaults,
//! the values they take and what they mean are read off [`Settings::TEMPLATE`] by
//! [`bt_core::template_keys`] — the template is the one place every setting is already written
//! with its default and its comment — so a key added there is a key here, with no edit in this
//! file. A `KEY` is the dotted path (`appearance.split_style`) or, where only one section has
//! it, the bare key (`split_style`).
//!
//! **Nothing here is a second writer or a second parser either.** `set` and `reset` write
//! through [`settings::write_value`], i.e. the settings window's writer
//! (`Settings::with_edit`'s body): only that key's value changes, comments, order and unknown
//! keys stay, a missing file is first created from the template (the "Open settings.toml"
//! path). The value is judged by [`Settings::parse`] — the reading the app does on every
//! save — and one it would reject is not written: the parser's sentence is printed with the
//! values the key takes, and the exit code is [`EXIT_USAGE`].
//!
//! The running app watches `~/.config/bateri/` (`watch`, [`settings::watched_paths`]) and
//! applies a write to `settings.toml` the way it applies an editor's save, so `set` takes effect
//! in every open window at once; `shell.integration` alone waits for the next shell, as it
//! does when changed anywhere else.

use std::io::Write;
use std::path::Path;

use bt_core::{Settings, TemplateKey, plain_value, template_keys};

use crate::cli::Style;
use crate::settings::{self, FILE_NAME};

/// The subcommand's word: `bateri config …`.
pub const SUBCOMMAND: &str = "config";

/// Bad usage, an unknown key, or a value the parser refuses — `EX_USAGE` (sysexits.h), the
/// code `main` gives an unknown subcommand.
pub const EXIT_USAGE: i32 = 64;

/// The settings file can't be read or parsed — `EX_CONFIG`. Nothing is written over it.
pub const EXIT_CONFIG: i32 = 78;

/// The file couldn't be created or written — `EX_IOERR`.
pub const EXIT_IO: i32 = 74;

/// The usage text, printed by `bateri config help` and after a usage error.
pub const USAGE: &str = "\
usage: bateri config list [--changed] [FILTER]   every setting: value, default, values, meaning
       bateri config get KEY                     the value in effect
       bateri config set KEY VALUE               change one setting (checked, comments kept)
       bateri config reset KEY                   put one setting back to its default
       bateri config path                        where the settings file is
       bateri config edit                        open the settings file in $EDITOR

KEY is section.key (appearance.split_style) or just the key (split_style).
Changes apply at once in every open bateri window.";

/// What the command needs from outside: the settings root (`~/.config/bateri`), where to
/// print, and how. The editor is launched by [`run_editor`], not through here.
pub struct Io<'a> {
    pub root: &'a Path,
    pub out: &'a mut dyn Write,
    pub err: &'a mut dyn Write,
    pub style: Style,
}

/// `bateri config ARGS…` (the arguments after `config`) → the exit code.
pub fn config_main(argv: &[String], io: &mut Io<'_>) -> i32 {
    let args: Vec<&str> = argv.iter().map(String::as_str).collect();
    let result = match args.as_slice() {
        ["list", rest @ ..] => list(rest, io),
        ["get", key] => get(key, io),
        ["set", key, value @ ..] if !value.is_empty() => set(key, &value.join(" "), io),
        ["reset", key] => reset(key, io),
        ["path"] => {
            let _ = writeln!(io.out, "{}", io.root.join(FILE_NAME).display());
            Ok(0)
        }
        ["edit"] => edit(io),
        ["help" | "--help" | "-h"] => {
            let _ = writeln!(io.out, "{USAGE}");
            Ok(0)
        }
        _ => Err(Failure::usage(String::new())),
    };
    match result {
        Ok(code) => code,
        Err(failure) => {
            if !failure.message.is_empty() {
                let _ = writeln!(
                    io.err,
                    "{} {}",
                    io.style.alert("bateri config:"),
                    failure.message
                );
            }
            if failure.code == EXIT_USAGE && failure.show_usage {
                let _ = writeln!(io.err, "{USAGE}");
            }
            failure.code
        }
    }
}

/// Why a command ended without doing its work.
struct Failure {
    code: i32,
    message: String,
    /// Print [`USAGE`] after the message: the command line itself was wrong.
    show_usage: bool,
}

impl Failure {
    fn usage(message: String) -> Self {
        Self {
            code: EXIT_USAGE,
            message,
            show_usage: true,
        }
    }

    /// A refused key or value: the arguments were well formed, the message says what's wrong.
    fn refused(message: String) -> Self {
        Self {
            code: EXIT_USAGE,
            message,
            show_usage: false,
        }
    }

    fn config(message: String) -> Self {
        Self {
            code: EXIT_CONFIG,
            message,
            show_usage: false,
        }
    }
}

/// The file's text, `None` for no file (every key at its default).
fn read(root: &Path) -> Result<Option<String>, Failure> {
    settings::file_text(root)
        .map_err(|err| Failure::config(format!("{FILE_NAME} could not be read: {err}")))
}

/// The file's text and the diagnostics the app has for it; a file that can't be parsed is the
/// app's own sentence.
fn read_parsed(root: &Path) -> Result<(String, Vec<bt_core::Diagnostic>), Failure> {
    let text = read(root)?.unwrap_or_default();
    let parsed = Settings::parse(&text).map_err(|d| Failure::config(settings::notice(&d)))?;
    Ok((text, parsed.diagnostics))
}

/// One key as the file has it.
struct Shown {
    /// The value in the file, TOML as written; the default when the file doesn't hold the key.
    value: String,
    /// The file holds the key and its value isn't the default.
    changed: bool,
    /// The app's diagnostic for the key, when it doesn't accept the file's value.
    refused: Option<String>,
}

fn shown(found: &TemplateKey, text: &str, diagnostics: &[bt_core::Diagnostic]) -> Shown {
    let written = found.written_in(text);
    let refused = diagnostics
        .iter()
        .find(|d| d.key == Some(found.path))
        .map(|d| d.message.clone());
    match written {
        Some(value) => Shown {
            changed: !found.is_default(&value),
            value,
            refused,
        },
        None => Shown {
            value: found.default_value().to_owned(),
            changed: false,
            refused,
        },
    }
}

/// The key a command names: the dotted path, or a bare key only one section has. An unknown
/// name lists the nearest keys.
pub fn resolve(name: &str) -> Result<&'static TemplateKey, String> {
    let keys = template_keys();
    if let Some(found) = keys.iter().find(|found| found.path == name) {
        return Ok(found);
    }
    let bare: Vec<&TemplateKey> = keys.iter().filter(|found| found.key == name).collect();
    match bare.as_slice() {
        [one] => return Ok(one),
        [] => {}
        many => {
            let paths: Vec<&str> = many.iter().map(|found| found.path).collect();
            return Err(format!(
                "`{name}` is in more than one section: {}",
                paths.join(", ")
            ));
        }
    }
    let near = nearest(name);
    Err(if near.is_empty() {
        format!("no setting `{name}`; `bateri config list` shows them all")
    } else {
        format!("no setting `{name}`; did you mean {}?", near.join(" or "))
    })
}

/// Up to three keys a mistyped `name` most likely meant: by edit distance to the path or the
/// bare key, or containing it.
fn nearest(name: &str) -> Vec<&'static str> {
    let name = name.to_lowercase();
    let mut scored: Vec<(usize, &'static str)> = template_keys()
        .iter()
        .filter_map(|found| {
            let distance = distance(&name, found.path).min(distance(&name, found.key));
            let near = distance <= (name.chars().count() / 3).max(2);
            let contains = name.len() >= 3 && found.path.contains(name.as_str());
            (near || contains).then_some((if near { distance } else { 3 }, found.path))
        })
        .collect();
    scored.sort();
    scored.into_iter().take(3).map(|(_, path)| path).collect()
}

/// Levenshtein distance, by characters.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (above + 1)
                .min(row[j] + 1)
                .min(diagonal + usize::from(ca != *cb));
            diagonal = above;
        }
    }
    row[b.len()]
}

fn list(args: &[&str], io: &mut Io<'_>) -> Result<i32, Failure> {
    let mut changed_only = false;
    let mut filter: Option<&str> = None;
    for arg in args {
        match *arg {
            "--changed" => changed_only = true,
            flag if flag.starts_with('-') => {
                return Err(Failure::usage(format!("unknown option `{flag}`")));
            }
            word if filter.is_none() => filter = Some(word),
            _ => return Err(Failure::usage("list takes one filter".to_owned())),
        }
    }
    let (text, diagnostics) = read_parsed(io.root)?;
    let style = io.style;
    let mut section = "";
    let mut printed = 0;
    for found in template_keys() {
        if filter.is_some_and(|filter| !found.path.contains(filter)) {
            continue;
        }
        let shown = shown(found, &text, &diagnostics);
        if changed_only && !shown.changed && shown.refused.is_none() {
            continue;
        }
        if found.section != section {
            section = found.section;
            let gap = if printed == 0 { "" } else { "\n" };
            let _ = writeln!(io.out, "{gap}{}", style.strong(&format!("[{section}]")));
        }
        printed += 1;
        let value = if shown.changed {
            style.mark(&shown.value)
        } else {
            shown.value.clone()
        };
        let mut line = format!("  {} = {value}", style.strong(found.key));
        if shown.changed {
            line.push_str(&style.quiet(&format!("  (default {})", found.default_value())));
        }
        let _ = writeln!(io.out, "{line}");
        let _ = writeln!(
            io.out,
            "      {}",
            style.quiet(&format!("{} — {}", found.allowed(), found.summary()))
        );
        if let Some(message) = shown.refused {
            let _ = writeln!(io.out, "      {}", style.alert(&format!("! {message}")));
        }
    }
    if printed == 0 {
        let what = if changed_only {
            "every setting is at its default"
        } else {
            "no setting matches"
        };
        let _ = writeln!(io.err, "{what}");
    }
    Ok(0)
}

fn get(name: &str, io: &mut Io<'_>) -> Result<i32, Failure> {
    let found = resolve(name).map_err(Failure::refused)?;
    let (text, diagnostics) = read_parsed(io.root)?;
    let shown = shown(found, &text, &diagnostics);
    let _ = writeln!(io.out, "{}", plain_value(&shown.value));
    if let Some(message) = shown.refused {
        let _ = writeln!(io.err, "{} {message}", io.style.alert("note:"));
    }
    Ok(0)
}

fn set(name: &str, raw: &str, io: &mut Io<'_>) -> Result<i32, Failure> {
    let found = resolve(name).map_err(Failure::refused)?;
    write(found, &found.toml_value(raw), io)
}

fn reset(name: &str, io: &mut Io<'_>) -> Result<i32, Failure> {
    let found = resolve(name).map_err(Failure::refused)?;
    write(found, found.default_value(), io)
}

/// Writes `value` (TOML) to `found`'s key. The verdict is asked first on the file as it is (or
/// the template, when there is none) so a refused value says why with the values the key takes;
/// the write itself asks again on the text it writes over ([`settings::write_value`]).
fn write(found: &TemplateKey, value: &str, io: &mut Io<'_>) -> Result<i32, Failure> {
    let before = read(io.root)?;
    let base = before.as_deref().unwrap_or(Settings::TEMPLATE);
    if let Err(diagnostic) = Settings::with_checked_value(base, found.path, value) {
        return Err(if diagnostic.key == Some(found.path) {
            // The parser's sentence without what the app would fall back to
            // (`; using "block"`): here nothing is used, nothing is written.
            let message = ["; using ", "; keeping "]
                .iter()
                .filter_map(|tail| diagnostic.message.find(tail))
                .min()
                .map_or(diagnostic.message.as_str(), |end| {
                    &diagnostic.message[..end]
                });
            Failure::refused(format!(
                "{message}; nothing was written\n  {} takes: {}",
                found.path,
                found.allowed()
            ))
        } else {
            Failure::config(settings::notice(&diagnostic))
        });
    }
    let was = before
        .as_deref()
        .and_then(|text| found.written_in(text))
        .unwrap_or_else(|| found.default_value().to_owned());
    settings::write_value(io.root, found.path, value).map_err(|message| Failure {
        code: EXIT_IO,
        message,
        show_usage: false,
    })?;
    let now = read(io.root)?
        .and_then(|text| found.written_in(&text))
        .unwrap_or_else(|| value.to_owned());
    let style = io.style;
    let mut line = format!("{} = {}", style.strong(found.path), style.accent(&now));
    if was != now {
        line.push_str(&style.quiet(&format!("  (was {was})")));
    }
    let _ = writeln!(io.out, "{line}");
    Ok(0)
}

fn edit(io: &mut Io<'_>) -> Result<i32, Failure> {
    let path = settings::create_if_missing(io.root).map_err(|err| Failure {
        code: EXIT_IO,
        message: format!("{FILE_NAME} could not be created: {err}"),
        show_usage: false,
    })?;
    run_editor(&path).map_err(|message| Failure {
        code: 1,
        message,
        show_usage: false,
    })
}

/// Opens `path` in the user's editor and waits: `$VISUAL`, then `$EDITOR` — through `sh`, so
/// a value with arguments (`code -w`) works as it does for git — or, with neither, `open -t`,
/// the default text editor (the settings window's "Open settings.toml" opens the file the same
/// way). The editor's exit code is the command's.
pub fn run_editor(path: &Path) -> Result<i32, String> {
    let editor = ["VISUAL", "EDITOR"]
        .iter()
        .filter_map(std::env::var_os)
        .find(|value| !value.is_empty());
    let status = match editor {
        Some(editor) => std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(format!("{} \"$1\"", editor.to_string_lossy()))
            .arg("sh")
            .arg(path)
            .status(),
        None => std::process::Command::new("/usr/bin/open")
            .arg("-t")
            .arg(path)
            .status(),
    };
    match status {
        Ok(status) => Ok(status.code().unwrap_or(1)),
        Err(err) => Err(format!(
            "the editor could not be started ({err}); the file is {}",
            path.display()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::TempRoot;

    /// Runs `bateri config ARGS` against `root`: (code, stdout, stderr).
    fn run(root: &Path, args: &[&str]) -> (i32, String, String) {
        let argv: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = config_main(
            &argv,
            &mut Io {
                root,
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

    fn file(root: &Path) -> String {
        std::fs::read_to_string(root.join(FILE_NAME)).expect("settings file")
    }

    #[test]
    fn get_reads_the_default_without_a_file_and_creates_nothing() {
        let root = TempRoot::new("cli-get-default");
        assert_eq!(
            run(&root.0, &["get", "split_style"]),
            (0, "cards\n".to_owned(), String::new())
        );
        assert_eq!(
            run(&root.0, &["get", "terminal.scrollback"]).1,
            "10000\n".to_owned()
        );
        assert!(!root.0.join(FILE_NAME).exists());
    }

    #[test]
    fn set_creates_the_template_and_changes_one_line() {
        let root = TempRoot::new("cli-set-template");
        let (code, out, err) = run(&root.0, &["set", "split_style", "lines"]);
        assert_eq!((code, err.as_str()), (0, ""));
        assert_eq!(out, "appearance.split_style = \"lines\"  (was \"cards\")\n");
        // The "Open settings.toml" template, with that one line changed.
        assert_eq!(
            file(&root.0),
            Settings::TEMPLATE.replace("split_style = \"cards\"", "split_style = \"lines\"")
        );
        assert_eq!(run(&root.0, &["get", "split_style"]).1, "lines\n");
    }

    #[test]
    fn set_keeps_comments_order_and_unknown_keys() {
        let root = TempRoot::new("cli-set-keeps");
        let text = "# mine\n[font]\nsize = 12 # small\nmystery = 1\n\n[appearance]\n# hi\ntheme = \"nord\"\n";
        std::fs::write(root.0.join(FILE_NAME), text).expect("write");
        assert_eq!(run(&root.0, &["set", "font.size", "14.5"]).0, 0);
        assert_eq!(file(&root.0), text.replace("size = 12 #", "size = 14.5 #"));
        // A key the file lacks is added to its section.
        assert_eq!(
            run(&root.0, &["set", "font.family", "JetBrains", "Mono"]).0,
            0
        );
        assert_eq!(
            file(&root.0),
            text.replace(
                "mystery = 1\n",
                "mystery = 1\nfamily = \"JetBrains Mono\"\n"
            )
            .replace("size = 12 #", "size = 14.5 #")
        );
    }

    #[test]
    fn a_refused_value_is_not_written_and_says_what_the_key_takes() {
        let root = TempRoot::new("cli-set-refused");
        let text = "[appearance]\nsplit_style = \"cards\"\n";
        std::fs::write(root.0.join(FILE_NAME), text).expect("write");
        for (key, value) in [
            ("split_style", "stripes"),
            ("terminal.scrollback", "-5"),
            ("font.line_height", "7"),
            ("tab_icons", "yes"),
            ("cursor_radius", "round"),
        ] {
            let (code, out, err) = run(&root.0, &["set", key, value]);
            assert_eq!(code, EXIT_USAGE, "{key} = {value}");
            assert_eq!(out, "");
            assert!(err.contains(" takes: "), "{err}");
            assert_eq!(file(&root.0), text, "{key} = {value} was written");
        }
        let (_, _, err) = run(&root.0, &["set", "split_style", "stripes"]);
        assert!(err.contains("\"cards\" | \"lines\""), "{err}");
        // Without a file a refused value creates none.
        let empty = TempRoot::new("cli-set-refused-empty");
        assert_eq!(run(&empty.0, &["set", "split_style", "x"]).0, EXIT_USAGE);
        assert!(!empty.0.join(FILE_NAME).exists());
    }

    #[test]
    fn a_file_that_cannot_be_parsed_is_not_written_over() {
        let root = TempRoot::new("cli-broken");
        let text = "[font\nsize = 12\n";
        std::fs::write(root.0.join(FILE_NAME), text).expect("write");
        assert_eq!(run(&root.0, &["set", "font.size", "14"]).0, EXIT_CONFIG);
        assert_eq!(run(&root.0, &["get", "font.size"]).0, EXIT_CONFIG);
        assert_eq!(file(&root.0), text);
    }

    #[test]
    fn reset_writes_the_default_in_place() {
        let root = TempRoot::new("cli-reset");
        let text = "[terminal]\n# keep\nscrollback = 500 # why\n";
        std::fs::write(root.0.join(FILE_NAME), text).expect("write");
        let (code, out, _) = run(&root.0, &["reset", "scrollback"]);
        assert_eq!(code, 0);
        assert_eq!(out, "terminal.scrollback = 10000  (was 500)\n");
        assert_eq!(
            file(&root.0),
            "[terminal]\n# keep\nscrollback = 10000 # why\n"
        );
        // The key without a default goes back to "none": the empty string.
        std::fs::write(root.0.join(FILE_NAME), "[font]\nfamily = \"Menlo\"\n").expect("write");
        assert_eq!(run(&root.0, &["reset", "font.family"]).0, 0);
        assert_eq!(file(&root.0), "[font]\nfamily = \"\"\n");
        assert_eq!(
            Settings::parse(&file(&root.0)).map(|p| p.settings.font.family),
            Ok(None)
        );
    }

    #[test]
    fn list_shows_every_template_key_and_changed_shows_the_changed() {
        let root = TempRoot::new("cli-list");
        let (code, out, _) = run(&root.0, &["list"]);
        assert_eq!(code, 0);
        // Every key the template has, under its section — none listed by hand.
        for found in template_keys() {
            assert!(
                out.contains(&format!("\n  {} = ", found.key))
                    || out.contains(&format!("]\n  {} = ", found.key)),
                "{} missing from list",
                found.path
            );
            assert!(out.contains(&format!("[{}]", found.section)));
        }
        assert!(out.contains("  split_style = \"cards\"\n      \"cards\" | \"lines\" — How a split tab sets its panes apart\n"));
        assert_eq!(run(&root.0, &["list", "--changed"]).1, "");

        std::fs::write(
            root.0.join(FILE_NAME),
            "[appearance]\nsplit_style = \"lines\"\ntab_icons = true\n[terminal]\ncursor = \"x\"\n",
        )
        .expect("write");
        let (_, changed, _) = run(&root.0, &["list", "--changed"]);
        assert!(
            changed.contains("split_style = \"lines\"  (default \"cards\")"),
            "{changed}"
        );
        // A value equal to the default isn't a change; a refused one is shown with why.
        assert!(!changed.contains("tab_icons"), "{changed}");
        assert!(changed.contains("cursor = \"x\""), "{changed}");
        assert!(changed.contains("! `terminal.cursor` must be"), "{changed}");
        // A filter narrows the list.
        let (_, font, _) = run(&root.0, &["list", "font."]);
        assert!(
            font.starts_with("[font]\n") && !font.contains("[terminal]"),
            "{font}"
        );
    }

    #[test]
    fn unknown_and_ambiguous_keys_are_refused_with_the_near_ones() {
        let root = TempRoot::new("cli-unknown");
        let (code, _, err) = run(&root.0, &["get", "split_styl"]);
        assert_eq!(code, EXIT_USAGE);
        assert!(err.contains("did you mean appearance.split_style"), "{err}");
        let (code, _, err) = run(&root.0, &["set", "integration", "off"]);
        assert_eq!(code, EXIT_USAGE);
        assert!(
            err.contains("shell.integration") && err.contains("remote.integration"),
            "{err}"
        );
        assert!(
            run(&root.0, &["get", "zzzzzzzzzz"])
                .2
                .contains("bateri config list")
        );
        assert!(!root.0.join(FILE_NAME).exists());
    }

    #[test]
    fn usage_errors_print_the_usage() {
        let root = TempRoot::new("cli-usage");
        for args in [
            &[][..],
            &["frobnicate"],
            &["get"],
            &["set", "theme"],
            &["list", "--x"],
        ] {
            let (code, _, err) = run(&root.0, args);
            assert_eq!(code, EXIT_USAGE, "{args:?}");
            assert!(err.contains("usage: bateri config"), "{args:?}: {err}");
        }
        let (code, out, _) = run(&root.0, &["--help"]);
        assert_eq!(code, 0);
        assert!(out.starts_with("usage: bateri config"));
        assert_eq!(
            run(&root.0, &["path"]).1,
            format!("{}\n", root.0.join(FILE_NAME).display())
        );
    }

    #[test]
    fn every_template_key_is_reachable_by_its_path_and_takes_its_default() {
        // The "nothing left out" check: each key the template has can be read, set to its
        // default and reset from the command line, by its dotted path.
        let root = TempRoot::new("cli-every");
        for found in template_keys() {
            assert_eq!(resolve(found.path).map(|k| k.path), Ok(found.path));
            assert_eq!(run(&root.0, &["get", found.path]).0, 0, "{}", found.path);
            assert_eq!(
                run(&root.0, &["set", found.path, found.default_value()]).0,
                0,
                "{}",
                found.path
            );
            assert_eq!(run(&root.0, &["reset", found.path]).0, 0, "{}", found.path);
        }
        // Set to every default, the file still reads as the defaults.
        assert_eq!(
            Settings::parse(&file(&root.0)).map(|p| p.settings),
            Ok(Settings::default())
        );
    }
}
