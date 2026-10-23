//! A page with a lot to say to its program, to see the sandbox hold it to what the program can
//! take and no more: on any message of the program it starts to say numbered messages
//! (`{"push":{"id":1,"value":{"n":1}}}`, then 2, …) as long as the program has the room, keeps
//! the one it was told *full* to, and goes on from it when it is called in [`Page::room`]. It
//! never says more than `PER_CALL` in a call, which is a number of its own and below what a call
//! may say, so that the tests see a module that is told *full* and not one that is closed.
//!
//! What the program receives is the test: the numbers arrive one after the other with none lost,
//! none twice, and the page is never closed for being fast.

use bateri_view_sdk::{Event, Frame, Layout, Page, Reply, Size, div, export_page, say};

/// The most it says in one call.
const PER_CALL: usize = 30;

/// The most numbered messages in all.
const TOTAL: u64 = 100_000;

struct Chatty {
    /// The number of the next message to say.
    next: u64,
    /// The message the program had no room for, kept to be said first.
    kept: Option<String>,
    /// The program said something, and the page has begun to talk back.
    started: bool,
}

impl Chatty {
    fn text(n: u64) -> String {
        format!(r#"{{"push":{{"id":1,"value":{{"n":{n}}}}}}}"#)
    }

    /// Says what it can: until the program has no more room, a call's worth, or the end.
    fn talk(&mut self) {
        for _ in 0..PER_CALL {
            let text = match self.kept.take() {
                Some(kept) => kept,
                None if self.next <= TOTAL => {
                    let text = Self::text(self.next);
                    self.next += 1;
                    text
                }
                None => return,
            };
            if !say(&text) {
                self.kept = Some(text);
                return;
            }
        }
    }
}

impl Page for Chatty {
    fn new() -> Self {
        Self {
            next: 1,
            kept: None,
            started: false,
        }
    }

    fn render(&mut self, _size: Size, _now: f64, _reduce_motion: bool) -> Frame {
        Frame::still(div())
    }

    fn settle(&mut self, _layout: &Layout) -> bool {
        false
    }

    fn handle(&mut self, _event: Event, _layout: &Layout) -> Reply {
        Reply::none()
    }

    fn message(&mut self, _text: &str, _layout: &Layout) -> Reply {
        self.started = true;
        self.talk();
        Reply::none()
    }

    fn room(&mut self, _layout: &Layout) -> Reply {
        if self.started {
            self.talk();
        }
        Reply::none()
    }
}

export_page!(Chatty);
