//! A tab let go **in** the panes of the tab on screen: with ⌥⌘ held, a tab pulled out of
//! its strip is a block of panes, and the panes it is over answer for it with their regions —
//! the same ones a carried pane gets ([`crate::pane_drag`]), drawn for the whole block — and
//! letting go lands the block there ([`TerminalWindow::merge_tab_at`]; in another window,
//! [`AppDelegate::tab_to_other_tab`]). Without the keys the drop is what it always was: a
//! window where the pointer is ([`crate::tab_drag`]).
//!
//! **The session is AppKit's, so this reads it from the outside.** A tab dragged out of the
//! strip is a dragging session whose destinations are the bars; nothing else of ours is asked
//! while it runs and no event reaches a monitor. The source reports every move of the image
//! ([`AppDelegate::tab_merge_read`]) and a short poll covers the keys, which change without a
//! move ([`POLL`]); the last word is read where the session ends
//! ([`AppDelegate::tab_merge_end`]), from the keys then.
//!
//! **Which tab is "the tab on screen"?** A press on a chip selects its tab, so by the time a
//! tab is pulled away it is the one on screen — and a tab cannot land in itself. The tab that
//! was open before the press is the one the block lands in: it comes back on screen when the
//! keys go down over the panes ([`TabBar::selected_before`](crate::tab_bar::TabBar)), and a
//! tab pulled out of the one that was already open has none to land in (open the other first).
//! If the carried tab is taken back (Esc) the selection goes back to it.

use std::time::Duration;

use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadOnly};
use objc2_app_kit::NSEvent;
use objc2_foundation::NSPoint;

use crate::app::AppDelegate;
use crate::arrange;
use crate::pane_drag::{Zones, preview};
use crate::split::Verdict;
use crate::tab::TerminalTab;
use crate::window::{Joins, TerminalWindow};

/// How often the keys are read while a tab is carried: the system reports no move when only a
/// key changes.
const POLL: Duration = Duration::from_millis(60);

/// A tab carried out of its strip, and what its block shows over the panes.
pub(crate) struct Merge {
    /// The carried tab.
    tab: u64,
    /// The regions, drawn in the tab they belong to.
    zones: Option<(u64, Retained<Zones>)>,
    verdict: Verdict,
    /// The tab on screen was changed to the one the block lands in: taken back, the carried
    /// tab is put on screen again.
    opened: bool,
}

/// What the end of a tab's drag session came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum End {
    /// The block landed in the tab on screen.
    Merged,
    /// The regions were up and refused, or the landing did not hold: the tab stays.
    Kept,
    /// Not the block's business: the session's own end — a bar's drop, a window — goes on.
    Free,
}

/// The tab a carried tab's block lands in: the one on screen if it is another (the keys
/// brought it, or the carried tab was never the one on screen), else the one that was open
/// before the press that took the chip (`before`, if it is still in the window). `None`: the
/// tab was already the only one in sight and has none to land in.
fn host_of(
    open: Option<u64>,
    before: Option<u64>,
    carried: u64,
    in_window: impl Fn(u64) -> bool,
) -> Option<u64> {
    match open {
        Some(open) if open != carried => Some(open),
        _ => before.filter(|before| *before != carried && in_window(*before)),
    }
}

/// Whether ⌥⌘ are both down now. Read off the hardware: no flags event is delivered to us
/// while a session runs.
fn arranging() -> bool {
    arrange::swallows(NSEvent::modifierFlags_class())
}

/// Where a carried tab's block would land.
struct Aim {
    /// The window the pointer is over, whose tab on screen (or the one the press left, in the
    /// carried tab's own window) takes the block.
    window: Retained<TerminalWindow>,
    /// The window the tab is carried out of.
    from: Retained<TerminalWindow>,
    carried: Retained<TerminalTab>,
    host: Retained<TerminalTab>,
    /// The pointer, in the window's coordinates.
    point: NSPoint,
}

impl AppDelegate {
    /// A tab has been pulled out of its strip: the regions of its block are read from here on.
    pub(crate) fn tab_merge_begin(&self, tab: u64) {
        let iv = self.ivars();
        if let Some(mut old) = iv.tab_merge.replace(None) {
            hide(&mut old);
        }
        iv.tab_merge.replace(Some(Merge {
            tab,
            zones: None,
            verdict: Verdict::Nothing,
            opened: false,
        }));
        let generation = iv.tab_poll.get().wrapping_add(1);
        iv.tab_poll.set(generation);
        self.tab_merge_poll(generation);
    }

    fn tab_merge_poll(&self, generation: u64) {
        arrange::after(POLL, move |app| {
            if app.ivars().tab_poll.get() != generation {
                return;
            }
            app.tab_merge_read(None);
            app.tab_merge_poll(generation);
        });
    }

    /// The image of the carried tab moved to `screen` (the pointer's place now if `None`, for a
    /// read the poll asks): the block's regions follow. The state is out of its cell while it
    /// works — landing a tab selects another, and whatever that touches must find none.
    pub(crate) fn tab_merge_read(&self, screen: Option<NSPoint>) {
        let screen = screen.unwrap_or_else(NSEvent::mouseLocation);
        let Some(mut merge) = self.ivars().tab_merge.take() else {
            return;
        };
        self.sense(&mut merge, screen);
        self.ivars().tab_merge.replace(Some(merge));
    }

