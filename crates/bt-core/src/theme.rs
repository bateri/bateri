//! The theme file: the pure path from the text of `themes/{name}.toml` to a
//! [`Theme`].
//!
//! It does **not** see the file system: `bt-shell` resolves the name to a file
//! or an embedded theme and reads the text. Its explanation to the user is
//! `docs/SETTINGS.md` → Themes.
//!
//! **Read on top of a base:** every key is optional, a missing key comes from
//! the base — a user can copy an embedded theme and leave only what they
//! changed. The error rule is the same as the settings file's: text that
//! cannot be parsed is a separate result (`Err`), a color that is not accepted
//! in parsed text takes the base's value and leaves a diagnostic, an unknown
//! key is silent — a status role a later version adds must not count as an
//! error in an earlier one.

use toml_edit::TableLike;

use crate::color::Theme;
use crate::settings::{Diagnostic, document, kind, line_of, section};

/// The keys of the `[ansi]` section, in [`Theme::ansi`] order; the second is
/// the dotted path in the diagnostic (`Diagnostic::key` wants `'static`).
const ANSI_KEYS: [(&str, &str); 16] = [
    ("black", "ansi.black"),
    ("red", "ansi.red"),
    ("green", "ansi.green"),
    ("yellow", "ansi.yellow"),
    ("blue", "ansi.blue"),
    ("magenta", "ansi.magenta"),
    ("cyan", "ansi.cyan"),
    ("white", "ansi.white"),
    ("bright_black", "ansi.bright_black"),
    ("bright_red", "ansi.bright_red"),
    ("bright_green", "ansi.bright_green"),
    ("bright_yellow", "ansi.bright_yellow"),
    ("bright_blue", "ansi.bright_blue"),
    ("bright_magenta", "ansi.bright_magenta"),
    ("bright_cyan", "ansi.bright_cyan"),
    ("bright_white", "ansi.bright_white"),
];

impl Theme {
    /// The theme file's text → the theme read on top of `base` + diagnostics,
    /// or unparseable.
    ///
    /// Roles (`background`, `foreground`, `dim`, `accent`, `cursor`,
    /// `selection`, `search_match`, `search_current`, `success`, `error`,
    /// `info`, `warning`) at the root, the 16 colors
    /// in the `[ansi]` section; a color is `"#rrggbb"` (uppercase is fine too).
    /// `Err` only on invalid TOML, in the same sense as in the settings file.
    ///
    /// The base is a parameter, not a constant: in production it is always
    /// `Theme::BATERI` (where a missing key comes from is `bt-shell`'s
    /// decision), but the test of the theme block in the document reads with a
    /// base whose **every value is distinct**, so that a missing key is not
    /// silently filled from the base.
    pub fn parse(text: &str, base: &Theme) -> Result<(Theme, Vec<Diagnostic>), Diagnostic> {
        let doc = document(text)?;
        let root = doc.as_table();
        let mut theme = *base;
        let mut diagnostics = Vec::new();
        let roles = [
            ("background", &mut theme.background),
            ("foreground", &mut theme.foreground),
            ("dim", &mut theme.dim),
            ("accent", &mut theme.accent),
            ("cursor", &mut theme.cursor),
            ("selection", &mut theme.selection),
            ("search_match", &mut theme.search_match),
            ("search_current", &mut theme.search_current),
            ("success", &mut theme.success),
            ("error", &mut theme.error),
            ("info", &mut theme.info),
            ("warning", &mut theme.warning),
        ];
        for (key, slot) in roles {
            read_color(text, root, key, key, slot, &mut diagnostics);
        }
        if let Some(ansi) = section(text, root, "ansi", &mut diagnostics) {
            for ((key, path), slot) in ANSI_KEYS.into_iter().zip(&mut theme.ansi) {
                read_color(text, ansi, key, path, slot, &mut diagnostics);
            }
        }
        Ok((theme, diagnostics))
    }
}

/// Reads one color key; if absent it leaves the slot alone, if not accepted it
/// leaves a diagnostic and the base value in the slot stays.
fn read_color(
    text: &str,
    table: &dyn TableLike,
    key: &str,
    path: &'static str,
    slot: &mut u32,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(item) = table.get(key) else {
        return;
    };
    let found = match item.as_str() {
        Some(value) => match hex_color(value) {
            Some(color) => {
                *slot = color;
                return;
            }
            None => format!("{value:?}"),
        },
        None => kind(item).to_owned(),
    };
    diagnostics.push(Diagnostic {
        key: Some(path),
        line: item.span().and_then(|span| line_of(text, span.start)),
        message: format!(
            "`{path}` must be a color like \"#rrggbb\", found {found}; using #{:06x}",
            *slot
        ),
    });
}

