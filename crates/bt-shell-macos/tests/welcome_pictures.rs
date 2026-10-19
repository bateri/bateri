//! The welcome window draws every page, in a dark and a light theme, without a window on
//! screen: each page becomes a PNG that is not empty. `BT_WELCOME_SHOTS=dir` keeps them there
//! (for docs and reviews); otherwise they go to a temporary directory and are removed.
//!
//! Its own `main` (`harness = false`): AppKit wants the main thread. Without a window server
//! session (an ssh login) it says so and passes, as `embed`'s test does.

use std::process::Command;

use objc2::{AnyThread, MainThreadMarker};

fn main() {
    let aqua = Command::new("launchctl")
        .arg("managername")
        .output()
        .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).trim() == "Aqua");
    if !aqua {
        println!("welcome_pictures: SKIPPED, no Aqua session");
        return;
    }
    let Some(mtm) = MainThreadMarker::new() else {
        panic!("welcome_pictures runs on the main thread");
    };
    let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
    // A test binary has no bundle: give it bateri's icon, as the `.app` has.
    let icon = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/bundle/bateri.png"
    );
    if let Some(image) = objc2_app_kit::NSImage::initWithContentsOfFile(
        objc2_app_kit::NSImage::alloc(),
        &objc2_foundation::NSString::from_str(icon),
    ) {
        // SAFETY: a plain image for the application's icon.
        unsafe { app.setApplicationIconImage(Some(&image)) };
    }
    let keep = std::env::var_os("BT_WELCOME_SHOTS").map(std::path::PathBuf::from);
    let dir = keep.clone().unwrap_or_else(|| {
        std::env::temp_dir().join(format!("bateri-welcome-{}", std::process::id()))
    });
    let written = bt_shell_macos::welcome_pictures(mtm, &dir).expect("pictures");
    assert_eq!(
        written.len(),
        10,
        "four pages and the theme row, in two themes"
    );
    for path in &written {
        let size = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
        assert!(size > 1_000, "{} is empty", path.display());
    }
    // The keys reach the page through the window's own event path, whatever has the focus:
    // ⇥ ⇥ to the setup page, ← there changes a value and stays, ⇧⇥ / ⌘[ / ⌘← / ⌫ go back,
    // ⌘] and ⌘4 go on.
    const TAB: u16 = 48;
    let pages = bt_shell_macos::welcome_key_walk(
        mtm,
        &[
            (TAB, "\t", false, false),
            (TAB, "\t", false, false),
            (123, "\u{F702}", false, false),
            (TAB, "\u{19}", true, false),
            (TAB, "\t", false, false),
            (33, "[", false, true),
            (30, "]", false, true),
            (123, "\u{F702}", false, true),
            (TAB, "\t", false, false),
            (51, "\u{7f}", false, false),
            (21, "4", false, true),
            (124, "\u{F703}", false, true),
        ],
    );
    assert_eq!(
        pages,
        [2, 3, 3, 2, 3, 2, 3, 2, 3, 2, 4, 4],
        "the pages the keys led to"
    );
    if keep.is_none() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    println!("welcome_pictures: {} pages drawn", written.len());
}
