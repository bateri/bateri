//! What this Mac keeps of a profile's sync between runs, and the files the sync works with.
//!
//! - **The state file** (`<data>/sync/<folder>`): which computer this is to the home (a random id,
//!   made once), which home it last spoke to (its depot id), the newest record it has taken from
//!   it (`since`), and for every record it knows the number it had at the home when the Mac
//!   took it and the sum of the value the Mac has for it (`known`). With these a run starts
//!   by asking the home for what is newer than `since` only, and knows which records the Mac
//!   changed while the home was out of reach.
//! - **The blob cache** (`<data>/blobs/<sha256>`): the bytes of files the home keeps for the
//!   profile (an IndexedDB's files, a value too large for a record), kept by their sums so
//!   that nothing is fetched twice. Written whole or not at all.
//!
//! The state file is written whole beside itself and renamed over, mode 0600.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use kablo_proto::profile::{Digest, hex};

use super::records::Id;

const SIGNATURE: &str = "bateri-profile-sync 1";

/// What the Mac knows of one record: the number the home gave the version the Mac has, and the
/// sum of that version's value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Known {
    pub applied: u64,
    pub digest: Digest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Saved {
    /// Who this computer is to the home.
    pub client: [u8; 16],
    /// The home this was last spoken to (a home that was made again is another).
    pub depot: Option<[u8; 16]>,
    /// The newest record taken (and put in the profile) from the home.
    pub since: u64,
    pub known: HashMap<Id, Known>,
}

pub fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn decode_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

fn array<const N: usize>(text: &str) -> Option<[u8; N]> {
    decode_hex(text)?.try_into().ok()
}

fn random_id() -> [u8; 16] {
    let mut id = [0u8; 16];
    // `/dev/urandom` always answers; a short read leaves what a clock and an address make of it.
    if File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut id))
        .is_err()
    {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        id.copy_from_slice(&now.to_be_bytes());
    }
    id
}

impl Saved {
    /// A computer that has never joined.
    pub fn fresh() -> Saved {
        Saved {
            client: random_id(),
            depot: None,
            since: 0,
            known: HashMap::new(),
        }
    }

    /// What `path` says; a file that is not there or is not one this build wrote is a computer
    /// that has never joined (what it knew is asked of the home again, which costs only time).
    pub fn load(path: &Path) -> Saved {
        let Ok(text) = fs::read_to_string(path) else {
            return Saved::fresh();
        };
        Saved::parse(&text).unwrap_or_else(Saved::fresh)
    }

    fn parse(text: &str) -> Option<Saved> {
        let mut lines = text.lines();
        if lines.next()? != SIGNATURE {
            return None;
        }
        let mut saved = Saved {
            client: [0; 16],
            depot: None,
            since: 0,
            known: HashMap::new(),
        };
        let mut has_client = false;
        for line in lines {
            let mut words = line.split(' ');
            match words.next()? {
                "client" => {
                    saved.client = array(words.next()?)?;
                    has_client = true;
                }
                "depot" => {
                    saved.depot = match words.next()? {
                        "-" => None,
                        word => Some(array(word)?),
                    }
                }
                "since" => saved.since = words.next()?.parse().ok()?,
                "rec" => {
                    let kind: u8 = words.next()?.parse().ok()?;
                    let key = decode_hex(words.next()?)?;
                    let applied: u64 = words.next()?.parse().ok()?;
                    let digest: Digest = array(words.next()?)?;
                    saved.known.insert((kind, key), Known { applied, digest });
                }
                _ => {}
            }
        }
        has_client.then_some(saved)
    }

    fn render(&self) -> String {
        let mut out = format!("{SIGNATURE}\nclient {}\n", encode_hex(&self.client));
        match &self.depot {
            Some(depot) => out += &format!("depot {}\n", encode_hex(depot)),
            None => out += "depot -\n",
        }
        out += &format!("since {}\n", self.since);
        let mut known: Vec<_> = self.known.iter().collect();
        known.sort_by(|a, b| a.0.cmp(b.0));
        for ((kind, key), k) in known {
            out += &format!(
                "rec {kind} {} {} {}\n",
                encode_hex(key),
                k.applied,
                hex(&k.digest)
            );
        }
        out
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)?;
        }
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(format!(".tmp{}", std::process::id()));
        let tmp = PathBuf::from(tmp);
        let written = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)
            .and_then(|mut f| f.write_all(self.render().as_bytes()))
            .and_then(|()| fs::rename(&tmp, path));
        if written.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        written
    }
}

/// The bytes of the files the home keeps, by the sum of their bytes.
#[derive(Clone, Debug)]
pub struct Cache {
    dir: PathBuf,
}

