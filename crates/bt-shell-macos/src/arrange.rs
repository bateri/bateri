//! The shell's key moments and its small shared helpers: what the held modifiers mean
//! ([`hold_of`]: ⌘ shows the tabs' keys, ⌃⌘ the groups', after a wait), the count of a
//! modifier pressed twice ([`taps`]), a job run on the main queue after a delay ([`after`]),
//! and the drawing a carried pane's card shares ([`draw_grip`], [`label`]).
//!
//! **One watch for the holds.** `tab_bar::watch_command_key` is the only local monitor of
//! modifier changes; it classifies the flags once ([`hold_of`]) and `AppDelegate::hold_changed`
//! tells the key window's bar. ⌥⌘ held lifts nothing: a pane is carried by its header, a tab of
//! one pane by its row or chip, with no key held (`pane_drag`, `tab_merge`).

use std::time::Duration;

use bt_core::Mods;
use bt_shell_common::shortcuts::{Held, Taps};
use dispatch2::{DispatchQueue, DispatchTime};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSBezierPath, NSEvent, NSEventModifierFlags, NSFont, NSFontWeightMedium, NSLineBreakMode,
    NSTextField,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

use crate::app;
use crate::tab_bar::{HINT_DELAY, Tint};

// ─── The hold ────────────────────────────────────────────────────────────

/// Which modifiers are held, as far as the shell's two key moments care.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Hold {
    #[default]
    Nothing,
    /// Select Tab's modifiers and nothing else (⌘ until the file says otherwise): the tabs show
    /// their keys after a wait.
    Tabs,
    /// Select Group's modifiers and nothing else (⌃⌘): the column's group headers show their
    /// keys (⌃⌘1…⌃⌘9), after the same wait.
    Group,
}

/// The modifiers `flags` hold among the four a shortcut uses: Caps Lock and the keypad's and the
/// function key's bits are not among them, so ⌘ alone is still ⌘ alone with Caps Lock on.
pub(crate) fn held_mods(flags: NSEventModifierFlags) -> Mods {
    Mods::new(
        flags.contains(NSEventModifierFlags::Control),
        flags.contains(NSEventModifierFlags::Option),
        flags.contains(NSEventModifierFlags::Shift),
        flags.contains(NSEventModifierFlags::Command),
    )
}

/// The keys `flags` hold that a double press counts: the four modifiers and fn.
pub(crate) fn held_keys(flags: NSEventModifierFlags) -> Held {
    Held {
        mods: held_mods(flags),
        function: flags.contains(NSEventModifierFlags::Function),
    }
}

/// A count of a modifier pressed twice, with this shell's timing: a tap is shorter than the
/// hint's wait ([`HINT_DELAY`] — a press that long is a hold, which shows the hints), and the
/// second press comes within the system's double-click interval of the first one's release.
/// Every count of a double press — the live trigger and the recorder of Settings ▸ Keyboard —
/// is made here, so they cannot disagree on what a double press is.
pub(crate) fn taps() -> Taps {
    Taps::new(HINT_DELAY.as_secs_f64(), NSEvent::doubleClickInterval())
}

/// What `flags` hold, against the modifiers of Select Tab (`tabs`) and of Select Group
/// (`groups`) — the file moves them, `None` when it took them away: those modifiers alone are
/// the hold that shows the keys. Anything else — ⌥⌘ too, which lifts nothing — is no hold.
pub(crate) fn hold_of(
    flags: NSEventModifierFlags,
    tabs: Option<Mods>,
    groups: Option<Mods>,
) -> Hold {
    let held = held_mods(flags);
    if Some(held) == tabs {
        Hold::Tabs
    } else if Some(held) == groups {
        Hold::Group
    } else {
        Hold::Nothing
    }
}

/// Runs `job` on the main queue after `delay`, with the application: found
/// again by the queue, as it is not `Send`.
pub(crate) fn after(delay: Duration, job: impl FnOnce(&app::AppDelegate) + Send + 'static) {
    let Ok(when) = DispatchTime::try_from(delay) else {
        return;
    };
    let _ = DispatchQueue::main().after(when, move || {
        // audit: a block running on the main queue is on the main thread by definition.
        let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
        if let Some(app) = app::delegate(mtm) {
            job(&app);
        }
    });
}

// ─── What a carried card draws ───────────────────────────────────────────

/// Six dots in two columns: the hold.
pub(crate) fn draw_grip(left: f64, middle: f64, ink: Tint) {
    ink.color().setFill();
    for column in 0..2 {
        for row in 0..3 {
            let x = left + 4.5 + f64::from(column) * 6.0;
            let y = middle - 5.5 + f64::from(row) * 5.5;
            let dot = NSBezierPath::bezierPathWithOvalInRect(NSRect::new(
                NSPoint::new(x - 1.25, y - 1.25),
                NSSize::new(2.5, 2.5),
            ));
            dot.fill();
        }
    }
}

