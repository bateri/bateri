//! What a profile's records are on the wire of its home, and back.
//!
//! The helper hands bateri records ([`Record`]: a cookie, an item of Local Storage, an origin's
//! IndexedDB) and the home keeps them as a kind, a key and a value of bytes it does not read
//! (`kablo_proto::profile::Body`). This is the one place that says how the one becomes the
//! other: the key is a block of entries in ascending order of their numbers (so that the same
//! record is always the same bytes: the home tells records apart by them), the value another.
//!
//! The numbers are the protocol's description and the plan's: they never change meaning.
//!
//! - cookie key: 1 domain, 2 path, 3 name, 4 the partition's top-level site and 5 whether it
//!   has a cross-site ancestor (u8), the last two only for a partitioned cookie;
//!   value: 1 value, 2 expiry (the bits of an f64, absent for a session cookie), 3 secure,
//!   4 http-only, 5 same-site, 6 priority, 7 source scheme (u8 each), 8 source port (i32);
//! - Local Storage key: 1 storage key (the engine's name of the origin), 2 item key;
//!   value: the item's text;
//! - IndexedDB key: 1 the folder's name for the origin; value: 1 format (text), 2 the engine's
//!   major version (u16), 3 file, repeated: {1 path, 2 sha256, 3 size (u64)}, and the bytes of
//!   the files are blobs of the home (`linked`).

use browser_ipc::message::{CookieValue, IndexedDbFile, IndexedDbValue, Partition, Record};
use kablo_proto::profile::{self as wire, BlobRef, Body, Digest, MAX_VALUE, kind};
use kablo_proto::tlv::{self, Writer};

/// What tells one record from another: the home's own identity (kind and key bytes).
pub type Id = (u8, Vec<u8>);

/// A record that is not what this code writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unreadable(pub &'static str);

impl From<kablo_proto::Error> for Unreadable {
    fn from(_: kablo_proto::Error) -> Unreadable {
        Unreadable("a block of entries")
    }
}

/// A file of an IndexedDB, with the sum of its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummedFile {
    pub path: String,
    pub sha256: Digest,
    pub size: u64,
}

/// An IndexedDB's folder as the home keeps it: the files are named by the sums of their bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    pub origin: String,
    pub format: String,
    pub major: u16,
    pub files: Vec<SummedFile>,
}

/// A record in the form the home keeps, and what has to be on the home for it to be taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wired {
    pub body: Body,
    /// The bytes of a value too large for a record, that go to the home as a blob (the body
    /// points at them).
    pub big_value: Option<Vec<u8>>,
}

impl Wired {
    pub fn id(&self) -> Id {
        (self.body.kind, self.body.key.clone())
    }
}

/// The identity of a record.
pub fn id_of(record: &Record) -> Id {
    match record {
        Record::Cookie {
            domain,
            path,
            name,
            partition,
            ..
        } => {
            let mut key = Writer::new();
            key.put(1, domain.as_bytes());
            key.put(2, path.as_bytes());
            key.put(3, name.as_bytes());
            if let Some(partition) = partition {
                key.put(4, partition.site.as_bytes());
                key.put_u8(5, u8::from(partition.cross_site));
            }
            (kind::COOKIE, key.finish())
        }
        Record::LocalStorage { origin, key, .. } => {
            let mut block = Writer::new();
            block.put(1, origin.as_bytes());
            block.put(2, key.as_bytes());
            (kind::LOCAL_STORAGE, block.finish())
        }
        Record::IndexedDb { origin, .. } => {
            let mut block = Writer::new();
            block.put(1, origin.as_bytes());
            (kind::INDEXED_DB, block.finish())
        }
    }
}

/// The same record with its value gone: what says the thing is deleted.
pub fn without_value(record: &Record) -> Record {
    match record {
        Record::Cookie {
            domain,
            path,
            name,
            partition,
            ..
        } => Record::Cookie {
            domain: domain.clone(),
            path: path.clone(),
            name: name.clone(),
            partition: partition.clone(),
            value: None,
        },
        Record::LocalStorage { origin, key, .. } => Record::LocalStorage {
            origin: origin.clone(),
            key: key.clone(),
            value: None,
        },
        Record::IndexedDb { origin, .. } => Record::IndexedDb {
            origin: origin.clone(),
            value: None,
        },
    }
}

