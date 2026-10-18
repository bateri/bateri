//! Tab groups: which tabs of a window are gathered under a name, and every rule that follows —
//! where a new tab opens, what a closed tab leaves behind, what ⌘1…⌘9 and ⌃⌘0…⌃⌘9 reach, which
//! tabs a click selects together. The platform shell draws the headers and calls these operations
//! from the places that change a window's tabs; nothing here sees a UI toolkit.
//!
//! **The order stays one flat list.** [`Tabs`] keeps the order and the selected tab exactly as it
//! did before groups existed, and [`Groups`] sits beside it holding the groups and who belongs to
//! them. What ties the two is an invariant that every operation here keeps ([`Groups::check`]):
//! the flat order is the ungrouped tabs first, then each group's tabs **next to each other**, the
//! groups in their own order, and no group is empty. So everything that reads the flat order —
//! ⌘ numbers, the strip's and the column's places, the C ABI's strip — keeps reading it, and a
//! window without groups is the plain list it was: every operation here then answers what the
//! `Tabs` operation behind it answers.
//!
//! The operations take `&mut Tabs` next to `&mut self` for that reason: a group is never changed
//! without the order under it being changed to match. A caller that moved a tab in the `Tabs`
//! by itself would break the invariant, and the shell's tests ask [`Groups::check`] after its own
//! changes.
//!
//! **Clusters.** The ungrouped tabs are a cluster like each group, always the first one and with
//! no header; a cluster is named by an `Option<GroupId>` with `None` for them. A window shows
//! one cluster in its tab bar at the top and all of them in the side column. Each cluster
//! remembers the tab the user was last on in it (its *resident*), which is where ⌃⌘N and a closed
//! tab's successor group come back to.

use crate::tabs::{self, Column, Entry, Tabs};

/// The name a group takes when nothing better is known.
pub const UNTITLED: &str = "Untitled";

/// A group of one window. Identities are the window's own: they are counted from 1 in each
/// [`Groups`] and mean nothing in another one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GroupId(u64);

impl GroupId {
    /// The number the shell keys its views by.
    pub fn get(self) -> u64 {
        self.0
    }
}

/// One group, as the shell reads it. It is changed only through [`Groups`], which keeps the
/// order of the tabs in step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group<K> {
    pub id: GroupId,
    pub name: String,
    /// The column shows the header and, while the tab on screen is of the group, that one tab.
    pub collapsed: bool,
    resident: Option<K>,
}

impl<K: Copy> Group<K> {
    /// The tab the user was last on in the group, if it is still in it.
    pub fn resident(&self) -> Option<K> {
        self.resident
    }
}

/// A window's groups and who belongs to them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Groups<K> {
    /// In the order the column and the ⌃⌘ numbers list them; the flat order of the tabs follows it.
    groups: Vec<Group<K>>,
    /// A tab that is in a group; a tab that is not here is ungrouped.
    members: Vec<(K, GroupId)>,
    /// The ungrouped cluster's resident.
    loose: Option<K>,
    next: u64,
}

impl<K> Default for Groups<K> {
    fn default() -> Self {
        Self {
            groups: Vec::new(),
            members: Vec::new(),
            loose: None,
            next: 1,
        }
    }
}

/// What a list of tabs is shown as: the column's whole list or the bar's one cluster.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    /// The side column: every cluster, each group under its header.
    Side,
    /// The tab bar: the cluster the selected tab is in.
    Top,
}

/// One place of the side column's list ([`Groups::side_items`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item<K> {
    Header(GroupId),
    Tab(K),
}

impl<K> Item<K> {
    /// What the layout needs to know of it ([`tabs::Side::layout_with`]).
    pub fn entry(&self) -> Entry {
        match self {
            Item::Header(_) => Entry::Header,
            Item::Tab(_) => Entry::Tab,
        }
    }
}

/// Where tabs are put ([`Groups::place`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target<K> {
    /// Out of any group, among the ungrouped tabs: before `before`, or after the last of them.
    Loose { before: Option<K> },
    /// Into `group`: before `before` (a tab of the group), or after its last tab.
    Group { group: GroupId, before: Option<K> },
    /// Into a group of their own, made for them after the others, with this name (made unique,
    /// [`Groups::unique_name`]).
    New(String),
}

/// A place the side column takes carried tabs at ([`Groups::side_spots`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Spot<K> {
    /// From this y (the column's space, top-down) down to the next spot's `from`, a pointer
    /// means this place: the zones tile the column, the first reaching up past its top and the
    /// last down past its foot.
    pub from: f64,
    /// Where the line that marks the place runs: in the gap between the two things the tabs
    /// would stand between.
    pub line: f64,
    /// Where the tabs would be put.
    pub target: Target<K>,
}

/// The spot a pointer at `y` is over: the last whose zone has begun, the first for a pointer above
/// the list. `None` for no spot at all (a list that is not what the column was laid out from).
pub fn spot_at<K>(spots: &[Spot<K>], y: f64) -> Option<&Spot<K>> {
    spot_slot(spots, y).map(|slot| &spots[slot])
}

/// The index of [`spot_at`]'s spot — the slot [`Groups::side_preview`] takes.
pub fn spot_slot<K>(spots: &[Spot<K>], y: f64) -> Option<usize> {
    spots
        .iter()
        .rposition(|spot| spot.from <= y)
        .or_else(|| (!spots.is_empty()).then_some(0))
}

/// A place the side column takes a carried group at, among the groups
/// ([`Groups::group_spots`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroupSpot {
    /// As [`Spot::from`].
    pub from: f64,
    /// As [`Spot::line`]: the gap above the header of the group it stands before, below the last
    /// group at the end.
    pub line: f64,
    /// The slot among the groups: before the group at this index, `len` at the end.
    pub slot: usize,
}

/// The spot of `spots` that a pointer at `y` is over, for groups ([`spot_at`]'s twin).
pub fn group_spot_at(spots: &[GroupSpot], y: f64) -> Option<&GroupSpot> {
    spots
        .iter()
        .rev()
        .find(|spot| spot.from <= y)
        .or(spots.first())
}

/// The side column's list with one row put at a place of it ([`Groups::side_preview`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SidePreview<K> {
    /// The listed tabs in order, the one that is put among them.
    pub rows: Vec<K>,
    /// For each group's header in order, the index in [`Self::rows`] of the row under it — the
    /// number of rows when there is none (a closed group that lists nothing).
    pub firsts: Vec<(GroupId, usize)>,
}

/// The place a group at `from` among the groups takes when it is let go at slot `slot` (before the
/// group at that index of the list as it is, `len` for the end): the index [`Groups::move_group`]
/// takes — a slot after the group counts one more than the place it ends at, as the group is not
/// in the way once it has been taken out.
pub fn group_place(from: usize, slot: usize) -> usize {
    if slot > from { slot - 1 } else { slot }
}

/// What ⌃⌘0…⌃⌘9 reaches ([`Groups::reach`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach<K> {
    /// Go to `tab`; when `expand` names a group, it is closed and opens first.
    Tab { tab: K, expand: Option<GroupId> },
    /// ⌃⌘0 with no ungrouped tab: a new one opens ([`Groups::open_loose`]).
    NewLoose,
    /// There is no such group.
    Nothing,
}

/// How a click on a row selects ([`Groups::pick`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Click {
    /// No modifier: the tab opens and any selection is let go.
    Plain,
    /// ⇧: from the tab on screen to the clicked one.
    Range,
    /// ⌘: the clicked one joins the selection or leaves it.
    Toggle,
}

/// What a click on a row comes to ([`Groups::pick`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picked<K> {
    /// The tabs selected together after the click, in the order of the list; empty for none.
    pub picks: Vec<K>,
    /// The tab the click opens: `Some` when it acted as a plain click, and then `picks` is
    /// empty. The tab on screen is otherwise not changed.
    pub open: Option<K>,
}

/// A group as a saved layout holds it, with the tabs it gathers ([`Groups::from_parts`],
/// [`Groups::parts`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part<K> {
    pub name: String,
    pub collapsed: bool,
    /// The tab the user was last on in the group; one of [`Self::tabs`].
    pub resident: Option<K>,
    /// The group's tabs, in the order of the strip.
    pub tabs: Vec<K>,
}

/// The name a group starts with from its first tab's title: the folder it stands in (`~/code/api`
/// → `api`), `home` for the home directory (`~`), the host for a remote tab (`⇄ db-1` → `db-1`),
/// and [`UNTITLED`] when there is nothing to take.
pub fn name_from(title: &str) -> String {
    let title = title.trim();
    let title = title.strip_prefix('⇄').unwrap_or(title).trim();
    let folder = title
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(title)
        .trim();
    match folder {
        "" => UNTITLED.to_owned(),
        "~" => "home".to_owned(),
        folder => folder.to_owned(),
    }
}

impl<K: Copy + Eq> Groups<K> {
    pub fn new() -> Self {
        Self::default()
    }

    /// No group: the window is a plain list of tabs.
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    pub fn len(&self) -> usize {
        self.groups.len()
    }

    /// The groups, in the order they are listed.
    pub fn groups(&self) -> &[Group<K>] {
        &self.groups
    }

    pub fn group(&self, id: GroupId) -> Option<&Group<K>> {
        self.groups.iter().find(|group| group.id == id)
    }

    /// Where the group stands among the groups.
    pub fn position(&self, id: GroupId) -> Option<usize> {
        self.groups.iter().position(|group| group.id == id)
    }

    /// The group a tab is in, `None` for an ungrouped one.
    pub fn group_of(&self, tab: K) -> Option<GroupId> {
        self.members
            .iter()
            .find(|(member, _)| *member == tab)
            .map(|&(_, group)| group)
    }

    /// The tabs of a cluster, in the order of the strip.
    pub fn cluster(&self, tabs: &Tabs<K>, of: Option<GroupId>) -> Vec<K> {
        tabs.ids()
            .iter()
            .copied()
            .filter(|&tab| self.group_of(tab) == of)
            .collect()
    }

    /// The cluster of the selected tab: the one the tab bar shows. `None` is the ungrouped tabs
    /// (and a window with no tab).
    pub fn shown(&self, tabs: &Tabs<K>) -> Option<GroupId> {
        tabs.selected().and_then(|tab| self.group_of(tab))
    }

    /// The tab the user was last on in a cluster.
    pub fn resident(&self, of: Option<GroupId>) -> Option<K> {
        match of {
            Some(id) => self.group(id).and_then(|group| group.resident),
            None => self.loose,
        }
    }

