//! What the container list does when it is used: its rows, the cursor and the marks, the
//! filter, the keys, the pointer, the menu — and what each of them changes in the fixed list
//! the page shows.
//!
//! The page is a function of this state; the host tells it what the person did ([`Input`]) and
//! lays it out ([`Containers::settle`]) in between. Nothing here reads a clock or a device.
//! There is no Docker behind the list: an action that would need one (stop, restart, remove)
//! changes the list itself, so a menu does something the person can see.

use super::{
    CHOSEN, Container, Containers, GROUP_BUTTON, GROUP_GAP, HOVERED, LIST, ROW, SEARCH, State, wave,
};
use crate::{
    Button, Cursor, Id, Input, Key, Keyboard, Menu, MenuEntry, Mods, Point, Queries, Reply,
};

/// Which containers the list shows by their state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Status {
    All,
    Running,
    Stopped,
    Issues,
}

impl Status {
    pub(super) const ALL: [Self; 4] = [Self::All, Self::Running, Self::Stopped, Self::Issues];

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Running => "Running",
            Self::Stopped => "Stopped",
            Self::Issues => "Issues",
        }
    }

    pub(super) fn id(self) -> Id {
        Id::named(match self {
            Self::All => "containers status all",
            Self::Running => "containers status running",
            Self::Stopped => "containers status stopped",
            Self::Issues => "containers status issues",
        })
    }

    fn shows(self, c: &Container) -> bool {
        match self {
            Self::All => true,
            Self::Running => c.is_up(),
            Self::Stopped => !c.is_up(),
            Self::Issues => c.has_issue(),
        }
    }
}

/// How many containers each state of the filter would show.
pub(super) struct Counts {
    pub all: usize,
    pub running: usize,
    pub stopped: usize,
    pub issues: usize,
}

