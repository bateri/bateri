//! The helper's install on this Mac, against a server of the test's own on the loopback (nothing
//! goes to GitHub) and apps the test signs itself (ad hoc: the rule under test is "this
//! requirement", which an ad-hoc signature can meet or fail like any other).

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bt_shell_common::browser::layout::APP;
use bt_shell_common::browser::notice::{Refusal, arch};
use bt_shell_common::digest::sha256_hex;

use super::*;

const IDENTIFIER: &str = "dev.bateri.test.helper";

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bateri-install-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A server of fixed files on the loopback.
struct Server {
    port: u16,
    files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    /// Files that are sent a piece of and then not at all (a download that is stuck).
    stalled: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
}

impl Server {
    fn start() -> Server {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let files: Arc<Mutex<HashMap<String, Vec<u8>>>> = Arc::default();
        let stalled: Arc<Mutex<Vec<String>>> = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        let (served, halt, stuck) = (files.clone(), stop.clone(), stalled.clone());
        std::thread::spawn(move || {
            while !halt.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let (served, stuck) = (served.clone(), stuck.clone());
                        std::thread::spawn(move || answer(stream, &served, &stuck));
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(5)),
                }
            }
        });
        Server {
            port,
            files,
            stalled,
            stop,
        }
    }

    fn base(&self) -> String {
        format!("http://127.0.0.1:{}/release", self.port)
    }

    fn stall(&self, name: &str) {
        self.stalled
            .lock()
            .unwrap()
            .push(format!("/release/{name}"));
    }

    fn put(&self, name: &str, body: &[u8]) {
        self.files
            .lock()
            .unwrap()
            .insert(format!("/release/{name}"), body.to_vec());
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

fn answer(
    mut stream: TcpStream,
    files: &Mutex<HashMap<String, Vec<u8>>>,
    stalled: &Mutex<Vec<String>>,
) {
    stream.set_nonblocking(false).unwrap();
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if stream.read(&mut byte).unwrap_or(0) == 0 {
            return;
        }
        head.push(byte[0]);
    }
    let head = String::from_utf8_lossy(&head).into_owned();
    let path = head.split_whitespace().nth(1).unwrap_or("").to_owned();
    if stalled.lock().unwrap().contains(&path) {
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: 10000000\r\nConnection: close\r\n\r\n"
        );
        let _ = stream.write_all(&[0u8; 1024]);
        std::thread::sleep(Duration::from_secs(5));
        return;
    }
    let body = files.lock().unwrap().get(&path).cloned();
    let (status, body) = body.map_or(("404 Not Found", Vec::new()), |body| ("200 OK", body));
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(&body);
}

/// A signed app: a copy of a system program with the identifier and one sealed resource.
fn signed_app(dir: &Path, identifier: &str) -> PathBuf {
    let app = dir.join(APP);
    std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
    std::fs::create_dir_all(app.join("Contents/Resources")).unwrap();
    std::fs::copy("/usr/bin/true", app.join("Contents/MacOS/bateri-browser")).unwrap();
    std::fs::write(app.join("Contents/Resources/data.txt"), "sealed").unwrap();
    std::fs::write(
        app.join("Contents/Info.plist"),
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>bateri-browser</string>
<key>CFBundleIdentifier</key><string>{identifier}</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleVersion</key><string>1</string>
</dict></plist>"#
        ),
    )
    .unwrap();
    let signed = Command::new("/usr/bin/codesign")
        .args(["--force", "--sign", "-", "--identifier", identifier])
        .arg(&app)
        .output()
        .unwrap();
    assert!(
        signed.status.success(),
        "{}",
        String::from_utf8_lossy(&signed.stderr)
    );
    app
}

fn zip(app: &Path, to: &Path) {
    let status = Command::new("/usr/bin/ditto")
        .args(["-c", "-k", "--sequesterRsrc", "--keepParent"])
        .arg(app)
        .arg(to)
        .status()
        .unwrap();
    assert!(status.success());
}

fn is_ours() -> Trust {
    Arc::new(|app| signature::verify(app, &format!("identifier \"{IDENTIFIER}\"")))
}

/// What a release holds: the archive, its list of sums and a notice naming `version`.
struct Release {
    version: &'static str,
    minimum: &'static str,
    protocol: u32,
    /// What the notice says of the archive: a wrong size or digest is a lie of the notice.
    size_off: i64,
    digest: Option<String>,
    sums_digest: Option<String>,
    tamper: bool,
}

impl Release {
    fn new(version: &'static str) -> Self {
        Release {
            version,
            minimum: "0.1.0",
            protocol: PROTOCOL,
            size_off: 0,
            digest: None,
            sums_digest: None,
            tamper: false,
        }
    }

    fn publish(&self, server: &Server, scratch: &Path) {
        let app = signed_app(&scratch.join(self.version), IDENTIFIER);
        if self.tamper {
            std::fs::write(app.join("Contents/Resources/data.txt"), "changed").unwrap();
        }
        let name = format!("bateri-browser-{}-{}.zip", self.version, arch());
        let archive = scratch.join(&name);
        zip(&app, &archive);
        let bytes = std::fs::read(&archive).unwrap();
        let real = sha256_hex(&bytes);
        let digest = self.digest.clone().unwrap_or_else(|| real.clone());
        let sums = self.sums_digest.clone().unwrap_or(real);
        server.put(&name, &bytes);
        server.put("SHA256SUMS", format!("{sums}  {name}\n").as_bytes());
        let size = i64::try_from(bytes.len()).unwrap() + self.size_off;
        server.put(
            "notice.json",
            format!(
                r#"{{"format":1,"version":"{}","chromium":"156.0.0.0","protocol":{},"minimum":"{}","assets":[{{"arch":"{}","name":"{name}","size":{size},"sha256":"{digest}"}}]}}"#,
                self.version,
                self.protocol,
                self.minimum,
                arch()
            )
            .as_bytes(),
        );
    }
}

struct Rig {
    server: Server,
    installer: Installer,
    root: PathBuf,
}

fn rig(name: &str) -> Rig {
    let root = temp(name);
    let server = Server::start();
    let installer = Installer::new(Layout::new(root.join("helper")), &server.base(), is_ours());
    Rig {
        server,
        installer,
        root,
    }
}

fn offer(check: Check) -> Offer {
    match check {
        Check::Offer(offer) => offer,
        other => panic!("expected an offer, got {other:?}"),
    }
}

