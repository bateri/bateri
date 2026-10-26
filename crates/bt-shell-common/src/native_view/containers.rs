//! A container list, in the look of a Docker tool's own: compose projects with their
//! containers under them, a state dot, the name, the image, a CPU sparkline, memory against
//! its limit, the published ports and how long it has been so; above it the section's
//! title, the filter, the tabs of what to show; beside it the sections' rail; below it the
//! line of keys.
//!
//! The data is fixed (`Containers::fixture`) — the page is a sample of what the engine does, not a
//! client of anything — and the page is a function of the size it is given: a narrower pane
//! drops the columns that matter least, the way the list this was drawn from does, and a
//! narrow one drops the rail too.

use pencere_view::{Align, Element, Id, Role, Rows, Size, Track, Ui, div, spacer, spark, text};

mod interact;

use interact::{Row, RowKey, Status, What};

/// The rail folds away under this page width, points.
const RAIL_BELOW: f32 = 780.0;
/// The rail's width, points.
const RAIL_WIDTH: f32 = 188.0;
/// The columns the list drops as it narrows, by the list's width, points.
const MEDIUM_BELOW: f32 = 860.0;
const NARROW_BELOW: f32 = 660.0;
const TINY_BELOW: f32 = 470.0;
/// One row's height, points.
const ROW: f32 = 32.0;
/// The room above a project's row, points.
const GROUP_GAP: f32 = 6.0;
/// The room before the list's first row and after its last, points.
const LIST_LEAD: f32 = 6.0;
const LIST_TRAIL: f32 = 8.0;

/// The list of containers: the box that scrolls.
pub(crate) const LIST: Id = Id::named("containers list");
/// The filter's box, and the room inside it the host's text field takes.
pub(crate) const SEARCH: Id = Id::named("containers filter");
pub(crate) const FIELD: Id = Id::named("containers filter field");
/// What the host's field says while it is empty: the text in its room.
pub(crate) const FIELD_HINT: &str = "Filter by name, image, port\u{2026}";
/// The button that groups the list by project.
const GROUP_BUTTON: Id = Id::named("containers group");
/// The button that would sort it: there is one order in the fixed list, so it only looks it.
const SORT_BUTTON: Id = Id::named("containers sort");

/// The roles of an element's fades (`Id::role`).
const HOVERED: u8 = 0;
const CHOSEN: u8 = 1;

/// What a container is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Running,
    /// Running, and its health check has not passed yet.
    Starting,
    /// Running, and its health check fails.
    Unhealthy,
    /// Frozen: still there, doing nothing.
    Paused,
    /// Ended with this exit code.
    Exited(i32),
}

/// One container of the fixed list.
#[derive(Clone, Debug)]
pub struct Container {
    pub name: &'static str,
    pub project: Option<&'static str>,
    pub image: &'static str,
    pub state: State,
    /// CPU now, percent of one core.
    pub cpu: f32,
    /// CPU over the last seconds, newest last.
    pub history: Vec<f32>,
    /// Memory now, and its share of what the container may use (`0.0..=1.0`).
    pub memory: (String, f32),
    pub ports: Vec<u16>,
    /// How long it has been so: `up 3m`, `2m ago`.
    pub age: &'static str,
}

impl Container {
    fn is_up(&self) -> bool {
        !matches!(self.state, State::Exited(_))
    }

    fn has_issue(&self) -> bool {
        matches!(
            self.state,
            State::Unhealthy | State::Exited(1..) | State::Exited(..=-1)
        )
    }
}

/// The columns the list has room for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Width {
    Wide,
    Medium,
    Narrow,
    Tiny,
}

impl Width {
    fn of(list: f32) -> Self {
        if list < TINY_BELOW {
            Self::Tiny
        } else if list < NARROW_BELOW {
            Self::Narrow
        } else if list < MEDIUM_BELOW {
            Self::Medium
        } else {
            Self::Wide
        }
    }

    fn image(self) -> bool {
        matches!(self, Self::Wide | Self::Medium)
    }

    fn ports(self) -> bool {
        self == Self::Wide
    }

    fn memory(self) -> bool {
        self != Self::Tiny
    }

    /// The grid's columns: the dot, the name, then what fits.
    fn tracks(self) -> Vec<Track> {
        let mut tracks = vec![Track::px(18.0)];
        tracks.push(match self {
            Self::Wide | Self::Medium => Track::range(120.0, 1.5),
            Self::Narrow => Track::range(100.0, 1.0),
            Self::Tiny => Track::range(90.0, 1.0),
        });
        if self.image() {
            tracks.push(Track::range(
                if self == Self::Wide { 90.0 } else { 80.0 },
                1.0,
            ));
        }
        tracks.push(Track::px(96.0));
        if self.memory() {
            tracks.push(Track::px(110.0));
        }
        if self.ports() {
            tracks.push(Track::range(64.0, 0.8));
        }
        tracks.push(Track::px(128.0));
        tracks
    }
}

