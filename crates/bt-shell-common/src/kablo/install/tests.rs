use std::cell::RefCell;
use std::os::unix::fs::PermissionsExt;

use super::*;
use crate::settings::TempRoot;

/// A package of four binaries (one per target) that are the same script, and
/// the list of their sums.
fn package(root: &Path, version: &str, body: &str) -> Bundle {
    let dir = root.join("kablo");
    std::fs::create_dir_all(&dir).expect("package dir");
    let mut sums = String::new();
    for target in Target::ALL {
        let name = install::artifact_name(version, target).expect("name");
        std::fs::write(dir.join(&name), body).expect("binary");
        sums.push_str(&format!("{}  {name}\n", sha256_hex(body.as_bytes())));
    }
    sums.push_str(&format!("{}  LICENSE\n", sha256_hex(b"license")));
    std::fs::write(dir.join(install::CHECKSUMS), &sums).expect("sums");
    Bundle::open(&dir).expect("a package")
}

/// A kablo that is a script: it says its version the way the real one does.
const KABLO: &str = "#!/bin/sh\necho \"version=0.2.0 wire_min=1 wire_max=2\"\n";

/// A channel that runs the scripts for real with `sh`, in a directory that
/// stands for the server's home, and counts what it was asked.
struct Home {
    dir: PathBuf,
    calls: RefCell<Vec<String>>,
    /// Corrupts the bytes on their way (a bad link).
    damage: bool,
    /// What `uname` says instead of the machine's own.
    uname: Option<&'static str>,
    /// Said before every script: a limit on the size of a file makes the write fail.
    prefix: &'static str,
}

impl Home {
    fn new(dir: &Path) -> Home {
        Home {
            dir: dir.to_owned(),
            calls: RefCell::new(Vec::new()),
            damage: false,
            uname: None,
            prefix: "",
        }
    }

    fn installed(&self, version: &str) -> PathBuf {
        self.dir.join(format!(".local/share/kablo/{version}/kablo"))
    }

    fn uploads(&self) -> usize {
        self.calls
            .borrow()
            .iter()
            .filter(|script| script.contains("cat >"))
            .count()
    }
}

impl Channel for Home {
    fn run(&self, script: &str, input: &[u8]) -> io::Result<Reply> {
        self.calls.borrow_mut().push(script.to_owned());
        if let Some(words) = self.uname
            && script == install::UNAME_COMMAND
        {
            return Ok(Reply {
                status: Some(0),
                out: format!("{words}\n"),
                err: String::new(),
            });
        }
        let mut bytes = input.to_vec();
        if self.damage
            && let Some(first) = bytes.first_mut()
        {
            *first ^= 1;
        }
        // The server's side: `sh -c <script>` in the home directory.
        let command = format!("sh -c {}", sq(&format!("{}{script}", self.prefix)));
        let reply = run_command(
            &[
                "sh".into(),
                "-c".into(),
                format!("cd {} && {command}", sq(&self.dir.to_string_lossy())),
            ],
            &bytes,
            Duration::from_secs(20),
        )?;
        Ok(reply)
    }
}

#[test]
fn sha256_is_the_standard_one() {
    for (text, sum) in [
        (
            "",
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        ),
        (
            "abc",
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        ),
        (
            "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
        ),
    ] {
        assert_eq!(sha256_hex(text.as_bytes()), sum, "{text:?}");
    }
    // Lengths around the padding's edge: 55 and 56 bytes need one and two blocks.
    assert_eq!(
        sha256_hex(&[b'a'; 55]),
        "9f4390f8d30c2dd92ec9f095b65e2b9ae9b0a925a5258e241c9f1e910f734318"
    );
    assert_eq!(
        sha256_hex(&[b'a'; 56]),
        "b35439a4ac6f0948b6d6f9e3c6af0f5f590ce20f1bde7090ef7970686ec6738a"
    );
    assert_eq!(
        sha256_hex(&vec![b'a'; 1_000_000]),
        "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
    );
}

