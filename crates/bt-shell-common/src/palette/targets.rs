//! Where the palette can take the user: folders, servers and ports (⌘O and the root).
//!
//! The shell reads what each pane says of itself — the directory its shell reported, the host it
//! is connected to, the ports it listens on — and what `~/.ssh/config` names ([`Targets`]); the
//! rules here turn that into rows, order them, and say what choosing one comes to ([`Go`]).
//!
//! **Nothing here sends a byte to a shell that runs.** A folder opens a new tab *in* it (the shell
//! is born there; nothing is typed); a server opens a new local tab with `ssh host` left on the
//! line ([`ssh_line`]); ⌥⏎ on a folder writes `cd '…'` to the line of the pane on screen
//! ([`cd_line`]). Neither line has a newline in it, and the user's ⏎ is what runs it.
//!
//! **Folders** are the directories panes reported (newest first, [`Seen`]) and, when the field
//! holds a path (`~/Pro`, `/usr/lo` — [`PathQuery`]), the entries of that directory. Listing a
//! directory is blocking I/O ([`list_dirs`]); the shell does it off the main thread and hands the
//! names to [`path_list`].
//!
//! **Identity.** A folder is `dir.<absolute path>`, a server `host.<name as typed after ssh>`, a
//! port `port.<key>` (`3000` for a port on this Mac, `db1:8080` for a server's).

use std::path::Path;

use bt_core::HostMark;

use crate::{quote, ssh_hosts};

use super::list::List;
use super::path::PathQuery;
use super::rank::Hit;
use super::row::{Kind, Row, Scope, Status, Tone};
use super::server::{self, ServerFacts};
use super::state::Modifier;

/// How many folders the session remembers.
pub const FOLDER_LIMIT: usize = 40;
/// How many servers the session remembers.
pub const HOST_LIMIT: usize = 40;
/// The most entries a typed path lists.
pub const ENTRY_LIMIT: usize = 200;
/// The most directory entries one listing reads before it stops (a directory of a million files
/// must not stall the palette).
pub const READ_LIMIT: usize = 20_000;

// ─── What was seen ────────────────────────────────────────────────────────

/// Names in the order they were last seen, newest first, bounded: the folders panes reported and
/// the servers they were connected to. In memory only — the palette keeps no history on disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seen {
    limit: usize,
    newest_first: Vec<String>,
}

impl Seen {
    pub fn new(limit: usize) -> Seen {
        Seen {
            limit,
            newest_first: Vec::new(),
        }
    }

    /// `name` was seen now: it becomes the newest.
    pub fn note(&mut self, name: &str) {
        self.newest_first.retain(|seen| seen != name);
        self.newest_first.insert(0, name.to_owned());
        self.newest_first.truncate(self.limit);
    }

    /// `name` is there but when it came is not known (found open as the palette opens): it is
    /// added at the back, and a name already seen keeps its place.
    pub fn note_unseen(&mut self, name: &str) {
        if !self.newest_first.iter().any(|seen| seen == name)
            && self.newest_first.len() < self.limit
        {
            self.newest_first.push(name.to_owned());
        }
    }

    pub fn names(&self) -> &[String] {
        &self.newest_first
    }
}

// ─── Home and paths ───────────────────────────────────────────────────────

/// `path` with the home directory written `~`: `/Users/a/Projects` → `~/Projects`. The tab
/// summary card's rule ([`crate::program::tilde`]), so the two say a directory the same way.
pub fn tilde(path: &str, home: &str) -> String {
    crate::program::tilde(Path::new(path), Some(Path::new(home)))
}

/// `path` with a leading `~` made the home directory: `~/Projects` → `/Users/a/Projects`. A path
/// that does not begin with `~` or `~/` is returned as it is (`~user` is not expanded). The
/// settings' rule for a folder key ([`bt_core::expand_home`]).
pub fn expand(path: &str, home: &str) -> String {
    bt_core::expand_home(path, Some(Path::new(home))).map_or_else(
        || path.to_owned(),
        |full| full.to_string_lossy().into_owned(),
    )
}

