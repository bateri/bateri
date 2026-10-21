use std::os::unix::fs::PermissionsExt as _;

use super::*;
use crate::settings::TempRoot;

const PANE: &str = "0F1E2D3C-4B5A-6978-8796-A5B4C3D2E1F0";

fn pane() -> PaneUuid {
    PaneUuid::parse(PANE).expect("a uuid")
}

fn id(name: &str) -> SessionId {
    SessionId::new(name).expect("an id")
}

fn server(key: &str, args: &[&str]) -> KabloHost {
    KabloHost {
        host: "rasp".to_owned(),
        port: Some(2222),
        argv: args.iter().map(|arg| (*arg).to_owned()).collect(),
        // What `ssh -G` said: never kept.
        config: "user ada\nidentityfile ~/.ssh/id\n".to_owned(),
        key: key.to_owned(),
    }
}

fn installed() -> Installed {
    Installed {
        target: Target::LinuxAarch64,
        version: "0.2.0".to_owned(),
        path: ".local/share/kablo/0.2.0/kablo".to_owned(),
        fresh: true,
    }
}

/// A ledger with an account that has a live terminal, an account with another, and a
/// terminal to end of an account the ledger never made.
fn rich() -> Rows {
    let mut rows = Rows::default();
    let known = Known {
        key: "ada@rasp:2222 via my jump:22".to_owned(),
        host: "rasp".to_owned(),
        port: Some(2222),
        argv: ["ssh", "-p", "2222", "-J", "my jump", "ada@rasp"]
            .iter()
            .map(|arg| (*arg).to_owned())
            .collect(),
        target: Target::LinuxAarch64,
        version: "0.2.0".to_owned(),
    };
    rows.accounts.insert(known.key.clone(), known);
    rows.accounts.insert(
        "root@prod:22".to_owned(),
        Known {
            key: "root@prod:22".to_owned(),
            host: "prod".to_owned(),
            port: None,
            argv: vec!["ssh".to_owned(), "prod".to_owned()],
            target: Target::MacosAarch64,
            version: "0.2.1-rc.1".to_owned(),
        },
    );
    rows.terminals.push(Row {
        account: "ada@rasp:2222 via my jump:22".to_owned(),
        terminal: id("b-0123456789abcdef0123456789abcdef"),
        pane: Some(pane()),
        ending: false,
    });
    rows.terminals.push(Row {
        account: "root@prod:22".to_owned(),
        terminal: id("b-fedcba9876543210fedcba9876543210"),
        pane: None,
        ending: false,
    });
    rows.terminals.push(Row {
        account: "gone@elsewhere:22".to_owned(),
        terminal: id("b-1"),
        pane: None,
        ending: true,
    });
    rows
}

/// Version 1 for [`rich`], frozen: the text a bateri of this version wrote, which every later
/// one reads. Never updated — a change of the format is a new version with a new fixture
/// beside this one.
const V1_FIXTURE: &str = concat!(
    "bateri-kablo 1\n",
    r"A ada@rasp:2222\svia\smy\sjump:22 rasp 2222 aarch64-unknown-linux-musl 0.2.0 ssh -p 2222 -J my\sjump ada@rasp",
    "\n",
    "A root@prod:22 prod - aarch64-apple-darwin 0.2.1-rc.1 ssh prod\n",
    r"T ada@rasp:2222\svia\smy\sjump:22 b-0123456789abcdef0123456789abcdef 0F1E2D3C-4B5A-6978-8796-A5B4C3D2E1F0 live",
    "\n",
    "T root@prod:22 b-fedcba9876543210fedcba9876543210 - live\n",
    "T gone@elsewhere:22 b-1 - ending\n",
);

#[test]
fn the_text_of_the_first_version_is_read_and_written_byte_for_byte() {
    assert_eq!(Rows::parse(V1_FIXTURE), Some(rich()));
    assert_eq!(rich().render(), V1_FIXTURE);
}

