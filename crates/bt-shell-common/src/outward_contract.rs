//! The outward contract: what programs outside bateri read of it, held here as text.
//!
//! Another application — a status strip of AI sessions (evlat) — finds the pane its agent
//! runs in from the shell's environment, opens `bateri://tab/<UUID>` to bring that pane to the
//! front and asks `bateri focus` whether the user is looking at it. It does so across bateri's
//! versions and is never rebuilt with it, so a rename here breaks it silently: no build, no
//! clippy and no other test of ours would turn red. Each value below is written out literally,
//! not taken from the constant it guards — the test must change when the contract does.
//!
//! Inside bateri the names are free: the identity's type, its fields and the modules that
//! carry it may be called anything. What stays is the text: the URL's `tab/` path naming a
//! **pane**, the uppercase UUID and its value, the three variables, the `focus` subcommand with
//! its arguments, answer lines and exit codes, and the bundle identifier the other application
//! finds bateri by. A new answer token is added beside these lines, never instead of them; a
//! tab given an identity of its own gets an address of its own.

use std::path::PathBuf;

use bt_core::{PaneUuid, pane_env};

use crate::focus::{self, Answer};

const LOWER: &str = "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0";
const UPPER: &str = "0F1E2D3C-4B5A-6978-8796-A5B4C3D2E1F0";

fn pane() -> PaneUuid {
    PaneUuid::parse(LOWER).expect("a canonical UUID in any letter case is an identity")
}

#[test]
fn a_pane_is_named_by_its_uppercase_uuid_under_bateri_tab() {
    let id = pane();
    assert_eq!(id.as_str(), UPPER);
    assert_eq!(id.url(), format!("bateri://tab/{UPPER}"));
    assert_eq!(
        PaneUuid::from_url(&format!("bateri://tab/{UPPER}")),
        Some(id.clone())
    );
    assert_eq!(
        PaneUuid::from_url(&format!("bateri://tab/{LOWER}")),
        Some(id)
    );
}

#[test]
fn the_shell_is_told_its_pane_in_these_three_variables() {
    let url = format!("bateri://tab/{UPPER}");
    assert_eq!(
        pane_env(&pane()),
        [
            ("TERM_SESSION_ID", UPPER.to_owned()),
            ("BATERI_TAB_URL", url.clone()),
            ("LC_BATERI_TAB_URL", url),
        ]
    );
}

#[test]
fn bateri_focus_keeps_its_word_its_lines_and_its_codes() {
    assert_eq!(focus::SUBCOMMAND, "focus");
    assert_eq!(
        Answer::Live {
            focused: true,
            idle_secs: 42
        }
        .token_line(),
        "pane=live focused=1 idle=42"
    );
    assert_eq!(
        Answer::Live {
            focused: false,
            idle_secs: 0
        }
        .token_line(),
        "pane=live focused=0 idle=0"
    );
    assert_eq!(Answer::None.token_line(), "pane=none");
    assert_eq!(Answer::Unknown.token_line(), "pane=unknown");
    assert_eq!(
        (focus::EXIT_ANSWER, focus::EXIT_USAGE, focus::EXIT_UNKNOWN),
        (0, 2, 3)
    );

    // Both argument forms are read; with no live instance the answer is `none`, on one line.
    let url = format!("bateri://tab/{UPPER}");
    let no_instance: [PathBuf; 0] = [];
    for args in [
        vec![url.clone()],
        vec!["--pid".to_owned(), "123".to_owned(), url.clone()],
    ] {
        let mut out = Vec::new();
        assert_eq!(
            focus::focus_main(&args, &no_instance, &mut out),
            0,
            "{args:?}"
        );
        assert_eq!(out, b"pane=none\n", "{args:?}");
    }
    let mut out = Vec::new();
    assert_eq!(
        focus::focus_main(&["--pid".to_owned(), url], &no_instance, &mut out),
        2
    );
    assert!(out.is_empty(), "a usage error writes nothing to stdout");
}

#[test]
fn the_bundle_identifier_is_dev_bateri_bateri() {
    let plist = include_str!("../../../assets/bundle/Info.plist.in");
    let after = plist
        .split_once("<key>CFBundleIdentifier</key>")
        .map(|(_, rest)| rest.trim_start())
        .expect("the template names a bundle identifier");
    assert!(
        after.starts_with("<string>dev.bateri.bateri</string>"),
        "the bundle identifier is what other applications find bateri by"
    );
}
