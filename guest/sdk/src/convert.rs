//! The two crossings of the page interface on the page's side: the tree and the reply the
//! page hands out, put into the interface's types, and the event the application tells, read
//! into the engine's. The application's side of the same crossings is
//! `crates/bt-view-wasm/src/world.rs`.

use bt_view::wire::{Direction, Display, Items, Justify, Kind, Length, Node, Overflow, Style};
use bt_view::{
    Align, Button, Color, Cursor, Event, Family, Input, Key, Keyboard, MenuEntry, Mods, Point,
    Reply, Tone, Track,
};

use crate::Frame;
use crate::bindings::bateri::view::types as wit;

// --- What the page hands out ---------------------------------------------------------------

fn tone(tone: Tone) -> wit::Tone {
    match tone {
        Tone::Background => wit::Tone::Background,
        Tone::Foreground => wit::Tone::Foreground,
        Tone::Dim => wit::Tone::Dim,
        Tone::Accent => wit::Tone::Accent,
        Tone::Cursor => wit::Tone::Cursor,
        Tone::Success => wit::Tone::Success,
        Tone::Error => wit::Tone::Error,
        Tone::Info => wit::Tone::Info,
        Tone::Warning => wit::Tone::Warning,
        Tone::Raised(steps) => wit::Tone::Raised(steps),
        Tone::Quiet => wit::Tone::Quiet,
        Tone::Line => wit::Tone::Line,
        Tone::Shadow => wit::Tone::Shadow,
    }
}

fn color(color: Color) -> wit::Color {
    wit::Color {
        tone: tone(color.tone),
        alpha: color.alpha,
    }
}

fn length(length: Length) -> wit::Length {
    match length {
        Length::Auto => wit::Length::Auto,
        Length::Px(points) => wit::Length::Px(points),
        Length::Percent(percent) => wit::Length::Percent(percent),
    }
}

fn track(track: &Track) -> wit::Track {
    match *track {
        Track::Px(points) => wit::Track::Px(points),
        Track::Fr(fr) => wit::Track::Fr(fr),
        Track::Range { min, fr } => wit::Track::Range(wit::Range { min, fr }),
        Track::Auto => wit::Track::Auto,
    }
}

fn edges(edges: [f32; 4]) -> wit::Edges {
    wit::Edges {
        top: edges[0],
        right: edges[1],
        bottom: edges[2],
        left: edges[3],
    }
}

fn style(style: Style) -> wit::Style {
    wit::Style {
        display: match style.display {
            Display::Flex => wit::Display::Flex,
            Display::Grid => wit::Display::Grid,
            Display::None => wit::Display::None,
        },
        direction: match style.direction {
            Direction::Row => wit::Direction::Row,
            Direction::Column => wit::Direction::Column,
        },
        wrap: style.wrap,
        absolute: style.absolute,
        inset: wit::Inset {
            top: style.inset[0],
            right: style.inset[1],
            bottom: style.inset[2],
            left: style.inset[3],
        },
        width: length(style.width),
        height: length(style.height),
        min_width: length(style.min_width),
        min_height: length(style.min_height),
        max_width: length(style.max_width),
        max_height: length(style.max_height),
        padding: edges(style.padding),
        margin: edges(style.margin),
        gap_x: style.gap.0,
        gap_y: style.gap.1,
        grow: style.grow,
        shrink: style.shrink,
        items: style.items.map(|items| match items {
            Items::Start => wit::Items::Start,
            Items::Center => wit::Items::Center,
            Items::End => wit::Items::End,
            Items::Stretch => wit::Items::Stretch,
        }),
        justify: style.justify.map(|justify| match justify {
            Justify::Start => wit::Justify::Start,
            Justify::Center => wit::Justify::Center,
            Justify::End => wit::Justify::End,
            Justify::Between => wit::Justify::Between,
        }),
        columns: style.columns.iter().map(track).collect(),
        rows: style.rows.iter().map(track).collect(),
        column_span: style.column_span,
        overflow: match style.overflow {
            Overflow::Visible => wit::Overflow::Visible,
            Overflow::Hidden => wit::Overflow::Hidden,
            Overflow::Scroll => wit::Overflow::Scroll,
        },
        scroll_x: style.scroll.0,
        scroll_y: style.scroll.1,
        fill: style.fill.map(color),
        border: style.border.map(|border| wit::Border {
            width: border.width,
            color: color(border.color),
        }),
        radii: wit::Corners {
            top_left: style.radii[0],
            top_right: style.radii[1],
            bottom_right: style.radii[2],
            bottom_left: style.radii[3],
        },
        shadow: style.shadow.map(|shadow| wit::Shadow {
            dx: shadow.dx,
            dy: shadow.dy,
            blur: shadow.blur,
            color: color(shadow.color),
        }),
        text: wit::TextStyle {
            family: style.text.family.map(|family| match family {
                Family::System => wit::Family::System,
                Family::Terminal => wit::Family::Terminal,
            }),
            size: style.text.size,
            weight: style.text.weight,
            color: style.text.color.map(color),
            align: style.text.align.map(|align| match align {
                Align::Start => wit::Align::Start,
                Align::Center => wit::Align::Center,
                Align::End => wit::Align::End,
            }),
        },
        truncate: style.truncate,
    }
}

