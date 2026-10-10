//! What the pointer's modifier keys say over a pane: ⌥⌘ together are the arrangement's — the
//! press is never the program's — and ⌘ without ⌥ is a link's key. One reading for every
//! surface that takes the pointer (the pane's view, its links, a carried tab), so the two keys
//! never mean two things.

use objc2_app_kit::NSEventModifierFlags;

/// Whether a mouse event carrying `flags` is the arrangement's, not the
/// program's: ⌘ and ⌥ both down (a third modifier does not hand it back — a
/// ⇧⌥⌘ press is nothing the program was ever told about).
pub(crate) fn swallows(flags: NSEventModifierFlags) -> bool {
    flags.contains(NSEventModifierFlags::Command) && flags.contains(NSEventModifierFlags::Option)
}

/// Whether ⌘ is down **as a link's key**: with ⌥ it is the arrangement's.
pub(crate) fn link_command(flags: NSEventModifierFlags) -> bool {
    flags.contains(NSEventModifierFlags::Command) && !flags.contains(NSEventModifierFlags::Option)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_press_with_option_and_command_is_the_arrangements() {
        let both = NSEventModifierFlags::Command | NSEventModifierFlags::Option;
        assert!(swallows(both));
        assert!(swallows(both | NSEventModifierFlags::Shift));
        assert!(!swallows(NSEventModifierFlags::Command));
        assert!(!swallows(NSEventModifierFlags::Option));
        // The link's key is ⌘ alone of the two.
        assert!(link_command(NSEventModifierFlags::Command));
        assert!(!link_command(both));
        assert!(!link_command(NSEventModifierFlags::empty()));
    }
}