fn install(rig: &Rig, offer: &Offer) -> Result<Version, InstallError> {
    rig.installer
        .install(offer, &|_, _| {}, &AtomicBool::new(false))
}

fn v(text: &str) -> Version {
    Version::parse(text).unwrap()
}

#[test]
fn a_release_is_downloaded_checked_unpacked_and_put_in_place() {
    let rig = rig("install");
    Release::new("0.2.0").publish(&rig.server, &rig.root);
    let offer = offer(rig.installer.check());
    assert_eq!(offer.version, v("0.2.0"));
    assert_eq!(offer.chromium, "156.0.0.0");
    let reported = Mutex::new(Vec::new());
    let installed = rig
        .installer
        .install(
            &offer,
            &|received, total| reported.lock().unwrap().push((received, total)),
            &AtomicBool::new(false),
        )
        .unwrap();
    assert_eq!(installed, v("0.2.0"));
    let layout = rig.installer.layout();
    assert_eq!(layout.installed(), vec![v("0.2.0")]);
    assert!(layout.executable(&v("0.2.0")).is_file());
    assert_eq!(layout.floor(), v("0.2.0"));
    // The app is where it will be run from, and still ours there.
    signature::verify(
        &layout.app(&v("0.2.0")),
        &format!("identifier \"{IDENTIFIER}\""),
    )
    .unwrap();
    assert!(!reported.lock().unwrap().is_empty(), "the progress is told");
    // Nothing of the staging is left.
    let leftovers: Vec<_> = std::fs::read_dir(layout.root())
        .unwrap()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(".staging"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
    // It is the release now: nothing more is on offer.
    assert_eq!(rig.installer.check(), Check::Current);
    std::fs::remove_dir_all(&rig.root).unwrap();
}

/// A release made by `make browser-release` in a folder is what the real downloader takes an engine
/// from, as it takes one from GitHub (`make dev BROWSER_RELEASES=<folder>`): the notice, the sums,
/// the archive and its unpacking are the ones the other tests run against a server.
#[test]
fn a_local_release_folder_made_by_the_tool_is_downloaded_and_put_in_place() {
    let root = temp("local-release");
    let app = signed_app(&root.join("build"), IDENTIFIER);
    // The tool reads the version from the app, which is sealed: sign again with it in.
    let inserted = Command::new("/usr/bin/plutil")
        .args(["-insert", "CFBundleShortVersionString", "-string", "0.3.0"])
        .arg(app.join("Contents/Info.plist"))
        .status()
        .unwrap();
    assert!(inserted.success());
    let signed = Command::new("/usr/bin/codesign")
        .args(["--force", "--sign", "-", "--identifier", IDENTIFIER])
        .arg(&app)
        .output()
        .unwrap();
    assert!(signed.status.success());
    let folder = root.join("release");
    let made = Command::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tools/browser-release.sh"
    ))
    .arg(&app)
    .arg(&folder)
    .output()
    .unwrap();
    assert!(
        made.status.success(),
        "{}",
        String::from_utf8_lossy(&made.stderr)
    );
    // Made again over what is there, it is the same release.
    assert!(
        Command::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tools/browser-release.sh"
        ))
        .arg(&app)
        .arg(&folder)
        .output()
        .unwrap()
        .status
        .success()
    );

    let base = folder_base(&folder).expect("an address for the folder");
    assert!(base.starts_with("file://"), "{base}");
    let layout = Layout::new(root.join("helper"));
    // No signature is asked of an engine from a folder a person made for their own Mac.
    let installer = Installer::new(layout, &base, Arc::new(|_| Ok(())));
    let offer = offer(installer.check());
    assert_eq!(offer.version, v("0.3.0"));
    let reported = Mutex::new(Vec::new());
    let installed = installer
        .install(
            &offer,
            &|received, total| reported.lock().unwrap().push((received, total)),
            &AtomicBool::new(false),
        )
        .unwrap();
    assert_eq!(installed, v("0.3.0"));
    assert_eq!(installer.layout().installed(), vec![v("0.3.0")]);
    assert!(installer.layout().executable(&v("0.3.0")).is_file());
    assert!(!reported.lock().unwrap().is_empty(), "the progress is told");
    assert_eq!(installer.check(), Check::Current);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn an_archive_that_is_not_what_the_notice_says_is_not_unpacked() {
    for (what, release) in [
        (
            "a digest the notice got wrong",
            Release {
                digest: Some("0".repeat(64)),
                ..Release::new("0.2.0")
            },
        ),
        (
            "a size the notice got wrong",
            Release {
                size_off: 1,
                ..Release::new("0.2.0")
            },
        ),
        (
            "sums that say another digest",
            Release {
                sums_digest: Some("1".repeat(64)),
                ..Release::new("0.2.0")
            },
        ),
    ] {
        let rig = rig("digest");
        release.publish(&rig.server, &rig.root);
        let offer = offer(rig.installer.check());
        let error = install(&rig, &offer).unwrap_err();
        assert!(matches!(error, InstallError::Digest(_)), "{what}: {error}");
        assert!(rig.installer.layout().installed().is_empty(), "{what}");
        std::fs::remove_dir_all(&rig.root).unwrap();
    }
}

#[test]
fn an_app_whose_signature_is_broken_is_never_put_in_place() {
    let rig = rig("signature");
    // The archive is what the notice says (its digest is of the changed app): the one thing wrong
    // is that a sealed file was changed after the signing.
    Release {
        tamper: true,
        ..Release::new("0.2.0")
    }
    .publish(&rig.server, &rig.root);
    let offer = offer(rig.installer.check());
    let error = install(&rig, &offer).unwrap_err();
    assert!(matches!(error, InstallError::Signature(_)), "{error}");
    assert!(rig.installer.layout().installed().is_empty());
    assert_eq!(rig.installer.layout().floor(), Version::zero());
    std::fs::remove_dir_all(&rig.root).unwrap();
}

#[test]
fn an_app_signed_for_another_requirement_is_refused() {
    let rig = rig("team");
    Release::new("0.2.0").publish(&rig.server, &rig.root);
    // The requirement the product uses: Apple's chain and a team. An ad-hoc app has neither.
    let installer = Installer::new(
        Layout::new(rig.root.join("helper")),
        &rig.server.base(),
        Arc::new(|app| signature::verify(app, &signature::requirement_for_team("ABCDE12345")?)),
    );
    let offer = offer(installer.check());
    let error = installer
        .install(&offer, &|_, _| {}, &AtomicBool::new(false))
        .unwrap_err();
    assert!(matches!(error, InstallError::Signature(_)), "{error}");
    assert!(installer.layout().installed().is_empty());
    std::fs::remove_dir_all(&rig.root).unwrap();
}

