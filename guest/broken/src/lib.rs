//! A page that goes wrong on purpose, to see the sandbox hold: four rows, each a way for a module
//! to misbehave. Click one (or press its letter) and the application closes the page and gives the
//! pane back to the terminal — none of them can hang or crash the application.
//!
//! - **L** never returns from its event,
//! - **M** takes memory until it is refused,
//! - **S** calls itself until the stack is gone,
//! - **T** traps.
//!
//! Esc closes the page the polite way.

use bateri_view_sdk::{
    Align, Button, Cursor, Event, Frame, Id, Input, Key, Layout, Page, Queries, Reply, Size, Tone,
    div, text,
};

const LOOP: Id = Id::named("broken loop");
const MEMORY: Id = Id::named("broken memory");
const STACK: Id = Id::named("broken stack");
const TRAP: Id = Id::named("broken trap");

struct Broken;

fn row(id: Id, key: char, title: &str, detail: &str) -> bateri_view_sdk::Element {
    div()
        .row()
        .items_center()
        .gap(12.0)
        .px(14.0)
        .h(44.0)
        .rounded(8.0)
        .bg(Tone::Raised(1))
        .border(1.0, Tone::Line)
        .press(id)
        .child(
            div()
                .w(26.0)
                .h(26.0)
                .rounded(6.0)
                .border(1.0, Tone::Line)
                .items_center()
                .justify_center()
                .child(text(key.to_string().to_uppercase()).weight(600)),
        )
        .child(text(title).weight(600))
        .child(text(detail).color(Tone::Dim))
}

fn misbehave(id: Id) {
    if id == LOOP {
        loop {
            std::hint::black_box(());
        }
    } else if id == MEMORY {
        let mut held: Vec<Vec<u8>> = Vec::new();
        loop {
            held.push(vec![1; 1 << 20]);
            std::hint::black_box(&held);
        }
    } else if id == STACK {
        std::hint::black_box(recurse(1));
    } else if id == TRAP {
        panic!("this page trapped on purpose");
    }
}

#[allow(unconditional_recursion)]
fn recurse(depth: u64) -> u64 {
    std::hint::black_box(recurse(depth + 1)) + depth
}

impl Page for Broken {
    fn new() -> Self {
        Self
    }

    fn render(&mut self, _size: Size, _now: f64, _reduce_motion: bool) -> Frame {
        Frame::still(
            div()
                .size_full()
                .p(24.0)
                .gap(10.0)
                .child(text("A page that goes wrong on purpose").text_size(15.0).weight(600))
                .child(
                    text("Each row is a way for a module to misbehave. The application closes the page and the terminal comes back.")
                        .color(Tone::Dim)
                        .align(Align::Start),
                )
                .child(div().h(6.0))
                .child(row(LOOP, 'l', "Loop forever", "never returns from the event"))
                .child(row(MEMORY, 'm', "Take memory", "until it is refused"))
                .child(row(STACK, 's', "Recurse", "until the stack is gone"))
                .child(row(TRAP, 't', "Trap", "an unreachable"))
                .child(text("Esc closes the page.").color(Tone::Dim)),
        )
    }

    fn settle(&mut self, _layout: &Layout) -> bool {
        false
    }

    fn handle(&mut self, event: Event, layout: &Layout) -> Reply {
        match event {
            Event::Input(Input::Release {
                at,
                button: Button::Left,
            }) => {
                if let Some(id) = layout.hit(at) {
                    misbehave(id);
                }
                Reply::none()
            }
            Event::Input(Input::Move(at)) => {
                let over = layout.hit(at).is_some();
                Reply::none().with_cursor(if over { Cursor::Pointer } else { Cursor::Arrow })
            }
            Event::Input(Input::Key { key, .. }) => match key {
                Key::Char('l') => {
                    misbehave(LOOP);
                    Reply::none()
                }
                Key::Char('m') => {
                    misbehave(MEMORY);
                    Reply::none()
                }
                Key::Char('s') => {
                    misbehave(STACK);
                    Reply::none()
                }
                Key::Char('t') => {
                    misbehave(TRAP);
                    Reply::none()
                }
                Key::Escape => Reply {
                    close: true,
                    ..Reply::none()
                },
                _ => Reply::none(),
            },
            _ => Reply::none(),
        }
    }
}

bateri_view_sdk::export_page!(Broken);
