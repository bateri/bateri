//! The mouse pointer's shape a program asks for — OSC 22, by kitty's protocol:
//! `OSC 22 ; name ST` sets it, an empty request resets it, `>a,b` pushes,
//! `<` pops and `?a,b` asks which names are supported (plus `__current__`,
//! `__default__` and `__grabbed__`). The shell shows the shape over the grid
//! ([`crate::Session::pointer_shape`]); a page drawn by a program
//! (terminal-browser) gets a hand over its links this way.
//!
//! **Two stacks, one per screen**, as the protocol asks: a full-screen
//! program's shapes live on the alternate screen's stack, so leaving it brings
//! back the main screen's without the program resetting anything, and a reset
//! (`ESC c`) empties both. On top of the protocol the terminal empties both
//! when the shell prints its prompt: a program that ends on the main screen
//! without a reset does not leave its pointer over the prompt. The prompt's
//! mark is scanned before that read's bytes apply, so a shape set in the same
//! read as the prompt outlives it — a known limit.
//!
//! **Names are CSS's**, read by `cursor-icon` (kitty's thirty and four more:
//! `context-menu`, `col-resize`, `row-resize`, `all-scroll`); xterm's X11
//! names are optional in the protocol and not read. A set takes the first
//! known name of its list; a set or a push with no known name leaves the stack
//! alone.
//!
//! **Only the live stream keeps them.** The replays that rebuild a session's
//! `Term` (a bare replay) have no sink and drop the requests. Unlike DECSET 1016 this is
//! deliberate: a lost shape breaks no program, the pointer is bateri's own after an update
//! or a crash until the program asks again — a page under the pointer does at the next
//! hover.

use std::str::FromStr;
use std::sync::{Mutex, MutexGuard, PoisonError};

// vte does not re-export this type; the reason for the edge is in the root
// `Cargo.toml`.
use cursor_icon::CursorIcon;

/// How many shapes a screen's stack keeps: the protocol's minimum. A push
/// past it drops the bottom one.
const STACK_LIMIT: usize = 16;

/// What `?__default__` and `?__grabbed__` answer: the pane shows the arrow
/// over the grid, and mouse reporting does not change it.
const OWN_SHAPE: &str = "default";

/// A pointer shape a program can ask for — the CSS names the protocol reads,
/// for the shell to map to its platform's cursors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PointerShape {
    Default,
    ContextMenu,
    Help,
    Pointer,
    Progress,
    Wait,
    Cell,
    Crosshair,
    Text,
    VerticalText,
    Alias,
    Copy,
    Move,
    NoDrop,
    NotAllowed,
    Grab,
    Grabbing,
    EResize,
    NResize,
    NeResize,
    NwResize,
    SResize,
    SeResize,
    SwResize,
    WResize,
    EwResize,
    NsResize,
    NeswResize,
    NwseResize,
    ColResize,
    RowResize,
    AllScroll,
    ZoomIn,
    ZoomOut,
}

impl From<CursorIcon> for PointerShape {
    fn from(icon: CursorIcon) -> Self {
        match icon {
            CursorIcon::ContextMenu => Self::ContextMenu,
            CursorIcon::Help => Self::Help,
            CursorIcon::Pointer => Self::Pointer,
            CursorIcon::Progress => Self::Progress,
            CursorIcon::Wait => Self::Wait,
            CursorIcon::Cell => Self::Cell,
            CursorIcon::Crosshair => Self::Crosshair,
            CursorIcon::Text => Self::Text,
            CursorIcon::VerticalText => Self::VerticalText,
            CursorIcon::Alias => Self::Alias,
            CursorIcon::Copy => Self::Copy,
            CursorIcon::Move => Self::Move,
            CursorIcon::NoDrop => Self::NoDrop,
            CursorIcon::NotAllowed => Self::NotAllowed,
            CursorIcon::Grab => Self::Grab,
            CursorIcon::Grabbing => Self::Grabbing,
            CursorIcon::EResize => Self::EResize,
            CursorIcon::NResize => Self::NResize,
            CursorIcon::NeResize => Self::NeResize,
            CursorIcon::NwResize => Self::NwResize,
            CursorIcon::SResize => Self::SResize,
            CursorIcon::SeResize => Self::SeResize,
            CursorIcon::SwResize => Self::SwResize,
            CursorIcon::WResize => Self::WResize,
            CursorIcon::EwResize => Self::EwResize,
            CursorIcon::NsResize => Self::NsResize,
            CursorIcon::NeswResize => Self::NeswResize,
            CursorIcon::NwseResize => Self::NwseResize,
            CursorIcon::ColResize => Self::ColResize,
            CursorIcon::RowResize => Self::RowResize,
            CursorIcon::AllScroll => Self::AllScroll,
            CursorIcon::ZoomIn => Self::ZoomIn,
            CursorIcon::ZoomOut => Self::ZoomOut,
            // `Default`, and the icons no name parses to (`dnd-ask`,
            // `all-resize`, whatever a later version adds).
            _ => Self::Default,
        }
    }
}

