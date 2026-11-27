//! The text fields of a page, as AppKit lays them out in the rooms the kit gives them
//! (`bt_shell_macos::page_field_problems`): the line of the face the fields are set in, in the
//! page's points at a zoom (Monaco: 16 at the kit's size, 15.5 at zoom 2); a box of three rows holds
//! three lines of text, whole, and four are seen to be a cut; the capitals of a field of one row
//! stand in the middle of its box; a press in the room's padding is the field's.
//!
//! Its own `main`: AppKit wants the main thread. Without a window server session (an ssh login) it
//! says so and passes, as `hotkey_row` does.

use std::process::Command;

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

fn main() {
    let aqua = Command::new("launchctl")
        .arg("managername")
        .output()
        .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).trim() == "Aqua");
    if !aqua {
        println!("page_field: SKIPPED (no window server session)");
        return;
    }
    let mtm = MainThreadMarker::new().expect("a harness-less test runs on the main thread");
    NSApplication::sharedApplication(mtm)
        .setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let problems = bt_shell_macos::page_field_problems(mtm);
    for problem in &problems {
        eprintln!("page_field: {problem}");
    }
    assert!(
        problems.is_empty(),
        "{} problems in the page's fields",
        problems.len()
    );
    println!("page_field: ok");
}
