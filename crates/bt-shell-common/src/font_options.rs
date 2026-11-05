//! The font options the settings window and the palette offer for the selected font.
//!
//! The options are **the font's own**: they come from the feature list its file carries
//! (`bt_gpu::font_features`), so a font without any (Menlo) offers nothing and one with a
//! mask for secrets offers Hide Secrets. Two of them have a name of their own, because people
//! look for them by what they do and not by the tag: **Ligatures** (`calt` and `liga`, the
//! rules a code font ships on) and **Hide Secrets** (the stylistic set whose name is "Mask
//! secrets"). The rest are the font's stylistic sets and character variants under the names the
//! font gives them (an unnamed one under its tag) and `zero`. Every other tag — `frac`, `sups`,
//! `salt`, `ccmp`, … — stays out of the list and can still be written by hand.
//!
//! The window and the palette ask here and nowhere else, so a row says the same in both: what
//! the option is called, whether it is on in the file, and the edits that turn it on or off.
//! A choice is written under the family the file selects, because `ss01` does another thing in
//! every font; turning an option **off** removes its line (the font's own default again) and
//! never writes `false` for a tag the font does not ship on — except Ligatures, whose default
//! is on and whose off is the two lines `calt = false` and `liga = false`.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use bt_core::{FontOptions, SettingsEdit};
use bt_gpu::FontFeature;

/// The id of the Ligatures option.
pub const LIGATURES: &str = "ligatures";
/// The id of the Hide Secrets option.
pub const SECRETS: &str = "secrets";

/// The tags Ligatures switches together.
const LIGATURE_TAGS: [&str; 2] = ["calt", "liga"];

/// The name a font gives the stylistic set that masks secrets (Obel Mono's `ss01`).
const SECRETS_NAME: &str = "Mask secrets";

/// What an option does to its tags.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// On unless a tag is written `false`; off writes every tag `false`, on removes them.
    Ligatures,
    /// One tag, on when written `true`; off removes the line.
    Switch,
}

/// One option of the selected font.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontOption {
    id: String,
    title: String,
    tags: Vec<String>,
    kind: Kind,
}

impl FontOption {
    /// The option's id in a palette row (`set.font.features.<id>`): `ligatures`, `secrets` or
    /// the font's tag.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// What the row says: "Ligatures", "Hide Secrets", the font's name for a set or the tag.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The font tags the option writes, which a search can also match.
    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    /// Whether the option is on in `font`'s choices (the family in effect's table).
    pub fn is_on(&self, font: &FontOptions) -> bool {
        match self.kind {
            Kind::Ligatures => self.tags.iter().all(|tag| font.feature(tag) != Some(false)),
            Kind::Switch => self.tags.iter().all(|tag| font.feature(tag) == Some(true)),
        }
    }

    /// The edits that make the option `on` for `font`'s family, one line each.
    pub fn edits(&self, font: &FontOptions, on: bool) -> Vec<SettingsEdit> {
        let family = font.family.as_deref().unwrap_or_default().trim().to_owned();
        self.tags
            .iter()
            .map(|tag| SettingsEdit::FontFeature {
                family: family.clone(),
                tag: tag.clone(),
                on: match (self.kind, on) {
                    (Kind::Ligatures, false) => Some(false),
                    (Kind::Switch, true) => Some(true),
                    _ => None,
                },
            })
            .collect()
    }
}

/// The options a font with `features` offers, in the order the window lists them: Ligatures,
/// Hide Secrets, then the font's own sets and variants by tag, then Slashed Zero.
pub fn options_of(features: &[FontFeature]) -> Vec<FontOption> {
    let has = |tag: &str| features.iter().any(|feature| feature.tag == tag);
    let mut options = Vec::new();
    let ligatures: Vec<String> = LIGATURE_TAGS
        .iter()
        .filter(|tag| has(tag))
        .map(|tag| (*tag).to_owned())
        .collect();
    if !ligatures.is_empty() {
        options.push(FontOption {
            id: LIGATURES.to_owned(),
            title: "Ligatures".to_owned(),
            tags: ligatures,
            kind: Kind::Ligatures,
        });
    }
    let secrets = features.iter().find(|feature| {
        is_set(&feature.tag)
            && feature
                .name
                .as_deref()
                .is_some_and(|name| name.eq_ignore_ascii_case(SECRETS_NAME))
    });
    if let Some(feature) = secrets {
        options.push(FontOption {
            id: SECRETS.to_owned(),
            title: "Hide Secrets".to_owned(),
            tags: vec![feature.tag.clone()],
            kind: Kind::Switch,
        });
    }
    for feature in features {
        let shown = is_set(&feature.tag) || feature.tag == "zero";
        if !shown || secrets.is_some_and(|secret| secret.tag == feature.tag) {
            continue;
        }
        let title = match (&feature.name, feature.tag.as_str()) {
            (Some(name), _) => name.clone(),
            (None, "zero") => "Slashed Zero".to_owned(),
            (None, tag) => tag.to_owned(),
        };
        options.push(FontOption {
            id: feature.tag.clone(),
            title,
            tags: vec![feature.tag.clone()],
            kind: Kind::Switch,
        });
    }
    options
}

