//! What the app answers when a machine asks about programs (`package`): the rules a server's
//! request is answered by, and what the answer carries.

use std::io;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use bt_core::pencere::apps;
use bt_core::pencere::{SUPPORTED_VERSIONS, offered_views};
use bt_shell_common::apps::package::{Limits, Who};
use bt_shell_common::apps::{Answerer, Config, Fetch, store};
use bt_shell_common::kablo::install::{Channel, Reply as StepReply};
use kablo_bateri::message::{Kind, Reply};
use kablo_bateri::package::{Answer, Fetch as Bring, KeyChanged, Offer, Package, State};

use super::support::{Author, Net, OTHER_TARGET, Release, TARGET, Temp};

const SOURCE: &str = "https://apps.example.test/demo/app.json";

/// A machine that writes nothing: it keeps what it was asked to write.
#[derive(Default)]
struct Sink {
    steps: Mutex<Vec<(String, Vec<u8>)>>,
}

impl Sink {
    fn steps(&self) -> Vec<(String, Vec<u8>)> {
        self.steps.lock().expect("lock").clone()
    }
}

impl Channel for Sink {
    fn run(&self, script: &str, input: &[u8]) -> io::Result<StepReply> {
        self.steps
            .lock()
            .expect("lock")
            .push((script.to_owned(), input.to_vec()));
        Ok(StepReply {
            status: Some(0),
            out: String::new(),
            err: String::new(),
        })
    }
}

struct World {
    net: Arc<Net>,
    root: Temp,
    sink: Sink,
}

impl World {
    fn new(label: &str) -> Self {
        Self {
            net: Arc::new(Net::default()),
            root: Temp::new(label),
            sink: Sink::default(),
        }
    }

    fn config(&self) -> Config {
        Config {
            root: self.root.path().to_owned(),
            target: TARGET.to_owned(),
            protocols: SUPPORTED_VERSIONS.to_vec(),
            views: offered_views(),
            in_app_key: None,
        }
    }

    fn answerer(&self) -> Answerer {
        Answerer::new(self.config(), Arc::clone(&self.net) as Arc<dyn Fetch>)
    }

    fn limited(&self, most: usize) -> Answerer {
        Answerer::with_limits(
            self.config(),
            Arc::clone(&self.net) as Arc<dyn Fetch>,
            Limits {
                window: Duration::from_secs(600),
                most,
                keep: Duration::from_secs(300),
            },
        )
    }

    fn ask(&self, answerer: &Answerer, who: &Who, package: Package) -> Reply {
        answerer.answer(who, package, &self.sink)
    }
}

fn server(account: &str) -> Who {
    Who {
        account: account.to_owned(),
        host: "dev-1".to_owned(),
        local: false,
    }
}

fn this_computer() -> Who {
    Who {
        account: "local".to_owned(),
        host: String::new(),
        local: true,
    }
}

fn offer(source: &str, have: Option<&str>) -> Package {
    Package::Offer(Offer {
        source: source.to_owned(),
        target: TARGET.to_owned(),
        key: None,
        have: have.map(str::to_owned),
    })
}

fn bring(source: &str, version: &str, target: &str, part: &str) -> Package {
    Package::Fetch(Bring {
        name: "demo".to_owned(),
        version: version.to_owned(),
        target: target.to_owned(),
        source: source.to_owned(),
        part: part.to_owned(),
    })
}

fn offered(reply: Reply) -> kablo_bateri::package::Offered {
    match reply {
        Reply::Package(Answer::Offered(offered)) => offered,
        other => panic!("an offer was expected, found {other:?}"),
    }
}

fn failed(reply: Reply) -> kablo_bateri::message::Failure {
    match reply {
        Reply::Failed(failure) => failure,
        other => panic!("a failure was expected, found {other:?}"),
    }
}

