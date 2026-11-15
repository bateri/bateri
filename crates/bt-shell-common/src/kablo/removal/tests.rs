use std::cell::RefCell;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::*;
use crate::kablo::install::{Reply, run_command};
use crate::settings::TempRoot;
use crate::upload::sq;

const KEY: &str = "ada@rasp:22";

/// A kablo that says its version (not logged), lists what is in `$HOME/listing` and says what else
/// it was asked in `$HOME/calls`.
const KABLO: &str = "#!/bin/sh\n\
    [ \"$1\" = version ] && { echo \"version=0.2.0 wire_min=1 wire_max=2\"; exit 0; }\n\
    echo \"$*\" >> \"$HOME/calls\"\n\
    case \"$1\" in list) cat \"$HOME/listing\";; esac\nexit 0\n";

fn terminal(name: &str, state: &str) -> String {
    format!(
        "{{\"id\":\"{name}\",\"state\":\"{state}\",\"clients\":0,\
         \"version\":\"0.2.0\",\"warden_version\":\"0.2.0\"}}"
    )
}

/// `sh` in a directory that stands for the server's home.
struct At(PathBuf);

impl Channel for At {
    fn run(&self, script: &str, _: &[u8]) -> io::Result<Reply> {
        let dir = sq(&self.0.to_string_lossy());
        run_command(
            &[
                "sh".into(),
                "-c".into(),
                format!("cd {dir} && HOME={dir} sh -c {}", sq(script)),
            ],
            &[],
            Duration::from_secs(20),
        )
    }
}

/// A channel that answers from a table, in order.
struct Canned(RefCell<Vec<Reply>>);

impl Channel for Canned {
    fn run(&self, _: &str, _: &[u8]) -> io::Result<Reply> {
        let mut replies = self.0.borrow_mut();
        assert!(!replies.is_empty(), "more was asked than the table has");
        Ok(replies.remove(0))
    }
}

fn reply(status: i32, out: &str, err: &str) -> Reply {
    Reply {
        status: Some(status),
        out: out.to_owned(),
        err: err.to_owned(),
    }
}

/// The server, as the application's world would answer for it.
struct Server {
    key: Option<String>,
    reach: RefCell<Option<Result<Box<dyn Channel>, Reach>>>,
    answer: bool,
    /// What writing the settings says.
    entry: Result<(), String>,
    log: RefCell<Vec<String>>,
}

impl Server {
    fn new(channel: Result<Box<dyn Channel>, Reach>, answer: bool) -> Server {
        Server {
            key: Some(KEY.to_owned()),
            reach: RefCell::new(Some(channel)),
            answer,
            entry: Ok(()),
            log: RefCell::new(Vec::new()),
        }
    }

    fn log(&self) -> Vec<String> {
        self.log.borrow().clone()
    }
}

impl World for Server {
    fn key(&self) -> Option<String> {
        self.key.clone()
    }

    fn channel(&self) -> Result<Box<dyn Channel>, Reach> {
        self.log.borrow_mut().push("channel".to_owned());
        self.reach.borrow_mut().take().expect("asked for once")
    }

    fn version(&self) -> Option<String> {
        Some("0.2.0".to_owned())
    }

    fn confirm(&self, terminals: usize) -> bool {
        self.log.borrow_mut().push(format!("confirm {terminals}"));
        self.answer
    }

    fn removing(&self, key: &str) {
        self.log.borrow_mut().push(format!("removing {key}"));
    }

    fn removal_failed(&self, key: &str) {
        self.log.borrow_mut().push(format!("failed {key}"));
    }

    fn account_gone(&self, key: &str) {
        self.log.borrow_mut().push(format!("gone {key}"));
    }

    fn entry_off(&self) -> Result<(), String> {
        self.log.borrow_mut().push("entry off".to_owned());
        self.entry.clone()
    }
}

/// The state file with the server bateri has put its files on, and another one.
fn state(root: &Path) -> PathBuf {
    let path = root.join("remote-hosts");
    for (fact, key) in [
        (Fact::Touched, KEY),
        (Fact::Posix, KEY),
        (Fact::Touched, "ada@pi:22"),
    ] {
        ssh_wrap::record(&path, fact, key, 1, ssh_wrap::LOCK_PATIENCE).expect("a row");
    }
    path
}