fn cookie_value(value: &CookieValue) -> Vec<u8> {
    let mut block = Writer::new();
    block.put(1, value.value.as_bytes());
    if let Some(expires) = value.expires {
        block.put_u64(2, expires.to_bits());
    }
    block.put_u8(3, u8::from(value.secure));
    block.put_u8(4, u8::from(value.http_only));
    block.put_u8(5, value.same_site);
    block.put_u8(6, value.priority);
    block.put_u8(7, value.source_scheme);
    block.put_u32(8, value.source_port as u32);
    block.finish()
}

/// The milliseconds a cookie runs out at, for the home to let go of it then.
fn runs_out(value: &CookieValue) -> Option<u64> {
    let seconds = value.expires?;
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    Some((seconds * 1000.0) as u64)
}

/// The record as the home keeps it. An IndexedDB record needs the sums of its files
/// (`sums`, in the order of the record's files); the others take none.
pub fn wire_of(record: &Record, sums: &[Digest]) -> Wired {
    let (kind, key) = id_of(record);
    let mut body = Body {
        kind,
        key,
        ..Body::default()
    };
    let mut big_value = None;
    match record {
        Record::Cookie { value, .. } => match value {
            None => body.deleted = true,
            Some(value) => {
                body.value = cookie_value(value);
                body.expires = runs_out(value);
            }
        },
        Record::LocalStorage { value, .. } => match value {
            None => body.deleted = true,
            Some(text) => {
                if text.len() > MAX_VALUE {
                    let bytes = text.as_bytes().to_vec();
                    body.value_blob = Some(BlobRef {
                        sha256: wire::sum(&bytes),
                        size: bytes.len() as u64,
                    });
                    big_value = Some(bytes);
                } else {
                    body.value = text.as_bytes().to_vec();
                }
            }
        },
        Record::IndexedDb { value, .. } => match value {
            None => body.deleted = true,
            Some(value) => {
                let mut block = Writer::new();
                block.put(1, value.format.as_bytes());
                block.put_u16(2, value.major);
                for (file, sum) in value.files.iter().zip(sums) {
                    let mut entry = Writer::new();
                    entry.put(1, file.path.as_bytes());
                    entry.put(2, sum);
                    entry.put_u64(3, file.size);
                    block.put(3, &entry.finish());
                    body.linked.push(BlobRef {
                        sha256: *sum,
                        size: file.size,
                    });
                }
                let bytes = block.finish();
                if bytes.len() > MAX_VALUE {
                    body.value_blob = Some(BlobRef {
                        sha256: wire::sum(&bytes),
                        size: bytes.len() as u64,
                    });
                    big_value = Some(bytes);
                } else {
                    body.value = bytes;
                }
            }
        },
    }
    Wired { body, big_value }
}

/// Whether `path` is a relative path of plain names (`leveldb/000003.log`): no `..`, no `.`, no
/// empty part, no root, no NUL. What a home says is not trusted to name where a file goes on this
/// Mac: a path that leaves the folder it is joined to would write anywhere the person can.
pub fn is_plain_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains('\0')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

/// Whether `name` is one plain name, as a database's origin folder is.
pub fn is_plain_name(name: &str) -> bool {
    is_plain_path(name) && !name.contains('/')
}

fn text(value: &[u8], what: &'static str) -> Result<String, Unreadable> {
    String::from_utf8(value.to_vec()).map_err(|_| Unreadable(what))
}

fn one(value: &[u8], what: &'static str) -> Result<u8, Unreadable> {
    Ok(tlv::read_u8(value, what)?)
}

