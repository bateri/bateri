//! What the apps folder holds, and the only code that reads or writes it.
//!
//! ```text
//! <root>/                          ~/Library/Application Support/bateri/apps
//!   <name>/
//!     source                       the address of the app.json this name follows (one line)
//!     key                          the key its releases are signed with, once approved (one line)
//!     <version>/                   a release that landed whole
//!       app.json, app.json.sig     the release as it was fetched
//!       <target>                   the program for a target, e.g. aarch64-apple-darwin
//! ```
//!
//! A release folder is made beside the others under a temporary name and renamed: a folder with a
//! version for a name is one that is complete. The files `source` and `key` are one line each,
//! written the same way; a reader takes the first line and nothing else.
//!
//! The folder is **the record**: there is no list of what was downloaded anywhere else, so a
//! folder deleted by hand is a release not downloaded, and the next check fetches it.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bt_core::pencere::apps::Version;

/// How long a release that is not the newest two stays.
pub const KEEP: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// The folder of a program.
pub fn app_dir(root: &Path, name: &str) -> PathBuf {
    root.join(name)
}

/// The folder of one release of a program.
pub fn release_dir(root: &Path, name: &str, version: Version) -> PathBuf {
    app_dir(root, name).join(version.to_string())
}

/// The program's file for `target` in a release's folder.
pub fn program_path(root: &Path, name: &str, version: Version, target: &str) -> PathBuf {
    release_dir(root, name, version).join(target)
}

/// A name that no other call has had: not in this process (a counter), not in another one (its
/// process id), not on another computer or after a restart (the clock). For the temporary files
/// and folders a download or an upload is built in, so that two at once do not meet in one.
pub fn unique() -> String {
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    format!(
        "{:x}-{nanos:x}-{:x}",
        std::process::id(),
        COUNT.fetch_add(1, Ordering::Relaxed)
    )
}

/// The first line of a small text file, trimmed; `None` if it is missing or empty.
fn first_line(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let line = text.lines().next()?.trim();
    (!line.is_empty()).then(|| line.to_owned())
}

/// A small file written whole under a temporary name and renamed over the old one.
pub fn write_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(format!(".part-{}", unique()));
    let temporary = PathBuf::from(temporary);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::File::create(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, path).inspect_err(|_| {
        let _ = fs::remove_file(&temporary);
    })
}

/// The key approved for `name`, as the file keeps it (`ed25519:` and base64).
pub fn approved_key(root: &Path, name: &str) -> Option<String> {
    first_line(&app_dir(root, name).join("key"))
}

/// Approves `key` for `name`: its releases are taken from now on when they are signed with it,
/// and with no other. The settings page's Approve does this (and writes the source into the
/// settings); until it exists, so does a person with a text editor.
pub fn approve(root: &Path, name: &str, key: &str) -> io::Result<()> {
    write_atomically(
        &app_dir(root, name).join("key"),
        format!("{key}\n").as_bytes(),
    )
}

/// The address of the `app.json` `name` follows, once one of its releases has landed.
pub fn followed(root: &Path, name: &str) -> Option<String> {
    first_line(&app_dir(root, name).join("source"))
}

/// Records that `name` follows `address`.
pub fn follow(root: &Path, name: &str, address: &str) -> io::Result<()> {
    write_atomically(
        &app_dir(root, name).join("source"),
        format!("{address}\n").as_bytes(),
    )
}

/// The names that follow an address (any), with theirs: every program folder with a `source`.
pub fn followed_names(root: &Path) -> Vec<(String, String)> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut found: Vec<(String, String)> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let address = followed(root, &name)?;
            Some((name, address))
        })
        .collect();
    found.sort();
    found
}

/// The program that follows `address`, if one does.
pub fn name_following(root: &Path, address: &str) -> Option<String> {
    followed_names(root)
        .into_iter()
        .find(|(_, followed)| followed == address)
        .map(|(name, _)| name)
}

/// A release that landed, and when.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Landed {
    /// Its version.
    pub version: Version,
    /// When its `app.json` was written: how old the release is.
    pub at: SystemTime,
}

/// The releases of `name` that landed, the oldest version first.
pub fn landed(root: &Path, name: &str) -> Vec<Landed> {
    let Ok(entries) = fs::read_dir(app_dir(root, name)) else {
        return Vec::new();
    };
    let mut found: Vec<Landed> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let version = Version::parse(entry.file_name().to_str()?)?;
            let at = fs::metadata(entry.path().join("app.json"))
                .ok()?
                .modified()
                .ok()?;
            Some(Landed { version, at })
        })
        .collect();
    found.sort_by_key(|release| release.version);
    found
}

/// The newest release of `name` that landed.
pub fn newest(root: &Path, name: &str) -> Option<Version> {
    landed(root, name).last().map(|release| release.version)
}

/// Keeps the release in use, the one before it, and whatever is newer than [`KEEP`]; deletes the
/// rest of `name`'s releases. Two machines that follow the same program must not delete what
/// the other still uses, and an hour-old release of an app that changes twice a day is not
/// stale.
pub fn prune(root: &Path, name: &str, now: SystemTime) {
    let releases = landed(root, name);
    let keep_from = releases.len().saturating_sub(2);
    for (index, release) in releases.iter().enumerate() {
        let recent = now
            .duration_since(release.at)
            .map_or(true, |age| age < KEEP);
        if index < keep_from && !recent {
            let _ = fs::remove_dir_all(release_dir(root, name, release.version));
        }
    }
}

/// Takes `name` off this folder: its folder is deleted at once.
pub fn remove(root: &Path, name: &str) -> io::Result<()> {
    fs::remove_dir_all(app_dir(root, name))
}