/// A home with bateri's folders on it, and two terminals, one of them over.
fn home(root: &Path) -> PathBuf {
    let home = root.join("home");
    let listing = format!(
        "[\n{},\n{}\n]\n",
        terminal("p1", "running"),
        terminal("p2", "ended")
    );
    for (path, body) in [
        (".local/share/bateri/kablo/0.2.0/kablo", KABLO),
        (".local/share/bateri/shell/boot.sh", "boot"),
        (".local/share/kablo/0.1.0/kablo", KABLO),
        (".config/keep", "keep"),
        ("listing", listing.as_str()),
    ] {
        let file = home.join(path);
        std::fs::create_dir_all(file.parent().expect("a folder")).expect("folder");
        std::fs::write(&file, body).expect("file");
        if body == KABLO {
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755))
                .expect("runnable");
        }
    }
    home
}

fn calls(home: &Path) -> String {
    std::fs::read_to_string(home.join("calls")).unwrap_or_default()
}

#[test]
fn a_server_bateri_never_wrote_to_from_this_mac_is_not_asked_for_a_login_or_anything_else() {
    let root = TempRoot::new("removal-untouched");
    let path = state(&root.0);
    let mut world = Server::new(Err(Reach::Cancelled), true);
    world.key = Some("ada@elsewhere:22".to_owned());
    assert_eq!(
        run("elsewhere", &path, &world),
        Outcome::Refused("bateri has not put anything on elsewhere from this Mac.".to_owned())
    );
    assert!(world.log().is_empty(), "{:?}", world.log());
    // ssh that cannot say what the host is.
    world.key = None;
    assert!(matches!(
        run("elsewhere", &path, &world),
        Outcome::Refused(_)
    ));
    assert!(world.log().is_empty());
}

#[test]
fn a_login_that_is_not_given_says_nothing_and_one_that_fails_says_why_and_nothing_is_forgotten() {
    let root = TempRoot::new("removal-reach");
    let path = state(&root.0);
    let cancelled = Server::new(Err(Reach::Cancelled), true);
    assert_eq!(run("rasp", &path, &cancelled), Outcome::Cancelled);
    let failed = Server::new(Err(Reach::Failed("Connection refused".to_owned())), true);
    assert_eq!(
        run("rasp", &path, &failed),
        Outcome::Refused("bateri could not reach rasp: Connection refused".to_owned())
    );
    assert_eq!(failed.log(), ["channel"], "nothing was asked of the person");
    assert!(ssh_wrap::load(&path).knows(Fact::Touched, KEY));
}

#[test]
fn a_machine_whose_terminals_cannot_be_listed_has_nothing_removed_and_the_person_is_not_asked() {
    let root = TempRoot::new("removal-unlisted");
    let path = state(&root.0);
    let channel = Canned(RefCell::new(vec![reply(
        1,
        "",
        "kablo: no socket directory",
    )]));
    let world = Server::new(Ok(Box::new(channel)), true);
    let Outcome::Refused(why) = run("rasp", &path, &world) else {
        panic!("refused");
    };
    assert!(
        why.contains("could not list the terminals on rasp"),
        "{why}"
    );
    assert_eq!(world.log(), ["channel"]);
    assert!(ssh_wrap::load(&path).knows(Fact::Touched, KEY));
}

