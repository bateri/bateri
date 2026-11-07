//! One check of the sources: the rules of [`super`], written on [`Fetch`] and [`store`] so a test
//! puts a map of addresses where the network is and a temporary folder where the Mac's is.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::SystemTime;

use bt_core::app_source_address;
use bt_core::pencere::Digest;
use bt_core::pencere::apps::{
    self, File, MAX_MANIFEST_BYTES, Manifest, Previous, Selection, Version, signature_address,
};

use super::fetch::Fetch;
use super::store;
use super::{Config, Failure, RESERVED_NAMES, State, Status};

/// The largest program file a release may have: the real ones are a few megabytes; a limit only
/// keeps a wrong address from filling the disk.
const MAX_PROGRAM_BYTES: u64 = 256 * 1024 * 1024;

/// The signature is one short line (`apps.md`: at most 128 bytes); a longer answer is read as far
/// as the reader says no.
const MAX_SIGNATURE_BYTES: u64 = 1024;

/// Checks every source in order and lands what it can; returns how each one stands.
///
/// `authoritative` says `sources` is what the person wrote. Only then is a program that is not on
/// the list deleted ([`store::remove`]) — the list from a settings file that could not be read is
/// the defaults, which would delete a program the person added.
pub fn check(
    config: &Config,
    sources: &[String],
    authoritative: bool,
    fetch: &dyn Fetch,
    now: SystemTime,
) -> Vec<Status> {
    let addresses: Vec<Option<String>> = sources
        .iter()
        .map(|source| app_source_address(source))
        .collect();
    if authoritative {
        take_off_unlisted(&config.root, &addresses, now);
    }
    let cx = Cx { config, fetch, now };
    let mut taken: Vec<String> = Vec::new();
    sources
        .iter()
        .zip(&addresses)
        .map(|(source, address)| {
            let outcome = match address {
                Some(address) => cx.follow(address, &mut taken),
                None => Outcome::failed(None, Failure::Source),
            };
            Status {
                source: source.clone(),
                name: outcome.name,
                state: outcome.state,
                problem: outcome.problem,
            }
        })
        .collect()
}

/// Deletes the programs that follow an address the list does not have any more.
fn take_off_unlisted(root: &Path, listed: &[Option<String>], now: SystemTime) {
    for (name, address) in store::followed_names(root) {
        if !listed.iter().flatten().any(|listed| *listed == address)
            && let Err(e) = store::remove(root, &name, now)
        {
            eprintln!("bateri: could not remove the program {name}: {e}");
        }
    }
}

struct Cx<'a> {
    config: &'a Config,
    fetch: &'a dyn Fetch,
    now: SystemTime,
}

/// What one source came to.
struct Outcome {
    name: Option<String>,
    state: State,
    problem: Option<Failure>,
}

impl Outcome {
    fn new(name: &str, state: State) -> Self {
        Self {
            name: Some(name.to_owned()),
            state,
            problem: None,
        }
    }

    fn failed(name: Option<String>, failure: Failure) -> Self {
        Self {
            name,
            state: State::Error(failure),
            problem: None,
        }
    }
}

/// A release as it was fetched: what it says, and the bytes the signature is over.
struct Release {
    manifest: Manifest,
    bytes: Vec<u8>,
    signature: Vec<u8>,
}

/// Whether a release found through `source` may name `address`: a source on the network may
/// not send the reader to a file on this Mac (a manifest cannot read a file its author has no
/// business with), one on this Mac may name either.
fn reachable(source: &str, address: &str) -> bool {
    source.starts_with("file:") || address.starts_with("https://")
}

