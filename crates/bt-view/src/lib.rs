//! `bt-view`: the engine of the UI kit's pages — a tree of boxes and lines of text, laid out
//! as flexbox and grid, that comes out as a draw list.
//!
//! A page is written as a function from the size it is given to an [`Element`] tree, in the
//! chain of a style sheet's properties (`div().row().gap(8.0).child(...)`), in **points**,
//! with colours named for the theme ([`Tone`]). [`layout`] puts the tree on a page of pixels —
//! asking the host, through [`Measure`], how large each line of text is — and
//! [`Laid::paint`] turns it into a [`DrawList`] of boxes and lines of text.
//!
//! The engine draws nothing and links to nothing of the terminal: not its fonts (the host
//! measures what it also draws), not its colours (the host resolves the names in the pane's
//! theme), not its renderer (the host turns the list into what it draws). It depends only on
//! the layout crate and stands apart from the rest of the workspace, so that what a page is
//! written in can be taken, as it is, to wherever a page is shown.
//!
//! A page is not only drawn: it answers the pointer, the wheel and the keyboard. Elements are
//! named ([`Id`]) and the ones that answer the pointer say so ([`Element::press`]); what a page
//! keeps between builds — what is hovered, pressed, focused, how far a list is scrolled, how
//! far a highlight has faded — is a [`Ui`] kept by those names; a long list builds only the
//! rows in view ([`Rows`]). What the host tells a page is an [`Input`], and what the page
//! tells back a [`Reply`].
//!
//! A page need not live in the host that shows it: [`wire`] lays an [`Element`] tree down as a flat
//! list and picks it up again, and [`Queries`] is what a page asks of itself laid out — the pair a
//! page kept in a sandbox is written against, as [`Laid`] answers the same questions to one that
//! is not.
//!
//! [`demo`] is a worked page — a container list — that is also the engine's gauge.

pub mod demo;

mod color;
mod element;
mod fades;
mod geometry;
mod id;
mod input;
mod layout;
mod paint;
mod rows;
mod style;
mod text;
mod ui;
pub mod wire;

pub use color::{Color, Tone};
pub use element::{Element, div, spacer, spark, text};
pub use fades::{FADE_SECONDS, Fades};
pub use geometry::{Point, Rect, Size};
pub use id::Id;
pub use input::{Button, Cursor, Event, Input, Key, Keyboard, Menu, MenuEntry, Mods, Reply};
pub use layout::{Extent, Laid, Queries, layout};
pub use paint::{DrawList, Paint, Quad, Run};
pub use rows::Rows;
pub use style::{Border, Shadow, Track};
pub use text::{Align, Family, Font, Measure, Metrics};
pub use ui::Ui;

#[cfg(test)]
mod interact_tests;
#[cfg(test)]
mod tests;