/// What a body says, with the bytes of the value when they are in a blob (`blob`).
pub fn record_of(body: &Body, blob: Option<&[u8]>) -> Result<Record, Unreadable> {
    let value_bytes: Option<&[u8]> = if body.deleted {
        None
    } else if body.value_blob.is_some() {
        Some(blob.ok_or(Unreadable("a value that is in a blob that is not here"))?)
    } else {
        Some(body.value.as_slice())
    };
    match body.kind {
        kind::COOKIE => {
            let (mut domain, mut path, mut name) = (None, None, None);
            let (mut site, mut cross) = (None, false);
            for entry in tlv::entries(&body.key) {
                let (k, v) = entry?;
                match k {
                    1 => domain = Some(text(v, "cookie domain")?),
                    2 => path = Some(text(v, "cookie path")?),
                    3 => name = Some(text(v, "cookie name")?),
                    4 => site = Some(text(v, "cookie partition")?),
                    5 => cross = one(v, "cookie partition ancestor")? != 0,
                    _ => {}
                }
            }
            let (Some(domain), Some(path), Some(name)) = (domain, path, name) else {
                return Err(Unreadable("a cookie without its domain, path or name"));
            };
            let partition = site.map(|site| Partition {
                site,
                cross_site: cross,
            });
            let value = value_bytes.map(read_cookie_value).transpose()?;
            Ok(Record::Cookie {
                domain,
                path,
                name,
                partition,
                value,
            })
        }
        kind::LOCAL_STORAGE => {
            let (mut origin, mut key) = (None, None);
            for entry in tlv::entries(&body.key) {
                let (k, v) = entry?;
                match k {
                    1 => origin = Some(text(v, "storage origin")?),
                    2 => key = Some(text(v, "storage key")?),
                    _ => {}
                }
            }
            let (Some(origin), Some(key)) = (origin, key) else {
                return Err(Unreadable("an item without its origin or key"));
            };
            let value = value_bytes
                .map(|bytes| text(bytes, "storage value"))
                .transpose()?;
            Ok(Record::LocalStorage { origin, key, value })
        }
        kind::INDEXED_DB => {
            let mut origin = None;
            for entry in tlv::entries(&body.key) {
                let (k, v) = entry?;
                if k == 1 {
                    origin = Some(text(v, "database origin")?);
                }
            }
            let origin = origin.ok_or(Unreadable("a database without its origin"))?;
            if !is_plain_name(&origin) {
                return Err(Unreadable("a database origin that is not a plain name"));
            }
            let value = value_bytes
                .map(|bytes| folder_value(bytes).map(|f| (f.format, f.major, f.files)))
                .transpose()?
                .map(|(format, major, files)| IndexedDbValue {
                    format,
                    major,
                    files: files
                        .into_iter()
                        .map(|file| IndexedDbFile {
                            path: file.path,
                            size: file.size,
                        })
                        .collect(),
                });
            Ok(Record::IndexedDb { origin, value })
        }
        _ => Err(Unreadable("a record of a kind this build does not know")),
    }
}

fn read_cookie_value(bytes: &[u8]) -> Result<CookieValue, Unreadable> {
    let mut out = CookieValue {
        value: String::new(),
        expires: None,
        secure: false,
        http_only: false,
        same_site: 0,
        priority: 1,
        source_scheme: 0,
        source_port: -1,
    };
    for entry in tlv::entries(bytes) {
        let (k, v) = entry?;
        match k {
            1 => out.value = text(v, "cookie value")?,
            2 => out.expires = Some(f64::from_bits(tlv::read_u64(v, "cookie expiry")?)),
            3 => out.secure = one(v, "cookie secure")? != 0,
            4 => out.http_only = one(v, "cookie http-only")? != 0,
            5 => out.same_site = one(v, "cookie same-site")?,
            6 => out.priority = one(v, "cookie priority")?,
            7 => out.source_scheme = one(v, "cookie source scheme")?,
            8 => out.source_port = tlv::read_u32(v, "cookie source port")? as i32,
            _ => {}
        }
    }
    Ok(out)
}

/// The folder an IndexedDB value names, with the sums of its files.
pub fn folder_value(bytes: &[u8]) -> Result<Folder, Unreadable> {
    let (mut format, mut major, mut files) = (None, None, Vec::new());
    for entry in tlv::entries(bytes) {
        let (k, v) = entry?;
        match k {
            1 => format = Some(text(v, "database format")?),
            2 => major = Some(tlv::read_u16(v, "database version")?),
            3 => {
                let (mut path, mut sum, mut size) = (None, None, None);
                for inner in tlv::entries(v) {
                    let (ik, iv) = inner?;
                    match ik {
                        1 => path = Some(text(iv, "database file")?),
                        2 => {
                            sum = Some(
                                Digest::try_from(iv)
                                    .map_err(|_| Unreadable("a database file's sum"))?,
                            )
                        }
                        3 => size = Some(tlv::read_u64(iv, "database file size")?),
                        _ => {}
                    }
                }
                let (Some(path), Some(sha256), Some(size)) = (path, sum, size) else {
                    return Err(Unreadable("a database file without its parts"));
                };
                if !is_plain_path(&path) {
                    return Err(Unreadable("a database file whose path leaves its folder"));
                }
                files.push(SummedFile { path, sha256, size });
            }
            _ => {}
        }
    }
    match (format, major) {
        (Some(format), Some(major)) => Ok(Folder {
            origin: String::new(),
            format,
            major,
            files,
        }),
        _ => Err(Unreadable("a database without its format")),
    }
}

