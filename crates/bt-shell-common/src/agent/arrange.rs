//! Where the keyboard may go: the rule `focus`, `zoom`, `swap`, `move`, `resize` and `--focus`
//! share, and the layout requests they are.
//!
//! The rule protects what the person is typing. A command that changes **where the keyboard
//! types** is allowed only while the cursor is in the pane that gave the command, or while the
//! person is not typing in bateri at all:
//!
//! | situation                                                      | result                                             |
//! |----------------------------------------------------------------|----------------------------------------------------|
//! | the cursor is in the pane that gave the command                | works; another window as target comes to the front |
//! | bateri is behind                                               | works; bateri does not come to the front           |
//! | bateri is in front, the target is in another window            | works; the cursor stays, that window stays behind  |
//! | bateri is in front, the target is in the window being typed in, the cursor is in another pane | refused (77) with the reason |
//!
//! It holds at every level, `full` too, and looks at the **target's window**. A layout command
//! (`zoom`, `swap`, `move`, `resize`) whose tab is not on screen changes nothing anyone types
//! into and is free. Everything here is a pure function; the Mac reads the keyboard and carries
//! the answer out.

/// Where the keyboard is, as the Mac reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Keyboard {
    /// bateri is the application in front.
    pub front: bool,
    /// The window the person types in (the key window), when bateri is in front.
    pub window: Option<u64>,
    /// The cursor is in the pane that gave the command.
    pub caller_holds: bool,
}

/// What may be done about a target in window `target`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Go {
    /// The target takes the keyboard and its window comes to the front.
    Raise,
    /// bateri is behind: the target is chosen in its window, which is put in front of the others
    /// of bateri without bateri coming to the front.
    Order,
    /// The target is chosen inside its window; the cursor stays where it is and the window stays
    /// where it is.
    Within,
    /// Not done: the person would lose the keyboard from under their hands.
    Refused,
}

/// The four rows of the table.
pub const fn go(keyboard: Keyboard, target: u64) -> Go {
    if keyboard.caller_holds {
        return Go::Raise;
    }
    if !keyboard.front {
        return Go::Order;
    }
    match keyboard.window {
        Some(typing) if typing == target => Go::Refused,
        _ => Go::Within,
    }
}

/// Whether a layout command on a tab of window `target` may go on: a tab not on screen is free;
/// one on screen follows [`go`] (only its refusal matters, a layout command raises no window).
pub const fn lays_out(keyboard: Keyboard, target: u64, on_screen: bool) -> bool {
    !on_screen || !matches!(go(keyboard, target), Go::Refused)
}

/// The one line that says why the keyboard was not moved, and what to use instead.
pub const REFUSAL: &str = "the keyboard is in another pane of that window and this would move it; \
     ask the user to look with `bateri notify` instead";

#[cfg(test)]
mod tests {
    use super::*;

    const fn keyboard(front: bool, window: Option<u64>, caller_holds: bool) -> Keyboard {
        Keyboard {
            front,
            window,
            caller_holds,
        }
    }

    #[test]
    fn the_four_rows_of_the_table_hold() {
        // The cursor is in the pane that gave the command: it works, in its own window and in
        // another, which comes to the front.
        assert_eq!(go(keyboard(true, Some(1), true), 1), Go::Raise);
        assert_eq!(go(keyboard(true, Some(1), true), 2), Go::Raise);
        // bateri is behind: it works, nothing comes to the front.
        assert_eq!(go(keyboard(false, None, false), 1), Go::Order);
        assert_eq!(go(keyboard(false, None, false), 2), Go::Order);
        // bateri is in front, the target is in another window: the cursor stays.
        assert_eq!(go(keyboard(true, Some(1), false), 2), Go::Within);
        // The target is in the window being typed in and the cursor is elsewhere: refused.
        assert_eq!(go(keyboard(true, Some(1), false), 1), Go::Refused);
        // In front with no window typed in (a window being closed, a menu): nothing is lost.
        assert_eq!(go(keyboard(true, None, false), 1), Go::Within);
    }

    #[test]
    fn a_layout_command_is_free_where_nobody_types_and_refused_where_they_do() {
        let typing = keyboard(true, Some(1), false);
        assert!(!lays_out(typing, 1, true));
        assert!(lays_out(typing, 1, false), "a tab that is not on screen");
        assert!(lays_out(typing, 2, true), "another window");
        assert!(lays_out(keyboard(true, Some(1), true), 1, true));
        assert!(lays_out(keyboard(false, None, false), 1, true));
    }

    #[test]
    fn the_refusal_points_at_notify_and_is_one_line() {
        assert!(REFUSAL.contains("bateri notify"));
        assert!(!REFUSAL.contains('\n'));
    }
}