#[test]
fn the_package_names_its_version_and_hands_out_files_that_are_the_lists() {
    let root = TempRoot::new("kablo-bundle");
    let bundle = package(&root.0, "0.2.0", KABLO);
    assert_eq!(bundle.version(), "0.2.0");
    assert_eq!(bundle.remote_path(), ".local/share/kablo/0.2.0/kablo");
    for target in Target::ALL {
        let artifact = bundle.artifact(target).expect("every target");
        assert_eq!(artifact.name, format!("kablo-0.2.0-{}", target.triple()));
        assert_eq!(bundle.read(&artifact).expect("whole"), KABLO.as_bytes());
    }
    // A file that is not what the list says is not handed out.
    let artifact = bundle.artifact(Target::LinuxX86_64).expect("artifact");
    std::fs::write(&artifact.path, "swapped").expect("write");
    let error = bundle.read(&artifact).expect_err("a different file");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn the_version_is_the_names_not_the_one_this_build_links() {
    let sum = sha256_hex(b"x");
    let triples = |version: &str| -> String {
        Target::ALL
            .iter()
            .map(|t| format!("{sum}  kablo-{version}-{}\n", t.triple()))
            .collect()
    };
    let dir = Path::new("/nowhere");
    let bundle = Bundle::from_sums(dir, &triples("9.8.7-rc.1")).expect("a package");
    assert_eq!(bundle.version(), "9.8.7-rc.1");
    // `sha256sum -b` marks binary files with a star.
    let starred = triples("0.3.0").replace("  kablo", " *kablo");
    assert_eq!(
        Bundle::from_sums(dir, &starred).expect("starred").version(),
        "0.3.0"
    );
    // Two versions are not a package.
    let mixed = format!("{}{}", triples("0.2.0"), triples("0.3.0"));
    assert!(Bundle::from_sums(dir, &mixed).is_err());
    // No binary, no package; a line that is not a sum is skipped.
    assert!(Bundle::from_sums(dir, &format!("{sum}  LICENSE\nnot a sum\n")).is_err());
    assert!(Bundle::from_sums(dir, "").is_err());
    // A sum of the wrong length is not one.
    assert!(Bundle::from_sums(dir, "abc  kablo-0.2.0-aarch64-apple-darwin\n").is_err());
}

#[test]
fn the_first_connection_writes_kablo_and_the_next_finds_it() {
    let root = TempRoot::new("kablo-install");
    let bundle = package(&root.0, "0.2.0", KABLO);
    let home = Home::new(&root.0.join("home"));
    std::fs::create_dir_all(&home.dir).expect("home");

    let first = ensure(&home, &bundle).expect("installed");
    assert!(first.fresh, "written now");
    assert_eq!(first.version, "0.2.0");
    assert_eq!(first.path, ".local/share/kablo/0.2.0/kablo");
    let file = home.installed("0.2.0");
    assert_eq!(std::fs::read(&file).expect("the file"), KABLO.as_bytes());
    let mode = std::fs::metadata(&file)
        .expect("metadata")
        .permissions()
        .mode();
    assert_eq!(mode & 0o755, 0o755, "runnable");
    // Nothing of the temporary name stays.
    let left: Vec<_> = std::fs::read_dir(file.parent().expect("dir"))
        .expect("dir")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left, ["kablo"]);
    assert_eq!(home.uploads(), 1);

    // The second time the sum is the same: nothing is written.
    let second = ensure(&home, &bundle).expect("found");
    assert!(!second.fresh);
    assert_eq!(home.uploads(), 1, "no second upload");
    assert_eq!(std::fs::read(&file).expect("the file"), KABLO.as_bytes());
}

#[test]
fn a_binary_that_is_there_and_is_not_the_packages_is_left_alone() {
    let root = TempRoot::new("kablo-differs");
    let bundle = package(&root.0, "0.2.0", KABLO);
    let mut home = Home::new(&root.0.join("home"));
    // A Linux binary is not signed: its sum is its identity.
    home.uname = Some("Linux x86_64");
    let file = home.installed("0.2.0");
    std::fs::create_dir_all(file.parent().expect("dir")).expect("dir");
    std::fs::write(&file, "#!/bin/sh\necho \"version=0.2.0\"\necho other\n").expect("other");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let before = std::fs::read(&file).expect("read");
    assert!(
        matches!(ensure(&home, &bundle), Err(Failure::Transient(_))),
        "a sum that differs is not trusted"
    );
    assert_eq!(std::fs::read(&file).expect("read"), before, "untouched");
    assert_eq!(home.uploads(), 0);
}

#[test]
fn a_macos_binary_another_package_signed_is_the_same_kablo() {
    // Two packages of one kablo release sign the macOS binary each its own way,
    // so the sums differ though the file is the same kablo: the version it says
    // is the identity there, or the server would be a plain `ssh` for good.
    let root = TempRoot::new("kablo-resigned");
    let bundle = package(&root.0, "0.2.0", KABLO);
    let mut home = Home::new(&root.0.join("home"));
    home.uname = Some("Darwin arm64");
    let file = home.installed("0.2.0");
    std::fs::create_dir_all(file.parent().expect("dir")).expect("dir");
    std::fs::write(&file, format!("{KABLO}# another signature\n")).expect("other");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let found = ensure(&home, &bundle).expect("the same kablo");
    assert!(!found.fresh);
    assert_eq!(home.uploads(), 0, "left as it is");
    // A file that names another version is not it, signed or not.
    std::fs::write(&file, "#!/bin/sh\necho \"version=0.1.0\"\n").expect("older");
    assert!(matches!(ensure(&home, &bundle), Err(Failure::Transient(_))));
}

