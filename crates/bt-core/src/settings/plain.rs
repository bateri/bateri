//! The settings as plain keys: what `bateri config` lists, reads and changes. A plain key is one
//! the template writes with a single value — a string, a number, `true` or `false` — named by its
//! dotted path (`appearance.split_style`), with the template's value as its default.
//!
//! **No second list of settings.** The keys are read off [`Settings::TEMPLATE`], where every
//! setting is already written with its default: a `key = value` line of a section, and a
//! `# key = value` line of a section that is no example (`# family = "Menlo"`, a key the template
//! leaves unset, which has no default). A key added to the template is a plain key with no other
//! edit. A list (`hosts`, `sources`) and the tables of `[keys]` and `[font.features]` are not:
//! they are changed in Settings or in the file.
//!
//! **No second writer or reader either.** [`Settings::with_plain`] writes through the settings
//! window's writer ([`Settings::with_edit`]'s body) and judges the result with
//! [`Settings::parse`], the reading the app does on every save: a value it would not accept is
//! not written.

use std::sync::OnceLock;

use toml_edit::{Document, Item, Value};

use super::keys::{entry_line, without_ranges};
use super::{Diagnostic, Settings, document, kind, line_of, rendered, write_value};

/// A setting the template writes with one value.
#[derive(Clone, Debug, PartialEq)]
pub struct PlainKey {
    /// The section, `appearance`.
    pub section: &'static str,
    /// The key in it, `split_style`.
    pub key: &'static str,
    /// Both, `appearance.split_style`: the name `bateri config` takes and prints.
    pub path: &'static str,
    /// The template's value; `None` for a key the template leaves unset (`font.family`).
    pub default: Option<PlainValue>,
    /// The kind of value the key takes: the default's, or the commented example's.
    kind: Kind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Text,
    Whole,
    Number,
    Switch,
}

/// One value as `bateri config` reads and writes it.
#[derive(Clone, Debug, PartialEq)]
pub enum PlainValue {
    Text(String),
    Whole(i64),
    Number(f64),
    Switch(bool),
}

impl PlainValue {
    /// As the file writes it: `"lines"`, `10000`, `0.1`, `true`.
    pub fn toml(&self) -> String {
        self.value().to_string().trim().to_owned()
    }

    /// As a script reads it: a string without its quotes, anything else as the file writes it.
    pub fn plain(&self) -> String {
        match self {
            PlainValue::Text(text) => text.clone(),
            _ => self.toml(),
        }
    }

    fn value(&self) -> Value {
        match self {
            PlainValue::Text(text) => Value::from(text.as_str()),
            PlainValue::Whole(number) => Value::from(*number),
            PlainValue::Number(number) => Value::from(*number),
            PlainValue::Switch(on) => Value::from(*on),
        }
    }

    fn of(value: &Value) -> Option<PlainValue> {
        match value {
            Value::String(text) => Some(PlainValue::Text(text.value().clone())),
            Value::Integer(number) => Some(PlainValue::Whole(*number.value())),
            Value::Float(number) => Some(PlainValue::Number(*number.value())),
            Value::Boolean(on) => Some(PlainValue::Switch(*on.value())),
            _ => None,
        }
    }

    fn kind(&self) -> Kind {
        match self {
            PlainValue::Text(_) => Kind::Text,
            PlainValue::Whole(_) => Kind::Whole,
            PlainValue::Number(_) => Kind::Number,
            PlainValue::Switch(_) => Kind::Switch,
        }
    }
}

/// The template's plain keys, in its order. Read once; the paths live as long as the process.
pub fn plain_keys() -> &'static [PlainKey] {
    static KEYS: OnceLock<Vec<PlainKey>> = OnceLock::new();
    KEYS.get_or_init(|| read_template(Settings::TEMPLATE))
}

