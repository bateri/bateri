//! Where the installed helpers are, and which of them stays (Karar 5).
//!
//! One folder per version under a root only this user can enter (`0700`): the helper is
//! started from here, so another user able to write here could be a helper. A version is
//! installed when its folder holds the app; it comes into place by one rename out of a staging
//! folder of its own, so a download that stops halfway is never taken for one. What is below the
//! newest version and no running helper uses is deleted ([`Layout::prune`]): a helper holds a
//! shared lock on its version's `.in-use` while it runs, and prune takes that lock to delete,
//! so it can neither delete a version in use (of this bateri or of another one that shares the
//! folder) nor race the helper's start.
//!
//! Beside the folders sits `floor`: the highest version number any notice has named as the oldest
//! to run and the highest ever installed. Nothing below it is installed again (`notice::judge`).

use std::fs::{self, File};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, SystemTime};

use super::version::Version;

/// The app inside a version's folder.
pub const APP: &str = "bateri-browser.app";
/// The helper's executable inside the app.
pub const EXECUTABLE: &str = "Contents/MacOS/bateri-browser";
const IN_USE: &str = ".in-use";
const FLOOR: &str = "floor";
const STAGING: &str = ".staging-";
/// A staging folder nobody has written to for this long is the leftover of a stopped install.
const STAGING_LIFE: Duration = Duration::from_secs(60 * 60);

static STAGINGS: AtomicU32 = AtomicU32::new(0);

#[derive(Clone, Debug)]
pub struct Layout {
    root: PathBuf,
}

/// A folder the app is unpacked into before it is a version. Removed when dropped.
pub struct Staging {
    path: PathBuf,
}

impl Staging {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// A running helper's claim on its version: while it lives, the version is not pruned.
pub struct Hold {
    _file: File,
}

impl Layout {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn version_dir(&self, version: &Version) -> PathBuf {
        self.root.join(version.to_string())
    }

    pub fn app(&self, version: &Version) -> PathBuf {
        self.version_dir(version).join(APP)
    }

    pub fn executable(&self, version: &Version) -> PathBuf {
        self.app(version).join(EXECUTABLE)
    }

    /// The versions that are installed, oldest first.
    pub fn installed(&self) -> Vec<Version> {
        let Ok(entries) = fs::read_dir(&self.root) else {
            return Vec::new();
        };
        let mut versions: Vec<Version> = entries
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name();
                let name = name.to_str()?;
                let version = Version::parse(name)?;
                (version.to_string() == name && self.app(&version).is_dir()).then_some(version)
            })
            .collect();
        versions.sort();
        versions
    }

    pub fn newest(&self) -> Option<Version> {
        self.installed().pop()
    }

    /// The root, made `0700` if missing; refused if others can enter it.
    fn private_root(&self) -> io::Result<()> {
        make_private(&self.root)
    }

    /// An empty folder of its own to unpack into.
    pub fn staging(&self) -> io::Result<Staging> {
        self.private_root()?;
        let path = self.root.join(format!(
            "{STAGING}{}-{}",
            std::process::id(),
            STAGINGS.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path)?;
        Ok(Staging { path })
    }

    /// Puts the app unpacked in `staging` (`APP`, by one rename) in `version`'s place, and raises
    /// the floor to `minimum` or `version`, whichever is higher. A folder already there (a
    /// stopped install's, or a damaged one) is taken away first.
    pub fn commit(
        &self,
        staging: &Staging,
        version: &Version,
        minimum: &Version,
    ) -> io::Result<()> {
        let unpacked = staging.path().join(APP);
        if !unpacked.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("the archive holds no {APP}"),
            ));
        }
        let place = self.version_dir(version);
        match fs::remove_dir_all(&place) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        fs::DirBuilder::new().mode(0o700).create(&place)?;
        if let Err(e) = fs::rename(&unpacked, place.join(APP)) {
            let _ = fs::remove_dir_all(&place);
            return Err(e);
        }
        self.raise_floor(minimum.max(version))
    }

    /// The highest version bateri will not go below (zero when none has been set).
    pub fn floor(&self) -> Version {
        fs::read_to_string(self.root.join(FLOOR))
            .ok()
            .and_then(|text| Version::parse(text.trim()))
            .unwrap_or_else(Version::zero)
    }

    /// Raises the floor to `version`; a lower one changes nothing.
    pub fn raise_floor(&self, version: &Version) -> io::Result<()> {
        if *version <= self.floor() {
            return Ok(());
        }
        self.private_root()?;
        let temporary = self.root.join(format!("{FLOOR}.{}", std::process::id()));
        fs::write(&temporary, version.to_string())?;
        fs::rename(&temporary, self.root.join(FLOOR))
    }

    /// Claims `version` for a helper about to run from it.
    pub fn hold(&self, version: &Version) -> io::Result<Hold> {
        let file = File::options()
            .create(true)
            .append(true)
            .open(self.version_dir(version).join(IN_USE))?;
        flock(&file, libc::LOCK_SH)?;
        Ok(Hold { _file: file })
    }

    /// Deletes what is below the newest version and not in use, and the staging folders a stopped
    /// install left; returns the versions deleted.
    pub fn prune(&self) -> Vec<Version> {
        let mut deleted = Vec::new();
        let installed = self.installed();
        let Some(newest) = installed.last().cloned() else {
            return deleted;
        };
        for version in installed.iter().filter(|version| **version < newest) {
            let dir = self.version_dir(version);
            let Ok(claim) = File::options()
                .create(true)
                .append(true)
                .open(dir.join(IN_USE))
            else {
                continue;
            };
            // Exclusive and without waiting: a helper holding its version shared says it is in use.
            if flock(&claim, libc::LOCK_EX | libc::LOCK_NB).is_ok()
                && fs::remove_dir_all(&dir).is_ok()
            {
                deleted.push(version.clone());
            }
        }
        if let Ok(entries) = fs::read_dir(&self.root) {
            for entry in entries.flatten() {
                let stale = entry.file_name().to_string_lossy().starts_with(STAGING)
                    && entry
                        .metadata()
                        .and_then(|meta| meta.modified())
                        .ok()
                        .and_then(|time| SystemTime::now().duration_since(time).ok())
                        .is_some_and(|age| age > STAGING_LIFE);
                if stale {
                    let _ = fs::remove_dir_all(entry.path());
                }
            }
        }
        deleted
    }
}

