//! The terminals a server keeps for the user that no pane of the window shows, in the big view:
//! one section for each server under the tabs, its tiles in the same grid, dim until selected.
//!
//! **Sections are the second skeleton.** The tabs come first and the sections follow, by the
//! server's name, each under a heading of its own ([`Row::Section`]) with its tiles in the strip
//! of the tabs' width. A tile is an [`Entry`] of the server card's own list
//! ([`crate::server_card::entry_of`]: the same title, folder and activity) and its picture is the
//! terminal's screen as the server builds it, which the shell asks for and draws
//! (`bt_core::pictured`). Nothing here reads a server: the shell hands the sections in
//! ([`Overview::set_background`]) and gets an [`Act`] out.
//!
//! **The cells are numbered tabs first, then the tiles** section by section, so the selection,
//! the arrows and the layout take one number for either ([`Overview::cell`]).

use super::*;
use crate::server_card::Entry;

/// The words of the verbs: ↵, ⌥↵, ⌘↵, the menu's, and the one on a terminal another computer has
/// open (the server card's own).
pub const OPEN_IN_TAB: &str = "Open in New Tab";
pub const MOVE_HERE: &str = crate::server_card::MOVE;
pub const OPEN_BESIDE: &str = "Open Beside";
pub const OPEN_ALL: &str = "Open All in New Tab";
pub const END_TERMINAL: &str = "End Terminal";

/// The heading of a server's section: `In the background on ⇄ rasp · 3`.
pub fn section_text(host: &str, count: usize) -> String {
    format!(
        "{} on ⇄ {host} · {count}",
        crate::palette::BACKGROUND_HEADING
    )
}

/// One server's terminals in the background, as the card lists them (all of them, not its first
/// six), the last to write first.
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    /// The server's account ([`crate::ssh_wrap::host_key`]).
    pub account: String,
    /// The server as the user typed it (`rasp`).
    pub host: String,
    /// The terminals the server keeps that no computer shows.
    pub entries: Vec<Entry>,
    /// The terminals another computer has open ([`Entry::by`] names it): listed in a group of
    /// their own, moved here with ↵ and never opened beside, in a tab or with the rest.
    pub elsewhere: Vec<Entry>,
}

/// What the search found in one terminal's tile.
#[derive(Clone, Debug, PartialEq)]
pub struct AwayHit {
    /// The letters of the title the query matched.
    pub name_marks: Vec<usize>,
    /// The letters of the folder it matched, when it did.
    pub place_marks: Vec<usize>,
}

/// A cell of the grid: a tab, or a terminal in the background.
#[derive(Clone, Copy, Debug)]
pub enum Cell<'a> {
    Tab(&'a Tab),
    Away {
        /// Where the section stands among [`Overview::sections`].
        section: usize,
        entry: &'a Entry,
        /// The terminal is another computer's (the group *On other computers*), not the
        /// server's own in the background.
        other: bool,
    },
}

/// The key a terminal in the background is selected by: its account and its name there.
pub type Key = (String, String);

/// Whether `host` is the server of `section` (`user@` and port aside: a pane says
/// `me@rasp`, the server's list `rasp`).
fn same_host(host: &str, section: &Section) -> bool {
    bt_core::bare_host(host).eq_ignore_ascii_case(bt_core::bare_host(&section.host))
}

/// The one the query makes of `entry`: found when its title or its folder matches, by ⌘P's own
/// rows ([`score`], a kind with no cluster word).
fn away_hit(entry: &Entry, query: &str) -> Option<AwayHit> {
    let marks_of = |text: &str| {
        score(&PaletteRow::new(Kind::Pane, "away", text.to_owned()), query).map(|found| found.marks)
    };
    let name = marks_of(&entry.title);
    let place = entry.place.as_deref().and_then(marks_of);
    if name.is_none() && place.is_none() {
        return None;
    }
    Some(AwayHit {
        name_marks: name.unwrap_or_default(),
        place_marks: place.unwrap_or_default(),
    })
}