/// A stylistic set (`ss01`–`ss20`) or a character variant (`cv01`–`cv99`).
fn is_set(tag: &str) -> bool {
    let number = |digits: &str| {
        digits.len() == 2 && digits.bytes().all(|b| b.is_ascii_digit()) && digits != "00"
    };
    match (tag.get(..2), tag.get(2..)) {
        (Some("ss"), Some(digits)) => number(digits) && digits <= "20",
        (Some("cv"), Some(digits)) => number(digits),
        _ => false,
    }
}

/// The options of `family` (`None` and an empty name are the default chain's font). The font's
/// feature list is read once per family and kept for the life of the process: reading it opens
/// the font, and the palette asks on every keystroke.
pub fn options(family: Option<&str>) -> Vec<FontOption> {
    static LISTS: OnceLock<Mutex<HashMap<String, Vec<FontOption>>>> = OnceLock::new();
    let family = family.map(str::trim).filter(|name| !name.is_empty());
    let key = family.unwrap_or_default().to_lowercase();
    let mut lists = LISTS
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    lists
        .entry(key)
        .or_insert_with(|| options_of(&bt_gpu::font_features(family)))
        .clone()
}

/// The monospaced families of this machine, the list the settings window and the palette's Font
/// page both show. Taken **once** per process and shared by the two: the answer opens every
/// installed family and reads its tables, which takes a fraction of a second at the first call
/// and must not be paid by whichever window asks first ([`warm_families`]).
pub fn families() -> Vec<String> {
    static FAMILIES: OnceLock<Vec<String>> = OnceLock::new();
    FAMILIES.get_or_init(bt_gpu::monospaced_families).clone()
}

/// Takes [`families`] on a thread of its own, so that the list is there by the time the first
/// window that shows it opens; a window that asks before it is done waits for the rest, not for
/// a second pass.
pub fn warm_families() {
    // A thread that cannot be born costs the first window the wait it would have had anyway.
    let _ = std::thread::Builder::new()
        .name("font-families".to_owned())
        .spawn(|| {
            let _ = families();
        });
}

