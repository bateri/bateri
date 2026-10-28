//! The big view (Window ▸ Show All Tabs, ⇧⌘\): the window's tabs as pictures of their panes'
//! real screens, in the strip's order, in a grid that covers the window — the model, with no
//! AppKit in it. The shell draws what this says and hands the events back.
//!
//! **The tabs are the skeleton.** One cell per tab, in the order of the strip: the ungrouped
//! tabs first, then each group's tabs under its dim heading ([`Row::Heading`]), a closed
//! group's hidden tabs among them. Each cell is the tab's picture (its panes where the split tree
//! puts them, [`Layout::pane_rect`]) with its name row under it. The tab the window shows when the
//! view opens is the selected one ([`Overview::new`]).
//!
//! **Selection is one thing, the pointer's pane another.** The arrows move the selection over the
//! cells ([`Overview::step`]): along a row without wrapping at its ends, up and down to the
//! nearest column of the next row of tabs, over the headings. The pointer selects the cell under it
//! and, in a split tab, names the pane under it ([`Overview::point`]): that pane is lit, the others
//! dim, and ↵ goes to it ([`Overview::act`]); with the keys the tab's own focused pane is where
//! ↵ lands. The model reads nothing from the windows: the shell hands the tabs in
//! ([`Tab`], their panes the card's own [`CardPane`] list) and gets an [`Act`] out.

use crate::remote_stats::Detail;
use crate::split::Rect;
use crate::tabs::CardPane;

/// The narrowest and widest a tab's picture is drawn. The narrowest keeps a split tab's panes
/// readable (a terminal at a quarter of the tile is still more than 30 columns on a 14-inch
/// screen); the widest keeps a lone tab from filling a wide window with one enormous picture,
/// which tells no more than a smaller one does.
pub const MIN_TILE: f64 = 240.0;
pub const MAX_TILE: f64 = 400.0;

/// The room between two pictures along a row, and between two rows.
pub const GAP: f64 = 28.0;
pub const ROW_GAP: f64 = 22.0;

/// The room at the grid's two sides.
pub const MARGIN: f64 = 48.0;

/// A picture's name row (icon, name, the key) and a group's heading, points.
pub const NAME_ROW: f64 = 30.0;
pub const HEADING_ROW: f64 = 40.0;

/// The thin gap between two panes of a tab, as the split view's own (the card's map has the
/// same, [`crate::tabs::MAP_GAP`]).
pub const PANE_GAP: f64 = 2.0;

/// The shape a tab's picture may take, height over width: a window's content is rarely outside
/// it, and one that is does not make a tile taller than the screen.
pub const MIN_ASPECT: f64 = 0.35;
pub const MAX_ASPECT: f64 = 1.2;

/// What ↵ does with a tab.
pub const GO_TO_TAB: &str = "Go to Tab";

/// A group a tab belongs to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub id: u64,
    pub name: String,
}

/// One tab of the window, as the view lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct Tab {
    /// The tab's identity in its window.
    pub id: u64,
    pub name: String,
    pub group: Option<Group>,
    /// The key that reaches the tab, as the strip says it (`⌘3`); `None` for a tab no digit
    /// reaches.
    pub key: Option<String>,
    /// The tab's panes where they stand in it, in tree order (the card's list).
    pub panes: Vec<CardPane>,
}

impl Tab {
    /// The pane whose place holds the point `(x, y)`, fractions of the tab's picture.
    pub fn pane_at(&self, x: f64, y: f64) -> Option<u64> {
        self.panes
            .iter()
            .find(|pane| {
                let place = pane.place;
                x >= place.x
                    && x < place.x + place.width
                    && y >= place.y
                    && y < place.y + place.height
            })
            .map(|pane| pane.id)
    }

    /// The pane ↵ lands on when nothing points at one: the tab's focused pane.
    pub fn focused(&self) -> Option<u64> {
        self.panes
            .iter()
            .find(|pane| pane.focused)
            .or_else(|| self.panes.first())
            .map(|pane| pane.id)
    }
}

/// An arrow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

/// What ↵ (or a click) asks the shell to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    /// Show tab `tab` with `pane` focused: the pane the pointer is over, else `None` (the tab's
    /// own focused pane stays).
    GoToTab { tab: u64, pane: Option<u64> },
}

/// The name of the row that lists every tab, and of the one that lists this computer's: the
/// local machine is not named after the hardware it is (the shell on Linux says the same).
pub const ALL_TABS: &str = "All tabs";
pub const LOCAL: &str = "This Computer";

/// What the left column selects: a tab shows in the grid when one of its panes is on the machine.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Machine {
    /// Every tab.
    All,
    /// This computer: the panes with no ssh.
    Local,
    /// A server a pane of the window is in, as the pane's session names its host
    /// ([`CardPane::host`]).
    Host(String),
}

impl Machine {
    /// The row's name.
    pub fn title(&self) -> String {
        match self {
            Machine::All => ALL_TABS.to_owned(),
            Machine::Local => LOCAL.to_owned(),
            Machine::Host(host) => format!("⇄ {host}"),
        }
    }

    /// Whether `pane` is on the machine.
    pub fn holds(&self, pane: &CardPane) -> bool {
        match self {
            Machine::All => true,
            Machine::Local => pane.host.is_none(),
            Machine::Host(host) => pane.host.as_deref() == Some(host),
        }
    }
}

/// A row of the left column.
#[derive(Clone, Debug, PartialEq)]
pub struct MachineRow {
    pub machine: Machine,
    pub title: String,
    /// "2 tabs" under the name.
    pub count: String,
    /// The kablo account of a server whose panes have one: its load shows under the row when it
    /// is the selected one. `None` for the rows that are not a server and for a plain ssh.
    pub account: Option<String>,
}