/// The page: the list and what the person has done to it.
#[derive(Clone, Debug)]
pub struct Containers {
    containers: Vec<Container>,
    /// The row the cursor is on.
    cursor: Option<RowKey>,
    /// The containers marked, to act on many at once.
    marked: Vec<&'static str>,
    /// Projects whose rows are folded away.
    collapsed: Vec<&'static str>,
    /// What the filter field says.
    query: String,
    /// Which state of container to show.
    status: Status,
    /// Whether the list is grouped by project.
    grouped: bool,
    /// What the page remembers between builds: the hover, the scroll, the fades.
    ui: Ui,
    /// Where the pointer last was over the page, points: the hover is asked again where it
    /// stands when the page moves under it.
    pointer: Option<pencere_view::Point>,
    /// How tall the list's window was when the page was last laid out, points.
    list_view: f32,
    /// What the entries of the menu that is up do, by entry; and the containers they do it to.
    menu: interact::MenuState,
    /// The modifiers held when the last button went down: a click on a project's row folds it
    /// unless it was a click that marks.
    press_mods: pencere_view::Mods,
    /// Reduce Motion, as of the last frame: a change snaps.
    reduce_motion: bool,
}

impl Containers {
    /// A small, made-up Docker host: a shop with a container in each state, a blog that is
    /// stopped, a project folded away and one container of no project.
    pub fn fixture() -> Self {
        let running =
            |name, project, image, cpu, history, memory: (&str, f32), ports, age| Container {
                name,
                project,
                image,
                state: State::Running,
                cpu,
                history,
                memory: (memory.0.to_owned(), memory.1),
                ports,
                age,
            };
        let shop = Some("shop");
        let blog = Some("blog");
        let stats = Some("stats");
        let stopped = |name, project, image, code, age| Container {
            name,
            project,
            image,
            state: State::Exited(code),
            cpu: 0.0,
            history: Vec::new(),
            memory: (String::new(), 0.0),
            ports: Vec::new(),
            age,
        };
        let containers = vec![
            running(
                "api",
                shop,
                "shop/api:2.4",
                0.9,
                wave(3, 0.8, 4.0),
                ("112MiB", 0.22),
                vec![8080],
                "up 3m",
            ),
            running(
                "cache",
                shop,
                "redis:alpine",
                0.3,
                wave(5, 0.4, 2.5),
                ("7.2MiB", 0.04),
                vec![6379],
                "up 3m",
            ),
            running(
                "db",
                shop,
                "postgres:17-alpine",
                1.2,
                wave(11, 1.0, 6.0),
                ("203MiB", 0.48),
                vec![5432],
                "up 3m",
            ),
            stopped("mailer", shop, "mailhog/mailhog", 1, "2m ago"),
            Container {
                state: State::Unhealthy,
                ..running(
                    "search",
                    shop,
                    "opensearch:2.14",
                    0.0,
                    wave(2, 0.1, 9.0),
                    ("976MiB", 0.81),
                    vec![9200],
                    "up 3m",
                )
            },
            running(
                "web",
                shop,
                "nginx:alpine",
                0.0,
                wave(7, 0.0, 3.0),
                ("6.1MiB", 0.03),
                vec![8081],
                "up 3m",
            ),
            Container {
                state: State::Starting,
                ..running(
                    "worker",
                    shop,
                    "shop/worker:2.4",
                    37.6,
                    wave(13, 30.0, 14.0),
                    ("936KiB", 0.01),
                    Vec::new(),
                    "up 12s",
                )
            },
            stopped("ghost", blog, "ghost:5-alpine", 0, "3d ago"),
            stopped("mysql", blog, "mysql:8", 0, "3d ago"),
            running(
                "collector",
                stats,
                "prom/node-exporter",
                0.1,
                wave(17, 0.1, 0.5),
                ("18.4MiB", 0.09),
                vec![9100],
                "up 2d",
            ),
            running(
                "grafana",
                stats,
                "grafana/grafana",
                0.4,
                wave(19, 0.4, 1.2),
                ("91MiB", 0.19),
                vec![3000],
                "up 2d",
            ),
            stopped("alerts", stats, "prom/alertmanager", 0, "2d ago"),
            running(
                "toolbox",
                None,
                "busybox",
                0.0,
                wave(23, 0.0, 0.2),
                ("128KiB", 0.0),
                Vec::new(),
                "up 2m",
            ),
        ];
        let mut page = Self {
            containers,
            cursor: Some(RowKey::Container("api")),
            marked: Vec::new(),
            collapsed: vec!["stats"],
            query: String::new(),
            status: Status::All,
            grouped: true,
            ui: Ui::default(),
            pointer: None,
            list_view: 400.0,
            menu: interact::MenuState::default(),
            press_mods: pencere_view::Mods::default(),
            reduce_motion: false,
        };
        // The choice starts shown, not fading in.
        if let Some(key) = page.cursor {
            page.ui.fades.snap(key.id().role(CHOSEN), 1.0);
        }
        page
    }