/// The plain keys of a template's text, line by line: a section's `key = value`, and a
/// `# key = value` the section does not also write, unless it follows an example table's
/// commented header (`# [font.features."JetBrains Mono"]`). A line whose rest is no single TOML
/// value — prose (`# integration = true or false.`), a list — is no key. `[keys]` is a table of
/// shortcuts, not of settings.
fn read_template(text: &'static str) -> Vec<PlainKey> {
    let mut keys: Vec<PlainKey> = Vec::new();
    let mut section: Option<&'static str> = None;
    let mut example = false;
    for line in text.lines() {
        let line = line.trim_end();
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            section = Some(name);
            example = false;
            continue;
        }
        if line.starts_with("# [") {
            example = true;
            continue;
        }
        let (commented, entry) = match line.strip_prefix("# ") {
            Some(rest) => (true, rest),
            None => (false, line),
        };
        let Some(section) = section.filter(|name| *name != "keys") else {
            continue;
        };
        if commented && example {
            continue;
        }
        let Some((key, value)) = entry_of(entry) else {
            continue;
        };
        if keys
            .iter()
            .any(|known| known.section == section && known.key == key)
        {
            continue;
        }
        keys.push(PlainKey {
            section,
            key,
            path: Box::leak(format!("{section}.{key}").into_boxed_str()),
            kind: value.kind(),
            default: (!commented).then_some(value),
        });
    }
    // A key the template writes after a commented example of it takes the written one.
    let written: Vec<(&str, &str)> = keys
        .iter()
        .filter(|key| key.default.is_some())
        .map(|key| (key.section, key.key))
        .collect();
    keys.retain(|key| key.default.is_some() || !written.contains(&(key.section, key.key)));
    keys
}

/// `key = value` of one line, when the value is one plain value.
fn entry_of(line: &'static str) -> Option<(&'static str, PlainValue)> {
    let (key, _) = line.split_once(" = ")?;
    if key.is_empty()
        || !key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return None;
    }
    let parsed = Document::parse(line).ok()?;
    let value = parsed.as_table().get(key)?.as_value()?;
    Some((key, PlainValue::of(value)?))
}

/// The key `name` stands for: a path (`appearance.theme`), or a key alone that one section has
/// (`split_style`). `Err` with the paths a bare key is in more than one section of; empty when
/// nothing is called that.
pub fn plain_key(name: &str) -> Result<&'static PlainKey, Vec<&'static str>> {
    let keys = plain_keys();
    if let Some(key) = keys.iter().find(|key| key.path == name) {
        return Ok(key);
    }
    let bare: Vec<&'static PlainKey> = keys.iter().filter(|key| key.key == name).collect();
    match bare.as_slice() {
        [one] => Ok(one),
        _ => Err(bare.iter().map(|key| key.path).collect()),
    }
}

impl PlainKey {
    /// The value the file's text gives this key; `None` when the file does not write it. A key
    /// written as a list or a table is `Err`: it is no plain value there.
    pub fn read(&self, text: &str) -> Result<Option<PlainValue>, Diagnostic> {
        let parsed = document(text)?;
        let Some(item) = parsed
            .as_table()
            .get(self.section)
            .and_then(Item::as_table_like)
            .and_then(|table| table.get(self.key))
        else {
            return Ok(None);
        };
        match item.as_value().and_then(PlainValue::of) {
            Some(value) => Ok(Some(value)),
            None => Err(Diagnostic {
                key: Some(self.path),
                line: item.span().and_then(|span| line_of(text, span.start)),
                message: format!(
                    "`{}` is {} in the file, not one value",
                    self.path,
                    kind(item)
                ),
            }),
        }
    }

    /// The value `words` stand for, of the kind this key takes; `Err` says why not. A string may
    /// be typed bare (`Kode Mono`) or between double quotes, as `bateri config list` prints it.
    pub fn value_of(&self, words: &str) -> Result<PlainValue, String> {
        let path = self.path;
        match self.kind {
            Kind::Text => {
                let text = words
                    .strip_prefix('"')
                    .and_then(|rest| rest.strip_suffix('"'))
                    .unwrap_or(words);
                Ok(PlainValue::Text(text.to_owned()))
            }
            Kind::Whole => words
                .parse()
                .map(PlainValue::Whole)
                .map_err(|_| format!("{path} takes a whole number, not {words:?}")),
            Kind::Number => words
                .parse::<f64>()
                .ok()
                .filter(|number| number.is_finite())
                .map(PlainValue::Number)
                .ok_or_else(|| format!("{path} takes a number, not {words:?}")),
            Kind::Switch => match words {
                "true" => Ok(PlainValue::Switch(true)),
                "false" => Ok(PlainValue::Switch(false)),
                _ => Err(format!("{path} takes true or false, not {words:?}")),
            },
        }
    }
}

