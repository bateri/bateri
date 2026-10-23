//! The page interface (`wit/view.wit`) and the two crossings it asks for: what a module hands
//! out is read into the engine's own types — checked, because the module is not trusted — and
//! what the engine tells a module is put into the interface's.

use std::sync::Arc;

use bt_view::wire::{
    self, Direction, Display, Items, Justify, Kind, Length, Meta, Node, Overflow, Spark, Style,
    TextStyle,
};
use bt_view::{
    Align, Border, Button, Color, Cursor, Element, Event, Extent, Family, Id, Input, Key, Keyboard,
    Laid, Menu, MenuEntry, Mods, Point, Rect, Reply, Shadow, Size, Tone, Track,
};
use wasmtime::Store;
use wasmtime::component::{Component, HasSelf, Linker};

use crate::module::{Frame, Runner};
use crate::sandbox::{self, Fault, Host, Limits};

wasmtime::component::bindgen!({
    path: "wit",
    world: "view",
});

use bateri::view::types as wit;

// --- What the module asks of the layout ----------------------------------------------------

impl bateri::view::types::Host for Host {}

impl bateri::view::program::Host for Host {
    fn send(&mut self, message: String) {
        let (messages, bytes) = self.allowed;
        if self.sent.len() >= messages || message.len() > bytes {
            // Not kept, and the application closes the module when the call is over: a call
            // cannot fill the program's input however it is written.
            self.overflowed = true;
            return;
        }
        self.sent.push(message);
    }
}

fn rect_of(rect: Rect) -> wit::Rect {
    wit::Rect {
        x: rect.x,
        y: rect.y,
        w: rect.w,
        h: rect.h,
    }
}

impl Host {
    /// The layout the module asks of, if the application has laid anything out yet.
    fn laid(&self) -> Option<&Laid> {
        self.laid.as_deref()
    }
}

impl bateri::view::layout::Host for Host {
    fn hit(&mut self, at: wit::Point) -> Option<u64> {
        let laid = self.laid()?;
        laid.hit(Point::new(at.x, at.y)).map(id_out)
    }

    fn ids_at(&mut self, at: wit::Point) -> Vec<u64> {
        self.laid().map_or_else(Vec::new, |laid| {
            laid.ids_at(Point::new(at.x, at.y))
                .into_iter()
                .map(id_out)
                .collect()
        })
    }

    fn bounds(&mut self, id: u64) -> Option<wit::Rect> {
        self.laid()?.bounds(Id::new(id)).map(rect_of)
    }

    fn scroll_extent(&mut self, id: u64) -> Option<wit::Extent> {
        let Extent { content, view } = self.laid()?.scroll_extent(Id::new(id))?;
        Some(wit::Extent { content, view })
    }

    fn focus_order(&mut self) -> Vec<u64> {
        self.laid().map_or_else(Vec::new, |laid| {
            laid.focus_order().into_iter().map(id_out).collect()
        })
    }

    fn path_of(&mut self, id: u64) -> Vec<u64> {
        self.laid().map_or_else(Vec::new, |laid| {
            laid.path_of(Id::new(id)).into_iter().map(id_out).collect()
        })
    }

    fn text_field(&mut self) -> Option<wit::Rect> {
        self.laid()?.text_field().map(rect_of)
    }
}

fn id_out(id: Id) -> u64 {
    id.number()
}

// --- What the engine tells the module -----------------------------------------------------

fn point_in(at: Point) -> wit::Point {
    wit::Point { x: at.x, y: at.y }
}

fn button_in(button: Button) -> wit::Button {
    match button {
        Button::Left => wit::Button::Left,
        Button::Right => wit::Button::Right,
    }
}

fn mods_in(mods: Mods) -> wit::Mods {
    wit::Mods {
        shift: mods.shift,
        control: mods.control,
        alt: mods.option,
        command: mods.command,
    }
}

fn key_in(key: Key) -> wit::Key {
    match key {
        Key::Up => wit::Key::Up,
        Key::Down => wit::Key::Down,
        Key::Left => wit::Key::Left,
        Key::Right => wit::Key::Right,
        Key::PageUp => wit::Key::PageUp,
        Key::PageDown => wit::Key::PageDown,
        Key::Home => wit::Key::Home,
        Key::End => wit::Key::End,
        Key::Enter => wit::Key::Enter,
        Key::Escape => wit::Key::Escape,
        Key::Tab => wit::Key::Tab,
        Key::Char(c) => wit::Key::Character(c),
    }
}

