//! The browser helper on this Mac: finding it, downloading it, checking it, running it. The rules
//! are `bt_shell_common::browser`'s; this is what needs the platform: `NSURLSession`
//! ([`fetch`]), `ditto`, the signature ([`signature`]) and the process.
//!
//! **No download without the person's word.** [`Installer::check`] reads the release notice and
//! says what is on offer; [`Installer::install`] downloads and is called after the person has
//! said yes ("Download the browser engine?", with the size the notice gives). Once an engine is
//! installed, that yes covers its updates: [`Installer::update_if_due`] looks once a day and
//! installs a newer one, which runs from the next start of the helper.
//!
//! **What is trusted.** The notice tells where the archive is and its size and SHA-256; the
//! release's `SHA256SUMS` must say the same; the archive is checked against both before it is
//! unpacked, and the unpacked app's signature is checked before it is put in place and again, at
//! its own path, just before it is run. A notice is never believed beyond that: it cannot lower
//! the floor, name a protocol bateri does not speak, or offer a version older than what is
//! installed (`notice::judge`).
//!
//! **The data** is `~/Library/Application Support/bateri[-profile]/browser/`: `helper/` (one
//! folder per version), `profiles/` (the helper's `--root`) and `run/` (the socket and the
//! helper's log), all `0700`.

mod fetch;
pub(crate) mod signature;
#[cfg(test)]
mod tests;

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime};

use browser_ipc::PROTOCOL;
use browser_ipc::manifest::Manifest;
use bt_shell_common::browser::helper::{self, Config, Manager, Process, Source, Spawn, Trust};
use bt_shell_common::browser::layout::{self, Layout};
use bt_shell_common::browser::notice::{self, Offer, Refusal, Verdict};
use bt_shell_common::browser::version::Version;
use bt_shell_common::digest::{parse_sums, sha256_file};

/// Where a release's files are: the newest release's, by name. The notice, `SHA256SUMS` and the
/// archive are fetched from one place, so a release that is replaced between two of the fetches
/// fails the digest and is tried again at the next check.
pub(crate) const RELEASES: &str = "https://github.com/bateri/browser/releases/latest/download";
const NOTICE: &str = "notice.json";
const SUMS: &str = "SHA256SUMS";
/// A notice and a list of sums are a few hundred bytes; nothing longer is one.
const SMALL: usize = 64 * 1024;
/// How often an installed engine asks whether there is a newer one.
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
const CHECKED: &str = "checked";

/// What the release notice comes to.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Check {
    /// Newer than what is installed, or nothing is installed.
    Offer(Offer),
    /// The installed engine is the release's.
    Current,
    /// The notice is not one to follow.
    Refused(Refusal),
    /// The notice could not be had or read (offline, no release yet, a Mac with no archive): not
    /// a failure of anything the person did, so it is a quiet state.
    Unavailable(String),
}

#[derive(Debug)]
pub(crate) enum InstallError {
    /// The person stopped it.
    Cancelled,
    /// The archive is not what the notice and the list of sums say.
    Digest(String),
    /// The unpacked app is not signed by us.
    Signature(String),
    Io(io::Error),
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstallError::Cancelled => f.write_str("the download was stopped"),
            InstallError::Digest(why) => write!(f, "the download is not the release: {why}"),
            InstallError::Signature(why) => write!(f, "the download is not signed by us: {why}"),
            InstallError::Io(e) => write!(f, "the download failed: {e}"),
        }
    }
}

impl From<io::Error> for InstallError {
    fn from(e: io::Error) -> Self {
        if e.kind() == io::ErrorKind::Interrupted {
            InstallError::Cancelled
        } else {
            InstallError::Io(e)
        }
    }
}

pub(crate) struct Installer {
    layout: Layout,
    /// The release's folder (a URL without the file's name).
    base: String,
    trust: Trust,
}

impl Installer {
    pub(crate) fn new(layout: Layout, base: &str, trust: Trust) -> Self {
        Self {
            layout,
            base: base.trim_end_matches('/').to_owned(),
            trust,
        }
    }

    #[cfg(test)]
    pub(crate) fn layout(&self) -> &Layout {
        &self.layout
    }

