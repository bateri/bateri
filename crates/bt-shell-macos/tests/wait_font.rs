//! The font the line of a pane's wait is set in is the terminal's own, found by AppKit by its
//! name, and has one width for every character. The fonts the app ships are registered for the
//! process by `bt_gpu::register_app_fonts` (the atlas does it at launch: AppKit asks the same
//! process-wide registration), so a family the app ships can silently fail to open by name for
//! AppKit while the atlas has it, and then the line would fall back to the system's font. This
//! checks that it does not.
//!
//! Its own process: the registration is once per process.

use std::path::Path;

use bt_shell_macos::embed::wait_line_font;

/// The app's fonts, registered as the application does, from the directory it ships them from.
fn register() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts");
    let outcome = bt_gpu::register_app_fonts(&dir);
    assert!(
        bt_gpu::app_default_family().is_some(),
        "the app's fonts did not register: {outcome:?}"
    );
}

/// With no family written the line is in JetBrains Mono, which AppKit opens by name here, and
/// that is a font of one width.
#[test]
fn the_default_font_of_the_line_is_the_one_the_app_ships_and_has_one_width() {
    register();
    assert_eq!(bt_gpu::app_default_family(), Some("JetBrains Mono"));
    let (family, fixed, cell) = wait_line_font(None, 13.0);
    assert_eq!(
        family, "JetBrains Mono",
        "AppKit did not find the default family"
    );
    assert!(fixed, "JetBrains Mono is not of one width to AppKit");
    // A cell of JetBrains Mono is 0.6 of its size: the bars' 7.8 points are one cell at 13.
    assert!((cell - 7.8).abs() < 0.05, "a cell is {cell} points");
}

/// A family the person writes is the one the line is in, if it exists; one that does not exist
/// is the default, as the terminal's is.
#[test]
fn the_family_the_person_writes_is_the_lines_and_one_that_does_not_exist_is_not() {
    register();
    let (family, fixed, _) = wait_line_font(Some("Menlo"), 13.0);
    assert_eq!(family, "Menlo");
    assert!(fixed);
    let (family, fixed, _) = wait_line_font(Some("a family nobody has"), 13.0);
    assert_eq!(family, "JetBrains Mono");
    assert!(fixed);
}
