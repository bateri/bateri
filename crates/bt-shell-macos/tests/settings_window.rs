//! The settings window's pages, laid out and measured (`bt_shell_macos::settings_layout_problems`):
//! a group's rows inside its box and one under another, a row's text beside its control, a
//! page's example above its groups. A layout that leaves a height open builds without a word and
//! draws everything over one another; this is the check that sees it without eyes.
//!
//! Its own `main` (`harness = false`): AppKit wants the main thread. Without a window server
//! session (an ssh login) it says so and passes. `BT_SETTINGS_SHOTS=<dir>` also writes each page
//! there as a PNG, for a person to look at.

use std::path::PathBuf;
use std::process::Command;

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

fn main() {
    let aqua = Command::new("launchctl")
        .arg("managername")
        .output()
        .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).trim() == "Aqua");
    if !aqua {
        println!("settings_window: SKIPPED (no window server session)");
        return;
    }
    let mtm = MainThreadMarker::new().expect("a harness-less test runs on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let shots = std::env::var_os("BT_SETTINGS_SHOTS").map(PathBuf::from);
    if let Some(dir) = &shots {
        std::fs::create_dir_all(dir).expect("the pictures' folder can be made");
    }
    // First: the window's palette is not set yet, as when the application opens the window.
    let titles = bt_shell_macos::settings_title_problems(mtm);
    for problem in &titles {
        eprintln!("settings_window: {problem}");
    }
    assert!(
        titles.is_empty(),
        "{} buttons titled in a colour that is not the theme's",
        titles.len()
    );
    let problems = bt_shell_macos::settings_layout_problems(mtm, shots.as_deref());
    for problem in &problems {
        eprintln!("settings_window: {problem}");
    }
    assert!(
        problems.is_empty(),
        "{} layout problems in the settings window",
        problems.len()
    );
    // The card a new server asks with, in each of its states and both appearances.
    let card = bt_shell_macos::apps_card_layout_problems(mtm, shots.as_deref());
    for problem in &card {
        eprintln!("settings_window: {problem}");
    }
    assert!(
        card.is_empty(),
        "{} layout problems in the card a server asks with",
        card.len()
    );
    println!("settings_window: ok");
}