/// "2 tabs".
fn tabs_text(count: usize) -> String {
    format!("{count} {}", if count == 1 { "tab" } else { "tabs" })
}

/// The view's state: the tabs, the machine the left column selects, the selected tab and where
/// the pointer is over it.
#[derive(Clone, Debug, PartialEq)]
pub struct Overview {
    all: Vec<Tab>,
    /// The places in [`Self::all`] of the tabs the machine leaves in the grid, in the strip's
    /// order.
    shown: Vec<usize>,
    machine: Machine,
    /// The machine the pointer is over in the left column: its panes light in the grid, the
    /// selection stays.
    glance: Option<Machine>,
    selected: Option<u64>,
    /// The pane the pointer is over in the selected tab (a split tab's), `None` for the keys'
    /// selection.
    pointed: Option<u64>,
}

impl Overview {
    /// The view of `tabs` (the strip's order), with `here`, the tab the window shows, selected;
    /// the first tab when `here` is not among them. All tabs are shown.
    pub fn new(tabs: Vec<Tab>, here: Option<u64>) -> Self {
        let selected = here
            .filter(|here| tabs.iter().any(|tab| tab.id == *here))
            .or_else(|| tabs.first().map(|tab| tab.id));
        let mut overview = Self {
            all: tabs,
            shown: Vec::new(),
            machine: Machine::All,
            glance: None,
            selected,
            pointed: None,
        };
        overview.reshow();
        overview
    }

    /// The places of the tabs the machine leaves, and the selection kept on a tab that is
    /// still among them.
    fn reshow(&mut self) {
        let machine = &self.machine;
        self.shown = (0..self.all.len())
            .filter(|&at| self.all[at].panes.iter().any(|pane| machine.holds(pane)))
            .collect();
        if !self
            .selected
            .is_some_and(|selected| self.is_shown(selected))
        {
            self.selected = self.shown.first().map(|&at| self.all[at].id);
        }
        if !self
            .pointed
            .is_some_and(|pane| self.selected_has_pane(pane))
        {
            self.pointed = None;
        }
    }

    fn is_shown(&self, tab: u64) -> bool {
        self.shown.iter().any(|&at| self.all[at].id == tab)
    }

    /// How many tabs the grid holds.
    pub fn len(&self) -> usize {
        self.shown.len()
    }

    pub fn is_empty(&self) -> bool {
        self.shown.is_empty()
    }

    /// The `index`th tab of the grid.
    pub fn tab(&self, index: usize) -> &Tab {
        &self.all[self.shown[index]]
    }

    /// The tabs of the grid, in the strip's order.
    pub fn tabs(&self) -> impl Iterator<Item = &Tab> {
        self.shown.iter().map(|&at| &self.all[at])
    }

    pub fn selected(&self) -> Option<u64> {
        self.selected
    }

    /// The selected tab's place in the grid.
    pub fn selected_index(&self) -> Option<usize> {
        let selected = self.selected?;
        self.tabs().position(|tab| tab.id == selected)
    }

    /// The tabs and what they report, read afresh (the view is open while the programs go on):
    /// the selection stays on its tab, and on the tab that took its place when it is gone; the
    /// machine stays unless it has no tab left.
    pub fn refresh(&mut self, tabs: Vec<Tab>) {
        let at = self.selected_index().unwrap_or(0);
        self.all = tabs;
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
        let before = self.selected;
        self.reshow();
        if before.is_some() && self.selected != before {
            // The tab that took its place: the one now at its old place, the last if none.
            let place = at.min(self.shown.len().saturating_sub(1));
            self.selected = self.shown.get(place).map(|&at| self.all[at].id);
        }
    }

    fn selected_has_pane(&self, pane: u64) -> bool {
        self.selected_index().is_some_and(|at| {
            self.tab(at)
                .panes
                .iter()
                .any(|candidate| candidate.id == pane)
        })
    }

    /// Selects tab `tab` (a key, never the pointer: no pane is pointed at); `false` if the
    /// grid does not hold it.
    pub fn select(&mut self, tab: u64) -> bool {
        if !self.is_shown(tab) {
            return false;
        }
        self.selected = Some(tab);
        self.pointed = None;
        true
    }

    /// The pointer is over tab `tab`, over its pane `pane` when there is one: the tab is
    /// selected. `false` if the grid does not hold the tab.
    pub fn point(&mut self, tab: u64, pane: Option<u64>) -> bool {
        if !self.is_shown(tab) {
            return false;
        }
        self.selected = Some(tab);
        self.pointed = pane.filter(|&pane| self.selected_has_pane(pane));
        true
    }

    /// The pane the pointer is over in the selected tab, if it names one.
    pub fn pointed(&self) -> Option<u64> {
        self.pointed
    }

    /// Whether `pane` of tab `tab` is drawn dim: it is not on the machine the grid is of (or the
    /// one the pointer is over in the left column), or the pointer is over another pane of the
    /// tab and a split tab's other panes give way to it.
    pub fn dim(&self, tab: u64, pane: &CardPane) -> bool {
        let lit = self.glance.as_ref().unwrap_or(&self.machine);
        !lit.holds(pane)
            || (self.selected == Some(tab)
                && self.pointed.is_some_and(|pointed| pointed != pane.id))
    }

    // ─── The machines ────────────────────────────────────────────────────