fn event_in(event: Event) -> wit::Event {
    match event {
        Event::Input(Input::Move(at)) => wit::Event::PointerMove(point_in(at)),
        Event::Input(Input::Leave) => wit::Event::PointerLeave,
        Event::Input(Input::Press { at, button, mods }) => wit::Event::Press(wit::Press {
            at: point_in(at),
            button: button_in(button),
            mods: mods_in(mods),
        }),
        Event::Input(Input::Release { at, button }) => wit::Event::Release(wit::Release {
            at: point_in(at),
            button: button_in(button),
        }),
        Event::Input(Input::Scroll { at, dy }) => wit::Event::Scroll(wit::Scroll {
            at: point_in(at),
            dy,
        }),
        Event::Input(Input::Key { key, mods }) => wit::Event::Key(wit::KeyPress {
            key: key_in(key),
            mods: mods_in(mods),
        }),
        Event::Text(text) => wit::Event::Text(text),
        Event::MenuPick(index) => wit::Event::MenuPick(u32::try_from(index).unwrap_or(u32::MAX)),
    }
}

// --- What the module hands out ------------------------------------------------------------

/// A coordinate or a length from the module: finite and within what a page can mean. A page
/// that says NaN or a billion points is not trusted to lay anything out with it.
fn pt(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(-1.0e6, 1.0e6)
    } else {
        0.0
    }
}

fn opt_pt(value: Option<f32>) -> Option<f32> {
    value.map(pt)
}

fn share(value: f32) -> f32 {
    pt(value).max(0.0)
}

fn tone_out(tone: &wit::Tone) -> Tone {
    match tone {
        wit::Tone::Background => Tone::Background,
        wit::Tone::Foreground => Tone::Foreground,
        wit::Tone::Dim => Tone::Dim,
        wit::Tone::Accent => Tone::Accent,
        wit::Tone::Cursor => Tone::Cursor,
        wit::Tone::Success => Tone::Success,
        wit::Tone::Error => Tone::Error,
        wit::Tone::Info => Tone::Info,
        wit::Tone::Warning => Tone::Warning,
        wit::Tone::Raised(steps) => Tone::Raised(*steps),
        wit::Tone::Quiet => Tone::Quiet,
        wit::Tone::Line => Tone::Line,
        wit::Tone::Shadow => Tone::Shadow,
    }
}

fn color_out(color: &wit::Color) -> Color {
    let alpha = if color.alpha.is_finite() {
        color.alpha.clamp(0.0, 1.0)
    } else {
        1.0
    };
    tone_out(&color.tone).alpha(alpha)
}

fn length_out(length: &wit::Length) -> Length {
    match length {
        wit::Length::Auto => Length::Auto,
        wit::Length::Px(points) => Length::Px(pt(*points)),
        wit::Length::Percent(percent) => Length::Percent(pt(*percent).clamp(0.0, 100.0)),
    }
}

fn track_out(track: &wit::Track) -> Track {
    match track {
        wit::Track::Px(points) => Track::Px(share(*points)),
        wit::Track::Fr(fr) => Track::Fr(share(*fr)),
        wit::Track::Range(wit::Range { min, fr }) => Track::Range {
            min: share(*min),
            fr: share(*fr),
        },
        wit::Track::Auto => Track::Auto,
    }
}

fn tracks_out(tracks: &[wit::Track], limits: &Limits) -> Result<Vec<Track>, Fault> {
    if tracks.len() > limits.tracks {
        return Err(Fault::Malformed(format!(
            "a grid with {} tracks (at most {})",
            tracks.len(),
            limits.tracks
        )));
    }
    Ok(tracks.iter().map(track_out).collect())
}

fn edges_out(edges: &wit::Edges) -> [f32; 4] {
    [
        pt(edges.top),
        pt(edges.right),
        pt(edges.bottom),
        pt(edges.left),
    ]
}

fn family_out(family: wit::Family) -> Family {
    match family {
        wit::Family::System => Family::System,
        wit::Family::Terminal => Family::Terminal,
    }
}

