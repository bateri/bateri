//! What a server is asked about when it is new to the person's programs: the ones the person
//! follows that the server neither has nor was told it need not have, and that their author built
//! for the server's kind of computer.

use std::fs;
use std::time::SystemTime;

use bt_core::pencere::{SUPPORTED_VERSIONS, offered_views};
use bt_shell_common::apps::card::missing;
use bt_shell_common::apps::package::MachineState;
use bt_shell_common::apps::{Config, store};

use super::support::{Author, Net, OTHER_TARGET, Release, TARGET, Temp, v};

const SITE: &str = "https://apps.example.test";

fn address(name: &str) -> String {
    format!("{SITE}/{name}/app.json")
}

/// A folder in which `programs` (a name and the targets its author built for) have landed.
fn followed(label: &str, programs: &[(&str, &[&'static str])]) -> (Temp, Author) {
    followed_at(label, programs, SITE)
}

/// As [`followed`], with the programs' sources on `site`.
fn followed_at(label: &str, programs: &[(&str, &[&'static str])], site: &str) -> (Temp, Author) {
    let address = |name: &str| format!("{site}/{name}/app.json");
    let net = Net::default();
    let root = Temp::new(label);
    let author = Author::new(7);
    let mut sources = Vec::new();
    for (name, targets) in programs {
        Release::new(name, "1.0.0")
            .targets(targets)
            .publish(&net, &address(name), &author);
        store::approve(root.path(), name, &author.key()).expect("approved");
        sources.push(address(name));
    }
    let config = Config {
        root: root.path().to_owned(),
        target: TARGET.to_owned(),
        protocols: SUPPORTED_VERSIONS.to_vec(),
        views: offered_views(),
        in_app_key: None,
    };
    let statuses = bt_shell_common::apps::check(&config, &sources, true, &net, SystemTime::now());
    assert_eq!(statuses.len(), programs.len());
    (root, author)
}

fn machine(target: &str, installed: &[&str], not_here: &[&str]) -> MachineState {
    MachineState {
        target: target.to_owned(),
        installed: installed.iter().map(|name| (*name).to_owned()).collect(),
        not_here: not_here.iter().map(|name| (*name).to_owned()).collect(),
    }
}

fn names(rows: &[bt_shell_common::apps::card::Row]) -> Vec<&str> {
    rows.iter().map(|row| row.name.as_str()).collect()
}

#[test]
fn a_server_is_asked_about_what_it_has_neither_got_nor_declined() {
    let (root, author) = followed(
        "card-missing",
        &[
            ("demo", &[TARGET, OTHER_TARGET]),
            ("have", &[TARGET, OTHER_TARGET]),
            ("declined", &[TARGET, OTHER_TARGET]),
        ],
    );
    let rows = missing(
        root.path(),
        &machine(OTHER_TARGET, &["have"], &["declined"]),
    );
    assert_eq!(names(&rows), ["demo"]);
    // What the row brings is what kablo is typed at a terminal: the source and the key.
    assert_eq!(
        rows[0].words,
        [
            "package".to_owned(),
            "install".to_owned(),
            address("demo"),
            "--trust".to_owned(),
            author.key()
        ]
    );
}

#[test]
fn a_program_its_author_did_not_build_for_the_server_is_not_asked_about() {
    let (root, _) = followed(
        "card-target",
        &[("mac", &[TARGET]), ("both", &[TARGET, OTHER_TARGET])],
    );
    let rows = missing(root.path(), &machine(OTHER_TARGET, &[], &[]));
    assert_eq!(names(&rows), ["both"]);
    let rows = missing(root.path(), &machine(TARGET, &[], &[]));
    assert_eq!(names(&rows), ["both", "mac"]);
}

#[test]
fn a_release_that_cannot_be_vouched_for_is_not_offered() {
    let (root, _) = followed(
        "card-broken",
        &[
            ("good", &[TARGET, OTHER_TARGET]),
            ("bad", &[TARGET, OTHER_TARGET]),
        ],
    );
    let folder = store::release_dir(root.path(), "bad", v("1.0.0"));
    fs::write(folder.join("app.json.sig"), b"not a signature").expect("spoiled");
    let rows = missing(root.path(), &machine(OTHER_TARGET, &[], &[]));
    assert_eq!(names(&rows), ["good"]);
}

#[test]
fn a_followed_program_that_never_landed_is_not_offered_and_nothing_followed_asks_nothing() {
    let root = Temp::new("card-none");
    assert!(missing(root.path(), &machine(OTHER_TARGET, &[], &[])).is_empty());
    store::follow(root.path(), "waiting", &address("waiting")).expect("followed");
    assert!(missing(root.path(), &machine(OTHER_TARGET, &[], &[])).is_empty());
}

#[test]
fn a_program_only_this_computer_can_reach_is_not_asked_of_a_server() {
    let (root, _) = followed_at(
        "card-lan",
        &[("demo", &[TARGET, OTHER_TARGET])],
        "https://nas.local",
    );
    let rows = missing(root.path(), &machine(OTHER_TARGET, &[], &[]));
    assert!(
        rows.is_empty(),
        "a server is refused such a source, so it is not asked: {rows:?}"
    );
}