impl Cx<'_> {
    fn root(&self) -> &Path {
        &self.config.root
    }

    /// The `app.json` at `address` and its signature, read and verified.
    fn read(&self, address: &str) -> Result<Release, Failure> {
        let limit = MAX_MANIFEST_BYTES as u64;
        let bytes = self.fetch.get(address, limit).map_err(Failure::Fetch)?;
        let signature = self
            .fetch
            .get(&signature_address(address), MAX_SIGNATURE_BYTES)
            .map_err(Failure::Fetch)?;
        let manifest = apps::verify(&bytes, &signature).map_err(Failure::Manifest)?;
        Ok(Release {
            manifest,
            bytes,
            signature,
        })
    }

    /// What the folder says when the check could not go on: the newest release that landed, with
    /// the reason it could not, or the failure when nothing landed (or `name` is not known, or
    /// belongs to another source).
    fn resting(
        &self,
        address: &str,
        name: Option<String>,
        latest: Option<Version>,
        failure: Failure,
    ) -> Outcome {
        let name = name.or_else(|| store::name_following(self.root(), address));
        let landed = name
            .as_deref()
            .filter(|name| store::followed(self.root(), name).as_deref() == Some(address))
            .and_then(|name| store::newest(self.root(), name));
        match landed {
            Some(version) => Outcome {
                name,
                state: ready(version, latest),
                problem: Some(failure),
            },
            None => Outcome::failed(name, failure),
        }
    }

    fn follow(&self, address: &str, taken: &mut Vec<String>) -> Outcome {
        let release = match self.read(address) {
            Ok(release) => release,
            Err(failure) => return self.resting(address, None, None, failure),
        };
        let manifest = &release.manifest;
        let name = manifest.name.as_str();
        if RESERVED_NAMES.contains(&name) {
            return Outcome::failed(Some(name.to_owned()), Failure::Reserved);
        }
        let owner = store::followed(self.root(), name);
        if taken.iter().any(|taken| taken == name)
            || owner.as_deref().is_some_and(|owner| owner != address)
        {
            return Outcome::failed(Some(name.to_owned()), Failure::NameTaken);
        }
        taken.push(name.to_owned());

        let approved = store::approved_key(self.root(), name);
        let in_app = self.config.in_app_key.as_deref() == Some(manifest.key.as_str());
        if !in_app && approved.as_deref() != Some(manifest.key.as_str()) {
            return Outcome::new(
                name,
                State::Awaiting {
                    version: manifest.version,
                    key: manifest.key.clone(),
                    changed: approved.is_some(),
                },
            );
        }

        let latest = manifest.version;
        let chosen = match manifest.select(&self.config.protocols, &self.config.views) {
            Selection::Current => None,
            Selection::Previous(entry) => match self.earlier(address, &release, entry) {
                Ok(earlier) => Some(earlier),
                Err(failure) => {
                    return self.resting(address, Some(name.to_owned()), Some(latest), failure);
                }
            },
            Selection::Unsupported { version, .. } => {
                let state = match store::newest(self.root(), name) {
                    Some(landed) => ready(landed, Some(version)),
                    None => State::WaitsForUpdate { version },
                };
                return Outcome::new(name, state);
            }
        };
        let chosen = chosen.as_ref().unwrap_or(&release);
        let version = chosen.manifest.version;
        let Some(file) = chosen.manifest.files.get(&self.config.target) else {
            return Outcome::new(name, State::NotHere { version });
        };
        match self.land(address, chosen, file) {
            Ok(()) => Outcome::new(name, ready(version, Some(latest))),
            Err(failure) => self.resting(address, Some(name.to_owned()), Some(latest), failure),
        }
    }

    /// The release the latest one lists in `previous`, read from its own `app.json`: it must be
    /// the same program and the same key, the version and the requirement the entry says.
    fn earlier(
        &self,
        source: &str,
        latest: &Release,
        entry: &Previous,
    ) -> Result<Release, Failure> {
        if !reachable(source, &entry.manifest) {
            return Err(Failure::Unreachable);
        }
        let release = self.read(&entry.manifest)?;
        let (this, listed) = (&release.manifest, &latest.manifest);
        if this.name != listed.name {
            Err(Failure::Mismatch("an earlier release is another program"))
        } else if this.key != listed.key {
            Err(Failure::Mismatch(
                "an earlier release is signed with another key",
            ))
        } else if entry.version != this.version
            || entry.protocol != this.protocol
            || entry.views != this.views
        {
            Err(Failure::Mismatch(
                "an earlier release is not what the list says",
            ))
        } else {
            Ok(release)
        }
    }

    /// Puts `release` in its folder: its file for this machine's target downloaded and checked
    /// against the signed sum, then the folder renamed into place. A folder already there with
    /// the right file is left alone. Then the folder's records: who follows, which key.
    fn land(&self, source: &str, release: &Release, file: &File) -> Result<(), Failure> {
        let root = self.root();
        let manifest = &release.manifest;
        let (name, version) = (manifest.name.as_str(), manifest.version);
        let target = self.config.target.as_str();
        let program = store::program_path(root, name, version, target);
        let landed = store::release_dir(root, name, version)
            .join("app.json")
            .is_file()
            && fs::read(&program).is_ok_and(|bytes| sum(&bytes) == file.sha256);
        if !landed {
            if !reachable(source, &file.url) {
                return Err(Failure::Unreachable);
            }
            self.download(release, file)?;
        }
        store::follow(root, name, source).map_err(disk)?;
        if store::approved_key(root, name).is_none() {
            store::approve(root, name, &manifest.key).map_err(disk)?;
        }
        store::forget_removal(root, name, self.now);
        store::prune(root, name, self.now);
        Ok(())
    }

    fn download(&self, release: &Release, file: &File) -> Result<(), Failure> {
        let root = self.root();
        let manifest = &release.manifest;
        let (name, version) = (manifest.name.as_str(), manifest.version);
        let folder = store::app_dir(root, name);
        let temporary = folder.join(format!(".{version}.part-{}", std::process::id()));
        let _ = fs::remove_dir_all(&temporary);
        fs::create_dir_all(&temporary).map_err(disk)?;
        let result = self.fill(&temporary, release, file);
        let result = result.and_then(|()| {
            let place = store::release_dir(root, name, version);
            let _ = fs::remove_dir_all(&place);
            fs::rename(&temporary, &place).map_err(disk)
        });
        if result.is_err() {
            let _ = fs::remove_dir_all(&temporary);
        }
        result
    }

    /// The program, its sum checked and made executable, and the `app.json` with its signature,
    /// in the folder a release is built in.
    fn fill(&self, folder: &Path, release: &Release, file: &File) -> Result<(), Failure> {
        download_program(self.fetch, file, &folder.join(&self.config.target))?;
        fs::write(folder.join("app.json"), &release.bytes).map_err(disk)?;
        fs::write(folder.join("app.json.sig"), &release.signature).map_err(disk)
    }
}