/// The last component of `path` for a row's title: `~` for the home directory, `/` for the root.
fn folder_name(path: &str, home: &str) -> String {
    let shown = tilde(path, home);
    if shown == "~" || shown == "/" {
        return shown;
    }
    shown
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(&shown)
        .to_owned()
}

fn joined(parts: impl IntoIterator<Item = Option<String>>) -> String {
    parts
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

fn count_note(count: usize, what: &str) -> Option<String> {
    (count > 0).then(|| format!("{count} {what}"))
}

// ─── Facts the shell reads ────────────────────────────────────────────────

/// A folder a pane stands in now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenFolder {
    /// Absolute.
    pub path: String,
    pub branch: Option<String>,
}

/// What the palette is told of one folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderFacts {
    /// Absolute.
    pub path: String,
    /// How many tabs stand in it now.
    pub tabs: usize,
    pub branch: Option<String>,
}

/// The folders to list: those seen, newest first, each with the tabs standing in it and the git
/// branch there; a folder open now that was never seen follows.
pub fn folder_facts(seen: &Seen, open: &[OpenFolder]) -> Vec<FolderFacts> {
    let mut paths: Vec<&str> = seen.names().iter().map(String::as_str).collect();
    for folder in open {
        if !paths.contains(&folder.path.as_str()) {
            paths.push(&folder.path);
        }
    }
    paths
        .into_iter()
        .map(|path| {
            let here: Vec<&OpenFolder> = open.iter().filter(|f| f.path == path).collect();
            FolderFacts {
                path: path.to_owned(),
                tabs: here.len(),
                branch: here.iter().find_map(|f| f.branch.clone()),
            }
        })
        .collect()
}

/// What the palette is told of one server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostFacts {
    /// What goes after `ssh`.
    pub name: String,
    /// The `[remote] hosts` mark.
    pub mark: HostMark,
    /// It is in `~/.ssh/config`.
    pub configured: bool,
    /// How many tabs are connected to it now.
    pub tabs: usize,
}

/// The servers to list: those a pane was connected to (newest first), then those `~/.ssh/config`
/// names that were not. A name ssh would read as an option, or that has a space, is dropped
/// ([`ssh_hosts::is_typeable`]); `deploy@db-1` and `db-1` are one server, listed as it was seen.
/// `open` is the host of each tab connected now, as the tab shows it.
pub fn host_facts(
    seen: &Seen,
    open: &[String],
    configured: &[String],
    mark: impl Fn(&str) -> HostMark,
) -> Vec<HostFacts> {
    let bare = |name: &str| bt_core::bare_host(name).to_owned();
    let mut names: Vec<String> = Vec::new();
    let mut push = |name: &str| {
        if ssh_hosts::is_typeable(name) && !names.iter().any(|known| bare(known) == bare(name)) {
            names.push(name.to_owned());
        }
    };
    for name in seen.names() {
        push(name);
    }
    for name in open {
        push(name);
    }
    for name in configured {
        push(name);
    }
    names
        .into_iter()
        .map(|name| HostFacts {
            mark: mark(&name),
            configured: configured.iter().any(|c| bare(c) == bare(&name)),
            tabs: open.iter().filter(|o| bare(o) == bare(&name)).count(),
            name,
        })
        .collect()
}

/// What the palette is told of one listening port.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortFacts {
    /// The identity's key: the port (`3000`), or the server and the port (`db1:8080`).
    pub key: String,
    pub title: String,
    /// The process and the tab it belongs to.
    pub subtitle: String,
    /// What opens it now, and what ⌘⏎ copies.
    pub url: String,
}

