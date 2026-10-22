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
//! [`demo`] is a worked page — a container list — that is also the engine's gauge.

pub mod demo;

mod color;
mod element;
mod geometry;
mod layout;
mod paint;
mod style;
mod text;

pub use color::{Color, Tone};
pub use element::{Element, div, spacer, spark, text};
pub use geometry::{Point, Rect, Size};
pub use layout::{Laid, layout};
pub use paint::{DrawList, Paint, Quad, Run};
pub use style::{Border, Shadow, Track};
pub use text::{Align, Family, Font, Measure, Metrics};

#[cfg(test)]
mod tests;
