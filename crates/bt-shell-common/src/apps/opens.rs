//! The followed programs that open files and folders: the ones whose `app.json` says so
//! (`opens`, pencere's `spec/draft/apps.md` Section 4.7). What Settings ▸ Apps offers as the
//! program a clicked path opens in, and what the click's rule asks about a name.
//!
//! The answer is read from the folder ([`store`]) as the card reads it ([`super::card`]): the
//! `app.json` of the newest release that landed, through the same reader that checked it when it
//! came, so that there is no second reader of the file. A folder that does not hold the release
//! it is named for, or whose file does not verify or is of another name, says nothing.

use std::fs;
use std::path::Path;

use bt_core::pencere::apps::{self, Opens};

use super::store;

/// A followed program that opens paths, and which kinds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opener {
    /// The program's name.
    pub name: String,
    /// What it takes, each kind once: never empty.
    pub opens: Vec<Opens>,
}

/// The followed programs of `root` that say they open a file, a folder or both, in the order of
/// their names.
pub fn openers(root: &Path) -> Vec<Opener> {
    store::followed_names(root)
        .into_iter()
        .filter_map(|(name, _)| {
            let opens = opens_of(root, &name)?;
            Some(Opener { name, opens })
        })
        .collect()
}

/// What the release of `name` that landed says it opens; `None` when it says nothing, or when the
/// folder cannot vouch for it.
fn opens_of(root: &Path, name: &str) -> Option<Vec<Opens>> {
    let version = store::newest(root, name)?;
    let folder = store::release_dir(root, name, version);
    let bytes = fs::read(folder.join("app.json")).ok()?;
    let signature = fs::read(folder.join("app.json.sig")).ok()?;
    let manifest = apps::verify(&bytes, &signature).ok()?;
    (manifest.name == name && !manifest.opens.is_empty()).then_some(manifest.opens)
}