    /// Reads the release notice and says what it comes to. Fetches nothing else.
    pub(crate) fn check(&self) -> Check {
        let url = format!("{}/{NOTICE}", self.base);
        let bytes = match fetch::bytes(&url, SMALL) {
            Ok(bytes) => bytes,
            Err(e) => return Check::Unavailable(format!("the release notice: {e}")),
        };
        let manifest = match Manifest::parse(&bytes) {
            Ok(manifest) => manifest,
            Err(e) => return Check::Unavailable(format!("the release notice: {e}")),
        };
        let Some(offer) = notice::offer(&manifest, notice::arch()) else {
            return Check::Unavailable(format!("the release has no engine for {}", notice::arch()));
        };
        match notice::judge(
            offer,
            PROTOCOL,
            &self.layout.floor(),
            self.layout.newest().as_ref(),
        ) {
            Verdict::Install(offer) => Check::Offer(offer),
            Verdict::Current => Check::Current,
            Verdict::Refused(refusal) => Check::Refused(refusal),
        }
    }

    /// Downloads, checks and puts in place what `offer` names. Slow: a thread of its own.
    /// `progress` hears bytes received and the total; `cancel` set stops it.
    pub(crate) fn install(
        &self,
        offer: &Offer,
        progress: fetch::Progress<'_>,
        cancel: &AtomicBool,
    ) -> Result<Version, InstallError> {
        let sums = fetch::bytes(&format!("{}/{SUMS}", self.base), SMALL)?;
        let sums = parse_sums(&String::from_utf8_lossy(&sums));
        if sums.get(&offer.asset.name) != Some(&offer.asset.sha256) {
            return Err(InstallError::Digest(format!(
                "SHA256SUMS does not list {} as the notice does",
                offer.asset.name
            )));
        }
        let staging = self.layout.staging()?;
        let archive = staging.path().join(&offer.asset.name);
        fetch::file(
            &format!("{}/{}", self.base, offer.asset.name),
            &archive,
            progress,
            cancel,
        )?;
        let (digest, length) = sha256_file(&archive)?;
        if length != offer.asset.size || digest != offer.asset.sha256 {
            return Err(InstallError::Digest(format!(
                "{} is {length} bytes of {digest}, not what the notice names",
                offer.asset.name
            )));
        }
        unpack(&archive, staging.path())?;
        std::fs::remove_file(&archive)?;
        (self.trust)(&staging.path().join(layout::APP)).map_err(InstallError::Signature)?;
        self.layout
            .commit(&staging, &offer.version, &offer.minimum)?;
        self.layout.prune();
        Ok(offer.version.clone())
    }

    /// When an engine is installed and the last look at the notice is a day old: looks, and
    /// installs a newer one. `None`: nothing was due.
    pub(crate) fn update_if_due(
        &self,
        progress: fetch::Progress<'_>,
        cancel: &AtomicBool,
    ) -> Option<Result<Version, InstallError>> {
        self.layout.newest()?;
        let stamp = self.layout.root().join(CHECKED);
        let due = std::fs::metadata(&stamp)
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|time| SystemTime::now().duration_since(time).ok())
            .is_none_or(|age| age >= CHECK_EVERY);
        if !due {
            return None;
        }
        match self.check() {
            Check::Offer(offer) => {
                // The stamp says "looked and done": a download that failed or was stopped is
                // tried again at the next start and not in a day.
                let installed = self.install(&offer, progress, cancel);
                if installed.is_ok() {
                    let _ = std::fs::write(&stamp, "");
                }
                Some(installed)
            }
            Check::Unavailable(_) => None,
            Check::Current | Check::Refused(_) => {
                let _ = std::fs::write(&stamp, "");
                None
            }
        }
    }
}

