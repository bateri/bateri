//! `bateri-view-sdk`: write a page for bateri's sandbox.
//!
//! A page is a type that says what it looks like for a viewport ([`Page::render`]) and what
//! it does when the person does something ([`Page::handle`]); [`export_page!`] makes the
//! component of it. What a page looks like is written in the style language of the engine that
//! lays it out, which is this crate's own vocabulary (`div().row().gap(8.0).child(...)`, in
//! points, with colours named for the theme): the pages of the terminal itself are written in it,
//! and a page here is written the same way.
//!
//! The component imports **nothing but the page's interface** (`bateri:view`): a page cannot
//! read a file, open a socket, ask the time or print — the application gives it what it needs
//! as an event, and what it can ask of the application is the layout of the page it last
//! drew ([`Layout`]). A panic ends the page, and says why on the way out. A page that a program
//! in the pane opened can also talk to that program: [`say`] sends it a message and
//! [`Page::message`] hears the program's (the messages are
//! `bateri-view-messages`'; the terminal only carries them). What the program can take is the
//! program's: `say` answers whether it has the room, and a page that was told *full* keeps what
//! it meant to say and is called once, in [`Page::room`], when it has. A page that runs forever,
//! takes memory without end or traps is closed by the application, and the terminal underneath
//! comes back.
//!
//! ```ignore
//! use bateri_view_sdk::{Event, Frame, Layout, Page, Reply, Size, div, export_page, text};
//!
//! struct Hello;
//!
//! impl Page for Hello {
//!     fn new() -> Self { Hello }
//!     fn render(&mut self, _size: Size, _now: f64, _reduce_motion: bool) -> Frame {
//!         Frame::still(div().p(16.0).child(text("hello")))
//!     }
//!     fn settle(&mut self, _layout: &Layout) -> bool { false }
//!     fn handle(&mut self, _event: Event, _layout: &Layout) -> Reply { Reply::none() }
//! }
//!
//! export_page!(Hello);
//! ```

#[doc(hidden)]
pub mod bindings {
    wit_bindgen::generate!({
        path: "../../crates/bt-view-wasm/wit",
        world: "view",
        pub_export_macro: true,
        default_bindings_module: "bateri_view_sdk::bindings",
    });
}

mod convert;

use bindings::bateri::view::diagnostics as to_diagnostics;
use bindings::bateri::view::layout as host;
use bindings::bateri::view::program as to_program;

pub use bt_view::wire;
pub use bt_view::{
    Align, Border, Button, Color, Cursor, Element, Event, Extent, Fades, Family, Id, Input, Key,
    Keyboard, Menu, MenuEntry, Mods, Point, Queries, Rect, Reply, Rows, Shadow, Size, Tone, Track,
    Ui, div, spacer, spark, text,
};

#[doc(hidden)]
pub use convert::{from_wit_event, to_wit_frame, to_wit_reply};

/// A page ready to be laid out, and whether it is still moving.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub root: Element,
    /// The page is still moving (a highlight on its way): the application asks for the next frame
    /// itself until this says no, so a page at rest costs no frames.
    pub animating: bool,
    /// When the page at rest wants to be rendered again if nothing happens before then, seconds
    /// on the clock [`Page::render`] is given (`now + 5.0`: in five seconds). It is rendered once
    /// at that time, not at every frame until then: what a page needs to show that something it
    /// waits for has not come (a reading that stopped). Not read while `animating`.
    pub wake_at: Option<f64>,
}

impl Frame {
    /// A page that is not moving.
    pub fn still(root: Element) -> Self {
        Self {
            root,
            animating: false,
            wake_at: None,
        }
    }
}

/// What the page can ask of itself laid out by the application: where things stand, what is under
/// the pointer, how far a box scrolls. The answers are those of the layout the application last
/// made, which can be a frame behind the tree the page has handed out since.
#[derive(Clone, Copy, Debug, Default)]
pub struct Layout;

impl Queries for Layout {
    fn bounds(&self, id: Id) -> Option<Rect> {
        host::bounds(id.number()).map(|r| Rect::new(r.x, r.y, r.w, r.h))
    }

    fn ids_at(&self, at: Point) -> Vec<Id> {
        host::ids_at(point_in(at))
            .into_iter()
            .map(Id::new)
            .collect()
    }

    fn hit(&self, at: Point) -> Option<Id> {
        host::hit(point_in(at)).map(Id::new)
    }

    fn scroll_extent(&self, id: Id) -> Option<Extent> {
        host::scroll_extent(id.number()).map(|e| Extent {
            content: e.content,
            view: e.view,
        })
    }

    fn focus_order(&self) -> Vec<Id> {
        host::focus_order().into_iter().map(Id::new).collect()
    }

    fn path_of(&self, id: Id) -> Vec<Id> {
        host::path_of(id.number())
            .into_iter()
            .map(Id::new)
            .collect()
    }

