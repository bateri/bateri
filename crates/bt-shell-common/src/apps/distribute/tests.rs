//! The scripts a machine is told, run for real by every `sh` the test machine has (a server's `sh`
//! may be dash or BusyBox's ash; `make linux-server` runs these in the container), in a folder
//! that stands for the machine's home directory; and what is decided from what the machine says.

use std::cell::RefCell;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use super::*;
use crate::apps::fetch::Curl;
use crate::kablo::install::run_command_in;

const TARGET: &str = "x86_64-unknown-linux-musl";

fn v(text: &str) -> Version {
    Version::parse(text).expect("a version")
}

/// A folder of the tests, deleted when it goes out of scope.
struct Temp(PathBuf);

impl Temp {
    fn new(label: &str) -> Self {
        static COUNT: AtomicU32 = AtomicU32::new(0);
        let n = COUNT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "bateri-distribute-{label}-{}-{n}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("a temporary folder");
        Self(path)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The shells a server may have as `sh` that this machine has.
fn shells() -> Vec<PathBuf> {
    let mut found = Vec::new();
    for candidate in [
        "/bin/sh",
        "/bin/dash",
        "/usr/bin/dash",
        "/bin/bash",
        "/usr/bin/bash",
    ] {
        let path = PathBuf::from(candidate);
        let real = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        if path.exists() && !found.iter().any(|(_, r): &(PathBuf, PathBuf)| *r == real) {
            found.push((path, real));
        }
    }
    found.into_iter().map(|(path, _)| path).collect()
}

/// A machine: a shell and the folder that is its home. Runs the scripts for real and keeps them.
struct Machine {
    shell: PathBuf,
    home: PathBuf,
    scripts: RefCell<Vec<String>>,
}

impl Machine {
    fn new(shell: &Path, home: &Path) -> Self {
        Self {
            shell: shell.to_owned(),
            home: home.to_owned(),
            scripts: RefCell::new(Vec::new()),
        }
    }

    fn calls(&self) -> usize {
        self.scripts.borrow().len()
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.home.join(ROOT).join(relative)
    }
}

impl Channel for Machine {
    fn run(&self, script: &str, input: &[u8]) -> std::io::Result<Reply> {
        self.scripts.borrow_mut().push(script.to_owned());
        run_command_in(
            &[
                self.shell.display().to_string(),
                "-c".to_owned(),
                script.to_owned(),
            ],
            input,
            Duration::from_secs(60),
            Some(&self.home),
        )
    }
}

/// This computer's apps folder with a release of `name` in it: the program for `TARGET`, which says
/// its version when run, and the `app.json`.
fn release(root: &Path, name: &str, version: &str) -> Want {
    let dir = root.join(name).join(version);
    fs::create_dir_all(&dir).expect("a release folder");
    fs::write(
        dir.join(TARGET),
        format!("#!/bin/sh\necho {name} {version}\n"),
    )
    .expect("a program");
    fs::write(
        dir.join("app.json"),
        format!("{{\"name\":\"{name}\",\"version\":\"{version}\"}}"),
    )
    .expect("an app.json");
    Want {
        name: name.to_owned(),
        version: v(version),
        dir,
    }
}

/// The program of a want, for the target the tests run as, with the sum its release signs.
fn program(want: &Want) -> Result<Option<Program>, String> {
    let path = want.dir.join(TARGET);
    let sha256 = Digest::of(&fs::read(&path).map_err(|e| e.to_string())?).to_string();
    Ok(Some(Program { path, sha256 }))
}

fn nothing(_: &Want) -> Result<Option<Program>, String> {
    Ok(None)
}

/// The folders of a release's temporary kind that a machine has under `name`.
fn parts(machine: &Machine, name: &str) -> Vec<String> {
    let Ok(entries) = fs::read_dir(machine.path(name)) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|entry| entry.contains(".part"))
        .collect()
}

fn visit_with(machine: &Machine, wanted: &[Want], gone: &[String]) -> Result<Summary, String> {
    visit(machine, wanted, gone, &program)
}

fn link(machine: &Machine, name: &str) -> Option<String> {
    fs::read_link(machine.path(&format!("bin/{name}")))
        .ok()
        .map(|target| target.display().to_string())
}

/// A folder `age_days` old, as `find -mtime` counts.
fn age(path: &Path, age_days: u64) {
    fs::File::open(path)
        .expect("a folder")
        .set_modified(SystemTime::now() - Duration::from_secs(age_days * 24 * 60 * 60 + 3600))
        .expect("a time");
}

#[test]
fn there_is_at_least_one_shell_to_try() {
    assert!(!shells().is_empty());
}

#[test]
fn a_first_visit_puts_the_program_there_and_points_the_link_at_it() {
    for shell in shells() {
        let (mac, home) = (Temp::new("mac"), Temp::new("home"));
        let want = release(&mac.0, "demo", "1.0.0");
        let machine = Machine::new(&shell, &home.0);
        let summary = visit_with(&machine, std::slice::from_ref(&want), &[]).expect("a visit");
        assert_eq!(
            summary.shipped,
            [("demo".to_owned(), v("1.0.0"))],
            "{shell:?}"
        );
        assert_eq!(summary.problems, Vec::<String>::new(), "{shell:?}");
        assert_eq!(
            link(&machine, "demo").as_deref(),
            Some("../demo/1.0.0/demo")
        );
        let program = machine.path("demo/1.0.0/demo");
        assert_eq!(
            fs::read(&program).expect("the program"),
            fs::read(want.dir.join(TARGET)).expect("the computer's")
        );
        assert_eq!(
            fs::metadata(&program)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
        assert_eq!(
            fs::read(machine.path("demo/1.0.0/app.json")).expect("app.json"),
            fs::read(want.dir.join("app.json")).expect("the computer's")
        );
        // The temporary folder is gone, and the command runs through the link.
        assert_eq!(parts(&machine, "demo"), Vec::<String>::new(), "{shell:?}");
        let out = std::process::Command::new(machine.path("bin/demo"))
            .output()
            .expect("the command runs");
        assert_eq!(String::from_utf8_lossy(&out.stdout), "demo 1.0.0\n");
    }
}

#[test]
fn the_same_release_is_not_sent_again() {
    for shell in shells() {
        let (mac, home) = (Temp::new("mac"), Temp::new("home"));
        let want = release(&mac.0, "demo", "1.0.0");
        let machine = Machine::new(&shell, &home.0);
        visit_with(&machine, std::slice::from_ref(&want), &[]).expect("a visit");
        let before = machine.calls();
        let summary = visit_with(&machine, std::slice::from_ref(&want), &[]).expect("a visit");
        assert_eq!(summary, Summary::default(), "{shell:?}");
        assert_eq!(machine.calls() - before, 1, "only the look: {shell:?}");
    }
}

#[test]
fn a_newer_release_switches_the_link_and_the_one_before_stays() {
    for shell in shells() {
        let (mac, home) = (Temp::new("mac"), Temp::new("home"));
        let machine = Machine::new(&shell, &home.0);
        visit_with(&machine, &[release(&mac.0, "demo", "1.0.0")], &[]).expect("a visit");
        let summary =
            visit_with(&machine, &[release(&mac.0, "demo", "1.1.0")], &[]).expect("a visit");
        assert_eq!(
            summary.shipped,
            [("demo".to_owned(), v("1.1.0"))],
            "{shell:?}"
        );
        assert_eq!(
            link(&machine, "demo").as_deref(),
            Some("../demo/1.1.0/demo")
        );
        assert!(machine.path("demo/1.0.0/demo").is_file());
        // Greater by number, not by text: 1.10.0 is after 1.9.0.
        visit_with(&machine, &[release(&mac.0, "demo", "1.9.0")], &[]).expect("a visit");
        visit_with(&machine, &[release(&mac.0, "demo", "1.10.0")], &[]).expect("a visit");
        assert_eq!(
            link(&machine, "demo").as_deref(),
            Some("../demo/1.10.0/demo")
        );
    }
}

#[test]
fn an_older_release_asked_for_leaves_the_link_where_it_is() {
    for shell in shells() {
        let (mac, home) = (Temp::new("mac"), Temp::new("home"));
        let machine = Machine::new(&shell, &home.0);
        visit_with(&machine, &[release(&mac.0, "demo", "2.0.0")], &[]).expect("a visit");
        let before = machine.calls();
        let older = release(&mac.0, "demo", "1.0.0");
        let summary = visit_with(&machine, &[older], &[]).expect("a visit");
        assert_eq!(summary, Summary::default(), "{shell:?}");
        assert_eq!(machine.calls() - before, 1, "{shell:?}");
        assert_eq!(
            link(&machine, "demo").as_deref(),
            Some("../demo/2.0.0/demo")
        );
        assert!(!machine.path("demo/1.0.0").exists());
    }
}

#[test]
fn a_release_with_no_file_for_the_machine_gives_it_nothing() {
    for shell in shells() {
        let (mac, home) = (Temp::new("mac"), Temp::new("home"));
        let machine = Machine::new(&shell, &home.0);
        let want = release(&mac.0, "demo", "1.0.0");
        let summary = visit(&machine, &[want], &[], &nothing).expect("a visit");
        assert_eq!(summary, Summary::default(), "{shell:?}");
        assert_eq!(machine.calls(), 1, "only the look: {shell:?}");
        assert!(!machine.path("bin").exists());
        assert!(!machine.path("demo").exists());
    }
}

#[test]
fn the_cleanup_keeps_the_release_in_use_the_one_before_and_what_is_recent() {
    for shell in shells() {
        let (mac, home) = (Temp::new("mac"), Temp::new("home"));
        let machine = Machine::new(&shell, &home.0);
        // The machine has three old releases and a recent one.
        for old in ["0.5.0", "0.9.0", "1.0.0"] {
            let folder = machine.path(&format!("demo/{old}"));
            fs::create_dir_all(&folder).expect("a release");
            fs::write(folder.join("demo"), "x").expect("a file");
            age(&folder, 40);
        }
        fs::create_dir_all(machine.path("demo/0.8.0")).expect("a recent release");
        let summary =
            visit_with(&machine, &[release(&mac.0, "demo", "1.1.0")], &[]).expect("a visit");
        assert_eq!(summary.problems, Vec::<String>::new(), "{shell:?}");
        // 1.1.0 is in use, 1.0.0 is the one before it, 0.8.0 is recent: 0.5.0 and 0.9.0 go.
        assert_eq!(summary.pruned, ["demo/0.5.0", "demo/0.9.0"], "{shell:?}");
        let mut left: Vec<String> = fs::read_dir(machine.path("demo"))
            .expect("the folder")
            .map(|entry| {
                entry
                    .expect("an entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        left.sort();
        assert_eq!(left, ["0.8.0", "1.0.0", "1.1.0"], "{shell:?}");
    }
}

#[test]
fn a_program_taken_off_the_list_is_deleted_and_others_are_left() {
    for shell in shells() {
        let (mac, home) = (Temp::new("mac"), Temp::new("home"));
        let machine = Machine::new(&shell, &home.0);
        // `old` was put there by this computer; `other` by another computer; `mine` is the person's.
        visit_with(&machine, &[release(&mac.0, "old", "1.0.0")], &[]).expect("a visit");
        visit_with(&machine, &[release(&mac.0, "other", "1.0.0")], &[]).expect("a visit");
        fs::write(machine.path("bin/mine"), "#!/bin/sh\n").expect("the person's command");

        let gone = ["old".to_owned(), "never-here".to_owned()];
        let summary = visit_with(&machine, &[], &gone).expect("a visit");
        assert_eq!(summary.removed, ["old"], "{shell:?}");
        assert!(!machine.path("old").exists());
        assert_eq!(link(&machine, "old"), None);
        assert_eq!(
            link(&machine, "other").as_deref(),
            Some("../other/1.0.0/other")
        );
        assert!(machine.path("other/1.0.0/other").is_file());
        assert!(machine.path("bin/mine").is_file());
    }
}

#[test]
fn a_command_of_the_persons_own_in_bin_is_not_touched() {
    for shell in shells() {
        let (mac, home) = (Temp::new("mac"), Temp::new("home"));
        let machine = Machine::new(&shell, &home.0);
        fs::create_dir_all(machine.path("bin")).expect("bin");
        fs::write(machine.path("bin/demo"), "mine").expect("a command");
        let summary =
            visit_with(&machine, &[release(&mac.0, "demo", "1.0.0")], &[]).expect("a visit");
        assert!(summary.shipped.is_empty(), "{shell:?}");
        assert_eq!(summary.problems.len(), 1, "{summary:?}");
        assert_eq!(fs::read(machine.path("bin/demo")).expect("kept"), b"mine");
        assert_eq!(machine.calls(), 1, "nothing was sent: {shell:?}");
        // A link of the person's that points elsewhere is not ours either.
        fs::remove_file(machine.path("bin/demo")).expect("removed");
        std::os::unix::fs::symlink("/usr/bin/true", machine.path("bin/demo")).expect("a link");
        let summary =
            visit_with(&machine, &[release(&mac.0, "demo", "1.0.0")], &[]).expect("a visit");
        assert_eq!(summary.problems.len(), 1, "{summary:?}");
        assert_eq!(link(&machine, "demo").as_deref(), Some("/usr/bin/true"));
    }
}

#[test]
fn a_link_someone_else_moved_in_the_meantime_is_left_where_they_put_it() {
    for shell in shells() {
        let home = Temp::new("home");
        let machine = Machine::new(&shell, &home.0);
        fs::create_dir_all(machine.path("bin")).expect("bin");
        std::os::unix::fs::symlink("../demo/2.0.0/demo", machine.path("bin/demo")).expect("link");
        // The computer saw 0.9.0 and is putting 1.0.0 there: the machine says 2.0.0 now.
        let sum = Digest::of(b"the bytes").to_string();
        let uploaded = machine
            .run(&upload_script("demo", v("1.0.0"), &sum, "p1"), b"the bytes")
            .expect("a reply");
        assert_eq!(uploaded.status, Some(0), "{shell:?}");
        let reply = machine
            .run(
                &switch_script("demo", v("1.0.0"), &sum, "p1", Some("../demo/0.9.0/demo")),
                b"{}",
            )
            .expect("a reply");
        assert_eq!(reply.status, Some(EXIT_MOVED), "{shell:?}");
        assert_eq!(
            link(&machine, "demo").as_deref(),
            Some("../demo/2.0.0/demo")
        );
    }
}

#[test]
fn a_program_that_arrives_damaged_is_not_put_in_place() {
    for shell in shells() {
        let home = Temp::new("home");
        let machine = Machine::new(&shell, &home.0);
        let sum = Digest::of(b"the right bytes").to_string();
        let reply = machine
            .run(
                &upload_script("demo", v("1.0.0"), &sum, "p1"),
                b"other bytes",
            )
            .expect("a reply");
        // A machine with no tool to sum with takes the file's own word, so the test needs one.
        if has_sum_tool() {
            assert_eq!(reply.status, Some(EXIT_DAMAGED), "{shell:?}");
            assert_eq!(parts(&machine, "demo"), Vec::<String>::new(), "{shell:?}");
        }
        assert!(!machine.path("demo/1.0.0").exists());
        assert!(!machine.path("bin/demo").exists());
    }
}

/// A machine has a tool to sum with (one that has none takes a file's own word).
fn has_sum_tool() -> bool {
    std::process::Command::new("sh")
        .arg("-c")
        .arg("command -v sha256sum >/dev/null 2>&1 || command -v shasum >/dev/null 2>&1")
        .status()
        .is_ok_and(|status| status.success())
}

#[test]
fn two_visits_at_once_each_have_a_folder_of_their_own_and_leave_one_whole_release() {
    for shell in shells() {
        let home = Temp::new("home");
        let machine = Machine::new(&shell, &home.0);
        let sum = Digest::of(b"the bytes").to_string();
        for part in ["a", "b"] {
            let reply = machine
                .run(&upload_script("demo", v("1.0.0"), &sum, part), b"the bytes")
                .expect("a reply");
            assert_eq!(reply.status, Some(0), "{shell:?}");
        }
        // Both uploads are there side by side: the second did not wipe the first.
        assert_eq!(parts(&machine, "demo").len(), 2, "{shell:?}");
        // The first to switch makes the link; the second finds it made (both saw none) and leaves
        // it, with the release it brought already in place.
        for (part, status) in [("a", 0), ("b", EXIT_MOVED)] {
            let reply = machine
                .run(
                    &switch_script("demo", v("1.0.0"), &sum, part, None),
                    b"{\"name\":\"demo\"}",
                )
                .expect("a reply");
            assert_eq!(reply.status, Some(status), "{part}: {shell:?}");
        }
        assert_eq!(
            fs::read(machine.path("demo/1.0.0/demo")).expect("the program"),
            b"the bytes"
        );
        assert_eq!(parts(&machine, "demo"), Vec::<String>::new(), "{shell:?}");
        assert_eq!(
            link(&machine, "demo").as_deref(),
            Some("../demo/1.0.0/demo"),
            "{shell:?}"
        );
    }
}

#[test]
fn a_release_whose_upload_is_gone_or_changed_is_not_put_in_place() {
    for shell in shells() {
        let home = Temp::new("home");
        let machine = Machine::new(&shell, &home.0);
        let sum = Digest::of(b"the bytes").to_string();
        // Nothing was uploaded for this visit (another visit's sweep took it): no release, with no
        // program in it.
        let reply = machine
            .run(&switch_script("demo", v("1.0.0"), &sum, "p1", None), b"{}")
            .expect("a reply");
        assert_eq!(reply.status, Some(EXIT_INCOMPLETE), "{shell:?}");
        assert!(!machine.path("demo/1.0.0").exists(), "{shell:?}");
        assert!(!machine.path("bin/demo").exists(), "{shell:?}");
        // The file changed between the upload and the switch.
        let uploaded = machine
            .run(&upload_script("demo", v("1.0.0"), &sum, "p2"), b"the bytes")
            .expect("a reply");
        assert_eq!(uploaded.status, Some(0), "{shell:?}");
        fs::write(machine.path("demo/.1.0.0.part-p2/demo"), b"changed").expect("changed");
        let reply = machine
            .run(&switch_script("demo", v("1.0.0"), &sum, "p2", None), b"{}")
            .expect("a reply");
        if has_sum_tool() {
            assert_eq!(reply.status, Some(EXIT_DAMAGED), "{shell:?}");
            assert!(!machine.path("demo/1.0.0").exists(), "{shell:?}");
            assert!(!machine.path("bin/demo").exists(), "{shell:?}");
        }
    }
}

#[test]
fn an_upload_sweeps_the_temporary_folders_of_visits_that_ended_badly_long_ago() {
    for shell in shells() {
        let home = Temp::new("home");
        let machine = Machine::new(&shell, &home.0);
        let (old, recent) = (
            machine.path("demo/.0.9.0.part-old"),
            machine.path("demo/.0.9.0.part-recent"),
        );
        fs::create_dir_all(&old).expect("old");
        fs::create_dir_all(&recent).expect("recent");
        let long_ago = SystemTime::now() - Duration::from_secs(3 * 24 * 60 * 60);
        fs::File::open(&old)
            .and_then(|folder| folder.set_modified(long_ago))
            .expect("aged");
        let sum = Digest::of(b"the bytes").to_string();
        let reply = machine
            .run(&upload_script("demo", v("1.0.0"), &sum, "p1"), b"the bytes")
            .expect("a reply");
        assert_eq!(reply.status, Some(0), "{shell:?}");
        assert!(!old.exists(), "{shell:?}");
        assert!(recent.exists(), "{shell:?}");
    }
}

#[test]
fn a_program_that_is_not_the_signed_one_is_not_sent() {
    for shell in shells() {
        let (mac, home) = (Temp::new("mac"), Temp::new("home"));
        let machine = Machine::new(&shell, &home.0);
        let want = release(&mac.0, "demo", "1.0.0");
        let wrong_sum = |want: &Want| {
            Ok(Some(Program {
                path: want.dir.join(TARGET),
                sha256: Digest::of(b"what the author signed").to_string(),
            }))
        };
        let summary = visit(&machine, &[want], &[], &wrong_sum).expect("a visit");
        assert_eq!(summary.problems.len(), 1, "{summary:?}");
        assert_eq!(machine.calls(), 1, "only the look: {shell:?}");
        assert!(!machine.path("demo").exists(), "{shell:?}");
    }
}

#[test]
fn the_look_with_nothing_to_look_for_is_still_whole() {
    for shell in shells() {
        let home = Temp::new("home");
        let machine = Machine::new(&shell, &home.0);
        let reply = machine.run(&probe_script(&[], &[]), b"").expect("a reply");
        assert_eq!(reply.status, Some(0), "{shell:?}");
        assert!(Probe::read(&reply.out).is_some(), "{}", reply.out);
        let reply = machine
            .run(&probe_script(&[], &["gone".to_owned()]), b"")
            .expect("a reply");
        assert_eq!(reply.status, Some(0), "{shell:?}");
        assert!(Probe::read(&reply.out).is_some(), "{}", reply.out);
    }
}

#[test]
fn the_look_reads_back_what_the_machine_has() {
    let output = "Welcome, noise\napp demo\nlink ../demo/1.2.0/demo\ndir 1.0.0\ndir 1.2.0\n\
                  dir not-a-version\nold 1.0.0\napp other\nforeign\nstale gone\nend\n";
    let probe = Probe::read(output).expect("whole");
    let demo = &probe.apps["demo"];
    assert_eq!(demo.link.as_deref(), Some("../demo/1.2.0/demo"));
    assert_eq!(demo.dirs, [v("1.0.0"), v("1.2.0")]);
    assert_eq!(demo.old, [v("1.0.0")]);
    assert_eq!(demo.current("demo"), Some(v("1.2.0")));
    assert!(probe.apps["other"].foreign);
    assert_eq!(probe.stale, ["gone"]);
    // Cut short: not the machine's state.
    assert_eq!(Probe::read("app demo\nlink ../demo/1.2.0/demo\n"), None);
}

#[test]
fn only_a_link_a_mac_writes_names_a_release() {
    let seen = |link: &str| Seen {
        link: Some(link.to_owned()),
        ..Seen::default()
    };
    assert_eq!(seen("../demo/1.2.3/demo").current("demo"), Some(v("1.2.3")));
    for other in [
        "../demo/1.2.3/other",
        "../other/1.2.3/demo",
        "/usr/bin/demo",
        "../demo/latest/demo",
        "../demo/1.2/demo",
        "demo/1.2.3/demo",
    ] {
        assert_eq!(seen(other).current("demo"), None, "{other}");
    }
}

// ---- the visits of a distributor -----------------------------------------------------------------

/// A distributor over a folder that holds one landed program, for a machine a test runs scripts on.
fn distributor_of(root: &Path) -> Distributor {
    Distributor::new(root.to_owned(), Arc::new(Curl))
}

#[test]
fn a_distributor_visits_a_machine_once_while_nothing_changes_and_reports_it() {
    let (mac, home) = (Temp::new("mac"), Temp::new("home"));
    // Nothing followed: no call at all.
    let distributor = distributor_of(&mac.0);
    let shell = shells().remove(0);
    let machine = Machine::new(&shell, &home.0);
    let now = SystemTime::now();
    distributor.sync_unless_recent("rasp", TARGET, &machine, now);
    assert_eq!(machine.calls(), 0);

    // A program that landed and is followed. Its signed `app.json` is not here, so the visit finds
    // the release unreadable for the machine: the report says so, and the visit is not repeated
    // at once.
    release(&mac.0, "demo", "1.0.0");
    store::follow(&mac.0, "demo", "https://example.test/demo/app.json").expect("followed");
    distributor.sync_unless_recent("rasp", TARGET, &machine, now);
    let calls = machine.calls();
    assert!(calls >= 1);
    let reports = distributor.reports();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].machine, "rasp");
    let summary = reports[0].result.as_ref().expect("the machine answered");
    assert_eq!(summary.problems.len(), 1, "{summary:?}");

    distributor.sync_unless_recent("rasp", TARGET, &machine, now + Duration::from_secs(60));
    assert_eq!(machine.calls(), calls, "nothing changed, a minute later");
    distributor.sync_unless_recent("rasp", TARGET, &machine, now + SPACING * 2);
    assert!(machine.calls() > calls, "and later it looks again");

    // Something changed here: a visit at once, whatever the time.
    let calls = machine.calls();
    release(&mac.0, "demo", "1.1.0");
    distributor.sync_unless_recent("rasp", TARGET, &machine, now + SPACING * 2);
    assert!(machine.calls() > calls);
}