/// The session's pointer shapes: both screens' stacks and the shape last
/// shown, behind a leaf lock — the wrapper applies the stream to it under the
/// `Term` lock, the shell reads the shown shape from the main thread.
#[derive(Default)]
pub(crate) struct PointerShapes(Mutex<Stacks>);

impl PointerShapes {
    /// Applies a request — OSC 22's payload as written — to the screen's
    /// stack: the answer to a query, ready to write, and whether the shown
    /// shape changed.
    pub(crate) fn request(
        &self,
        alternate: bool,
        request: &[u8],
        terminator: &str,
    ) -> (Option<Vec<u8>>, bool) {
        let mut stacks = self.lock();
        let reply = stacks.apply(alternate, request, terminator);
        (reply, stacks.show(alternate))
    }

    /// Shows the screen's top shape — after a move to or from the alternate
    /// screen; whether it changed.
    pub(crate) fn show(&self, alternate: bool) -> bool {
        self.lock().show(alternate)
    }

    /// Empties both stacks — a reset, the shell's prompt; whether the shown
    /// shape changed.
    pub(crate) fn clear(&self) -> bool {
        self.lock().clear()
    }

    /// The shape shown: the top of the current screen's stack, `None` with
    /// the stack empty (the pane's own pointer).
    pub(crate) fn shown(&self) -> Option<PointerShape> {
        self.lock().shown.map(PointerShape::from)
    }

    fn lock(&self) -> MutexGuard<'_, Stacks> {
        // The slot holds plain data and no call under it can leave it half
        // written; a poisoned lock is read as it is.
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Both screens' stacks, bottom first, and the shape last shown.
#[derive(Default)]
struct Stacks {
    main: Vec<CursorIcon>,
    alternate: Vec<CursorIcon>,
    shown: Option<CursorIcon>,
}

impl Stacks {
    fn stack(&mut self, alternate: bool) -> &mut Vec<CursorIcon> {
        if alternate {
            &mut self.alternate
        } else {
            &mut self.main
        }
    }

    /// The request's operation applied to the screen's stack; the reply of a
    /// query. A request that is not UTF-8 names nothing and does nothing.
    fn apply(&mut self, alternate: bool, request: &[u8], terminator: &str) -> Option<Vec<u8>> {
        let request = std::str::from_utf8(request).ok()?;
        let stack = self.stack(alternate);
        let (op, names) = match request.as_bytes().first() {
            Some(op @ (b'=' | b'>' | b'<' | b'?')) => (*op, &request[1..]),
            _ => (b'=', request),
        };
        let known = || {
            names
                .split(',')
                .filter_map(|name| CursorIcon::from_str(name).ok())
        };
        match op {
            b'>' => {
                for icon in known() {
                    if stack.len() == STACK_LIMIT {
                        stack.remove(0);
                    }
                    stack.push(icon);
                }
            }
            b'<' => {
                stack.pop();
            }
            b'?' => {
                if names.is_empty() {
                    return None;
                }
                let current = stack.last().map_or("0", CursorIcon::name);
                let answers: Vec<&str> = names
                    .split(',')
                    .map(|name| match name {
                        "__current__" => current,
                        "__default__" | "__grabbed__" => OWN_SHAPE,
                        _ if CursorIcon::from_str(name).is_ok() => "1",
                        _ => "0",
                    })
                    .collect();
                return Some(format!("\x1b]22;{}{terminator}", answers.join(",")).into_bytes());
            }
            // A set: nothing named resets the shape, a known name replaces
            // the top (or is the first one).
            _ if names.is_empty() => stack.clear(),
            _ => {
                if let Some(icon) = known().next() {
                    match stack.last_mut() {
                        Some(top) => *top = icon,
                        None => stack.push(icon),
                    }
                }
            }
        }
        None
    }

    fn show(&mut self, alternate: bool) -> bool {
        let top = self.stack(alternate).last().copied();
        let changed = top != self.shown;
        self.shown = top;
        changed
    }