    fn clusters(&self) -> impl Iterator<Item = Option<GroupId>> + '_ {
        std::iter::once(None).chain(self.groups.iter().map(|group| Some(group.id)))
    }

    /// The rank of a cluster in the strip: 0 for the ungrouped tabs, then the groups.
    fn rank(&self, of: Option<GroupId>) -> usize {
        of.map_or(0, |id| self.position(id).map_or(usize::MAX, |at| at + 1))
    }

    // ─── What is listed ──────────────────────────────────────────────────

    /// The side column's list: the ungrouped tabs, then each group's header and its tabs. A closed
    /// group lists the tab on screen if it is the group's, and no other.
    pub fn side_items(&self, tabs: &Tabs<K>) -> Vec<Item<K>> {
        let shown = tabs.selected();
        let mut items: Vec<Item<K>> = self
            .cluster(tabs, None)
            .into_iter()
            .map(Item::Tab)
            .collect();
        for group in &self.groups {
            items.push(Item::Header(group.id));
            items.extend(
                self.cluster(tabs, Some(group.id))
                    .into_iter()
                    .filter(|&tab| !group.collapsed || Some(tab) == shown)
                    .map(Item::Tab),
            );
        }
        items
    }

    /// The tabs ⌘1…⌘9 count, in the order they are shown: down the column's list (a closed
    /// group's hidden tabs have no number), or the bar's cluster.
    pub fn numbered(&self, tabs: &Tabs<K>, view: View) -> Vec<K> {
        match view {
            View::Side => self
                .side_items(tabs)
                .into_iter()
                .filter_map(|item| match item {
                    Item::Tab(tab) => Some(tab),
                    Item::Header(_) => None,
                })
                .collect(),
            View::Top => self.cluster(tabs, self.shown(tabs)),
        }
    }

    /// The place in the flat strip of the `listed`th tab a view lists ([`Self::numbered`]'s
    /// order), and past the last: what a row dragged or a tab carried to a place of the list is
    /// let go at, for the appliers that count the strip. The column lists every cluster but hides
    /// a closed group's tabs, and the bar lists one cluster, so the two differ from the strip
    /// as soon as one is closed or a cluster is not the whole window; the place is the listed
    /// tab's own, which puts a tab let go before it beside it (the cluster rule of
    /// [`Self::insert_at`]). Past the last listed tab it is the strip's end in the column, and in
    /// the bar the place after the cluster's last tab.
    pub fn listed_place(&self, tabs: &Tabs<K>, view: View, listed: usize) -> usize {
        let numbered = self.numbered(tabs, view);
        numbered
            .get(listed)
            .and_then(|&tab| tabs.index_of(tab))
            .or_else(|| match view {
                View::Side => None,
                View::Top => numbered
                    .last()
                    .and_then(|&last| tabs.index_of(last))
                    .map(|at| at + 1),
            })
            .unwrap_or_else(|| tabs.len())
    }

    // ─── Where a drop lands ──────────────────────────────────────────────

    /// The places the side column takes carried tabs at, top to bottom, for `column` laid out from
    /// [`Self::side_items`]: a place between every two things the list holds (and above the first,
    /// below the last), each naming where the tabs would be put and where the line that marks it
    /// runs. **Two places stand at a group's border**: below the last row of a group is that
    /// group's end, between its header and its first row is its head — a flat place cannot say
    /// which, and the second is what [`Self::landing`] would have chosen alone. The pointer
    /// decides by the middle of the header: above it the gap opens over the header, at the end of
    /// the group above (the lower half of its last row and the gap over the header are the
    /// same place); below it the gap opens under the header, at the head of the group (with the
    /// [`tabs::GAP`] under the header and the upper half of its first row). **A closed group's
    /// header is all the group's**: it lists no row to be the end of, so a pointer anywhere on it
    /// means the group's end. Empty when `column` is not what the list lays out to (the list
    /// changed under a held button): the caller has nothing to drop on then.
    ///
    /// The tabs being carried are not named: a place before one of them stands before the next
    /// that stays ([`Self::place`]), and one that changes nothing is refused by the move.
    pub fn side_spots(&self, tabs: &Tabs<K>, column: &Column) -> Vec<Spot<K>> {
        let items = self.side_items(tabs);
        let Some(mut frames) = item_frames(&items, column) else {
            return Vec::new();
        };
        // An open header is read by its middle, a closed one by its top (all of it is the
        // group's).
        for (item, frame) in items.iter().zip(&mut frames) {
            if let Item::Header(group) = item
                && self.group(*group).is_some_and(|group| !group.collapsed)
            {
                frame.split = (frame.top + frame.bottom) / 2.0;
            }
        }
        (0..=items.len())
            .map(|slot| {
                let line = match slot {
                    0 => frames[0].top - tabs::GAP / 2.0,
                    slot if slot == items.len() => frames[slot - 1].bottom + tabs::GAP / 2.0,
                    slot => (frames[slot - 1].bottom + frames[slot].top) / 2.0,
                };
                Spot {
                    from: if slot == 0 {
                        f64::NEG_INFINITY
                    } else {
                        frames[slot - 1].split
                    },
                    line,
                    target: self.slot_target(tabs, &items, slot),
                }
            })
            .collect()
    }

    /// The places the side column takes a carried group at: before each group and after the last,
    /// the pointer deciding by the middle of each group's block (its header and the rows under it)
    /// — over the upper half of a block the group stands before it, over the lower half after it.
    /// The ungrouped tabs stay above whatever place is named: the first place is below them. Empty
    /// as [`Self::side_spots`] is.
    pub fn group_spots(&self, tabs: &Tabs<K>, column: &Column) -> Vec<GroupSpot> {
        let items = self.side_items(tabs);
        let Some(frames) = item_frames(&items, column) else {
            return Vec::new();
        };
        // A block runs from its header to the item before the next one.
        let heads: Vec<usize> = items
            .iter()
            .enumerate()
            .filter(|(_, item)| matches!(item, Item::Header(_)))
            .map(|(at, _)| at)
            .collect();
        if heads.is_empty() {
            return Vec::new();
        }
        let block = |index: usize| {
            let end = heads.get(index + 1).copied().unwrap_or(items.len());
            (frames[heads[index]].top, frames[end - 1].bottom)
        };
        (0..=heads.len())
            .map(|slot| {
                let line = match slot {
                    slot if slot == heads.len() => block(slot - 1).1 + tabs::GAP / 2.0,
                    slot => {
                        let top = frames[heads[slot]].top;
                        match heads[slot] {
                            0 => top - tabs::GAP / 2.0,
                            at => (frames[at - 1].bottom + top) / 2.0,
                        }
                    }
                };
                GroupSpot {
                    from: if slot == 0 {
                        f64::NEG_INFINITY
                    } else {
                        let (top, bottom) = block(slot - 1);
                        (top + bottom) / 2.0
                    },
                    line,
                    slot,
                }
            })
            .collect()
    }

    /// The side column's list as it would stand if `tab` were let go at slot `slot` of
    /// [`Self::side_spots`] (the same numbering: a place between two things of the list as it is,
    /// above the first or below the last) — the rows in order and, for each group, the index of
    /// the row under its header — which a row slid along the column is laid out as while it is
    /// held, so the others make room where it would stand: over a header, at the end of the group
    /// above, or under it, at the head of the group. `None` for a tab that is not listed or a slot
    /// that is not there.
    ///
    /// What the drop does is [`Self::place`] of [`Self::slot_target`]; the two agree on where
    /// the row stands (the tests make each slot say so), so what is shown is what is done.
    pub fn side_preview(&self, tabs: &Tabs<K>, tab: K, slot: usize) -> Option<SidePreview<K>> {
        let mut items = self.side_items(tabs);
        let from = items.iter().position(|item| *item == Item::Tab(tab))?;
        if slot > items.len() {
            return None;
        }
        items.remove(from);
        items.insert(if from < slot { slot - 1 } else { slot }, Item::Tab(tab));
        let mut preview = SidePreview {
            rows: Vec::new(),
            firsts: Vec::new(),
        };
        for item in items {
            match item {
                Item::Tab(tab) => preview.rows.push(tab),
                Item::Header(group) => preview.firsts.push((group, preview.rows.len())),
            }
        }
        Some(preview)
    }

    /// Where a tab (or a pane that becomes one) let go at gap `gap` of what a view lists — before
    /// the `gap`th listed tab, `len` for the end — is put, **for a window that has groups**: in the
    /// column the gap is the line above the headers that stand over that tab (the gap between two
    /// groups, [`tabs::Column::between`]), so it is the **end** of the group above; in the bar it is
    /// a place of the cluster on screen, whose end is its own. `None` for a plain list of tabs,
    /// where the strip's place says everything.
    pub fn gap_target(&self, tabs: &Tabs<K>, view: View, gap: usize) -> Option<Target<K>> {
        if self.is_empty() {
            return None;
        }
        Some(match view {
            View::Side => {
                let items = self.side_items(tabs);
                let mut listed = 0;
                let mut slot = items.len();
                for (at, item) in items.iter().enumerate() {
                    if listed == gap {
                        slot = at;
                        break;
                    }
                    if matches!(item, Item::Tab(_)) {
                        listed += 1;
                    }
                }
                self.slot_target(tabs, &items, slot)
            }
            View::Top => {
                let of = self.shown(tabs);
                let before = self.cluster(tabs, of).get(gap).copied();
                match of {
                    Some(group) => Target::Group { group, before },
                    None => Target::Loose { before },
                }
            }
        })
    }

    /// Where `tab`, let go on its own title row at `place` of what the bar lists, goes: the place
    /// it ends at, as a chip dragged along the bar takes ([`Self::move_within`]) — the `place`th
    /// of its cluster's tabs once it has gone, its end past the last. The bar lists the tab
    /// with the tab in it, which [`Self::gap_target`] counts for a tab coming from elsewhere;
    /// for a tab that is already there a place to the right is one more of the list than the
    /// gap before the tab it names.
    pub fn own_target(&self, tabs: &Tabs<K>, tab: K, place: usize) -> Target<K> {
        let of = self.group_of(tab);
        let before = self
            .cluster(tabs, of)
            .into_iter()
            .filter(|&other| other != tab)
            .nth(place);
        match of {
            Some(group) => Target::Group { group, before },
            None => Target::Loose { before },
        }
    }

    /// Where a tab opened right of `tab` goes in a window that has groups: `tab`'s own cluster,
    /// after it — not the next cluster's head, which is where a bare place of the strip would put
    /// it when `tab` is the last of its cluster. `None` for a plain list of tabs.
    pub fn beside_target(&self, tabs: &Tabs<K>, tab: K) -> Option<Target<K>> {
        if self.is_empty() {
            return None;
        }
        let of = self.group_of(tab);
        let cluster = self.cluster(tabs, of);
        let before = cluster
            .iter()
            .position(|&other| other == tab)
            .and_then(|at| cluster.get(at + 1))
            .copied();
        Some(match of {
            Some(group) => Target::Group { group, before },
            None => Target::Loose { before },
        })
    }

    /// Where tabs let go at `slot` of `items` (between the item before it and the one at it) are
    /// put ([`Self::side_spots`]).
    fn slot_target(&self, tabs: &Tabs<K>, items: &[Item<K>], slot: usize) -> Target<K> {
        let of = |group: Option<GroupId>, before: Option<K>| match group {
            Some(group) => Target::Group { group, before },
            None => Target::Loose { before },
        };
        let prev = slot.checked_sub(1).and_then(|at| items.get(at));
        match (prev, items.get(slot)) {
            // Under a header, before a row: the head of the group, whatever is the row listed first
            // (a closed group lists only the tab on screen).
            (Some(&Item::Header(group)), Some(Item::Tab(_))) => of(
                Some(group),
                self.cluster(tabs, Some(group)).first().copied(),
            ),
            // Among rows of one cluster, or above the first row.
            (_, Some(&Item::Tab(next))) => of(self.group_of(next), Some(next)),
            // At the end of what is listed, or above a header: the end of the cluster above.
            (Some(&Item::Tab(last)), _) => of(self.group_of(last), None),
            (Some(&Item::Header(group)), _) => of(Some(group), None),
            (None, _) => of(None, None),
        }
    }

    /// The tab ⌘`tag` reaches ([`tabs::tab_index`]: ⌘9 is the last).
    pub fn shortcut(&self, tabs: &Tabs<K>, view: View, tag: u8) -> Option<K> {
        let numbered = self.numbered(tabs, view);
        tabs::tab_index(tag, numbered.len()).map(|index| numbered[index])
    }

    /// What ⌃⌘`tag` reaches: ⌃⌘1…⌃⌘8 the nth group, ⌃⌘9 the last ([`tabs::tab_index`]'s rule
    /// for groups), ⌃⌘0 the ungrouped tabs. A group is entered on the tab the user was last on
    /// in it, else its first.
    pub fn reach(&self, tabs: &Tabs<K>, tag: u8) -> Reach<K> {
        let of = match tag {
            0 => None,
            tag => match tabs::tab_index(tag, self.groups.len()) {
                Some(index) => Some(self.groups[index].id),
                None => return Reach::Nothing,
            },
        };
        self.reach_in(tabs, of)
    }

    /// What reaching cluster `of` comes to: [`Self::reach`] for a cluster named by its identity,
    /// as the list under the title row's group button names it — it lists a group that no key
    /// reaches past the ninth. [`Reach::Nothing`] for a group that is not here.
    pub fn reach_in(&self, tabs: &Tabs<K>, of: Option<GroupId>) -> Reach<K> {
        if of.is_some_and(|id| self.position(id).is_none()) {
            return Reach::Nothing;
        }
        let tab = self
            .resident(of)
            .filter(|&tab| tabs.index_of(tab).is_some() && self.group_of(tab) == of)
            .or_else(|| self.cluster(tabs, of).first().copied());
        match tab {
            Some(tab) => Reach::Tab {
                tab,
                expand: of.filter(|&id| self.group(id).is_some_and(|group| group.collapsed)),
            },
            None => Reach::NewLoose,
        }
    }

    // ─── Opening, selecting, closing ─────────────────────────────────────

    /// Selects a tab; a cluster remembers the tab it was left on. `false` when it already was.
    pub fn select(&mut self, tabs: &mut Tabs<K>, tab: K) -> bool {
        let changed = tabs.select(tab);
        self.settle(tabs);
        changed
    }

    /// Opens a tab where ⌘T does: right of the selected one ([`Tabs::insert`]) and in its group,
    /// a closed group opening for it. A tab that is here is selected instead.
    pub fn open(&mut self, tabs: &mut Tabs<K>, tab: K) {
        if tabs.index_of(tab).is_some() {
            self.select(tabs, tab);
            return;
        }
        let into = self.shown(tabs);
        tabs.insert(tab);
        if let Some(id) = into {
            self.members.push((tab, id));
            self.expand(id);
        }
        self.settle(tabs);
    }

    /// Opens a tab among the ungrouped ones, after the last of them (Shell ▸ New Ungrouped Tab,
    /// and ⌃⌘0 when there is none: at the top of the list). A tab that is here is moved there.
    pub fn open_loose(&mut self, tabs: &mut Tabs<K>, tab: K) {
        if tabs.index_of(tab).is_some() {
            self.place(tabs, &[tab], Target::Loose { before: None });
            self.select(tabs, tab);
            return;
        }
        tabs.insert_at(tab, self.end_of(tabs, None));
        self.settle(tabs);
    }

    /// Opens a tab at the end of a group (the `+` on its header), which opens if it was closed.
    /// A group that is not here makes it [`Self::open`]. A tab that is here is moved there.
    pub fn open_in(&mut self, tabs: &mut Tabs<K>, tab: K, group: GroupId) {
        if self.group(group).is_none() {
            self.open(tabs, tab);
            return;
        }
        if tabs.index_of(tab).is_some() {
            self.place(
                tabs,
                &[tab],
                Target::Group {
                    group,
                    before: None,
                },
            );
        } else {
            tabs.insert_at(tab, self.end_of(tabs, Some(group)));
            self.members.push((tab, group));
        }
        self.expand(group);
        self.select(tabs, tab);
    }

    /// Opens a tab in a group made for it, after the others (New Group). A tab that is here is
    /// moved there.
    pub fn open_in_new_group(&mut self, tabs: &mut Tabs<K>, tab: K, name: &str) -> GroupId {
        if tabs.index_of(tab).is_none() {
            tabs.insert_at(tab, tabs.len());
        }
        let group = self.create(name);
        self.place(
            tabs,
            &[tab],
            Target::Group {
                group,
                before: None,
            },
        );
        self.select(tabs, tab);
        group
    }

    /// A tab let go on the strip at place `gap` (before the tab that stands there; the strip's
    /// length is the end) joins **the cluster of the tab it goes before** — of the last tab at the
    /// end — and is selected: where a tab dragged along or onto a strip lands when the strip is
    /// all the pointer named. A tab that is here moves instead of being added twice. In a window
    /// without groups it is [`Tabs::insert_at`].
    pub fn insert_at(&mut self, tabs: &mut Tabs<K>, tab: K, gap: usize) {
        self.put_at(tabs, tab, gap);
        self.select(tabs, tab);
    }

    /// A new tab at place `gap` ([`Self::insert_at`]'s cluster rule) with the selection left where
    /// it was — a pane that became a tab of its own. `false` if the tab is here already. In a
    /// window without groups it is [`Tabs::place_new`].
    pub fn place_new(&mut self, tabs: &mut Tabs<K>, tab: K, gap: usize) -> bool {
        if tabs.index_of(tab).is_some() {
            return false;
        }
        self.put_at(tabs, tab, gap);
        true
    }

    /// A tab that joins from another window at the end (Merge All Windows), the selection staying
    /// where it was: **among the ungrouped tabs**, after the last of them — a groupless tab at the
    /// far end would stand behind the groups and break the order. `false` if it is here already.
    /// In a window without groups it is [`Tabs::append`].
    pub fn append(&mut self, tabs: &mut Tabs<K>, tab: K) -> bool {
        if tabs.index_of(tab).is_some() {
            return false;
        }
        tabs.append(tab);
        self.place(tabs, &[tab], Target::Loose { before: None });
        true
    }

    /// A tab takes place `index` in the strip (clamped to the last place) and goes into the
    /// cluster of the tab it then stands before, or of the last tab; the selection stays on its
    /// tab. `true` if the order or the tab's group changed. In a window without groups it is
    /// [`Tabs::move_to`].
    pub fn move_to(&mut self, tabs: &mut Tabs<K>, tab: K, index: usize) -> bool {
        if tabs.index_of(tab).is_none() {
            return false;
        }
        let before = tabs.ids().to_vec();
        let group = self.group_of(tab);
        let rest: Vec<K> = before
            .iter()
            .copied()
            .filter(|&other| other != tab)
            .collect();
        let target = self.landing(&rest, index.min(rest.len()));
        self.place(tabs, &[tab], target);
        tabs.ids() != before || self.group_of(tab) != group
    }

    /// A tab takes place `to` among the tabs of **its own cluster** (past the end is the end) and
    /// stays in it: a tab dragged along the bar, which lists one cluster and so cannot name the
    /// place at the cluster's end in the strip — [`Self::move_to`] would put it before the next
    /// cluster's first tab, which is that cluster's. `to` is the place the tab takes among the
    /// cluster's tabs once it has moved. `true` if the order changed. In a window without groups
    /// it is [`Tabs::move_to`].
    pub fn move_within(&mut self, tabs: &mut Tabs<K>, tab: K, to: usize) -> bool {
        if tabs.index_of(tab).is_none() {
            return false;
        }
        let of = self.group_of(tab);
        let before = tabs.ids().to_vec();
        let before_tab = self
            .cluster(tabs, of)
            .into_iter()
            .filter(|&other| other != tab)
            .nth(to);
        let target = match of {
            Some(group) => Target::Group {
                group,
                before: before_tab,
            },
            None => Target::Loose { before: before_tab },
        };
        self.place(tabs, &[tab], target);
        tabs.ids() != before
    }

    /// The tab `source`, whose last pane went into tab `target`, is gone and the screen follows
    /// the pane if it was on `source` ([`Tabs::pane_to_tab`]'s rule, with the group left empty
    /// going too). `true` if `source` was removed.
    pub fn pane_to_tab(&mut self, tabs: &mut Tabs<K>, source: K, emptied: bool, target: K) -> bool {
        if !emptied
            || source == target
            || tabs.index_of(source).is_none()
            || tabs.index_of(target).is_none()
        {
            return false;
        }
        let selected = tabs.selected() == Some(source);
        self.close(tabs, &[source]);
        if selected {
            self.select(tabs, target);
        }
        true
    }

    /// Takes tabs out of the window; how many were there. When the selected one goes,
    /// [`Self::fronting`] says which comes forward. A group that is left empty goes with them.
    pub fn close(&mut self, tabs: &mut Tabs<K>, closing: &[K]) -> usize {
        let closing: Vec<K> = tabs
            .ids()
            .iter()
            .copied()
            .filter(|tab| closing.contains(tab))
            .collect();
        if closing.is_empty() {
            return 0;
        }
        let selected_goes = tabs.selected().is_some_and(|tab| closing.contains(&tab));
        let next = self.fronting(tabs, &closing);
        for &tab in &closing {
            tabs.close(tab);
        }
        self.members.retain(|(tab, _)| !closing.contains(tab));
        self.forget(&closing);
        self.drop_empty();
        if selected_goes {
            if let Some(next) = next {
                tabs.select(next);
            }
        }
        self.settle(tabs);
        closing.len()
    }

    /// The tab on screen once `leaving` have gone: the selected one if it stays. Otherwise, in
    /// its own cluster, the next tab and failing that the one before it — so a tab's closing does
    /// not take the window out of its group; and when the cluster is left empty (**a group does
    /// not stay empty**) the next cluster, failing that the one before — the groups in their
    /// order, the ungrouped tabs before them — on the tab the user was last on in it, else its
    /// first (when going forward) or last. `None` when no tab remains.
    ///
    /// A window that shows one cluster at a time (the tab bar) and keeps its place when tabs are
    /// moved away from it asks this too: if the answer is in another cluster, the cluster is
    /// gone and the window goes with the tabs.
    pub fn fronting(&self, tabs: &Tabs<K>, leaving: &[K]) -> Option<K> {
        let shown = tabs.selected()?;
        if !leaving.contains(&shown) {
            return Some(shown);
        }
        let here = self.group_of(shown);
        let cluster = self.cluster(tabs, here);
        let at = cluster.iter().position(|&tab| tab == shown)?;
        let stays = |tab: &&K| !leaving.contains(*tab);
        if let Some(&tab) = cluster[at + 1..]
            .iter()
            .find(stays)
            .or_else(|| cluster[..at].iter().rev().find(stays))
        {
            return Some(tab);
        }
        let order: Vec<Option<GroupId>> = self.clusters().collect();
        let index = order.iter().position(|&of| of == here)?;
        let enter = |of: Option<GroupId>, forward: bool| -> Option<K> {
            let rest: Vec<K> = self
                .cluster(tabs, of)
                .into_iter()
                .filter(|tab| !leaving.contains(tab))
                .collect();
            self.resident(of)
                .filter(|tab| rest.contains(tab))
                .or_else(|| {
                    if forward {
                        rest.first().copied()
                    } else {
                        rest.last().copied()
                    }
                })
        };
        order[index + 1..]
            .iter()
            .find_map(|&of| enter(of, true))
            .or_else(|| order[..index].iter().rev().find_map(|&of| enter(of, false)))
    }

    // ─── Membership and order ────────────────────────────────────────────

    /// Puts tabs into a cluster and into a place in it, whichever clusters they were in; the
    /// selection stays on its tab. The tabs go in the order they have on the strip, whatever
    /// order they were named in. Groups they leave empty go. `None`, and nothing changes, when no
    /// tab named is here or the target group is not.
    ///
    /// This is every way a tab changes group: Move to Group (`before: None`), Move to New Group,
    /// Remove from Group (`Loose`), a drop between two rows, and [`Self::ungroup`].
    pub fn place(
        &mut self,
        tabs: &mut Tabs<K>,
        moving: &[K],
        target: Target<K>,
    ) -> Option<Option<GroupId>> {
        let moving: Vec<K> = tabs
            .ids()
            .iter()
            .copied()
            .filter(|tab| moving.contains(tab))
            .collect();
        if moving.is_empty() {
            return None;
        }
        let (into, before) = match target {
            Target::Loose { before } => (None, before),
            Target::Group { group, before } => {
                self.position(group)?;
                (Some(group), before)
            }
            Target::New(name) => (Some(self.create(&name)), None),
        };
        // A tab to stand before that is itself on the move is not there to stand before: the
        // next of the cluster that stays is (a row let go on its own place).
        let before = before.and_then(|before| {
            if !moving.contains(&before) {
                return Some(before);
            }
            let cluster = self.cluster(tabs, into);
            let at = cluster.iter().position(|&tab| tab == before)?;
            cluster[at + 1..]
                .iter()
                .copied()
                .find(|tab| !moving.contains(tab))
        });
        for &tab in &moving {
            let from = self.group_of(tab);
            if from != into {
                self.forget_in(from, tab);
            }
            self.members.retain(|(member, _)| *member != tab);
            if let Some(id) = into {
                self.members.push((tab, id));
            }
        }
        self.drop_empty();
        let order = self.arranged(tabs, &moving, into, before);
        Self::apply(tabs, &order);
        self.settle(tabs);
        Some(into)
    }

    /// Dissolves a group: its tabs become ungrouped, after the last ungrouped tab, in their order.
    /// `false` for a group that is not here.
    pub fn ungroup(&mut self, tabs: &mut Tabs<K>, group: GroupId) -> bool {
        let members = self.cluster(tabs, Some(group));
        self.place(tabs, &members, Target::Loose { before: None })
            .is_some()
    }

    /// Moves a group to a place among the groups (past the end is the end); its tabs go with it.
    /// The ungrouped tabs stay first. `false` for a group that is not here or already there.
    pub fn move_group(&mut self, tabs: &mut Tabs<K>, group: GroupId, to: usize) -> bool {
        let Some(from) = self.position(group) else {
            return false;
        };
        let to = to.min(self.groups.len() - 1);
        if from == to {
            return false;
        }
        let moved = self.groups.remove(from);
        self.groups.insert(to, moved);
        let order = self.arranged(tabs, &[], None, None);
        Self::apply(tabs, &order);
        true
    }

    /// Renames a group to the name the user typed, without the whitespace round it. `false` when
    /// that is empty (the name stays) or what it already is.
    pub fn rename(&mut self, group: GroupId, name: &str) -> bool {
        let name = name.trim();
        match self.groups.iter_mut().find(|group_| group_.id == group) {
            Some(group) if !name.is_empty() && group.name != name => {
                name.clone_into(&mut group.name);
                true
            }
            _ => false,
        }
    }

    /// Closes or opens a group's list. `false` for a group that is not here or already so.
    pub fn set_collapsed(&mut self, group: GroupId, collapsed: bool) -> bool {
        match self.groups.iter_mut().find(|group_| group_.id == group) {
            Some(group) if group.collapsed != collapsed => {
                group.collapsed = collapsed;
                true
            }
            _ => false,
        }
    }

    fn expand(&mut self, group: GroupId) {
        self.set_collapsed(group, false);
    }

    // ─── Names ───────────────────────────────────────────────────────────

    /// `name` made one that no group has: itself when it is free, otherwise it with ` 2`, ` 3`…
    /// after it (`web` → `web 2`; a `web 2` that is taken becomes `web 3`, not `web 2 2`). Empty
    /// is [`UNTITLED`].
    pub fn unique_name(&self, name: &str) -> String {
        let name = name.trim();
        if name.is_empty() {
            return self.unique_name(UNTITLED);
        }
        let taken = |name: &str| self.groups.iter().any(|group| group.name == name);
        if !taken(name) {
            return name.to_owned();
        }
        let base = match name.rsplit_once(' ') {
            Some((base, number))
                if !base.trim().is_empty()
                    && !number.is_empty()
                    && number.bytes().all(|byte| byte.is_ascii_digit()) =>
            {
                base.trim_end()
            }
            _ => name,
        };
        let mut number = 2;
        loop {
            let candidate = format!("{base} {number}");
            if !taken(&candidate) {
                return candidate;
            }
            number += 1;
        }
    }

    /// The name a group made for a tab with this title starts with ([`name_from`]), made unique.
    pub fn suggest_name(&self, title: &str) -> String {
        self.unique_name(&name_from(title))
    }

    // ─── Selecting several ───────────────────────────────────────────────

    /// What a click on the row of `clicked` comes to, given the tabs selected together now
    /// (`picks`) and the tab on screen — the one ⇧ counts from. Selection lives in **one
    /// cluster**: a ⇧ or ⌘ click on a tab of another cluster than the selection's (than the
    /// screen tab's, while nothing is selected) lets the selection go and acts as a plain click.
    /// ⇧ selects every listed tab of the cluster from the tab on screen to the clicked one,
    /// again from the tab on screen every time; ⌘ puts the clicked one in or takes it out, the
    /// tab on screen counting as selected while nothing is — and a selection that comes down to
    /// the tab on screen alone is no selection.
    pub fn pick(&self, tabs: &Tabs<K>, picks: &[K], click: Click, clicked: K) -> Picked<K> {
        let plain = || Picked {
            picks: Vec::new(),
            open: Some(clicked),
        };
        let Some(shown) = tabs.selected() else {
            return plain();
        };
        let listed = self.numbered(tabs, View::Side);
        if click == Click::Plain || !listed.contains(&clicked) {
            return plain();
        }
        let cluster = picks
            .first()
            .map_or_else(|| self.group_of(shown), |&pick| self.group_of(pick));
        if self.group_of(clicked) != cluster {
            return plain();
        }
        let here: Vec<K> = listed
            .into_iter()
            .filter(|&tab| self.group_of(tab) == cluster)
            .collect();
        let picked = match click {
            Click::Range => {
                let (Some(from), Some(to)) = (
                    here.iter().position(|&tab| tab == shown),
                    here.iter().position(|&tab| tab == clicked),
                ) else {
                    return plain();
                };
                here[from.min(to)..=from.max(to)].to_vec()
            }
            Click::Toggle | Click::Plain => {
                let mut set: Vec<K> = if picks.is_empty() {
                    vec![shown]
                } else {
                    picks.to_vec()
                };
                match set.iter().position(|&tab| tab == clicked) {
                    Some(at) => {
                        set.remove(at);
                    }
                    None => set.push(clicked),
                }
                here.into_iter().filter(|tab| set.contains(tab)).collect()
            }
        };
        Picked {
            picks: if picked == [shown] {
                Vec::new()
            } else {
                picked
            },
            open: None,
        }
    }

    // ─── Saving and restoring ────────────────────────────────────────────

    /// The groups with the tabs they gather, in the order they are listed — what a saved layout
    /// keeps. The ungrouped tabs are what no part names; the cluster of ungrouped tabs remembers
    /// no resident on disk (the first one is entered again).
    pub fn parts(&self, tabs: &Tabs<K>) -> Vec<Part<K>> {
        self.groups
            .iter()
            .map(|group| Part {
                name: group.name.clone(),
                collapsed: group.collapsed,
                resident: group.resident,
                tabs: self.cluster(tabs, Some(group.id)),
            })
            .collect()
    }

    /// The groups a saved layout names over `tabs`, which are already in strip order: each part's
    /// tabs are its members, names kept as saved (not made unique: they were when they were
    /// made). `None` when what is saved is not a consistent grouping of `tabs` ([`Self::check`]):
    /// a tab in two parts or none of this strip, a group with no tab, a blank name, the groups
    /// not standing in the order the strip has, a resident outside its group.
    pub fn from_parts(tabs: &Tabs<K>, parts: Vec<Part<K>>) -> Option<Self> {
        let mut groups = Self::new();
        for part in parts {
            if part.name.trim().is_empty() {
                return None;
            }
            let id = GroupId(groups.next);
            groups.next += 1;
            groups.groups.push(Group {
                id,
                name: part.name,
                collapsed: part.collapsed,
                resident: part.resident,
            });
            groups
                .members
                .extend(part.tabs.into_iter().map(|tab| (tab, id)));
        }
        groups.check(tabs).ok()?;
        Some(groups)
    }

    // ─── The invariant ───────────────────────────────────────────────────

    /// Whether the groups and the order agree: the strip is the ungrouped tabs, then each group's
    /// tabs next to each other in the groups' order; no group is empty or has an identity twice;
    /// every member is a tab of the strip and in one group only; every remembered tab is in the
    /// cluster that remembers it. Every operation here leaves it true. The reason is what fails.
    pub fn check(&self, tabs: &Tabs<K>) -> Result<(), String> {
        for (at, group) in self.groups.iter().enumerate() {
            if self.groups[..at].iter().any(|other| other.id == group.id) {
                return Err(format!("group {} is listed twice", group.id.0));
            }
            if !self.members.iter().any(|&(_, id)| id == group.id) {
                return Err(format!("group {} is empty", group.id.0));
            }
        }
        for (at, &(tab, id)) in self.members.iter().enumerate() {
            if tabs.index_of(tab).is_none() {
                return Err("a member is not in the strip".to_owned());
            }
            if self.position(id).is_none() {
                return Err("a member is in a group that is not here".to_owned());
            }
            if self.members[..at].iter().any(|&(other, _)| other == tab) {
                return Err("a tab is a member twice".to_owned());
            }
        }
        let mut last = 0;
        for &tab in tabs.ids() {
            let rank = self.rank(self.group_of(tab));
            if rank < last {
                return Err(
                    "the strip is not the ungrouped tabs and then the groups in order, \
                            each group's tabs together"
                        .to_owned(),
                );
            }
            last = rank;
        }
        for of in self.clusters() {
            if let Some(tab) = self.resident(of) {
                if tabs.index_of(tab).is_none() || self.group_of(tab) != of {
                    return Err("a cluster remembers a tab that is not in it".to_owned());
                }
            }
        }
        Ok(())
    }

    // ─── Keeping them in step ────────────────────────────────────────────

    fn create(&mut self, name: &str) -> GroupId {
        let id = GroupId(self.next);
        self.next += 1;
        let name = self.unique_name(name);
        self.groups.push(Group {
            id,
            name,
            collapsed: false,
            resident: None,
        });
        id
    }

    /// The target of a tab let go at place `at` of `list` (the strip without the tab, when it is a
    /// tab that moves): the cluster of the tab at `at`, before it; at the end, the last tab's
    /// cluster, after its last tab; an empty list is the ungrouped tabs.
    fn landing(&self, list: &[K], at: usize) -> Target<K> {
        match list.get(at) {
            Some(&before) => match self.group_of(before) {
                Some(group) => Target::Group {
                    group,
                    before: Some(before),
                },
                None => Target::Loose {
                    before: Some(before),
                },
            },
            None => match list.last().and_then(|&last| self.group_of(last)) {
                Some(group) => Target::Group {
                    group,
                    before: None,
                },
                None => Target::Loose { before: None },
            },
        }
    }

    /// Puts a tab at place `gap` of the strip, in the cluster [`Self::landing`] names; the
    /// selection stays on its tab.
    fn put_at(&mut self, tabs: &mut Tabs<K>, tab: K, gap: usize) {
        let rest: Vec<K> = tabs
            .ids()
            .iter()
            .copied()
            .filter(|&other| other != tab)
            .collect();
        let target = self.landing(&rest, gap.min(rest.len()));
        if tabs.index_of(tab).is_none() {
            tabs.place_new(tab, tabs.len());
        }
        self.place(tabs, &[tab], target);
    }

    /// The strip's length up to and including a cluster: where a tab put at its end goes.
    fn end_of(&self, tabs: &Tabs<K>, of: Option<GroupId>) -> usize {
        let rank = self.rank(of);
        tabs.ids()
            .iter()
            .filter(|&&tab| self.rank(self.group_of(tab)) <= rank)
            .count()
    }

    /// The strip's tabs the way the groups say they stand, with `moving` — which the groups
    /// already say are in cluster `into` — put in it before `before`, or last.
    fn arranged(
        &self,
        tabs: &Tabs<K>,
        moving: &[K],
        into: Option<GroupId>,
        before: Option<K>,
    ) -> Vec<K> {
        let mut order = Vec::with_capacity(tabs.len());
        for of in self.clusters() {
            let mut list: Vec<K> = tabs
                .ids()
                .iter()
                .copied()
                .filter(|tab| !moving.contains(tab) && self.group_of(*tab) == of)
                .collect();
            if of == into {
                let at = before
                    .and_then(|before| list.iter().position(|&tab| tab == before))
                    .unwrap_or(list.len());
                list.splice(at..at, moving.iter().copied());
            }
            order.extend(list);
        }
        order
    }

    /// Brings the strip to `order`, a rearrangement of it, leaving the selection on its tab.
    fn apply(tabs: &mut Tabs<K>, order: &[K]) {
        debug_assert_eq!(order.len(), tabs.len());
        for (at, &tab) in order.iter().enumerate() {
            tabs.move_to(tab, at);
        }
    }

    /// The selected tab is what its cluster was last on.
    fn settle(&mut self, tabs: &Tabs<K>) {
        let Some(shown) = tabs.selected() else {
            return;
        };
        match self.group_of(shown) {
            Some(id) => {
                if let Some(group) = self.groups.iter_mut().find(|group| group.id == id) {
                    group.resident = Some(shown);
                }
            }
            None => self.loose = Some(shown),
        }
    }

    /// Cluster `of` no longer remembers `tab`, which left it.
    fn forget_in(&mut self, of: Option<GroupId>, tab: K) {
        match of {
            Some(id) => {
                if let Some(group) = self.groups.iter_mut().find(|group| group.id == id) {
                    if group.resident == Some(tab) {
                        group.resident = None;
                    }
                }
            }
            None => {
                if self.loose == Some(tab) {
                    self.loose = None;
                }
            }
        }
    }

    fn forget(&mut self, gone: &[K]) {
        for group in &mut self.groups {
            if group.resident.is_some_and(|tab| gone.contains(&tab)) {
                group.resident = None;
            }
        }
        if self.loose.is_some_and(|tab| gone.contains(&tab)) {
            self.loose = None;
        }
    }

    /// A group with no tab is not kept.
    fn drop_empty(&mut self) {
        let members = &self.members;
        self.groups
            .retain(|group| members.iter().any(|&(_, id)| id == group.id));
    }
}

