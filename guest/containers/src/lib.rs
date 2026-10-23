//! The container list of the engine's demo, as a page that runs in the sandbox.
//!
//! The page is the engine's own `Containers` (the same one the application shows without a
//! sandbox): this crate is what makes it a module — what it is told goes to the page and what
//! the page hands out goes back, through the SDK. The page keeps its own state (the hover, the
//! scroll, the highlights on their way) in the module and asks the application where things
//! stand on the layout through [`Layout`].
//!
//! When a program opened the page, the page asks it for the numbers of the running containers
//! (`subscribe` to `stats`) and takes what the program pushes, a reading for each container
//! once a second: nothing asks again, so a page whose program is quiet draws nothing.

use bateri_view_messages::Message;
use bateri_view_sdk::{Event, Frame, Layout, Page, Reply, Size, export_page, say};
use bt_view::demo::{Containers, Reading};
use serde_json::Value;

/// The number the page gives its one subscription.
const STATS: u64 = 1;

struct ContainerList(Containers);

impl Page for ContainerList {
    fn new() -> Self {
        // Said into the void where no program opened the page: the list stays as it opens.
        say(&Message::subscribe(STATS, "stats", Value::Null).text());
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

    fn message(&mut self, text: &str, _layout: &Layout) -> Reply {
        let Ok(Message::Push(push)) = Message::parse(text) else {
            // A response to nothing we asked, a message that is not one: nothing to show.
            return Reply::none();
        };
        if push.id != STATS {
            return Reply::none();
        }
        let readings = readings_of(&push.value);
        if self.0.live(&readings) {
            Reply::redraw()
        } else {
            Reply::none()
        }
    }
}

/// The readings a `stats` push carries: `{"readings": [{"name", "cpu", "memory", "share"}, ..]}`.
/// What is not shaped so is left out.
fn readings_of(value: &Value) -> Vec<Reading> {
    let Some(list) = value.get("readings").and_then(Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|one| {
            Some(Reading {
                name: one.get("name")?.as_str()?.to_owned(),
                cpu: one.get("cpu")?.as_f64()? as f32,
                memory: one.get("memory")?.as_str()?.to_owned(),
                share: one.get("share")?.as_f64()? as f32,
            })
        })
        .take(256)
        .collect()
}

export_page!(ContainerList);