#[test]
fn text_that_is_not_exactly_understood_is_an_empty_ledger() {
    let broken = |from: &str, to: &str| {
        assert!(V1_FIXTURE.contains(from), "{from:?}");
        V1_FIXTURE.replacen(from, to, 1)
    };
    for text in [
        String::new(),
        "bateri-kablo 2\n".to_owned(),
        "bateri-kablo 1 extra\n".to_owned(),
        "bateri-kablo 1".to_owned(),
        broken("aarch64-unknown-linux-musl", "vax-unknown-vms"),
        broken("rasp 2222", "rasp 70000"),
        broken(" 0.2.0 ssh", " ../x ssh"),
        // An account that names no way to reach the server.
        broken("0.2.1-rc.1 ssh prod\n", "0.2.1-rc.1\n"),
        broken(" live\nT root@prod:22", " idle\nT root@prod:22"),
        broken("b-1 - ending", "b/1 - ending"),
        broken("b-1 - ending", "b-1 not-a-pane ending"),
        // The same terminal twice.
        format!("{V1_FIXTURE}T gone@elsewhere:22 b-1 - live\n"),
        format!("{V1_FIXTURE}X something\n"),
        format!("{V1_FIXTURE}\n"),
    ] {
        assert_eq!(Rows::parse(&text), None, "{text:?}");
    }
    assert!(Rows::parse(V1_FIXTURE).is_some(), "the control");
}

#[test]
fn what_a_terminal_needs_to_come_back_is_what_the_ledger_was_told() {
    let ledger = Ledger::memory();
    let server = server("ada@rasp:2222", &["ssh", "-p", "2222", "ada@rasp"]);
    ledger.remember(&server, &installed());
    ledger.shown("ada@rasp:2222", &id("b-1"), Some(&pane()));

    let known = ledger.known("ada@rasp:2222").expect("remembered");
    assert_eq!(known.server().argv, server.argv);
    assert_eq!(known.server().host, "rasp");
    assert_eq!(known.server().port, Some(2222));
    assert_eq!(known.server().key, "ada@rasp:2222");
    // `ssh -G`'s dump is never kept.
    assert_eq!(known.server().config, "");
    let again = known.installed();
    assert_eq!(again.target, Target::LinuxAarch64);
    assert_eq!(again.version, "0.2.0");
    assert_eq!(again.path, ".local/share/kablo/0.2.0/kablo");
    assert!(!again.fresh, "what was installed is not new again");
    assert!(ledger.keeps("ada@rasp:2222", &id("b-1")));
    assert!(
        !ledger.keeps("ada@rasp:2222", &id("b-2")),
        "another terminal"
    );
    assert!(!ledger.keeps("root@rasp:22", &id("b-1")), "another account");
}

#[test]
fn a_terminal_asked_to_end_waits_in_the_ledger_until_it_is_seen_to_have() {
    let ledger = Ledger::memory();
    ledger.remember(&server("ada@rasp:2222", &["ssh", "rasp"]), &installed());
    ledger.shown("ada@rasp:2222", &id("b-1"), Some(&pane()));
    ledger.shown("ada@rasp:2222", &id("b-2"), None);

    ledger.end("ada@rasp:2222", &id("b-1"));
    assert!(
        !ledger.keeps("ada@rasp:2222", &id("b-1")),
        "no pane is shown it again"
    );
    assert_eq!(ledger.ending("ada@rasp:2222"), [id("b-1")]);
    assert!(
        ledger.ending("root@rasp:22").is_empty(),
        "another account's"
    );
    // Asking twice changes nothing.
    ledger.end("ada@rasp:2222", &id("b-1"));
    assert_eq!(ledger.ending("ada@rasp:2222"), [id("b-1")]);
    // Starting it again does not take the request back.
    ledger.shown("ada@rasp:2222", &id("b-1"), Some(&pane()));
    assert!(!ledger.keeps("ada@rasp:2222", &id("b-1")));

    ledger.gone("ada@rasp:2222", &id("b-1"));
    assert!(ledger.ending("ada@rasp:2222").is_empty());
    // The other terminal keeps its account.
    assert!(ledger.known("ada@rasp:2222").is_some());
    ledger.gone("ada@rasp:2222", &id("b-2"));
    assert!(
        ledger.known("ada@rasp:2222").is_none(),
        "an account with no terminal left is only a row to show them with"
    );
    // A terminal nobody started is ended by name all the same.
    ledger.end("ada@rasp:2222", &id("b-9"));
    assert_eq!(ledger.ending("ada@rasp:2222"), [id("b-9")]);
}

