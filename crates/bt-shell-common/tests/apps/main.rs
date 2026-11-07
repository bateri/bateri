//! The programs bateri follows (`bt_shell_common::apps`): a source's `app.json` read, trusted,
//! chosen from, downloaded and kept, on a temporary apps folder and a made-up network — and, for
//! the `file:` sources, on a real `curl`.
//!
//! One signed release was made once with pencere's `pencere dist` (`fixture/`, see README.md): it is
//! the proof that what the tool writes is what bateri reads. The rest are signed here, with keys
//! the tests invent, because they have to say what no real program says (a release that needs more
//! than this bateri has, another author's key).

mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime};

use bt_core::pencere::apps;
use bt_core::pencere::{SUPPORTED_VERSIONS, offered_views};
use bt_shell_common::apps::{
    Config, Curl, Distributor, Failure, Fetch, State, Status, Tracker, check, fetch_target, store,
};
use bt_shell_common::kablo::local::LocalChannel;
use support::{Author, Net, OTHER_TARGET, Release, TARGET, Temp, v};

const SOURCE: &str = "https://apps.example.test/demo/app.json";

/// A made-up network, an apps folder, and the settings of a check.
struct World {
    net: Arc<Net>,
    root: Temp,
}

impl World {
    fn new(label: &str) -> Self {
        Self {
            net: Arc::new(Net::default()),
            root: Temp::new(label),
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

    fn check(&self, sources: &[&str]) -> Vec<Status> {
        self.check_as(self.config(), sources, true)
    }

    fn check_as(&self, config: Config, sources: &[&str], authoritative: bool) -> Vec<Status> {
        let sources: Vec<String> = sources.iter().map(|source| (*source).to_owned()).collect();
        check(
            &config,
            &sources,
            authoritative,
            self.net.as_ref(),
            SystemTime::now(),
        )
    }

    fn one(&self, source: &str) -> Status {
        let mut found = self.check(&[source]);
        assert_eq!(found.len(), 1);
        found.remove(0)
    }

    fn approve(&self, name: &str, author: &Author) {
        store::approve(self.root.path(), name, &author.key()).expect("approval written");
    }

    fn versions(&self, name: &str) -> Vec<String> {
        store::landed(self.root.path(), name)
            .iter()
            .map(|release| release.version.to_string())
            .collect()
    }
}

fn ready(version: &str) -> State {
    State::Ready {
        version: v(version),
        newer: None,
    }
}

fn error(status: &Status) -> &Failure {
    match &status.state {
        State::Error(failure) => failure,
        other => panic!("an error was expected, found {other:?}"),
    }
}

// ---- the release the tool made ------------------------------------------------------------------

const FIXTURE_BASE: &str = "https://apps.example.test/demo/1.0.0/";

/// What `pencere dist` wrote, put where its `app.json` says the files are.
fn fixture(net: &Net) -> String {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/apps/fixture");
    for entry in fs::read_dir(&dir).expect("the fixture folder") {
        let entry = entry.expect("an entry");
        let name = entry.file_name().into_string().expect("a name");
        if name == "throwaway.key" || name == "SHA256SUMS" {
            continue;
        }
        net.put(
            &format!("{FIXTURE_BASE}{name}"),
            fs::read(entry.path()).expect("a file"),
        );
    }
    format!("{FIXTURE_BASE}app.json")
}

fn fixture_key() -> String {
    let bytes = fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/apps/fixture/app.json"))
        .expect("the fixture's app.json");
    apps::parse(&bytes).expect("it parses").key
}

#[test]
fn what_the_tool_wrote_waits_for_its_key_and_then_lands() {
    let world = World::new("fixture");
    let source = fixture(&world.net);
    let key = fixture_key();

    // Nobody approved the key: the `app.json` was read, nothing else was fetched.
    let status = world.one(&source);
    assert_eq!(status.name.as_deref(), Some("demo"));
    assert_eq!(
        status.state,
        State::Awaiting {
            version: v("1.0.0"),
            key: key.clone(),
            changed: false,
        }
    );
    assert!(
        world
            .net
            .asked()
            .iter()
            .all(|address| address.ends_with("app.json") || address.ends_with("app.json.sig")),
        "{:?}",
        world.net.asked()
    );
    assert!(world.versions("demo").is_empty());

    // The person approves it: the next check lands the file of this machine's target.
    store::approve(world.root.path(), "demo", &key).expect("approval written");
    let status = world.one(&source);
    assert_eq!(status.state, ready("1.0.0"));
    assert_eq!(status.problem, None);
    let folder = world.root.path().join("demo/1.0.0");
    let fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/apps/fixture");
    assert_eq!(
        fs::read(folder.join(TARGET)).expect("the program"),
        fs::read(fixture_dir.join(format!("demo-1.0.0-{TARGET}"))).expect("the fixture's")
    );
    assert_eq!(
        fs::metadata(folder.join(TARGET))
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
    assert_eq!(
        fs::read(folder.join("app.json")).expect("app.json"),
        fs::read(fixture_dir.join("app.json")).expect("the fixture's")
    );
    assert_eq!(
        fs::read(folder.join("app.json.sig")).expect("signature"),
        fs::read(fixture_dir.join("app.json.sig")).expect("the fixture's")
    );
    // Only this machine's file came down: another target's is for a machine that needs it.
    assert!(!folder.join(OTHER_TARGET).exists());
    assert_eq!(
        store::followed(world.root.path(), "demo").as_deref(),
        Some(source.as_str())
    );

    // Checked again, nothing is downloaded again.
    let program = format!("{FIXTURE_BASE}demo-1.0.0-{TARGET}");
    let before = world.net.asked().iter().filter(|a| **a == program).count();
    assert_eq!(world.one(&source).state, ready("1.0.0"));
    let after = world.net.asked().iter().filter(|a| **a == program).count();
    assert_eq!((before, after), (1, 1));
}

#[test]
fn the_key_that_comes_in_the_application_needs_no_approval() {
    let world = World::new("in-app");
    let source = fixture(&world.net);
    let mut config = world.config();
    config.in_app_key = Some(fixture_key());
    let found = world.check_as(config, &[&source], true);
    assert_eq!(found[0].state, ready("1.0.0"));
    // The key is kept, so a release another key signs later stops and asks.
    assert_eq!(
        store::approved_key(world.root.path(), "demo"),
        Some(fixture_key())
    );
}

// ---- trust ---------------------------------------------------------------------------------------

#[test]
fn a_release_signed_with_another_key_than_the_approved_one_stops() {
    let world = World::new("key");
    let (author, other) = (Author::new(1), Author::new(2));
    Release::new("demo", "1.0.0").publish(&world.net, SOURCE, &author);
    world.approve("demo", &author);
    assert_eq!(world.one(SOURCE).state, ready("1.0.0"));

    Release::new("demo", "1.1.0").publish(&world.net, SOURCE, &other);
    let status = world.one(SOURCE);
    assert_eq!(
        status.state,
        State::Awaiting {
            version: v("1.1.0"),
            key: other.key(),
            changed: true,
        }
    );
    // What was there stays, and so does the key that was approved.
    assert_eq!(world.versions("demo"), ["1.0.0"]);
    assert_eq!(
        store::approved_key(world.root.path(), "demo"),
        Some(author.key())
    );
}

#[test]
fn an_update_of_the_same_key_lands_and_keeps_the_release_before_it() {
    let world = World::new("update");
    let author = Author::new(1);
    Release::new("demo", "1.0.0").publish(&world.net, SOURCE, &author);
    world.approve("demo", &author);
    assert_eq!(world.one(SOURCE).state, ready("1.0.0"));

    Release::new("demo", "1.1.0").publish(&world.net, SOURCE, &author);
    assert_eq!(world.one(SOURCE).state, ready("1.1.0"));
    assert_eq!(world.versions("demo"), ["1.0.0", "1.1.0"]);
    let program =
        fs::read_to_string(world.root.path().join("demo/1.1.0").join(TARGET)).expect("the program");
    assert!(program.contains("demo 1.1.0"), "{program}");
}

// ---- selection -----------------------------------------------------------------------------------

#[test]
fn the_latest_release_this_bateri_cannot_open_gives_way_to_the_one_listed_before_it() {
    let world = World::new("previous");
    let author = Author::new(1);
    world.approve("demo", &author);
    let old_manifest = "https://apps.example.test/demo/1.0.0/app.json";
    let old = Release::new("demo", "1.0.0");
    old.publish(&world.net, old_manifest, &author);
    // 2.0.0 shows a kind of view this bateri does not open.
    let new = Release::new("demo", "2.0.0")
        .views(r#"[{"kind":"lua","versions":["9.0"]}]"#)
        .previous(&format!("[{}]", old.as_previous(old_manifest)));
    new.publish(&world.net, SOURCE, &author);

    let status = world.one(SOURCE);
    assert_eq!(
        status.state,
        State::Ready {
            version: v("1.0.0"),
            newer: Some(v("2.0.0")),
        }
    );
    assert_eq!(world.versions("demo"), ["1.0.0"]);
}

#[test]
fn a_release_nothing_opens_waits_for_a_bateri_that_does() {
    let world = World::new("waits");
    let author = Author::new(1);
    world.approve("demo", &author);
    Release::new("demo", "2.0.0")
        .views(r#"[{"kind":"lua","versions":["9.0"]}]"#)
        .publish(&world.net, SOURCE, &author);
    let status = world.one(SOURCE);
    assert_eq!(
        status.state,
        State::WaitsForUpdate {
            version: v("2.0.0")
        }
    );
    assert!(world.versions("demo").is_empty());

    // A protocol this bateri does not speak is the same.
    let fixed = Release {
        protocol: "[7]".to_owned(),
        ..Release::new("demo", "3.0.0")
    };
    fixed.publish(&world.net, SOURCE, &author);
    assert_eq!(
        world.one(SOURCE).state,
        State::WaitsForUpdate {
            version: v("3.0.0")
        }
    );
}

#[test]
fn an_earlier_release_of_another_key_is_not_taken() {
    let world = World::new("earlier-key");
    let (author, other) = (Author::new(1), Author::new(2));
    world.approve("demo", &author);
    let old_manifest = "https://apps.example.test/demo/1.0.0/app.json";
    let old = Release::new("demo", "1.0.0");
    old.publish(&world.net, old_manifest, &other);
    Release::new("demo", "2.0.0")
        .views(r#"[{"kind":"lua","versions":["9.0"]}]"#)
        .previous(&format!("[{}]", old.as_previous(old_manifest)))
        .publish(&world.net, SOURCE, &author);
    let status = world.one(SOURCE);
    assert_eq!(
        error(&status),
        &Failure::Mismatch("an earlier release is signed with another key")
    );
    assert!(world.versions("demo").is_empty());
}

#[test]
fn a_release_without_a_file_for_this_target_has_nothing_to_run_here() {
    let world = World::new("not-here");
    let author = Author::new(1);
    world.approve("demo", &author);
    Release::new("demo", "1.0.0")
        .targets(&[OTHER_TARGET])
        .publish(&world.net, SOURCE, &author);
    assert_eq!(
        world.one(SOURCE).state,
        State::NotHere {
            version: v("1.0.0")
        }
    );
    assert!(world.versions("demo").is_empty());
}

// ---- names ---------------------------------------------------------------------------------------

#[test]
fn the_names_of_bateri_kablo_and_pencere_are_not_a_programs() {
    let world = World::new("reserved");
    let author = Author::new(1);
    for name in ["bateri", "kablo", "pencere"] {
        let address = format!("https://apps.example.test/{name}/app.json");
        world.approve(name, &author);
        Release::new(name, "1.0.0").publish(&world.net, &address, &author);
        let status = world.one(&address);
        assert_eq!(error(&status), &Failure::Reserved, "{name}");
        assert!(world.versions(name).is_empty());
    }
}

#[test]
fn a_name_another_source_has_is_not_taken_a_second_time() {
    let world = World::new("taken");
    let author = Author::new(1);
    world.approve("demo", &author);
    let mirror = "https://mirror.example.test/demo/app.json";
    Release::new("demo", "1.0.0").publish(&world.net, SOURCE, &author);
    Release::new("demo", "1.0.0").publish(&world.net, mirror, &author);

    // In one check, the first of the list has it.
    let found = world.check(&[SOURCE, mirror]);
    assert_eq!(found[0].state, ready("1.0.0"));
    assert_eq!(error(&found[1]), &Failure::NameTaken);

    // And in a later one, whichever is listed first: the folder says who follows the name.
    let found = world.check(&[mirror, SOURCE]);
    assert_eq!(error(&found[0]), &Failure::NameTaken);
    assert_eq!(found[1].state, ready("1.0.0"));
}

// ---- what the list says --------------------------------------------------------------------------

#[test]
fn a_program_that_leaves_the_list_is_deleted_and_its_name_remembered() {
    let world = World::new("leaves");
    let author = Author::new(1);
    world.approve("demo", &author);
    Release::new("demo", "1.0.0").publish(&world.net, SOURCE, &author);
    assert_eq!(world.one(SOURCE).state, ready("1.0.0"));
    assert!(world.root.path().join("demo/1.0.0").join(TARGET).exists());

    assert!(world.check(&[]).is_empty());
    assert!(!world.root.path().join("demo").exists());
    let removed = store::removed(world.root.path(), SystemTime::now());
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0].name, "demo");

    // Added again, it is followed again — and it asks for its key again, which went with it.
    assert!(matches!(world.one(SOURCE).state, State::Awaiting { .. }));
    world.approve("demo", &author);
    assert_eq!(world.one(SOURCE).state, ready("1.0.0"));
    assert!(store::removed(world.root.path(), SystemTime::now()).is_empty());
}

#[test]
fn a_list_that_may_not_be_the_persons_deletes_nothing() {
    let world = World::new("not-authoritative");
    let author = Author::new(1);
    world.approve("demo", &author);
    Release::new("demo", "1.0.0").publish(&world.net, SOURCE, &author);
    assert_eq!(world.one(SOURCE).state, ready("1.0.0"));
    // The settings file could not be read: the list is the defaults.
    assert!(world.check_as(world.config(), &[], false).is_empty());
    assert_eq!(world.versions("demo"), ["1.0.0"]);
    assert!(store::removed(world.root.path(), SystemTime::now()).is_empty());
}

#[test]
fn the_names_taken_off_the_list_are_remembered_for_thirty_days() {
    let world = World::new("thirty");
    let root = world.root.path();
    let now = SystemTime::now();
    for name in ["old", "new"] {
        fs::create_dir_all(root.join(name)).expect("a folder");
    }
    store::remove(root, "old", now - Duration::from_secs(31 * 24 * 60 * 60)).expect("removed");
    store::remove(root, "new", now - Duration::from_secs(29 * 24 * 60 * 60)).expect("removed");
    let names: Vec<String> = store::removed(root, now)
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    assert_eq!(names, ["new"]);
}

#[test]
fn an_entry_that_names_no_address_is_an_error_of_its_own() {
    let world = World::new("source");
    let found = world.check(&["not a source"]);
    assert_eq!(error(&found[0]), &Failure::Source);
}

// ---- what can go wrong ---------------------------------------------------------------------------

#[test]
fn an_app_json_that_was_changed_after_signing_is_refused() {
    let world = World::new("tampered");
    let author = Author::new(1);
    world.approve("demo", &author);
    let json = Release::new("demo", "1.0.0").publish(&world.net, SOURCE, &author);
    let altered = String::from_utf8(json)
        .expect("text")
        .replace("1.0.0", "1.0.1");
    world.net.put(SOURCE, altered);
    let status = world.one(SOURCE);
    assert_eq!(error(&status), &Failure::Manifest(apps::Error::Signature));
    assert_eq!(
        error(&status).to_string(),
        "app.json: the signature does not hold"
    );
    assert!(world.versions("demo").is_empty());
}

#[test]
fn a_file_that_is_not_the_one_signed_lands_nowhere() {
    let world = World::new("digest");
    let author = Author::new(1);
    world.approve("demo", &author);
    let release = Release::new("demo", "1.0.0");
    release.publish(&world.net, SOURCE, &author);
    world.net.put(
        &release.file_address(SOURCE, TARGET),
        "#!/bin/sh\necho evil\n",
    );
    let status = world.one(SOURCE);
    assert_eq!(error(&status), &Failure::Digest);
    assert!(world.versions("demo").is_empty());
    // Nothing of the attempt is left in the program's folder, not even its temporary one.
    let left: Vec<String> = fs::read_dir(world.root.path().join("demo"))
        .expect("the folder")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(left, ["key"]);
}

#[test]
fn a_network_that_fails_leaves_what_landed() {
    let world = World::new("offline");
    let author = Author::new(1);
    world.approve("demo", &author);
    // Nothing landed yet: the fetch error is the state.
    let status = world.one(SOURCE);
    assert!(matches!(error(&status), Failure::Fetch(_)));

    Release::new("demo", "1.0.0").publish(&world.net, SOURCE, &author);
    assert_eq!(world.one(SOURCE).state, ready("1.0.0"));
    world.net.take_down(SOURCE);
    let status = world.one(SOURCE);
    assert_eq!(status.state, ready("1.0.0"));
    assert_eq!(status.name.as_deref(), Some("demo"));
    assert!(matches!(status.problem, Some(Failure::Fetch(_))));
    assert_eq!(world.versions("demo"), ["1.0.0"]);
}

#[test]
fn a_source_on_the_network_may_not_send_the_reader_to_a_file_on_this_mac() {
    let world = World::new("scheme");
    let author = Author::new(1);
    world.approve("demo", &author);
    Release::new("demo", "1.0.0")
        .files_at("file:///etc/")
        .publish(&world.net, SOURCE, &author);
    assert_eq!(error(&world.one(SOURCE)), &Failure::Unreachable);
    assert!(
        world
            .net
            .asked()
            .iter()
            .all(|address| !address.starts_with("file:")),
        "{:?}",
        world.net.asked()
    );
}

#[test]
fn a_release_that_is_neither_the_newest_two_nor_recent_goes() {
    let world = World::new("prune");
    let author = Author::new(1);
    world.approve("demo", &author);
    let root = world.root.path();
    let publish = |version: &str| {
        Release::new("demo", version).publish(&world.net, SOURCE, &author);
        assert_eq!(world.one(SOURCE).state, ready(version));
    };
    let age = |version: &str| {
        let manifest = root.join("demo").join(version).join("app.json");
        fs::File::options()
            .write(true)
            .open(manifest)
            .expect("app.json")
            .set_modified(SystemTime::now() - Duration::from_secs(40 * 24 * 60 * 60))
            .expect("a time");
    };
    publish("1.0.0");
    publish("1.1.0");
    publish("1.2.0");
    assert_eq!(world.versions("demo"), ["1.0.0", "1.1.0", "1.2.0"]);
    age("1.0.0");
    age("1.2.0");
    publish("1.3.0");
    // 1.0.0 is old and not one of the newest two; 1.1.0 is recent; 1.2.0 is old but the second.
    assert_eq!(world.versions("demo"), ["1.1.0", "1.2.0", "1.3.0"]);
}

// ---- the real `curl` -----------------------------------------------------------------------------

#[test]
fn curl_reads_a_file_source_and_lands_its_program() {
    let world = World::new("curl");
    let served = Temp::new("served");
    let author = Author::new(1);
    world.approve("demo", &author);
    let source = format!("file://{}/demo/app.json", served.path().display());
    Release::new("demo", "1.0.0").publish(&world.net, &source, &author);
    world.net.write_files();

    let sources = [source.clone()];
    let found = check(&world.config(), &sources, true, &Curl, SystemTime::now());
    assert_eq!(found[0].state, ready("1.0.0"), "{:?}", found[0]);
    let program =
        fs::read_to_string(world.root.path().join("demo/1.0.0").join(TARGET)).expect("the program");
    assert!(program.contains("demo 1.0.0"), "{program}");

    // A file that is not there is an error with curl's words.
    fs::remove_file(served.path().join("demo/app.json")).expect("removed");
    let found = check(&world.config(), &sources, true, &Curl, SystemTime::now());
    assert_eq!(found[0].state, ready("1.0.0"));
    assert!(
        matches!(&found[0].problem, Some(Failure::Fetch(why)) if why.contains("curl")),
        "{:?}",
        found[0].problem
    );
}

#[test]
fn curl_does_not_read_more_than_the_limit() {
    let served = Temp::new("limit");
    let path = served.path().join("big");
    fs::write(&path, vec![b'x'; 5000]).expect("a file");
    let address = format!("file://{}", path.display());
    let curl = Curl;
    assert_eq!(curl.get(&address, 5000).expect("read").len(), 5000);
    assert!(curl.get(&address, 4999).is_err());
    let part = served.path().join("part");
    assert!(curl.download(&address, 4999, &part).is_err());
    assert!(!part.exists());
    curl.download(&address, 5000, &part).expect("downloaded");
    assert_eq!(fs::metadata(&part).expect("metadata").len(), 5000);
    // Only the two protocols a source has are spoken, and a call on the network does not read a
    // file on this Mac.
    assert!(curl.get("ftp://example.invalid/x", 10).is_err());
    assert!(curl.get(&address.replace("file:", "https:"), 10).is_err());
}

// ---- the worker ----------------------------------------------------------------------------------

fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < until, "gave up waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn the_worker_checks_at_start_and_again_when_the_list_changes() {
    let world = World::new("worker");
    let author = Author::new(1);
    Release::new("demo", "1.0.0").publish(&world.net, SOURCE, &author);
    let checks = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&checks);
    let tracker = Tracker::start_with(
        world.config(),
        Arc::clone(&world.net) as Arc<dyn Fetch>,
        vec![SOURCE.to_owned()],
        true,
        Box::new(move || {
            counted.fetch_add(1, Ordering::SeqCst);
        }),
    )
    .expect("the worker starts");
    assert!(tracker.statuses().is_empty());

    // What the application does with a landed program runs after every check, on the worker.
    wait_for("the first check", || checks.load(Ordering::SeqCst) == 1);
    assert!(!tracker.statuses().is_empty());
    assert!(matches!(
        tracker.statuses()[0].state,
        State::Awaiting { changed: false, .. }
    ));

    // The same list is not a reason to check again; an approval is.
    world.approve("demo", &author);
    tracker.set_sources(vec![SOURCE.to_owned()], true);
    tracker.refresh();
    wait_for("the program", || {
        tracker.statuses().first().map(|s| s.state.clone()) == Some(ready("1.0.0"))
    });
    wait_for("the second check's follow-up", || {
        checks.load(Ordering::SeqCst) == 2
    });

    tracker.set_sources(Vec::new(), true);
    wait_for("the program to go", || {
        tracker.statuses().is_empty() && !world.root.path().join("demo").exists()
    });
}

// ---- the programs go to machines -----------------------------------------------------------------

/// The apps folder of a machine whose home is `home`, as the machine's shells find it.
fn machine_apps(home: &Path) -> std::path::PathBuf {
    home.join(".local/share/bateri/apps")
}

/// What the command `name` of a machine says when it is run: the shell's `PATH` is not at stake
/// here, the link is.
fn runs(home: &Path, name: &str) -> String {
    let output = std::process::Command::new(machine_apps(home).join("bin").join(name))
        .output()
        .expect("the command runs");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

#[test]
fn a_machine_of_another_kind_gets_its_own_file_and_follows_the_releases_and_the_removal() {
    let world = World::new("distribute");
    let author = Author::new(1);
    world.approve("demo", &author);
    Release::new("demo", "1.0.0").publish(&world.net, SOURCE, &author);
    assert_eq!(world.one(SOURCE).state, ready("1.0.0"));
    let folder = world.root.path().join("demo/1.0.0");
    assert!(
        !folder.join(OTHER_TARGET).exists(),
        "only this Mac's file came down"
    );

    let distributor = Distributor::new(
        world.root.path().to_owned(),
        Arc::clone(&world.net) as Arc<dyn Fetch>,
    );
    let server = Temp::new("server-home");
    let channel = LocalChannel::new(server.path().to_owned());
    let mac = Temp::new("mac-home");
    let this_mac = LocalChannel::new(mac.path().to_owned());
    let now = SystemTime::now();
    let later = |hours: u64| now + Duration::from_secs(hours * 60 * 60);

    // The server is of another kind: its file is fetched when it is wanted, and runs there.
    distributor.sync_unless_recent("rasp", OTHER_TARGET, &channel, now);
    let reports = distributor.reports();
    let summary = reports[0].result.as_ref().expect("the machine answered");
    assert_eq!(summary.shipped, [("demo".to_owned(), v("1.0.0"))]);
    assert_eq!(summary.problems, Vec::<String>::new());
    assert_eq!(runs(server.path(), "demo"), "demo 1.0.0");
    assert!(
        folder.join(OTHER_TARGET).is_file(),
        "fetched for the machine"
    );
    assert_eq!(
        fs::read(machine_apps(server.path()).join("demo/1.0.0/app.json")).expect("app.json"),
        fs::read(folder.join("app.json")).expect("the Mac's")
    );
    // This Mac's own account has the file that came down with the release: nothing more is asked.
    let asked = world.net.asked().len();
    distributor.sync_unless_recent("this Mac", TARGET, &this_mac, now);
    assert_eq!(runs(mac.path(), "demo"), "demo 1.0.0");
    assert_eq!(world.net.asked().len(), asked);

    // A newer release goes to both, and the one before stays on the machines.
    Release::new("demo", "1.1.0").publish(&world.net, SOURCE, &author);
    assert_eq!(world.one(SOURCE).state, ready("1.1.0"));
    distributor.sync_unless_recent("rasp", OTHER_TARGET, &channel, later(1));
    distributor.sync_unless_recent("this Mac", TARGET, &this_mac, later(1));
    for home in [server.path(), mac.path()] {
        assert_eq!(runs(home, "demo"), "demo 1.1.0");
        assert!(machine_apps(home).join("demo/1.0.0/demo").is_file());
    }

    // Taken off the list: the Mac deletes it, and the machines do at their next visit.
    store::remove(world.root.path(), "demo", later(2)).expect("removed");
    distributor.sync_unless_recent("rasp", OTHER_TARGET, &channel, later(2));
    let reports = distributor.reports();
    let summary = reports
        .iter()
        .find(|report| report.machine == "rasp")
        .and_then(|report| report.result.as_ref().ok())
        .expect("the machine answered");
    assert_eq!(summary.removed, ["demo"]);
    assert!(!machine_apps(server.path()).join("demo").exists());
    assert!(!machine_apps(server.path()).join("bin/demo").exists());
    // This Mac's own account loses it the same way.
    distributor.sync_unless_recent("this Mac", TARGET, &this_mac, later(2));
    assert!(!machine_apps(mac.path()).join("demo").exists());
}

#[test]
fn a_file_for_a_machine_is_fetched_only_from_a_release_that_still_holds() {
    let world = World::new("fetch-target");
    let author = Author::new(1);
    world.approve("demo", &author);
    let release = Release::new("demo", "1.0.0");
    release.publish(&world.net, SOURCE, &author);
    assert_eq!(world.one(SOURCE).state, ready("1.0.0"));
    let folder = world.root.path().join("demo/1.0.0");
    let ask = |target: &str| {
        fetch_target(
            world.root.path(),
            "demo",
            v("1.0.0"),
            target,
            world.net.as_ref(),
        )
    };

    // A target the release has no file for gives nothing, and asks nothing.
    let asked = world.net.asked().len();
    assert_eq!(ask("riscv64-unknown-linux-musl"), Ok(None));
    assert_eq!(world.net.asked().len(), asked);

    // What the network gives back for the file is checked against the `app.json`'s sum, and
    // nothing of it stays.
    let address = release.file_address(SOURCE, OTHER_TARGET);
    world.net.put(&address, "#!/bin/sh\necho something else\n");
    assert_eq!(ask(OTHER_TARGET), Err(Failure::Digest));
    assert!(!folder.join(OTHER_TARGET).exists());
    let leftovers: Vec<String> = fs::read_dir(&folder)
        .expect("the folder")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name.contains(".part"))
        .collect();
    assert_eq!(leftovers, Vec::<String>::new());

    // The right bytes arrive, and are kept; asked again, they are not fetched again.
    world.net.put(&address, release.body.as_bytes());
    assert_eq!(ask(OTHER_TARGET), Ok(Some(folder.join(OTHER_TARGET))));
    let asked = world.net.asked().len();
    assert_eq!(ask(OTHER_TARGET), Ok(Some(folder.join(OTHER_TARGET))));
    assert_eq!(world.net.asked().len(), asked);

    // An `app.json` on the disk that is not what the author signed is not a release.
    let mut manifest = fs::read(folder.join("app.json")).expect("app.json");
    manifest.extend_from_slice(b" ");
    fs::write(folder.join("app.json"), manifest).expect("tampered");
    fs::remove_file(folder.join(OTHER_TARGET)).expect("removed");
    assert!(matches!(ask(OTHER_TARGET), Err(Failure::Manifest(_))));
}