    /// The tab and window a block at `screen` would land in, if the keys are down and the
    /// pointer is over panes: not over a bar (a drop there is the bar's). In the carried tab's
    /// own window a tab that is not the carried one is the one to land in; in another window
    /// the tab it shows is.
    fn aim(&self, tab: u64, screen: NSPoint) -> Option<Aim> {
        if !arranging() {
            return None;
        }
        let from = self.window_holding(tab)?;
        let carried = self.tab(tab)?;
        let window = self.window_at(screen)?;
        let point = window.ns_window().convertPointFromScreen(screen);
        if window.bar().holds(point) {
            return None;
        }
        let host = if window.id() == from.id() {
            host_of(
                window.try_selected_tab().map(|open| open.id()),
                window.bar().selected_before(tab),
                tab,
                |candidate| window.index_of(candidate).is_some(),
            )
        } else {
            window.try_selected_tab().map(|open| open.id())
        }
        .and_then(|host| self.tab(host))?;
        Some(Aim {
            window,
            from,
            carried,
            host,
            point,
        })
    }

    /// The block's verdict over the host's panes.
    fn weigh(aim: &Aim) -> Verdict {
        let incoming = aim.carried.container().tree();
        let moving = aim.carried.panes();
        aim.host
            .container()
            .verdict_of(&incoming, &moving, aim.point)
    }

    fn sense(&self, merge: &mut Merge, screen: NSPoint) {
        let Some(aim) = self.aim(merge.tab, screen) else {
            hide(merge);
            return;
        };
        // The host's panes answer only on screen.
        if !aim.window.is_selected(aim.host.id()) {
            if !aim.window.select_tab(aim.host.id()) {
                hide(merge);
                return;
            }
            merge.opened = true;
        }
        let host = aim.host.id();
        if merge.zones.as_ref().is_none_or(|(held, _)| *held != host) {
            hide(merge);
            let Some(theme) = aim
                .host
                .focused_pane()
                .session()
                .map(|session| session.theme())
            else {
                return;
            };
            let container = aim.host.container();
            let zones = Zones::new(self.mtm(), container.bounds(), &theme, None);
            container.addSubview(&zones);
            merge.zones = Some((host, zones));
        }
        let verdict = Self::weigh(&aim);
        if verdict == merge.verdict {
            return;
        }
        let panes = aim.carried.panes().len();
        let mut shown = preview(&verdict, panes);
        if panes > 1
            && let Verdict::Lands { placement, .. } = &verdict
        {
            shown.inner = aim
                .host
                .container()
                .frames_of(&placement.tree, &aim.carried.container().tree().leaves());
        }
        if let Some((_, zones)) = &merge.zones {
            zones.show(shown);
        }
        merge.verdict = verdict;
    }

    /// The tab's drag session ended at `screen`. `free` is that nothing took the drop and it
    /// was not taken back — the case where the keys decide: down over panes, the regions that
    /// were up are the answer (the block lands where they said, or the tab stays where they
    /// refused); otherwise the session's own end goes on.
    pub(crate) fn tab_merge_end(
        &self,
        merge: Option<Merge>,
        tab: u64,
        screen: NSPoint,
        free: bool,
    ) -> End {
        let iv = self.ivars();
        iv.tab_poll.set(iv.tab_poll.get().wrapping_add(1));
        let Some(mut merge) = merge else {
            return End::Free;
        };
        hide(&mut merge);
        let opened = merge.opened;
        let back = |window: &TerminalWindow| {
            if opened && self.tab(tab).is_some() {
                window.select_tab(tab);
            }
        };
        if !free {
            // A bar took it, or it was taken back: the carried tab was on screen before
            // the keys brought another, and it is the one the session goes on with.
            if let Some(window) = self.window_holding(tab) {
                back(&window);
            }
            return End::Free;
        }
        let Some(aim) = self.aim(tab, screen) else {
            return End::Free;
        };
        let landed = match Self::weigh(&aim) {
            Verdict::Lands { placement, .. } if aim.window.id() == aim.from.id() => {
                aim.window.merge_tab_at(tab, aim.host.id(), placement.tree)
            }
            Verdict::Lands { placement, .. } => self.tab_to_other_tab(
                &aim.from,
                tab,
                &aim.window,
                aim.host.id(),
                Joins::Planned(placement.tree),
            ),
            _ => false,
        };
        if landed {
            End::Merged
        } else {
            back(&aim.from);
            End::Kept
        }
    }

    /// Whether a tab is being carried out of a strip now.
    pub(crate) fn tab_dragging(&self) -> bool {
        self.ivars().tab_drag.borrow().is_some()
    }

    /// Takes the carried tab's state out of the cell, for the end of its session.
    pub(crate) fn tab_merge_take(&self) -> Option<Merge> {
        self.ivars().tab_merge.take()
    }
}

/// Takes the regions off the panes.
fn hide(merge: &mut Merge) {
    if let Some((_, zones)) = merge.zones.take() {
        zones.removeFromSuperview();
    }
    merge.verdict = Verdict::Nothing;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_lands_in_the_tab_on_screen_or_the_one_the_press_left() {
        let here = |tab: u64| tab != 9;
        // Another tab is on screen (the keys brought it): that one.
        assert_eq!(host_of(Some(2), Some(3), 1, here), Some(2));
        // The carried tab is on screen, as a press leaves it: the tab before the press.
        assert_eq!(host_of(Some(1), Some(3), 1, here), Some(3));
        // The tab before the press is the carried one (pressed while open) or gone: none.
        assert_eq!(host_of(Some(1), Some(1), 1, here), None);
        assert_eq!(host_of(Some(1), Some(9), 1, here), None);
        assert_eq!(host_of(Some(1), None, 1, here), None);
        assert_eq!(host_of(None, Some(3), 1, here), Some(3));
    }
}