    /// The left column: All tabs, This Computer (when a pane of the window is local), then the
    /// servers a pane of the window is in, by name — an order that does not change as tabs move,
    /// so ⇥ goes the same way twice. A machine with no tab in the window has no row.
    pub fn machines(&self) -> Vec<MachineRow> {
        let count = |machine: &Machine| {
            self.all
                .iter()
                .filter(|tab| tab.panes.iter().any(|pane| machine.holds(pane)))
                .count()
        };
        let row = |machine: Machine, count: usize, account: Option<String>| MachineRow {
            title: machine.title(),
            machine,
            count: tabs_text(count),
            account,
        };
        let mut rows = vec![row(Machine::All, self.all.len(), None)];
        let local = count(&Machine::Local);
        if local > 0 {
            rows.push(row(Machine::Local, local, None));
        }
        let mut hosts: Vec<(&str, Option<&String>)> = Vec::new();
        for pane in self.all.iter().flat_map(|tab| tab.panes.iter()) {
            let Some(host) = pane.host.as_deref() else {
                continue;
            };
            match hosts.iter_mut().find(|(known, _)| *known == host) {
                Some((_, account)) => {
                    if account.is_none() {
                        *account = pane.account.as_ref();
                    }
                }
                None => hosts.push((host, pane.account.as_ref())),
            }
        }
        hosts.sort_by_key(|(host, _)| host.to_lowercase());
        for (host, account) in hosts {
            let machine = Machine::Host(host.to_owned());
            let tabs = count(&machine);
            rows.push(row(machine, tabs, account.cloned()));
        }
        rows
    }

    /// The machine the grid is of.
    pub fn machine(&self) -> &Machine {
        &self.machine
    }

    /// The kablo account of the selected machine, when it is a server that has one.
    pub fn account(&self) -> Option<String> {
        self.machines()
            .into_iter()
            .find(|row| row.machine == self.machine)
            .and_then(|row| row.account)
    }

    /// Selects `machine`; `false` if the left column has no such row. The tab selected stays when
    /// the machine has it, else the first the machine has.
    pub fn set_machine(&mut self, machine: Machine) -> bool {
        if !self.machines().iter().any(|row| row.machine == machine) {
            return false;
        }
        self.machine = machine;
        self.glance = None;
        self.reshow();
        true
    }

    /// ⇥ (`forward`) or ⇧⇥: the next machine of the column, wrapping.
    pub fn cycle(&mut self, forward: bool) {
        let rows = self.machines();
        let at = rows
            .iter()
            .position(|row| row.machine == self.machine)
            .unwrap_or(0);
        let next = if forward {
            (at + 1) % rows.len()
        } else {
            (at + rows.len() - 1) % rows.len()
        };
        self.set_machine(rows[next].machine.clone());
    }

    /// The pointer is over `machine`'s row (`Some`) or left the column: that machine's panes
    /// light in the grid without selecting it.
    pub fn glance(&mut self, machine: Option<Machine>) -> bool {
        let machine = machine.filter(|machine| *machine != self.machine);
        let changed = self.glance != machine;
        self.glance = machine;
        changed
    }

    // ─── Moving and acting ───────────────────────────────────────────────

    /// The selection one cell along `direction` in `layout`; `false` when there is nowhere to go
    /// (the row's end, the first or last row of tabs).
    pub fn step(&mut self, layout: &Layout, direction: Direction) -> bool {
        let Some(at) = self.selected_index() else {
            return false;
        };
        let Some(&(row, column)) = layout.cells.get(at) else {
            return false;
        };
        let length = |row: usize| match &layout.rows[row] {
            Row::Tiles(tabs) => tabs.len(),
            Row::Heading { .. } => 0,
        };
        let target = match direction {
            Direction::Left => column.checked_sub(1).map(|column| (row, column)),
            Direction::Right => (column + 1 < length(row)).then_some((row, column + 1)),
            Direction::Up => (0..row)
                .rev()
                .find(|&above| length(above) > 0)
                .map(|above| (above, column.min(length(above) - 1))),
            Direction::Down => (row + 1..layout.rows.len())
                .find(|&below| length(below) > 0)
                .map(|below| (below, column.min(length(below) - 1))),
        };
        let Some((row, column)) = target else {
            return false;
        };
        let Row::Tiles(tabs) = &layout.rows[row] else {
            return false;
        };
        let Some(&index) = tabs.get(column) else {
            return false;
        };
        self.selected = Some(self.tab(index).id);
        self.pointed = None;
        true
    }

    /// What ↵ does now.
    pub fn act(&self) -> Option<Act> {
        let tab = self.selected?;
        Some(Act::GoToTab {
            tab,
            pane: self.pointed,
        })
    }

    /// The word beside the selected tab's name that says what ↵ does.
    pub fn verb(&self) -> Option<&'static str> {
        self.selected.map(|_| GO_TO_TAB)
    }

    /// The grid for a view `width` points wide whose tabs have the shape `aspect` (the window's
    /// content, height over width).
    pub fn layout(&self, width: f64, aspect: f64) -> Layout {
        let tabs: Vec<&Tab> = self.tabs().collect();
        Layout::new(&tabs, width, aspect)
    }
}

/// How many pictures fit along a row of a view `width` wide.
pub fn columns(width: f64) -> usize {
    let room = width - 2.0 * MARGIN + GAP;
    if !room.is_finite() || room <= 0.0 {
        return 1;
    }
    ((room / (MIN_TILE + GAP)).floor() as usize).max(1)
}

/// The width of a picture among `columns` in a view `width` wide: they share the room, up to
/// [`MAX_TILE`].
pub fn tile_width(width: f64, columns: usize) -> f64 {
    let columns = columns.max(1) as f64;
    let room = (width - 2.0 * MARGIN - (columns - 1.0) * GAP) / columns;
    if !room.is_finite() {
        return 1.0;
    }
    room.clamp(1.0, MAX_TILE).floor()
}