#[test]
fn a_file_that_cannot_run_is_replaced_and_a_write_that_fails_halfway_is_a_bad_moment() {
    let root = TempRoot::new("kablo-halfway");
    let bundle = package(&root.0, "0.2.0", KABLO);
    // What an earlier, interrupted install left: the file is there and is not
    // runnable, so the probe saw nothing and the upload writes over it.
    let home = Home::new(&root.0.join("home-a"));
    let file = home.installed("0.2.0");
    std::fs::create_dir_all(file.parent().expect("dir")).expect("dir");
    std::fs::write(&file, "half").expect("half");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).expect("chmod");
    let installed = ensure(&home, &bundle).expect("written");
    assert!(installed.fresh);
    assert_eq!(std::fs::read(&file).expect("the file"), KABLO.as_bytes());
    assert_eq!(
        std::fs::metadata(&file).expect("meta").permissions().mode() & 0o111,
        0o111
    );
    // A write that fails once the file is made (here: a limit on file size, as
    // a full disk would) is not a server kablo cannot be kept on, and nothing
    // is left of it.
    let mut home = Home::new(&root.0.join("home-b"));
    home.prefix = "ulimit -f 0; ";
    std::fs::create_dir_all(&home.dir).expect("home");
    assert!(matches!(ensure(&home, &bundle), Err(Failure::Transient(_))));
    let left: Vec<_> = std::fs::read_dir(home.installed("0.2.0").parent().expect("dir"))
        .expect("dir")
        .map(|entry| entry.expect("entry").file_name())
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

#[test]
fn a_server_kablo_cannot_be_kept_on_is_told_apart_from_a_bad_moment() {
    let root = TempRoot::new("kablo-impossible");
    let bundle = package(&root.0, "0.2.0", KABLO);

    // A platform it is not built for.
    let mut home = Home::new(&root.0.join("home-a"));
    std::fs::create_dir_all(&home.dir).expect("home");
    home.uname = Some("FreeBSD amd64");
    assert_eq!(
        ensure(&home, &bundle),
        Err(Failure::Impossible(Why::Platform("FreeBSD amd64".into())))
    );
    // A 32-bit userland on a 64-bit kernel is not a target either.
    home.uname = Some("Linux armv7l");
    assert!(matches!(
        ensure(&home, &bundle),
        Err(Failure::Impossible(Why::Platform(_)))
    ));

    // A home that cannot be written to: `.local` is a file.
    let home = Home::new(&root.0.join("home-b"));
    std::fs::create_dir_all(&home.dir).expect("home");
    std::fs::write(home.dir.join(".local"), "in the way").expect("file");
    assert_eq!(
        ensure(&home, &bundle),
        Err(Failure::Impossible(Why::Unwritable))
    );

    // A file that will not run there.
    let broken = package(&root.0.join("broken"), "0.2.0", "not a program");
    let home = Home::new(&root.0.join("home-c"));
    std::fs::create_dir_all(&home.dir).expect("home");
    assert_eq!(
        ensure(&home, &broken),
        Err(Failure::Impossible(Why::WontRun))
    );
    assert!(
        !home.installed("0.2.0").exists(),
        "what will not run is not left at the place"
    );

    // A file that arrives damaged is a bad moment, and is not kept either.
    let mut home = Home::new(&root.0.join("home-d"));
    std::fs::create_dir_all(&home.dir).expect("home");
    home.damage = true;
    assert!(matches!(ensure(&home, &bundle), Err(Failure::Transient(_))));
    assert!(!home.installed("0.2.0").exists());
    let dir = home.installed("0.2.0");
    let left = std::fs::read_dir(dir.parent().expect("dir"))
        .expect("dir")
        .count();
    assert_eq!(left, 0, "no temporary file stays");
}

#[test]
fn a_channel_that_cannot_be_used_is_a_bad_moment() {
    struct Down;
    impl Channel for Down {
        fn run(&self, _: &str, _: &[u8]) -> io::Result<Reply> {
            Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "the connection went",
            ))
        }
    }
    let root = TempRoot::new("kablo-down");
    let bundle = package(&root.0, "0.2.0", KABLO);
    assert!(matches!(ensure(&Down, &bundle), Err(Failure::Transient(_))));
    // `uname` that fails (ssh's own 255) is no verdict on the platform.
    struct Failing;
    impl Channel for Failing {
        fn run(&self, _: &str, _: &[u8]) -> io::Result<Reply> {
            Ok(Reply {
                status: Some(255),
                out: String::new(),
                err: "Connection closed".into(),
            })
        }
    }
    assert!(matches!(
        ensure(&Failing, &bundle),
        Err(Failure::Transient(_))
    ));
}