impl Overview {
    /// The servers' terminals, read afresh (the view is open while they come and go): the
    /// sections in the order of their servers' names, the selection kept on its terminal and on
    /// the one that took its place when it is gone.
    pub fn set_background(&mut self, mut sections: Vec<Section>) {
        sections.retain(|section| !section.entries.is_empty() || !section.elsewhere.is_empty());
        sections.sort_by_key(|section| section.host.to_lowercase());
        if sections == self.sections {
            return;
        }
        // Where the selected terminal stands among the terminals, for the one that takes its
        // place when it is gone.
        let place = self.selected_away.as_ref().and_then(|key| {
            self.away
                .iter()
                .position(|&(s, e, o)| self.key_of(s, e, o) == *key)
        });
        let before = self.selected_away.clone();
        let was_other = self.selected_other();
        self.sections = sections;
        if !self
            .machines()
            .iter()
            .any(|row| row.machine == self.machine)
        {
            self.machine = Machine::All;
        }
        if self
            .glance
            .as_ref()
            .is_some_and(|glance| !self.machines().iter().any(|row| row.machine == *glance))
        {
            self.glance = None;
        }
        self.reshow();
        // A terminal another computer opened meanwhile is the same terminal under the same
        // selection, but ↵ on it would now take it from there: the selection leaves it as it
        // leaves a terminal that is gone.
        let taken = self.selected_other() && !was_other;
        if before.is_some() && (self.selected_away != before || taken) {
            self.reselect_away(place.unwrap_or(0), was_other);
        }
    }

    /// The selection of a terminal that is gone (or that another computer opened) goes to the
    /// terminal that took its `place`, else to the one before it; one another computer has open
    /// is never chosen for the user unless the selection was in that group already (`other`).
    /// With none to choose it goes to the first tab.
    fn reselect_away(&mut self, place: usize, other: bool) {
        let fits = |at: &usize| other || !self.away[*at].2;
        let place = place.min(self.away.len());
        let chosen = (place..self.away.len())
            .find(fits)
            .or_else(|| (0..place).rev().find(fits));
        match chosen {
            Some(at) => {
                let (s, e, o) = self.away[at];
                self.selected = None;
                self.selected_away = Some(self.key_of(s, e, o));
            }
            None => {
                self.selected_away = None;
                self.selected = self.shown.first().map(|&at| self.all[at].id);
            }
        }
    }

    /// The servers' sections (all of them, whatever the machine and the search leave).
    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    pub(super) fn key_of(&self, section: usize, entry: usize, other: bool) -> Key {
        let entry = self.entry_of(section, entry, other);
        (entry.account.clone(), entry.terminal.clone())
    }

    fn entry_of(&self, section: usize, entry: usize, other: bool) -> &Entry {
        let section = &self.sections[section];
        if other {
            &section.elsewhere[entry]
        } else {
            &section.entries[entry]
        }
    }

    /// The first terminal of the server's own that the grid holds: where a search puts the
    /// selection when it finds no tab. A terminal another computer has open is never chosen for
    /// the user (↵ on it would take it from the computer that shows it).
    pub(super) fn first_background(&self) -> Option<Key> {
        self.away
            .iter()
            .find(|&&(_, _, other)| !other)
            .map(|&(s, e, o)| self.key_of(s, e, o))
    }

    /// Whether the selected terminal is another computer's.
    pub fn selected_other(&self) -> bool {
        let Some(key) = self.selected_away.as_ref() else {
            return false;
        };
        self.away.iter().any(|&(s, e, o)| {
            let entry = self.entry_of(s, e, o);
            o && (&entry.account, &entry.terminal) == (&key.0, &key.1)
        })
    }

    /// The places of the terminals the machine and the search leave, section by section.
    pub(super) fn reaway(&mut self) {
        let query = self.query.trim().to_owned();
        self.away.clear();
        self.away_hits.clear();
        for (s, section) in self.sections.iter().enumerate() {
            let held = match &self.machine {
                Machine::All => true,
                Machine::Local => false,
                Machine::Host(host) => same_host(host, section),
            };
            if !held {
                continue;
            }
            for (other, entries) in [(false, &section.entries), (true, &section.elsewhere)] {
                for (e, entry) in entries.iter().enumerate() {
                    if query.is_empty() {
                        self.away.push((s, e, other));
                    } else if let Some(hit) = away_hit(entry, &query) {
                        self.away_hits
                            .insert((entry.account.clone(), entry.terminal.clone()), hit);
                        self.away.push((s, e, other));
                    }
                }
            }
        }
    }

    /// How many terminals the grid holds.
    pub fn away_len(&self) -> usize {
        self.away.len()
    }

    /// How many cells the grid holds: the tabs, then the terminals.
    pub fn cells(&self) -> usize {
        self.shown.len() + self.away.len()
    }