#[test]
fn an_older_release_is_not_installed_over_a_newer_one() {
    let rig = rig("downgrade");
    Release::new("0.2.0").publish(&rig.server, &rig.root);
    let first = offer(rig.installer.check());
    install(&rig, &first).unwrap();
    Release::new("0.1.0").publish(&rig.server, &rig.root);
    assert_eq!(
        rig.installer.check(),
        Check::Refused(Refusal::Downgrade {
            offered: v("0.1.0"),
            floor: v("0.2.0")
        })
    );
    std::fs::remove_dir_all(&rig.root).unwrap();
}

#[test]
fn a_newer_release_replaces_the_installed_and_the_older_one_goes_when_nothing_runs_from_it() {
    let rig = rig("update");
    Release::new("0.1.0").publish(&rig.server, &rig.root);
    install(&rig, &offer(rig.installer.check())).unwrap();
    let running = rig.installer.layout().hold(&v("0.1.0")).unwrap();
    Release::new("0.2.0").publish(&rig.server, &rig.root);
    install(&rig, &offer(rig.installer.check())).unwrap();
    assert_eq!(
        rig.installer.layout().installed(),
        vec![v("0.1.0"), v("0.2.0")],
        "in use: kept"
    );
    drop(running);
    assert_eq!(rig.installer.layout().prune(), vec![v("0.1.0")]);
    std::fs::remove_dir_all(&rig.root).unwrap();
}

#[test]
fn a_protocol_this_bateri_does_not_speak_is_not_an_offer() {
    let rig = rig("protocol");
    Release {
        protocol: PROTOCOL + 1,
        ..Release::new("0.2.0")
    }
    .publish(&rig.server, &rig.root);
    assert_eq!(
        rig.installer.check(),
        Check::Refused(Refusal::Protocol {
            offered: PROTOCOL + 1,
            ours: PROTOCOL
        })
    );
    std::fs::remove_dir_all(&rig.root).unwrap();
}

#[test]
fn no_release_yet_is_a_quiet_state() {
    let rig = rig("quiet");
    // Nothing is published: that is an answer (it looks the same on the next look), and not a
    // network that did not answer.
    assert!(
        matches!(rig.installer.check(), Check::Absent(_)),
        "a server with no notice"
    );
    let closed = {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.local_addr().unwrap().port()
    };
    let installer = Installer::new(
        Layout::new(rig.root.join("helper")),
        &format!("http://127.0.0.1:{closed}/release"),
        is_ours(),
    );
    assert!(
        matches!(installer.check(), Check::Unavailable(_)),
        "nobody listening"
    );
    rig.server.put("notice.json", b"not json");
    assert!(
        matches!(rig.installer.check(), Check::Unavailable(_)),
        "a notice that is not one"
    );
    std::fs::remove_dir_all(&rig.root).unwrap();
}

/// What the first click's question reads: a release that is not there is not a network that did not
/// answer, for a server and for a folder of releases alike.
#[test]
fn a_release_that_is_not_there_is_told_from_a_network_that_did_not_answer() {
    let rig = rig("absent");
    let missing = fetch::bytes(&format!("{}/notice.json", rig.server.base()), 100)
        .expect_err("nothing is published");
    assert_eq!(missing.kind(), std::io::ErrorKind::NotFound);
    let folder = rig.root.join("releases");
    std::fs::create_dir_all(&folder).unwrap();
    let installer = Installer::new(
        Layout::new(rig.root.join("helper")),
        &folder_base(&folder).expect("a folder has an address"),
        is_ours(),
    );
    assert!(
        matches!(installer.check(), Check::Absent(_)),
        "a folder with no notice"
    );
    let closed = {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.local_addr().unwrap().port()
    };
    let unreachable = fetch::bytes(&format!("http://127.0.0.1:{closed}/notice.json"), 100)
        .expect_err("nobody listening");
    assert_ne!(unreachable.kind(), std::io::ErrorKind::NotFound);
    std::fs::remove_dir_all(&rig.root).unwrap();
}

#[test]
fn a_download_is_stopped_when_the_person_stops_it() {
    let rig = rig("cancel");
    let release = Release::new("0.2.0");
    release.publish(&rig.server, &rig.root);
    let offer = offer(rig.installer.check());
    rig.server.stall(&offer.asset.name);
    let cancel = Arc::new(AtomicBool::new(false));
    let stopper = {
        let cancel = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            cancel.store(true, Ordering::Release);
        })
    };
    let started = std::time::Instant::now();
    let error = rig
        .installer
        .install(&offer, &|_, _| {}, &cancel)
        .unwrap_err();
    stopper.join().unwrap();
    assert!(matches!(error, InstallError::Cancelled), "{error}");
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "it did not wait for the server"
    );
    assert!(rig.installer.layout().installed().is_empty());
    std::fs::remove_dir_all(&rig.root).unwrap();
}

#[test]
fn the_daily_look_is_made_once_a_day_and_only_where_an_engine_is_installed() {
    let rig = rig("daily");
    Release::new("0.1.0").publish(&rig.server, &rig.root);
    let never = AtomicBool::new(false);
    assert!(
        rig.installer.update_if_due(&|_, _| {}, &never).is_none(),
        "nothing is installed: no consent yet"
    );
    install(&rig, &offer(rig.installer.check())).unwrap();
    Release::new("0.2.0").publish(&rig.server, &rig.root);
    let updated = rig
        .installer
        .update_if_due(&|_, _| {}, &never)
        .expect("due")
        .unwrap();
    assert_eq!(updated, v("0.2.0"));
    Release::new("0.3.0").publish(&rig.server, &rig.root);
    assert!(
        rig.installer.update_if_due(&|_, _| {}, &never).is_none(),
        "looked a moment ago"
    );
    std::fs::remove_dir_all(&rig.root).unwrap();
}