fn align_out(align: wit::Align) -> Align {
    match align {
        wit::Align::Start => Align::Start,
        wit::Align::Center => Align::Center,
        wit::Align::End => Align::End,
    }
}

fn text_style_out(text: &wit::TextStyle) -> TextStyle {
    TextStyle {
        family: text.family.map(family_out),
        // A line of text set in a size that is not one is not drawn at all.
        size: text.size.map(|size| pt(size).clamp(1.0, 512.0)),
        weight: text.weight,
        color: text.color.as_ref().map(color_out),
        align: text.align.map(align_out),
    }
}

fn style_out(style: &wit::Style, limits: &Limits) -> Result<Style, Fault> {
    Ok(Style {
        display: match style.display {
            wit::Display::Flex => Display::Flex,
            wit::Display::Grid => Display::Grid,
            wit::Display::None => Display::None,
        },
        direction: match style.direction {
            wit::Direction::Row => Direction::Row,
            wit::Direction::Column => Direction::Column,
        },
        wrap: style.wrap,
        absolute: style.absolute,
        inset: [
            opt_pt(style.inset.top),
            opt_pt(style.inset.right),
            opt_pt(style.inset.bottom),
            opt_pt(style.inset.left),
        ],
        width: length_out(&style.width),
        height: length_out(&style.height),
        min_width: length_out(&style.min_width),
        min_height: length_out(&style.min_height),
        max_width: length_out(&style.max_width),
        max_height: length_out(&style.max_height),
        padding: edges_out(&style.padding),
        margin: edges_out(&style.margin),
        gap: (pt(style.gap_x), pt(style.gap_y)),
        grow: share(style.grow),
        shrink: style.shrink.map(share),
        items: style.items.map(|items| match items {
            wit::Items::Start => Items::Start,
            wit::Items::Center => Items::Center,
            wit::Items::End => Items::End,
            wit::Items::Stretch => Items::Stretch,
        }),
        justify: style.justify.map(|justify| match justify {
            wit::Justify::Start => Justify::Start,
            wit::Justify::Center => Justify::Center,
            wit::Justify::End => Justify::End,
            wit::Justify::Between => Justify::Between,
        }),
        columns: tracks_out(&style.columns, limits)?,
        rows: tracks_out(&style.rows, limits)?,
        column_span: style.column_span,
        overflow: match style.overflow {
            wit::Overflow::Visible => Overflow::Visible,
            wit::Overflow::Hidden => Overflow::Hidden,
            wit::Overflow::Scroll => Overflow::Scroll,
        },
        scroll: (pt(style.scroll_x), pt(style.scroll_y)),
        fill: style.fill.as_ref().map(color_out),
        border: style.border.as_ref().map(|border| Border {
            width: share(border.width),
            color: color_out(&border.color),
        }),
        radii: [
            share(style.radii.top_left),
            share(style.radii.top_right),
            share(style.radii.bottom_right),
            share(style.radii.bottom_left),
        ],
        shadow: style.shadow.as_ref().map(|shadow| Shadow {
            dx: pt(shadow.dx),
            dy: pt(shadow.dy),
            blur: share(shadow.blur),
            color: color_out(&shadow.color),
        }),
        text: text_style_out(&style.text),
        truncate: style.truncate,
    })
}

fn kind_out(content: wit::Content, limits: &Limits) -> Result<Kind, Fault> {
    Ok(match content {
        wit::Content::Container => Kind::Box,
        wit::Content::Text(text) => {
            if text.len() > limits.text {
                return Err(Fault::Malformed(format!(
                    "a line of text of {} bytes (at most {})",
                    text.len(),
                    limits.text
                )));
            }
            Kind::Text(text)
        }
        wit::Content::Spark(spark) => {
            if spark.values.len() > limits.values {
                return Err(Fault::Malformed(format!(
                    "a sparkline of {} values (at most {})",
                    spark.values.len(),
                    limits.values
                )));
            }
            Kind::Spark(Spark {
                values: spark.values.into_iter().map(pt).collect(),
                color: color_out(&spark.color),
                peak: spark.peak.map(pt),
            })
        }
    })
}