/// A row of the grid.
#[derive(Clone, Debug, PartialEq)]
pub enum Row {
    /// A group's dim heading ("api-work · 2 tabs").
    Heading { group: u64, text: String },
    /// Tabs, by their place in [`Overview::tabs`], left to right.
    Tiles(Vec<usize>),
}

/// Where everything stands for one width: rows from the top (origin 0, before any scroll), in
/// the view's own points.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub columns: usize,
    /// A picture's size, points.
    pub tile: (f64, f64),
    pub rows: Vec<Row>,
    /// Each tab's row and column in [`Self::rows`].
    cells: Vec<(usize, usize)>,
    /// The top of each row.
    tops: Vec<f64>,
    /// The grid's left edge: the rows are centred as a whole.
    pub left: f64,
    /// The grid's height, from the first row's top to the last row's bottom.
    pub height: f64,
}

impl Layout {
    fn new(tabs: &[&Tab], width: f64, aspect: f64) -> Self {
        let columns = columns(width);
        let tile_w = tile_width(width, columns);
        let aspect = if aspect.is_finite() {
            aspect.clamp(MIN_ASPECT, MAX_ASPECT)
        } else {
            MIN_ASPECT
        };
        let tile = (tile_w, (tile_w * aspect).round());
        let mut rows: Vec<Row> = Vec::new();
        let mut cells = Vec::with_capacity(tabs.len());
        let mut group: Option<u64> = None;
        for (index, tab) in tabs.iter().enumerate() {
            let tab_group = tab.group.as_ref().map(|group| group.id);
            let new_group = tab_group != group;
            if new_group {
                group = tab_group;
                if let Some(of) = &tab.group {
                    let count = tabs
                        .iter()
                        .filter(|other| other.group.as_ref().map(|group| group.id) == Some(of.id))
                        .count();
                    rows.push(Row::Heading {
                        group: of.id,
                        text: heading_text(&of.name, count),
                    });
                }
            }
            let last = rows.len().saturating_sub(1);
            match rows.last_mut() {
                Some(Row::Tiles(row)) if !new_group && row.len() < columns => {
                    cells.push((last, row.len()));
                    row.push(index);
                }
                _ => {
                    cells.push((rows.len(), 0));
                    rows.push(Row::Tiles(vec![index]));
                }
            }
        }
        let mut tops = Vec::with_capacity(rows.len());
        let mut bottom = 0.0;
        for (at, row) in rows.iter().enumerate() {
            let top = if at == 0 { 0.0 } else { bottom + ROW_GAP };
            tops.push(top);
            bottom = top
                + match row {
                    Row::Heading { .. } => HEADING_ROW,
                    Row::Tiles(_) => tile.1 + NAME_ROW,
                };
        }
        let grid = columns as f64 * tile.0 + (columns as f64 - 1.0) * GAP;
        Self {
            columns,
            tile,
            rows,
            cells,
            tops,
            left: ((width - grid) / 2.0).floor().max(0.0),
            height: bottom,
        }
    }

    /// The tab at `index`: where its picture is (without its name row).
    pub fn picture(&self, index: usize) -> Option<Rect> {
        let &(row, column) = self.cells.get(index)?;
        Some(Rect::new(
            self.left + column as f64 * (self.tile.0 + GAP),
            self.tops[row],
            self.tile.0,
            self.tile.1,
        ))
    }

    /// The tab at `index`: its name row, under the picture.
    pub fn name_row(&self, index: usize) -> Option<Rect> {
        let picture = self.picture(index)?;
        Some(Rect::new(
            picture.x,
            picture.y + picture.height,
            picture.width,
            NAME_ROW,
        ))
    }

    /// Row `row`'s heading line, for a [`Row::Heading`].
    pub fn heading(&self, row: usize) -> Option<Rect> {
        let top = *self.tops.get(row)?;
        Some(Rect::new(
            self.left,
            top,
            self.columns as f64 * (self.tile.0 + GAP) - GAP,
            HEADING_ROW,
        ))
    }

    /// Where pane `pane` of `tab` is drawn, in the view's points: its place in the tab's picture
    /// less the thin gap between neighbours ([`crate::tabs::inset_tile`]).
    pub fn pane_rect(&self, index: usize, pane: &CardPane) -> Option<Rect> {
        let picture = self.picture(index)?;
        let tile = crate::tabs::inset_tile(pane.place, picture.width, picture.height, PANE_GAP);
        Some(Rect::new(
            picture.x + tile.x,
            picture.y + tile.y,
            tile.width,
            tile.height,
        ))
    }

    /// The tab whose cell (picture and name row) holds the point `(x, y)`.
    pub fn tab_at(&self, x: f64, y: f64) -> Option<usize> {
        (0..self.cells.len()).find(|&index| {
            self.picture(index).is_some_and(|picture| {
                x >= picture.x
                    && x < picture.x + picture.width
                    && y >= picture.y
                    && y < picture.y + picture.height + NAME_ROW
            })
        })
    }

    /// The point `(x, y)` as fractions of tab `index`'s picture, for [`Tab::pane_at`]; `None`
    /// outside the picture (the name row).
    pub fn in_picture(&self, index: usize, x: f64, y: f64) -> Option<(f64, f64)> {
        let picture = self.picture(index)?;
        let (fx, fy) = (
            (x - picture.x) / picture.width,
            (y - picture.y) / picture.height,
        );
        ((0.0..1.0).contains(&fx) && (0.0..1.0).contains(&fy)).then_some((fx, fy))
    }