/// Everything the folders, servers and ports rows are made from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Targets {
    pub home: String,
    pub folders: Vec<FolderFacts>,
    pub hosts: Vec<HostFacts>,
    pub ports: Vec<PortFacts>,
    /// What the servers kablo keeps terminals on keep and show ([`ServerFacts`]); a server that
    /// is not among them is a plain `ssh` and has neither a count nor a page.
    pub servers: Vec<ServerFacts>,
}

// ─── Rows ─────────────────────────────────────────────────────────────────

/// The row of a folder: its name, then where it is (`~/…`), how many tabs are in it and the
/// branch. ⇥ puts its path in the field to look inside.
pub fn folder_row(facts: &FolderFacts, home: &str) -> Row {
    let name = folder_name(&facts.path, home);
    let place = tilde(&facts.path, home);
    Row::new(Kind::Folder, &facts.path, name.clone())
        .subtitle(joined([
            (place != name).then_some(place.clone()),
            count_note(facts.tabs, if facts.tabs == 1 { "tab" } else { "tabs" }),
            facts.branch.clone(),
        ]))
        .completion(PathQuery::complete(&place))
}

/// The status a mark gives a server: the theme's own roles, `Production` the error one.
pub fn mark_status(mark: HostMark) -> Option<Status> {
    let (text, tone) = match mark {
        HostMark::Production => ("Production", Tone::Error),
        HostMark::Staging => ("Staging", Tone::Warning),
        HostMark::Development => ("Development", Tone::Success),
        HostMark::Rgb(_) => ("Marked", Tone::Accent),
        HostMark::None => return None,
    };
    Some(Status {
        text: text.to_owned(),
        tone,
    })
}

/// The row of a server: its name, where it comes from, how many tabs are on it, how many terminals
/// it keeps in the background and its mark. A server kablo keeps terminals on (`server`) opens its
/// page on →; ⏎ is still the new tab.
pub fn host_row(facts: &HostFacts, server: Option<&ServerFacts>) -> Row {
    let mut row = Row::new(Kind::Host, &facts.name, facts.name.clone())
        .keywords("ssh server remote connect machine")
        .subtitle(joined([
            Some(
                if facts.configured {
                    "ssh config"
                } else {
                    "seen this session"
                }
                .to_owned(),
            ),
            count_note(facts.tabs, if facts.tabs == 1 { "tab" } else { "tabs" }),
            server.and_then(|server| count_note(server.background.len(), "in the background")),
        ]));
    row.status = mark_status(facts.mark);
    if server.is_some() {
        row = row.opens(Scope::Server(facts.name.clone()), false);
    }
    row
}

/// The row of a listening port.
pub fn port_row(facts: &PortFacts) -> Row {
    Row::new(Kind::Port, &facts.key, facts.title.clone())
        .keywords("port localhost server listen http browser")
        .subtitle(facts.subtitle.clone())
}

pub fn folder_rows(targets: &Targets) -> Vec<Row> {
    targets
        .folders
        .iter()
        .map(|facts| folder_row(facts, &targets.home))
        .collect()
}

pub fn host_rows(targets: &Targets) -> Vec<Row> {
    targets
        .hosts
        .iter()
        .map(|facts| host_row(facts, server::server_of(targets, &facts.name)))
        .collect()
}

pub fn port_rows(targets: &Targets) -> Vec<Row> {
    targets.ports.iter().map(port_row).collect()
}

// ─── A typed path ─────────────────────────────────────────────────────────

/// The directories inside `dir`, by name; blocking, so off the main thread. A directory that
/// cannot be read is `None`, unlike an empty one: only a folder that is there is offered as a
/// place to go. A symbolic link counts when it points at a directory. At most [`READ_LIMIT`]
/// entries are looked at.
pub fn list_dirs(dir: &Path) -> Option<Vec<String>> {
    let listing = std::fs::read_dir(dir).ok()?;
    Some(
        listing
            .filter_map(Result::ok)
            .take(READ_LIMIT)
            .filter(|entry| std::fs::metadata(entry.path()).is_ok_and(|meta| meta.is_dir()))
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect(),
    )
}

