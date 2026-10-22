//! The application's strip over a page of the UI kit: the strip a program's view has
//! (`pencere_view::strip`), saying whose page it is and giving the one way out — its button
//! closes the page here, where over a program's view it writes ⌃C to the program.
//!
//! The page itself is `bt-shell-common`'s (`native_view`): the engine's draw list on the
//! renderer, painted in place of the grid by the pane's link. What the pane keeps for it is
//! the strip's place at the top and the room it takes (`view_strip::HEIGHT`), which the page
//! never draws into.
//!
//! Debug builds only for now (`Debug ▸ Show Container List Demo`).

use std::rc::Rc;

use bt_core::Theme;
use bt_shell_common::view_strip::Strip;
use objc2::rc::Retained;
use objc2::{MainThreadOnly, sel};

use crate::pane::TerminalPane;
use crate::pencere_view::face::Faces;
use crate::pencere_view::strip::StripView;
use crate::pencere_view::strip_area;

/// The demo's strip over `pane`: the made-up program's name and the title of its page, a button
/// that closes the page, painted with `theme`.
pub(crate) fn strip(pane: &TerminalPane, theme: &Theme) -> Retained<StripView> {
    let text = Strip {
        place: None,
        program: Some("dockerpen".to_owned()),
        title: Some("Docker".to_owned()),
        quit_tip: "Closes the demo page. The terminal comes back.".to_owned(),
    };
    let strip = StripView::new(
        pane.mtm(),
        strip_area(pane.bounds()),
        &text,
        Rc::new(Faces::new(&[])),
        theme,
    );
    strip.aim(pane, sel!(closeUiPage:));
    strip
}