    fn text_field(&self) -> Option<Rect> {
        host::text_field().map(|r| Rect::new(r.x, r.y, r.w, r.h))
    }
}

/// Says something to the program that opened the page: one JSON object, written as text (a
/// message of `bateri-view-messages`). Returns at once; the program answers in its own time
/// ([`Page::message`]).
///
/// True when the application has the message and carries it to the program. False when the
/// program has not read what it was told before: the message is **not** taken, the page keeps
/// it, and [`Page::room`] is called once when the program has read enough — the page does not
/// try again by itself. A page that says more in one call than the application allows (a number
/// of messages, or one longer than the protocol's limit of 1 MiB) is closed.
#[must_use = "a message the program had no room for is not sent: keep it for Page::room"]
pub fn say(message: &str) -> bool {
    matches!(to_program::send(message), to_program::Delivery::Accepted)
}

fn point_in(at: Point) -> bindings::bateri::view::types::Point {
    bindings::bateri::view::types::Point { x: at.x, y: at.y }
}

/// Makes a panic say why before it ends the page: the application is told the panic's words
/// (what was wrong and where) and not only that the module trapped. Installed by
/// [`export_page!`], before the page is made.
#[doc(hidden)]
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        to_diagnostics::fail(&info.to_string());
    }));
}

/// A page.
pub trait Page: Sized + 'static {
    /// The page as it is when it opens.
    fn new() -> Self;

    /// The page for a viewport of `size` points at `now` seconds (on the application's clock).
    /// `reduce_motion` is the person's setting: a page that animates snaps instead.
    fn render(&mut self, size: Size, now: f64, reduce_motion: bool) -> Frame;

    /// The application laid the page out: bring what depends on the layout (how far a list is
    /// scrolled against how far it can be) back within it. True when something changed and the
    /// page is worth rendering again.
    fn settle(&mut self, layout: &Layout) -> bool;

    /// The person did something (or the application's text field changed, or an entry of a menu
    /// the page asked for was picked).
    fn handle(&mut self, event: Event, layout: &Layout) -> Reply;

    /// The program that opened the page said something: one JSON object, as the program wrote
    /// it. The program is not trusted — it may be a program the person did not mean to start.
    /// The reply's `redraw` says the page changed. A page no program opened is never told.
    fn message(&mut self, _text: &str, _layout: &Layout) -> Reply {
        Reply::none()
    }

    /// The program has read enough for the page to say what [`say`] told it the program had no
    /// room for. Called once for each time the page was told so, and never when it was not.
    /// The reply's `redraw` says the page changed.
    fn room(&mut self, _layout: &Layout) -> Reply {
        Reply::none()
    }
}

/// Makes the component of a page: `export_page!(MyPage);` at the root of the crate (a
/// `cdylib`).
#[macro_export]
macro_rules! export_page {
    ($page:ty) => {
        const _: () = {
            ::std::thread_local! {
                static PAGE: ::std::cell::RefCell<$page> = ::std::cell::RefCell::new({
                    $crate::install_panic_hook();
                    <$page as $crate::Page>::new()
                });
            }

            struct Glue;

            impl $crate::bindings::exports::bateri::view::page::Guest for Glue {
                fn render(
                    size: $crate::bindings::bateri::view::types::Size,
                    now: f64,
                    reduce_motion: bool,
                ) -> $crate::bindings::bateri::view::types::Frame {
                    PAGE.with_borrow_mut(|page| {
                        let size = $crate::Size::new(size.w, size.h);
                        $crate::to_wit_frame(<$page as $crate::Page>::render(
                            page,
                            size,
                            now,
                            reduce_motion,
                        ))
                    })
                }

                fn settle() -> bool {
                    PAGE.with_borrow_mut(|page| {
                        <$page as $crate::Page>::settle(page, &$crate::Layout)
                    })
                }

                fn message(
                    text: ::std::string::String,
                ) -> $crate::bindings::bateri::view::types::Reply {
                    PAGE.with_borrow_mut(|page| {
                        $crate::to_wit_reply(<$page as $crate::Page>::message(
                            page,
                            &text,
                            &$crate::Layout,
                        ))
                    })
                }

                fn room() -> $crate::bindings::bateri::view::types::Reply {
                    PAGE.with_borrow_mut(|page| {
                        $crate::to_wit_reply(<$page as $crate::Page>::room(page, &$crate::Layout))
                    })
                }

                fn handle(
                    event: $crate::bindings::bateri::view::types::Event,
                ) -> $crate::bindings::bateri::view::types::Reply {
                    PAGE.with_borrow_mut(|page| {
                        $crate::to_wit_reply(<$page as $crate::Page>::handle(
                            page,
                            $crate::from_wit_event(event),
                            &$crate::Layout,
                        ))
                    })
                }
            }

    $crate::bindings::export!(Glue with_types_in $crate::bindings);
        };
    };
}