    /// The page for a pane of `size` points.
    pub fn view(&self, size: Size) -> Element {
        let rail = size.w >= RAIL_BELOW;
        let list_width = size.w - if rail { RAIL_WIDTH + 1.0 } else { 0.0 } - 16.0;
        let width = Width::of(list_width);
        let main = div()
            .col()
            .grow(1.0)
            .min_w(0.0)
            .clip()
            .child(self.header())
            .child(self.toolbar())
            .child(self.list(width, size.h));
        let body = div()
            .row()
            .grow(1.0)
            .min_h(0.0)
            .clip()
            .child_if(rail.then(|| self.rail()))
            .child(main);
        div()
            .col()
            .size_full()
            .bg(Role::Background)
            .text_size(12.5)
            .color(Role::Foreground)
            .child(body)
            .child(status_line(size.w))
    }

    fn up(&self) -> usize {
        self.containers.iter().filter(|c| c.is_up()).count()
    }

    fn issues(&self) -> usize {
        self.containers.iter().filter(|c| c.has_issue()).count()
    }

    // --- The rail ------------------------------------------------------------------------

    fn rail(&self) -> Element {
        let item = |label: &str, current: bool, count: Option<String>, badge: Option<usize>| {
            let mut row = div()
                .row()
                .items_center()
                .gap(10.0)
                .h(30.0)
                .px(8.0)
                .rounded(6.0)
                .no_shrink()
                .color(if current { Role::Foreground } else { Role::Dim })
                .child(text(label).grow(1.0).truncate());
            if current {
                row = row
                    .bg(Role::Accent.alpha(0.13))
                    .border(1.0, Role::Accent.alpha(0.24));
            }
            match (badge, count) {
                (Some(n), _) => row.child(
                    div()
                        .row()
                        .items_center()
                        .justify_center()
                        .min_w(16.0)
                        .h(16.0)
                        .px(4.0)
                        .rounded(8.0)
                        .bg(Role::Error)
                        .child(
                            text(n.to_string())
                                .text_size(10.0)
                                .weight(650)
                                .color(Role::Background),
                        ),
                ),
                (None, Some(count)) => row.child(text(count).text_size(11.0).color(Role::Quiet)),
                (None, None) => row,
            }
        };
        let issues = self.issues();
        let nav = div()
            .col()
            .gap(2.0)
            .child(item(
                "Containers",
                true,
                None,
                (issues > 0).then_some(issues),
            ))
            .child(item("Images", false, Some("14".into()), None))
            .child(item("Volumes", false, Some("6".into()), None))
            .child(item("Networks", false, Some("4".into()), None))
            .child(item("System", false, None, None))
            .child(item("Activity", false, None, None));

        let meter = |label: &str, values: &[f32], tone: Role, value: &str| {
            div()
                .row()
                .items_center()
                .gap(6.0)
                .text_size(10.5)
                .child(text(label).color(Role::Quiet).w(26.0))
                .child(spark(values, tone).size(70.0, 14.0))
                .child(
                    text(value)
                        .color(Role::Dim)
                        .weight(500)
                        .grow(1.0)
                        .align(Align::End),
                )
        };
        let engine = div()
            .col()
            .gap(6.0)
            .p(8.0)
            .rounded(8.0)
            .bg(Role::Background.alpha(0.55))
            .border(1.0, Role::Border)
            .child(
                div()
                    .row()
                    .items_center()
                    .gap(7.0)
                    .text_size(11.0)
                    .color(Role::Dim)
                    .child(dot(7.0, Role::Success, false))
                    .child(text("Alpine Linux v3.24").truncate()),
            )
            .child(meter("cpu", &wave(29, 30.0, 12.0), Role::Accent, "39%"))
            .child(meter("mem", &wave(31, 40.0, 3.0), Role::Success, "36.8MiB"))
            .child(
                text(format!("{} running \u{b7} 6 cpu", self.up()))
                    .text_size(10.5)
                    .color(Role::Quiet)
                    .truncate(),
            );
        div()
            .row()
            .no_shrink()
            .child(
                div()
                    .col()
                    .w(RAIL_WIDTH)
                    .min_h(0.0)
                    .clip()
                    .px(8.0)
                    .pt(10.0)
                    .pb(8.0)
                    .bg(Role::Raised(1))
                    .child(nav)
                    .child(spacer())
                    .child(engine),
            )
            .child(div().w(1.0).bg(Role::Border))
    }

    // --- The section's top -----------------------------------------------------------------