    /// The tabs whose cells show in a viewport `height` tall scrolled `scroll` down: the ones
    /// that need a picture now.
    pub fn visible(&self, scroll: f64, height: f64) -> Vec<usize> {
        (0..self.cells.len())
            .filter(|&index| {
                self.picture(index).is_some_and(|picture| {
                    picture.y + picture.height + NAME_ROW > scroll && picture.y < scroll + height
                })
            })
            .collect()
    }

    /// The most the view scrolls in a viewport `height` tall.
    pub fn max_scroll(&self, height: f64) -> f64 {
        (self.height - height).max(0.0)
    }

    /// The scroll that has tab `index`'s cell in a viewport `height` tall, with `pad` of air
    /// beyond it, moving as little as it can; the current `scroll` when it is in view.
    pub fn reveal(&self, index: usize, scroll: f64, height: f64, pad: f64) -> f64 {
        let Some(picture) = self.picture(index) else {
            return scroll.clamp(0.0, self.max_scroll(height));
        };
        let (top, bottom) = (picture.y - pad, picture.y + picture.height + NAME_ROW + pad);
        let moved = if top < scroll {
            top
        } else if bottom > scroll + height {
            bottom - height
        } else {
            scroll
        };
        moved.clamp(0.0, self.max_scroll(height))
    }
}

/// What the left column says of a server under its row.
#[derive(Clone, Debug, PartialEq)]
pub struct Load {
    /// Label and value, top to bottom: CPU, memory, disk and how long it has been up.
    pub rows: Vec<(String, String)>,
    /// The operating system, if the server said.
    pub os: Option<String>,
}

/// A server's last sample as the left column says it; the numbers are the server card's own
/// ([`crate::server_card::load`]).
pub fn load(detail: &Detail) -> Load {
    let card = crate::server_card::load("", Some(detail));
    let mut rows = vec![
        (card.cpu_label, card.cpu.value),
        ("Memory".to_owned(), card.mem.value),
    ];
    if let Some(disk) = detail.disk {
        rows.push(("Disk".to_owned(), format!("{disk}%")));
    }
    if let Some(seconds) = detail.uptime {
        rows.push(("Up".to_owned(), uptime_text(seconds)));
    }
    Load {
        rows,
        os: detail.os.clone(),
    }
}

/// How long a server has been up: "12 days", "3 h 20 min", "5 min".
pub fn uptime_text(seconds: u64) -> String {
    let (days, hours, minutes) = (seconds / 86_400, seconds / 3_600 % 24, seconds / 60 % 60);
    match (days, hours) {
        (0, 0) => format!("{minutes} min"),
        (0, hours) => format!("{hours} h {minutes} min"),
        (1, _) => "1 day".to_owned(),
        (days, _) => format!("{days} days"),
    }
}