#[test]
fn a_release_is_offered_with_what_the_machine_needs_to_judge_it_and_not_when_it_has_it() {
    let world = World::new("offer");
    let author = Author::new(1);
    let json = Release::new("demo", "1.0.0").publish(&world.net, SOURCE, &author);
    let answerer = world.answerer();

    let one = offered(world.ask(&answerer, &server("u@h:22"), offer(SOURCE, None)));
    assert_eq!(one.source, SOURCE);
    assert_eq!(one.manifest.as_bytes(), json);
    // What the machine gets is a release whose signature holds, by the key the file names.
    let manifest = apps::verify(one.manifest.as_bytes(), one.signature.as_bytes()).expect("holds");
    assert_eq!(manifest.key, author.key());

    // The machine has it, or a newer one: nothing newer is offered.
    for have in ["1.0.0", "2.0.0"] {
        assert_eq!(
            world.ask(&answerer, &server("u@h:22"), offer(SOURCE, Some(have))),
            Reply::Package(Answer::Current),
            "{have}"
        );
    }

    // A newer release is offered once the sources are looked at again.
    Release::new("demo", "1.1.0").publish(&world.net, SOURCE, &author);
    answerer.forget();
    let two = offered(world.ask(&answerer, &server("u@h:22"), offer(SOURCE, Some("1.0.0"))));
    let manifest = apps::verify(two.manifest.as_bytes(), two.signature.as_bytes()).expect("holds");
    assert_eq!(manifest.version.to_string(), "1.1.0");
}

#[test]
fn a_release_with_no_file_for_the_kind_of_machine_is_no_offer() {
    let world = World::new("no-file");
    Release::new("demo", "1.0.0")
        .targets(&[OTHER_TARGET])
        .publish(&world.net, SOURCE, &Author::new(1));
    let reply = world.ask(&world.answerer(), &server("a"), offer(SOURCE, None));
    let failure = failed(reply);
    assert_eq!(failure.kind, Kind::Failed);
    assert!(failure.text.contains(TARGET), "{}", failure.text);
}

#[test]
fn a_source_on_this_computer_is_for_this_computers_own_account_only() {
    let world = World::new("file-source");
    let address = "file:///Users/someone/demo/app.json";
    Release::new("demo", "1.0.0").publish(&world.net, address, &Author::new(1));
    let answerer = world.answerer();

    let refusal = failed(world.ask(&answerer, &server("u@h:22"), offer(address, None)));
    assert_eq!(refusal.kind, Kind::Refused);
    assert!(world.net.asked().is_empty(), "{:?}", world.net.asked());
    // Nor is it the way to a file when the program is asked for.
    let refusal = failed(world.ask(
        &answerer,
        &server("u@h:22"),
        bring(address, "1.0.0", TARGET, "p1"),
    ));
    assert_eq!(refusal.kind, Kind::Refused);
    assert!(world.sink.steps().is_empty());

    // This computer's own account reads it, and a file named beside it.
    let one = offered(world.ask(&answerer, &this_computer(), offer(address, None)));
    assert_eq!(one.source, address);
    assert_eq!(
        world.ask(
            &answerer,
            &this_computer(),
            bring(address, "1.0.0", TARGET, "p1")
        ),
        Reply::Package(Answer::Brought)
    );
}

#[test]
fn a_server_is_not_sent_to_a_host_of_this_machine_or_of_the_network_behind_it() {
    let world = World::new("private");
    let answerer = world.answerer();
    for address in [
        "https://localhost/demo/app.json",
        "https://127.0.0.1/demo/app.json",
        "https://[::1]/demo/app.json",
        "https://192.168.1.1/demo/app.json",
        "https://10.0.0.7:8443/demo/app.json",
        "https://169.254.169.254/latest/meta-data",
        "https://printer.local/demo/app.json",
        "https://router/demo/app.json",
        "https://user:secret@apps.example.test/demo/app.json",
        "https://2130706433/demo/app.json",
        "https://0x7f.1/demo/app.json",
    ] {
        let refusal = failed(world.ask(&answerer, &server("u@h:22"), offer(address, None)));
        assert_eq!(refusal.kind, Kind::Refused, "{address}");
    }
    assert!(world.net.asked().is_empty(), "{:?}", world.net.asked());
}