fn node_out(node: wit::Node, limits: &Limits) -> Result<Node, Fault> {
    Ok(Node {
        style: style_out(&node.style, limits)?,
        kind: kind_out(node.content, limits)?,
        meta: Meta {
            id: node.id.map(Id::new),
            pressable: node.pressable,
            focusable: node.focusable,
            text_field: node.text_field,
        },
        children: node.children,
    })
}

/// The tree a module handed out, if it is one within the limits.
fn frame_out(frame: wit::Frame, limits: &Limits) -> Result<Frame, Fault> {
    if frame.nodes.len() > limits.nodes {
        return Err(Fault::Malformed(format!(
            "a page of {} elements (at most {})",
            frame.nodes.len(),
            limits.nodes
        )));
    }
    let nodes = frame
        .nodes
        .into_iter()
        .map(|node| node_out(node, limits))
        .collect::<Result<Vec<_>, _>>()?;
    let root = Element::unfold(nodes, limits.depth)
        .map_err(|why: wire::Malformed| Fault::Malformed(why.to_string()))?;
    Ok(Frame {
        root,
        animating: frame.animating,
    })
}

fn reply_out(reply: wit::Reply, limits: &Limits) -> Result<Reply, Fault> {
    let text = |text: String| {
        if text.len() > limits.text.saturating_mul(16) {
            Err(Fault::Malformed(format!(
                "a text of {} bytes in a reply",
                text.len()
            )))
        } else {
            Ok(text)
        }
    };
    let menu = match reply.menu {
        None => None,
        Some(menu) => {
            if menu.entries.len() > limits.menu {
                return Err(Fault::Malformed(format!(
                    "a menu of {} entries (at most {})",
                    menu.entries.len(),
                    limits.menu
                )));
            }
            let mut entries = Vec::with_capacity(menu.entries.len());
            for entry in menu.entries {
                entries.push(match entry {
                    wit::MenuEntry::Separator => MenuEntry::Separator,
                    wit::MenuEntry::Item(item) => {
                        if item.title.len() > limits.text {
                            return Err(Fault::Malformed("a menu title that long".into()));
                        }
                        MenuEntry::Item {
                            title: item.title,
                            key: item.key,
                            enabled: item.enabled,
                        }
                    }
                });
            }
            Some(Menu {
                at: Point::new(pt(menu.at.x), pt(menu.at.y)),
                entries,
            })
        }
    };
    Ok(Reply {
        redraw: reply.redraw,
        close: reply.close,
        menu,
        keyboard: reply.keyboard.map(|keyboard| match keyboard {
            wit::Keyboard::Page => Keyboard::Page,
            wit::Keyboard::Field => Keyboard::Filter,
        }),
        copy: reply.copy.map(text).transpose()?,
        cursor: match reply.cursor {
            wit::Cursor::Arrow => Cursor::Arrow,
            wit::Cursor::Pointer => Cursor::Pointer,
            wit::Cursor::Text => Cursor::Text,
        },
    })
}

// --- The module --------------------------------------------------------------------------

/// A module of the page interface that is running: its store, and the budget its calls have.
pub(crate) struct World {
    store: Store<Host>,
    view: View,
    limits: Limits,
}

impl World {
    /// Compiles `bytes` (a component, binary or text) and starts it. The compiling is slow —
    /// the caller is on a thread of its own.
    pub(crate) fn load(bytes: &[u8], limits: Limits) -> Result<Self, Fault> {
        if bytes.len() > limits.component {
            return Err(Fault::Load(format!(
                "the component is {} bytes; the most that is run is {}",
                bytes.len(),
                limits.component
            )));
        }
        let engine = sandbox::engine()?;
        let component =
            Component::new(&engine, bytes).map_err(|why| Fault::Load(format!("{why:#}")))?;
        let mut linker = Linker::<Host>::new(&engine);
        View::add_to_linker::<Host, HasSelf<Host>>(&mut linker, |host| host)
            .map_err(|why| Fault::Load(format!("{why:#}")))?;
        let mut store = sandbox::store(&engine, &limits);
        // The module's own start runs under the start budget; what it does not offer to the
        // page is found out here (an import nobody gave).
        let view = sandbox::call(&mut store, limits.start, |store| {
            View::instantiate(store, &component, &linker)
        })
        .map_err(|fault| match fault {
            Fault::Trap(why) => Fault::Load(why),
            other => other,
        })?;
        Ok(Self {
            store,
            view,
            limits,
        })
    }
}