/// The list a typed path leaves: the directory's entries that begin with the stem (case and
/// accents aside), sorted by name, hidden ones only when the stem itself begins with a dot, at
/// most [`ENTRY_LIMIT`]. A row's title is the path as typed so far completed (`~/Projects`), and
/// the typed part is what is marked; ⇥ on a row puts its path and a slash in the field.
///
/// A path that ends in a slash (`~/Desktop/`, what ⇥ leaves) lists the folder itself first, so
/// ⏎ goes where the field points rather than into its first entry; `names` is `None` when that
/// folder cannot be read, and then nothing is listed.
pub fn path_list(query: &PathQuery, names: Option<&[String]>, home: &str) -> List {
    let Some(names) = names else {
        return List::default();
    };
    let here = query.stem.is_empty().then(|| Hit {
        // No completion: the field is already inside it, so ⇥ has nowhere to step.
        row: Row::new(Kind::Folder, &expand(&query.base, home), query.base.clone())
            .subtitle("This folder"),
        score: 0.0,
        marks: (0..query.base.chars().count()).collect(),
    });
    let hidden = query.stem.starts_with('.');
    let mut kept: Vec<&String> = names
        .iter()
        .filter(|name| (hidden || !name.starts_with('.')) && query.accepts(name))
        .collect();
    kept.sort_by_key(|name| name.to_lowercase());
    kept.truncate(ENTRY_LIMIT);
    let marks = query.marks();
    List {
        hits: here
            .into_iter()
            .chain(kept.into_iter().map(|name| {
                let shown = query.path_of(name);
                let path = expand(&shown, home);
                Hit {
                    row: Row::new(Kind::Folder, &path, shown.clone())
                        .completion(PathQuery::complete(&shown)),
                    score: 0.0,
                    marks: marks.clone(),
                }
            }))
            .collect(),
        headings: Vec::new(),
    }
}

// ─── What choosing does ───────────────────────────────────────────────────

/// What choosing a folder, server or port row comes to; the shell does it once the palette is
/// down.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Go {
    /// ⏎ on a folder: a new tab, its shell born there.
    OpenFolder(String),
    /// ⌥⏎ on a folder: `cd '…'` left on the line of the pane on screen.
    TypeCd(String),
    /// ⌘⏎ on a folder: shown in Finder.
    ShowFolder(String),
    /// ⏎ on a server: a new local tab that connects — `ssh name` runs as its shell's first
    /// command. Unlike a command from the history it is run, not left waiting: the tab is a fresh
    /// one the choice itself opens, so there is no other line it could land on.
    OpenHost(String),
    /// ⌥⏎ on a server: the same beside the pane on screen — a split to its right, below it when
    /// that does not fit, a new tab when neither does ([`beside`]).
    SplitHost(String),
    /// ⏎ on a port: its address in the browser (the port's key).
    OpenPort(String),
    /// ⌘⏎ on a port: its address to the pasteboard (the port's key).
    CopyPort(String),
    /// ⏎ on a terminal a server keeps in the background: a new tab that is that terminal, its
    /// only pane. The account's key and the terminal's name.
    OpenBackground { account: String, terminal: String },
    /// ⌥⏎ on a terminal in the background: the same beside the pane on screen ([`beside`]).
    SplitBackground { account: String, terminal: String },
    /// ⏎ on Open All: every terminal the server (the name after `ssh`) keeps in the background,
    /// together in new tabs ([`crate::split::arrange`]). What is opened is what the list holds
    /// when this is done, not when the row was listed.
    OpenAllBackground(String),
    /// ⏎ on a terminal another Mac has open: a new tab that is that terminal, taken from the other
    /// Mac, whose pane goes dim. The account's key and the terminal's name; what is taken is the
    /// terminal as it stands when this is done.
    MoveHere { account: String, terminal: String },
    /// ⏎ on a pane of the server's page: its tab comes forward and the pane has the keyboard.
    FocusPane { tab: u64, pane: u64 },
}

