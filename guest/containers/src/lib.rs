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
//!
//! One thing more than the list: **Shift-U sends the program a large payload** (4 MiB, in chunks
//! of 48 KiB), as an example of a page with a lot to say to a program that reads when it can. The
//! program may not have read what it was told before; `say` answers that, and the page keeps the
//! chunk and goes on when it is called in [`Page::room`]. It does not try again by itself, and it
//! says at most a few chunks in a call (a call may say only so many), going on in the next frame.

use bateri_view_messages::Message;
use bateri_view_sdk::{
    Event, Frame, Input, Key, Layout, Page, Reply, Size, Tone, div, export_page, say, text,
};
use bt_view::demo::{Containers, Reading};
use serde_json::{Value, json};

/// The number the page gives its one subscription.
const STATS: u64 = 1;

/// The key that sends the large payload.
const UPLOAD_KEY: char = 'U';

/// How much of the payload there is, in bytes of the data the chunks carry.
const UPLOAD_BYTES: usize = 4 << 20;

/// How much of it one message carries.
const CHUNK: usize = 48 << 10;

/// The call number of the last chunk: the program answers that one, with how much it counted.
const LAST_CALL: u64 = UPLOAD_BYTES.div_ceil(CHUNK) as u64;

/// The most chunks the page says in one call: a call may say 32 messages and is closed if it
/// says more, and the room the program has is not a reason to say them all at once.
const PER_CALL: usize = 8;

/// The payload on its way out.
struct Upload {
    /// The call number of the next chunk, from 1.
    next: u64,
    /// Bytes of data the program has been told of so far.
    told: usize,
    /// A chunk the program had no room for: the one to say first when there is.
    kept: Option<(String, usize)>,
    /// The program has no room: nothing is said until the page is called in [`Page::room`].
    waiting: bool,
}

impl Upload {
    fn new() -> Self {
        Self {
            next: 1,
            told: 0,
            kept: None,
            waiting: false,
        }
    }

    /// The next chunk and how much data it carries, if there is more. The last one says so.
    fn chunk(&mut self, offered: usize) -> Option<(String, usize)> {
        if offered >= UPLOAD_BYTES {
            return None;
        }
        let size = CHUNK.min(UPLOAD_BYTES - offered);
        let last = offered + size >= UPLOAD_BYTES;
        let call = self.next;
        self.next += 1;
        let params = json!({ "seq": call, "data": "x".repeat(size), "last": last });
        Some((Message::call(call, "upload", params).text(), size))
    }

    /// Says what it can of the payload: stops at the call's own limit, and when the program has
    /// no room. Whether there is more to say and the page may go on without being called.
    fn pump(&mut self) -> bool {
        let mut offered = self.told;
        let mut said = 0;
        while said < PER_CALL {
            let (text, size) = match self.kept.take() {
                Some(kept) => kept,
                None => match self.chunk(offered) {
                    Some(next) => next,
                    None => return false,
                },
            };
            if say(&text) {
                self.told += size;
                offered = self.told;
                said += 1;
            } else {
                self.kept = Some((text, size));
                self.waiting = true;
                return false;
            }
        }
        self.told < UPLOAD_BYTES
    }

    /// How much of the payload the program has been told, in percent.
    fn percent(&self) -> usize {
        self.told * 100 / UPLOAD_BYTES
    }
}

struct ContainerList {
    list: Containers,
    /// The subscription could not be said yet (the program had no room), and goes first when
    /// the page is called in [`Page::room`].
    subscribe: bool,
    upload: Option<Upload>,
    /// The program has said something: there is one, and it is listening. A page nobody opened
    /// (the engine's own demo) has no one to send a payload to.
    heard: bool,
    /// What the program counted of the last payload, once it has said.
    counted: Option<u64>,
    /// There is more of the payload to say and nothing but the next frame calls the page.
    more: bool,
}

impl ContainerList {
    fn subscribe_message() -> String {
        Message::subscribe(STATS, "stats", Value::Null).text()
    }

