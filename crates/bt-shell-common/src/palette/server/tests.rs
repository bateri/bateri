use bt_core::HostMark;

use super::super::list::{Fallbacks, root};
use super::super::row::Scope;
use super::super::targets::{HostFacts, host_row, host_rows};
use super::*;

const ACCOUNT: &str = "u@rasp:22";

fn waiting(terminal: &str, title: &str, running: Option<u64>) -> BackgroundFacts {
    BackgroundFacts {
        account: ACCOUNT.to_owned(),
        host: "rasp".to_owned(),
        terminal: terminal.to_owned(),
        title: title.to_owned(),
        place: Some("~/app".to_owned()),
        running: running.map(Duration::from_secs),
        by: String::new(),
    }
}

/// A terminal another Mac has open.
fn moved(terminal: &str, title: &str, by: &str) -> BackgroundFacts {
    BackgroundFacts {
        by: by.to_owned(),
        ..waiting(terminal, title, None)
    }
}

fn pane(tab: u64, pane: u64, title: &str, tab_title: &str, key: Option<&str>) -> ServerPane {
    ServerPane {
        tab,
        pane,
        title: title.to_owned(),
        tab_title: tab_title.to_owned(),
        key: key.map(str::to_owned),
    }
}

fn server(background: Vec<BackgroundFacts>, panes: Vec<ServerPane>) -> ServerFacts {
    ServerFacts {
        name: "rasp".to_owned(),
        background,
        elsewhere: Vec::new(),
        panes,
    }
}

fn host(name: &str) -> HostFacts {
    HostFacts {
        name: name.to_owned(),
        mark: HostMark::None,
        configured: true,
        tabs: 2,
    }
}

fn targets(servers: Vec<ServerFacts>) -> Targets {
    Targets {
        hosts: vec![host("rasp"), host("plain")],
        servers,
        ..Targets::default()
    }
}

fn ids(rows: &[Row]) -> Vec<&str> {
    rows.iter().map(|row| row.id.as_str()).collect()
}

fn two_waiting() -> Vec<BackgroundFacts> {
    vec![
        waiting("b-2", "claude", Some(720)),
        waiting("b-1", "zsh", None),
    ]
}

#[test]
fn a_terminal_in_the_background_says_what_it_is_where_and_for_how_long() {
    let row = background_row(&waiting("b-2", "claude", Some(720)));
    assert_eq!(row.id, "bg.b-2@u@rasp:22");
    assert_eq!(row.title, "claude");
    assert_eq!(row.subtitle, "rasp · ~/app");
    assert_eq!(row.kind, Kind::Background);
    assert_eq!(row.status.expect("a status").text, "Running · 12m 00s");
    assert_eq!(
        row.hints,
        [
            ("↵", "Open in New Tab".to_owned()),
            ("⌥↵", "Open Beside".to_owned())
        ]
    );
    // A shell waiting for a command is not running anything.
    assert_eq!(background_row(&waiting("b-1", "zsh", None)).status, None);
}

#[test]
fn the_facts_of_a_terminal_come_from_what_the_server_said_of_it() {
    let background = Background {
        account: ACCOUNT.to_owned(),
        host: "rasp".to_owned(),
        argv: Vec::new(),
        terminal: crate::kablo::SessionId::new("b-3").expect("an id"),
        program: Some("claude".to_owned()),
        cwd: Some("/srv/app".to_owned()),
        label: None,
        started: Some(100),
        front_started: Some(1_000),
        front_is_own: Some(false),
        output_at: Some(1_700),
        clients: 0,
        clients_named: Vec::new(),
    };
    let facts = BackgroundFacts::of(&background, 1_720);
    assert_eq!(facts.title, "claude");
    assert_eq!(facts.terminal, "b-3");
    assert_eq!(facts.place.as_deref(), Some("/srv/app"));
    assert_eq!(facts.running, Some(Duration::from_secs(720)));
    // A label is what a person called it; a shell waiting has run nothing.
    let labelled = Background {
        label: Some("review".to_owned()),
        front_is_own: Some(true),
        ..background
    };
    let facts = BackgroundFacts::of(&labelled, 1_720);
    assert_eq!(facts.title, "review");
    assert_eq!(facts.running, None);
}