#[test]
fn what_a_release_names_is_judged_too_so_a_private_file_is_not_downloaded_for_a_server() {
    let world = World::new("private-file");
    let author = Author::new(1);
    Release::new("demo", "1.0.0")
        .files_at("https://192.168.1.5/files/")
        .publish(&world.net, SOURCE, &author);
    let answerer = world.answerer();

    // The release is the author's to publish, so it is offered; its file is not fetched.
    offered(world.ask(&answerer, &server("u@h:22"), offer(SOURCE, None)));
    let asked = world.net.asked().len();
    let failure = failed(world.ask(
        &answerer,
        &server("u@h:22"),
        bring(SOURCE, "1.0.0", TARGET, "p1"),
    ));
    assert!(failure.text.contains("refused"), "{}", failure.text);
    assert_eq!(world.net.asked().len(), asked, "{:?}", world.net.asked());
    assert!(world.sink.steps().is_empty());
}

#[test]
fn the_release_before_the_latest_is_offered_when_this_bateri_cannot_open_the_latest() {
    let world = World::new("previous");
    let author = Author::new(1);
    let old_manifest = "https://apps.example.test/demo/1.0.0/app.json";
    let old = Release::new("demo", "1.0.0");
    old.publish(&world.net, old_manifest, &author);
    // 2.0.0 shows a kind of view this bateri does not open.
    Release::new("demo", "2.0.0")
        .views(r#"[{"kind":"lua","versions":["9.0"]}]"#)
        .previous(&format!("[{}]", old.as_previous(old_manifest)))
        .publish(&world.net, SOURCE, &author);
    let one = offered(world.ask(&world.answerer(), &server("a"), offer(SOURCE, None)));
    let manifest = apps::verify(one.manifest.as_bytes(), one.signature.as_bytes()).expect("holds");
    assert_eq!(manifest.version.to_string(), "1.0.0");
}

#[test]
fn an_earlier_release_a_source_lists_at_a_private_address_is_not_read_for_a_server() {
    let world = World::new("previous-private");
    let author = Author::new(1);
    let old_manifest = "https://192.168.1.9/demo/1.0.0/app.json";
    let old = Release::new("demo", "1.0.0");
    old.publish(&world.net, old_manifest, &author);
    Release::new("demo", "2.0.0")
        .views(r#"[{"kind":"lua","versions":["9.0"]}]"#)
        .previous(&format!("[{}]", old.as_previous(old_manifest)))
        .publish(&world.net, SOURCE, &author);
    let answerer = world.answerer();

    let failure = failed(world.ask(&answerer, &server("a"), offer(SOURCE, None)));
    assert!(failure.text.contains("refused"), "{}", failure.text);
    assert!(
        world
            .net
            .asked()
            .iter()
            .all(|address| !address.contains("192.168")),
        "{:?}",
        world.net.asked()
    );
    // This computer's own account is not so limited.
    offered(world.ask(&answerer, &this_computer(), offer(SOURCE, None)));
}

#[test]
fn bytes_whose_signature_or_sum_does_not_hold_are_not_sent() {
    let world = World::new("holds");
    let author = Author::new(1);
    let release = Release::new("demo", "1.0.0");
    let json = release.publish(&world.net, SOURCE, &author);
    let answerer = world.answerer();

    // A file that is not the one signed.
    let address = release.file_address(SOURCE, TARGET);
    world.net.put(&address, "#!/bin/sh\necho something else\n");
    let failure = failed(world.ask(
        &answerer,
        &server("u@h:22"),
        bring(SOURCE, "1.0.0", TARGET, "p1"),
    ));
    assert!(failure.text.contains("sum"), "{}", failure.text);
    assert!(world.sink.steps().is_empty());

    // An `app.json` changed after it was signed.
    let changed = String::from_utf8(json)
        .expect("text")
        .replace("1.0.0", "9.9.9");
    world.net.put(SOURCE, changed);
    answerer.forget();
    let failure = failed(world.ask(&answerer, &server("u@h:22"), offer(SOURCE, None)));
    assert_eq!(failure.kind, Kind::Failed);
    assert!(world.sink.steps().is_empty());
}

#[test]
fn a_program_nobody_follows_is_sent_once_whole_and_nothing_of_it_is_kept() {
    let world = World::new("not-followed");
    let author = Author::new(1);
    let release = Release::new("demo", "1.0.0");
    release.publish(&world.net, SOURCE, &author);
    let answerer = world.answerer();

    let reply = world.ask(
        &answerer,
        &server("u@h:22"),
        bring(SOURCE, "1.0.0", TARGET, "p1"),
    );
    assert_eq!(reply, Reply::Package(Answer::Brought));
    let steps = world.sink.steps();
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].1, release.body.as_bytes());
    assert!(steps[0].0.contains(".1.0.0.part-p1"), "{}", steps[0].0);
    // Not a byte of it is in the folder of the programs the person follows.
    assert!(
        std::fs::read_dir(world.root.path())
            .expect("the folder")
            .next()
            .is_none()
    );
}

