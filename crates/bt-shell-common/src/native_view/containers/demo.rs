//! The container list run as a page module is ([`pencere_sandbox::Runner`]), in this process: the
//! drive tells it what the person does and asks it for its page on a thread of its own
//! (`pencere_drive::Page::spawn`), as it does the list of pencere's sandboxed module
//! (`kit/guest/containers`), so that a debug build's demo is driven by the drive every page is, and
//! not by a drive of its own. No program is behind it: the list stays as it opens.

use std::sync::Arc;

use pencere_drive::NoText;
use pencere_sandbox::{Fault, Frame, Runner};
use pencere_view::{Event, Laid, Reply, ScrollTo, Size, div};

use super::Containers;

/// The container list, as a module.
pub(crate) struct Demo {
    list: Containers,
    /// The page as it was last laid out: what the person does is read against it. A layout of
    /// nothing until the first frame's, so that a key before it is answered and a pointer is over
    /// nothing.
    laid: Arc<Laid>,
}

impl Demo {
    pub(crate) fn new() -> Self {
        Self {
            list: Containers::fixture(),
            laid: Arc::new(pencere_view::layout(
                div(),
                Size::new(1.0, 1.0),
                1.0,
                &mut NoText,
            )),
        }
    }
}

impl Runner for Demo {
    fn render(&mut self, size: Size, now: f64, reduce_motion: bool) -> Result<Frame, Fault> {
        self.list.advance(now, reduce_motion);
        Ok(Frame {
            root: self.list.view(size),
            animating: self.list.animating(),
            wake_at: None,
            wake: None,
        })
    }

    fn settle(&mut self) -> Result<bool, Fault> {
        Ok(self.list.settle(&*self.laid))
    }

    fn handle(&mut self, event: Event) -> Result<Reply, Fault> {
        Ok(match event {
            Event::Input(input) => self.list.input(input, &*self.laid),
            Event::Text { text, .. } => self.list.set_query(&text),
            Event::MenuPick(index) => self.list.pick(index),
            Event::Scrolled(boxes) => self.list.scrolled(&boxes),
            // The list has no editor (no pointer over one, no edit, no selection), no field to
            // take the keyboard, no command, takes no files and asks no wake.
            _ => Reply::none(),
        })
    }

    fn lay_out(&mut self, laid: Arc<Laid>) {
        self.laid = laid;
    }

    fn take_asks(&mut self) -> Vec<ScrollTo> {
        self.list.take_asks()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pencere_view::{Input, Key, Mods, Point};

    /// What the list answers before it was ever laid out: a key, which needs no layout, and a
    /// pointer, which is over nothing.
    #[test]
    fn the_keyboard_is_answered_before_the_first_frame() {
        let mut demo = Demo::new();
        let escape = Input::Key {
            key: Key::Escape,
            mods: Mods::default(),
        };
        assert!(demo.handle(Event::Input(escape)).expect("a reply").close);
        let moved = demo
            .handle(Event::Input(Input::Move(Point::new(5.0, 50.0))))
            .expect("a reply");
        assert!(!moved.redraw);
    }
}