#[test]
fn a_removal_that_fails_after_the_question_keeps_what_it_knows_so_that_it_can_be_made_again() {
    let root = TempRoot::new("removal-failed");
    let path = state(&root.0);
    let channel = Canned(RefCell::new(vec![
        reply(0, ".local/share/bateri/kablo/0.2.0\n---\n[]\n", ""),
        reply(1, "", "rm: cannot remove: Device busy\n"),
    ]));
    let world = Server::new(Ok(Box::new(channel)), true);
    let Outcome::Refused(why) = run("rasp", &path, &world) else {
        panic!("refused");
    };
    assert!(
        why.contains("Device busy") && why.contains("run again"),
        "{why}"
    );
    assert_eq!(
        world.log(),
        [
            "channel",
            "confirm 0",
            "removing ada@rasp:22",
            "failed ada@rasp:22"
        ],
        "the service is held back for the removal and let go of it again; nothing is forgotten"
    );
    assert!(ssh_wrap::load(&path).knows(Fact::Touched, KEY));
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_no_leaves_the_server_exactly_as_it_was() {
    let root = TempRoot::new("removal-declined");
    let path = state(&root.0);
    let home = home(&root.0);
    let world = Server::new(Ok(Box::new(At(home.clone()))), false);
    assert_eq!(run("rasp", &path, &world), Outcome::Declined);
    assert_eq!(
        world.log(),
        ["channel", "confirm 1"],
        "the terminal that is over is not counted"
    );
    assert!(home.join(".local/share/bateri/shell/boot.sh").exists());
    assert!(home.join(".local/share/kablo").exists());
    assert!(!calls(&home).contains("end"), "{}", calls(&home));
    assert!(ssh_wrap::load(&path).knows(Fact::Touched, KEY));
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_yes_ends_the_terminals_takes_the_folders_off_and_forgets_the_server_and_only_it() {
    let root = TempRoot::new("removal-done");
    let path = state(&root.0);
    let home = home(&root.0);
    let world = Server::new(Ok(Box::new(At(home.clone()))), true);
    assert_eq!(
        run("rasp", &path, &world),
        Outcome::Removed {
            terminals: 1,
            entry_error: None
        }
    );
    assert!(calls(&home).contains("end p1"), "{}", calls(&home));
    assert!(!calls(&home).contains("end p2"), "it was over already");
    assert!(!home.join(".local/share/bateri").exists());
    assert!(!home.join(".local/share/kablo").exists());
    assert!(home.join(".config/keep").exists());
    // After: this Mac does not believe it has put anything there, the other server stays so, and
    // the service and the settings are told last.
    let state = ssh_wrap::load(&path);
    assert!(!state.knows(Fact::Touched, KEY));
    assert!(!state.knows(Fact::Posix, KEY));
    assert!(state.knows(Fact::Touched, "ada@pi:22"));
    assert_eq!(
        world.log(),
        [
            "channel",
            "confirm 1",
            "removing ada@rasp:22",
            "gone ada@rasp:22",
            "entry off"
        ]
    );
}

#[test]
fn a_settings_entry_that_could_not_be_written_is_said_and_the_server_is_clean_all_the_same() {
    let root = TempRoot::new("removal-entry");
    let path = state(&root.0);
    let channel = Canned(RefCell::new(vec![
        reply(0, ".local/share/bateri/kablo/0.2.0\n---\n[]\n", ""),
        reply(0, "", ""),
    ]));
    let mut world = Server::new(Ok(Box::new(channel)), true);
    world.entry = Err("settings.toml is not available".to_owned());
    assert_eq!(
        run("rasp", &path, &world),
        Outcome::Removed {
            terminals: 0,
            entry_error: Some("settings.toml is not available".to_owned())
        }
    );
    // What this Mac believed of the server went, and the service let go of the account.
    assert!(!ssh_wrap::load(&path).knows(Fact::Touched, KEY));
    assert_eq!(
        world.log(),
        [
            "channel",
            "confirm 0",
            "removing ada@rasp:22",
            "gone ada@rasp:22",
            "entry off"
        ]
    );
}

#[test]
fn the_entry_turns_a_host_that_every_server_follows_off_for_that_host_alone() {
    use bt_core::{RemoteIntegration, Settings};
    let text = "[remote]\nintegration = \"auto\"\n";
    let edited = Settings::with_edit(text, &entry_off_edit("deploy@rasp")).expect("written");
    let settings = Settings::parse(&edited).expect("parses").settings;
    assert_eq!(
        settings.integration_for("deploy@rasp"),
        RemoteIntegration::Off
    );
    assert_eq!(settings.integration_for("other"), RemoteIntegration::Auto);
    // A host that was a "yes" in its own entry becomes a "no" there.
    let text =
        "[remote]\nintegration = \"ask\"\nhosts = [{ host = \"rasp\", integration = true }]\n";
    let edited = Settings::with_edit(text, &entry_off_edit("rasp")).expect("written");
    let settings = Settings::parse(&edited).expect("parses").settings;
    assert_eq!(settings.integration_for("rasp"), RemoteIntegration::Off);
}