/// The option `id` of `font`'s family; `None` when the font does not offer it.
pub fn find(font: &FontOptions, id: &str) -> Option<FontOption> {
    options(font.family.as_deref())
        .into_iter()
        .find(|option| option.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feature(tag: &str, name: Option<&str>) -> FontFeature {
        FontFeature {
            tag: tag.to_owned(),
            name: name.map(str::to_owned),
        }
    }

    /// What the two fonts of the tests list, by tag as the reader sorts them.
    fn obel() -> Vec<FontFeature> {
        vec![
            feature("ccmp", None),
            feature("cv02", Some("Single-story a")),
            feature("cv03", None),
            feature("frac", None),
            feature("ss01", Some("Mask secrets")),
            feature("ss02", Some("Alternate zero")),
            feature("zero", None),
        ]
    }

    fn jetbrains() -> Vec<FontFeature> {
        vec![
            feature("calt", None),
            feature("liga", None),
            feature("ss19", Some("Broken equals ligatures")),
            feature("zero", None),
        ]
    }

    fn ids(options: &[FontOption]) -> Vec<&str> {
        options.iter().map(FontOption::id).collect()
    }

    fn titles(options: &[FontOption]) -> Vec<&str> {
        options.iter().map(FontOption::title).collect()
    }

    #[test]
    fn a_font_offers_what_it_lists_under_the_names_it_gives() {
        let obel = options_of(&obel());
        assert_eq!(ids(&obel), ["secrets", "cv02", "cv03", "ss02", "zero"]);
        assert_eq!(
            titles(&obel),
            [
                "Hide Secrets",
                "Single-story a",
                "cv03",
                "Alternate zero",
                "Slashed Zero"
            ]
        );
        let jetbrains = options_of(&jetbrains());
        assert_eq!(ids(&jetbrains), ["ligatures", "ss19", "zero"]);
        assert_eq!(jetbrains[0].tags(), ["calt", "liga"]);
        // `frac` and `ccmp` are the font's and are not offered; a font without any
        // list (Menlo) offers nothing.
        assert!(options_of(&[]).is_empty());
        assert!(options_of(&[feature("frac", None), feature("ccmp", None)]).is_empty());
    }

    #[test]
    fn ligatures_offers_only_the_tags_the_font_lists() {
        let calt_only = options_of(&[feature("calt", None)]);
        assert_eq!(calt_only[0].tags(), ["calt"]);
        let liga_only = options_of(&[feature("liga", None)]);
        assert_eq!(liga_only[0].tags(), ["liga"]);
    }

    #[test]
    fn a_set_is_ss01_to_ss20_and_cv01_to_cv99() {
        for tag in ["ss01", "ss20", "cv01", "cv99"] {
            assert!(is_set(tag), "{tag}");
        }
        for tag in [
            "ss00", "ss21", "cv00", "cv1", "ss1x", "zero", "salt", "cv100", "",
        ] {
            assert!(!is_set(tag), "{tag}");
        }
    }

    fn font_with(family: Option<&str>, choices: &[(&str, bool)]) -> FontOptions {
        FontOptions {
            family: family.map(str::to_owned),
            features: [(
                family.unwrap_or_default().to_lowercase(),
                choices
                    .iter()
                    .map(|&(tag, on)| (tag.to_owned(), on))
                    .collect(),
            )]
            .into(),
            ..FontOptions::default()
        }
    }

    #[test]
    fn hide_secrets_is_off_until_the_file_says_true_and_off_again_removes_the_line() {
        let secrets = options_of(&obel()).remove(0);
        assert!(!secrets.is_on(&font_with(Some("Obel Mono"), &[])));
        assert!(!secrets.is_on(&font_with(Some("Obel Mono"), &[("ss01", false)])));
        let on = font_with(Some("Obel Mono"), &[("ss01", true)]);
        assert!(secrets.is_on(&on));
        let write = |on| SettingsEdit::FontFeature {
            family: "Obel Mono".to_owned(),
            tag: "ss01".to_owned(),
            on,
        };
        assert_eq!(secrets.edits(&on, true), [write(Some(true))]);
        assert_eq!(secrets.edits(&on, false), [write(None)]);
    }

    #[test]
    fn ligatures_are_on_without_a_line_and_off_writes_both_tags() {
        let ligatures = options_of(&jetbrains()).remove(0);
        let default = font_with(Some("JetBrains Mono"), &[]);
        assert!(ligatures.is_on(&default));
        assert!(ligatures.is_on(&font_with(Some("JetBrains Mono"), &[("calt", true)])));
        // Either tag written off is off: a font that ligates through `liga` alone is
        // not shown as on.
        assert!(!ligatures.is_on(&font_with(Some("JetBrains Mono"), &[("liga", false)])));
        let tag = |tag: &str, on| SettingsEdit::FontFeature {
            family: "JetBrains Mono".to_owned(),
            tag: tag.to_owned(),
            on,
        };
        assert_eq!(
            ligatures.edits(&default, false),
            [tag("calt", Some(false)), tag("liga", Some(false))]
        );
        assert_eq!(
            ligatures.edits(&default, true),
            [tag("calt", None), tag("liga", None)]
        );
    }

    #[test]
    fn a_choice_belongs_to_the_family_in_effect() {
        // Another family's table says nothing about this one.
        let font = FontOptions {
            family: Some("Obel Mono".to_owned()),
            features: [("other".to_owned(), [("ss01".to_owned(), true)].into())].into(),
            ..FontOptions::default()
        };
        assert!(!options_of(&obel()).remove(0).is_on(&font));
        // The default chain's table is the empty name; the edit names no family.
        let default = font_with(None, &[("zero", true)]);
        let zero = options_of(&obel()).pop().expect("zero");
        assert!(zero.is_on(&default));
        assert_eq!(
            zero.edits(&default, false),
            [SettingsEdit::FontFeature {
                family: String::new(),
                tag: "zero".to_owned(),
                on: None
            }]
        );
    }

    #[test]
    fn a_default_chain_without_a_name_and_one_named_blank_are_the_same_list() {
        assert_eq!(options(None), options(Some("  ")));
        assert_eq!(options(Some("No Such Family 1234")), Vec::new());
    }
}