/// A group's heading: "api-work · 2 tabs".
fn heading_text(name: &str, count: usize) -> String {
    format!(
        "{name} · {count} {}",
        if count == 1 { "tab" } else { "tabs" }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::program::identity::Kind;

    fn pane(id: u64, place: Rect, focused: bool) -> CardPane {
        CardPane {
            id,
            place,
            kind: Kind::Shell,
            face: None,
            name: format!("pane {id}"),
            indicator: None,
            exit: None,
            focused,
            host: None,
            directory: None,
            account: None,
        }
    }

    fn whole() -> Rect {
        Rect::new(0.0, 0.0, 1.0, 1.0)
    }

    fn tab(id: u64, group: Option<(u64, &str)>) -> Tab {
        Tab {
            id,
            name: format!("tab {id}"),
            group: group.map(|(id, name)| Group {
                id,
                name: name.to_owned(),
            }),
            key: None,
            panes: vec![pane(id * 10, whole(), true)],
        }
    }

    fn plain(count: u64) -> Vec<Tab> {
        (1..=count).map(|id| tab(id, None)).collect()
    }

    /// A window `columns` pictures wide.
    fn width_for(columns: usize) -> f64 {
        2.0 * MARGIN + columns as f64 * MIN_TILE + (columns as f64 - 1.0) * GAP
    }

    #[test]
    fn the_width_decides_how_many_pictures_a_row_holds() {
        assert_eq!(columns(0.0), 1, "no room still has a column");
        assert_eq!(columns(f64::NAN), 1);
        assert_eq!(columns(width_for(1)), 1);
        assert_eq!(columns(width_for(3) - 1.0), 2, "one point short of three");
        assert_eq!(columns(width_for(3)), 3);
        assert_eq!(columns(width_for(5)), 5);
    }

    #[test]
    fn pictures_share_the_room_up_to_the_widest() {
        let width = width_for(3);
        assert_eq!(tile_width(width, 3), MIN_TILE);
        assert_eq!(
            tile_width(3000.0, 4),
            MAX_TILE,
            "a wide window does not stretch them"
        );
        assert!(
            tile_width(100.0, 1) >= 1.0,
            "a window narrower than its margins"
        );
    }

    #[test]
    fn rows_fill_left_to_right_in_the_strips_order() {
        let overview = Overview::new(plain(5), None);
        let layout = overview.layout(width_for(3), 0.6);
        assert_eq!(
            layout.rows,
            vec![Row::Tiles(vec![0, 1, 2]), Row::Tiles(vec![3, 4])]
        );
        let (first, third, fourth) = (
            layout.picture(0).unwrap(),
            layout.picture(2).unwrap(),
            layout.picture(3).unwrap(),
        );
        assert_eq!(first.y, third.y, "one row");
        assert!(third.x > first.x);
        assert_eq!(fourth.x, first.x, "the next row starts at the left again");
        assert_eq!(
            fourth.y,
            first.y + layout.tile.1 + NAME_ROW + ROW_GAP,
            "the second row stands below the first one's name row"
        );
    }

    #[test]
    fn a_group_stands_under_its_heading_and_a_closed_groups_tabs_are_listed() {
        // Tab 1 and 2 ungrouped, 3 and 4 in "api", 5 in "web" (a closed group's tabs are in the
        // strip's list too: the shell hands every tab).
        let tabs = vec![
            tab(1, None),
            tab(2, None),
            tab(3, Some((7, "api"))),
            tab(4, Some((7, "api"))),
            tab(5, Some((8, "web"))),
        ];
        let layout = Overview::new(tabs, None).layout(width_for(3), 0.6);
        assert_eq!(
            layout.rows,
            vec![
                Row::Tiles(vec![0, 1]),
                Row::Heading {
                    group: 7,
                    text: "api · 2 tabs".to_owned()
                },
                Row::Tiles(vec![2, 3]),
                Row::Heading {
                    group: 8,
                    text: "web · 1 tab".to_owned()
                },
                Row::Tiles(vec![4]),
            ]
        );
    }

    #[test]
    fn the_view_opens_on_the_tab_the_window_shows() {
        let overview = Overview::new(plain(4), Some(3));
        assert_eq!(overview.selected(), Some(3));
        assert_eq!(overview.selected_index(), Some(2));
        assert_eq!(
            Overview::new(plain(4), Some(99)).selected(),
            Some(1),
            "a tab the view does not list: the first"
        );
        assert_eq!(Overview::new(Vec::new(), None).selected(), None);
    }

    #[test]
    fn the_arrows_move_along_rows_without_wrapping() {
        let mut overview = Overview::new(plain(5), Some(1));
        let layout = overview.layout(width_for(3), 0.6);
        assert!(!overview.step(&layout, Direction::Left), "the row's start");
        assert!(overview.step(&layout, Direction::Right));
        assert!(overview.step(&layout, Direction::Right));
        assert_eq!(overview.selected(), Some(3));
        assert!(
            !overview.step(&layout, Direction::Right),
            "the row's end does not wrap to the next row"
        );
        assert_eq!(overview.selected(), Some(3));
    }

    #[test]
    fn down_and_up_keep_the_nearest_column() {
        let mut overview = Overview::new(plain(5), Some(3));
        let layout = overview.layout(width_for(3), 0.6);
        // Tab 3 is the third column of the first row; the second row has two tabs.
        assert!(overview.step(&layout, Direction::Down));
        assert_eq!(
            overview.selected(),
            Some(5),
            "the nearest column the row has"
        );
        assert!(!overview.step(&layout, Direction::Down), "the last row");
        assert!(overview.step(&layout, Direction::Up));
        assert_eq!(
            overview.selected(),
            Some(2),
            "back in the column it is in now"
        );
        assert!(!overview.step(&layout, Direction::Up), "the first row");
    }

    #[test]
    fn the_arrows_pass_over_a_groups_heading() {
        let tabs = vec![
            tab(1, None),
            tab(2, Some((7, "api"))),
            tab(3, Some((7, "api"))),
        ];
        let mut overview = Overview::new(tabs, Some(1));
        let layout = overview.layout(width_for(2), 0.6);
        assert!(overview.step(&layout, Direction::Down));
        assert_eq!(overview.selected(), Some(2), "the heading is not a stop");
        assert!(overview.step(&layout, Direction::Right));
        assert_eq!(overview.selected(), Some(3));
        assert!(overview.step(&layout, Direction::Up));
        assert_eq!(overview.selected(), Some(1), "up over the heading again");
    }

    #[test]
    fn the_pointer_selects_and_names_a_pane_of_a_split_tab() {
        let mut split = tab(1, None);
        split.panes = vec![
            pane(11, Rect::new(0.0, 0.0, 0.5, 1.0), true),
            pane(12, Rect::new(0.5, 0.0, 0.5, 1.0), false),
        ];
        let mut overview = Overview::new(vec![split.clone(), tab(2, None)], Some(2));
        assert_eq!(split.pane_at(0.7, 0.4), Some(12));
        assert_eq!(split.pane_at(0.2, 0.9), Some(11));
        assert_eq!(split.focused(), Some(11));
        assert!(overview.point(1, split.pane_at(0.7, 0.4)));
        assert_eq!(overview.selected(), Some(1));
        assert_eq!(overview.pointed(), Some(12));
        assert!(
            overview.dim(1, &split.panes[0]),
            "the pane the pointer is not over gives way"
        );
        assert!(!overview.dim(1, &split.panes[1]));
        let other = overview.tab(1).panes[0].clone();
        assert!(
            !overview.dim(2, &other),
            "another tab's panes are not touched"
        );
        assert_eq!(
            overview.act(),
            Some(Act::GoToTab {
                tab: 1,
                pane: Some(12)
            })
        );
        // A key takes the pointer's pane away: ↵ lands on the focused one again.
        assert!(overview.select(1));
        assert_eq!(overview.pointed(), None);
        assert!(!overview.dim(1, &split.panes[0]));
        assert_eq!(overview.act(), Some(Act::GoToTab { tab: 1, pane: None }));
        assert!(!overview.point(9, None), "a tab the view does not list");
    }

    #[test]
    fn a_pane_the_tab_does_not_have_is_not_pointed_at() {
        let mut overview = Overview::new(plain(2), None);
        assert!(overview.point(2, Some(999)));
        assert_eq!(overview.pointed(), None);
    }

    #[test]
    fn a_tab_that_goes_while_the_view_is_open_gives_its_place_to_the_next() {
        let mut overview = Overview::new(plain(4), Some(2));
        overview.refresh(vec![tab(1, None), tab(3, None), tab(4, None)]);
        assert_eq!(overview.selected(), Some(3), "the tab that took its place");
        overview.refresh(vec![tab(1, None)]);
        assert_eq!(overview.selected(), Some(1));
        overview.refresh(Vec::new());
        assert_eq!(overview.selected(), None);
        assert_eq!(overview.act(), None);
    }

    #[test]
    fn a_point_finds_its_cell_and_its_pane_in_the_picture() {
        let overview = Overview::new(plain(4), None);
        let layout = overview.layout(width_for(2), 0.5);
        let second = layout.picture(1).unwrap();
        assert_eq!(layout.tab_at(second.x + 5.0, second.y + 5.0), Some(1));
        assert_eq!(
            layout.tab_at(second.x + 5.0, second.y + second.height + NAME_ROW - 1.0),
            Some(1),
            "the name row is the cell's too"
        );
        assert_eq!(
            layout.tab_at(second.x - 1.0, second.y + 5.0),
            None,
            "the gap"
        );
        let (fx, fy) = layout
            .in_picture(1, second.x + second.width / 2.0, second.y)
            .unwrap();
        assert!((fx - 0.5).abs() < 1e-9 && fy.abs() < 1e-9);
        assert_eq!(
            layout.in_picture(1, second.x + 2.0, second.y + second.height + 4.0),
            None,
            "under the picture"
        );
    }

    #[test]
    fn panes_stand_where_the_split_tree_puts_them_a_gap_apart() {
        let left = pane(1, Rect::new(0.0, 0.0, 0.5, 1.0), true);
        let right = pane(2, Rect::new(0.5, 0.0, 0.5, 1.0), false);
        let mut split = tab(1, None);
        split.panes = vec![left.clone(), right.clone()];
        let overview = Overview::new(vec![split], None);
        let layout = overview.layout(width_for(1), 0.6);
        let picture = layout.picture(0).unwrap();
        let (a, b) = (
            layout.pane_rect(0, &left).unwrap(),
            layout.pane_rect(0, &right).unwrap(),
        );
        assert_eq!(a.x, picture.x, "the picture's edge is reached");
        assert!(
            (b.x - (a.x + a.width) - PANE_GAP).abs() < 1e-9,
            "a gap between neighbours"
        );
        assert!((b.x + b.width - (picture.x + picture.width)).abs() < 1e-9);
        assert_eq!(a.height, picture.height);
    }

    #[test]
    fn only_the_cells_in_the_viewport_need_a_picture() {
        let overview = Overview::new(plain(9), None);
        let layout = overview.layout(width_for(3), 0.6);
        let row = layout.tile.1 + NAME_ROW + ROW_GAP;
        assert_eq!(layout.visible(0.0, row), vec![0, 1, 2], "the first row");
        assert_eq!(
            layout.visible(row - ROW_GAP - 4.0, ROW_GAP + 8.0),
            vec![0, 1, 2, 3, 4, 5],
            "a scroll between two rows shows both"
        );
        assert_eq!(layout.visible(0.0, layout.height).len(), 9);
    }

    #[test]
    fn the_scroll_moves_only_as_far_as_the_selection_needs() {
        let overview = Overview::new(plain(9), None);
        let layout = overview.layout(width_for(3), 0.6);
        let viewport = layout.tile.1 + NAME_ROW + 20.0;
        assert_eq!(layout.reveal(0, 0.0, viewport, 8.0), 0.0, "in view: stays");
        let down = layout.reveal(6, 0.0, viewport, 8.0);
        assert!(down > 0.0 && down <= layout.max_scroll(viewport));
        let picture = layout.picture(6).unwrap();
        assert!(
            picture.y + picture.height + NAME_ROW <= down + viewport,
            "the cell is in view"
        );
        assert_eq!(
            layout.reveal(0, down, viewport, 8.0),
            0.0,
            "back to the first row, as far up as the grid goes"
        );
        assert_eq!(
            layout.max_scroll(layout.height + 100.0),
            0.0,
            "a view that fits"
        );
    }

    // ─── The machines ───

    /// A pane on `host` (`None`: this computer), with a kablo account when it is a server's door.
    fn on(id: u64, host: Option<&str>, account: Option<&str>) -> CardPane {
        CardPane {
            host: host.map(str::to_owned),
            account: account.map(str::to_owned),
            ..pane(id, whole(), false)
        }
    }

    fn with_panes(id: u64, panes: Vec<CardPane>) -> Tab {
        Tab {
            panes,
            ..tab(id, None)
        }
    }

    /// `web` has two panes on rasp and one here; `api` is here; `db` is on db1.
    fn mixed() -> Overview {
        Overview::new(
            vec![
                with_panes(
                    1,
                    vec![
                        on(11, Some("rasp"), Some("u@rasp:22")),
                        on(12, Some("rasp"), Some("u@rasp:22")),
                        on(13, None, None),
                    ],
                ),
                with_panes(2, vec![on(21, None, None)]),
                with_panes(3, vec![on(31, Some("db1"), None)]),
            ],
            Some(2),
        )
    }

    fn names(overview: &Overview) -> Vec<String> {
        overview
            .machines()
            .into_iter()
            .map(|row| row.title)
            .collect()
    }

    #[test]
    fn the_column_lists_all_this_computer_and_the_servers_by_name() {
        let overview = mixed();
        assert_eq!(
            names(&overview),
            vec!["All tabs", "This Computer", "⇄ db1", "⇄ rasp"]
        );
        let counts: Vec<String> = overview
            .machines()
            .into_iter()
            .map(|row| row.count)
            .collect();
        assert_eq!(counts, vec!["3 tabs", "2 tabs", "1 tab", "1 tab"]);
        let accounts: Vec<Option<String>> = overview
            .machines()
            .into_iter()
            .map(|row| row.account)
            .collect();
        assert_eq!(
            accounts,
            vec![None, None, None, Some("u@rasp:22".to_owned())],
            "a plain ssh has no account: no load is measured there"
        );
        assert!(
            !names(&Overview::new(
                vec![with_panes(1, vec![on(1, Some("x"), None)])],
                None
            ))
            .contains(&"This Computer".to_owned()),
            "no local pane in the window: no row for this computer"
        );
    }

    #[test]
    fn a_machine_leaves_the_tabs_that_have_a_pane_on_it_and_dims_the_others() {
        let mut overview = mixed();
        assert!(overview.set_machine(Machine::Host("rasp".to_owned())));
        assert_eq!(overview.len(), 1);
        let web = overview.tab(0).clone();
        assert_eq!(web.id, 1);
        assert!(!overview.dim(1, &web.panes[0]), "a pane on rasp is lit");
        assert!(
            overview.dim(1, &web.panes[2]),
            "the local pane of web is dim"
        );
        // The same tab is in This Computer's grid, the other way round.
        assert!(overview.set_machine(Machine::Local));
        assert_eq!(
            overview.tabs().map(|tab| tab.id).collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert!(overview.dim(1, &web.panes[0]));
        assert!(!overview.dim(1, &web.panes[2]));
        assert!(!overview.set_machine(Machine::Host("nowhere".to_owned())));
    }

    #[test]
    fn the_selected_tab_stays_unless_the_machine_has_not_got_it() {
        let mut overview = mixed();
        assert_eq!(overview.selected(), Some(2));
        overview.set_machine(Machine::Local);
        assert_eq!(overview.selected(), Some(2), "api is here");
        overview.set_machine(Machine::Host("db1".to_owned()));
        assert_eq!(
            overview.selected(),
            Some(3),
            "the first tab the machine has"
        );
    }

    #[test]
    fn tab_goes_round_the_column_both_ways() {
        let mut overview = mixed();
        let mut seen = vec![overview.machine().title()];
        for _ in 0..4 {
            overview.cycle(true);
            seen.push(overview.machine().title());
        }
        assert_eq!(
            seen,
            vec!["All tabs", "This Computer", "⇄ db1", "⇄ rasp", "All tabs"],
            "after the last server, All tabs again"
        );
        overview.cycle(false);
        assert_eq!(overview.machine(), &Machine::Host("rasp".to_owned()));
    }

    #[test]
    fn a_server_whose_last_tab_is_closed_goes_and_takes_the_selection_with_it() {
        let mut overview = mixed();
        overview.set_machine(Machine::Host("db1".to_owned()));
        overview.refresh(vec![
            with_panes(1, vec![on(13, None, None)]),
            with_panes(2, vec![on(21, None, None)]),
        ]);
        assert_eq!(overview.machine(), &Machine::All);
        assert_eq!(names(&overview), vec!["All tabs", "This Computer"]);
        assert_eq!(overview.len(), 2);
    }

    #[test]
    fn the_pointer_over_a_row_lights_that_machines_panes_and_selects_nothing() {
        let mut overview = mixed();
        let web = overview.tab(0).clone();
        assert!(overview.glance(Some(Machine::Host("rasp".to_owned()))));
        assert!(!overview.dim(1, &web.panes[0]), "rasp's pane lights");
        assert!(overview.dim(1, &web.panes[2]), "the local one goes dim");
        assert_eq!(overview.machine(), &Machine::All);
        assert!(overview.glance(None));
        assert!(!overview.dim(1, &web.panes[2]));
        assert!(
            !overview.glance(Some(Machine::All)),
            "the machine the grid is of is nothing new"
        );
    }

    #[test]
    fn the_arrows_walk_the_grid_the_machine_leaves() {
        let mut overview = mixed();
        overview.set_machine(Machine::Local);
        let layout = overview.layout(width_for(3), 0.6);
        assert_eq!(overview.selected(), Some(2));
        assert!(
            !overview.step(&layout, Direction::Right),
            "api is the last of two"
        );
        assert!(overview.step(&layout, Direction::Left));
        assert_eq!(overview.selected(), Some(1));
    }

    #[test]
    fn the_load_is_said_as_the_card_says_it_with_the_disk_and_the_uptime() {
        let detail = Detail {
            os: Some("Ubuntu 24.04 LTS".to_owned()),
            cores: Some(4),
            cpu: Some(23),
            mem_used: 3 << 30,
            mem_total: 8 << 30,
            disk: Some(54),
            uptime: Some(12 * 86_400 + 5),
            ..Detail::default()
        };
        let load = load(&detail);
        assert_eq!(load.rows[0], ("CPU · 4 cores".to_owned(), "23%".to_owned()));
        assert_eq!(load.rows[1].0, "Memory");
        assert_eq!(load.rows[2], ("Disk".to_owned(), "54%".to_owned()));
        assert_eq!(load.rows[3], ("Up".to_owned(), "12 days".to_owned()));
        assert_eq!(load.os.as_deref(), Some("Ubuntu 24.04 LTS"));
        let bare = super::load(&Detail::default());
        assert_eq!(bare.rows.len(), 2, "no disk and no uptime: no such rows");
        assert_eq!(bare.os, None);
    }

    #[test]
    fn an_uptime_is_said_in_its_biggest_unit() {
        assert_eq!(uptime_text(59), "0 min");
        assert_eq!(uptime_text(5 * 60), "5 min");
        assert_eq!(uptime_text(3 * 3600 + 20 * 60), "3 h 20 min");
        assert_eq!(uptime_text(86_400), "1 day");
        assert_eq!(uptime_text(40 * 86_400), "40 days");
    }
}