    fn header(&self) -> Element {
        let ghost = |label: &str| {
            div()
                .row()
                .items_center()
                .h(28.0)
                .px(10.0)
                .rounded(6.0)
                .color(Role::Dim)
                .child(text(label))
        };
        div()
            .row()
            .items_center()
            .gap(10.0)
            .px(14.0)
            .pt(10.0)
            .pb(8.0)
            .min_h(46.0)
            .child(text("Containers").text_size(13.5).weight(650).no_shrink())
            .child(
                text(format!(
                    "{} running \u{b7} {} total",
                    self.up(),
                    self.containers.len()
                ))
                .text_size(11.5)
                .color(Role::Quiet)
                .truncate(),
            )
            .child(spacer())
            .child(ghost("Pull"))
            .child(ghost("Clean up"))
    }

    fn toolbar(&self) -> Element {
        // The host's text field stands in the box: the page draws the box and the key that
        // reaches it, and keeps the room (`FIELD`).
        let search = div()
            .row()
            .items_center()
            .gap(6.0)
            .h(28.0)
            .w(220.0)
            .min_w(120.0)
            .px(8.0)
            .rounded(6.0)
            .bg(Role::Raised(1))
            .border(1.0, Role::Border)
            .press(SEARCH)
            .child(
                div()
                    .text_field(FIELD)
                    .grow(1.0)
                    .min_w(0.0)
                    .h(18.0)
                    .child(text(FIELD_HINT)),
            )
            .child(kbd("/", 10.5));
        let counts = self.counts();
        let option = |status: Status, count: usize, tone: Option<Role>| {
            let active = self.status == status;
            let id = status.id();
            let glow = self.glow(id);
            let mut cell = div()
                .row()
                .items_center()
                .gap(6.0)
                .h(24.0)
                .px(9.0)
                .rounded(6.0)
                .press(id)
                .color(if active || glow > 0.0 {
                    Role::Foreground
                } else {
                    Role::Dim
                });
            if active {
                cell = cell.bg(Role::Raised(2)).border(1.0, Role::Border).shadow(
                    0.0,
                    1.0,
                    2.0,
                    Role::Shadow.alpha(0.5),
                );
            } else if glow > 0.0 {
                cell = cell.bg(Role::Foreground.alpha(0.07 * glow));
            }
            let count_color = tone.unwrap_or(if active { Role::Dim } else { Role::Quiet });
            cell.child(text(status.label()))
                .child(text(count.to_string()).text_size(10.5).color(count_color))
        };
        let segmented = div()
            .row()
            .p(2.0)
            .gap(1.0)
            .rounded(8.0)
            .bg(Role::Raised(1))
            .border(1.0, Role::Border)
            .no_shrink()
            .child(option(Status::All, counts.all, None))
            .child(option(Status::Running, counts.running, None))
            .child(option(Status::Stopped, counts.stopped, None))
            .child(option(Status::Issues, counts.issues, Some(Role::Error)));
        let button = |label: &str, id: Id, pressed: bool| {
            let glow = self.glow(id);
            let mut button = div()
                .row()
                .items_center()
                .h(28.0)
                .px(9.0)
                .rounded(6.0)
                .no_shrink()
                .color(if pressed || glow > 0.0 {
                    Role::Foreground
                } else {
                    Role::Dim
                });
            if pressed {
                button = button
                    .bg(Role::Accent.alpha(0.13))
                    .border(1.0, Role::Accent.alpha(0.4));
            } else if glow > 0.0 {
                button = button.bg(Role::Foreground.alpha(0.07 * glow));
            }
            // A button that does nothing does not answer the pointer either.
            button = if id == SORT_BUTTON {
                button.id(id)
            } else {
                button.press(id)
            };
            button.child(text(label).text_size(11.5))
        };
        div()
            .row()
            .wrap()
            .items_center()
            .gap(6.0)
            .px(14.0)
            .pb(8.0)
            .child(search)
            .child(segmented)
            .child(button("Group", GROUP_BUTTON, self.grouped))
            .child(button("Name \u{2193}", SORT_BUTTON, false))
    }

    // --- The list ----------------------------------------------------------------------------

    fn list(&self, width: Width, view: f32) -> Element {
        let head = |label: &str, active: bool| {
            text(if active {
                format!("{} \u{25be}", label.to_uppercase())
            } else {
                label.to_uppercase()
            })
            .truncate()
        };
        let mut heads = vec![div().w(0.0)];
        heads.push(head("Name", true));
        if width.image() {
            heads.push(head("Image", false));
        }
        heads.push(head("CPU", false));
        if width.memory() {
            heads.push(head("Memory", false));
        }
        if width.ports() {
            heads.push(head("Ports", false));
        }
        heads.push(head("Status", false).align(Align::End));
        let head = div()
            .grid(width.tracks())
            .gap_x(10.0)
            .items_center()
            .h(28.0)
            .px(16.0)
            .no_shrink()
            .text_size(10.5)
            .color(Role::Quiet)
            .children(heads);

        // Only the rows in view are made; the rest of the list's length is two spacers.
        let rows = self.rows();
        let places = Self::places(&rows);
        let offset = self.ui.scroll_offset(LIST);
        let window = places.window(offset, view, 2.0 * ROW);
        let mut body = div()
            .col()
            .id(LIST)
            .focusable()
            .grow(1.0)
            .min_h(0.0)
            .px(8.0)
            .scroll_y(offset)
            .child(div().h(places.top(window.start)).no_shrink());
        if rows.is_empty() {
            body = body.child(self.nothing());
        }
        for row in &rows[window.clone()] {
            body = body.child(self.row(row, width));
        }
        body = body.child(div().h(places.total() - places.top(window.end)).no_shrink());

        div()
            .col()
            .grow(1.0)
            .min_h(0.0)
            .clip()
            .child(head)
            .child(div().h(1.0).no_shrink().bg(Role::Border.alpha(0.6)))
            .child(body)
    }