#[test]
fn a_release_that_is_not_the_one_offered_is_not_sent_in_its_place() {
    let world = World::new("moved-on");
    let author = Author::new(1);
    Release::new("demo", "1.0.0").publish(&world.net, SOURCE, &author);
    let answerer = world.answerer();
    offered(world.ask(&answerer, &server("a"), offer(SOURCE, None)));

    Release::new("demo", "1.1.0").publish(&world.net, SOURCE, &author);
    answerer.forget();
    let failure = failed(world.ask(
        &answerer,
        &server("a"),
        bring(SOURCE, "1.0.0", TARGET, "p1"),
    ));
    assert!(failure.text.contains("look again"), "{}", failure.text);
    assert!(world.sink.steps().is_empty());
}

#[test]
fn a_followed_program_is_answered_from_its_folder_and_the_file_stays_for_the_next_machine() {
    let world = World::new("followed");
    let author = Author::new(1);
    let release = Release::new("demo", "1.0.0");
    release.publish(&world.net, SOURCE, &author);
    store::approve(world.root.path(), "demo", &author.key()).expect("approved");
    let statuses = bt_shell_common::apps::check(
        &world.config(),
        &[SOURCE.to_owned()],
        true,
        world.net.as_ref(),
        SystemTime::now(),
    );
    assert_eq!(statuses.len(), 1);
    let answerer = world.answerer();

    // A bare name stands for what the person follows.
    let one = offered(world.ask(&answerer, &server("a"), offer("demo", None)));
    assert_eq!(one.source, SOURCE);
    assert_eq!(
        world.ask(
            &answerer,
            &server("a"),
            bring("demo", "1.0.0", OTHER_TARGET, "p1")
        ),
        Reply::Package(Answer::Brought)
    );
    let folder = store::release_dir(world.root.path(), "demo", super::support::v("1.0.0"));
    assert!(folder.join(OTHER_TARGET).is_file(), "kept for the next");

    // The next machine's file comes from the folder: the network is not asked for it again.
    let asked = world.net.asked().len();
    assert_eq!(
        world.ask(
            &answerer,
            &server("b"),
            bring("demo", "1.0.0", OTHER_TARGET, "p2")
        ),
        Reply::Package(Answer::Brought)
    );
    assert_eq!(world.net.asked().len(), asked, "{:?}", world.net.asked());
    assert_eq!(world.sink.steps().len(), 2);
}

#[test]
fn a_bare_name_is_a_program_the_person_follows_and_no_other() {
    let world = World::new("bare");
    let failure = failed(world.ask(&world.answerer(), &server("a"), offer("demo", None)));
    assert_eq!(failure.kind, Kind::Failed);
    assert!(
        failure.text.contains("follows no program"),
        "{}",
        failure.text
    );
}