    fn clear(&mut self) -> bool {
        self.main.clear();
        self.alternate.clear();
        self.shown.take().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ST: &str = "\x1b\\";

    /// Applies each request to the main screen; the shown shape and the
    /// replies, in order.
    fn after(requests: &[&str]) -> (Option<PointerShape>, Vec<String>) {
        let shapes = PointerShapes::default();
        let replies = requests
            .iter()
            .filter_map(|request| shapes.request(false, request.as_bytes(), ST).0)
            .map(|reply| String::from_utf8(reply).expect("a reply is text"))
            .collect();
        (shapes.shown(), replies)
    }

    #[test]
    fn a_set_replaces_the_top_and_nothing_named_resets() {
        assert_eq!(after(&["pointer"]).0, Some(PointerShape::Pointer));
        assert_eq!(after(&["=pointer"]).0, Some(PointerShape::Pointer));
        assert_eq!(after(&["pointer", "text"]).0, Some(PointerShape::Text));
        assert_eq!(after(&["pointer", ""]).0, None);
        assert_eq!(after(&["pointer", "="]).0, None);
        // A set does not stack: one pop after two sets leaves nothing.
        assert_eq!(after(&["pointer", "text", "<"]).0, None);
    }

    #[test]
    fn a_set_takes_the_first_known_name_and_an_unknown_one_changes_nothing() {
        assert_eq!(after(&["no-such,wait,text"]).0, Some(PointerShape::Wait));
        assert_eq!(
            after(&["pointer", "no-such"]).0,
            Some(PointerShape::Pointer)
        );
        // xterm's X11 names are not read.
        assert_eq!(after(&["hand2"]).0, None);
        assert_eq!(after(&[">no-such"]).0, None);
    }

    #[test]
    fn a_push_stacks_every_name_and_a_pop_goes_back() {
        assert_eq!(after(&[">wait,pointer"]).0, Some(PointerShape::Pointer));
        assert_eq!(after(&[">wait,pointer", "<"]).0, Some(PointerShape::Wait));
        assert_eq!(after(&[">wait,pointer", "<", "<", "<"]).0, None);
        assert_eq!(
            after(&["text", ">pointer", "<"]).0,
            Some(PointerShape::Text)
        );
    }

    #[test]
    fn a_full_stack_drops_its_bottom() {
        let mut requests = vec![">text"];
        requests.extend(std::iter::repeat_n(">pointer", STACK_LIMIT));
        requests.extend(std::iter::repeat_n("<", STACK_LIMIT - 1));
        assert_eq!(
            after(&requests).0,
            Some(PointerShape::Pointer),
            "the text went"
        );
    }

    #[test]
    fn a_query_answers_each_name_in_turn() {
        let (_, replies) = after(&[
            "?pointer,crosshair,no-such,wait,hand2",
            "?__current__",
            ">grab",
            "?__current__,__default__,__grabbed__",
            "?",
        ]);
        assert_eq!(
            replies,
            [
                "\x1b]22;1,1,0,1,0\x1b\\",
                "\x1b]22;0\x1b\\",
                "\x1b]22;grab,default,default\x1b\\",
            ]
        );
        let shapes = PointerShapes::default();
        let (reply, changed) = shapes.request(false, b"?__current__", "\x07");
        assert_eq!(
            reply.as_deref(),
            Some(&b"\x1b]22;0\x07"[..]),
            "the query's terminator"
        );
        assert!(!changed, "a query changes nothing");
    }

    #[test]
    fn each_screen_keeps_its_own_stack() {
        let shapes = PointerShapes::default();
        assert!(shapes.request(false, b"text", ST).1);
        assert!(shapes.show(true), "the alternate screen's stack is empty");
        assert_eq!(shapes.shown(), None);
        assert!(shapes.request(true, b"pointer", ST).1);
        assert_eq!(shapes.shown(), Some(PointerShape::Pointer));
        assert!(shapes.show(false));
        assert_eq!(shapes.shown(), Some(PointerShape::Text));
        assert!(!shapes.show(false), "the same shape is no change");
        assert!(shapes.clear());
        assert_eq!(shapes.shown(), None);
        assert!(!shapes.show(true), "a clear empties both");
        assert!(!shapes.clear(), "nothing shown, nothing changed");
    }

    #[test]
    fn a_request_that_is_not_text_does_nothing() {
        let shapes = PointerShapes::default();
        assert_eq!(shapes.request(false, b"po\xffinter", ST), (None, false));
        assert_eq!(shapes.request(false, b"?\xff", ST), (None, false));
    }
}
