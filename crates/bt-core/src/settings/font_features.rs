//! `[font.features."Family"]`: the OpenType feature choices of a font, one
//! `tag = true|false` line each, kept per family.
//!
//! The reading leaves a diagnostic for what it cannot accept and keeps the
//! rest; the writing ([`with_feature`]) changes only the line it is asked
//! about, so a comment, another family and an unknown key stay in place.
//! What a tag means is the font's business (`bt-atlas`): this module only
//! knows it is four letters or digits.

use std::collections::BTreeMap;
use std::ops::Range;

use toml_edit::{DocumentMut, InlineTable, Item, Value};

use super::keys::{entry_line, without_ranges};
use super::{Diagnostic, document, ensure_section, kind, line_of, rendered, section};

/// The path every diagnostic of this table carries.
const KEY: &str = "font.features";

/// The key a family is kept under: trimmed and lowercased, so the lookup
/// ignores case. `""` is the default chain.
pub(super) fn family_key(name: &str) -> String {
    name.trim().to_lowercase()
}

/// An OpenType tag as the file writes it: four letters or digits (`ss01`,
/// `calt`). Whether the font has it is not asked here.
fn is_tag(tag: &str) -> bool {
    tag.len() == 4 && tag.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Reads `[font.features]`: a section of family tables, each a section of
/// `tag = true|false` lines. A family that is not a section, a name that is
/// not a tag and a value that is not a boolean each leave a diagnostic and
/// drop **their own line**; a value that is not a boolean keeps what `fallback`
/// had for it (the rule of every other key: the value that is not accepted does
/// not change its key).
pub(super) fn read(
    text: &str,
    item: &Item,
    fallback: &BTreeMap<String, BTreeMap<String, bool>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> BTreeMap<String, BTreeMap<String, bool>> {
    let line = |item: &Item| item.span().and_then(|span| line_of(text, span.start));
    let Some(families) = item.as_table_like() else {
        diagnostics.push(Diagnostic {
            key: Some(KEY),
            line: line(item),
            message: format!(
                "`{KEY}` must be a section of font families (`[{KEY}.\"JetBrains Mono\"]`), \
                 found {}; leaving the feature choices as they were",
                kind(item)
            ),
        });
        return fallback.clone();
    };
    let mut features: BTreeMap<String, BTreeMap<String, bool>> = BTreeMap::new();
    for (family, item) in families.iter() {
        let Some(tags) = item.as_table_like() else {
            diagnostics.push(Diagnostic {
                key: Some(KEY),
                line: line(item),
                message: format!(
                    "`{KEY}.\"{family}\"` must be a section of `tag = true` lines, found {}; \
                     ignoring it",
                    kind(item)
                ),
            });
            continue;
        };
        let held = family_key(family);
        for (tag, item) in tags.iter() {
            if !is_tag(tag) {
                diagnostics.push(Diagnostic {
                    key: Some(KEY),
                    line: line(item),
                    message: format!(
                        "`{tag}` under `{KEY}.\"{family}\"` is not a feature tag (four letters \
                         or digits, like `ss01`); ignoring it"
                    ),
                });
                continue;
            }
            let on = match item.as_bool() {
                Some(on) => Some(on),
                None => {
                    let previous = fallback.get(&held).and_then(|tags| tags.get(tag)).copied();
                    let using = match previous {
                        Some(on) => format!("using {on}"),
                        None => "ignoring it".to_owned(),
                    };
                    diagnostics.push(Diagnostic {
                        key: Some(KEY),
                        line: line(item),
                        message: format!(
                            "`{KEY}.\"{family}\".{tag}` must be true or false, found {}; {using}",
                            kind(item)
                        ),
                    });
                    previous
                }
            };
            if let Some(on) = on {
                features
                    .entry(held.clone())
                    .or_default()
                    .insert(tag.to_owned(), on);
            }
        }
    }
    features
}

/// The writing of [`SettingsEdit::FontFeature`](super::SettingsEdit): `on`
/// becomes `tag`'s line under `family`'s table, `None` removes the line.
///
/// The family is found ignoring case and is written as given when its table
/// is new. A `[font]`, `features` or family that is not a section is refused
/// (writing in its place would erase what is there); a tag whose value is not
/// a boolean is replaced, since that is what the write is for. Where several
/// tables name the family (`"Fira Code"` and `"fira code"`) the reading takes
/// the last, so the write goes to the last and a removal clears them all.
pub(super) fn with_feature(
    text: &str,
    family: &str,
    tag: &str,
    on: Option<bool>,
) -> Result<String, Diagnostic> {
    let refuse = |message: String| Diagnostic {
        key: Some(KEY),
        line: None,
        message,
    };
    if !is_tag(tag) {
        return Err(refuse(format!(
            "`{tag}` is not a feature tag (four letters or digits, like `ss01`)"
        )));
    }
    let parsed = document(text)?;
    let mut refused = Vec::new();
    let held = family_key(family);
    if let Some(font) = section(text, parsed.as_table(), "font", &mut refused)
        && let Some(features) = font.get("features")
    {
        match features.as_table_like() {
            None => refused.push(Diagnostic {
                key: Some(KEY),
                line: features.span().and_then(|span| line_of(text, span.start)),
                message: format!(
                    "`{KEY}` must be a section of font families, found {}",
                    kind(features)
                ),
            }),
            Some(families) => {
                for (name, item) in families.iter().filter(|(name, _)| family_key(name) == held) {
                    match item.as_table_like() {
                        None => refused.push(Diagnostic {
                            key: Some(KEY),
                            line: item.span().and_then(|span| line_of(text, span.start)),
                            message: format!(
                                "`{KEY}.\"{name}\"` must be a section, found {}",
                                kind(item)
                            ),
                        }),
                        Some(tags) => {
                            if let Some(entry) = tags.get(tag).filter(|entry| !entry.is_value()) {
                                refused.push(Diagnostic {
                                    key: Some(KEY),
                                    line: entry.span().and_then(|span| line_of(text, span.start)),
                                    message: format!(
                                        "`{KEY}.\"{name}\".{tag}` must be true or false, found {}",
                                        kind(entry)
                                    ),
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    if let Some(diagnostic) = refused.pop() {
        return Err(diagnostic);
    }
    match on {
        Some(on) => Ok(written(text, parsed, &held, family, tag, on)),
        None => Ok(removed(text, parsed, &held, tag)),
    }
}

/// The text with `tag = on` under the last table of the family, the table
/// made when there is none.
fn written(
    text: &str,
    parsed: toml_edit::Document<&str>,
    held: &str,
    family: &str,
    tag: &str,
    on: bool,
) -> String {
    let mut doc = parsed.into_mut();
    ensure_section(&mut doc, "font");
    // A comment at the end of the document belongs to the table it was written
    // under: a new table is written in front of it, as `ensure_section` does a
    // section, or the `# family = "Menlo"` under `[font]` would pass to the new
    // table and the line of a user who uncommented it would be read there.
    let trailing = doc.trailing().as_str().unwrap_or_default().to_owned();
    let mut took_trailing = false;
    // `section` above has seen that `[font]` is a section, and `ensure_section`
    // made it when it was missing.
    let Some(font) = doc.get_mut("font").and_then(Item::as_table_like_mut) else {
        return text.to_owned();
    };
    if font.get("features").is_none() {
        // Implicit: only the family's header is written, not an empty
        // `[font.features]` above it.
        let mut features = toml_edit::Table::new();
        features.set_implicit(true);
        font.insert("features", Item::Table(features));
    }
    let Some(features) = font.get_mut("features") else {
        return text.to_owned();
    };
    // The container's spelling decides the family's: a table gets a header
    // table, an inline table an inline one.
    let inline = features.is_value();
    let Some(families) = features.as_table_like_mut() else {
        return text.to_owned();
    };
    let at = families
        .iter()
        .filter(|(name, _)| family_key(name) == held)
        .map(|(name, _)| name.to_owned())
        .last();
    let name = at.unwrap_or_else(|| {
        let item = if inline {
            Item::Value(Value::InlineTable(InlineTable::new()))
        } else {
            let mut table = toml_edit::Table::new();
            if !trailing.trim().is_empty() {
                table.decor_mut().set_prefix(format!("{trailing}\n"));
                took_trailing = true;
            }
            Item::Table(table)
        };
        families.insert(family.trim(), item);
        family.trim().to_owned()
    });
    if let Some(tags) = families.get_mut(&name).and_then(Item::as_table_like_mut) {
        match tags.get_mut(tag).and_then(Item::as_value_mut) {
            // The decor (the space after `=`, the comment at the end of the line) sits on
            // the value, and the new one inherits it.
            Some(old) => {
                let decor = old.decor().clone();
                *old = Value::from(on);
                *old.decor_mut() = decor;
            }
            None => {
                tags.insert(tag, Item::Value(Value::from(on)));
            }
        }
    }
    if took_trailing {
        doc.set_trailing("");
    }
    rendered(text, &doc)
}

/// The text without the `tag` line of every table that names the family. A
/// line alone on its own is cut out of the text itself, so what is around it
/// stays to the byte; `toml_edit` takes out the rest (a dotted key, an inline
/// table). A table left empty keeps its header.
fn removed(text: &str, parsed: toml_edit::Document<&str>, held: &str, tag: &str) -> String {
    let families = |root: &toml_edit::Table| -> Vec<String> {
        root.get("font")
            .and_then(Item::as_table_like)
            .and_then(|font| font.get("features"))
            .and_then(Item::as_table_like)
            .map(|families| {
                families
                    .iter()
                    .filter(|(name, item)| {
                        family_key(name) == held
                            && item
                                .as_table_like()
                                .is_some_and(|tags| tags.contains_key(tag))
                    })
                    .map(|(name, _)| name.to_owned())
                    .collect()
            })
            .unwrap_or_default()
    };
    let names = families(parsed.as_table());
    if names.is_empty() {
        return text.to_owned();
    }
    let ranges: Option<Vec<Range<usize>>> = names
        .iter()
        .map(|name| {
            let tags = parsed
                .as_table()
                .get("font")?
                .as_table_like()?
                .get("features")?
                .as_table_like()?
                .get(name)?
                .as_table_like()?;
            let (key, item) = tags.get_key_value(tag)?;
            entry_line(text, key.span()?.start, item.span()?.end)
        })
        .collect();
    if let Some(ranges) = ranges {
        return without_ranges(text, ranges);
    }
    let mut doc: DocumentMut = parsed.into_mut();
    for name in &names {
        if let Some(tags) = doc
            .get_mut("font")
            .and_then(Item::as_table_like_mut)
            .and_then(|font| font.get_mut("features"))
            .and_then(Item::as_table_like_mut)
            .and_then(|families| families.get_mut(name))
            .and_then(Item::as_table_like_mut)
        {
            tags.remove(tag);
        }
    }
    rendered(text, &doc)
}

#[cfg(test)]
mod tests {
    use super::super::{FontOptions, Settings, SettingsEdit};

    fn clean(text: &str) -> Settings {
        let parsed = Settings::parse(text).expect("parseable text");
        assert_eq!(
            parsed.diagnostics,
            Vec::new(),
            "no diagnostic expected: {text}"
        );
        parsed.settings
    }

    fn edit(family: &str, tag: &str, on: Option<bool>) -> SettingsEdit {
        SettingsEdit::FontFeature {
            family: family.to_owned(),
            tag: tag.to_owned(),
            on,
        }
    }

    fn write(text: &str, family: &str, tag: &str, on: Option<bool>) -> String {
        Settings::with_edit(text, &edit(family, tag, on)).expect("writable text")
    }

    #[test]
    fn features_are_read_per_family_and_ignore_case() {
        let text = "[font]\nfamily = \"jetbrains MONO\"\n\n\
                    [font.features.\"JetBrains Mono\"]\ncalt = false\nss19 = true\n\n\
                    [font.features.\"\"]\nzero = true\n";
        let font = clean(text).font;
        // The family in effect, found ignoring case; the default chain's table is its own.
        assert_eq!(font.feature("calt"), Some(false));
        assert_eq!(font.feature("ss19"), Some(true));
        assert_eq!(font.feature("zero"), None);
        assert_eq!(
            font.active_features(),
            vec![("calt".to_owned(), false), ("ss19".to_owned(), true)]
        );
        let chain = clean("[font.features.\"\"]\nzero = true\n").font;
        assert_eq!(chain.active_features(), vec![("zero".to_owned(), true)]);
        // Two spellings of one family are one record.
        let merged = clean(
            "[font]\nfamily = \"Fira Code\"\n[font.features.\"Fira Code\"]\ncalt = false\n\
             [font.features.\"fira code\"]\nss01 = true\n",
        )
        .font;
        assert_eq!(merged.active_features().len(), 2);
        // Nothing chosen, nothing listed; a family without a table has none.
        assert!(clean("").font.active_features().is_empty());
        assert!(font_of_other(&font).active_features().is_empty());
    }

    fn font_of_other(font: &FontOptions) -> FontOptions {
        FontOptions {
            family: Some("Menlo".to_owned()),
            ..font.clone()
        }
    }

    #[test]
    fn a_malformed_feature_drops_only_its_own_line() {
        let text = "[font.features.\"X\"]\ncalt = false\nss01 = \"yes\"\ntoolong = true\nzero = true\n\
                    [font.features]\nY = 5\n";
        let parsed = Settings::parse(text).expect("parseable text");
        let paths: Vec<_> = parsed.diagnostics.iter().map(|d| d.key).collect();
        assert_eq!(
            paths,
            vec![Some("font.features"); 3],
            "{:?}",
            parsed.diagnostics
        );
        // The lines around the bad ones are read.
        let features = &parsed.settings.font.features["x"];
        assert_eq!(features.len(), 2);
        assert!(!features["calt"]);
        assert!(features["zero"]);
        // The line of a diagnostic is its own.
        assert_eq!(parsed.diagnostics[0].line, Some(3));
        // `features` itself that is no section leaves the choices as they were.
        let current = Settings {
            font: FontOptions {
                features: features_of("x", "calt", true),
                ..FontOptions::default()
            },
            ..Settings::default()
        };
        let parsed = Settings::parse_keeping("[font]\nfeatures = 3\n", &current);
        assert_eq!(
            parsed.expect("parseable").settings.font.features,
            current.font.features
        );
        // A value that is no boolean keeps the previous value of its own key.
        let parsed = Settings::parse_keeping("[font.features.x]\ncalt = \"no\"\n", &current)
            .expect("parseable");
        assert_eq!(parsed.settings.font.features, current.font.features);
        assert_eq!(parsed.diagnostics.len(), 1);
    }

    fn features_of(
        family: &str,
        tag: &str,
        on: bool,
    ) -> std::collections::BTreeMap<String, std::collections::BTreeMap<String, bool>> {
        [(family.to_owned(), [(tag.to_owned(), on)].into())].into()
    }

    #[test]
    fn a_feature_is_written_into_a_new_table_and_read_back() {
        let text = "# my settings\n[font]\nsize = 14 # points\n\n[notes]\nx = 1\n";
        let written = write(text, "JetBrains Mono", "calt", Some(false));
        assert!(
            written.starts_with("# my settings\n[font]\nsize = 14 # points\n"),
            "{written}"
        );
        assert!(
            written.contains("[font.features.\"JetBrains Mono\"]\ncalt = false\n"),
            "{written}"
        );
        assert!(written.contains("[notes]\nx = 1\n"), "{written}");
        assert_eq!(
            clean(&written).font.features,
            features_of("jetbrains mono", "calt", false)
        );
        // No `[font]` at all, and an empty file.
        for text in ["", "[appearance]\ntheme = \"x\"\n"] {
            let written = write(text, "Obel Mono", "ss01", Some(true));
            assert_eq!(
                clean(&written).font.features,
                features_of("obel mono", "ss01", true)
            );
            assert!(clean(&written).font.family.is_none());
        }
    }

    #[test]
    fn a_comment_at_the_end_stays_under_the_table_it_was_written_under() {
        let text = "[font]\nsize = 1\n# family = \"Menlo\"\n";
        let written = write(text, "Obel Mono", "ss01", Some(true));
        let comment = written.find("# family").expect("the comment");
        let header = written.find("[font.features").expect("the header");
        assert!(comment < header, "{written}");
        // Uncommented, it is the font's family, not a feature.
        let uncommented = written.replace("# family", "family");
        assert_eq!(clean(&uncommented).font.family.as_deref(), Some("Menlo"));
    }

    #[test]
    fn a_feature_write_keeps_comments_and_unknown_keys() {
        let text = "[font]\nsize = 13\n\n[font.features.\"Obel Mono\"]\n\
                    # the secret mask\nss01 = false # keep\nfuture = true\n\
                    [font.features.\"Other\"]\ncalt = true\n";
        // The line's value changes in place, its comment stays, case of the family is not asked.
        let written = write(text, "obel mono", "ss01", Some(true));
        assert!(
            written.contains("# the secret mask\nss01 = true # keep\nfuture = true\n"),
            "{written}"
        );
        assert!(
            written.contains("[font.features.\"Other\"]\ncalt = true\n"),
            "{written}"
        );
        assert!(
            !written.contains("\"obel mono\""),
            "a second table was born: {written}"
        );
        // A new tag lands in the existing table, not in a second one.
        let written = write(text, "OBEL MONO", "cv02", Some(true));
        assert_eq!(written.matches("[font.features.").count(), 2, "{written}");
        assert!(written.contains("future = true\n"), "{written}");
        // `future` is no tag, so the reading leaves a diagnostic for that one line
        // and takes the rest.
        let parsed = Settings::parse(&written).expect("parseable text");
        assert_eq!(parsed.diagnostics.len(), 1);
        assert_eq!(parsed.settings.font.features["obel mono"].len(), 2);
    }

    #[test]
    fn a_feature_is_removed_by_cutting_its_line() {
        let text = "[font.features.\"Obel Mono\"]\n# keeps\nss01 = true # secret\ncv02 = true\n\n[notes]\nx = 1\n";
        let written = write(text, "Obel Mono", "ss01", None);
        assert_eq!(
            written,
            "[font.features.\"Obel Mono\"]\n# keeps\ncv02 = true\n\n[notes]\nx = 1\n"
        );
        // Removing what is not there changes nothing; so does an unknown family.
        assert_eq!(write(text, "Obel Mono", "zero", None), text);
        assert_eq!(write(text, "Menlo", "ss01", None), text);
        // Two spellings of the family: the removal clears both, so the reading is off.
        let two = "[font.features.\"Fira Code\"]\ncalt = false\n[font.features.\"fira code\"]\ncalt = true\n";
        let written = write(two, "fira code", "calt", None);
        assert!(clean(&written).font.features.is_empty(), "{written}");
        // The last table wins on reading, so a write goes to the last.
        let written = write(two, "FIRA CODE", "calt", Some(false));
        assert_eq!(
            clean(&written).font.features,
            features_of("fira code", "calt", false)
        );
    }

    #[test]
    fn every_spelling_of_the_tables_is_written_in_its_own_form() {
        // Dotted and inline spellings go through `toml_edit` and keep their form.
        let dotted = "[font]\nfeatures.\"Obel Mono\".ss01 = false\n";
        let written = write(dotted, "Obel Mono", "ss01", Some(true));
        assert_eq!(written, "[font]\nfeatures.\"Obel Mono\".ss01 = true\n");
        let written = write(dotted, "Obel Mono", "ss01", None);
        assert!(clean(&written).font.features.is_empty(), "{written}");
        let inline = "[font]\nfeatures = { \"Obel Mono\" = { ss01 = false } }\n";
        let written = write(inline, "Obel Mono", "ss01", Some(true));
        assert_eq!(
            written,
            "[font]\nfeatures = { \"Obel Mono\" = { ss01 = true } }\n"
        );
        let written = write(inline, "Menlo", "calt", Some(true));
        assert_eq!(clean(&written).font.features.len(), 2, "{written}");
    }

    #[test]
    fn what_cannot_be_written_into_is_refused() {
        for text in [
            "font = 3\n",
            "[font]\nfeatures = 3\n",
            "[font.features]\n\"Obel Mono\" = 3\n",
            "[font.features.\"Obel Mono\".ss01]\nx = 1\n",
        ] {
            let err =
                Settings::with_edit(text, &edit("Obel Mono", "ss01", Some(true))).expect_err(text);
            assert!(
                matches!(err.key, Some("font" | "font.features")),
                "{text}: {err:?}"
            );
        }
        // A tag that is not four letters or digits is refused before the file is read.
        for tag in ["", "ss1", "toolong", "ss 1", "ss0\n"] {
            Settings::with_edit("", &edit("Obel Mono", tag, Some(true))).expect_err(tag);
        }
        // Text that is not TOML is not written over.
        Settings::with_edit("[font", &edit("Obel Mono", "ss01", Some(true))).expect_err("syntax");
    }

    #[test]
    fn crlf_files_stay_crlf() {
        let text = "[font]\r\nsize = 13\r\n";
        let written = write(text, "Obel Mono", "ss01", Some(true));
        assert!(!written.replace("\r\n", "").contains('\n'), "{written:?}");
        assert!(written.contains("ss01 = true"));
    }

    #[test]
    fn the_atlas_is_asked_in_tag_order() {
        let text = "[font.features.\"\"]\nzero = true\nss01 = true\ncalt = false\n";
        let tags: Vec<String> = clean(text)
            .font
            .active_features()
            .into_iter()
            .map(|(tag, _)| tag)
            .collect();
        assert_eq!(tags, ["calt", "ss01", "zero"]);
    }
}