    /// Where the rows stand in the list: their heights, and the room around them.
    fn places(rows: &[Row]) -> Rows {
        Rows::new(rows.iter().map(Row::height)).padded(LIST_LEAD, LIST_TRAIL)
    }

    /// What the list says where no container is shown.
    fn nothing(&self) -> Element {
        let reason = if self.query.is_empty() {
            format!("No {} containers.", self.status.label().to_lowercase())
        } else {
            format!("No container matches \u{201c}{}\u{201d}.", self.query)
        };
        div()
            .col()
            .items_center()
            .gap(4.0)
            .pt(48.0)
            .no_shrink()
            .child(text(reason).color(Role::Dim))
            .child(
                text("Change the filter, or press Esc in the field to leave it.")
                    .text_size(11.0)
                    .color(Role::Quiet),
            )
    }

    /// The element of one row of the list.
    fn row(&self, row: &Row, width: Width) -> Element {
        let id = row.key.id();
        let chosen = self.ui.fades.level(id.role(CHOSEN));
        let hovered = self.ui.fades.level(id.role(HOVERED));
        let look = Look { chosen, hovered };
        match row.what {
            What::Group {
                project,
                ref members,
                folded,
            } => {
                let members: Vec<&Container> =
                    members.iter().map(|at| &self.containers[*at]).collect();
                group_row(id, project, &members, folded, look)
            }
            What::Container(at) => {
                let c = &self.containers[at];
                container_row(id, c, width, look, self.marked.contains(&c.name))
            }
        }
    }
}

/// How lit a row is: the choice of it (the cursor's, or the pointer's click) and the pointer's
/// being over it, each between 0 and 1 as the fades have it.
#[derive(Clone, Copy)]
struct Look {
    chosen: f32,
    hovered: f32,
}

impl Look {
    /// The row dressed: a chosen one on the accent with its border, one the pointer is over
    /// on a faint light.
    fn dress(self, row: Element) -> Element {
        if self.chosen > 0.0 {
            row.bg(Role::Accent.alpha(0.13 * self.chosen))
                .border(1.0, Role::Accent.alpha(0.55 * self.chosen))
        } else if self.hovered > 0.0 {
            row.bg(Role::Foreground.alpha(0.055 * self.hovered))
        } else {
            row
        }
    }
}

fn group_row(
    id: Id,
    project: Option<&str>,
    members: &[&Container],
    folded: bool,
    look: Look,
) -> Element {
    let up = members.iter().filter(|m| m.is_up()).count();
    let issues = members.iter().filter(|m| m.has_issue()).count();
    let stack = members
        .iter()
        .take(12)
        .fold(div().row().gap(2.0), |row, m| {
            let color = if m.has_issue() {
                Role::Error.solid()
            } else if m.is_up() {
                Role::Success.solid()
            } else {
                Role::Quiet.alpha(0.4)
            };
            row.child(div().size(5.0, 10.0).rounded(2.0).bg(color))
        });
    let summary = if issues > 0 {
        format!("{up}/{} up \u{b7} {issues} with issues", members.len())
    } else {
        format!("{up}/{} up", members.len())
    };
    look.dress(
        div()
            .row()
            .items_center()
            .gap(8.0)
            .h(ROW)
            .mt(GROUP_GAP)
            .px(8.0)
            .rounded(6.0)
            .no_shrink()
            .press(id)
            .color(Role::Dim)
            .child(
                div()
                    .row()
                    .justify_center()
                    .w(18.0)
                    .child(text(if folded { "\u{25b8}" } else { "\u{25be}" }).text_size(11.0)),
            )
            .child(
                text(project.unwrap_or("Standalone"))
                    .weight(650)
                    .color(Role::Foreground)
                    .no_shrink(),
            )
            .child(stack)
            .child(text(summary).text_size(11.5).color(Role::Quiet).truncate()),
    )
}