impl World {
    /// A call that sent more than a call may is the end of the module: what it sent is dropped.
    fn within_what_a_call_may_send(&mut self) -> Result<(), Fault> {
        let host = self.store.data_mut();
        if std::mem::take(&mut host.overflowed) {
            host.sent.clear();
            return Err(Fault::Flooded(format!(
                "at most {} messages of {} bytes in one call",
                self.limits.messages, self.limits.message
            )));
        }
        Ok(())
    }
}

impl Runner for World {
    fn render(&mut self, size: Size, now: f64, reduce_motion: bool) -> Result<Frame, Fault> {
        let page = self.view.bateri_view_page();
        let size = wit::Size {
            w: size.w,
            h: size.h,
        };
        let frame = sandbox::call(&mut self.store, self.limits.call, |store| {
            page.call_render(store, size, now, reduce_motion)
        })?;
        self.within_what_a_call_may_send()?;
        frame_out(frame, &self.limits)
    }

    fn settle(&mut self) -> Result<bool, Fault> {
        let page = self.view.bateri_view_page();
        let changed = sandbox::call(&mut self.store, self.limits.call, |store| {
            page.call_settle(store)
        })?;
        self.within_what_a_call_may_send()?;
        Ok(changed)
    }

    fn handle(&mut self, event: Event) -> Result<Reply, Fault> {
        let page = self.view.bateri_view_page();
        let event = event_in(event);
        let reply = sandbox::call(&mut self.store, self.limits.call, |store| {
            page.call_handle(store, &event)
        })?;
        self.within_what_a_call_may_send()?;
        reply_out(reply, &self.limits)
    }

    fn message(&mut self, text: String) -> Result<Reply, Fault> {
        let page = self.view.bateri_view_page();
        let reply = sandbox::call(&mut self.store, self.limits.call, |store| {
            page.call_message(store, &text)
        })?;
        self.within_what_a_call_may_send()?;
        reply_out(reply, &self.limits)
    }

    fn take_sent(&mut self) -> Vec<String> {
        std::mem::take(&mut self.store.data_mut().sent)
    }

