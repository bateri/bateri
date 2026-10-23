//! The container list of the engine's demo, as a page that runs in the sandbox.
//!
//! The page is the engine's own `Containers` (the same one the application shows without a
//! sandbox): this crate is what makes it a module — what it is told goes to the page and what
//! the page hands out goes back, through the SDK. The page keeps its own state (the hover, the
//! scroll, the highlights on their way) in the module and asks the application where things
//! stand on the layout through [`Layout`].

use bateri_view_sdk::{Event, Frame, Layout, Page, Reply, Size, export_page};
use bt_view::demo::Containers;

struct ContainerList(Containers);

impl Page for ContainerList {
    fn new() -> Self {
        Self(Containers::fixture())
    }

    fn render(&mut self, size: Size, now: f64, reduce_motion: bool) -> Frame {
        self.0.advance(now, reduce_motion);
        Frame {
            root: self.0.view(size),
            animating: self.0.animating(),
        }
    }

    fn settle(&mut self, layout: &Layout) -> bool {
        self.0.settle(layout)
    }

    fn handle(&mut self, event: Event, layout: &Layout) -> Reply {
        match event {
            Event::Input(input) => self.0.input(input, layout),
            Event::Text(text) => self.0.set_query(&text),
            Event::MenuPick(index) => self.0.pick(index),
        }
    }
}

export_page!(ContainerList);