    /// Cell `index`: a tab or a terminal in the background.
    pub fn cell(&self, index: usize) -> Option<Cell<'_>> {
        if index < self.shown.len() {
            return Some(Cell::Tab(self.tab(index)));
        }
        let &(section, entry, other) = self.away.get(index - self.shown.len())?;
        Some(Cell::Away {
            section,
            entry: self.entry_of(section, entry, other),
            other,
        })
    }

    /// What the search found in the terminal `key`, while there is a query.
    pub fn away_hit(&self, key: &Key) -> Option<&AwayHit> {
        self.away_hits.get(key)
    }

    /// The terminal selected, if one is.
    pub fn selected_away(&self) -> Option<&Key> {
        self.selected_away.as_ref()
    }

    /// The selected cell's place among the cells, a tab's or a terminal's.
    pub fn selected_cell(&self) -> Option<usize> {
        if let Some(index) = self.selected_index() {
            return Some(index);
        }
        let key = self.selected_away.as_ref()?;
        let at = self
            .away
            .iter()
            .position(|&(s, e, o)| self.key_of(s, e, o) == *key)?;
        Some(self.shown.len() + at)
    }

    /// Selects cell `index`; `false` when there is none.
    pub(super) fn select_cell(&mut self, index: usize) -> bool {
        match self.cell(index) {
            Some(Cell::Tab(tab)) => {
                self.selected = Some(tab.id);
                self.selected_away = None;
                self.pointed = None;
                true
            }
            Some(Cell::Away { entry, .. }) => {
                self.selected_away = Some((entry.account.clone(), entry.terminal.clone()));
                self.selected = None;
                self.pointed = None;
                true
            }
            None => false,
        }
    }

    /// The pointer is over cell `index` (the same as the keys' selection: a tab or a terminal).
    pub fn point_cell(&mut self, index: usize) -> bool {
        let before = (self.selected, self.selected_away.clone());
        let moved = self.select_cell(index);
        moved && before != (self.selected, self.selected_away.clone())
    }

    /// The server whose section the selected terminal is in, when it is one of the server's own
    /// in the background.
    pub(super) fn away_host(&self) -> Option<&Section> {
        let key = self.selected_away.as_ref()?;
        self.sections.iter().find(|section| {
            section
                .entries
                .iter()
                .any(|entry| (&entry.account, &entry.terminal) == (&key.0, &key.1))
        })
    }

    /// The server ⌘↵ opens every terminal of: the selected terminal's, else the machine's when it
    /// is a server with a section; `None` when there is none to open.
    pub fn open_all_host(&self) -> Option<String> {
        if let Some(section) = self.away_host() {
            return Some(section.host.clone());
        }
        let Machine::Host(host) = &self.machine else {
            return None;
        };
        self.sections
            .iter()
            .find(|section| same_host(host, section))
            .map(|section| section.host.clone())
    }

    /// The Act of the selected terminal opened the other way (⌥↵): beside the pane on screen.
    pub fn act_beside(&self) -> Option<Act> {
        if self.selected_other() {
            return None;
        }
        let (account, terminal) = self.selected_away.clone()?;
        Some(Act::OpenBeside { account, terminal })
    }

    /// The terminals the grid holds, section by section, as `(section, entry)`.
    pub fn away_cells(&self) -> &[(usize, usize, bool)] {
        &self.away
    }

    /// The text under the sections' headings, one for each section the grid shows with a
    /// terminal: heading, then how many tiles.
    pub(super) fn parts(&self) -> Vec<Part> {
        let mut parts: Vec<Part> = Vec::new();
        for &(s, _, other) in &self.away {
            match parts.last_mut() {
                Some(part) if part.section == s && part.other == other => part.count += 1,
                _ => parts.push(Part {
                    section: s,
                    other,
                    text: String::new(),
                    count: 1,
                }),
            }
        }
        for part in &mut parts {
            let section = &self.sections[part.section];
            // The second group's heading is the server card's own; a server with nothing of its
            // own in the grid says whose group it is.
            let alone = !self.away.iter().any(|&(s, _, o)| s == part.section && !o);
            part.text = if !part.other {
                section_text(&section.host, part.count)
            } else if alone {
                format!(
                    "{} on ⇄ {} · {}",
                    crate::server_card::OTHERS_HEADING,
                    section.host,
                    part.count
                )
            } else {
                crate::server_card::others_heading(part.count)
            };
        }
        parts
    }
}

/// A section as the grid shows it: its heading and how many tiles stand under it.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    /// Where the section stands among [`Overview::sections`].
    pub section: usize,
    /// The group of terminals other computers have open, not the server's own.
    pub other: bool,
    pub text: String,
    pub count: usize,
}
