//! A reset that keeps what the user has scrolled through.
//!
//! A picture of a screen (the bytes that build it, as a kablo server sends
//! them) is written to "a freshly reset terminal": it sets what differs from a
//! new terminal and says nothing about the rest. The usual way to get one is
//! `RIS` (`ESC c`), which also throws the scrollback away, and in a pane the
//! scrollback is not only the picture's terminal's: it holds the commands run
//! before the program in it started, the output of the ones before this
//! program's, everything the user could still scroll back to. [`keeping_history`]
//! is `RIS` without that: the terminal comes out the way `RIS` leaves it, the
//! history is what it was, and the screen that was showing is in it.
//!
//! **What it costs.** The screen that was showing goes into the history and
//! the picture's own history comes after it, so the lines that were on screen
//! at the disconnect and the picture's history can be there twice. A picture
//! that was abandoned half-written (the connection went again) stays in the
//! history for the same reason: no sequence takes lines back out of it.
//!
//! **What it leaves alone, on purpose.**
//!
//! - The palette (OSC 4/10/11/12): alacritty's `RIS` does not reset it either,
//!   and a pane's palette is as likely to be the local shell's (a theme script
//!   in an rc file) as the remote program's.
//! - The title stack: a pane's stack holds the entry the local shell pushed
//!   when it ran the command and pops when it ends, and nothing can pop only
//!   the entries that are not that one. (The title itself is not reset by
//!   `RIS` in bateri either: it is fed by the events.)
//! - The scroll position: a view scrolled up stays where it is, as it does
//!   for any output.
//! - Kitty graphics placements in the history: they stay with their lines.
//!   The ones on the screen are deleted by the erase, as on any `ED 2`.
//!
//! **Why each part is there** is at its line in [`keeping_history`]; the test
//! module runs the same stream through `RIS` and through this and compares
//! what the terminal ends up as.

use std::fmt::Write;

/// How deep the kitty keyboard stack can get (alacritty's
/// `KEYBOARD_MODE_STACK_MAX_DEPTH`): a pop of this many empties any stack.
const KEYBOARD_STACK_DEPTH: u16 = 4096;

/// The tab stop spacing of a new terminal (alacritty's `INITIAL_TABSTOPS`).
const TAB_STOP_EVERY: u16 = 8;

/// The bytes that bring a terminal of `columns` columns to what `RIS` leaves
/// it as, except for its history: the screen that was showing is pushed into
/// the history first (the erase of `ED 2` does it on the main screen, as on
/// Ctrl-L) and the screen is blank when the bytes end.
///
/// Written to the terminal's stream, in the stream's order; it needs no
/// answer and holds no state, so the stream may be cut anywhere inside it.
/// `columns` is the width the picture that follows was made for: the tab
/// stops of a new terminal depend on it.
#[must_use]
pub fn keeping_history(columns: u16) -> Vec<u8> {
    let mut out = String::new();

    // The screen that is showing goes first, while the terminal is on the
    // main screen with a plain pen: a full-screen program's screen is not the
    // history's, and the erase fills with the pen it finds.
    //
    // Off the alternate screen (a no-op on the main one): the program that
    // had it is gone, and so are the images it placed.
    out.push_str("\x1b[?1049l");
    // The pen and the link a cursor writes with: plain, nothing open.
    out.push_str("\x1b[0m\x1b]8;;\x1b\\");
    // Origin mode and the scrolling region: a picture's rows are written from
    // the top of the whole screen. (`DECSTBM` homes the cursor.)
    out.push_str("\x1b[?6l\x1b[r");
    // The screen into the history, up to its last row with anything in it;
    // the rest is blanked.
    out.push_str("\x1b[2J");

    // The cursor every cursor starts from: home, ASCII in all four sets and
    // the first of them in use. The alternate screen's cursor is a copy of
    // this one when it is entered, and so is its saved one below.
    out.push_str("\x1b[H\x1b(B\x1b)B\x1b*B\x1b+B\x0f");

    // The alternate screen's own state, which only a visit reaches (all of it
    // is per screen): the saved cursor (`DECSC` stores the copy that entering
    // made, which is the plain one above), the kitty keyboard stack and the
    // pointer shapes. Entering also saves the main screen's cursor as that
    // plain copy, which is how the main screen's saved cursor comes out
    // plain as well. (It also makes the session note the newest block's
    // command, as it does for a program taking the screen; that is the
    // command running in the pane, which the command memory reads anyway, so
    // the note is a duplicate it drops.)
    out.push_str("\x1b[?1049h\x1b7");
    keyboard_and_pointer(&mut out);
    out.push_str("\x1b[?1049l");

    // The same two on the main screen.
    keyboard_and_pointer(&mut out);

    // DECSET 1016 and 2048 are kept by the stream's wrapper, not by `Term`.
    out.push_str("\x1b[?1016l\x1b[?2048l");

    // The modes a new terminal starts in: cursor keys and keypad normal,
    // auto-wrap on, cursor shown, no mouse reporting or encoding, no focus
    // reports, alternate scroll on, urgency hints on, no bracketed paste,
    // replace mode and line feed without a carriage return. (`?3` is left
    // out: it clears the screen and `?12` is the cursor style's, below.)
    out.push_str(
        "\x1b[?1l\x1b>\x1b[?7h\x1b[?25h\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1004l\
         \x1b[?1005l\x1b[?1006l\x1b[?1007h\x1b[?1042h\x1b[?2004l\x1b[4l\x1b[20l",
    );

    // The cursor style the application chose: none.
    out.push_str("\x1b[0 q");

    // The tab stops of a terminal this wide: every eighth column, the first
    // included. Only a picture whose terminal has other stops writes any, so
    // the stops of the terminal being reset are not the picture's to fix.
    if columns > 0 {
        out.push_str("\x1b[3g");
        for column in (0..columns).step_by(usize::from(TAB_STOP_EVERY)) {
            // Writing to a `String` cannot fail.
            let _ = write!(out, "\x1b[{}G\x1bH", u32::from(column) + 1);
        }
    }

    // The cursor home: the first picture row is written from here (the tab
    // stops moved it).
    out.push_str("\x1b[H");

    out.into_bytes()
}

/// A screen's kitty keyboard stack, emptied, and its pointer shapes (OSC 22,
/// an empty request resets the screen's stack), for the screen that is on.
fn keyboard_and_pointer(out: &mut String) {
    // A pop of the stack's depth leaves the stack empty and the protocol's
    // flags at none; a stack that is not empty has more than the picture
    // writes.
    let _ = write!(out, "\x1b[<{KEYBOARD_STACK_DEPTH}u");
    out.push_str("\x1b]22;\x1b\\");
}

#[cfg(test)]
mod tests;