/// `dir` and its parents, made `0700`; refused when it exists and others can enter it.
pub fn make_private(dir: &Path) -> io::Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    let mode = fs::metadata(dir)?.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{} is open to other users (mode {mode:o})", dir.display()),
        ));
    }
    Ok(())
}

fn flock(file: &File, operation: libc::c_int) -> io::Result<()> {
    loop {
        // SAFETY: the descriptor is the open file's, which outlives the call.
        if unsafe { libc::flock(file.as_raw_fd(), operation) } == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bateri-layout-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    /// Installs a version the way the installer does: unpack into a staging folder, commit.
    fn install(layout: &Layout, version: &str, minimum: &str) {
        let staging = layout.staging().unwrap();
        fs::create_dir_all(staging.path().join(APP).join("Contents/MacOS")).unwrap();
        fs::write(staging.path().join(APP).join(EXECUTABLE), version).unwrap();
        layout.commit(&staging, &v(version), &v(minimum)).unwrap();
    }

    #[test]
    fn installed_versions_are_listed_oldest_first_and_compared_as_numbers() {
        let root = temp("list");
        let layout = Layout::new(&root);
        assert_eq!(layout.newest(), None);
        install(&layout, "9.0.0", "0");
        install(&layout, "10.0.0", "0");
        install(&layout, "0.2.0", "0");
        assert_eq!(
            layout.installed(),
            vec![v("0.2.0"), v("9.0.0"), v("10.0.0")]
        );
        assert_eq!(layout.newest(), Some(v("10.0.0")));
        assert_eq!(
            fs::read_to_string(layout.executable(&v("9.0.0"))).unwrap(),
            "9.0.0"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_folder_without_the_app_is_not_a_version() {
        let root = temp("half");
        let layout = Layout::new(&root);
        install(&layout, "0.1.0", "0");
        fs::create_dir_all(root.join("0.2.0")).unwrap();
        fs::create_dir_all(root.join("junk")).unwrap();
        fs::create_dir_all(root.join("0.3.0").join("elsewhere")).unwrap();
        assert_eq!(layout.installed(), vec![v("0.1.0")]);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_floor_only_goes_up_and_an_install_raises_it() {
        let root = temp("floor");
        let layout = Layout::new(&root);
        assert_eq!(layout.floor(), Version::zero());
        install(&layout, "0.3.0", "0.2.0");
        assert_eq!(layout.floor(), v("0.3.0"));
        install(&layout, "0.5.0", "0.4.0");
        assert_eq!(layout.floor(), v("0.5.0"));
        layout.raise_floor(&v("0.1.0")).unwrap();
        assert_eq!(layout.floor(), v("0.5.0"));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_commit_without_the_app_is_refused_and_leaves_nothing() {
        let root = temp("empty");
        let layout = Layout::new(&root);
        let staging = layout.staging().unwrap();
        let path = staging.path().to_owned();
        assert!(layout.commit(&staging, &v("0.1.0"), &v("0")).is_err());
        assert!(layout.installed().is_empty());
        assert_eq!(layout.floor(), Version::zero());
        drop(staging);
        assert!(!path.exists(), "the staging folder goes with its owner");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn prune_deletes_what_is_older_and_unused_and_keeps_what_runs() {
        let root = temp("prune");
        let layout = Layout::new(&root);
        for version in ["0.1.0", "0.2.0", "0.3.0"] {
            install(&layout, version, "0");
        }
        let running = layout.hold(&v("0.2.0")).unwrap();
        assert_eq!(layout.prune(), vec![v("0.1.0")]);
        assert_eq!(layout.installed(), vec![v("0.2.0"), v("0.3.0")]);
        // The newest is never pruned, in use or not; the older one goes once nothing runs from it.
        drop(running);
        assert_eq!(layout.prune(), vec![v("0.2.0")]);
        assert_eq!(layout.installed(), vec![v("0.3.0")]);
        assert!(layout.prune().is_empty());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_root_is_private_and_a_root_others_can_enter_is_refused() {
        let root = temp("mode");
        let layout = Layout::new(&root);
        install(&layout, "0.1.0", "0");
        assert_eq!(
            fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o700
        );
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(layout.staging().is_err());
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        fs::remove_dir_all(&root).unwrap();
    }
}