/// What the row `id` chosen with `modifier` comes to; `None` for a row that is not one of these
/// or a use the row does not have.
pub fn go(id: &str, modifier: Modifier) -> Option<Go> {
    if let Some(go) = server::go(id, modifier) {
        return Some(go);
    }
    if let Some(path) = id.strip_prefix(Kind::Folder.prefix()) {
        let path = path.to_owned();
        return Some(match modifier {
            Modifier::None => Go::OpenFolder(path),
            Modifier::Option => Go::TypeCd(path),
            Modifier::Command => Go::ShowFolder(path),
        });
    }
    if let Some(name) = id.strip_prefix(Kind::Host.prefix()) {
        let name = name.to_owned();
        return match modifier {
            Modifier::None => Some(Go::OpenHost(name)),
            Modifier::Option => Some(Go::SplitHost(name)),
            Modifier::Command => None,
        };
    }
    if let Some(key) = id.strip_prefix(Kind::Port.prefix()) {
        let key = key.to_owned();
        return match modifier {
            Modifier::None => Some(Go::OpenPort(key)),
            Modifier::Command => Some(Go::CopyPort(key)),
            Modifier::Option => None,
        };
    }
    None
}

/// `cd '…'` for a folder, for the line to wait with; `None` for a path that is not absolute or
/// has a control character in it (a newline in the line would run it).
pub fn cd_line(path: &str) -> Option<String> {
    if !path.starts_with('/') || path.chars().any(char::is_control) {
        return None;
    }
    Some(format!("cd {}", quote::shell_quote(&[path.to_owned()])))
}

/// `ssh name` for a server; `None` for a name that cannot be typed after `ssh`
/// ([`ssh_hosts::is_typeable`]). A name made of the letters ssh host names are made of is written
/// as it is (`deploy@db-1`); any other is escaped.
pub fn ssh_line(name: &str) -> Option<String> {
    if !ssh_hosts::is_typeable(name) {
        return None;
    }
    let plain = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '@' | ':' | '%' | '+'));
    let arg = if plain {
        name.to_owned()
    } else {
        quote::shell_quote(&[name.to_owned()])
    };
    Some(format!("ssh {arg}"))
}

/// Where a Split Right that wants a place ends up: beside, below when beside does not fit, a new
/// tab when neither does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Beside {
    Right,
    Down,
    NewTab,
}

/// The place a Split Right takes, given whether each split fits (the smallest pane limit).
pub fn beside(fits_right: bool, fits_down: bool) -> Beside {
    if fits_right {
        Beside::Right
    } else if fits_down {
        Beside::Down
    } else {
        Beside::NewTab
    }
}

// ─── Pages ────────────────────────────────────────────────────────────────

