//! The page inspector's pane: the part of it that is the pane's and not the page's.
//!
//! The inspector is a page of the kit that bateri draws over a pane of its own ([`Tool`]) and
//! that shows the page of another pane. What the page asks it of its program, which is nobody,
//! is answered in this process (`bt_shell_common::native_view::Responder`); what this module
//! keeps is the two things that cannot be told from either end: which pane an inspector looks at
//! and which inspectors look at a pane, so that the pane's page coming and going, and the pane
//! closing, reach the inspectors that look at it.
//!
//! **The inspector is bound to a pane, not to a page** (the pane's page may be gone for a while
//! and come back): it says so while there is none and reads the page again when there is.

use std::cell::RefCell;

use bt_shell_common::native_view::{Responder, Subject, Target};
use bt_shell_common::view_strip::Strip;
use objc2::MainThreadMarker;

use crate::embed;
use crate::pencere_view::{Exit, Words};

thread_local! {
    /// Each inspector's pane and the pane it looks at, as `(inspector, target)`: the panes live on
    /// the main thread and so does this.
    static WATCHING: RefCell<Vec<(u64, u64)>> = const { RefCell::new(Vec::new()) };
}

/// The inspector `inspector` looks at the pane `target` from now on.
pub(crate) fn watch(inspector: u64, target: u64) {
    WATCHING.with(|watching| {
        let mut watching = watching.borrow_mut();
        watching.retain(|(known, _)| *known != inspector);
        watching.push((inspector, target));
    });
}

/// The inspector `inspector` is no more.
pub(crate) fn unwatch(inspector: u64) {
    WATCHING.with(|watching| {
        watching
            .borrow_mut()
            .retain(|(known, _)| *known != inspector)
    });
}

/// The inspectors that look at `target`, the oldest first.
pub(crate) fn inspectors_of(target: u64) -> Vec<u64> {
    WATCHING.with(|watching| {
        watching
            .borrow()
            .iter()
            .filter(|(_, looks_at)| *looks_at == target)
            .map(|(inspector, _)| *inspector)
            .collect()
    })
}

/// What the pane `target` shows changed (its page came up or went down): the inspectors that
/// look at it read it again.
pub(crate) fn target_changed(mtm: MainThreadMarker, target: u64) {
    for inspector in inspectors_of(target) {
        if let Some(pane) = embed::pane(mtm, inspector) {
            pane.refresh_inspector();
        }
    }
}

/// The pane `target` is closing: the inspectors that look at it go with it. The tab that closes
/// the pane closes the ones in its own tab first, before the pane leaves its place (a card that
/// stood beside the last pane of the flat area would take the area); this is for the ones that
/// are in another tab, and it asks each pane's owner for the close after the closing that is
/// going on.
pub(crate) fn target_closing(mtm: MainThreadMarker, target: u64) {
    for inspector in inspectors_of(target) {
        if let Some(pane) = embed::pane(mtm, inspector) {
            pane.end_tool();
        }
    }
}

/// What the header over the inspector says of it.
pub(crate) fn words() -> Words {
    let strip = Strip {
        place: None,
        program: Some("bateri".to_owned()),
        title: Some("Page inspector".to_owned()),
        quit_tip: "Closes the inspector.".to_owned(),
    };
    Words::new(strip, &[], Exit::Close)
}

/// What an inspector keeps while its page is up: the pane it looks at and what answers its page.
pub(crate) struct Inspecting {
    pub(crate) responder: Responder,
}

impl Inspecting {
    /// The inspector of `target`: its page's questions are answered by what the pane `target`
    /// shows, read when they are asked.
    pub(crate) fn new(mtm: MainThreadMarker, target: u64) -> Self {
        Self {
            responder: Responder::new(std::rc::Rc::new(Watched { mtm, id: target })),
        }
    }
}

/// The pane the inspector looks at, read each time the responder is asked.
struct Watched {
    mtm: MainThreadMarker,
    id: u64,
}

impl Subject for Watched {
    fn target(&self) -> Target {
        match embed::pane(self.mtm, self.id) {
            Some(pane) => Target {
                name: pane
                    .session()
                    .map_or_else(|| "the pane".to_owned(), |session| session.title()),
                page: pane.page_handle().is_some(),
                gone: false,
            },
            None => Target {
                name: String::new(),
                page: false,
                gone: true,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_inspector_is_found_by_the_pane_it_looks_at_until_it_is_no_more() {
        watch(9001, 5);
        watch(9002, 5);
        watch(9003, 6);
        assert_eq!(inspectors_of(5), [9001, 9002]);
        assert_eq!(inspectors_of(6), [9003]);
        // Looking at another pane replaces what it looked at.
        watch(9001, 6);
        assert_eq!(inspectors_of(5), [9002]);
        assert_eq!(inspectors_of(6), [9003, 9001]);
        for inspector in [9001, 9002, 9003] {
            unwatch(inspector);
        }
        assert!(inspectors_of(5).is_empty() && inspectors_of(6).is_empty());
    }
}