/// What names a row of the list: a project's, or a container's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RowKey {
    Group(Option<&'static str>),
    Container(&'static str),
}

impl RowKey {
    pub(super) fn id(self) -> Id {
        match self {
            Self::Group(Some(project)) => Id::named(&format!("containers group {project}")),
            Self::Group(None) => Id::named("containers group standalone"),
            Self::Container(name) => Id::named(&format!("containers container {name}")),
        }
    }
}

/// One row of the list as it stands now.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Row {
    pub key: RowKey,
    pub what: What,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum What {
    /// A project, and the containers of it the filter shows (indexes into the list).
    Group {
        project: Option<&'static str>,
        members: Vec<usize>,
        folded: bool,
    },
    Container(usize),
}

impl Row {
    /// How tall the row stands in the list, with the room above a project's.
    pub(super) fn height(&self) -> f32 {
        match self.what {
            What::Group { .. } => ROW + GROUP_GAP,
            What::Container(_) => ROW,
        }
    }
}

/// What a menu's entries do and to whom, as of the moment the menu was opened.
#[derive(Clone, Debug, Default)]
pub(super) struct MenuState {
    acts: Vec<Option<Act>>,
    targets: Vec<&'static str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    Start,
    Stop,
    Restart,
    Pause,
    Resume,
    CopyId,
    Kill,
    Remove,
}

impl Containers {
    // --- What the page shows ---------------------------------------------------------------

    /// Whether the filter lets `c` through: its state, and the words in the field among its
    /// name, project, image and published ports.
    fn shows(&self, c: &Container) -> bool {
        if !self.status.shows(c) {
            return false;
        }
        let query = self.query.trim().to_lowercase();
        query.is_empty()
            || c.name.to_lowercase().contains(&query)
            || c.project.is_some_and(|p| p.to_lowercase().contains(&query))
            || c.image.to_lowercase().contains(&query)
            || c.ports.iter().any(|port| port.to_string().contains(&query))
    }

    /// The rows of the list: each project's, with the containers under it unless folded, or —
    /// ungrouped — the containers by name. A filter at work unfolds every project, so what it
    /// finds is not hidden in a folded one.
    pub(super) fn rows(&self) -> Vec<Row> {
        let shown: Vec<usize> = (0..self.containers.len())
            .filter(|at| self.shows(&self.containers[*at]))
            .collect();
        let container_row = |at: usize| Row {
            key: RowKey::Container(self.containers[at].name),
            what: What::Container(at),
        };
        if !self.grouped {
            let mut flat = shown;
            flat.sort_by_key(|at| self.containers[*at].name);
            return flat.into_iter().map(container_row).collect();
        }
        let filtering = self.status != Status::All || !self.query.trim().is_empty();
        let mut rows = Vec::new();
        let mut done: Vec<Option<&'static str>> = Vec::new();
        for &at in &shown {
            let project = self.containers[at].project;
            if done.contains(&project) {
                continue;
            }
            done.push(project);
            let members: Vec<usize> = shown
                .iter()
                .copied()
                .filter(|member| self.containers[*member].project == project)
                .collect();
            let folded = !filtering && project.is_some_and(|p| self.collapsed.contains(&p));
            rows.push(Row {
                key: RowKey::Group(project),
                what: What::Group {
                    project,
                    members: members.clone(),
                    folded,
                },
            });
            if !folded {
                rows.extend(members.into_iter().map(container_row));
            }
        }
        rows
    }

    pub(super) fn counts(&self) -> Counts {
        let count = |status: Status| self.containers.iter().filter(|c| status.shows(c)).count();
        Counts {
            all: self.containers.len(),
            running: count(Status::Running),
            stopped: count(Status::Stopped),
            issues: count(Status::Issues),
        }
    }

    /// How lit the element named `id` is from the pointer being over it, `0.0..=1.0`.
    pub(super) fn glow(&self, id: Id) -> f32 {
        self.ui.fades.level(id.role(HOVERED))
    }

    // --- Moving the fades ----------------------------------------------------------------

    fn fade_to(&mut self, id: Id, target: f32) {
        if self.reduce_motion {
            self.ui.fades.snap(id, target);
        } else {
            self.ui.fades.to(id, target);
        }
    }

    /// The cursor goes to `key` (nowhere for `None`): the row it left fades out and the one it
    /// came to fades in. Returns whether it moved.
    fn set_cursor(&mut self, key: Option<RowKey>) -> bool {
        if self.cursor == key {
            return false;
        }
        if let Some(old) = self.cursor {
            self.fade_to(old.id().role(CHOSEN), 0.0);
        }
        if let Some(new) = key {
            self.fade_to(new.id().role(CHOSEN), 1.0);
        }
        self.cursor = key;
        true
    }

    fn hover_to(&mut self, laid: &dyn Queries, at: Option<Point>) -> bool {
        let was = self.ui.hover();
        if !self.ui.point(laid, at) {
            return false;
        }
        if let Some(old) = was {
            self.fade_to(old.role(HOVERED), 0.0);
        }
        if let Some(new) = self.ui.hover() {
            self.fade_to(new.role(HOVERED), 1.0);
        }
        true
    }

    /// The step of the fades to `now`; whether any is still on the way. Under Reduce Motion
    /// every change is already at its end.
    pub fn advance(&mut self, now: f64, reduce_motion: bool) -> bool {
        self.reduce_motion = reduce_motion;
        self.ui.fades.advance(now, reduce_motion)
    }

    /// Whether anything of the page is still moving: the host draws another frame while it is.
    pub fn animating(&self) -> bool {
        self.ui.fades.moving()
    }

    /// The page was laid out as `laid`: what depends on where things stand is brought up to
    /// date — the scrolled list kept within its reach, the window's height, what the pointer is
    /// over now that the page moved under it. Returns whether the page has to be built and laid
    /// out again (the rows in view depend on how far the list is scrolled).
    pub fn settle(&mut self, laid: &dyn Queries) -> bool {
        if let Some(extent) = laid.scroll_extent(LIST) {
            self.list_view = extent.view;
        }
        let moved = self.ui.settle(laid);
        if let Some(at) = self.pointer {
            self.hover_to(laid, Some(at));
        }
        moved
    }

    // --- The cursor --------------------------------------------------------------------

    fn cursor_at(&self, rows: &[Row]) -> Option<usize> {
        let key = self.cursor?;
        rows.iter().position(|row| row.key == key)
    }

    /// After the rows changed: the cursor stays on its row if it is still there, else it goes
    /// to the row nearest the place it had (`was`, an index into the rows before) — the cursor
    /// is on a row whenever there is one.
    fn rehome(&mut self, was: Option<usize>) {
        let rows = self.rows();
        if self.cursor_at(&rows).is_some() {
            return;
        }
        // With nothing chosen before (the filter found nothing, and then something), the first.
        let to = was.unwrap_or(0).min(rows.len().saturating_sub(1));
        self.set_cursor(rows.get(to).map(|row| row.key));
    }

    /// Scrolls the least that shows the cursor's row.
    fn reveal_cursor(&mut self) {
        let rows = self.rows();
        let Some(at) = self.cursor_at(&rows) else {
            return;
        };
        let places = Self::places(&rows);
        let offset = self.ui.scroll_offset(LIST);
        let to = places.reveal(at, offset, self.list_view);
        if to != offset {
            self.ui.set_scroll(LIST, to);
        }
    }

    // --- The reply -----------------------------------------------------------------------

    fn reply(&self, redraw: bool) -> Reply {
        Reply {
            redraw,
            cursor: match self.ui.hover() {
                // The host's text field is over the filter's box: its own shape.
                Some(SEARCH) => Cursor::Text,
                Some(_) => Cursor::Pointer,
                None => Cursor::Arrow,
            },
            ..Reply::default()
        }
    }

    // --- What the person does ------------------------------------------------------------

    /// What the person did, with the page as it was last laid out (`laid`).
    pub fn input(&mut self, input: Input, laid: &dyn Queries) -> Reply {
        match input {
            Input::Move(at) => {
                self.pointer = Some(at);
                let changed = self.hover_to(laid, Some(at));
                self.reply(changed)
            }
            Input::Leave => {
                self.pointer = None;
                let changed = self.hover_to(laid, None);
                self.reply(changed)
            }
            Input::Press { at, button, mods } => self.press(laid, at, button, mods),
            Input::Release { at, button } => match button {
                Button::Left => self.release(laid, at),
                Button::Right => self.reply(false),
            },
            Input::Scroll { at, dy } => {
                self.pointer = Some(at);
                let moved = self.ui.scroll_by(laid, at, dy);
                self.reply(moved)
            }
            Input::Key { key, mods } => self.key(key, mods, laid),
        }
    }

    fn row_of(&self, id: Id) -> Option<Row> {
        self.rows().into_iter().find(|row| row.key.id() == id)
    }

    fn press(&mut self, laid: &dyn Queries, at: Point, button: Button, mods: Mods) -> Reply {
        self.pointer = Some(at);
        self.press_mods = mods;
        let Some(id) = self.ui.press(laid, at) else {
            return self.reply(false);
        };
        if id == SEARCH {
            return Reply {
                keyboard: Some(Keyboard::Filter),
                ..self.reply(false)
            };
        }
        let Some(row) = self.row_of(id) else {
            return self.reply(false);
        };
        let mut changed = self.set_cursor(Some(row.key));
        if button == Button::Left && (mods.command || mods.control || mods.shift) {
            // ⌘-, ⌃- and ⇧-click mark, as the mouse's way to the space bar.
            changed |= self.toggle_mark(&row);
        }
        if button == Button::Right {
            let menu = self.open_menu(at);
            return Reply {
                redraw: changed,
                menu,
                ..self.reply(changed)
            };
        }
        self.reply(changed)
    }

    fn release(&mut self, laid: &dyn Queries, at: Point) -> Reply {
        let Some(id) = self.ui.release(laid, at) else {
            return self.reply(false);
        };
        let rows = self.rows();
        let was = self.cursor_at(&rows);
        if let Some(status) = Status::ALL.into_iter().find(|status| status.id() == id) {
            let changed = std::mem::replace(&mut self.status, status) != status;
            self.rehome(was);
            return self.reply(changed);
        }
        if id == GROUP_BUTTON {
            self.grouped = !self.grouped;
            self.rehome(was);
            return self.reply(true);
        }
        if let Some(Row {
            what: What::Group {
                project, folded, ..
            },
            ..
        }) = self.row_of(id)
            && let Some(project) = project
            && !(self.press_mods.command || self.press_mods.control || self.press_mods.shift)
        {
            self.fold(project, !folded);
            return self.reply(true);
        }
        self.reply(false)
    }

    fn fold(&mut self, project: &'static str, folded: bool) {
        self.collapsed.retain(|p| *p != project);
        if folded {
            self.collapsed.push(project);
        }
    }

    /// Marks the row's containers, or clears them when all are marked. Returns whether any mark
    /// changed.
    fn toggle_mark(&mut self, row: &Row) -> bool {
        let names: Vec<&'static str> = match &row.what {
            What::Container(at) => vec![self.containers[*at].name],
            What::Group { members, .. } => {
                members.iter().map(|at| self.containers[*at].name).collect()
            }
        };
        if names.is_empty() {
            return false;
        }
        if names.iter().all(|name| self.marked.contains(name)) {
            self.marked.retain(|marked| !names.contains(marked));
        } else {
            for name in names {
                if !self.marked.contains(&name) {
                    self.marked.push(name);
                }
            }
        }
        true
    }

    /// The keyboard's: moving the cursor, folding a project, marking, the filter, the
    /// commands. ⌃, ⌥ and ⌘ chords are the application's and are not read here.
    fn key(&mut self, key: Key, mods: Mods, laid: &dyn Queries) -> Reply {
        if mods.any_command() {
            return self.reply(false);
        }
        let rows = self.rows();
        let at = self.cursor_at(&rows);
        let page = ((self.list_view / ROW) as usize).saturating_sub(1).max(1);
        let last = rows.len().saturating_sub(1);
        let move_to = |to: usize| Some(to.min(last));
        let target = match key {
            Key::Down | Key::Char('j') => move_to(at.map_or(0, |at| at + 1)),
            Key::Up | Key::Char('k') => move_to(at.map_or(0, |at| at.saturating_sub(1))),
            Key::Home | Key::Char('g') => move_to(0),
            Key::End | Key::Char('G') => move_to(last),
            Key::PageDown => move_to(at.map_or(0, |at| at + page)),
            Key::PageUp => move_to(at.map_or(0, |at| at.saturating_sub(page))),
            _ => None,
        };
        if let Some(to) = target {
            let changed = rows
                .get(to)
                .is_some_and(|row| self.set_cursor(Some(row.key)));
            self.reveal_cursor();
            return self.reply(changed);
        }
        match key {
            Key::Escape => Reply {
                close: true,
                ..Reply::none()
            },
            Key::Tab | Key::Char('/') => Reply {
                keyboard: Some(Keyboard::Filter),
                ..self.reply(false)
            },
            Key::Enter => self.fold_key(&rows, at, None),
            Key::Right => self.fold_key(&rows, at, Some(false)),
            Key::Left => {
                let Some(row) = at.and_then(|at| rows.get(at)) else {
                    return self.reply(false);
                };
                if let What::Group { .. } = row.what {
                    return self.fold_key(&rows, at, Some(true));
                }
                // From a container, back to the project it is under.
                let project = match row.what {
                    What::Container(c) => self.containers[c].project,
                    What::Group { .. } => None,
                };
                let changed = self.grouped && self.set_cursor(Some(RowKey::Group(project)));
                self.reveal_cursor();
                self.reply(changed)
            }
            Key::Char(' ') => {
                let Some(row) = at.and_then(|at| rows.get(at)) else {
                    return self.reply(false);
                };
                let changed = self.toggle_mark(row);
                // Marking a container goes on to the next, to mark a run with the bar.
                if matches!(row.what, What::Container(_))
                    && let Some(next) = rows.get(at.unwrap_or(0) + 1)
                {
                    self.set_cursor(Some(next.key));
                    self.reveal_cursor();
                }
                self.reply(changed || at.is_some())
            }
            Key::Char('a') => {
                self.marked = rows
                    .iter()
                    .filter_map(|row| match row.what {
                        What::Container(at) => Some(self.containers[at].name),
                        What::Group { .. } => None,
                    })
                    .collect();
                self.reply(true)
            }
            Key::Char('A') => {
                let changed = !self.marked.is_empty();
                self.marked.clear();
                self.reply(changed)
            }
            Key::Char('f') => {
                let next = Status::ALL[(Status::ALL
                    .iter()
                    .position(|s| *s == self.status)
                    .unwrap_or(0)
                    + 1)
                    % 4];
                self.status = next;
                self.rehome(at);
                self.reveal_cursor();
                self.reply(true)
            }
            Key::Char('P') => {
                self.grouped = !self.grouped;
                self.rehome(at);
                self.reveal_cursor();
                self.reply(true)
            }
            Key::Char('.') => {
                // Under the row, a little in from its start: where the key's menu opens.
                let anchor = self
                    .cursor
                    .and_then(|key| laid.bounds(key.id()))
                    .map_or(Point::new(48.0, 120.0), |b| {
                        Point::new(b.x + 40.0, b.y + b.h - 4.0)
                    });
                Reply {
                    menu: self.open_menu(anchor),
                    ..self.reply(false)
                }
            }
            Key::Char(command) => match Self::command(command) {
                Some(act) => self.run(act, at),
                None => self.reply(false),
            },
            _ => self.reply(false),
        }
    }

    /// ⏎ (`None`: fold or unfold), → (`Some(false)`: unfold) and ← (`Some(true)`: fold) on a
    /// project's row.
    fn fold_key(&mut self, rows: &[Row], at: Option<usize>, fold: Option<bool>) -> Reply {
        let Some(Row {
            what:
                What::Group {
                    project: Some(project),
                    folded,
                    ..
                },
            ..
        }) = at.and_then(|at| rows.get(at))
        else {
            return self.reply(false);
        };
        let to = fold.unwrap_or(!folded);
        if to == *folded {
            return self.reply(false);
        }
        self.fold(project, to);
        self.reveal_cursor();
        self.reply(true)
    }

    // --- The filter ----------------------------------------------------------------------

    /// The text of the filter field changed.
    pub fn set_query(&mut self, text: &str) -> Reply {
        if self.query == text {
            return self.reply(false);
        }
        let rows = self.rows();
        let was = self.cursor_at(&rows);
        self.query = text.to_owned();
        self.rehome(was);
        self.reveal_cursor();
        self.reply(true)
    }

    // --- Acting on containers ------------------------------------------------------------

    /// The containers the commands and the menu act on: the marked ones, or else the cursor's
    /// row — a container, or a project's containers the filter shows.
    fn targets(&self) -> Vec<&'static str> {
        if !self.marked.is_empty() {
            return self
                .containers
                .iter()
                .map(|c| c.name)
                .filter(|name| self.marked.contains(name))
                .collect();
        }
        let rows = self.rows();
        match self
            .cursor_at(&rows)
            .and_then(|at| rows.get(at))
            .map(|r| &r.what)
        {
            Some(What::Container(at)) => vec![self.containers[*at].name],
            Some(What::Group { members, .. }) => {
                members.iter().map(|at| self.containers[*at].name).collect()
            }
            None => Vec::new(),
        }
    }

    fn selected(&self, names: &[&'static str]) -> Vec<&Container> {
        self.containers
            .iter()
            .filter(|c| names.contains(&c.name))
            .collect()
    }

    /// The command a key is, as the list's own keys have it.
    fn command(key: char) -> Option<Act> {
        Some(match key {
            's' => Act::Stop,
            'r' => Act::Restart,
            'p' => Act::Pause,
            'y' => Act::CopyId,
            'K' => Act::Kill,
            'd' => Act::Remove,
            _ => return None,
        })
    }

    /// Runs the command a key is on its targets: `s` stops what runs and starts what does
    /// not, `p` pauses or resumes.
    fn run(&mut self, act: Act, was: Option<usize>) -> Reply {
        let targets = self.targets();
        self.apply(act, &targets, was)
    }

    fn apply(&mut self, act: Act, names: &[&'static str], was: Option<usize>) -> Reply {
        if names.is_empty() {
            return self.reply(false);
        }
        let selected = self.selected(names);
        let any_up = selected.iter().any(|c| c.is_up());
        let any_paused = selected.iter().any(|c| c.state == State::Paused);
        // The keys are one for two things: `s` stops what is up, or starts what is not.
        let act = match act {
            Act::Stop if !any_up => Act::Start,
            Act::Pause if any_paused => Act::Resume,
            other => other,
        };
        let mut copy = None;
        let mut removed = Vec::new();
        for c in self
            .containers
            .iter_mut()
            .filter(|c| names.contains(&c.name))
        {
            match act {
                Act::Start if !c.is_up() => c.begin("up 0s", State::Running),
                Act::Stop if c.is_up() => c.end("now", 0),
                Act::Restart => c.begin("up 0s", State::Running),
                Act::Pause if c.is_up() && c.state != State::Paused => c.state = State::Paused,
                Act::Resume if c.state == State::Paused => c.state = State::Running,
                Act::Kill if c.is_up() => c.end("now", 137),
                Act::Remove => removed.push(c.name),
                Act::CopyId => copy = Some(c.id()),
                _ => {}
            }
        }
        if !removed.is_empty() {
            self.containers.retain(|c| !removed.contains(&c.name));
            self.marked.retain(|name| !removed.contains(name));
        }
        self.rehome(was);
        self.reveal_cursor();
        Reply {
            copy,
            ..self.reply(true)
        }
    }

    /// The menu of the row the cursor is on (or of the marked containers): the entries, in the
    /// order of the list's own keys; those that would need a program behind the list are shown
    /// and grey. `None` where there is nothing to act on.
    fn open_menu(&mut self, at: Point) -> Option<Menu> {
        let targets = self.targets();
        if targets.is_empty() {
            return None;
        }
        let selected = self.selected(&targets);
        let single = selected.len() == 1;
        let any_up = selected.iter().any(|c| c.is_up());
        let any_down = selected.iter().any(|c| !c.is_up());
        let any_paused = selected.iter().any(|c| c.state == State::Paused);
        let mut entries = Vec::new();
        let mut acts = Vec::new();
        let mut item = |title: &str, key: Option<char>, act: Option<Act>| {
            entries.push(MenuEntry::Item {
                title: title.to_owned(),
                key,
                enabled: act.is_some(),
            });
            acts.push(act);
        };
        if any_down {
            item("Start", Some('s'), Some(Act::Start));
        }
        if any_up {
            item("Stop", Some('s'), Some(Act::Stop));
        }
        item("Restart", Some('r'), Some(Act::Restart));
        if any_paused {
            item("Resume", Some('p'), Some(Act::Resume));
        } else if any_up {
            item("Pause", Some('p'), Some(Act::Pause));
        }
        let divide = |entries: &mut Vec<MenuEntry>, acts: &mut Vec<Option<Act>>| {
            if !matches!(entries.last(), None | Some(MenuEntry::Separator)) {
                entries.push(MenuEntry::Separator);
                acts.push(None);
            }
        };
        // Without a program behind the list these have nothing to show: grey, but there, as in
        // the tool this list is drawn from.
        let mut grey = Vec::new();
        if single {
            grey.extend([("Open a shell", 'e'), ("Logs", 'l'), ("Inspect", 'i')]);
        }
        let mut tail = Vec::new();
        if single {
            tail.push(("Copy id", Some('y'), Some(Act::CopyId)));
        }
        divide(&mut entries, &mut acts);
        for (title, key) in grey {
            entries.push(MenuEntry::Item {
                title: title.to_owned(),
                key: Some(key),
                enabled: false,
            });
            acts.push(None);
        }
        for (title, key, act) in tail {
            entries.push(MenuEntry::Item {
                title: title.to_owned(),
                key,
                enabled: act.is_some(),
            });
            acts.push(act);
        }
        divide(&mut entries, &mut acts);
        if any_up {
            entries.push(MenuEntry::Item {
                title: "Kill".to_owned(),
                key: Some('K'),
                enabled: true,
            });
            acts.push(Some(Act::Kill));
        }
        entries.push(MenuEntry::Item {
            title: "Remove".to_owned(),
            key: Some('d'),
            enabled: true,
        });
        acts.push(Some(Act::Remove));
        self.menu = MenuState { acts, targets };
        Some(Menu { at, entries })
    }

    /// The person picked entry `index` of the menu that was opened.
    pub fn pick(&mut self, index: usize) -> Reply {
        let MenuState { acts, targets } = std::mem::take(&mut self.menu);
        let Some(Some(act)) = acts.get(index).copied() else {
            return self.reply(false);
        };
        let rows = self.rows();
        let was = self.cursor_at(&rows);
        self.apply(act, &targets, was)
    }
}

impl Container {
    /// Runs again (or for the first time): up since now, with a quiet line to show.
    fn begin(&mut self, age: &'static str, state: State) {
        if !self.is_up() || self.history.is_empty() {
            let seed = self.name.bytes().fold(7_u32, |sum, byte| {
                sum.wrapping_mul(31).wrapping_add(u32::from(byte))
            });
            self.history = wave(seed, 0.3, 1.0);
            self.memory = ("12MiB".to_owned(), 0.06);
            self.cpu = 0.3;
        }
        self.state = state;
        self.age = age;
    }

    /// Ended with `code`, `age` ago.
    fn end(&mut self, age: &'static str, code: i32) {
        self.state = State::Exited(code);
        self.age = age;
        self.cpu = 0.0;
    }

    /// A made-up id, the same every time for the same name: twelve hex digits.
    fn id(&self) -> String {
        let hash = self
            .name
            .bytes()
            .fold(0xcbf2_9ce4_8422_2325_u64, |sum, byte| {
                (sum ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
            });
        format!("{:012x}", hash & 0xffff_ffff_ffff)
    }
}