/// A container's row, under its project's: the name is set in from the project's.
fn container_row(id: Id, c: &Container, width: Width, look: Look, marked: bool) -> Element {
    let up = c.is_up();
    let tone = match c.state {
        State::Running => Role::Success,
        State::Starting | State::Paused => Role::Warning,
        State::Unhealthy => Role::Error,
        State::Exited(code) if code != 0 => Role::Error,
        State::Exited(_) => Role::Quiet,
    };
    let state_tag = match c.state {
        State::Unhealthy => Some(("unhealthy".to_owned(), Role::Error)),
        State::Starting => Some(("starting".to_owned(), Role::Warning)),
        State::Paused => Some(("paused".to_owned(), Role::Warning)),
        State::Exited(code) if code != 0 => Some((format!("exited {code}"), Role::Error)),
        _ => None,
    };
    let mut name = div()
        .row()
        .items_center()
        .gap(8.0)
        .min_w(0.0)
        .pl(14.0)
        .child(if up {
            text(c.name).weight(600).truncate()
        } else {
            text(c.name).weight(500).color(Role::Dim).truncate()
        });
    // Where the name has little room, the state is the dot's colour alone.
    if width != Width::Tiny && width != Width::Narrow {
        if let Some((label, tone)) = state_tag {
            name = name.child(tag(&label, tone));
        }
    }

    let mut cells = vec![
        div()
            .row()
            .items_center()
            .justify_center()
            .child(state_dot(c.state, tone)),
        name,
    ];
    if width.image() {
        cells.push(text(c.image).color(Role::Dim).truncate());
    }
    cells.push(if up {
        div()
            .row()
            .items_center()
            .justify_between()
            .gap(6.0)
            .child(spark(&c.history, Role::Accent).size(44.0, 14.0).no_shrink())
            .child(value(format!("{:.1}%", c.cpu)))
    } else {
        dash()
    });
    if width.memory() {
        cells.push(if up {
            let share = c.memory.1.clamp(0.0, 1.0);
            let color = if share > 0.85 {
                Role::Error
            } else if share > 0.65 {
                Role::Warning
            } else {
                Role::Quiet
            };
            div()
                .row()
                .items_center()
                .gap(6.0)
                .child(
                    div()
                        .h(4.0)
                        .grow(1.0)
                        .min_w(0.0)
                        .rounded(2.0)
                        .bg(Role::Raised(2))
                        .clip()
                        .child(
                            div()
                                .h_full()
                                .w_pct((share * 100.0).max(2.0))
                                .rounded(2.0)
                                .bg(color),
                        ),
                )
                .child(value(c.memory.0.clone()))
        } else {
            dash()
        });
    }
    if width.ports() {
        let mut ports = div().row().items_center().gap(4.0).min_w(0.0).clip();
        for port in c.ports.iter().take(2) {
            ports = ports.child(port_chip(*port));
        }
        if c.ports.len() > 2 {
            ports = ports.child(
                text(format!("+{}", c.ports.len() - 2))
                    .text_size(11.0)
                    .color(Role::Quiet),
            );
        }
        cells.push(ports);
    }
    cells.push(
        text(c.age)
            .text_size(11.5)
            .color(Role::Dim)
            .align(Align::End)
            .truncate(),
    );

    let mut row = div()
        .grid(width.tracks())
        .gap_x(10.0)
        .items_center()
        .h(ROW)
        .px(8.0)
        .rounded(6.0)
        .no_shrink()
        .press(id)
        .children(cells);
    if marked {
        // A bar at the row's edge: the container is marked, to be acted on with the others.
        row = row.child(
            div()
                .absolute()
                .inset(Some(8.0), None, Some(8.0), Some(1.0))
                .w(3.0)
                .rounded(1.5)
                .bg(Role::Accent),
        );
    }
    look.dress(row)
}

/// A value at the right of a meter: the number, in a box of its own width.
fn value(content: String) -> Element {
    div().min_w(42.0).no_shrink().child(
        text(content)
            .text_size(11.5)
            .color(Role::Dim)
            .align(Align::End),
    )
}

/// A cell with nothing to say: a quiet dash at its end.
fn dash() -> Element {
    text("\u{2013}").color(Role::Quiet).align(Align::End)
}

/// A small label in a tone, on the tone's own faint ground.
fn tag(label: &str, tone: Role) -> Element {
    let (ink, ground) = (tone.solid(), tone.alpha(0.13));
    div()
        .row()
        .items_center()
        .h(18.0)
        .px(6.0)
        .rounded(4.0)
        .bg(ground)
        .no_shrink()
        .child(text(label).text_size(10.5).color(ink))
}

/// A published port: the tool's link colour on its faint ground.
fn port_chip(port: u16) -> Element {
    div()
        .row()
        .items_center()
        .h(20.0)
        .px(6.0)
        .rounded(4.0)
        .bg(Role::Info.alpha(0.11))
        .no_shrink()
        .child(text(format!(":{port}")).text_size(11.0).color(Role::Info))
}

