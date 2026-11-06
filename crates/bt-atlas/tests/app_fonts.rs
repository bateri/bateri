//! The fonts that ship with the app: registered from `assets/fonts` the way
//! the launch does, they are in the Font list under their exact names and each
//! gives the four faces the atlas asks for.
//!
//! A binary of its own on purpose: the registration is process-wide, and the
//! lib tests and the raster digest must never see these families, nor the
//! flag that says the app's fonts are there.
#![cfg(target_os = "macos")]

use std::path::PathBuf;

use bt_atlas::{
    Atlas, Face, Half, SizeClass, Spacing, Sprite, app_fonts_registered, monospaced_families,
    register_app_fonts,
};

/// The eight families of the Font list, as the list spells them, and whether
/// the family has an italic of its own (Fira Code has none).
const FAMILIES: [(&str, bool); 8] = [
    ("JetBrains Mono", true),
    ("Fira Code", false),
    ("Cascadia Code", true),
    ("Monaspace Neon", true),
    ("Geist Mono", true),
    ("Maple Mono", true),
    ("IBM Plex Mono", true),
    ("Obel Mono", true),
];

const SYMBOLS: &str = "Symbols Nerd Font Mono";

fn fonts_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts")
}

#[test]
fn the_eight_families_are_in_the_font_list_and_the_symbol_font_is_not() {
    let outcome = register_app_fonts(&fonts_dir());
    assert!(
        outcome.refused.is_empty(),
        "the font system refused {:?}",
        outcome.refused
    );
    // Every family was either registered or already on this Mac under its
    // name: nine in all, the eight and the symbol font.
    let mut seen: Vec<&str> = outcome
        .registered
        .iter()
        .chain(&outcome.kept)
        .map(String::as_str)
        .collect();
    seen.sort_unstable();
    let mut expected: Vec<&str> = FAMILIES.iter().map(|(name, _)| *name).collect();
    expected.push(SYMBOLS);
    expected.sort_unstable();
    assert_eq!(seen, expected);

    let list = monospaced_families();
    for (name, _) in FAMILIES {
        assert!(
            list.iter().any(|listed| listed == name),
            "{name} is not in the Font list: {list:?}"
        );
    }
    assert!(
        !list
            .iter()
            .any(|listed| listed.eq_ignore_ascii_case(SYMBOLS)),
        "the symbol font is in the Font list"
    );
    assert!(app_fonts_registered());
}

#[test]
fn each_family_gives_plain_bold_italic_and_bold_italic_faces() {
    register_app_fonts(&fonts_dir());
    for (name, has_italic) in FAMILIES {
        let mut atlas = Atlas::new(
            Some(name),
            13.0,
            2.0,
            Spacing {
                line: 1.0,
                letter: 1.0,
            },
        );
        assert!(
            atlas.font_issue().is_none(),
            "{name} opened with {:?}",
            atlas.font_issue()
        );
        // A face the family cannot give collapses onto a lower one and shares
        // its slot, so the slot numbers say which faces are the family's own.
        let mut slot = |face| {
            atlas
                .slot(Sprite::Char('e'), face, SizeClass::Normal, Half::Whole)
                .0
                .slot
        };
        let regular = slot(Face::Regular);
        let bold = slot(Face::Bold);
        let italic = slot(Face::Italic);
        let bold_italic = slot(Face::BoldItalic);
        assert_ne!(regular, 0, "{name}: no glyph for the plain face");
        assert_ne!(bold, regular, "{name}: no bold face of its own");
        if has_italic {
            assert_ne!(italic, regular, "{name}: no italic face of its own");
            assert_ne!(bold_italic, bold, "{name}: no bold italic face of its own");
            assert_ne!(bold_italic, italic, "{name}: bold italic is the italic");
        } else {
            // The family has no italic design: italic text is its own plain
            // face and bold italic its own bold face, never a system font.
            assert_eq!(italic, regular, "{name}: italic is not the plain face");
            assert_eq!(
                bold_italic, bold,
                "{name}: bold italic is not the bold face"
            );
        }
    }
}