#[test]
fn a_query_finds_a_terminal_by_its_program_or_its_server_and_the_empty_root_lists_none() {
    let targets = targets(vec![server(two_waiting(), Vec::new())]);
    let mut pool = host_rows(&targets);
    pool.extend(background_rows(&targets));

    let found = root(&pool, "claude", &[], &[], Fallbacks::ALL);
    assert_eq!(found.hits[0].row.id, "bg.b-2@u@rasp:22");
    assert_eq!(found.heading_before(0), Some(BACKGROUND_HEADING));

    let by_server = root(&pool, "rasp", &[], &[], Fallbacks::ALL);
    let found: Vec<&str> = by_server
        .hits
        .iter()
        .map(|hit| hit.row.id.as_str())
        .collect();
    assert!(found.contains(&"host.rasp"));
    assert!(found.contains(&"bg.b-1@u@rasp:22") && found.contains(&"bg.b-2@u@rasp:22"));

    let empty = root(&pool, "", &[], &[], Fallbacks::ALL);
    assert!(
        empty
            .hits
            .iter()
            .all(|hit| hit.row.kind != Kind::Background),
        "the empty field lists none of them"
    );
    assert!(
        !Kind::Background.remembered(),
        "choosing one is not a Recent"
    );
}

#[test]
fn a_server_kablo_keeps_terminals_on_has_a_page_and_a_count_and_a_plain_ssh_has_neither() {
    let kept = server(two_waiting(), Vec::new());
    let row = host_row(&host("rasp"), Some(&kept));
    assert_eq!(row.subtitle, "ssh config · 2 tabs · 2 in the background");
    assert_eq!(row.opens, Some(Scope::Server("rasp".to_owned())));
    assert!(!row.enters, "⏎ is still the new tab");
    let quiet = host_row(&host("rasp"), Some(&server(Vec::new(), Vec::new())));
    assert_eq!(quiet.subtitle, "ssh config · 2 tabs", "no count of nothing");
    assert!(
        quiet.opens.is_some(),
        "a page all the same: it lists the tabs"
    );

    let plain = host_row(&host("plain"), None);
    assert_eq!(plain.subtitle, "ssh config · 2 tabs");
    assert_eq!(plain.opens, None);

    // Through the targets: only the server in `servers` is one.
    let rows = host_rows(&targets(vec![kept]));
    assert!(rows[0].opens.is_some() && rows[1].opens.is_none());
}

#[test]
fn a_server_is_found_by_the_name_without_the_user_in_front() {
    let targets = targets(vec![server(Vec::new(), Vec::new())]);
    assert!(server_of(&targets, "rasp").is_some());
    assert!(server_of(&targets, "pi@rasp").is_some());
    assert!(server_of(&targets, "plain").is_none());
}

#[test]
fn the_last_row_of_a_servers_page_is_the_big_view_on_it_and_only_enter_chooses_it() {
    let targets = targets(vec![server(two_waiting(), Vec::new())]);
    let rows = server_rows("rasp", &targets);
    let last = rows.last().expect("a row");
    assert_eq!(last.id, "act.seeAll.rasp");
    assert_eq!(last.title, "See All on rasp");
    assert_eq!(
        go(&last.id, Modifier::None),
        Some(Go::SeeAll("rasp".to_owned()))
    );
    assert_eq!(go(&last.id, Modifier::Option), None);
    assert_eq!(go(&last.id, Modifier::Command), None);
}

