//! The options the Font page offers for each font that ships with the app, read from the fonts
//! themselves: which are rows, which are tiles, what the tiles show and what a font calls them.
//!
//! A binary of its own on purpose: registering the app's fonts is process-wide, and the lib
//! tests must never see these families.
#![cfg(target_os = "macos")]

use bt_core::FontOptions;
use bt_shell_common::font_options::{FontOption, lacking, options, register_app_fonts};

fn of(family: &str) -> Vec<FontOption> {
    register_app_fonts();
    options(Some(family))
}

fn find<'a>(options: &'a [FontOption], id: &str) -> &'a FontOption {
    options
        .iter()
        .find(|option| option.id() == id)
        .unwrap_or_else(|| panic!("no option {id}"))
}

#[test]
fn jetbrains_mono_changes_w_with_cv07_and_has_ligatures_as_a_row() {
    let options = of("JetBrains Mono");
    let cv07 = find(&options, "cv07");
    assert!(!cv07.is_row());
    assert!(cv07.chars().contains(&'W') && cv07.chars().contains(&'w'));
    assert_eq!(cv07.letters().as_deref(), Some("Ww"));
    assert!(find(&options, "ligatures").is_row());
    assert!(find(&options, "zero").is_row());
    // Named sets keep the name the font gives.
    assert_eq!(find(&options, "ss01").title(), "Classic construction");
}

#[test]
fn obel_mono_masks_secrets_in_a_row_and_not_in_the_tiles() {
    let options = of("Obel Mono");
    let secrets = find(&options, "secrets");
    assert_eq!(secrets.title(), "Hide Secrets");
    assert!(secrets.is_row());
    assert!(
        options
            .iter()
            .filter(|option| !option.is_row())
            .all(|option| option.id() != "ss01"),
        "the mask is a tile too"
    );
    assert_eq!(find(&options, "cv02").title(), "Single-story a");
}

#[test]
fn fira_codes_unnamed_options_are_labelled_by_their_tag() {
    let options = of("Fira Code");
    let ss01 = find(&options, "ss01");
    assert_eq!(ss01.title(), "ss01");
    assert!(ss01.chars().contains(&'r'));
    assert!(!ss01.is_row());
}

#[test]
fn maple_mono_and_monaspace_names_lose_their_tag_prefix() {
    for family in ["Maple Mono", "Monaspace Neon"] {
        let options = of(family);
        let named: Vec<&FontOption> = options
            .iter()
            .filter(|option| !option.is_row() && option.id().len() == 4)
            .filter(|option| option.title() != option.id())
            .collect();
        assert!(!named.is_empty(), "{family} names none of its sets");
        for option in named {
            let title = option.title().to_uppercase();
            assert!(
                !title.starts_with("SS") && !title.starts_with("CV")
                    || !title[2..].starts_with(|c: char| c.is_ascii_digit()),
                "{family}: {:?} still carries its tag",
                option.title()
            );
        }
    }
    assert_eq!(find(&of("Monaspace Neon"), "ss01").title(), "Equal Symbols");
}

#[test]
fn geist_mono_has_no_ligatures_row_and_a_tile_for_its_coding_ligatures() {
    let options = of("Geist Mono");
    assert!(options.iter().all(|option| option.id() != "ligatures"));
    let tile = options
        .iter()
        .find(|option| option.title() == "Coding ligatures")
        .expect("the set named Coding ligatures");
    assert!(!tile.is_row());
}

#[test]
fn a_tag_the_font_does_not_list_is_what_the_file_asked_for_in_vain() {
    register_app_fonts();
    let font = FontOptions {
        family: Some("Obel Mono".to_owned()),
        features: [(
            "obel mono".to_owned(),
            [("ss01".to_owned(), true), ("ss19".to_owned(), true)].into(),
        )]
        .into(),
        ..FontOptions::default()
    };
    assert_eq!(lacking(&font), ["ss19"]);
    assert!(lacking(&FontOptions::default()).is_empty());
}
