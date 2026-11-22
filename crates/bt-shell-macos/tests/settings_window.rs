//! The settings window's pages, laid out and measured (`bt_shell_macos::settings_layout_problems`):
//! a group's rows inside its box and one under another, a row's text beside its control, a
//! page's example above its groups. A layout that leaves a height open builds without a word and
//! draws everything over one another; this is the check that sees it without eyes.
//!
//! Its own `main` (`harness = false`): AppKit wants the main thread. Without a window server
//! session (an ssh login) it says so and passes. While the screen is locked the window server
//! draws no window, so the check that reads the buttons' titles from the screen says so and is
//! passed over; the rest draws into bitmaps and runs. `BT_SETTINGS_SHOTS=<dir>` also writes each
//! page there as a PNG, for a person to look at; with the scroll bars shown always, in
//! `<dir>/scroll-bars-always`.
//!
//! The checks run twice, once with each kind of scroll bar: the overlay that shows while a page
//! scrolls, and the one that always takes its lane beside the rows, which System Settings' "Show
//! scroll bars" gives when set to always, or to automatic with a mouse plugged in. A check that
//! ran only as the machine is set would pass or fail with the mouse. Each kind is a run of this
//! program of its own, with the setting in AppKit's argument domain (`-AppleShowScrollBars`):
//! AppKit reads it once, and the person's own setting is left as it is.

use std::path::PathBuf;
use std::process::Command;

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

/// The kind of scroll bar a run of the checks is given, set by the run that starts it.
const SCROLL_BARS: &str = "BT_SETTINGS_SCROLL_BARS";

/// `AppleShowScrollBars`' values, each with the folder its pictures go in under the shots' one.
const KINDS: [(&str, Option<&str>); 2] = [
    ("WhenScrolling", None),
    ("Always", Some("scroll-bars-always")),
];

fn main() {
    let aqua = Command::new("launchctl")
        .arg("managername")
        .output()
        .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).trim() == "Aqua");
    if !aqua {
        println!("settings_window: SKIPPED (no window server session)");
        return;
    }
    if let Ok(kind) = std::env::var(SCROLL_BARS) {
        check(&format!("settings_window (scroll bars {kind})"));
        return;
    }
    let me = std::env::current_exe().expect("the check knows its own program");
    let shots = std::env::var_os("BT_SETTINGS_SHOTS").map(PathBuf::from);
    for (kind, folder) in KINDS {
        let mut run = Command::new(&me);
        run.args(["-AppleShowScrollBars", kind])
            .env(SCROLL_BARS, kind);
        if let (Some(dir), Some(folder)) = (&shots, folder) {
            run.env("BT_SETTINGS_SHOTS", dir.join(folder));
        }
        let status = run.status().expect("the check starts again");
        assert!(
            status.success(),
            "the settings window's checks failed with the scroll bars {kind}"
        );
    }
    println!("settings_window: ok");
}

/// Every check of the window, its problems written under `who`.
fn check(who: &str) {
    let mtm = MainThreadMarker::new().expect("a harness-less test runs on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let shots = std::env::var_os("BT_SETTINGS_SHOTS").map(PathBuf::from);
    if let Some(dir) = &shots {
        std::fs::create_dir_all(dir).expect("the pictures' folder can be made");
    }
    // First: the window's palette is not set yet, as when the application opens the window.
    match bt_shell_macos::settings_title_problems(mtm) {
        None => println!(
            "{who}: button titles SKIPPED (the screen shows no window, as while it is locked)"
        ),
        Some(titles) => {
            for problem in &titles {
                eprintln!("{who}: {problem}");
            }
            assert!(
                titles.is_empty(),
                "{} buttons titled in a colour that is not the theme's",
                titles.len()
            );
        }
    }
    let problems = bt_shell_macos::settings_layout_problems(mtm, shots.as_deref());
    for problem in &problems {
        eprintln!("{who}: {problem}");
    }
    assert!(
        problems.is_empty(),
        "{} layout problems in the settings window",
        problems.len()
    );
    // The card a new server asks with, in each of its states and both appearances.
    let card = bt_shell_macos::apps_card_layout_problems(mtm, shots.as_deref());
    for problem in &card {
        eprintln!("{who}: {problem}");
    }
    assert!(
        card.is_empty(),
        "{} layout problems in the card a server asks with",
        card.len()
    );
    println!("{who}: ok");
}