#[test]
fn the_page_of_a_server_lists_open_all_the_background_the_panes_and_the_new_tab() {
    let panes = vec![
        pane(7, 70, "~/app", "web", Some("⌘2")),
        pane(7, 71, "~/logs", "web", Some("⌘2")),
        pane(9, 90, "~", "ops", None),
    ];
    let targets = targets(vec![server(two_waiting(), panes)]);
    let list = server_list("rasp", &targets, "");
    let listed: Vec<&str> = list.hits.iter().map(|hit| hit.row.id.as_str()).collect();
    assert_eq!(
        listed,
        [
            "act.openAll.rasp",
            "bg.b-2@u@rasp:22",
            "bg.b-1@u@rasp:22",
            "pane.7.70",
            "pane.7.71",
            "pane.9.90",
            "host.rasp",
            "act.seeAll.rasp",
        ]
    );
    assert_eq!(
        list.heading_before(0),
        None,
        "Open All stands first, headed by nothing"
    );
    assert_eq!(list.heading_before(1), Some(BACKGROUND_HEADING));
    assert_eq!(list.heading_before(3), Some(PANES_HEADING));
    assert_eq!(list.heading_before(6), Some("rasp"));
    assert_eq!(list.hits[0].row.subtitle, "2 terminals");
    assert_eq!(list.hits[6].row.title, "New Tab on rasp");
    assert_eq!(ids(&server_rows("rasp", &targets)), listed);

    // A pane says its tab at the right and the tab's digit.
    let row = &list.hits[3].row;
    assert_eq!(
        (row.title.as_str(), row.value.as_deref()),
        ("~/app", Some("web"))
    );
    assert_eq!(row.shortcut.as_deref(), Some("⌘2"));
    assert_eq!(list.hits[5].row.shortcut, None);
}

#[test]
fn a_server_with_nothing_in_the_background_has_no_open_all_and_no_empty_heading() {
    let targets = targets(vec![server(
        Vec::new(),
        vec![pane(7, 70, "~/app", "web", None)],
    )]);
    let list = server_list("rasp", &targets, "");
    let listed: Vec<&str> = list.hits.iter().map(|hit| hit.row.id.as_str()).collect();
    assert_eq!(listed, ["pane.7.70", "host.rasp", "act.seeAll.rasp"]);
    assert_eq!(list.headings[0], (0, PANES_HEADING.to_owned()));
    // A server the palette was not told of still offers the new tab.
    let unknown = server_list("elsewhere", &targets, "");
    assert_eq!(ids(&unknown.rows()), ["host.elsewhere"]);
}

#[test]
fn a_query_on_the_page_keeps_what_it_finds_in_each_section() {
    let panes = vec![
        pane(7, 70, "~/app", "web", None),
        pane(9, 90, "~", "ops", None),
    ];
    let targets = targets(vec![server(two_waiting(), panes)]);
    let by_program = server_list("rasp", &targets, "claude");
    assert_eq!(ids(&by_program.rows()), ["bg.b-2@u@rasp:22"]);
    assert_eq!(by_program.heading_before(0), Some(BACKGROUND_HEADING));
    // A pane is found by its tab's name.
    let by_tab = server_list("rasp", &targets, "ops");
    assert_eq!(ids(&by_tab.rows()), ["pane.9.90"]);
    assert_eq!(by_tab.heading_before(0), Some(PANES_HEADING));
    assert!(server_list("rasp", &targets, "zzzz").is_empty());
}