/// The program of `file` downloaded to `to`, its sum checked against the signed one, and made
/// executable. A file that is not the signed one is deleted, not left to be run.
fn download_program(fetch: &dyn Fetch, file: &File, to: &Path) -> Result<(), Failure> {
    let result = fetch
        .download(&file.url, MAX_PROGRAM_BYTES, to)
        .map_err(Failure::Fetch)
        .and_then(|()| {
            let bytes = fs::read(to).map_err(disk)?;
            if sum(&bytes) != file.sha256 {
                return Err(Failure::Digest);
            }
            fs::set_permissions(to, fs::Permissions::from_mode(0o755)).map_err(disk)
        });
    if result.is_err() {
        let _ = fs::remove_file(to);
    }
    result
}

/// The program of a release that landed, for a `target` that is not this machine's: a server
/// the release is to be put on is a machine of another kind, and its file is fetched when it is
/// wanted, not before. The folder's `app.json` and its signature are read again and must hold
/// (what is on the disk is not trusted because it was put there), and the address obeys the rule
/// of the source that brought it.
///
/// `Ok(None)`: the release has no file for the target, and nothing is to be put on that machine.
pub fn fetch_target(
    root: &Path,
    name: &str,
    version: Version,
    target: &str,
    fetch: &dyn Fetch,
) -> Result<Option<std::path::PathBuf>, Failure> {
    let folder = store::release_dir(root, name, version);
    let bytes = fs::read(folder.join("app.json")).map_err(disk)?;
    let signature = fs::read(folder.join("app.json.sig")).map_err(disk)?;
    let manifest = apps::verify(&bytes, &signature).map_err(Failure::Manifest)?;
    if manifest.name != name || manifest.version != version {
        return Err(Failure::Mismatch(
            "the folder does not hold the release it is named for",
        ));
    }
    let Some(file) = manifest.files.get(target) else {
        return Ok(None);
    };
    let program = store::program_path(root, name, version, target);
    if fs::read(&program).is_ok_and(|bytes| sum(&bytes) == file.sha256) {
        return Ok(Some(program));
    }
    let source = store::followed(root, name).ok_or(Failure::Source)?;
    if !reachable(&source, &file.url) {
        return Err(Failure::Unreachable);
    }
    let part = folder.join(format!(".{target}.part-{}", std::process::id()));
    download_program(fetch, file, &part)?;
    fs::rename(&part, &program).map_err(|e| {
        let _ = fs::remove_file(&part);
        disk(e)
    })?;
    Ok(Some(program))
}

/// `Ready`, with the newest release when it is newer than the one in use.
fn ready(version: Version, latest: Option<Version>) -> State {
    State::Ready {
        version,
        newer: latest.filter(|latest| *latest > version),
    }
}

fn sum(bytes: &[u8]) -> String {
    Digest::of(bytes).to_string()
}

fn disk(e: std::io::Error) -> Failure {
    Failure::Disk(e.to_string())
}