/// A key cap.
fn kbd(label: &str, size: f32) -> Element {
    div()
        .row()
        .items_center()
        .justify_center()
        .min_w(18.0)
        .h(18.0)
        .px(4.0)
        .rounded(4.0)
        .bg(Role::Raised(2))
        .border(1.0, Role::Border)
        .no_shrink()
        .child(text(label).text_size(size).color(Role::Dim))
}

/// A round dot of `size` points in `tone`; `ring` adds the soft halo of a live one.
fn dot(size: f32, tone: Role, ring: bool) -> Element {
    let core = div().size(size, size).rounded(size / 2.0).bg(tone);
    if ring {
        let halo = size + 6.0;
        div()
            .row()
            .items_center()
            .justify_center()
            .size(halo, halo)
            .rounded(halo / 2.0)
            .bg(tone.alpha(0.18))
            .child(core)
    } else {
        core
    }
}

/// The state's dot: solid with a halo for a container that is up, a hollow ring for one that
/// has ended (in the error colour when it ended badly).
fn state_dot(state: State, tone: Role) -> Element {
    match state {
        State::Exited(_) => div()
            .size(10.0, 10.0)
            .rounded(5.0)
            .border(1.5, tone.solid()),
        _ => dot(10.0, tone, true),
    }
}

/// The line of keys at the bottom: as many of them as the page's width has room for.
fn status_line(width: f32) -> Element {
    let hint = |key: &str, label: &str| {
        div()
            .row()
            .items_center()
            .gap(5.0)
            .h(20.0)
            .px(4.0)
            .no_shrink()
            .child(kbd(key, 10.0).min_w(16.0).h(16.0))
            .child(text(label).text_size(11.0).color(Role::Quiet))
    };
    let keys = if width >= 760.0 {
        6
    } else if width >= 560.0 {
        4
    } else {
        2
    };
    div()
        .col()
        .no_shrink()
        .bg(Role::Raised(1))
        .child(div().h(1.0).bg(Role::Border))
        .child(
            div()
                .row()
                .items_center()
                .gap(10.0)
                .h(25.0)
                .px(8.0)
                .clip()
                .child(
                    div()
                        .row()
                        .items_center()
                        .gap(4.0)
                        .min_w(0.0)
                        .clip()
                        .children(
                            [
                                ("s", "stop"),
                                ("r", "restart"),
                                ("e", "open a shell"),
                                ("l", "logs"),
                                (".", "more"),
                                ("space", "mark"),
                            ]
                            .into_iter()
                            .take(keys)
                            .map(|(key, label)| hint(key, label)),
                        ),
                )
                .child(spacer())
                .child(hint(":", "commands"))
                .child(hint("?", ""))
                .child(dot(8.0, Role::Success, false)),
        )
}

/// Thirty samples of a made-up CPU line: around `base`, with a spike of up to `spike` now and
/// then. A fixed generator: the page looks the same every time it is opened.
fn wave(seed: u32, base: f32, spike: f32) -> Vec<f32> {
    let mut state = seed.wrapping_mul(2_654_435_761).wrapping_add(1);
    (0..30)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let r = (state >> 8) as f32 / (1_u32 << 24) as f32;
            let s = (state.rotate_left(13) >> 8) as f32 / (1_u32 << 24) as f32;
            let bump = if s > 0.82 { spike * r } else { base * 0.35 * r };
            base * (0.8 + 0.2 * r) + bump
        })
        .collect()
}