#[test]
fn choosing_a_row_of_the_server_comes_to_what_the_row_says() {
    use super::super::targets::go;
    let account = ACCOUNT.to_owned();
    let terminal = "b-2".to_owned();
    assert_eq!(
        go("bg.b-2@u@rasp:22", Modifier::None),
        Some(Go::OpenBackground {
            account: account.clone(),
            terminal: terminal.clone()
        })
    );
    assert_eq!(
        go("bg.b-2@u@rasp:22", Modifier::Option),
        Some(Go::SplitBackground { account, terminal })
    );
    assert_eq!(go("bg.b-2@u@rasp:22", Modifier::Command), None);
    assert_eq!(go("bg.nonsense", Modifier::None), None, "no account in it");
    assert_eq!(
        go("act.openAll.rasp", Modifier::None),
        Some(Go::OpenAllBackground("rasp".to_owned()))
    );
    assert_eq!(go("act.openAll.rasp", Modifier::Option), None);
    assert_eq!(
        go("pane.7.70", Modifier::None),
        Some(Go::FocusPane { tab: 7, pane: 70 })
    );
    assert_eq!(go("pane.7", Modifier::None), None);
    // A terminal another Mac has open is moved here by ⏎ and by nothing else.
    assert_eq!(
        go("mv.b-2@u@rasp:22", Modifier::None),
        Some(Go::MoveHere {
            account: ACCOUNT.to_owned(),
            terminal: "b-2".to_owned()
        })
    );
    assert_eq!(go("mv.b-2@u@rasp:22", Modifier::Option), None);
    assert_eq!(go("mv.b-2@u@rasp:22", Modifier::Command), None);
    assert_eq!(go("mv.nonsense", Modifier::None), None, "no account in it");
    assert_eq!(go("pane.x.1", Modifier::None), None);
    // The new tab of the page is the server's own row: ⏎ and ⌥⏎ are what they always were.
    assert_eq!(
        go("host.rasp", Modifier::None),
        Some(Go::OpenHost("rasp".to_owned()))
    );
    assert_eq!(
        go("host.rasp", Modifier::Option),
        Some(Go::SplitHost("rasp".to_owned()))
    );
}

/// The terminals another Mac has open are rows of the server's page, after the tabs: that Mac's
/// name at the right, and the band says what ⏎ does to the pane there. A search from the root does
/// not find them — ⏎ on a row found by chance must not take a terminal from the Mac using it.
#[test]
fn the_terminals_on_other_macs_are_on_the_servers_page_and_in_no_search_from_the_root() {
    let row = elsewhere_row(&moved("b-5", "claude", "iMac"));
    assert_eq!(row.id, "mv.b-5@u@rasp:22");
    assert_eq!(row.kind, Kind::Elsewhere);
    assert_eq!(row.title, "claude");
    assert_eq!(row.subtitle, "rasp · ~/app");
    assert_eq!(row.value.as_deref(), Some("iMac"));
    assert_eq!(
        row.hints,
        [("↵", "Move Here · the pane on iMac goes dim".to_owned())]
    );

    let mut kept = server(two_waiting(), vec![pane(7, 70, "~/app", "web", Some("⌘1"))]);
    kept.elsewhere = vec![moved("b-5", "claude", "iMac")];
    let all = targets(vec![kept]);
    let list = server_list("rasp", &all, "");
    let titles: Vec<&str> = list
        .headings
        .iter()
        .map(|(_, text)| text.as_str())
        .collect();
    assert_eq!(
        titles,
        [BACKGROUND_HEADING, PANES_HEADING, ELSEWHERE_HEADING, "rasp"]
    );
    let ids: Vec<&str> = list.hits.iter().map(|hit| hit.row.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "act.openAll.rasp",
            "bg.b-2@u@rasp:22",
            "bg.b-1@u@rasp:22",
            "pane.7.70",
            "mv.b-5@u@rasp:22",
            "host.rasp",
            "act.seeAll.rasp"
        ]
    );
    // A query on the page keeps it when it finds it, by the Mac's name too.
    let found = server_list("rasp", &all, "imac");
    assert_eq!(found.hits.len(), 1);
    assert_eq!(found.hits[0].row.id, "mv.b-5@u@rasp:22");

    // Nothing at the root finds it: the root's rows are the background's.
    let mut pool = host_rows(&all);
    pool.extend(background_rows(&all));
    let found = root(&pool, "claude", &[], &[], Fallbacks::ALL);
    let root_ids: Vec<&str> = found.hits.iter().map(|hit| hit.row.id.as_str()).collect();
    assert!(
        root_ids.iter().all(|id| !id.starts_with("mv.")),
        "{root_ids:?}"
    );
    assert!(root_ids.iter().any(|id| id.starts_with("bg.")));
}
