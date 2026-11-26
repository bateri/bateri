//! The hotkey window's panel, opened through the shell's own setup and read back
//! (`bt_shell_macos::hotkey_panel_problems`): an `NSPanel` that does not take the application
//! from the one the person is in, that does not hide when bateri goes behind, that joins a
//! full-screen Space and stays out of the window cycle, whose frame is the rules' own even beyond
//! the screen's edge, and that is out of sight, and says so, once it is hidden.
//!
//! Its own `main` (`harness = false`): AppKit wants the main thread. Without a window server
//! session (an ssh login) it says so and passes. **No global key is registered and no event monitor
//! is made** (the window has none of its own): the check only opens a panel for the moment it
//! takes.

use std::process::Command;

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

fn main() {
    let aqua = Command::new("launchctl")
        .arg("managername")
        .output()
        .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).trim() == "Aqua");
    if !aqua {
        println!("hotkey_panel: SKIPPED (no window server session)");
        return;
    }
    let mtm = MainThreadMarker::new().expect("a harness-less test runs on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let problems = bt_shell_macos::hotkey_panel_problems(mtm);
    assert!(
        problems.is_empty(),
        "the hotkey panel has problems:\n  {}",
        problems.join("\n  ")
    );
    println!("hotkey_panel: ok");
}