#[test]
fn ten_machines_asking_about_a_source_make_one_look_at_it() {
    let world = World::new("cache");
    Release::new("demo", "1.0.0").publish(&world.net, SOURCE, &Author::new(1));
    let answerer = world.answerer();
    for n in 0..10 {
        offered(world.ask(
            &answerer,
            &server(&format!("u@h{n}:22")),
            offer(SOURCE, None),
        ));
    }
    // The `app.json` and its signature, once.
    assert_eq!(world.net.asked().len(), 2, "{:?}", world.net.asked());
}

#[test]
fn a_machine_that_makes_the_app_go_out_too_often_is_refused_and_another_is_not() {
    let world = World::new("limit");
    let author = Author::new(1);
    let sources: Vec<String> = (0..4)
        .map(|n| format!("https://apps.example.test/p{n}/app.json"))
        .collect();
    for (n, source) in sources.iter().enumerate() {
        Release::new(&format!("p{n}"), "1.0.0").publish(&world.net, source, &author);
    }
    let answerer = world.limited(3);

    for source in &sources[..3] {
        offered(world.ask(&answerer, &server("busy"), offer(source, None)));
    }
    let refusal = failed(world.ask(&answerer, &server("busy"), offer(&sources[3], None)));
    assert_eq!(refusal.kind, Kind::Refused);
    // What was read lately costs nothing, and another machine has its own count.
    offered(world.ask(&answerer, &server("busy"), offer(&sources[0], None)));
    offered(world.ask(&answerer, &server("quiet"), offer(&sources[3], None)));
}

#[test]
fn what_a_machine_has_and_a_key_that_stopped_a_program_are_remembered_once() {
    let world = World::new("state");
    let answerer = world.answerer();
    assert_eq!(answerer.machine("u@h:22"), None);
    let said = Package::State(State {
        target: TARGET.to_owned(),
        installed: vec!["textpen".to_owned()],
        not_here: vec!["pickpen".to_owned()],
    });
    assert_eq!(world.ask(&answerer, &server("u@h:22"), said), Reply::Done);
    let machine = answerer.machine("u@h:22").expect("remembered");
    assert_eq!(machine.installed, ["textpen"]);
    assert_eq!(machine.not_here, ["pickpen"]);

    let stopped = || {
        Package::KeyChanged(KeyChanged {
            name: "textpen".to_owned(),
            key: "ed25519:AAAA=".to_owned(),
        })
    };
    assert_eq!(
        world.ask(&answerer, &server("u@h:22"), stopped()),
        Reply::Done
    );
    assert_eq!(
        world.ask(&answerer, &server("u@h:22"), stopped()),
        Reply::Done
    );
    let changes = answerer.take_key_changes();
    assert_eq!(changes.len(), 1, "{changes:?}");
    assert_eq!(changes[0].name, "textpen");
    assert_eq!(changes[0].host, "dev-1");
    assert!(answerer.take_key_changes().is_empty());
    // Every start of the machine's watch says it again: it was told of, and is not told again,
    // not even after the first telling was taken.
    assert_eq!(
        world.ask(&answerer, &server("u@h:22"), stopped()),
        Reply::Done
    );
    assert!(answerer.take_key_changes().is_empty());
    // Another key is another change; this computer's own are for the Apps page; a name that is
    // not a program's is not told of.
    let other_key = Package::KeyChanged(KeyChanged {
        name: "textpen".to_owned(),
        key: "ed25519:BBBB=".to_owned(),
    });
    world.ask(&answerer, &server("u@h:22"), other_key);
    world.ask(&answerer, &this_computer(), stopped());
    world.ask(
        &answerer,
        &server("u@h:22"),
        Package::KeyChanged(KeyChanged {
            name: "not a name\nCall 555".to_owned(),
            key: "ed25519:CCCC=".to_owned(),
        }),
    );
    let more = answerer.take_key_changes();
    assert_eq!(more.len(), 1, "{more:?}");
    assert_eq!(more[0].key, "ed25519:BBBB=");
    let (title, body) = changes[0].notice();
    assert_eq!(title, "textpen stopped updating on dev-1");
    assert!(
        body.contains("bateri package update textpen on dev-1"),
        "{body}"
    );
}