/// `"#rrggbb"` → `0xRRGGBB`. No short (`#rgb`) or alpha (`#rrggbbaa`) form:
/// one form, one diagnostic.
pub(crate) fn hex_color(value: &str) -> Option<u32> {
    let digits = value.strip_prefix('#')?;
    // `from_str_radix` accepts a leading `+`; check for six digits first.
    if digits.len() != 6 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(digits, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A base whose every value is distinct from `Theme::BATERI` and from each
    /// other: when a missing key is read, the value that comes from the base
    /// must not be confused with any real color.
    const SENTINEL: Theme = Theme {
        background: 0x000001,
        foreground: 0x000002,
        dim: 0x000003,
        accent: 0x000004,
        cursor: 0x000007,
        selection: 0x000008,
        search_match: 0x000009,
        search_current: 0x00000a,
        success: 0x000005,
        error: 0x000006,
        info: 0x00000b,
        warning: 0x00000c,
        ansi: [
            0x000010, 0x000011, 0x000012, 0x000013, 0x000014, 0x000015, 0x000016, 0x000017,
            0x000018, 0x000019, 0x00001a, 0x00001b, 0x00001c, 0x00001d, 0x00001e, 0x00001f,
        ],
    };

    fn clean(text: &str, base: &Theme) -> Theme {
        let (theme, diagnostics) = Theme::parse(text, base).expect("parseable text");
        assert_eq!(
            diagnostics,
            Vec::new(),
            "no diagnostic was expected: {text}"
        );
        theme
    }

    #[test]
    fn empty_theme_is_the_base() {
        assert_eq!(clean("", &SENTINEL), SENTINEL);
        assert_eq!(clean("# comment only\n", &Theme::BATERI), Theme::BATERI);
    }

    #[test]
    fn the_cursor_role_reads_like_every_other() {
        // **One rule, no exceptions** (user decision, 2026-09-19). `cursor`
        // used to fall back to `accent` when missing; the reason was "theme
        // files written before the role should not change", but since the app
        // was not released there was no one it protected, and it was the
        // **only** exception among 20+ keys.
        let theme = clean("accent = \"#ff0000\"\ncursor = \"#00ff00\"\n", &SENTINEL);
        assert_eq!((theme.accent, theme.cursor), (0xff0000, 0x00ff00));
        // Writing only `accent` does **not** affect the cursor: it comes from the base.
        let theme = clean("accent = \"#ff0000\"\n", &SENTINEL);
        assert_eq!((theme.accent, theme.cursor), (0xff0000, SENTINEL.cursor));
        // A value that is not accepted leaves the slot at the base and leaves a diagnostic.
        let (theme, diagnostics) =
            Theme::parse("cursor = \"yeşil\"\n", &SENTINEL).expect("parseable text");
        assert_eq!(theme.cursor, SENTINEL.cursor);
        assert_eq!(diagnostics.len(), 1, "no diagnostic: {diagnostics:?}");
    }

    #[test]
    fn partial_theme_fills_from_the_base() {
        let theme = clean(
            "background = \"#FFFFFF\"\naccent = \"#ff0000\"\n[ansi]\nbright_white = \"#010203\"\n",
            &Theme::BATERI,
        );
        assert_eq!(
            theme,
            Theme {
                background: 0xffffff,
                accent: 0xff0000,
                ansi: {
                    let mut ansi = Theme::BATERI.ansi;
                    ansi[15] = 0x010203;
                    ansi
                },
                ..Theme::BATERI
            }
        );
    }

    #[test]
    fn ansi_keys_follow_the_palette_order() {
        let text = ANSI_KEYS
            .iter()
            .enumerate()
            .map(|(i, (key, _))| format!("{key} = \"#0000{:02x}\"\n", 0xa0 + i))
            .collect::<String>();
        let theme = clean(&format!("[ansi]\n{text}"), &SENTINEL);
        let expected: Vec<u32> = (0..16).map(|i| 0xa0 + i).collect();
        assert_eq!(theme.ansi.to_vec(), expected);
        // The names are in order too: 1 red, 9 bright red.
        assert_eq!((ANSI_KEYS[1].0, ANSI_KEYS[9].0), ("red", "bright_red"));
    }

    #[test]
    fn bad_color_keeps_the_base_value_with_diagnostic() {
        let (theme, diagnostics) = Theme::parse(
            "background = \"#12345\"\n[ansi]\nred = 16711680\ngreen = \"#+12345\"\n",
            &Theme::BATERI,
        )
        .expect("parseable text");
        assert_eq!(theme, Theme::BATERI);
        let lines: Vec<_> = diagnostics.iter().map(ToString::to_string).collect();
        assert_eq!(
            lines,
            [
                "line 1: `background` must be a color like \"#rrggbb\", found \"#12345\"; using #000000",
                "line 3: `ansi.red` must be a color like \"#rrggbb\", found an integer; using #d16d6a",
                "line 4: `ansi.green` must be a color like \"#rrggbb\", found \"#+12345\"; using #8bb58b",
            ]
        );
        assert_eq!(diagnostics[1].key, Some("ansi.red"));
    }

    #[test]
    fn ansi_of_wrong_type_is_diagnosed() {
        let (theme, diagnostics) =
            Theme::parse("ansi = \"#ffffff\"\n", &Theme::BATERI).expect("parseable text");
        assert_eq!(theme, Theme::BATERI);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].key, Some("ansi"));
    }

    #[test]
    fn unknown_keys_are_silent() {
        // A status role and other terminals' extra keys. The sentinel changed
        // once: `success` is now a known key, and if the test used it as the
        // example it would silently ask nothing.
        let theme = clean(
            "name = \"x\"\nwarning = \"#00ff00\"\n[ansi]\nred = \"#ff0000\"\norange = 1\n[meta]\n",
            &Theme::BATERI,
        );
        assert_eq!(theme.ansi[1], 0xff0000);
    }

    #[test]
    fn status_roles_are_read_and_inherited() {
        // The written role is read…
        let theme = clean("success = \"#0a0b0c\"\n", &SENTINEL);
        assert_eq!(
            theme,
            Theme {
                success: 0x0a0b0c,
                ..SENTINEL
            }
        );
        // …the unwritten role comes from the base. This is the migration
        // sentence: a user who wrote their own theme inherits the two roles
        // from the embedded `bateri`.
        let inherited = clean("background = \"#ffffff\"\n", &Theme::BATERI);
        assert_eq!(
            (inherited.success, inherited.error),
            (Theme::BATERI.success, Theme::BATERI.error)
        );
    }

    #[test]
    fn the_info_role_is_read_and_inherited() {
        // The rule has no exception — the written one is read, the unwritten one comes from the base.
        let theme = clean("info = \"#0a0b0c\"\n", &SENTINEL);
        assert_eq!(
            theme,
            Theme {
                info: 0x0a0b0c,
                ..SENTINEL
            }
        );
        let inherited = clean("background = \"#ffffff\"\n", &Theme::BATERI);
        assert_eq!(inherited.info, Theme::BATERI.info);
    }

    #[test]
    fn the_warning_role_is_read_and_inherited() {
        // `info`'s rule — optional, from the base if missing.
        let theme = clean("warning = \"#0a0b0c\"\n", &SENTINEL);
        assert_eq!(
            theme,
            Theme {
                warning: 0x0a0b0c,
                ..SENTINEL
            }
        );
        let inherited = clean("background = \"#ffffff\"\n", &Theme::BATERI);
        assert_eq!(inherited.warning, Theme::BATERI.warning);
    }

    #[test]
    fn search_roles_are_read_and_inherited() {
        // Neither of the two search roles is an exception to the rule: the written
        // one is read, the unwritten one comes from the base — even if one is
        // written and the other is not.
        let theme = clean("search_current = \"#0a0b0c\"\n", &SENTINEL);
        assert_eq!(
            theme,
            Theme {
                search_current: 0x0a0b0c,
                ..SENTINEL
            }
        );
        let theme = clean("search_match = \"#0d0e0f\"\n", &SENTINEL);
        assert_eq!(
            (theme.search_match, theme.search_current),
            (0x0d0e0f, SENTINEL.search_current)
        );
        let inherited = clean("background = \"#ffffff\"\n", &Theme::BATERI);
        assert_eq!(
            (inherited.search_match, inherited.search_current),
            (Theme::BATERI.search_match, Theme::BATERI.search_current)
        );
    }

    #[test]
    fn unparseable_theme_is_a_separate_result() {
        let err =
            Theme::parse("background = \"#ffffff\n", &Theme::BATERI).expect_err("invalid TOML");
        assert_eq!(err.line, Some(1));
        assert!(err.message.starts_with("invalid TOML: "), "{err}");
    }

    #[test]
    fn documented_blocks_are_the_embedded_themes() {
        // `docs/SETTINGS.md` gives a full block for every embedded theme as
        // "copy, change". Since the document **copies** values it drifts; this
        // test ties the blocks to the embedded themes. The base is distinct: a
        // key dropped from the block would be filled from the base and break
        // equality, so the block has to stay complete. The list comes from the
        // table itself: an added embedded theme has to come with its block.
        let doc = include_str!("../../../docs/SETTINGS.md");
        let names: Vec<_> = Theme::embedded_names().collect();
        assert_eq!(names, ["bateri", "bateri-light", "linen"]);
        for name in names {
            let theme = Theme::embedded(name);
            // The heading is searched with its line ending: "`bateri`" is a
            // prefix of "`bateri-light`" and an unbounded search would read the
            // wrong block.
            let heading = format!("### Embedded `{name}`\n");
            let (_, after) = doc
                .split_once(&heading)
                .unwrap_or_else(|| panic!("{heading:?} not found in SETTINGS.md"));
            let (_, block) = after
                .split_once("```toml\n")
                .expect("no toml block under the heading");
            let (block, _) = block
                .split_once("```")
                .expect("the toml block does not close");
            assert_eq!(Some(clean(block, &SENTINEL)), theme, "{name}");
        }
    }
}