/// A kablo that writes down what `upgrade` was asked and where it was run from,
/// beside itself, and says what the real one says when there is nothing to renew.
const KABLO_RENEWING: &str = "#!/bin/sh\ncase $1 in version) echo \"version=0.2.0 wire_min=1 wire_max=2\";; \
     upgrade) echo \"$* from $(pwd)\" >>\"$(dirname \"$0\")/upgrades\"; [ -e \"$(dirname \"$0\")/none\" ] && exit 1; exit 0;; esac\n";

#[test]
fn the_terminals_of_the_server_are_renewed_with_the_binary_that_was_written() {
    let root = TempRoot::new("kablo-renew");
    let bundle = package(&root.0, "0.2.0", KABLO_RENEWING);
    let home = Home::new(&root.0.join("home"));
    std::fs::create_dir_all(&home.dir).expect("home");
    let installed = ensure(&home, &bundle).expect("installed");
    assert!(installed.fresh);

    // The script is the version's own, run from the home directory the commands
    // of an ssh start in: kablo makes the relative path absolute from there.
    assert!(upgrade_sessions(&home, &installed));
    let log = home
        .installed("0.2.0")
        .parent()
        .expect("dir")
        .join("upgrades");
    let said = std::fs::read_to_string(&log).expect("asked");
    assert_eq!(
        said.trim(),
        format!(
            "upgrade --all .local/share/kablo/0.2.0/kablo from {}",
            home.dir.display()
        )
    );
    // Nothing else of the server is touched by it.
    assert_eq!(home.uploads(), 1);
    assert_eq!(
        std::fs::read(home.installed("0.2.0")).expect("the file"),
        KABLO_RENEWING.as_bytes()
    );

    // A server with no terminals says so with a failure, and that is not a reason
    // to do anything: the answer is only that nothing was renewed.
    std::fs::write(log.with_file_name("none"), "").expect("no terminals");
    assert!(!upgrade_sessions(&home, &installed));
    assert_eq!(
        std::fs::read_to_string(&log)
            .expect("asked")
            .lines()
            .count(),
        2
    );
}

#[test]
fn a_renewal_that_cannot_be_asked_is_nothing_more_than_a_no() {
    struct Failing;
    impl Channel for Failing {
        fn run(&self, _: &str, _: &[u8]) -> io::Result<Reply> {
            Err(io::Error::new(io::ErrorKind::TimedOut, "slow"))
        }
    }
    let installed = Installed {
        target: Target::LinuxX86_64,
        version: "0.2.0".into(),
        path: ".local/share/kablo/0.2.0/kablo".into(),
        fresh: true,
    };
    assert!(!upgrade_sessions(&Failing, &installed));
}

#[test]
fn the_scripts_are_one_line_of_plain_sh_a_login_shell_hands_on_as_it_is() {
    for script in [
        probe_script("0.2.0"),
        upload_script("0.2.0", &sha256_hex(b"x")),
        upgrade_script("0.2.0"),
    ] {
        // `is_inline`'s alphabet: what fish and csh read inside single quotes
        // as the characters they are.
        assert!(
            script
                .bytes()
                .all(|b| (b' '..=b'~').contains(&b) && !matches!(b, b'\'' | b'\\' | b'!')),
            "{script}"
        );
    }
}

#[test]
fn the_ssh_channel_sends_the_script_as_one_word_for_sh_and_ends_a_step_that_takes_too_long() {
    let root = TempRoot::new("kablo-channel");
    let log = root.0.join("argv");
    let fake = root.0.join("ssh");
    crate::child::tests::write_program(
        &fake,
        &format!(
            "#!/bin/sh\nfor a in \"$@\"; do printf '[%s]' \"$a\" >> {log}; done\ncat >> {log}.in\necho ok\n",
            log = log.display()
        ),
    );
    let channel = SshChannel {
        argv: vec![
            fake.to_string_lossy().into_owned(),
            "-T".into(),
            "rasp".into(),
        ],
        limit: Duration::from_secs(20),
    };
    let reply = channel.run("echo 'hi'; exit 3", b"bytes").expect("ran");
    assert_eq!(reply.out, "ok\n");
    assert_eq!(
        std::fs::read_to_string(&log).expect("log"),
        "[-T][rasp][sh -c 'echo '\"'\"'hi'\"'\"'; exit 3']",
        "the script is quoted as one word"
    );
    assert_eq!(
        std::fs::read(format!("{}.in", log.display())).expect("in"),
        b"bytes"
    );

    let slow = root.0.join("slow");
    crate::child::tests::write_program(&slow, "#!/bin/sh\nsleep 5\n");
    let started = Instant::now();
    let error = run_command(
        &[slow.to_string_lossy().into_owned()],
        &[],
        Duration::from_millis(150),
    )
    .expect_err("too long");
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(3));
}