#[cfg(test)]
mod interact_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use pencere_view::{Family, Font, Measure, Metrics, Paint, layout};

    /// A stand-in for the host's text: every character `0.55 × size` wide.
    struct Mono;

    impl Measure for Mono {
        fn measure(&mut self, font: &Font, text: &str) -> Metrics {
            Metrics {
                width: text.chars().count() as f32 * font.size * 0.55,
                height: (font.size * 1.3).round(),
            }
        }
    }

    fn page(width: f32, height: f32, scale: f32) -> (pencere_view::Laid, pencere_view::DrawList) {
        let view = Containers::fixture().view(Size::new(width, height));
        let laid = layout(
            view,
            Size::new(width * scale, height * scale),
            scale,
            &mut Mono,
        );
        let list = laid.paint(&mut Mono);
        (laid, list)
    }

    fn words(list: &pencere_view::DrawList) -> Vec<String> {
        list.items
            .iter()
            .filter_map(|item| match item {
                Paint::Text(run) => Some(run.text.clone()),
                Paint::Quad(_) | Paint::Image(_) => None,
            })
            .collect()
    }

    #[test]
    fn the_fixture_has_a_container_in_every_state() {
        let page = Containers::fixture();
        let states: Vec<State> = page.containers.iter().map(|c| c.state).collect();
        assert!(states.contains(&State::Running));
        assert!(states.contains(&State::Starting));
        assert!(states.contains(&State::Unhealthy));
        assert!(states.iter().any(|s| matches!(s, State::Exited(1..))));
        assert!(states.contains(&State::Exited(0)));
        assert_eq!(
            page.issues(),
            2,
            "the unhealthy one and the one that failed"
        );
    }

    #[test]
    fn the_history_is_the_same_every_time_and_thirty_long() {
        assert_eq!(wave(3, 0.8, 4.0), wave(3, 0.8, 4.0));
        assert_ne!(wave(3, 0.8, 4.0), wave(4, 0.8, 4.0));
        assert_eq!(wave(3, 0.8, 4.0).len(), 30);
        assert!(wave(3, 0.8, 4.0).iter().all(|v| *v >= 0.0));
    }

    #[test]
    fn the_list_says_each_project_its_containers_and_the_keys() {
        let (_, list) = page(1000.0, 600.0, 2.0);
        let words = words(&list);
        for expected in [
            "Containers",
            "shop",
            "blog",
            "stats",
            "Standalone",
            "api",
            "worker",
            "exited 1",
            "unhealthy",
            "starting",
            "6/7 up \u{b7} 2 with issues",
            "space",
            "open a shell",
        ] {
            assert!(
                words.iter().any(|w| w == expected),
                "{expected:?} is not on the page: {words:?}"
            );
        }
        // The project folded away shows its group and none of its containers.
        assert!(!words.iter().any(|w| w == "grafana"));
    }

    #[test]
    fn a_narrower_pane_drops_the_columns_that_matter_least() {
        let columns = |width: f32| {
            let (_, list) = page(width, 600.0, 1.0);
            let words = words(&list);
            (
                words.iter().any(|w| w == "IMAGE"),
                words.iter().any(|w| w == "PORTS"),
                words.iter().any(|w| w == "MEMORY"),
                words.iter().any(|w| w == "Images"),
            )
        };
        // The rail, the image and the ports all fit.
        assert_eq!(columns(1200.0), (true, true, true, true));
        // The list under 860 points: no ports; the rail goes under 780 points of page.
        assert_eq!(columns(1000.0), (true, false, true, true));
        assert_eq!(columns(760.0), (true, false, true, false));
        assert_eq!(columns(640.0), (false, false, true, false));
        assert_eq!(columns(440.0), (false, false, false, false));
    }

    #[test]
    fn everything_stays_inside_the_page_at_every_width() {
        for width in [1400.0, 1000.0, 820.0, 700.0, 560.0, 460.0, 360.0, 300.0] {
            for scale in [1.0, 2.0] {
                let (laid, list) = page(width, 600.0, scale);
                let edge = width * scale;
                for item in &list.items {
                    let (rect, clip) = match item {
                        Paint::Quad(q) => (q.rect, q.clip),
                        Paint::Image(p) => (p.rect, p.clip),
                        Paint::Text(r) => (
                            pencere_view::Rect::new(r.origin.x, r.origin.y, 1.0, 1.0),
                            r.clip,
                        ),
                    };
                    // What is drawn is what the clip lets through.
                    let seen = rect.intersect(clip);
                    if seen.w > 0.0 && seen.h > 0.0 {
                        assert!(
                            seen.right() <= edge + 1.0 && seen.bottom() <= 600.0 * scale + 1.0,
                            "{item:?} spills out of a {width} pt page at {scale}x"
                        );
                    }
                }
                assert_eq!(laid.size, Size::new(edge, 600.0 * scale));
            }
        }
    }

    #[test]
    fn the_chosen_row_is_told_apart_by_its_border_and_the_dots_by_their_tone() {
        let (_, list) = page(1000.0, 600.0, 2.0);
        let selected = list.items.iter().filter(|item| {
            matches!(item, Paint::Quad(q) if q.border.is_some_and(|b| b.color == Role::Accent.alpha(0.55))
                && q.fill == Some(Role::Accent.alpha(0.13)))
        });
        assert_eq!(selected.count(), 1);
        // A hollow ring for each container that ended: in the error colour when it ended badly.
        let ring = |tone: Role| {
            list.items
                .iter()
                .filter(|item| {
                    matches!(item, Paint::Quad(q) if q.fill.is_none()
                        && q.border.is_some_and(|b| b.color.role() == Some(tone) && b.width == 3.0))
                })
                .count()
        };
        assert_eq!(ring(Role::Error), 1, "the mailer ended badly");
        assert_eq!(ring(Role::Quiet), 2, "and the blog's two stopped quietly");
    }

    #[test]
    fn the_page_is_the_terminals_face_unless_a_box_says_otherwise() {
        let (_, list) = page(1000.0, 600.0, 2.0);
        assert!(list.items.iter().all(|item| match item {
            Paint::Text(run) => run.font.family == Family::Terminal,
            Paint::Quad(_) | Paint::Image(_) => true,
        }));
    }
}