fn node(node: Node) -> wit::Node {
    wit::Node {
        content: match node.kind {
            Kind::Box => wit::Content::Container,
            Kind::Text(text) => wit::Content::Text(text),
            Kind::Spark(spark) => wit::Content::Spark(wit::Spark {
                values: spark.values,
                color: color(spark.color),
                peak: spark.peak,
            }),
        },
        style: style(node.style),
        id: node.meta.id.map(|id| id.number()),
        pressable: node.meta.pressable,
        focusable: node.meta.focusable,
        text_field: node.meta.text_field,
        children: node.children,
    }
}

/// The page, as the interface's list of nodes.
pub fn to_wit_frame(frame: Frame) -> wit::Frame {
    wit::Frame {
        nodes: frame.root.fold().into_iter().map(node).collect(),
        animating: frame.animating,
        wake_at: frame.wake_at,
    }
}

/// The page's answer to an event, as the interface's.
pub fn to_wit_reply(reply: Reply) -> wit::Reply {
    wit::Reply {
        redraw: reply.redraw,
        close: reply.close,
        menu: reply.menu.map(|menu| wit::Menu {
            at: wit::Point {
                x: menu.at.x,
                y: menu.at.y,
            },
            entries: menu
                .entries
                .into_iter()
                .map(|entry| match entry {
                    MenuEntry::Separator => wit::MenuEntry::Separator,
                    MenuEntry::Item {
                        title,
                        key,
                        enabled,
                    } => wit::MenuEntry::Item(wit::MenuItem {
                        title,
                        key,
                        enabled,
                    }),
                })
                .collect(),
        }),
        keyboard: reply.keyboard.map(|keyboard| match keyboard {
            Keyboard::Page => wit::Keyboard::Page,
            Keyboard::Filter => wit::Keyboard::Field,
        }),
        copy: reply.copy,
        cursor: match reply.cursor {
            Cursor::Arrow => wit::Cursor::Arrow,
            Cursor::Pointer => wit::Cursor::Pointer,
            Cursor::Text => wit::Cursor::Text,
        },
    }
}

// --- What the application tells ------------------------------------------------------------

fn point(at: wit::Point) -> Point {
    Point::new(at.x, at.y)
}

fn button(button: wit::Button) -> Button {
    match button {
        wit::Button::Left => Button::Left,
        wit::Button::Right => Button::Right,
    }
}

fn mods(mods: wit::Mods) -> Mods {
    Mods {
        shift: mods.shift,
        control: mods.control,
        option: mods.alt,
        command: mods.command,
    }
}

fn key(key: wit::Key) -> Key {
    match key {
        wit::Key::Up => Key::Up,
        wit::Key::Down => Key::Down,
        wit::Key::Left => Key::Left,
        wit::Key::Right => Key::Right,
        wit::Key::PageUp => Key::PageUp,
        wit::Key::PageDown => Key::PageDown,
        wit::Key::Home => Key::Home,
        wit::Key::End => Key::End,
        wit::Key::Enter => Key::Enter,
        wit::Key::Escape => Key::Escape,
        wit::Key::Tab => Key::Tab,
        wit::Key::Character(c) => Key::Char(c),
    }
}

/// What the application told, as the engine's.
pub fn from_wit_event(event: wit::Event) -> Event {
    match event {
        wit::Event::PointerMove(at) => Event::Input(Input::Move(point(at))),
        wit::Event::PointerLeave => Event::Input(Input::Leave),
        wit::Event::Press(press) => Event::Input(Input::Press {
            at: point(press.at),
            button: button(press.button),
            mods: mods(press.mods),
        }),
        wit::Event::Release(release) => Event::Input(Input::Release {
            at: point(release.at),
            button: button(release.button),
        }),
        wit::Event::Scroll(scroll) => Event::Input(Input::Scroll {
            at: point(scroll.at),
            dy: scroll.dy,
        }),
        wit::Event::Key(pressed) => Event::Input(Input::Key {
            key: key(pressed.key),
            mods: mods(pressed.mods),
        }),
        wit::Event::Text(text) => Event::Text(text),
        wit::Event::MenuPick(index) => Event::MenuPick(index as usize),
    }
}
