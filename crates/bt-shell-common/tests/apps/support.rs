//! What the scenarios are made of: a network that is a map of addresses to bytes, a temporary
//! apps folder, and authors that sign the releases the tests publish.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use bt_core::pencere::Digest;
use bt_core::pencere::apps::Version;
use bt_shell_common::apps::Fetch;
use ed25519_dalek::{Signer, SigningKey};

/// The target the scenarios run as, whatever machine they run on.
pub const TARGET: &str = "aarch64-apple-darwin";

/// Another target, whose file a release also has.
pub const OTHER_TARGET: &str = "x86_64-unknown-linux-musl";

pub fn v(text: &str) -> Version {
    Version::parse(text).unwrap_or_else(|| panic!("not a version: {text}"))
}

/// A folder of the tests, deleted when it goes out of scope.
pub struct Temp(pub PathBuf);

impl Temp {
    pub fn new(label: &str) -> Self {
        static COUNT: AtomicU32 = AtomicU32::new(0);
        let n = COUNT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "bateri-apps-test-{label}-{}-{n}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("a temporary folder");
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The network: what is `put` at an address is what `get` finds there, and nothing else is.
#[derive(Default)]
pub struct Net {
    files: Mutex<HashMap<String, Vec<u8>>>,
    asked: Mutex<Vec<String>>,
}

impl Net {
    pub fn put(&self, address: &str, bytes: impl Into<Vec<u8>>) {
        self.files
            .lock()
            .expect("lock")
            .insert(address.to_owned(), bytes.into());
    }

    pub fn take_down(&self, address: &str) {
        self.files.lock().expect("lock").remove(address);
    }

    /// Writes every `file:///` address that was put to the disk, as a file server would have
    /// it: the folders are made.
    pub fn write_files(&self) {
        for (address, bytes) in self.files.lock().expect("lock").iter() {
            if let Some(path) = address.strip_prefix("file://") {
                let path = Path::new(path);
                fs::create_dir_all(path.parent().expect("a folder")).expect("a folder");
                fs::write(path, bytes).expect("a file");
            }
        }
    }

    /// The addresses asked for so far, in order.
    pub fn asked(&self) -> Vec<String> {
        self.asked.lock().expect("lock").clone()
    }

    fn find(&self, address: &str, limit: u64) -> Result<Vec<u8>, String> {
        self.asked.lock().expect("lock").push(address.to_owned());
        let bytes = self
            .files
            .lock()
            .expect("lock")
            .get(address)
            .cloned()
            .ok_or_else(|| "curl: (22) The requested URL returned error: 404".to_owned())?;
        if bytes.len() as u64 > limit {
            return Err(format!("more than {limit} bytes"));
        }
        Ok(bytes)
    }
}

impl Fetch for Net {
    fn get(&self, address: &str, limit: u64) -> Result<Vec<u8>, String> {
        self.find(address, limit)
    }

    fn download(&self, address: &str, limit: u64, path: &Path) -> Result<(), String> {
        let bytes = self.find(address, limit)?;
        fs::write(path, bytes).map_err(|e| e.to_string())
    }
}

/// Someone who signs releases, with a key the tests invent from one byte.
pub struct Author(SigningKey);

impl Author {
    pub fn new(seed: u8) -> Self {
        Self(SigningKey::from_bytes(&[seed; 32]))
    }

    /// The public half as an `app.json` names it.
    pub fn key(&self) -> String {
        format!("ed25519:{}", base64(self.0.verifying_key().as_bytes()))
    }

    /// The signature file's content over `bytes`.
    pub fn sign(&self, bytes: &[u8]) -> Vec<u8> {
        format!("ed25519:{}\n", base64(&self.0.sign(bytes).to_bytes())).into_bytes()
    }
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// A release the tests publish, which is a program of one line for each of two targets.
pub struct Release {
    pub name: String,
    pub version: String,
    /// The `protocol` list, as JSON.
    pub protocol: String,
    /// The `views` list, as JSON.
    pub views: String,
    /// The `previous` list, as JSON.
    pub previous: String,
    pub targets: Vec<&'static str>,
    /// What the program of every target says, so that two releases differ.
    pub body: String,
    /// Where the files are, when not beside the `app.json`.
    pub files_at: Option<String>,
}

impl Release {
    pub fn new(name: &str, version: &str) -> Self {
        Self {
            name: name.to_owned(),
            version: version.to_owned(),
            protocol: "[1]".to_owned(),
            views: r#"[{"kind":"lua","versions":["0.1"]}]"#.to_owned(),
            previous: "[]".to_owned(),
            targets: vec![TARGET, OTHER_TARGET],
            body: format!("#!/bin/sh\necho {name} {version}\n"),
            files_at: None,
        }
    }

    pub fn files_at(mut self, folder: &str) -> Self {
        self.files_at = Some(folder.to_owned());
        self
    }

    pub fn views(mut self, views: &str) -> Self {
        self.views = views.to_owned();
        self
    }

    pub fn previous(mut self, previous: &str) -> Self {
        self.previous = previous.to_owned();
        self
    }

    pub fn targets(mut self, targets: &[&'static str]) -> Self {
        self.targets = targets.to_vec();
        self
    }

    /// The address of the program for `target`, beside the `app.json` at `manifest`.
    pub fn file_address(&self, manifest: &str, target: &str) -> String {
        let folder = self
            .files_at
            .as_deref()
            .unwrap_or(&manifest[..=manifest.rfind('/').expect("an address has a /")]);
        format!("{folder}{}-{}-{target}", self.name, self.version)
    }

    /// Puts the `app.json` at `manifest`, its signature by `author`, and the programs beside it.
    /// Returns the `app.json`'s bytes.
    pub fn publish(&self, net: &Net, manifest: &str, author: &Author) -> Vec<u8> {
        let sum = Digest::of(self.body.as_bytes()).to_string();
        let files: Vec<String> = self
            .targets
            .iter()
            .map(|target| {
                let url = self.file_address(manifest, target);
                net.put(&url, self.body.as_bytes());
                format!(r#""{target}":{{"url":"{url}","sha256":"{sum}"}}"#)
            })
            .collect();
        let json = format!(
            r#"{{"name":"{}","version":"{}","protocol":{},"views":{},"key":"{}","files":{{{}}},"previous":{}}}"#,
            self.name,
            self.version,
            self.protocol,
            self.views,
            author.key(),
            files.join(","),
            self.previous,
        );
        net.put(manifest, json.as_bytes());
        net.put(&format!("{manifest}.sig"), author.sign(json.as_bytes()));
        json.into_bytes()
    }

    /// The entry of `previous` that lists this release, whose `app.json` is at `manifest`.
    pub fn as_previous(&self, manifest: &str) -> String {
        format!(
            r#"{{"version":"{}","protocol":{},"views":{},"manifest":"{manifest}"}}"#,
            self.version, self.protocol, self.views
        )
    }
}