impl Settings {
    /// `bateri config`'s write: `key` takes `value`, or with `None` its line goes and the key is
    /// back to its default ("A key you delete goes back to its default", the template says).
    /// Every other byte stays, as with [`Settings::with_edit`], whose writer this is; a line that
    /// goes is cut out whole, its comment above it stays.
    ///
    /// The result is read as the app reads the file, and a value that reading does not accept is
    /// `Err` with its sentence: nothing it would refuse is written.
    pub fn with_plain(
        text: &str,
        key: &PlainKey,
        value: Option<&PlainValue>,
    ) -> Result<String, Diagnostic> {
        let written = match value {
            Some(value) => write_value(text, key.section, key.key, key.path, value.value())?,
            None => without_key(text, key)?,
        };
        let parsed = Settings::parse(&written)?;
        match parsed
            .diagnostics
            .into_iter()
            .find(|diagnostic| diagnostic.key == Some(key.path))
        {
            Some(refused) => Err(refused),
            None => Ok(written),
        }
    }
}

/// The text without `key`'s line. A line it has alone is cut out to the byte; a key written
/// otherwise (an inline table, a dotted key) is taken out by `toml_edit`.
fn without_key(text: &str, key: &PlainKey) -> Result<String, Diagnostic> {
    let parsed = document(text)?;
    let found = parsed
        .as_table()
        .get(key.section)
        .and_then(Item::as_table_like)
        .and_then(|table| table.get_key_value(key.key));
    let Some((name, item)) = found else {
        return Ok(text.to_owned());
    };
    if let (Some(start), Some(end)) = (name.span(), item.span())
        && let Some(line) = entry_line(text, start.start, end.end)
    {
        return Ok(without_ranges(text, vec![line]));
    }
    let mut doc = parsed.into_mut();
    if let Some(table) = doc.get_mut(key.section).and_then(Item::as_table_like_mut) {
        table.remove(key.key);
    }
    Ok(rendered(text, &doc))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(name: &str) -> &'static PlainKey {
        plain_key(name).unwrap_or_else(|_| panic!("{name} is a plain key"))
    }

    /// The line reading of the template finds what `toml_edit` finds in it: every section's
    /// single values but `[keys]`, in order, and the one key the template leaves unset.
    #[test]
    fn the_plain_keys_are_the_templates_single_values() {
        let parsed = Document::parse(Settings::TEMPLATE).expect("the template parses");
        let mut found = Vec::new();
        for (section, item) in parsed.as_table() {
            if section == "keys" {
                continue;
            }
            let table = item.as_table_like().expect("a section");
            for (name, value) in table.iter() {
                if value.as_value().and_then(PlainValue::of).is_some() {
                    found.push(format!("{section}.{name}"));
                }
            }
        }
        let written: Vec<String> = plain_keys()
            .iter()
            .filter(|key| key.default.is_some())
            .map(|key| key.path.to_owned())
            .collect();
        assert_eq!(written, found);
        let unset: Vec<&str> = plain_keys()
            .iter()
            .filter(|key| key.default.is_none())
            .map(|key| key.path)
            .collect();
        assert_eq!(unset, ["font.family"]);
        assert!(plain_key("keys.copy_last_output").is_err());
        assert!(plain_key("remote.hosts").is_err());
        assert!(plain_key("apps.sources").is_err());
    }

    /// Every default is one the app's reading accepts, written alone into an empty file — so a
    /// key's kind is right and `reset` cannot leave a value the app refuses.
    #[test]
    fn every_default_is_accepted_written_alone() {
        for key in plain_keys() {
            let value = key
                .default
                .clone()
                .unwrap_or_else(|| PlainValue::Text("Menlo".to_owned()));
            let written = Settings::with_plain("", key, Some(&value))
                .unwrap_or_else(|refused| panic!("{}: {refused}", key.path));
            assert_eq!(key.read(&written), Ok(Some(value)), "{}", key.path);
        }
    }

    #[test]
    fn a_key_is_its_path_or_a_key_one_section_has() {
        assert_eq!(key("split_style").path, "appearance.split_style");
        assert_eq!(key("appearance.split_style").path, "appearance.split_style");
        assert_eq!(key("family").path, "font.family");
        // `integration` is in [shell] and in [remote]: the path says which.
        let both = plain_key("integration").expect_err("two sections");
        assert_eq!(both, ["shell.integration", "remote.integration"]);
        assert_eq!(plain_key("nothing"), Err(Vec::new()));
    }

    #[test]
    fn words_become_the_kind_of_value_the_key_takes() {
        assert_eq!(
            key("font.family").value_of("Kode Mono"),
            Ok(PlainValue::Text("Kode Mono".to_owned()))
        );
        assert_eq!(
            key("theme").value_of("\"nord\""),
            Ok(PlainValue::Text("nord".to_owned()))
        );
        assert_eq!(
            key("scrollback").value_of("5000"),
            Ok(PlainValue::Whole(5000))
        );
        assert!(key("scrollback").value_of("5000.5").is_err());
        assert_eq!(
            key("line_height").value_of("1"),
            Ok(PlainValue::Number(1.0))
        );
        assert!(key("line_height").value_of("NaN").is_err());
        assert_eq!(
            key("focus_frame").value_of("true"),
            Ok(PlainValue::Switch(true))
        );
        assert!(key("focus_frame").value_of("yes").is_err());
        assert_eq!(PlainValue::Number(1.0).toml(), "1.0");
        assert_eq!(PlainValue::Text("a \"b\"".to_owned()).toml(), r#"'a "b"'"#);
        assert_eq!(PlainValue::Text("lines".to_owned()).plain(), "lines");
    }

    #[test]
    fn a_write_changes_one_value_and_keeps_every_other_byte() {
        let text =
            "# mine\n[appearance]\n# how it splits\nsplit_style = \"cards\" # note\nfuture = 1\n";
        let written = Settings::with_plain(
            text,
            key("split_style"),
            Some(&PlainValue::Text("lines".to_owned())),
        )
        .expect("written");
        assert_eq!(
            written,
            "# mine\n[appearance]\n# how it splits\nsplit_style = \"lines\" # note\nfuture = 1\n"
        );
        // A key the file does not write goes into its section; a missing section is added.
        let written = Settings::with_plain(
            "[appearance]\n",
            key("font.size"),
            Some(&PlainValue::Whole(14)),
        )
        .expect("written");
        assert_eq!(written, "[appearance]\n\n[font]\nsize = 14\n");
    }

    #[test]
    fn a_value_the_reading_refuses_is_not_written() {
        let refused = Settings::with_plain(
            "",
            key("split_style"),
            Some(&PlainValue::Text("dots".to_owned())),
        )
        .expect_err("refused");
        assert_eq!(refused.key, Some("appearance.split_style"));
        assert!(Settings::with_plain("", key("scrollback"), Some(&PlainValue::Whole(-1))).is_err());
        // Text the app cannot read is not written over.
        assert!(Settings::with_plain("[font", key("size"), Some(&PlainValue::Whole(14))).is_err());
    }

    #[test]
    fn a_reset_cuts_the_line_and_leaves_its_comment() {
        let text = "[font]\n# my font\nfamily = \"Menlo\" # was Hack\nsize = 14\n";
        assert_eq!(
            Settings::with_plain(text, key("family"), None).as_deref(),
            Ok("[font]\n# my font\nsize = 14\n")
        );
        // Written inline, the key is taken out of the table.
        let inline = "font = { family = \"Menlo\", size = 14 }\n";
        let written = Settings::with_plain(inline, key("family"), None).expect("written");
        assert_eq!(key("size").read(&written), Ok(Some(PlainValue::Whole(14))));
        assert_eq!(key("family").read(&written), Ok(None));
        // Nothing to take out is no change.
        assert_eq!(
            Settings::with_plain("[font]\n", key("family"), None).as_deref(),
            Ok("[font]\n")
        );
    }

    #[test]
    fn a_list_where_a_value_was_expected_is_read_as_no_value() {
        let text = "[appearance]\ntheme = [\"a\"]\n";
        let read = key("theme").read(text).expect_err("a list");
        assert_eq!(read.key, Some("appearance.theme"));
        assert_eq!(read.line, Some(2));
    }
}