    /// Says the payload on, as far as the program has the room.
    fn pump_upload(&mut self) {
        let Some(upload) = &mut self.upload else {
            self.more = false;
            return;
        };
        self.more = upload.pump();
    }

    /// What the page says of the payload, if it has anything to say: how far it has got while
    /// the program is being told, what the program counted when it has said.
    fn note(&self) -> Option<String> {
        match (&self.upload, self.counted) {
            (Some(upload), _) if upload.waiting => Some(format!(
                "Sending 4 MiB to demopen... {}% (it has not read the last yet)",
                upload.percent()
            )),
            (Some(upload), _) => Some(format!("Sending 4 MiB to demopen... {}%", upload.percent())),
            (None, Some(bytes)) => Some(format!("Sent 4 MiB - demopen counted {bytes} bytes")),
            (None, None) => None,
        }
    }
}

impl Page for ContainerList {
    fn new() -> Self {
        // Said into the void where no program opened the page: the list stays as it opens.
        let said = say(&Self::subscribe_message());
        Self {
            list: Containers::fixture(),
            subscribe: !said,
            upload: None,
            heard: false,
            counted: None,
            more: false,
        }
    }

    fn render(&mut self, size: Size, now: f64, reduce_motion: bool) -> Frame {
        if self.more {
            self.pump_upload();
        }
        self.list.advance(now, reduce_motion);
        let list = self.list.view(size);
        // The payload says how it is doing over the list, above its status line.
        let root = match self.note() {
            Some(note) => div().size_full().child(list).child(
                div()
                    .absolute()
                    .inset(None, Some(12.0), Some(36.0), None)
                    .px(12.0)
                    .py(6.0)
                    .rounded(6.0)
                    .bg(Tone::Raised(2))
                    .border(1.0, Tone::Line)
                    .text_size(12.5)
                    .color(Tone::Foreground)
                    .child(text(note)),
            ),
            None => list,
        };
        Frame {
            root,
            animating: self.list.animating() || self.more,
        }
    }

    fn settle(&mut self, layout: &Layout) -> bool {
        self.list.settle(layout)
    }

    fn handle(&mut self, event: Event, layout: &Layout) -> Reply {
        match event {
            Event::Input(Input::Key {
                key: Key::Char(UPLOAD_KEY),
                mods,
            }) if !mods.control && !mods.option && !mods.command => {
                if self.upload.is_none() && self.heard {
                    self.upload = Some(Upload::new());
                    self.counted = None;
                    self.pump_upload();
                }
                Reply::redraw()
            }
            Event::Input(input) => self.list.input(input, layout),
            Event::Text(text) => self.list.set_query(&text),
            Event::MenuPick(index) => self.list.pick(index),
        }
    }

    fn room(&mut self, _layout: &Layout) -> Reply {
        if self.subscribe {
            self.subscribe = !say(&Self::subscribe_message());
        }
        if let Some(upload) = &mut self.upload {
            upload.waiting = false;
        }
        if !self.subscribe {
            self.pump_upload();
        }
        Reply::redraw()
    }

    fn message(&mut self, text: &str, _layout: &Layout) -> Reply {
        self.heard = true;
        let push = match Message::parse(text) {
            Ok(Message::Push(push)) => push,
            // The program counted the payload: it says how much it got.
            Ok(Message::Response(response))
                if response.id == LAST_CALL && self.upload.is_some() =>
            {
                self.counted = response
                    .result
                    .as_ref()
                    .and_then(|result| result.get("bytes"))
                    .and_then(Value::as_u64);
                self.upload = None;
                self.more = false;
                return Reply::redraw();
            }
            // A response to nothing we asked, a message that is not one: nothing to show.
            _ => return Reply::none(),
        };
        if push.id != STATS {
            return Reply::none();
        }
        let readings = readings_of(&push.value);
        if self.list.live(&readings) {
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