impl Cache {
    pub fn new(dir: PathBuf) -> Cache {
        Cache { dir }
    }

    pub fn path(&self, digest: &Digest) -> PathBuf {
        self.dir.join(hex(digest))
    }

    pub fn has(&self, digest: &Digest) -> bool {
        self.path(digest).is_file()
    }

    pub fn read(&self, digest: &Digest) -> io::Result<Vec<u8>> {
        fs::read(self.path(digest))
    }

    fn part(&self, digest: &Digest) -> PathBuf {
        self.dir.join(format!(".part-{}", hex(digest)))
    }

    /// Starts a blob that comes in parts.
    pub fn begin(&self, digest: &Digest) -> io::Result<File> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.dir)?;
        OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(self.part(digest))
    }

    /// Puts a finished blob in its place if its bytes are the ones the sum names; `false` and
    /// nothing kept otherwise.
    pub fn finish(&self, digest: &Digest) -> io::Result<bool> {
        let part = self.part(digest);
        let mut hasher = kablo_proto::profile::Sha256::new();
        let mut file = File::open(&part)?;
        let mut chunk = vec![0u8; 64 * 1024];
        loop {
            let n = file.read(&mut chunk)?;
            if n == 0 {
                break;
            }
            hasher.update(chunk.get(..n).unwrap_or_default());
        }
        if &hasher.finish() != digest {
            let _ = fs::remove_file(&part);
            return Ok(false);
        }
        fs::rename(&part, self.path(digest))?;
        Ok(true)
    }

    /// Puts bytes that are whole in their place.
    pub fn put(&self, digest: &Digest, bytes: &[u8]) -> io::Result<()> {
        let mut file = self.begin(digest)?;
        file.write_all(bytes)?;
        drop(file);
        if self.finish(digest)? {
            Ok(())
        } else {
            Err(io::Error::other(
                "bytes that are not the ones their sum names",
            ))
        }
    }

    /// Takes the file of an IndexedDB (from the stage the helper made) into the cache under its
    /// sum, unless it is there.
    pub fn take(&self, from: &Path, digest: &Digest) -> io::Result<()> {
        if self.has(digest) {
            return Ok(());
        }
        let bytes = fs::read(from)?;
        self.put(digest, &bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kablo_proto::profile::sum;

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bt-sync-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn what_was_known_is_known_again_after_a_run() {
        let root = dir("state");
        let path = root.join("sync").join("folder");
        let mut saved = Saved::fresh();
        saved.depot = Some([7; 16]);
        saved.since = 42;
        saved.known.insert(
            (1, b"\x00\x01cookie".to_vec()),
            Known {
                applied: 40,
                digest: sum(b"value"),
            },
        );
        saved.known.insert(
            (4, b"origin".to_vec()),
            Known {
                applied: 41,
                digest: sum(b"files"),
            },
        );
        saved.save(&path).unwrap();
        assert_eq!(Saved::load(&path), saved);
        // Mode 0600 in a folder of mode 0700.
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_file_that_is_not_there_or_not_one_of_ours_is_a_computer_that_never_joined() {
        let root = dir("fresh");
        let path = root.join("none");
        let fresh = Saved::load(&path);
        assert_eq!((fresh.since, fresh.depot, fresh.known.len()), (0, None, 0));
        fs::write(&path, "something else\n").unwrap();
        let other = Saved::load(&path);
        assert_eq!(other.since, 0);
        assert_ne!(other.client, fresh.client, "an id for each that is made");
        fs::write(&path, format!("{SIGNATURE}\nclient 00\n")).unwrap();
        assert_eq!(
            Saved::load(&path).since,
            0,
            "a client id of the wrong length"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_blob_is_kept_whole_and_by_its_sum() {
        let root = dir("cache");
        let cache = Cache::new(root.join("blobs"));
        let digest = sum(b"hello");
        assert!(!cache.has(&digest));
        cache.put(&digest, b"hello").unwrap();
        assert!(cache.has(&digest));
        assert_eq!(cache.read(&digest).unwrap(), b"hello");
        // Bytes that are not the ones the sum names are not kept.
        let other = sum(b"world");
        assert!(cache.put(&other, b"not it").is_err());
        assert!(!cache.has(&other));
        // In parts.
        let mut file = cache.begin(&other).unwrap();
        file.write_all(b"wor").unwrap();
        file.write_all(b"ld").unwrap();
        drop(file);
        assert!(cache.finish(&other).unwrap());
        assert_eq!(cache.read(&other).unwrap(), b"world");
        let _ = fs::remove_dir_all(&root);
    }
}