/// A label that never takes a press: the face under it decides.
pub(crate) fn label(
    mtm: MainThreadMarker,
    text: &str,
    size: f64,
    medium: bool,
    ink: Tint,
) -> Retained<NSTextField> {
    let field = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    let font = if medium {
        // SAFETY: AppKit's constant font weight.
        NSFont::systemFontOfSize_weight(size, unsafe { NSFontWeightMedium })
    } else {
        NSFont::systemFontOfSize(size)
    };
    field.setFont(Some(&font));
    field.setTextColor(Some(&ink.color()));
    field.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    field.setMaximumNumberOfLines(1);
    field
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Select Tab's and Select Group's modifiers as the table has them.
    const TABS: Option<Mods> = Some(Mods::CMD);
    const GROUPS: Option<Mods> = Some(Mods::new(true, false, false, true));

    fn hold(flags: NSEventModifierFlags) -> Hold {
        hold_of(flags, TABS, GROUPS)
    }

    #[test]
    fn the_holds_are_the_families_modifiers_alone() {
        let command = NSEventModifierFlags::Command;
        let option = NSEventModifierFlags::Option;
        assert_eq!(hold(command), Hold::Tabs);
        // ⌥⌘ lifts nothing: it is no hold, Caps Lock or not.
        assert_eq!(hold(command | option), Hold::Nothing);
        // Caps Lock, the keypad and the function key are not chords.
        assert_eq!(hold(command | NSEventModifierFlags::CapsLock), Hold::Tabs);
        assert_eq!(
            hold(command | option | NSEventModifierFlags::CapsLock),
            Hold::Nothing
        );
        assert_eq!(hold(command | NSEventModifierFlags::Function), Hold::Tabs);
        // A third modifier is a shortcut of its own.
        assert_eq!(
            hold(command | option | NSEventModifierFlags::Shift),
            Hold::Nothing
        );
        assert_eq!(
            hold(command | option | NSEventModifierFlags::Control),
            Hold::Nothing
        );
        assert_eq!(
            hold(command | NSEventModifierFlags::Control),
            Hold::Group,
            "⌃⌘ alone shows the groups' keys"
        );
        assert_eq!(
            hold(command | NSEventModifierFlags::Control | NSEventModifierFlags::Shift),
            Hold::Nothing
        );
        assert_eq!(hold(option), Hold::Nothing);
        assert_eq!(hold(NSEventModifierFlags::empty()), Hold::Nothing);
    }

    #[test]
    fn the_file_moves_the_hold_a_family_shows_its_keys_with() {
        let option = NSEventModifierFlags::Option;
        let command = NSEventModifierFlags::Command;
        let only_option = Some(Mods::NONE.with(bt_core::Modifier::Option));
        // `select_tab = "option"`: ⌥ alone shows the tabs' keys, ⌘ alone none.
        assert_eq!(hold_of(option, only_option, GROUPS), Hold::Tabs);
        assert_eq!(hold_of(command, only_option, GROUPS), Hold::Nothing);
        // `select_tab = "option+cmd"`: with nothing lifting on ⌥⌘ any more, the hold shows the
        // tabs' keys like any family's.
        let option_command = Some(Mods::new(false, true, false, true));
        assert_eq!(
            hold_of(option | command, option_command, GROUPS),
            Hold::Tabs
        );
        // `select_tab = ""`: no hold shows tab keys; `select_group` follows its own.
        assert_eq!(hold_of(command, None, GROUPS), Hold::Nothing);
        assert_eq!(
            hold_of(command | NSEventModifierFlags::Control, None, GROUPS),
            Hold::Group
        );
        assert_eq!(
            hold_of(command | NSEventModifierFlags::Control, TABS, None),
            Hold::Nothing
        );
    }

    /// The modifiers a shortcut uses, Caps Lock or not; the keypad's and the function key's bits
    /// are none of them.
    #[test]
    fn held_mods_ignore_caps_lock() {
        let command = NSEventModifierFlags::Command;
        assert_eq!(held_mods(command), Mods::CMD);
        assert_eq!(
            held_mods(command | NSEventModifierFlags::CapsLock),
            Mods::CMD
        );
        assert_eq!(
            held_mods(command | NSEventModifierFlags::Function),
            Mods::CMD
        );
        assert_ne!(held_mods(command | NSEventModifierFlags::Shift), Mods::CMD);
        assert_eq!(held_mods(NSEventModifierFlags::empty()), Mods::NONE);
    }

    /// A double press counts the four modifiers and fn — fn only to refuse it — and still not
    /// Caps Lock.
    #[test]
    fn held_keys_are_the_four_modifiers_and_fn() {
        let shift = NSEventModifierFlags::Shift;
        let held = held_keys(shift | NSEventModifierFlags::CapsLock);
        assert_eq!(held.mods, Mods::new(false, false, true, false));
        assert!(!held.function);
        let function = held_keys(NSEventModifierFlags::Function);
        assert_eq!(function.mods, Mods::NONE);
        assert!(function.function);
        assert_eq!(held_keys(NSEventModifierFlags::empty()), Held::default());
    }
}