/// Where one thing of the side column's list stands, and where the zone of the place below it
/// begins ([`Groups::side_spots`]).
struct Frame {
    top: f64,
    bottom: f64,
    /// A row's middle, an open header's middle, a closed header's top: the pointer is over the
    /// place before the thing above this point and the place after it below.
    split: f64,
}

/// The frames of `items` in `column`, in the list's order; `None` when the column does not have as
/// many rows and headers as the list has tabs and headers.
fn item_frames<K>(items: &[Item<K>], column: &Column) -> Option<Vec<Frame>> {
    let (mut rows, mut heads) = (column.rows.iter(), column.headers.iter());
    let frames: Option<Vec<Frame>> = items
        .iter()
        .map(|item| match item {
            Item::Tab(_) => rows.next().map(|row| Frame {
                top: row.y,
                bottom: row.y + row.height,
                split: row.y + row.height / 2.0,
            }),
            Item::Header(_) => heads.next().map(|head| Frame {
                top: head.rect.y,
                bottom: head.rect.y + head.rect.height,
                split: head.rect.y,
            }),
        })
        .collect();
    (rows.next().is_none() && heads.next().is_none() && !items.is_empty()).then_some(frames?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tabs 1…8 with api = 1 2 3 and web = 4 5, the rest ungrouped; 1 is selected. The strip
    /// reads `6 7 8 | 1 2 3 | 4 5`.
    fn scene() -> (Tabs<u32>, Groups<u32>, GroupId, GroupId) {
        let mut tabs = Tabs::new(1);
        for tab in 2..=8 {
            tabs.append(tab);
        }
        let mut groups = Groups::new();
        let api = groups
            .place(&mut tabs, &[1, 2, 3], Target::New("api".into()))
            .flatten()
            .unwrap();
        let web = groups
            .place(&mut tabs, &[4, 5], Target::New("web".into()))
            .flatten()
            .unwrap();
        (tabs, groups, api, web)
    }

    fn ok(groups: &Groups<u32>, tabs: &Tabs<u32>) {
        if let Err(why) = groups.check(tabs) {
            panic!("{why}: {tabs:?} {groups:?}");
        }
    }

    #[test]
    fn a_new_group_takes_its_tabs_and_stands_last() {
        let (tabs, groups, api, web) = scene();
        assert_eq!(tabs.ids(), [6, 7, 8, 1, 2, 3, 4, 5]);
        assert_eq!(
            groups
                .groups()
                .iter()
                .map(|group| group.name.as_str())
                .collect::<Vec<_>>(),
            ["api", "web"]
        );
        assert_eq!(groups.cluster(&tabs, Some(api)), [1, 2, 3]);
        assert_eq!(groups.cluster(&tabs, Some(web)), [4, 5]);
        assert_eq!(groups.cluster(&tabs, None), [6, 7, 8]);
        assert_eq!(tabs.selected(), Some(1), "the selection stays on its tab");
        assert_eq!(groups.shown(&tabs), Some(api));
        ok(&groups, &tabs);
    }

    #[test]
    fn a_new_tab_opens_beside_the_shown_one_in_its_group() {
        let (mut tabs, mut groups, api, web) = scene();
        groups.select(&mut tabs, 2);
        groups.open(&mut tabs, 9);
        assert_eq!(tabs.ids(), [6, 7, 8, 1, 2, 9, 3, 4, 5]);
        assert_eq!(groups.group_of(9), Some(api));
        assert_eq!(tabs.selected(), Some(9));
        ok(&groups, &tabs);

        // Beside the last tab of the whole strip, which is the last of web.
        groups.select(&mut tabs, 5);
        groups.open(&mut tabs, 10);
        assert_eq!(tabs.ids(), [6, 7, 8, 1, 2, 9, 3, 4, 5, 10]);
        assert_eq!(groups.group_of(10), Some(web));
        ok(&groups, &tabs);

        // Beside the last ungrouped tab: still among them, before the groups.
        groups.select(&mut tabs, 8);
        groups.open(&mut tabs, 11);
        assert_eq!(&tabs.ids()[..5], [6, 7, 8, 11, 1]);
        assert_eq!(groups.group_of(11), None);
        ok(&groups, &tabs);

        // A tab that is here is selected, not added twice.
        groups.open(&mut tabs, 3);
        assert_eq!((tabs.len(), tabs.selected()), (11, Some(3)));
    }

    #[test]
    fn a_closed_group_opens_for_a_new_tab_beside_its_tab() {
        let (mut tabs, mut groups, api, _) = scene();
        groups.select(&mut tabs, 2);
        groups.set_collapsed(api, true);
        groups.open(&mut tabs, 9);
        assert!(!groups.group(api).unwrap().collapsed);
        assert_eq!(groups.group_of(9), Some(api));
    }

    #[test]
    fn the_plus_on_a_header_opens_a_tab_at_the_groups_end() {
        let (mut tabs, mut groups, api, web) = scene();
        groups.set_collapsed(web, true);
        groups.open_in(&mut tabs, 9, api);
        assert_eq!(tabs.ids(), [6, 7, 8, 1, 2, 3, 9, 4, 5]);
        assert_eq!(groups.group_of(9), Some(api));
        groups.open_in(&mut tabs, 10, web);
        assert_eq!(tabs.ids().last(), Some(&10));
        assert!(!groups.group(web).unwrap().collapsed, "it opens");
        assert_eq!(tabs.selected(), Some(10));
        ok(&groups, &tabs);
    }

    #[test]
    fn new_ungrouped_and_new_group_tabs_go_where_their_cluster_is() {
        let (mut tabs, mut groups, _, _) = scene();
        groups.open_loose(&mut tabs, 9);
        assert_eq!(tabs.ids(), [6, 7, 8, 9, 1, 2, 3, 4, 5]);
        assert_eq!(groups.group_of(9), None);
        let fresh = groups.open_in_new_group(&mut tabs, 10, "docs");
        assert_eq!(tabs.ids().last(), Some(&10));
        assert_eq!(groups.group(fresh).unwrap().name, "docs");
        assert_eq!(groups.position(fresh), Some(2), "after the others");
        assert_eq!(tabs.selected(), Some(10));
        ok(&groups, &tabs);
    }

    #[test]
    fn closing_inside_a_group_stays_in_the_group() {
        let (mut tabs, mut groups, api, _) = scene();
        groups.select(&mut tabs, 2);
        groups.close(&mut tabs, &[2]);
        assert_eq!(tabs.selected(), Some(3), "the next tab of the group");
        groups.close(&mut tabs, &[3]);
        assert_eq!(
            tabs.selected(),
            Some(1),
            "the last of a group goes to the one before it, not to the next group's first"
        );
        assert_eq!(groups.cluster(&tabs, Some(api)), [1]);
        ok(&groups, &tabs);
        // A tab closed in the background leaves the selection alone.
        groups.select(&mut tabs, 6);
        assert_eq!(groups.close(&mut tabs, &[5, 99]), 1);
        assert_eq!(tabs.selected(), Some(6));
    }

    /// Tabs 1…6: ungrouped 6, a = 1, b = 2 3, c = 4 5. Standing on 3 in b, then on 5 in c, then
    /// on 1 in a.
    fn three() -> (Tabs<u32>, Groups<u32>) {
        let mut tabs = Tabs::new(1);
        for tab in 2..=6 {
            tabs.append(tab);
        }
        let mut groups = Groups::new();
        for (name, members) in [("a", [1].as_slice()), ("b", &[2, 3]), ("c", &[4, 5])] {
            groups.place(&mut tabs, members, Target::New(name.into()));
        }
        for tab in [3, 5, 1] {
            groups.select(&mut tabs, tab);
        }
        (tabs, groups)
    }

    #[test]
    fn the_last_tab_of_a_group_goes_on_to_the_next_group_where_it_was_left() {
        let (mut tabs, mut groups) = three();
        assert_eq!(tabs.ids(), [6, 1, 2, 3, 4, 5]);
        groups.close(&mut tabs, &[1]);
        assert_eq!(groups.len(), 2, "the group goes with its last tab");
        assert_eq!(
            tabs.selected(),
            Some(3),
            "the next group, on the tab last stood on there, not its first"
        );
        ok(&groups, &tabs);
    }

    #[test]
    fn with_no_next_group_the_one_before_takes_it_and_then_the_ungrouped_tabs() {
        let (mut tabs, mut groups) = three();
        groups.select(&mut tabs, 4);
        groups.close(&mut tabs, &[4, 5]);
        assert_eq!(
            tabs.selected(),
            Some(3),
            "c was last: the group before it, on its resident"
        );
        ok(&groups, &tabs);

        // Only the ungrouped tabs are before a: they take the window.
        let (mut tabs, mut groups) = three();
        groups.close(&mut tabs, &[2, 3, 4, 5]);
        assert_eq!(groups.len(), 1);
        groups.close(&mut tabs, &[1]);
        assert_eq!(groups.len(), 0);
        assert_eq!(tabs.ids(), [6]);
        assert_eq!(tabs.selected(), Some(6));
        ok(&groups, &tabs);
    }

    #[test]
    fn the_last_ungrouped_tab_goes_on_to_the_first_group() {
        let (mut tabs, mut groups) = three();
        groups.select(&mut tabs, 6);
        groups.close(&mut tabs, &[6]);
        assert_eq!(tabs.selected(), Some(1), "a's only tab");
        ok(&groups, &tabs);
    }

    #[test]
    fn the_last_tab_of_the_window_leaves_none_selected() {
        let (mut tabs, mut groups) = three();
        groups.close(&mut tabs, &[1, 2, 3, 4, 5, 6]);
        assert_eq!(
            (tabs.is_empty(), tabs.selected(), groups.len()),
            (true, None, 0)
        );
        ok(&groups, &tabs);
    }

    #[test]
    fn closing_several_comes_forward_on_what_stays() {
        let (mut tabs, mut groups, _, _) = scene();
        groups.select(&mut tabs, 2);
        groups.close(&mut tabs, &[2, 3]);
        assert_eq!(tabs.selected(), Some(1));
        let (mut tabs, mut groups, _, _) = scene();
        groups.select(&mut tabs, 1);
        groups.close(&mut tabs, &[1, 2]);
        assert_eq!(tabs.selected(), Some(3));
        assert_eq!(groups.len(), 2);
    }

    #[test]
    fn fronting_names_the_tab_that_stays_on_screen() {
        let (mut tabs, mut groups, _, _) = scene();
        groups.select(&mut tabs, 2);
        assert_eq!(groups.fronting(&tabs, &[7]), Some(2), "it does not leave");
        assert_eq!(groups.fronting(&tabs, &[2]), Some(3));
        assert_eq!(groups.fronting(&tabs, &[2, 3]), Some(1));
        assert_eq!(
            groups.fronting(&tabs, &[1, 2, 3]),
            Some(4),
            "the cluster is gone: the next one's first, nobody having stood on web"
        );
        assert_eq!(groups.fronting(&tabs, &[1, 2, 3, 4, 5, 6, 7, 8]), None);
    }

    #[test]
    fn a_window_without_groups_is_the_plain_list() {
        // The same walk on a bare `Tabs` and with `Groups` beside it, step by step.
        let mut rng = Rng(7);
        let mut plain = Tabs::new(0u32);
        let mut tabs = Tabs::new(0u32);
        let mut groups = Groups::new();
        let mut next = 1;
        for step in 0..400 {
            let ids = plain.ids().to_vec();
            match rng.below(4) {
                0 | 1 => {
                    plain.insert(next);
                    groups.open(&mut tabs, next);
                    next += 1;
                }
                2 if ids.len() > 1 => {
                    let tab = ids[rng.below(ids.len())];
                    plain.close(tab);
                    groups.close(&mut tabs, &[tab]);
                }
                _ => {
                    let tab = ids[rng.below(ids.len())];
                    plain.select(tab);
                    groups.select(&mut tabs, tab);
                }
            }
            assert_eq!(
                (tabs.ids(), tabs.selected()),
                (plain.ids(), plain.selected()),
                "step {step}"
            );
            assert_eq!(
                groups.numbered(&tabs, View::Side),
                plain.ids(),
                "step {step}"
            );
            assert_eq!(groups.numbered(&tabs, View::Top), plain.ids());
        }
    }

    #[test]
    fn a_window_without_groups_places_tabs_as_the_plain_list_does() {
        let mut rng = Rng(11);
        let mut plain = Tabs::new(0u32);
        let mut tabs = Tabs::new(0u32);
        let mut groups = Groups::new();
        let mut next = 1;
        for step in 0..600 {
            let ids = plain.ids().to_vec();
            let gap = rng.below(ids.len() + 2);
            match rng.below(6) {
                0 => {
                    plain.insert_at(next, gap);
                    groups.insert_at(&mut tabs, next, gap);
                    next += 1;
                }
                1 => {
                    assert_eq!(
                        plain.place_new(next, gap),
                        groups.place_new(&mut tabs, next, gap)
                    );
                    next += 1;
                }
                2 => {
                    assert_eq!(plain.append(next), groups.append(&mut tabs, next));
                    next += 1;
                }
                3 => {
                    let tab = ids[rng.below(ids.len())];
                    assert_eq!(
                        plain.move_to(tab, gap),
                        groups.move_to(&mut tabs, tab, gap),
                        "step {step}"
                    );
                }
                4 if ids.len() > 2 => {
                    let source = ids[rng.below(ids.len())];
                    let target = ids[rng.below(ids.len())];
                    assert_eq!(
                        plain.pane_to_tab(source, true, target),
                        groups.pane_to_tab(&mut tabs, source, true, target),
                        "step {step}"
                    );
                }
                _ => {
                    plain.insert(next);
                    groups.open(&mut tabs, next);
                    next += 1;
                }
            }
            assert_eq!(
                (tabs.ids(), tabs.selected()),
                (plain.ids(), plain.selected()),
                "step {step}"
            );
        }
    }

    /// A strip place names no cluster, so the tab joins the one of the tab it goes before — the
    /// start of a group is that group's, not the end of the ungrouped tabs.
    #[test]
    fn a_tab_let_go_on_the_strip_joins_the_cluster_it_goes_before() {
        // The strip reads `6 7 8 | 1 2 3 | 4 5`.
        for (gap, cluster, expect) in [
            (0, "ungrouped", vec![9, 6, 7, 8]),
            (2, "ungrouped", vec![6, 7, 9, 8]),
            (3, "api", vec![9, 1, 2, 3]),
            (4, "api", vec![1, 9, 2, 3]),
            (8, "web", vec![4, 5, 9]),
            (99, "web", vec![4, 5, 9]),
        ] {
            let (mut tabs, mut groups, api, web) = scene();
            groups.insert_at(&mut tabs, 9, gap);
            ok(&groups, &tabs);
            let of = match cluster {
                "api" => Some(api),
                "web" => Some(web),
                _ => None,
            };
            assert_eq!(groups.cluster(&tabs, of), expect, "gap {gap}");
            assert_eq!(
                tabs.selected(),
                Some(9),
                "gap {gap}: it is the tab looked at"
            );
        }
        // A tab that is made for a place (a pane that became a tab) leaves the screen alone.
        let (mut tabs, mut groups, api, _) = scene();
        assert!(groups.place_new(&mut tabs, 9, 4));
        assert!(!groups.place_new(&mut tabs, 9, 4));
        assert_eq!(groups.cluster(&tabs, Some(api)), [1, 9, 2, 3]);
        assert_eq!(tabs.selected(), Some(1));
        ok(&groups, &tabs);
    }

    #[test]
    fn a_tab_joining_at_the_end_stands_among_the_ungrouped_ones() {
        let (mut tabs, mut groups, _, _) = scene();
        assert!(groups.append(&mut tabs, 9));
        assert!(!groups.append(&mut tabs, 9));
        assert_eq!(tabs.ids(), [6, 7, 8, 9, 1, 2, 3, 4, 5]);
        assert_eq!(tabs.selected(), Some(1), "the window shows what it showed");
        ok(&groups, &tabs);
    }

    #[test]
    fn a_tab_dragged_along_the_strip_changes_cluster_where_it_lands() {
        // `6 7 8 | 1 2 3 | 4 5`; 1 is selected.
        let (mut tabs, mut groups, api, web) = scene();
        assert!(groups.move_to(&mut tabs, 6, 4));
        assert_eq!(groups.cluster(&tabs, Some(api)), [1, 2, 6, 3]);
        ok(&groups, &tabs);
        assert!(groups.move_to(&mut tabs, 1, 0));
        assert_eq!(groups.group_of(1), None);
        assert_eq!(tabs.ids(), [1, 7, 8, 2, 6, 3, 4, 5]);
        assert_eq!(groups.cluster(&tabs, None), [1, 7, 8]);
        assert_eq!(tabs.selected(), Some(1), "the selection stays on its tab");
        ok(&groups, &tabs);
        // To the very end: the last tab's group, after its last tab.
        assert!(groups.move_to(&mut tabs, 4, 99));
        assert_eq!(groups.cluster(&tabs, Some(web)), [5, 4]);
        // Where it already is: nothing changed.
        let before = (tabs.clone(), groups.clone());
        assert!(!groups.move_to(&mut tabs, 7, 1));
        assert_eq!((tabs.clone(), groups.clone()), before);
        assert!(
            !groups.move_to(&mut Tabs::new(1), 2, 0),
            "a tab that is not here"
        );
    }

    #[test]
    fn a_tab_folded_into_another_takes_its_group_with_it_when_it_was_the_last() {
        let (mut tabs, mut groups, api, _) = scene();
        // The selected tab folds: the screen goes to the tab its pane went to.
        assert!(groups.pane_to_tab(&mut tabs, 1, true, 4));
        assert_eq!(tabs.selected(), Some(4));
        assert_eq!(groups.cluster(&tabs, Some(api)), [2, 3]);
        ok(&groups, &tabs);
        // Not emptied, itself, or not here: nothing.
        assert!(!groups.pane_to_tab(&mut tabs, 2, false, 3));
        assert!(!groups.pane_to_tab(&mut tabs, 2, true, 2));
        assert!(!groups.pane_to_tab(&mut tabs, 2, true, 99));
        // The last tab of a group folding away ends the group.
        assert!(groups.pane_to_tab(&mut tabs, 2, true, 3));
        assert!(groups.pane_to_tab(&mut tabs, 3, true, 4));
        assert_eq!(groups.len(), 1, "api is gone");
        ok(&groups, &tabs);
    }

    #[test]
    fn the_parts_of_a_grouping_build_it_again() {
        let (tabs, mut groups, _, web) = scene();
        groups.set_collapsed(web, true);
        groups.rename(web, "api");
        let parts = groups.parts(&tabs);
        assert_eq!(
            parts
                .iter()
                .map(|part| (
                    part.name.as_str(),
                    part.collapsed,
                    part.resident,
                    part.tabs.clone()
                ))
                .collect::<Vec<_>>(),
            [
                ("api", false, Some(1), vec![1, 2, 3]),
                ("api", true, None, vec![4, 5])
            ],
            "names are kept as they are, even two alike"
        );
        assert_eq!(Groups::from_parts(&tabs, parts), Some(groups));
        assert_eq!(
            Groups::from_parts(&tabs, Vec::new()),
            Some(Groups::new()),
            "no group is a plain list"
        );
    }

    #[test]
    fn parts_that_are_no_grouping_of_the_strip_build_nothing() {
        let (tabs, groups, _, _) = scene();
        let good = groups.parts(&tabs);
        let rejected = |mutate: &dyn Fn(&mut Vec<Part<u32>>)| {
            let mut parts = good.clone();
            mutate(&mut parts);
            Groups::from_parts(&tabs, parts)
        };
        assert!(rejected(&|_| {}).is_some());
        assert!(
            rejected(&|parts| parts[0].name = " \t".into()).is_none(),
            "a blank name"
        );
        assert!(
            rejected(&|parts| parts[0].tabs.push(99)).is_none(),
            "a tab not in the strip"
        );
        assert!(
            rejected(&|parts| parts[1].tabs.push(1)).is_none(),
            "a tab in two groups"
        );
        assert!(
            rejected(&|parts| parts[1].tabs.clear()).is_none(),
            "a group with no tab"
        );
        assert!(
            rejected(&|parts| parts.swap(0, 1)).is_none(),
            "groups against the strip's order"
        );
        assert!(
            rejected(&|parts| parts[0].resident = Some(4)).is_none(),
            "a resident of another"
        );
        assert!(
            rejected(&|parts| parts[0].resident = Some(7)).is_none(),
            "a resident ungrouped"
        );
    }

    #[test]
    fn tabs_move_between_clusters_in_their_own_order() {
        let (mut tabs, mut groups, api, _) = scene();
        // Into a group, before one of its tabs; named out of order.
        groups.place(
            &mut tabs,
            &[8, 6],
            Target::Group {
                group: api,
                before: Some(2),
            },
        );
        assert_eq!(tabs.ids(), [7, 1, 6, 8, 2, 3, 4, 5]);
        assert_eq!(groups.cluster(&tabs, Some(api)), [1, 6, 8, 2, 3]);
        ok(&groups, &tabs);

        // Out of its group: after the last ungrouped tab.
        groups.place(&mut tabs, &[2], Target::Loose { before: None });
        assert_eq!(tabs.ids(), [7, 2, 1, 6, 8, 3, 4, 5]);
        // Out of it between two ungrouped tabs.
        groups.place(&mut tabs, &[4], Target::Loose { before: Some(2) });
        assert_eq!(tabs.ids(), [7, 4, 2, 1, 6, 8, 3, 5]);
        assert_eq!(groups.group_of(4), None);
        ok(&groups, &tabs);

        // A tab named that is not here, and a group that is not, change nothing.
        let before = (tabs.clone(), groups.clone());
        assert_eq!(
            groups.place(&mut tabs, &[99], Target::New("x".into())),
            None
        );
        assert_eq!(
            groups.place(
                &mut tabs,
                &[7],
                Target::Group {
                    group: GroupId(99),
                    before: None
                }
            ),
            None
        );
        assert_eq!((tabs.clone(), groups.clone()), before);
    }

    #[test]
    fn a_group_emptied_by_a_move_goes() {
        let (mut tabs, mut groups, api, web) = scene();
        groups.place(
            &mut tabs,
            &[4, 5],
            Target::Group {
                group: api,
                before: None,
            },
        );
        assert!(groups.group(web).is_none());
        assert_eq!(groups.cluster(&tabs, Some(api)), [1, 2, 3, 4, 5]);
        ok(&groups, &tabs);

        let made = groups
            .place(&mut tabs, &[7, 8], Target::New("web".into()))
            .flatten()
            .unwrap();
        assert_eq!(tabs.ids(), [6, 1, 2, 3, 4, 5, 7, 8]);
        assert_eq!(groups.group(made).unwrap().name, "web");
        ok(&groups, &tabs);
    }

    #[test]
    fn ungrouping_gives_the_tabs_back_after_the_ungrouped_ones() {
        let (mut tabs, mut groups, api, _) = scene();
        assert!(groups.ungroup(&mut tabs, api));
        assert_eq!(tabs.ids(), [6, 7, 8, 1, 2, 3, 4, 5]);
        assert_eq!(groups.cluster(&tabs, None), [6, 7, 8, 1, 2, 3]);
        assert_eq!(groups.len(), 1);
        assert!(!groups.ungroup(&mut tabs, api), "it is gone");
        ok(&groups, &tabs);
    }

    #[test]
    fn a_group_moves_among_the_groups_with_its_tabs() {
        let (mut tabs, mut groups, api, web) = scene();
        let docs = groups.open_in_new_group(&mut tabs, 9, "docs");
        assert!(groups.move_group(&mut tabs, docs, 0));
        assert_eq!(tabs.ids(), [6, 7, 8, 9, 1, 2, 3, 4, 5]);
        assert_eq!(groups.position(docs), Some(0));
        assert!(groups.move_group(&mut tabs, api, 99));
        assert_eq!(tabs.ids(), [6, 7, 8, 9, 4, 5, 1, 2, 3]);
        assert_eq!(groups.position(api), Some(2));
        assert_eq!(groups.position(web), Some(1));
        assert!(!groups.move_group(&mut tabs, api, 2), "already there");
        assert!(!groups.move_group(&mut tabs, GroupId(99), 0));
        assert_eq!(tabs.selected(), Some(9), "the selection stays on its tab");
        ok(&groups, &tabs);
    }

    #[test]
    fn a_name_and_the_closed_state_change_only_when_they_differ() {
        let (_, mut groups, api, _) = scene();
        assert!(groups.rename(api, "  core "));
        assert_eq!(groups.group(api).unwrap().name, "core");
        assert!(!groups.rename(api, "core"));
        assert!(!groups.rename(api, "   "), "emptying it leaves the name");
        assert!(!groups.rename(GroupId(99), "x"));
        assert!(groups.set_collapsed(api, true));
        assert!(!groups.set_collapsed(api, true));
        assert!(!groups.set_collapsed(GroupId(99), true));
    }

    #[test]
    fn a_group_is_named_for_the_folder_of_its_first_tab() {
        assert_eq!(name_from("⇄ db-1"), "db-1");
        assert_eq!(name_from("~"), "home");
        assert_eq!(name_from("⇄ ~"), "home");
        assert_eq!(name_from("~/code/api"), "api");
        assert_eq!(name_from("/var/log/"), "log");
        assert_eq!(name_from("bateri"), "bateri");
        assert_eq!(name_from(""), UNTITLED);
        assert_eq!(name_from("  ⇄  "), UNTITLED);
        assert_eq!(name_from("/"), UNTITLED);

        let (mut tabs, mut groups, _, _) = scene();
        assert_eq!(groups.suggest_name("~/code/web"), "web 2", "web is taken");
        assert_eq!(groups.suggest_name("~/code/docs"), "docs");
        assert_eq!(groups.unique_name("web"), "web 2");
        assert_eq!(groups.unique_name(""), UNTITLED);
        groups.open_in_new_group(&mut tabs, 9, "web 2");
        assert_eq!(groups.unique_name("web"), "web 3");
        assert_eq!(groups.unique_name("web 2"), "web 3", "not `web 2 2`");
        assert_eq!(groups.unique_name("web 7"), "web 7");
        assert_eq!(groups.unique_name("2"), "2", "a number alone is a name");
    }

    #[test]
    fn the_column_lists_headers_and_hides_a_closed_groups_other_tabs() {
        let (mut tabs, mut groups, api, web) = scene();
        let tab_ids = |items: &[Item<u32>]| -> Vec<String> {
            items
                .iter()
                .map(|item| match item {
                    Item::Header(id) => format!("#{}", id.get()),
                    Item::Tab(tab) => tab.to_string(),
                })
                .collect()
        };
        let (a, w) = (api.get(), web.get());
        assert_eq!(
            tab_ids(&groups.side_items(&tabs)),
            [
                "6",
                "7",
                "8",
                &format!("#{a}"),
                "1",
                "2",
                "3",
                &format!("#{w}"),
                "4",
                "5"
            ]
        );
        groups.set_collapsed(web, true);
        assert_eq!(
            tab_ids(&groups.side_items(&tabs)),
            [
                "6",
                "7",
                "8",
                &format!("#{a}"),
                "1",
                "2",
                "3",
                &format!("#{w}")
            ],
            "web is closed and the screen is in api"
        );
        groups.select(&mut tabs, 5);
        groups.set_collapsed(api, true);
        assert_eq!(
            tab_ids(&groups.side_items(&tabs)),
            ["6", "7", "8", &format!("#{a}"), &format!("#{w}"), "5"],
            "a closed group keeps the tab on screen under its header"
        );
        assert_eq!(
            groups
                .side_items(&tabs)
                .iter()
                .map(Item::entry)
                .filter(|entry| *entry == Entry::Header)
                .count(),
            2
        );
    }

    #[test]
    fn a_column_place_is_the_listed_tabs_place_in_the_strip() {
        let (tabs, mut groups, _, web) = scene();
        for listed in 0..tabs.len() {
            assert_eq!(
                groups.listed_place(&tabs, View::Side, listed),
                listed,
                "nothing is closed"
            );
        }
        assert_eq!(
            groups.listed_place(&tabs, View::Side, tabs.len()),
            tabs.len()
        );
        groups.set_collapsed(web, true);
        // 6 7 8 | api 1 2 3 | web (4 5 hidden): the column lists six tabs.
        assert_eq!(
            groups.listed_place(&tabs, View::Side, 5),
            5,
            "tab 3 is the strip's sixth"
        );
        assert_eq!(
            groups.listed_place(&tabs, View::Side, 6),
            tabs.len(),
            "past the last listed tab is the strip's end, behind the hidden ones"
        );
    }

    #[test]
    fn the_bars_listed_place_is_the_clusters_own_in_the_strip() {
        let (mut tabs, groups, api, _) = scene();
        // 6 7 8 | 1 2 3 | 4 5, with api on screen.
        assert_eq!(groups.shown(&tabs), Some(api));
        for (listed, place) in [(0, 3), (1, 4), (2, 5)] {
            assert_eq!(
                groups.listed_place(&tabs, View::Top, listed),
                place,
                "the bar lists api's three tabs, which stand from the strip's fourth place"
            );
        }
        assert_eq!(
            groups.listed_place(&tabs, View::Top, 3),
            6,
            "past the last is right after the cluster's last tab, not the strip's end"
        );
        let mut groups = groups;
        groups.select(&mut tabs, 7);
        assert_eq!(groups.listed_place(&tabs, View::Top, 0), 0);
        assert_eq!(groups.listed_place(&tabs, View::Top, 3), 3);
        groups.select(&mut tabs, 4);
        assert_eq!(
            groups.listed_place(&tabs, View::Top, 2),
            tabs.len(),
            "web is last: after its last tab is the end"
        );
    }

    #[test]
    fn a_tab_moved_along_its_own_cluster_stays_in_it() {
        let (mut tabs, mut groups, api, web) = scene();
        // 6 7 8 | 1 2 3 | 4 5: api's first tab to its end — `move_to` would give it to web.
        assert!(groups.move_within(&mut tabs, 1, 2));
        assert_eq!(tabs.ids(), [6, 7, 8, 2, 3, 1, 4, 5]);
        assert_eq!(groups.group_of(1), Some(api));
        assert_eq!(tabs.selected(), Some(1), "the selection stays on its tab");
        ok(&groups, &tabs);

        let mut flat = tabs.clone();
        let mut other = groups.clone();
        other.move_to(&mut flat, 3, 5);
        assert_eq!(
            flat.ids(),
            [6, 7, 8, 2, 1, 3, 4, 5].as_slice(),
            "the same strip as moving within api to its end, ..."
        );
        assert_eq!(
            other.group_of(3),
            Some(web),
            "... but the strip's place at a cluster's end is the next cluster's"
        );

        // Back to the front, past the end, and to where it is.
        assert!(groups.move_within(&mut tabs, 1, 0));
        assert_eq!(tabs.ids(), [6, 7, 8, 1, 2, 3, 4, 5]);
        assert!(groups.move_within(&mut tabs, 1, 99));
        assert_eq!(
            tabs.ids(),
            [6, 7, 8, 2, 3, 1, 4, 5],
            "past the end is the end"
        );
        assert!(!groups.move_within(&mut tabs, 1, 2), "already there");
        // The ungrouped tabs have a cluster too.
        assert!(groups.move_within(&mut tabs, 6, 2));
        assert_eq!(tabs.ids(), [7, 8, 6, 2, 3, 1, 4, 5]);
        assert_eq!(groups.group_of(6), None);
        assert!(
            !groups.move_within(&mut tabs, 42, 0),
            "a tab that is not here"
        );
        ok(&groups, &tabs);
    }

    #[test]
    fn without_groups_moving_within_is_moving() {
        for tab in 1..=5 {
            for to in 0..6 {
                let mut flat = Tabs::new(1);
                for id in 2..=5 {
                    flat.append(id);
                }
                let mut grouped = flat.clone();
                let mut groups = Groups::new();
                let moved = groups.move_within(&mut grouped, tab, to);
                let there = flat.ids().iter().position(|&id| id == tab).unwrap();
                flat.move_to(tab, to);
                assert_eq!(grouped.ids(), flat.ids(), "tab {tab} to {to}");
                assert_eq!(
                    moved,
                    there != flat.ids().iter().position(|&id| id == tab).unwrap()
                );
            }
        }
    }

    #[test]
    fn command_digits_count_the_tabs_as_they_are_shown() {
        let (mut tabs, mut groups, _, web) = scene();
        groups.set_collapsed(web, true);
        assert_eq!(groups.numbered(&tabs, View::Side), [6, 7, 8, 1, 2, 3]);
        assert_eq!(groups.shortcut(&tabs, View::Side, 4), Some(1));
        assert_eq!(
            groups.shortcut(&tabs, View::Side, 9),
            Some(3),
            "⌘9 is the last"
        );
        assert_eq!(groups.shortcut(&tabs, View::Side, 7), None);
        // In the bar it is the cluster on screen: api's.
        groups.select(&mut tabs, 2);
        assert_eq!(groups.numbered(&tabs, View::Top), [1, 2, 3]);
        assert_eq!(groups.shortcut(&tabs, View::Top, 1), Some(1));
        assert_eq!(groups.shortcut(&tabs, View::Top, 4), None);
        groups.select(&mut tabs, 7);
        assert_eq!(groups.numbered(&tabs, View::Top), [6, 7, 8]);
        // A closed web shows its tab once the screen is there.
        groups.select(&mut tabs, 4);
        assert_eq!(groups.numbered(&tabs, View::Side), [6, 7, 8, 1, 2, 3, 4]);
    }

    #[test]
    fn control_command_digits_reach_a_group_on_the_tab_left_in_it() {
        let (mut tabs, mut groups, api, web) = scene();
        groups.select(&mut tabs, 7);
        assert_eq!(
            groups.reach(&tabs, 0),
            Reach::Tab {
                tab: 7,
                expand: None
            },
            "the ungrouped tabs, on the one last stood on"
        );
        assert_eq!(
            groups.reach(&tabs, 1),
            Reach::Tab {
                tab: 1,
                expand: None
            }
        );
        assert_eq!(
            groups.reach(&tabs, 2),
            Reach::Tab {
                tab: 4,
                expand: None
            },
            "web was never stood on: its first"
        );
        groups.select(&mut tabs, 5);
        groups.select(&mut tabs, 2);
        groups.set_collapsed(web, true);
        assert_eq!(
            groups.reach(&tabs, 2),
            Reach::Tab {
                tab: 5,
                expand: Some(web)
            },
            "a closed group opens, on its resident"
        );
        assert_eq!(
            groups.reach(&tabs, 9),
            groups.reach(&tabs, 2),
            "⌃⌘9 is the last group"
        );
        assert_eq!(groups.reach(&tabs, 3), Reach::Nothing);
        assert_eq!(groups.reach(&tabs, 8), Reach::Nothing);
        assert_eq!(groups.group(api).unwrap().resident(), Some(2));
    }

    #[test]
    fn a_list_row_reaches_its_cluster_by_identity_as_the_digits_do() {
        let (mut tabs, mut groups, api, web) = scene();
        groups.select(&mut tabs, 7);
        for (of, tag) in [(None, 0), (Some(api), 1), (Some(web), 2)] {
            assert_eq!(
                groups.reach_in(&tabs, of),
                groups.reach(&tabs, tag),
                "{of:?} is what ⌃⌘{tag} reaches"
            );
        }
        groups.set_collapsed(web, true);
        assert_eq!(
            groups.reach_in(&tabs, Some(web)),
            Reach::Tab {
                tab: 4,
                expand: Some(web)
            }
        );
        // The row of a group that went since the list was drawn reaches nothing.
        groups.close(&mut tabs, &[4, 5]);
        assert_eq!(groups.reach_in(&tabs, Some(web)), Reach::Nothing);
        // A twelfth group has a row and no key.
        for index in 0..11 {
            groups.open_in_new_group(&mut tabs, 20 + index, &format!("g{index}"));
        }
        let twelfth = groups.groups()[11].id;
        assert_eq!(
            groups.reach_in(&tabs, Some(twelfth)),
            Reach::Tab {
                tab: 30,
                expand: None
            }
        );
    }

    #[test]
    fn control_command_zero_asks_for_a_new_tab_when_nothing_is_ungrouped() {
        let (mut tabs, mut groups, _, _) = scene();
        groups.close(&mut tabs, &[6, 7, 8]);
        assert_eq!(groups.cluster(&tabs, None), Vec::<u32>::new());
        assert_eq!(groups.reach(&tabs, 0), Reach::NewLoose);
        groups.open_loose(&mut tabs, 9);
        assert_eq!(tabs.ids()[0], 9, "at the top of the list");
        assert_eq!(
            groups.reach(&tabs, 0),
            Reach::Tab {
                tab: 9,
                expand: None
            }
        );
        ok(&groups, &tabs);
    }

    #[test]
    fn shift_selects_from_the_screen_tab_and_command_one_by_one() {
        let (mut tabs, mut groups, _, _) = scene();
        let pick = |groups: &Groups<u32>, tabs: &Tabs<u32>, picks: &[u32], click, tab| {
            groups.pick(tabs, picks, click, tab)
        };
        // ⇧ from 1 to 3: the tabs between, the screen tab among them.
        let range = pick(&groups, &tabs, &[], Click::Range, 3);
        assert_eq!((range.picks.as_slice(), range.open), (&[1, 2, 3][..], None));
        // Again, from the screen tab and not from the last click.
        let again = pick(&groups, &tabs, &[1, 2, 3], Click::Range, 2);
        assert_eq!(again.picks, [1, 2]);
        // Upwards, and in the order of the list.
        groups.select(&mut tabs, 3);
        assert_eq!(pick(&groups, &tabs, &[], Click::Range, 1).picks, [1, 2, 3]);
        groups.select(&mut tabs, 1);

        // ⌘: one by one, the screen tab counted while nothing is selected.
        let one = pick(&groups, &tabs, &[], Click::Toggle, 3);
        assert_eq!(one.picks, [1, 3]);
        let two = pick(&groups, &tabs, &one.picks, Click::Toggle, 2);
        assert_eq!(two.picks, [1, 2, 3]);
        let out = pick(&groups, &tabs, &two.picks, Click::Toggle, 1);
        assert_eq!(out.picks, [2, 3], "the screen tab may leave it");
        let alone = pick(&groups, &tabs, &[1, 3], Click::Toggle, 3);
        assert_eq!(
            alone.picks,
            Vec::<u32>::new(),
            "only the screen tab left is no selection"
        );
        let same = pick(&groups, &tabs, &[], Click::Toggle, 1);
        assert_eq!(same.picks, Vec::<u32>::new());

        // The ungrouped tabs are a cluster as well.
        groups.select(&mut tabs, 6);
        assert_eq!(pick(&groups, &tabs, &[], Click::Range, 8).picks, [6, 7, 8]);
    }

    #[test]
    fn a_selection_stays_in_one_cluster() {
        let (mut tabs, mut groups, _, web) = scene();
        // 1 and 2 selected in api: ⇧ or ⌘ on web's 4 lets them go and opens 4.
        for click in [Click::Range, Click::Toggle, Click::Plain] {
            let cross = groups.pick(&tabs, &[1, 2], click, 4);
            assert_eq!(
                (cross.picks.as_slice(), cross.open),
                (&[][..], Some(4)),
                "{click:?}"
            );
        }
        // With nothing selected the screen tab's cluster is the reference.
        let loose = groups.pick(&tabs, &[], Click::Toggle, 6);
        assert_eq!(loose.open, Some(6));
        // A closed group lists one tab, and a click on a tab not listed opens it.
        groups.set_collapsed(web, true);
        let hidden = groups.pick(&tabs, &[], Click::Toggle, 5);
        assert_eq!(hidden.open, Some(5));
        groups.select(&mut tabs, 4);
        let shown = groups.pick(&tabs, &[], Click::Range, 4);
        assert_eq!(shown.picks, Vec::<u32>::new(), "the screen tab alone");
    }

    #[test]
    fn the_check_names_what_is_out_of_order() {
        let (mut tabs, groups, _, _) = scene();
        ok(&groups, &tabs);
        tabs.move_to(4, 0);
        assert!(
            groups
                .check(&tabs)
                .unwrap_err()
                .contains("not the ungrouped")
        );
        let (mut tabs, groups, _, _) = scene();
        tabs.close(1);
        assert!(groups.check(&tabs).is_err(), "a member that is not a tab");
        let (tabs, mut groups, _, _) = scene();
        groups.members.retain(|&(tab, _)| tab != 4 && tab != 5);
        assert!(groups.check(&tabs).unwrap_err().contains("empty"));
    }

    /// A small deterministic generator: the walk below must be the same walk every run.
    struct Rng(u64);

    impl Rng {
        fn below(&mut self, count: usize) -> usize {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((self.0 >> 33) as usize) % count.max(1)
        }
    }

    #[test]
    fn every_operation_keeps_the_strip_and_the_groups_in_step() {
        // The walk must reach states worth checking: several groups at once.
        let most = (1..=40).map(|seed| walk(seed, 300)).max();
        assert!(most >= Some(3), "the walks made at most {most:?} groups");
    }

    /// One random walk of `steps` operations, the invariant asked after each; the most groups
    /// the window had at once.
    fn walk(seed: u64, steps: usize) -> usize {
        let mut rng = Rng(seed);
        let mut tabs = Tabs::new(0u32);
        let mut groups = Groups::new();
        let mut next = 1u32;
        let mut most = 0;
        for step in 0..steps {
            let ids = tabs.ids().to_vec();
            let some = |rng: &mut Rng| ids[rng.below(ids.len())];
            let a_group = |rng: &mut Rng, groups: &Groups<u32>| {
                // Now and then a group that is not there.
                match groups.groups().len() {
                    0 => GroupId(99),
                    count if rng.below(10) == 0 => GroupId(count as u64 + 50),
                    count => groups.groups()[rng.below(count)].id,
                }
            };
            let what = rng.below(20);
            if ids.is_empty() {
                groups.open(&mut tabs, next);
                next += 1;
            } else {
                match what {
                    0 | 1 => {
                        groups.open(&mut tabs, next);
                        next += 1;
                    }
                    2 => {
                        groups.open_loose(&mut tabs, next);
                        next += 1;
                    }
                    3 => {
                        let group = a_group(&mut rng, &groups);
                        groups.open_in(&mut tabs, next, group);
                        next += 1;
                    }
                    4 => {
                        groups.open_in_new_group(
                            &mut tabs,
                            next,
                            if rng.below(3) == 0 { "" } else { "g" },
                        );
                        next += 1;
                    }
                    5 | 6 => {
                        let closing: Vec<u32> =
                            (0..=rng.below(3)).map(|_| some(&mut rng)).collect();
                        groups.close(&mut tabs, &closing);
                    }
                    7 | 8 => {
                        let moving: Vec<u32> = (0..=rng.below(3)).map(|_| some(&mut rng)).collect();
                        let before = (rng.below(2) == 0).then(|| some(&mut rng));
                        let target = match rng.below(3) {
                            0 => Target::Loose { before },
                            1 => Target::Group {
                                group: a_group(&mut rng, &groups),
                                before,
                            },
                            _ => Target::New("n".into()),
                        };
                        groups.place(&mut tabs, &moving, target);
                    }
                    9 => {
                        let group = a_group(&mut rng, &groups);
                        groups.ungroup(&mut tabs, group);
                    }
                    10 => {
                        let group = a_group(&mut rng, &groups);
                        groups.move_group(&mut tabs, group, rng.below(5));
                    }
                    11 => {
                        let tab = some(&mut rng);
                        groups.select(&mut tabs, tab);
                    }
                    12 => {
                        let group = a_group(&mut rng, &groups);
                        let closed = rng.below(2) == 0;
                        groups.set_collapsed(group, closed);
                        groups.rename(group, "renamed");
                    }
                    14 => {
                        let gap = rng.below(ids.len() + 2);
                        groups.insert_at(&mut tabs, next, gap);
                        next += 1;
                    }
                    15 => {
                        let gap = rng.below(ids.len() + 2);
                        groups.place_new(&mut tabs, next, gap);
                        next += 1;
                    }
                    16 => {
                        groups.append(&mut tabs, next);
                        next += 1;
                    }
                    17 => {
                        let tab = some(&mut rng);
                        groups.move_to(&mut tabs, tab, rng.below(ids.len() + 1));
                    }
                    18 => {
                        let (source, target) = (some(&mut rng), some(&mut rng));
                        groups.pane_to_tab(&mut tabs, source, true, target);
                    }
                    19 => {
                        let tab = some(&mut rng);
                        let cluster = groups.group_of(tab);
                        groups.move_within(&mut tabs, tab, rng.below(ids.len() + 1));
                        assert_eq!(groups.group_of(tab), cluster, "it stays in its cluster");
                    }
                    _ => {
                        if let Reach::Tab { tab, expand } = groups.reach(&tabs, rng.below(10) as u8)
                        {
                            if let Some(group) = expand {
                                groups.set_collapsed(group, false);
                            }
                            groups.select(&mut tabs, tab);
                        }
                    }
                }
            }
            if let Err(why) = groups.check(&tabs) {
                panic!("seed {seed}, step {step} (op {what}): {why}\n{tabs:?}\n{groups:?}");
            }
            // What is listed is what is there: every tab once at most, a header per group.
            let items = groups.side_items(&tabs);
            let headers = items
                .iter()
                .filter(|item| matches!(item, Item::Header(_)))
                .count();
            assert_eq!(headers, groups.len(), "seed {seed}, step {step}");
            let listed = groups.numbered(&tabs, View::Side);
            assert!(listed.iter().all(|tab| tabs.index_of(*tab).is_some()));
            if let Some(shown) = tabs.selected() {
                assert!(listed.contains(&shown), "the screen tab is always listed");
                assert!(groups.numbered(&tabs, View::Top).contains(&shown));
            }
            // The column's layout takes the list as it is.
            let layout = tabs::Side {
                width: tabs::SIDE_WIDTH,
                height: 600.0,
                top: 40.0,
                count: 0,
                scroll: 0.0,
                warning: false,
                corner: 20.5,
                row_height: tabs::TAB_HEIGHT,
            }
            .layout_with(&items.iter().map(Item::entry).collect::<Vec<_>>());
            assert_eq!(layout.rows.len(), listed.len());
            assert_eq!(layout.headers.len(), groups.len());
            most = most.max(groups.len());
        }
        most
    }

    // ─── Where a drop lands ──────────────────────────────────────────────

    /// The column the list lays out to, as the bar lays it.
    fn column(groups: &Groups<u32>, tabs: &Tabs<u32>) -> Column {
        let items = groups.side_items(tabs);
        tabs::Side {
            width: tabs::SIDE_WIDTH,
            height: 900.0,
            top: 40.0,
            count: 0,
            scroll: 0.0,
            warning: false,
            corner: 20.5,
            row_height: tabs::TAB_HEIGHT,
        }
        .layout_with(&items.iter().map(Item::entry).collect::<Vec<_>>())
    }

    /// Where a drop with the pointer `at` of row `row`'s height (0 its top, 1 its foot) lands.
    fn over_row(
        groups: &Groups<u32>,
        tabs: &Tabs<u32>,
        row: usize,
        at: f64,
    ) -> Option<Target<u32>> {
        let column = column(groups, tabs);
        let y = column.rows[row].y + column.rows[row].height * at;
        spot_at(&groups.side_spots(tabs, &column), y).map(|spot| spot.target.clone())
    }

    fn over_header(groups: &Groups<u32>, tabs: &Tabs<u32>, header: usize, dy: f64) -> Target<u32> {
        let column = column(groups, tabs);
        let y = column.headers[header].rect.y + dy;
        spot_at(&groups.side_spots(tabs, &column), y)
            .unwrap()
            .target
            .clone()
    }

    #[test]
    fn rows_of_one_cluster_name_the_row_they_would_stand_before() {
        // The column lists `6 7 8 | api: 1 2 3 | web: 4 5`; rows 0…7.
        let (tabs, groups, api, _) = scene();
        let group = |before| Target::Group { group: api, before };
        // The upper half of 2's row: between 1 and 2.
        assert_eq!(over_row(&groups, &tabs, 4, 0.25), Some(group(Some(2))));
        // Its lower half: between 2 and 3.
        assert_eq!(over_row(&groups, &tabs, 4, 0.75), Some(group(Some(3))));
        // Above everything, among the ungrouped: before the first.
        let loose = |before| Target::Loose { before };
        let column = column(&groups, &tabs);
        let spots = groups.side_spots(&tabs, &column);
        assert_eq!(
            spot_at(&spots, -100.0).map(|spot| &spot.target),
            Some(&loose(Some(6)))
        );
    }

    #[test]
    fn a_groups_border_has_two_places_its_end_and_the_next_ones_head() {
        let (tabs, groups, api, web) = scene();
        // The lower half of the last row of api (3, row 5) and the gap above web's header: api's
        // end — what a flat place would have given to web's head.
        assert_eq!(
            over_row(&groups, &tabs, 5, 0.75),
            Some(Target::Group {
                group: api,
                before: None
            })
        );
        let column = column(&groups, &tabs);
        let above = column.headers[1].rect.y - 4.0;
        let spots = groups.side_spots(&tabs, &column);
        assert_eq!(
            spot_at(&spots, above).unwrap().target,
            Target::Group {
                group: api,
                before: None
            }
        );
        // The pointer decides by the middle of the header (24 high): over its upper half the gap
        // opens above it, api's end; over its lower half, and on the upper half of web's first
        // row, under it, web's head.
        let head = Target::Group {
            group: web,
            before: Some(4),
        };
        let end = Target::Group {
            group: api,
            before: None,
        };
        assert_eq!(over_header(&groups, &tabs, 1, 2.0), end);
        assert_eq!(over_header(&groups, &tabs, 1, 11.0), end);
        assert_eq!(over_header(&groups, &tabs, 1, 13.0), head);
        assert_eq!(over_header(&groups, &tabs, 1, 22.0), head);
        assert_eq!(over_row(&groups, &tabs, 6, 0.25), Some(head));
        // The two lines are apart: the end's is in the group gap, the head's under the header.
        let top = column.headers[1].rect.y;
        let end = spot_at(&spots, top + 2.0).unwrap().line;
        let head = spot_at(&spots, top + 20.0).unwrap().line;
        assert!(end < top, "above the header");
        assert!(head > top + 24.0 - 1.0, "under the header");
    }

    #[test]
    fn the_ungrouped_tabs_end_above_the_first_header_and_the_last_group_below_its_last_row() {
        let (tabs, groups, _, web) = scene();
        // The lower half of 8's row: the end of the ungrouped.
        assert_eq!(
            over_row(&groups, &tabs, 2, 0.75),
            Some(Target::Loose { before: None })
        );
        // Far below the list: the end of web.
        let column = column(&groups, &tabs);
        let spots = groups.side_spots(&tabs, &column);
        assert_eq!(
            spot_at(&spots, 10_000.0).unwrap().target,
            Target::Group {
                group: web,
                before: None
            }
        );
    }

    #[test]
    fn a_closed_group_takes_a_drop_on_its_header_at_its_end() {
        let (mut tabs, mut groups, api, web) = scene();
        groups.set_collapsed(web, true);
        // 1 is on screen in api: web lists no row. The header is the last thing in the list.
        let items = groups.side_items(&tabs);
        assert_eq!(items.last(), Some(&Item::Header(web)));
        assert_eq!(
            over_header(&groups, &tabs, 1, 4.0),
            Target::Group {
                group: web,
                before: None
            }
        );
        // The gap above it is still api's end.
        let column = column(&groups, &tabs);
        let spots = groups.side_spots(&tabs, &column);
        assert_eq!(
            spot_at(&spots, column.headers[1].rect.y - 3.0)
                .unwrap()
                .target,
            Target::Group {
                group: api,
                before: None
            }
        );
        // With web's second tab on screen the header lists it, and the head is before web's first.
        groups.select(&mut tabs, 5);
        assert_eq!(
            over_header(&groups, &tabs, 1, 4.0),
            Target::Group {
                group: web,
                before: Some(4)
            }
        );
    }

    #[test]
    fn a_column_that_is_not_the_list_has_nowhere_to_drop() {
        let (tabs, groups, _, _) = scene();
        let mut other = groups.clone();
        other.set_collapsed(other.groups()[1].id, true);
        let stale = column(&other, &tabs);
        // 1 on screen in api: closing web takes its two rows out of the list.
        assert!(groups.side_spots(&tabs, &stale).is_empty());
        assert!(groups.group_spots(&tabs, &stale).is_empty());
        assert_eq!(spot_at::<u32>(&[], 5.0), None);
    }

    #[test]
    fn a_tab_let_go_before_itself_stays_and_one_before_a_moving_tab_stands_before_the_next() {
        let (mut tabs, mut groups, api, _) = scene();
        groups.place(
            &mut tabs,
            &[2],
            Target::Group {
                group: api,
                before: Some(2),
            },
        );
        assert_eq!(tabs.ids(), [6, 7, 8, 1, 2, 3, 4, 5]);
        // Two rows let go before the first of them: before the one that stays.
        groups.place(
            &mut tabs,
            &[1, 2],
            Target::Group {
                group: api,
                before: Some(2),
            },
        );
        assert_eq!(tabs.ids(), [6, 7, 8, 1, 2, 3, 4, 5]);
        // The end of the group, behind its own last tab.
        groups.place(
            &mut tabs,
            &[1],
            Target::Group {
                group: api,
                before: None,
            },
        );
        assert_eq!(tabs.ids(), [6, 7, 8, 2, 3, 1, 4, 5]);
        ok(&groups, &tabs);
    }

    #[test]
    fn a_group_is_let_go_before_a_block_over_its_upper_half_and_after_it_over_the_lower() {
        let (tabs, groups, _, _) = scene();
        let column = column(&groups, &tabs);
        let spots = groups.group_spots(&tabs, &column);
        assert_eq!(spots.len(), 3, "before api, before web, after web");
        let (api, web) = (column.headers[0].rect, column.headers[1].rect);
        let slot = |y: f64| group_spot_at(&spots, y).unwrap().slot;
        // Over the ungrouped tabs: before the first group (they stay first).
        assert_eq!(slot(column.rows[0].y), 0);
        // Over api's header and its first rows: before api; over its last row: after it.
        assert_eq!(slot(api.y + 4.0), 0);
        assert_eq!(slot(column.rows[5].y + 20.0), 1);
        // The same for web: its header and first row, its second.
        assert_eq!(slot(web.y + 4.0), 1);
        assert_eq!(slot(column.rows[7].y + 20.0), 2);
        assert_eq!(slot(10_000.0), 2);
        // The lines: above api's header, above web's, below the last row.
        assert!(spots[0].line < api.y && spots[0].line > column.rows[2].y);
        assert!(spots[1].line < web.y && spots[1].line > column.rows[5].y);
        assert!(spots[2].line > column.rows[7].y);
    }

    #[test]
    fn a_groups_slot_is_the_place_it_takes_among_the_groups() {
        // [a, b, c]: a at 0 let go before c (slot 2) ends at 1; c let go before a ends at 0; a
        // group let go before or after itself stays.
        assert_eq!(group_place(0, 2), 1);
        assert_eq!(group_place(2, 0), 0);
        assert_eq!(group_place(1, 1), 1);
        assert_eq!(group_place(1, 2), 1);
        assert_eq!(group_place(0, 3), 2);
        let (mut tabs, mut groups, api, web) = scene();
        assert!(groups.move_group(&mut tabs, api, group_place(0, 2)));
        assert_eq!(groups.position(api), Some(1));
        assert_eq!(groups.position(web), Some(0));
        // web is first now; let go before itself or after itself it stays.
        assert!(!groups.move_group(&mut tabs, web, group_place(0, 0)));
        assert!(!groups.move_group(&mut tabs, web, group_place(0, 1)));
        ok(&groups, &tabs);
    }

    #[test]
    fn a_gap_of_the_column_is_the_line_above_the_headers_and_the_bars_is_its_clusters() {
        let (tabs, groups, api, web) = scene();
        let at = |gap| groups.gap_target(&tabs, View::Side, gap);
        // Before the first row: before it, among the ungrouped.
        assert_eq!(at(0), Some(Target::Loose { before: Some(6) }));
        // Before api's first row (gap 3): the line is above api's header, the end of the ungrouped.
        assert_eq!(at(3), Some(Target::Loose { before: None }));
        // Between two rows of api: before the second.
        assert_eq!(
            at(5),
            Some(Target::Group {
                group: api,
                before: Some(3)
            })
        );
        // Before web's first row: the end of api. Past the last: the end of web.
        assert_eq!(
            at(6),
            Some(Target::Group {
                group: api,
                before: None
            })
        );
        assert_eq!(
            at(8),
            Some(Target::Group {
                group: web,
                before: None
            })
        );
        // The bar lists the cluster on screen (api): a place of it, its end past the last.
        let top = |gap| groups.gap_target(&tabs, View::Top, gap);
        assert_eq!(
            top(0),
            Some(Target::Group {
                group: api,
                before: Some(1)
            })
        );
        assert_eq!(
            top(3),
            Some(Target::Group {
                group: api,
                before: None
            })
        );
        // A plain list of tabs has no clusters to name.
        let plain = Groups::<u32>::new();
        assert_eq!(plain.gap_target(&tabs, View::Side, 2), None);
        assert_eq!(plain.gap_target(&tabs, View::Top, 2), None);
    }

    #[test]
    fn a_tab_opened_beside_the_last_of_a_group_stays_in_it() {
        let (tabs, groups, api, _) = scene();
        assert_eq!(
            groups.beside_target(&tabs, 3),
            Some(Target::Group {
                group: api,
                before: None
            })
        );
        assert_eq!(
            groups.beside_target(&tabs, 1),
            Some(Target::Group {
                group: api,
                before: Some(2)
            })
        );
        assert_eq!(
            groups.beside_target(&tabs, 8),
            Some(Target::Loose { before: None })
        );
        assert_eq!(Groups::<u32>::new().beside_target(&tabs, 8), None);
    }
    #[test]
    fn a_tab_let_go_on_its_own_bar_ends_at_the_place_it_was_let_go_at() {
        let (mut tabs, mut groups, _, _) = scene();
        // api is `1 2 3`: 1 let go on the third place ends third, on the second second, and 3 on
        // the first first — a place to the right of the tab is not the gap before the tab there.
        for (tab, place, wanted) in [
            (1, 2, [2, 3, 1]),
            (1, 1, [2, 1, 3]),
            (3, 0, [3, 1, 2]),
            (2, 2, [1, 3, 2]),
            (2, 1, [1, 2, 3]),
        ] {
            let (mut tabs, mut groups) = (tabs.clone(), groups.clone());
            let target = groups.own_target(&tabs, tab, place);
            groups.place(&mut tabs, &[tab], target);
            assert_eq!(
                groups.cluster(&tabs, groups.group_of(tab)),
                wanted,
                "{tab} at {place}"
            );
            ok(&groups, &tabs);
        }
        // An ungrouped tab stays among the ungrouped.
        let target = groups.own_target(&tabs, 6, 2);
        groups.place(&mut tabs, &[6], target);
        assert_eq!(groups.cluster(&tabs, None), [7, 8, 6]);
        ok(&groups, &tabs);
    }
    /// The rows and the row under each header of the list as it stands.
    fn standing(groups: &Groups<u32>, tabs: &Tabs<u32>) -> SidePreview<u32> {
        let mut now = SidePreview {
            rows: Vec::new(),
            firsts: Vec::new(),
        };
        for item in groups.side_items(tabs) {
            match item {
                Item::Tab(tab) => now.rows.push(tab),
                Item::Header(group) => now.firsts.push((group, now.rows.len())),
            }
        }
        now
    }

    #[test]
    fn a_row_slid_to_a_groups_border_stands_in_the_group_the_pointer_says() {
        let (tabs, groups, api, web) = scene();
        // Listed: `6 7 8 | api 1 2 3 | web 4 5`: slots 0…10, 7 above web's header, 8 under it.
        // 3 is api's last tab: slid down past its row it is api's end above the header, web's
        // head under it.
        let above = groups.side_preview(&tabs, 3, 7).unwrap();
        assert_eq!(above.rows, [6, 7, 8, 1, 2, 3, 4, 5]);
        assert_eq!(above.firsts, [(api, 3), (web, 6)]);
        let under = groups.side_preview(&tabs, 3, 8).unwrap();
        assert_eq!(under.rows, [6, 7, 8, 1, 2, 3, 4, 5]);
        assert_eq!(
            under.firsts,
            [(api, 3), (web, 5)],
            "3 is the first row of web"
        );
        // An ungrouped tab slid to the same border: the end of api, or the head of web.
        let end = groups.side_preview(&tabs, 6, 7).unwrap();
        assert_eq!(end.rows, [7, 8, 1, 2, 3, 6, 4, 5]);
        assert_eq!(end.firsts, [(api, 2), (web, 6)]);
        let head = groups.side_preview(&tabs, 6, 8).unwrap();
        assert_eq!(head.rows, [7, 8, 1, 2, 3, 6, 4, 5]);
        assert_eq!(head.firsts, [(api, 2), (web, 5)]);
        // A tab that is not listed and a slot that is not there have no list.
        assert_eq!(groups.side_preview(&tabs, 99, 3), None);
        assert_eq!(groups.side_preview(&tabs, 3, 11), None);
    }

    /// Whatever slot a row is slid to, the list that is shown is the list the drop makes: for each
    /// tab on screen (the one a press selects, which a closed group lists), each slot of the
    /// column, with every group open or closed.
    #[test]
    fn the_preview_of_a_slot_is_the_list_its_drop_makes() {
        for closed in [(false, false), (true, false), (false, true), (true, true)] {
            for tab in 1..=8u32 {
                let (mut tabs, mut groups, api, web) = scene();
                groups.set_collapsed(api, closed.0);
                groups.set_collapsed(web, closed.1);
                groups.select(&mut tabs, tab);
                let column = column(&groups, &tabs);
                let spots = groups.side_spots(&tabs, &column);
                assert_eq!(spots.len(), groups.side_items(&tabs).len() + 1);
                for (slot, spot) in spots.iter().enumerate() {
                    let shown = groups.side_preview(&tabs, tab, slot).unwrap();
                    let (mut after, mut moved) = (tabs.clone(), groups.clone());
                    moved.place(&mut after, &[tab], spot.target.clone());
                    // Where it ends the selection is on it, so a closed group lists it.
                    let done = standing(&moved, &after);
                    assert_eq!(
                        shown.rows, done.rows,
                        "tab {tab}, slot {slot}, closed {closed:?}: {:?}",
                        spot.target
                    );
                    // A group the tab leaves empty goes with its header: the shown list keeps
                    // it, empty, for the moment the drop takes.
                    let kept: Vec<_> = shown
                        .firsts
                        .iter()
                        .copied()
                        .filter(|(group, _)| moved.group(*group).is_some())
                        .collect();
                    assert_eq!(
                        kept, done.firsts,
                        "tab {tab}, slot {slot}, closed {closed:?}: {:?}",
                        spot.target
                    );
                }
            }
        }
    }
}