/// `ditto` unpacks a release's archive (a zip made by `ditto -c -k --keepParent`, which keeps the
/// signature's extended attributes and links as they were).
fn unpack(archive: &Path, into: &Path) -> Result<(), InstallError> {
    let output = Command::new("/usr/bin/ditto")
        .arg("-x")
        .arg("-k")
        .arg(archive)
        .arg(into)
        .stdin(Stdio::null())
        .output()?;
    if !output.status.success() {
        return Err(InstallError::Io(io::Error::other(format!(
            "ditto: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))));
    }
    Ok(())
}

/// The signature check the product runs: the app is signed by the team bateri is signed by. A
/// bateri with no team refuses (it could not say whose the engine must be).
pub(crate) fn team_trust() -> Trust {
    Arc::new(|app| {
        static TEAM: OnceLock<Option<String>> = OnceLock::new();
        let team = TEAM
            .get_or_init(signature::own_team)
            .as_deref()
            .ok_or("this bateri has no signing team to hold the engine to")?;
        signature::verify(app, &signature::requirement_for_team(team)?)
    })
}

/// Starts the helper as a child of bateri: its own environment (nothing of bateri's, which may
/// hold secrets and `DYLD_*`), no input, its words in the run folder's log.
pub(crate) struct Spawner {
    pub(crate) log: PathBuf,
}

struct Running(Child);

impl Process for Running {
    fn id(&self) -> u32 {
        self.0.id()
    }

    fn exited(&mut self) -> io::Result<Option<String>> {
        Ok(self.0.try_wait()?.map(|status| status.to_string()))
    }

    fn kill(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Spawn for Spawner {
    fn spawn(&self, executable: &Path, args: &[String]) -> io::Result<Box<dyn Process>> {
        let mut command = Command::new(executable);
        command
            .args(args)
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::null());
        for name in [
            "HOME",
            "TMPDIR",
            "USER",
            "LOGNAME",
            "LANG",
            "LC_ALL",
            "LC_CTYPE",
            "__CF_USER_TEXT_ENCODING",
        ] {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        command.env("PATH", "/usr/bin:/bin");
        let log = std::fs::File::options()
            .create(true)
            .write(true)
            .truncate(true)
            .mode_0600()
            .open(&self.log);
        command.stderr(log.map_or_else(|_| Stdio::null(), Stdio::from));
        Ok(Box::new(Running(command.spawn()?)))
    }
}

trait Mode0600 {
    fn mode_0600(&mut self) -> &mut Self;
}

impl Mode0600 for std::fs::OpenOptions {
    fn mode_0600(&mut self) -> &mut Self {
        std::os::unix::fs::OpenOptionsExt::mode(self, 0o600)
    }
}

/// The browser data folder of a copy: `bateri/browser`, or `bateri-{profile}/browser` for a
/// development copy (the same split as the Apps folder: a copy with settings of its own does not
/// share an engine's data with the installed one).
pub(crate) fn browser_path(home: &Path, profile: Option<&str>) -> PathBuf {
    let folder = match bt_shell_common::settings::plain_profile(profile) {
        Some(profile) => format!("bateri-{profile}"),
        None => "bateri".to_owned(),
    };
    home.join("Library/Application Support")
        .join(folder)
        .join("browser")
}

/// The helper app a development copy runs where it is (`make dev BROWSER_APP=…` writes it into
/// the copy's `Info.plist` as `BateriBrowserApp`); `None` for every other copy.
fn development_app(profile: Option<&str>) -> Option<PathBuf> {
    profile.filter(|profile| *profile == "dev")?;
    let value = objc2_foundation::NSBundle::mainBundle()
        .objectForInfoDictionaryKey(objc2_foundation::ns_string!("BateriBrowserApp"))?;
    let path = value
        .downcast::<objc2_foundation::NSString>()
        .ok()?
        .to_string();
    Some(PathBuf::from(path))
}

/// The browser helper's manager: one for the process. `None` without a home directory.
pub(crate) fn manager() -> Option<Arc<Manager>> {
    static MANAGER: OnceLock<Option<Arc<Manager>>> = OnceLock::new();
    MANAGER
        .get_or_init(|| {
            let profile = crate::profile();
            let root = browser_path(&bt_shell_common::child::home()?, profile.as_deref());
            let fixed = development_app(profile.as_deref());
            let config = Config {
                source: fixed.clone().map_or_else(
                    || Source::Installed(Layout::new(root.join("helper"))),
                    Source::Fixed,
                ),
                profiles: root.join("profiles"),
                run: root.join("run"),
                // A copy run where it is is a development copy: its cookies are not worth the
                // keychain's question on every build.
                mock_keychain: fixed.is_some(),
                linger: helper::LINGER,
                start_timeout: helper::START_TIMEOUT,
                trust: if fixed.is_some() {
                    Arc::new(|_| Ok(()))
                } else {
                    team_trust()
                },
            };
            let spawner = Spawner {
                log: root.join("run").join("helper.log"),
            };
            Some(Manager::new(config, Arc::new(spawner)))
        })
        .clone()
}

/// The installer of the engine this copy would run; `None` without a home directory or for a
/// development copy that runs an app where it is (nothing is installed for it).
pub(crate) fn installer() -> Option<Installer> {
    let profile = crate::profile();
    if development_app(profile.as_deref()).is_some() {
        return None;
    }
    let root = browser_path(&bt_shell_common::child::home()?, profile.as_deref());
    Some(Installer::new(
        Layout::new(root.join("helper")),
        RELEASES,
        team_trust(),
    ))
}