    fn lay_out(&mut self, laid: Arc<Laid>) {
        self.store.data_mut().laid = Some(laid);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edges() -> wit::Edges {
        wit::Edges {
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
            left: 0.0,
        }
    }

    /// A style that sets nothing.
    fn style() -> wit::Style {
        wit::Style {
            display: wit::Display::Flex,
            direction: wit::Direction::Column,
            wrap: false,
            absolute: false,
            inset: wit::Inset {
                top: None,
                right: None,
                bottom: None,
                left: None,
            },
            width: wit::Length::Auto,
            height: wit::Length::Auto,
            min_width: wit::Length::Auto,
            min_height: wit::Length::Auto,
            max_width: wit::Length::Auto,
            max_height: wit::Length::Auto,
            padding: edges(),
            margin: edges(),
            gap_x: 0.0,
            gap_y: 0.0,
            grow: 0.0,
            shrink: None,
            items: None,
            justify: None,
            columns: Vec::new(),
            rows: Vec::new(),
            column_span: 0,
            overflow: wit::Overflow::Visible,
            scroll_x: 0.0,
            scroll_y: 0.0,
            fill: None,
            border: None,
            radii: wit::Corners {
                top_left: 0.0,
                top_right: 0.0,
                bottom_right: 0.0,
                bottom_left: 0.0,
            },
            shadow: None,
            text: wit::TextStyle {
                family: None,
                size: None,
                weight: None,
                color: None,
                align: None,
            },
            truncate: false,
        }
    }

    fn node(content: wit::Content, children: u32) -> wit::Node {
        wit::Node {
            content,
            style: style(),
            id: None,
            pressable: false,
            focusable: false,
            text_field: false,
            children,
        }
    }

    fn frame(nodes: Vec<wit::Node>) -> wit::Frame {
        wit::Frame {
            nodes,
            animating: false,
        }
    }

    fn container(children: u32) -> wit::Node {
        node(wit::Content::Container, children)
    }

    #[test]
    fn a_page_within_the_limits_is_the_tree_it_says() {
        let mut row = container(1);
        row.id = Some(7);
        row.pressable = true;
        row.style.width = wit::Length::Px(120.0);
        row.style.fill = Some(wit::Color {
            tone: wit::Tone::Raised(2),
            alpha: 0.5,
        });
        row.style.columns = vec![
            wit::Track::Fr(1.0),
            wit::Track::Range(wit::Range { min: 10.0, fr: 2.0 }),
        ];
        let mut page = frame(vec![row, node(wit::Content::Text("api".into()), 0)]);
        page.animating = true;
        let made = frame_out(page, &Limits::default()).expect("a page");
        assert!(made.animating);
        let nodes = made.root.fold();
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].meta.id, Some(Id::new(7)));
        assert!(nodes[0].meta.pressable);
        assert_eq!(nodes[0].style.width, Length::Px(120.0));
        assert_eq!(nodes[0].style.fill, Some(Tone::Raised(2).alpha(0.5)));
        assert_eq!(
            nodes[0].style.columns,
            vec![Track::Fr(1.0), Track::Range { min: 10.0, fr: 2.0 }]
        );
        assert_eq!(nodes[1].kind, Kind::Text("api".into()));
    }

    #[test]
    fn numbers_that_are_not_numbers_a_page_can_mean_are_made_ones_it_can() {
        let mut odd = container(0);
        odd.style.width = wit::Length::Px(f32::NAN);
        odd.style.height = wit::Length::Percent(1.0e9);
        odd.style.padding.top = f32::INFINITY;
        odd.style.margin.left = -1.0e30;
        odd.style.grow = f32::NEG_INFINITY;
        odd.style.text.size = Some(f32::NAN);
        odd.style.fill = Some(wit::Color {
            tone: wit::Tone::Accent,
            alpha: f32::NAN,
        });
        odd.style.shadow = Some(wit::Shadow {
            dx: f32::NAN,
            dy: 1.0e30,
            blur: -5.0,
            color: wit::Color {
                tone: wit::Tone::Shadow,
                alpha: 7.0,
            },
        });
        let made = frame_out(frame(vec![odd]), &Limits::default()).expect("a page");
        let style = &made.root.fold()[0].style;
        assert_eq!(style.width, Length::Px(0.0));
        assert_eq!(style.height, Length::Percent(100.0));
        assert_eq!(style.padding[0], 0.0);
        assert_eq!(style.margin[3], -1.0e6);
        assert_eq!(style.grow, 0.0);
        assert_eq!(style.text.size, Some(1.0));
        assert_eq!(style.fill, Some(Tone::Accent.alpha(1.0)));
        let shadow = style.shadow.expect("a shadow");
        assert_eq!((shadow.dx, shadow.dy, shadow.blur), (0.0, 1.0e6, 0.0));
        assert_eq!(shadow.color, Tone::Shadow.alpha(1.0));
    }

    #[test]
    fn a_page_past_any_limit_is_not_a_page() {
        let limits = Limits {
            nodes: 3,
            depth: 3,
            text: 8,
            tracks: 2,
            values: 2,
            ..Limits::default()
        };
        let refused =
            |page: wit::Frame| matches!(frame_out(page, &limits), Err(Fault::Malformed(_)));
        // Four elements where three are allowed.
        assert!(refused(frame(vec![
            container(3),
            container(0),
            container(0),
            container(0)
        ])));
        // Three deep is allowed and a fourth level is not (a chain of four, with room for them).
        let roomy = Limits {
            nodes: 10,
            ..limits.clone()
        };
        let chain = |n: u32| {
            (0..n)
                .map(|i| container(u32::from(i + 1 < n)))
                .collect::<Vec<_>>()
        };
        assert!(frame_out(frame(chain(3)), &roomy).is_ok());
        assert!(matches!(
            frame_out(frame(chain(4)), &roomy),
            Err(Fault::Malformed(_))
        ));
        // A line of text that long; a grid with that many tracks; a sparkline with that many values.
        assert!(refused(frame(vec![node(
            wit::Content::Text("123456789".into()),
            0
        )])));
        let mut grid = container(0);
        grid.style.columns = vec![wit::Track::Auto; 3];
        assert!(refused(frame(vec![grid])));
        let line = node(
            wit::Content::Spark(wit::Spark {
                values: vec![1.0; 3],
                color: wit::Color {
                    tone: wit::Tone::Accent,
                    alpha: 1.0,
                },
                peak: None,
            }),
            0,
        );
        assert!(refused(frame(vec![line])));
        // Children it has not got, children beyond the root, and nothing at all.
        assert!(refused(frame(vec![container(2), container(0)])));
        assert!(refused(frame(vec![container(0), container(0)])));
        assert!(refused(frame(Vec::new())));
    }

    #[test]
    fn a_reply_past_a_limit_is_refused_and_one_within_them_is_read() {
        let limits = Limits {
            menu: 2,
            text: 4,
            ..Limits::default()
        };
        let reply = |menu: Option<wit::Menu>, copy: Option<String>| wit::Reply {
            redraw: true,
            close: false,
            menu,
            keyboard: Some(wit::Keyboard::Field),
            copy,
            cursor: wit::Cursor::Pointer,
        };
        let entry = |title: &str| {
            wit::MenuEntry::Item(wit::MenuItem {
                title: title.into(),
                key: Some('x'),
                enabled: true,
            })
        };
        let menu = |entries| wit::Menu {
            at: wit::Point {
                x: 1.0,
                y: f32::NAN,
            },
            entries,
        };
        let read = reply_out(
            reply(
                Some(menu(vec![entry("go"), wit::MenuEntry::Separator])),
                Some("id".into()),
            ),
            &limits,
        )
        .expect("a reply");
        assert_eq!(read.keyboard, Some(Keyboard::Filter));
        assert_eq!(read.cursor, Cursor::Pointer);
        assert_eq!(read.copy.as_deref(), Some("id"));
        let at = read.menu.expect("a menu").at;
        assert_eq!((at.x, at.y), (1.0, 0.0));
        assert!(
            reply_out(
                reply(Some(menu(vec![entry("a"), entry("b"), entry("c")])), None),
                &limits
            )
            .is_err()
        );
        assert!(reply_out(reply(Some(menu(vec![entry("too long")])), None), &limits).is_err());
        assert!(reply_out(reply(None, Some("x".repeat(65))), &limits).is_err());
    }

    #[test]
    fn what_the_person_did_reaches_the_module_as_what_it_was() {
        let at = Point::new(3.0, 4.0);
        assert!(matches!(
            event_in(Event::Input(Input::Move(at))),
            wit::Event::PointerMove(wit::Point { x, y }) if (x, y) == (3.0, 4.0)
        ));
        assert!(matches!(
            event_in(Event::Input(Input::Press {
                at,
                button: Button::Right,
                mods: Mods {
                    shift: true,
                    control: false,
                    option: true,
                    command: false
                },
            })),
            wit::Event::Press(wit::Press {
                button: wit::Button::Right,
                mods: wit::Mods {
                    shift: true,
                    alt: true,
                    control: false,
                    command: false
                },
                ..
            })
        ));
        assert!(matches!(
            event_in(Event::Input(Input::Key {
                key: Key::Char('ç'),
                mods: Mods::default()
            })),
            wit::Event::Key(wit::KeyPress {
                key: wit::Key::Character('ç'),
                ..
            })
        ));
        assert!(matches!(
            event_in(Event::Text("api".into())),
            wit::Event::Text(text) if text == "api"
        ));
        assert!(matches!(
            event_in(Event::MenuPick(usize::MAX)),
            wit::Event::MenuPick(u32::MAX)
        ));
    }

    #[test]
    fn what_a_module_says_to_its_program_is_kept_within_what_a_call_may_send() {
        use bateri::view::program::Host as _;
        let limits = Limits {
            messages: 2,
            message: 10,
            ..Limits::default()
        };
        let mut host = Host::new(&limits);
        host.send("{}".into());
        host.send(r#"{"a":1}"#.into());
        assert_eq!(host.sent, ["{}", r#"{"a":1}"#]);
        assert!(!host.overflowed);
        // A third in one call, or one that is too long: not kept, and the call is past its limit.
        host.send("{}".into());
        assert!(host.overflowed);
        assert_eq!(host.sent.len(), 2);
        let mut host = Host::new(&limits);
        host.send(r#"{"too":"long"}"#.into());
        assert!(host.overflowed);
        assert!(host.sent.is_empty());
    }
}