#[test]
fn an_account_is_there_from_its_making_to_its_last_terminal() {
    let ledger = Ledger::memory();
    ledger.remember(&server("ada@rasp:2222", &["ssh", "rasp"]), &installed());
    assert!(
        ledger.known("ada@rasp:2222").is_some(),
        "its first terminal comes a moment after it"
    );
    ledger.shown("ada@rasp:2222", &id("b-1"), Some(&pane()));
    ledger.end_all();
    assert!(ledger.known("ada@rasp:2222").is_none());
}

#[test]
fn every_live_terminal_can_be_marked_to_end_at_once() {
    let ledger = Ledger::memory();
    ledger.remember(&server("ada@rasp:2222", &["ssh", "rasp"]), &installed());
    ledger.shown("ada@rasp:2222", &id("b-1"), Some(&pane()));
    ledger.shown("ada@rasp:2222", &id("b-2"), None);
    ledger.end("ada@rasp:2222", &id("b-2"));

    assert_eq!(ledger.end_all(), 1, "the one that was not already ending");
    assert_eq!(ledger.end_all(), 0);
    assert_eq!(ledger.ending("ada@rasp:2222"), [id("b-1"), id("b-2")]);
    assert!(!ledger.keeps("ada@rasp:2222", &id("b-1")));
}

#[test]
fn the_ledger_lives_in_its_file_and_a_second_process_sees_it() {
    let root = TempRoot::new("ledger-file");
    // The directory does not exist yet: the first write makes it, private.
    let path = root.0.join("session").join("kablo");
    let first = Ledger::open(path.clone());
    assert!(first.known("ada@rasp:2222").is_none());
    assert!(!path.exists(), "reading writes nothing");

    first.remember(
        &server("ada@rasp:2222", &["ssh", "-p", "2222", "rasp"]),
        &installed(),
    );
    first.shown("ada@rasp:2222", &id("b-1"), Some(&pane()));
    let mode = |path: &Path| {
        std::fs::metadata(path)
            .expect("exists")
            .permissions()
            .mode()
            & 0o777
    };
    assert_eq!(mode(&path), 0o600);
    assert_eq!(mode(path.parent().expect("a directory")), 0o700);

    // Another ledger over the same file is another process of the same bundle.
    let second = Ledger::open(path.clone());
    assert!(second.keeps("ada@rasp:2222", &id("b-1")));
    second.end("ada@rasp:2222", &id("b-1"));
    assert!(
        !first.keeps("ada@rasp:2222", &id("b-1")),
        "the file is the truth"
    );
    assert_eq!(first.ending("ada@rasp:2222"), [id("b-1")]);

    let text = std::fs::read_to_string(&path).expect("the file");
    assert!(text.starts_with("bateri-kablo 1\n"), "{text}");
    assert!(
        !text.contains("identityfile"),
        "what `ssh -G` said is not kept: {text}"
    );
}

#[test]
fn a_file_that_is_not_understood_is_an_empty_ledger_and_the_next_change_replaces_it() {
    let root = TempRoot::new("ledger-broken");
    let path = root.0.join("kablo");
    std::fs::write(&path, "bateri-kablo 99\nA from the future\n").expect("write");
    let ledger = Ledger::open(path.clone());
    assert!(ledger.terminals().is_empty());
    ledger.end("ada@rasp:2222", &id("b-1"));
    let text = std::fs::read_to_string(&path).expect("the file");
    assert_eq!(
        text, "bateri-kablo 1\nT ada@rasp:2222 b-1 - ending\n",
        "written in the version this bateri writes"
    );
}

#[test]
fn a_ledger_in_memory_touches_no_disk() {
    let ledger = Ledger::memory();
    assert!(ledger.path().is_none());
    ledger.end("ada@rasp:2222", &id("b-1"));
    assert_eq!(ledger.ending("ada@rasp:2222"), [id("b-1")]);
}