#[test]
fn an_update_that_did_not_come_to_an_end_is_looked_for_again_and_not_a_day_later() {
    let rig = rig("daily-failed");
    Release::new("0.1.0").publish(&rig.server, &rig.root);
    install(&rig, &offer(rig.installer.check())).unwrap();
    let release = Release::new("0.2.0");
    release.publish(&rig.server, &rig.root);
    let never = AtomicBool::new(false);
    // The download is not what the notice says it is.
    rig.server
        .put(&offer(rig.installer.check()).asset.name, b"not the archive");
    let failed = rig
        .installer
        .update_if_due(&|_, _| {}, &never)
        .expect("due");
    assert!(failed.is_err(), "a download that is not the release");
    // The day's look is the one that ended in an engine (or in none to be had): this one is due
    // again.
    release.publish(&rig.server, &rig.root);
    let updated = rig
        .installer
        .update_if_due(&|_, _| {}, &never)
        .expect("due again")
        .unwrap();
    assert_eq!(updated, v("0.2.0"));
    std::fs::remove_dir_all(&rig.root).unwrap();
}

#[test]
fn what_is_signed_meets_its_requirement_and_nothing_changed_after_does() {
    let dir = temp("signed");
    let app = signed_app(&dir, IDENTIFIER);
    let ours = format!("identifier \"{IDENTIFIER}\"");
    signature::verify(&app, &ours).unwrap();
    assert!(
        signature::verify(&app, "identifier \"dev.someone.else\"").is_err(),
        "another identifier"
    );
    assert!(
        signature::verify(
            &app,
            &signature::requirement_for_team("ABCDE12345").unwrap()
        )
        .is_err(),
        "another team"
    );
    assert!(signature::verify(&app, "this is not a requirement").is_err());

    std::fs::write(app.join("Contents/Resources/data.txt"), "changed").unwrap();
    assert!(
        signature::verify(&app, &ours).is_err(),
        "a sealed file changed"
    );
    std::fs::write(app.join("Contents/Resources/data.txt"), "sealed").unwrap();
    signature::verify(&app, &ours).unwrap();
    std::fs::write(app.join("Contents/Resources/added.txt"), "new").unwrap();
    assert!(
        signature::verify(&app, &ours).is_err(),
        "a file added to the bundle"
    );
    std::fs::remove_file(app.join("Contents/Resources/added.txt")).unwrap();

    let unsigned = dir.join("unsigned");
    std::fs::create_dir_all(&unsigned).unwrap();
    let bare = signed_app(&unsigned, IDENTIFIER);
    Command::new("/usr/bin/codesign")
        .args(["--remove-signature"])
        .arg(&bare)
        .output()
        .unwrap();
    assert!(signature::verify(&bare, &ours).is_err(), "no signature");
    assert!(signature::verify(&dir.join("missing.app"), &ours).is_err());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_team_identifier_is_ten_letters_and_digits_and_nothing_else_gets_into_the_requirement() {
    assert!(
        signature::requirement_for_team("ZHYD8FTFYT")
            .unwrap()
            .contains("\"ZHYD8FTFYT\"")
    );
    for bad in [
        "",
        "short",
        "ZHYD8FTFYT1",
        "zhyd8ftfyt",
        "ZHYD8FTFY\"",
        "ZHYD8FT FY",
    ] {
        assert!(signature::requirement_for_team(bad).is_err(), "{bad:?}");
    }
}

/// The real helper, when `BATERI_BROWSER_APP` names it: signed by the team of the
/// Developer ID, which an ad-hoc test cannot be. Not part of the gate (the app is a build of
/// another repository).
#[test]
fn the_real_helper_is_ours_when_one_is_at_hand() {
    let Some(app) = std::env::var_os("BATERI_BROWSER_APP").map(PathBuf::from) else {
        return;
    };
    let team = std::env::var("BATERI_BROWSER_TEAM").expect("BATERI_BROWSER_TEAM names the team");
    signature::verify(&app, &signature::requirement_for_team(&team).unwrap()).unwrap();
    assert!(
        signature::verify(
            &app,
            &signature::requirement_for_team("ABCDE12345").unwrap()
        )
        .is_err()
    );
}

#[test]
fn the_browser_data_is_beside_the_apps_folder_and_a_development_copy_has_its_own() {
    let home = Path::new("/Users/x");
    assert_eq!(
        browser_path(home, None),
        Path::new("/Users/x/Library/Application Support/bateri/browser")
    );
    assert_eq!(
        browser_path(home, Some("dev")),
        Path::new("/Users/x/Library/Application Support/bateri-dev/browser")
    );
}

#[test]
fn a_development_copy_and_a_second_one_beside_it_run_the_engine_where_it_is_and_no_other_copy_does()
{
    for named in ["dev", "dev2", "dev-b"] {
        assert!(is_development(Some(named)), "{named}");
    }
    // The application itself, a copy of another name and a name that is no plain word are not.
    for other in [
        None,
        Some(""),
        Some("work"),
        Some("2dev"),
        Some("dev 2"),
        Some("dev/2"),
    ] {
        assert!(!is_development(other), "{other:?}");
    }
    // The second copy's data is its own, beside the first's.
    let home = Path::new("/Users/x");
    assert_eq!(
        browser_path(home, Some("dev2")),
        Path::new("/Users/x/Library/Application Support/bateri-dev2/browser")
    );
    assert_ne!(
        browser_path(home, Some("dev2")),
        browser_path(home, Some("dev"))
    );
}

struct Titles(std::sync::mpsc::Sender<browser_ipc::message::Event>);

impl bt_shell_common::browser::helper::Sink for Titles {
    fn event(&self, event: browser_ipc::message::Event) {
        let _ = self.0.send(event);
    }

    fn frame(&self, _: browser_ipc::Frame, _: browser_ipc::Surface) {}

    fn lost(&self, why: String) {
        panic!("the helper was lost: {why}");
    }
}

/// The real helper under the real spawner and the real handshake: started from the app
/// `BATERI_BROWSER_APP` names, it loads a `data:` page and tells the page's title, and it ends
/// after its last browser is closed. Nothing is shown (the page is drawn off screen). Not part of
/// the gate: the app is a build of another repository.
#[test]
fn the_real_helper_starts_loads_a_page_and_ends_when_one_is_at_hand() {
    use browser_ipc::message::{Command, Event};
    use bt_shell_common::browser::profile::ProfileKey;

    let Some(app) = std::env::var_os("BATERI_BROWSER_APP").map(PathBuf::from) else {
        return;
    };
    let root = temp("real");
    let config = Config {
        source: Source::Fixed(app),
        profiles: root.join("profiles"),
        run: root.join("run"),
        linger: Duration::from_millis(200),
        start_timeout: helper::START_TIMEOUT,
        trust: Arc::new(|_| Ok(())),
    };
    let manager = Manager::new(
        config,
        Arc::new(Spawner {
            log: root.join("helper.log"),
        }),
    );
    let (events, heard) = std::sync::mpsc::channel();
    let key = ProfileKey::new("mac", "test", Some("real")).unwrap();
    let browser = manager
        .open(&key, (400, 300), 1.0, Arc::new(Titles(events)))
        .unwrap();
    browser
        .send(Command::Navigate {
            browser: browser.id(),
            url: "data:text/html,%3Ctitle%3Ehello%3C/title%3Ehi".into(),
        })
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        match heard.recv_timeout(left).expect("the page's title") {
            Event::State { title, .. } if title == "hello" => break,
            _ => {}
        }
    }
    assert!(manager.status().running);
    drop(browser);
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while manager.status().running {
        assert!(
            std::time::Instant::now() < deadline,
            "the helper did not end"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = std::fs::remove_dir_all(&root);
}

// ---- editing keys against the real engine ----

/// A key as the helper takes it.
fn key_data(vk: i32, native: i32, character: u16, mods: u32) -> browser_ipc::message::KeyData {
    browser_ipc::message::KeyData {
        vk,
        native,
        character,
        unmodified: character,
        mods,
        system: false,
    }
}

fn percent(text: &str) -> String {
    text.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// The real helper with one browser, for tests that press keys in a page.
struct Probe {
    browser: helper::Browser,
    events: std::sync::mpsc::Receiver<browser_ipc::message::Event>,
    title: String,
    returned: Vec<(browser_ipc::message::KeyKind, i32, u32)>,
    _manager: Arc<Manager>,
    root: PathBuf,
}

impl Probe {
    fn start(name: &str, app: PathBuf) -> Probe {
        use bt_shell_common::browser::profile::ProfileKey;
        let root = temp(name);
        let config = Config {
            source: Source::Fixed(app),
            profiles: root.join("profiles"),
            run: root.join("run"),
            linger: Duration::from_millis(200),
            start_timeout: helper::START_TIMEOUT,
            trust: Arc::new(|_| Ok(())),
        };
        let manager = Manager::new(
            config,
            Arc::new(Spawner {
                log: root.join("helper.log"),
            }),
        );
        let (events, heard) = std::sync::mpsc::channel();
        let key = ProfileKey::new("mac", "test", Some(name)).unwrap();
        let browser = manager
            .open(&key, (400, 300), 1.0, Arc::new(Titles(events)))
            .unwrap();
        Probe {
            browser,
            events: heard,
            title: String::new(),
            returned: Vec::new(),
            _manager: manager,
            root,
        }
    }

    fn send(&self, command: browser_ipc::message::Command) {
        self.browser.send(command).unwrap();
    }

    /// Reads events until `done` says one is what is waited for; everything read is remembered.
    fn until(&mut self, what: &str, mut done: impl FnMut(&browser_ipc::message::Event) -> bool) {
        use browser_ipc::message::Event;
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            let event = self
                .events
                .recv_timeout(left)
                .unwrap_or_else(|_| panic!("never happened: {what}"));
            match &event {
                Event::State { title, .. } => self.title.clone_from(title),
                Event::UnhandledKey { kind, key, .. } => {
                    self.returned.push((*kind, key.vk, key.mods));
                }
                _ => {}
            }
            if done(&event) {
                return;
            }
        }
    }

    fn load(&mut self, html: &str) {
        use browser_ipc::message::{Command, Event};
        let id = self.browser.id();
        self.send(Command::Navigate {
            browser: id,
            url: format!("data:text/html,{}", percent(html)),
        });
        self.until(
            "the page's title",
            |e| matches!(e, Event::State { title, loading: false, .. } if title == "init"),
        );
        self.send(Command::Focus {
            browser: id,
            on: true,
        });
        // The engine takes no key for a while after a load.
        self.drain(Duration::from_millis(700));
    }

    fn click(&self, x: i32, y: i32) {
        use browser_ipc::message::{Button, Command, MouseKind};
        let id = self.browser.id();
        for kind in [MouseKind::Move, MouseKind::Down, MouseKind::Up] {
            self.send(Command::Mouse {
                browser: id,
                kind,
                x,
                y,
                button: Button::Left,
                clicks: 1,
                mods: 0,
            });
        }
    }

    fn tap(&self, key: browser_ipc::message::KeyData) {
        self.tap_with(key, &[]);
    }

    /// A key down with `commands` (what the system's text handling names for it), and up.
    fn tap_with(&self, key: browser_ipc::message::KeyData, commands: &[&str]) {
        use browser_ipc::message::{Command, KeyKind};
        for kind in [KeyKind::Down, KeyKind::Up] {
            self.send(Command::Key {
                browser: self.browser.id(),
                kind,
                key,
                commands: if kind == KeyKind::Down {
                    commands.iter().map(|name| (*name).to_owned()).collect()
                } else {
                    Vec::new()
                },
            });
        }
    }

    /// Reads what comes for `wait`: a page's title follows its keys some time after.
    fn drain(&mut self, wait: Duration) {
        use browser_ipc::message::Event;
        let deadline = std::time::Instant::now() + wait;
        while let Ok(event) = self
            .events
            .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
        {
            match &event {
                Event::State { title, .. } => self.title.clone_from(title),
                Event::UnhandledKey { kind, key, .. } => {
                    self.returned.push((*kind, key.vk, key.mods));
                }
                _ => {}
            }
        }
    }

    /// Everything pressed so far has been told: a key nobody uses (F1) comes back after it.
    fn settle(&mut self) {
        use browser_ipc::message::{Event, KeyKind};
        self.tap(key_data(112, 122, 0xF704, 0));
        self.until(
            "the sentinel's release",
            |e| matches!(e, Event::UnhandledKey { kind: KeyKind::Up, key, .. } if key.vk == 112),
        );
        self.drain(Duration::from_millis(300));
    }
}

const EDIT_PAGE: &str = "<title>init</title><body style='margin:0'>\
    <input id=i style='position:absolute;left:10px;top:10px;width:300px;height:30px' value='hello world foo'>\
    <script>const i=document.getElementById('i');\
    const rep=()=>{document.title='s:'+i.selectionStart+':'+i.selectionEnd+':'+i.value.length};\
    i.addEventListener('keyup',rep);document.addEventListener('selectionchange',rep);</script>";

/// What the editing keys do in a field of the real engine, with the commands the system's text
/// handling names for them (`doCommandBySelector:`; the names are the ones of AppKit's standard
/// key bindings, `StandardKeyBinding.dict`), measured on the helper of `browser` rev 55687f4
/// (`BATERI_BROWSER_APP=… cargo test -p bt-shell-macos --lib the_real_engine_edits`): each of
/// them does its work in the field and does not come back as a key nobody used. Outside a field
/// the same key comes back (a page that binds ⌘← hears it), and the Edit menu's commands run on
/// the field with the system's pasteboard.
#[test]
fn the_real_engine_edits_text_with_these_keys_when_one_is_at_hand() {
    use browser_ipc::message::{Command, EditAction, KeyKind, modifier as m};
    let Some(app) = std::env::var_os("BATERI_BROWSER_APP").map(PathBuf::from) else {
        return;
    };
    let mut probe = Probe::start("edit", app);
    // (name, vk, native, character, mods, commands, caret first at the start, the field's state
    // after: caret or selection start, its end, the text's length)
    type Candidate<'a> = (&'a str, i32, i32, u16, u32, &'a [&'a str], bool, &'a str);
    let left = (37, 123, 0xF702);
    let candidates: &[Candidate] = &[
        (
            "left",
            left.0,
            left.1,
            left.2,
            0,
            &["moveLeft:"],
            false,
            "s:14:14:15",
        ),
        (
            "cmd-left",
            left.0,
            left.1,
            left.2,
            m::COMMAND,
            &["moveToLeftEndOfLine:"],
            false,
            "s:0:0:15",
        ),
        (
            "home",
            36,
            115,
            0xF729,
            0,
            &["scrollToBeginningOfDocument:"],
            false,
            "s:0:0:15",
        ),
        (
            "ctrl-a",
            65,
            0,
            1,
            m::CONTROL,
            &["moveToBeginningOfParagraph:"],
            false,
            "s:0:0:15",
        ),
        (
            "alt-left",
            left.0,
            left.1,
            left.2,
            m::ALT,
            &["moveWordLeft:"],
            false,
            "s:12:12:15",
        ),
        (
            "ctrl-left",
            left.0,
            left.1,
            left.2,
            m::CONTROL,
            &["moveToLeftEndOfLine:"],
            false,
            "s:0:0:15",
        ),
        (
            "shift-home",
            36,
            115,
            0xF729,
            m::SHIFT,
            &["moveToBeginningOfDocumentAndModifySelection:"],
            false,
            "s:0:15:15",
        ),
        (
            "cmd-shift-left",
            left.0,
            left.1,
            left.2,
            m::COMMAND | m::SHIFT,
            &["moveToLeftEndOfLineAndModifySelection:"],
            false,
            "s:0:15:15",
        ),
        (
            "cmd-backspace",
            8,
            51,
            0x7F,
            m::COMMAND,
            &["deleteToBeginningOfLine:"],
            false,
            "s:0:0:0",
        ),
        (
            "alt-backspace",
            8,
            51,
            0x7F,
            m::ALT,
            &["deleteWordBackward:"],
            false,
            "s:12:12:12",
        ),
        (
            "backspace",
            8,
            51,
            0x7F,
            0,
            &["deleteBackward:"],
            false,
            "s:14:14:14",
        ),
        (
            "ctrl-backspace",
            8,
            51,
            0x7F,
            m::CONTROL,
            &["deleteBackwardByDecomposingPreviousCharacter:"],
            false,
            "s:14:14:14",
        ),
        (
            "cmd-up",
            38,
            126,
            0xF700,
            m::COMMAND,
            &["moveToBeginningOfDocument:"],
            false,
            "s:0:0:15",
        ),
        (
            "ctrl-e",
            69,
            14,
            5,
            m::CONTROL,
            &["moveToEndOfParagraph:"],
            true,
            "s:15:15:15",
        ),
        (
            "ctrl-k",
            75,
            40,
            11,
            m::CONTROL,
            &["deleteToEndOfParagraph:"],
            true,
            "s:0:0:0",
        ),
        (
            "ctrl-b",
            66,
            11,
            2,
            m::CONTROL,
            &["moveBackward:"],
            false,
            "s:14:14:15",
        ),
    ];
    for (name, vk, native, character, mods, commands, from_start, after) in candidates {
        probe.load(EDIT_PAGE);
        probe.click(100, 25);
        probe.drain(Duration::from_millis(300));
        probe.tap(if *from_start {
            key_data(36, 115, 0xF729, 0)
        } else {
            key_data(35, 119, 0xF72B, 0)
        });
        probe.settle();
        let before = probe.title.clone();
        probe.returned.clear();
        probe.tap_with(key_data(*vk, *native, *character, *mods), commands);
        probe.settle();
        let came_back = probe
            .returned
            .iter()
            .any(|(kind, key, _)| *kind == KeyKind::Down && key == vk);
        // What the key does, once: the exact state of the field, and the key not given back.
        assert_eq!(
            (probe.title.as_str(), came_back),
            (*after, false),
            "{name}: before {before}"
        );
    }
    // A command the engine does not know takes nothing from the key: it is given back as if it
    // had none (Escape's `cancelOperation:` is none of the engine's, and neither is a made-up one).
    for (name, vk, native, character, mods, command) in [
        ("escape", 27, 53, 0x1B, 0, "cancelOperation:"),
        ("made-up", 8, 51, 0x7F, m::CONTROL, "aCommandNoEngineHas:"),
    ] {
        probe.load(EDIT_PAGE);
        probe.click(100, 25);
        probe.drain(Duration::from_millis(300));
        probe.tap(key_data(35, 119, 0xF72B, 0));
        probe.settle();
        probe.returned.clear();
        probe.tap_with(key_data(vk, native, character, mods), &[command]);
        probe.settle();
        assert!(
            probe
                .returned
                .iter()
                .any(|(kind, key, _)| *kind == KeyKind::Down && *key == vk),
            "{name} with the command the engine does not know did not come back: {:?}",
            probe.returned
        );
    }
    // Outside a field the same key is nobody's and comes back (as it does with no command at all):
    // a page that binds ⌘← hears it.
    probe.load(EDIT_PAGE);
    probe.click(100, 150);
    probe.drain(Duration::from_millis(300));
    probe.settle();
    let before = probe.title.clone();
    probe.returned.clear();
    probe.tap_with(
        key_data(left.0, left.1, left.2, m::COMMAND),
        &["moveToLeftEndOfLine:"],
    );
    probe.settle();
    assert!(
        probe
            .returned
            .iter()
            .any(|(kind, key, _)| *kind == KeyKind::Down && *key == left.0),
        "⌘← outside a field did not come back: {:?}",
        probe.returned
    );
    assert_eq!(probe.title, before);
    // The Edit menu's commands run on the field that has the focus.
    probe.load(EDIT_PAGE);
    probe.click(100, 25);
    probe.drain(Duration::from_millis(300));
    probe.tap(key_data(35, 119, 0xF72B, 0));
    probe.settle();
    let id = probe.browser.id();
    probe.send(Command::Edit {
        browser: id,
        action: EditAction::SelectAll,
    });
    probe.settle();
    assert_eq!(probe.title, "s:0:15:15", "select all in the field");
    let _ = std::fs::remove_dir_all(&probe.root);
}

/// The keys of the Edit menu with the real engine: sent as keys (the page has them first, as in
/// Chrome), the engine does none of their work itself and gives each one back when the page took
/// it not, and keeps one the page took (its `keydown` handler said `preventDefault`). That is why
/// the menu's command may wait for the key to come back ([`bt_shell_common::browser::rooms`]).
/// Measured on the helper of `browser` rev 55687f4
/// (`BATERI_BROWSER_APP=… cargo test -p bt-shell-macos --lib the_real_engine_gives_back`).
#[test]
fn the_real_engine_gives_back_an_edit_key_the_page_did_not_take_and_keeps_one_it_did() {
    use browser_ipc::message::{KeyKind, modifier as m};
    let Some(app) = std::env::var_os("BATERI_BROWSER_APP").map(PathBuf::from) else {
        return;
    };
    let mut probe = Probe::start("editkeys", app);
    // (name, virtual key, native key, character, mods)
    let keys: &[(&str, i32, i32, u16, u32)] = &[
        ("cmd-a", 65, 0, 'a' as u16, m::COMMAND),
        ("cmd-c", 67, 8, 'c' as u16, m::COMMAND),
        ("cmd-x", 88, 7, 'x' as u16, m::COMMAND),
        ("cmd-v", 86, 9, 'v' as u16, m::COMMAND),
        ("cmd-z", 90, 6, 'z' as u16, m::COMMAND),
        ("cmd-shift-z", 90, 6, 'z' as u16, m::COMMAND | m::SHIFT),
    ];
    for (name, vk, native, character, mods) in keys {
        probe.load(EDIT_PAGE);
        probe.click(100, 25);
        probe.drain(Duration::from_millis(300));
        probe.tap(key_data(35, 119, 0xF72B, 0));
        probe.settle();
        let before = probe.title.clone();
        probe.returned.clear();
        probe.tap(key_data(*vk, *native, *character, *mods));
        probe.settle();
        // The engine did none of the work (the field and the pasteboard are as they were) and
        // gave the press back, with its modifiers, and the release after it.
        let back: Vec<_> = probe
            .returned
            .iter()
            .filter(|(_, key, _)| key == vk)
            .map(|(kind, _, mods)| (*kind, *mods))
            .collect();
        assert_eq!(
            (probe.title.as_str(), back),
            (
                before.as_str(),
                vec![(KeyKind::Down, *mods), (KeyKind::Up, *mods)]
            ),
            "{name}"
        );
    }
    // A page that takes ⌘Z (its own undo: `preventDefault`) is not given it back, and the key is
    // the page's alone.
    probe.load(
        "<title>init</title><body><script>document.addEventListener('keydown',e=>{\
         if(e.metaKey&&e.key=='z'){e.preventDefault();document.title='mine'}})</script>",
    );
    probe.click(100, 150);
    probe.drain(Duration::from_millis(300));
    probe.settle();
    probe.returned.clear();
    probe.tap(key_data(90, 6, 'z' as u16, m::COMMAND));
    probe.settle();
    assert_eq!(probe.title, "mine");
    assert!(
        !probe
            .returned
            .iter()
            .any(|(kind, key, _)| *kind == KeyKind::Down && *key == 90),
        "a key the page took was given back: {:?}",
        probe.returned
    );
    let _ = std::fs::remove_dir_all(&probe.root);
}

/// A trackpad pinch with the real engine: the page has it first as `wheel` events with `ctrlKey`
/// (the way Chrome on a Mac gives it to a map), and a page that does not take it is zoomed
/// visually, its layout as it was. That the protocol bateri speaks (`Command::Pinch`) is the one
/// the helper reads is what is measured; how the point stays under the fingers is the helper's
/// own tests'. Measured on the helper of `browser` rev 0990358
/// (`BATERI_BROWSER_APP=… cargo test -p bt-shell-macos --lib the_real_engine_takes_a_pinch`).
#[test]
fn the_real_engine_takes_a_pinch_as_ctrl_wheel_events_and_zooms_a_page_that_leaves_it() {
    use browser_ipc::message::{Command, Event, PinchKind};
    let Some(app) = std::env::var_os("BATERI_BROWSER_APP").map(PathBuf::from) else {
        return;
    };
    let mut probe = Probe::start("pinch", app);
    // The title says nothing until the first `wheel` event, so the load's own title is `init`.
    probe.load(
        "<title>init</title><body style='margin:0;width:3000px;height:3000px'><script>\
         let n=0,c='-',t=0;addEventListener('wheel',e=>{n++;c=e.ctrlKey;if(!t)t=setInterval(()=>{\
         document.title='n='+n+' ctrl='+c+' s='+visualViewport.scale.toFixed(2)+' iw='+innerWidth},50)},\
         {passive:false})</script>",
    );
    let id = probe.browser.id();
    let pinch = |kind, scale| {
        probe.send(Command::Pinch {
            browser: id,
            kind,
            x: 200,
            y: 150,
            scale,
        })
    };
    pinch(PinchKind::Begin, 1.0);
    for _ in 0..4 {
        pinch(PinchKind::Update, 2f64.powf(0.25));
    }
    pinch(PinchKind::End, 1.0);
    probe.until(
        "the page zoomed to twice",
        |e| matches!(e, Event::State { title, .. } if title.contains("s=2.00")),
    );
    assert!(probe.title.contains("ctrl=true"), "{}", probe.title);
    assert!(probe.title.contains("iw=400"), "{}", probe.title);
    let _ = std::fs::remove_dir_all(&probe.root);
}

/// The two things a swipe between pages and a mouse's side buttons need of the engine, with the
/// real one: the helper says whether a sideways scroll at a point moves the page (`Command::CanScroll`:
/// not at the left edge of a wide page for the way back, yes for the way forward), and a side
/// button, pressed and let go over the page, goes back (`Command::Mouse` with `Button::Back`: the
/// page gets it as `button` 3 and the engine goes on the release). How the boxes, the frames and a
/// page that takes the `mouseup` are told is the helper's own tests'. Measured on the helper of
/// `browser` rev 24eb092 (`BATERI_BROWSER_APP=… cargo test -p bt-shell-macos --lib the_real_engine_answers`).
#[test]
fn the_real_engine_answers_whether_a_page_scrolls_sideways_and_goes_back_on_a_side_button() {
    use browser_ipc::message::{Button, Command, Event, MouseKind};
    let Some(app) = std::env::var_os("BATERI_BROWSER_APP").map(PathBuf::from) else {
        return;
    };
    let mut probe = Probe::start("sideways", app);
    probe.load("<title>init</title><body style='margin:0;width:3000px;height:600px'>first");
    let id = probe.browser.id();
    let ask = |probe: &mut Probe, req: u32, dx: i32| {
        probe.send(Command::CanScroll {
            browser: id,
            req,
            x: 50,
            y: 50,
            dx,
        });
        let mut answer = None;
        probe.until("the answer", |e| match e {
            Event::CanScroll { req: r, can, .. } if *r == req => {
                answer = Some(*can);
                true
            }
            _ => false,
        });
        answer.unwrap()
    };
    // At the left edge of a wide page the content cannot move right (that way is history's), and
    // it can move left.
    assert!(!ask(&mut probe, 1, 100));
    assert!(ask(&mut probe, 2, -100));
    // A second page, and the side button back to the first.
    probe.send(Command::Navigate {
        browser: id,
        url: format!(
            "data:text/html,{}",
            percent("<title>second</title><body style='margin:0;height:600px'>second")
        ),
    });
    probe.until(
        "the second page",
        |e| matches!(e, Event::State { title, loading: false, .. } if title == "second"),
    );
    probe.drain(Duration::from_millis(500));
    for kind in [MouseKind::Move, MouseKind::Down, MouseKind::Up] {
        probe.send(Command::Mouse {
            browser: id,
            kind,
            x: 50,
            y: 50,
            button: if kind == MouseKind::Move {
                Button::Left
            } else {
                Button::Back
            },
            clicks: 1,
            mods: 0,
        });
    }
    probe.until(
        "the first page again",
        |e| matches!(e, Event::State { title, loading: false, .. } if title == "init"),
    );
    let _ = std::fs::remove_dir_all(&probe.root);
}

/// The menu of a link with the real engine: it starts with the rows that open the link and copy
/// its address, and the words of the first are by where bateri opens a link (`Command::Windows`):
/// "Open Link in New Tab" for a tab of the page, "Open Link" where the browser goes there itself.
/// Picking it is a window asked for in the background, which bateri answers as the room says.
/// Measured on the helper of `browser` rev b6ed148
/// (`BATERI_BROWSER_APP=… cargo test -p bt-shell-macos --lib the_real_engines_link_menu`).
#[test]
fn the_real_engines_link_menu_starts_with_rows_worded_by_where_windows_go() {
    use browser_ipc::message::{Button, Command, Event, MouseKind, NewWindows};
    let Some(app) = std::env::var_os("BATERI_BROWSER_APP").map(PathBuf::from) else {
        return;
    };
    let mut probe = Probe::start("linkmenu", app);
    probe.load(
        "<title>init</title><body style='margin:0'><a href='http://other.test/x' \
         style='position:absolute;left:10px;top:10px;font-size:30px'>a link</a>",
    );
    let id = probe.browser.id();
    let ask = |probe: &mut Probe, mode: NewWindows| -> (u32, Vec<String>) {
        probe.send(Command::Windows { browser: id, mode });
        for kind in [MouseKind::Move, MouseKind::Down, MouseKind::Up] {
            probe.send(Command::Mouse {
                browser: id,
                kind,
                x: 30,
                y: 30,
                button: Button::Right,
                clicks: 1,
                mods: 0,
            });
        }
        let mut found = None;
        probe.until("the menu", |e| {
            if let Event::Menu { req, items, .. } = e {
                found = Some((
                    *req,
                    items.iter().map(|m| m.label.clone()).collect::<Vec<_>>(),
                ));
            }
            found.is_some()
        });
        found.unwrap()
    };
    let (req, labels) = ask(&mut probe, NewWindows::Page);
    assert_eq!(labels[..2], ["Open Link in New Tab", "Copy Link Address"]);
    probe.send(Command::MenuResult { req, command: None });
    let (req, labels) = ask(&mut probe, NewWindows::Here);
    assert_eq!(labels[..2], ["Open Link", "Copy Link Address"]);
    probe.send(Command::MenuResult { req, command: None });
    let _ = std::fs::remove_dir_all(&probe.root);
}

/// The whole way of a failure: a page of a machine the proxy has no way to is a 502 of the
/// proxy's with the header, and the real helper makes of it the page's load error with the code
/// the proxy named (the helper's side is tested in its own repository against a stand-in proxy;
/// this is bateri's proxy against the helper).
#[test]
fn the_real_engine_reads_the_proxys_failure_header_as_the_pages_load_error() {
    use browser_ipc::message::{Command, Event};
    let Some(app) = std::env::var_os("BATERI_BROWSER_APP").map(PathBuf::from) else {
        return;
    };
    let mut probe = Probe::start("noway", app);
    let id = probe.browser.id();
    probe.send(Command::Navigate {
        browser: id,
        url: "http://nowhere.test/".to_owned(),
    });
    let mut failed = None;
    probe.until("the load error", |event| {
        if let Event::State {
            error: Some(error), ..
        } = event
        {
            failed = Some(error.clone());
            true
        } else {
            false
        }
    });
    let error = failed.expect("an error");
    assert_eq!(error.url, "http://nowhere.test/");
    assert_eq!(error.code, -130, "no way to the machine: {error:?}");
    let _ = std::fs::remove_dir_all(&probe.root);
}
