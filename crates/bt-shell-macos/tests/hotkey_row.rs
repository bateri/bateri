//! The Hotkey Window page's shortcut row in each state a person finds it in — no shortcut, a
//! chord, a double tap, a double tap that waits for the permission — laid out and measured
//! (`bt_shell_macos::hotkey_row_problems`): the field and Remove are the same size at the same
//! place in every state, the warning is read whole and the row grows only while the key waits.
//! (`tests/settings_window.rs` runs the same measurement among the page checks.)
//!
//! With `--ignored` the page and the window are also drawn off the screen, as bitmaps and with no
//! window driven, into `target/shots/` as `hotkey-window-{none,chord,twice-waiting}.png` (the
//! page) and `hotkey-window-{…}-window.png` (the window), for a person to look at:
//! `cargo test -p bt-shell-macos --test hotkey_row -- --ignored`. A test with its own `main`
//! (`harness = false`) has no `#[ignore]`: the argument is read here.
//!
//! Its own `main`: AppKit wants the main thread. Without a window server session (an ssh login) it
//! says so and passes.

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
        println!("hotkey_row: SKIPPED (no window server session)");
        return;
    }
    let drawn = std::env::args().any(|arg| arg == "--ignored" || arg == "--include-ignored");
    let shots = drawn.then(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/shots"));
    if let Some(dir) = &shots {
        std::fs::create_dir_all(dir).expect("the pictures' folder can be made");
    }
    let mtm = MainThreadMarker::new().expect("a harness-less test runs on the main thread");
    NSApplication::sharedApplication(mtm)
        .setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let problems = bt_shell_macos::hotkey_row_problems(mtm, shots.as_deref());
    for problem in &problems {
        eprintln!("hotkey_row: {problem}");
    }
    assert!(
        problems.is_empty(),
        "{} problems in the shortcut row",
        problems.len()
    );
    match &shots {
        Some(dir) => {
            let dir = dir.canonicalize().unwrap_or_else(|_| dir.clone());
            println!("hotkey_row: ok, pictures in {}", dir.display());
        }
        None => println!("hotkey_row: ok (pictures: run with -- --ignored)"),
    }
}