/// The pages the root offers for these rows.
pub fn go_to(targets: &Targets) -> Vec<Row> {
    let mut pages = vec![
        Row::new(Kind::Scope, "hosts", "Hosts…")
            .keywords("ssh servers remote machines connect")
            .opens(Scope::Hosts, true),
    ];
    if !targets.ports.is_empty() {
        pages.push(
            Row::new(Kind::Scope, "ports", "Ports…")
                .keywords("listening servers localhost http open browser")
                .opens(Scope::Ports, true),
        );
    }
    pages
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "/Users/a";

    #[test]
    fn the_home_directory_is_written_with_a_tilde_and_read_back() {
        assert_eq!(tilde("/Users/a", HOME), "~");
        assert_eq!(tilde("/Users/a/Projects/x", HOME), "~/Projects/x");
        assert_eq!(tilde("/Users/ab/x", HOME), "/Users/ab/x");
        assert_eq!(tilde("/etc", HOME), "/etc");
        assert_eq!(expand("~", HOME), "/Users/a");
        assert_eq!(expand("~/Projects", HOME), "/Users/a/Projects");
        assert_eq!(expand("/etc", HOME), "/etc");
        assert_eq!(expand("~root/x", HOME), "~root/x");
        assert_eq!(expand("~/x", "/Users/a/"), "/Users/a/x");
    }

    #[test]
    fn seen_names_come_newest_first_and_are_bounded() {
        let mut seen = Seen::new(3);
        for name in ["a", "b", "c", "a"] {
            seen.note(name);
        }
        assert_eq!(seen.names(), ["a", "c", "b"]);
        seen.note("d");
        assert_eq!(seen.names(), ["d", "a", "c"]);
        // Found open, never seen: at the back, and a seen name keeps its place.
        seen.note_unseen("a");
        seen.note_unseen("zz");
        assert_eq!(seen.names(), ["d", "a", "c"]);
    }

    #[test]
    fn the_folders_come_in_the_order_seen_with_the_tabs_standing_in_them() {
        let mut seen = Seen::new(10);
        seen.note("/Users/a/old");
        seen.note("/Users/a/api");
        let open = [
            OpenFolder {
                path: "/Users/a/api".into(),
                branch: Some("main".into()),
            },
            OpenFolder {
                path: "/Users/a/api".into(),
                branch: None,
            },
            OpenFolder {
                path: "/tmp/new".into(),
                branch: None,
            },
        ];
        let facts = folder_facts(&seen, &open);
        let listed: Vec<_> = facts.iter().map(|f| (f.path.as_str(), f.tabs)).collect();
        assert_eq!(
            listed,
            [("/Users/a/api", 2), ("/Users/a/old", 0), ("/tmp/new", 1)]
        );
        assert_eq!(facts[0].branch.as_deref(), Some("main"));
        let row = folder_row(&facts[0], HOME);
        assert_eq!(row.id, "dir./Users/a/api");
        assert_eq!(row.title, "api");
        assert_eq!(row.subtitle, "~/api · 2 tabs · main");
        assert_eq!(row.completion.as_deref(), Some("~/api/"));
        assert_eq!(folder_row(&facts[1], HOME).subtitle, "~/old");
        // The home directory and the root are named as themselves.
        let home = FolderFacts {
            path: HOME.into(),
            tabs: 0,
            branch: None,
        };
        assert_eq!(folder_row(&home, HOME).title, "~");
        assert_eq!(folder_row(&home, HOME).subtitle, "");
    }

    #[test]
    fn a_server_is_listed_once_whatever_the_way_it_is_known_by() {
        let mut seen = Seen::new(10);
        seen.note("db-1");
        seen.note("deploy@web");
        let open = ["deploy@web".to_owned()];
        let configured = [
            "web".to_owned(),
            "-oProxyCommand=x".to_owned(),
            "db-1".to_owned(),
            "cache".to_owned(),
        ];
        let hosts = host_facts(&seen, &open, &configured, |name| {
            if name == "cache" {
                HostMark::Staging
            } else {
                HostMark::None
            }
        });
        let listed: Vec<_> = hosts.iter().map(|h| h.name.as_str()).collect();
        // Seen first (newest first), then the config's; `web` is `deploy@web`.
        assert_eq!(listed, ["deploy@web", "db-1", "cache"]);
        assert!(hosts[0].configured && hosts[0].tabs == 1);
        assert!(hosts[2].tabs == 0 && hosts[2].configured);
        let row = host_row(&hosts[2], None);
        assert_eq!(row.id, "host.cache");
        assert_eq!(row.subtitle, "ssh config");
        assert_eq!(row.status.as_ref().map(|s| s.tone), Some(Tone::Warning));
        assert_eq!(host_row(&hosts[0], None).subtitle, "ssh config · 1 tab");
    }

    #[test]
    fn a_host_mark_wears_the_themes_own_roles() {
        let tone = |mark| mark_status(mark).map(|s| s.tone);
        assert_eq!(tone(HostMark::Production), Some(Tone::Error));
        assert_eq!(tone(HostMark::Staging), Some(Tone::Warning));
        assert_eq!(tone(HostMark::Development), Some(Tone::Success));
        assert_eq!(tone(HostMark::None), None);
    }

    fn names(list: &List) -> Vec<&str> {
        list.hits.iter().map(|h| h.row.title.as_str()).collect()
    }

    #[test]
    fn a_typed_path_lists_the_entries_that_begin_with_the_stem() {
        let entries: Vec<String> = [
            "Projects",
            "Documents",
            "projects-old",
            ".config",
            "Pictures",
        ]
        .map(String::from)
        .to_vec();
        let q = PathQuery::parse("~/pro").unwrap();
        let list = path_list(&q, Some(&entries), HOME);
        assert_eq!(names(&list), ["~/Projects", "~/projects-old"]);
        assert_eq!(list.hits[0].row.id, "dir./Users/a/Projects");
        assert_eq!(list.hits[0].row.completion.as_deref(), Some("~/Projects/"));
        assert_eq!(list.hits[0].marks, q.marks());
        // Everything, in name order, hidden ones left out, after the folder itself.
        let all = path_list(&PathQuery::parse("~/").unwrap(), Some(&entries), HOME);
        assert_eq!(
            names(&all),
            [
                "~",
                "~/Documents",
                "~/Pictures",
                "~/Projects",
                "~/projects-old"
            ]
        );
        // A stem that begins with a dot asks for them.
        let dot = path_list(&PathQuery::parse("~/.c").unwrap(), Some(&entries), HOME);
        assert_eq!(names(&dot), ["~/.config"]);
        // The root has no prefix to double.
        let root = path_list(
            &PathQuery::parse("/u").unwrap(),
            Some(&["usr".to_owned()]),
            HOME,
        );
        assert_eq!(names(&root), ["/usr"]);
        assert_eq!(root.hits[0].row.id, "dir./usr");
    }

    #[test]
    fn a_path_ending_in_a_slash_offers_the_folder_itself_first() {
        let entries: Vec<String> = ["Screenshots", "Notes"].map(String::from).to_vec();
        // What ⇥ leaves in the field: ⏎ goes to the folder, not into its first entry.
        let q = PathQuery::parse("~/Desktop/").unwrap();
        let list = path_list(&q, Some(&entries), HOME);
        assert_eq!(
            names(&list),
            ["~/Desktop", "~/Desktop/Notes", "~/Desktop/Screenshots"]
        );
        assert_eq!(list.hits[0].row.id, "dir./Users/a/Desktop");
        assert_eq!(list.hits[0].row.completion, None);
        assert_eq!(
            list.hits[0].marks,
            (0.."~/Desktop".len()).collect::<Vec<_>>()
        );
        // An empty folder is still a place to go; one that cannot be read is not.
        assert_eq!(names(&path_list(&q, Some(&[]), HOME)), ["~/Desktop"]);
        assert!(path_list(&q, None, HOME).hits.is_empty());
        // The root is its own row.
        let root = path_list(
            &PathQuery::parse("/").unwrap(),
            Some(&["usr".to_owned()]),
            HOME,
        );
        assert_eq!(names(&root), ["/", "/usr"]);
        // With letters after the slash only the entries are listed.
        let typed = path_list(
            &PathQuery::parse("~/Desktop/n").unwrap(),
            Some(&entries),
            HOME,
        );
        assert_eq!(names(&typed), ["~/Desktop/Notes"]);
    }

    #[test]
    fn a_long_directory_lists_a_bounded_number_of_entries() {
        let many: Vec<String> = (0..ENTRY_LIMIT + 50).map(|n| format!("d{n:04}")).collect();
        let list = path_list(&PathQuery::parse("~/d").unwrap(), Some(&many), HOME);
        assert_eq!(list.len(), ENTRY_LIMIT);
    }

    #[test]
    fn the_directory_listing_keeps_directories_and_what_points_at_one() {
        let dir = std::env::temp_dir().join(format!("bt-palette-dirs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("file.txt"), "x").unwrap();
        std::os::unix::fs::symlink(dir.join("sub"), dir.join("link")).unwrap();
        std::os::unix::fs::symlink(dir.join("file.txt"), dir.join("filelink")).unwrap();
        let mut found = list_dirs(&dir).unwrap();
        found.sort();
        assert_eq!(found, ["link", "sub"]);
        assert_eq!(list_dirs(&dir.join("sub")), Some(Vec::new()));
        assert_eq!(list_dirs(&dir.join("missing")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn choosing_means_what_each_row_has_for_each_key() {
        use Modifier::{Command, None as Plain, Option as Alt};
        assert_eq!(go("dir./a/b", Plain), Some(Go::OpenFolder("/a/b".into())));
        assert_eq!(go("dir./a/b", Alt), Some(Go::TypeCd("/a/b".into())));
        assert_eq!(go("dir./a/b", Command), Some(Go::ShowFolder("/a/b".into())));
        assert_eq!(go("host.db-1", Plain), Some(Go::OpenHost("db-1".into())));
        assert_eq!(go("host.db-1", Alt), Some(Go::SplitHost("db-1".into())));
        assert_eq!(go("host.db-1", Command), None);
        assert_eq!(go("port.3000", Plain), Some(Go::OpenPort("3000".into())));
        assert_eq!(
            go("port.db1:8080", Command),
            Some(Go::CopyPort("db1:8080".into()))
        );
        assert_eq!(go("port.3000", Alt), None);
        assert_eq!(go("cmd.splitRight", Plain), None);
        assert_eq!(go("tab.4", Plain), None);
    }

    #[test]
    fn the_lines_left_to_wait_are_one_line_and_say_only_what_they_are_for() {
        assert_eq!(
            cd_line("/Users/a/Projects").as_deref(),
            Some("cd /Users/a/Projects")
        );
        assert_eq!(
            cd_line("/Users/a/My Projects").as_deref(),
            Some("cd /Users/a/My\\ Projects")
        );
        // A folder is a path, and one with a newline would end the line.
        assert_eq!(cd_line("relative"), None);
        assert_eq!(cd_line("/a\nb"), None);
        assert_eq!(ssh_line("db-1").as_deref(), Some("ssh db-1"));
        assert_eq!(ssh_line("deploy@db-1").as_deref(), Some("ssh deploy@db-1"));
        assert_eq!(ssh_line("a;b").as_deref(), Some("ssh a\\;b"));
        assert_eq!(ssh_line("-oProxyCommand=x"), None);
        assert_eq!(ssh_line("a\nb"), None);
        assert_eq!(ssh_line(""), None);
    }

    #[test]
    fn a_split_that_does_not_fit_falls_to_the_next_place() {
        assert_eq!(beside(true, true), Beside::Right);
        assert_eq!(beside(true, false), Beside::Right);
        assert_eq!(beside(false, true), Beside::Down);
        assert_eq!(beside(false, false), Beside::NewTab);
    }

    #[test]
    fn the_ports_page_is_offered_only_with_ports_to_list() {
        let none = Targets::default();
        assert!(go_to(&none).iter().all(|row| row.id != "scope.ports"));
        let some = Targets {
            ports: vec![PortFacts {
                key: "3000".into(),
                title: "localhost:3000".into(),
                subtitle: "node · api".into(),
                url: "http://localhost:3000".into(),
            }],
            ..Targets::default()
        };
        assert!(go_to(&some).iter().any(|row| row.id == "scope.ports"));
        assert_eq!(port_rows(&some)[0].id, "port.3000");
    }
}