/// The sum of a body, to tell whether what is here is the same as what is there: what the home
/// would keep (the value, the blobs), not who wrote it or when.
pub fn digest_of(body: &Body) -> Digest {
    let mut hasher = wire::Sha256::new();
    hasher.update(&[u8::from(body.deleted)]);
    hasher.update(&(body.value.len() as u64).to_be_bytes());
    hasher.update(&body.value);
    for blob in body.blobs() {
        hasher.update(&blob.sha256);
        hasher.update(&blob.size.to_be_bytes());
    }
    hasher.finish()
}

/// The sum of a record as it would be kept; a record of IndexedDB needs `sums`.
pub fn record_digest(record: &Record, sums: &[Digest]) -> Digest {
    digest_of(&wire_of(record, sums).body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cookie(name: &str, partition: Option<Partition>, value: Option<CookieValue>) -> Record {
        Record::Cookie {
            domain: ".example.com".into(),
            path: "/".into(),
            name: name.into(),
            partition,
            value,
        }
    }

    fn value() -> CookieValue {
        CookieValue {
            value: "abc".into(),
            expires: Some(1_800_000_000.5),
            secure: true,
            http_only: true,
            same_site: 2,
            priority: 2,
            source_scheme: 2,
            source_port: 443,
        }
    }

    fn back(record: &Record, sums: &[Digest]) -> Record {
        let wired = wire_of(record, sums);
        record_of(&wired.body, wired.big_value.as_deref()).unwrap()
    }

    #[test]
    fn a_cookie_goes_there_and_comes_back_as_it_was() {
        let sessions = CookieValue {
            expires: None,
            source_port: -1,
            ..value()
        };
        for record in [
            cookie("sid", None, Some(value())),
            cookie("session", None, Some(sessions)),
            cookie(
                "chips",
                Some(Partition {
                    site: "https://top.example".into(),
                    cross_site: true,
                }),
                Some(value()),
            ),
            cookie("gone", None, None),
        ] {
            assert_eq!(back(&record, &[]), record);
        }
    }

    #[test]
    fn what_a_cookie_is_there_is_its_domain_path_name_and_partition_and_nothing_of_its_value() {
        let a = id_of(&cookie("sid", None, Some(value())));
        let b = id_of(&cookie("sid", None, None));
        assert_eq!(a, b);
        let partitioned = id_of(&cookie(
            "sid",
            Some(Partition {
                site: "https://t".into(),
                cross_site: false,
            }),
            None,
        ));
        assert_ne!(a, partitioned);
        assert_ne!(a, id_of(&cookie("other", None, None)));
        // The key is the writer's own block with the numbers in order.
        let keys: Vec<u16> = tlv::entries(&a.1).map(|e| e.unwrap().0).collect();
        assert_eq!(keys, [1, 2, 3]);
        let keys: Vec<u16> = tlv::entries(&partitioned.1).map(|e| e.unwrap().0).collect();
        assert_eq!(keys, [1, 2, 3, 4, 5]);
    }

    #[test]
    fn a_cookie_that_runs_out_says_when_in_milliseconds_and_a_session_one_never() {
        let wired = wire_of(&cookie("sid", None, Some(value())), &[]);
        assert_eq!(wired.body.expires, Some(1_800_000_000_500));
        let session = CookieValue {
            expires: None,
            ..value()
        };
        assert_eq!(
            wire_of(&cookie("sid", None, Some(session)), &[])
                .body
                .expires,
            None
        );
    }

    #[test]
    fn local_storage_goes_there_whole_and_a_large_value_goes_as_a_blob() {
        let item = Record::LocalStorage {
            origin: "http://site.test/".into(),
            key: "token".into(),
            value: Some("abc".into()),
        };
        assert_eq!(back(&item, &[]), item);
        let big = Record::LocalStorage {
            origin: "http://site.test/".into(),
            key: "big".into(),
            value: Some("x".repeat(MAX_VALUE + 1)),
        };
        let wired = wire_of(&big, &[]);
        assert!(wired.body.value.is_empty());
        let blob = wired.big_value.clone().unwrap();
        assert_eq!(wired.body.value_blob.unwrap().sha256, wire::sum(&blob));
        assert_eq!(back(&big, &[]), big);
        let gone = Record::LocalStorage {
            origin: "http://site.test/".into(),
            key: "token".into(),
            value: None,
        };
        assert_eq!(back(&gone, &[]), gone);
        assert!(wire_of(&gone, &[]).body.deleted);
    }

    #[test]
    fn what_a_home_says_does_not_name_a_place_outside_the_folder_it_is_put_in() {
        for path in ["ok", "leveldb/000003.log", "blob/1/00/1"] {
            assert!(is_plain_path(path), "{path}");
        }
        for path in [
            "",
            "/etc/passwd",
            "../x",
            "a/../../x",
            "a//b",
            "./a",
            "a/.",
            "a/\0b",
            "..",
        ] {
            assert!(!is_plain_path(path), "{path:?}");
        }
        assert!(is_plain_name("https_site.test_0") && !is_plain_name("a/b"));
        // A database whose file leaves its folder, or whose origin does, is not a record.
        let database = |origin: &str, path: &str| Record::IndexedDb {
            origin: origin.into(),
            value: Some(IndexedDbValue {
                format: "chromium-idb-leveldb".into(),
                major: 1,
                files: vec![IndexedDbFile {
                    path: path.into(),
                    size: 1,
                }],
            }),
        };
        let sums = [wire::sum(b"x")];
        for (origin, path) in [
            ("http_site.test_0", "../../../Library/LaunchAgents/x.plist"),
            ("http_site.test_0", "/etc/cron.d/x"),
            ("../../outside", "leveldb/000003.log"),
            ("", "leveldb/000003.log"),
        ] {
            let body = wire_of(&database(origin, path), &sums).body;
            assert!(record_of(&body, None).is_err(), "{origin:?} {path:?}");
        }
        let body = wire_of(&database("http_site.test_0", "leveldb/000003.log"), &sums).body;
        assert!(record_of(&body, None).is_ok());
    }

    #[test]
    fn an_indexed_db_names_its_files_by_their_sums_and_keeps_them_alive() {
        let record = Record::IndexedDb {
            origin: "http_site.test_0".into(),
            value: Some(IndexedDbValue {
                format: "chromium-idb-leveldb".into(),
                major: 156,
                files: vec![
                    IndexedDbFile {
                        path: "leveldb/000003.log".into(),
                        size: 11,
                    },
                    IndexedDbFile {
                        path: "blob/1/00/1".into(),
                        size: 3,
                    },
                ],
            }),
        };
        let sums = [wire::sum(b"first"), wire::sum(b"second")];
        let wired = wire_of(&record, &sums);
        assert_eq!(wired.body.linked.len(), 2);
        assert_eq!(wired.body.linked[0].sha256, sums[0]);
        assert_eq!(wired.body.linked[1].size, 3);
        assert_eq!(back(&record, &sums), record);
        let folder = folder_value(&wired.body.value).unwrap();
        assert_eq!(folder.files[1].path, "blob/1/00/1");
        assert_eq!(folder.files[1].sha256, sums[1]);
        // The same files are the same sum; another sum of a file is another record.
        let other = [wire::sum(b"first"), wire::sum(b"changed")];
        assert_ne!(
            record_digest(&record, &sums),
            record_digest(&record, &other)
        );
        assert_eq!(record_digest(&record, &sums), record_digest(&record, &sums));
    }

    #[test]
    fn what_this_build_does_not_read_is_said_and_not_guessed() {
        let unknown = Body {
            kind: 9,
            key: b"k".to_vec(),
            ..Body::default()
        };
        assert!(record_of(&unknown, None).is_err());
        let torn = Body {
            kind: kind::COOKIE,
            key: vec![0, 1, 0, 0, 0, 9, b'x'],
            ..Body::default()
        };
        assert!(record_of(&torn, None).is_err());
        let in_blob = Body {
            kind: kind::LOCAL_STORAGE,
            key: id_of(&Record::LocalStorage {
                origin: "o".into(),
                key: "k".into(),
                value: None,
            })
            .1,
            value_blob: Some(BlobRef {
                sha256: wire::sum(b"x"),
                size: 1,
            }),
            ..Body::default()
        };
        assert!(record_of(&in_blob, None).is_err());
        assert!(record_of(&in_blob, Some(b"x")).is_ok());
        // An entry that came after this build is skipped.
        let mut grown = wire_of(&cookie("sid", None, Some(value())), &[]).body;
        let mut extra = Writer::new();
        extra.put(99, b"later");
        grown.value.extend_from_slice(&extra.finish());
        assert!(record_of(&grown, None).is_ok());
    }
}
