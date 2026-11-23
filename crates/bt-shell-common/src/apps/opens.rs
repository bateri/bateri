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

impl Opener {
    /// Whether the program opens a file or a folder, the paths a click opens: a program that opens
    /// web addresses only (browserpen) is not one a path is offered to.
    pub fn opens_paths(&self) -> bool {
        self.opens
            .iter()
            .any(|kind| matches!(kind, Opens::File | Opens::Folder))
    }

    /// Whether the program opens web addresses.
    pub fn opens_addresses(&self) -> bool {
        self.opens.contains(&Opens::Url)
    }
}

/// The followed programs of `root` that say they open a file, a folder or both, in the order of
/// their names.
pub fn openers(root: &Path) -> Vec<Opener> {
    among(
        root,
        store::followed_names(root)
            .into_iter()
            .map(|(name, _)| name),
    )
}

/// The programs of `names`, followed ones of `root`, that say they open a file, a folder or both:
/// for a caller that has read the followed names already.
pub fn among(root: &Path, names: impl IntoIterator<Item = String>) -> Vec<Opener> {
    names
        .into_iter()
        .filter_map(|name| {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn opener(opens: &[Opens]) -> Opener {
        Opener {
            name: "p".to_owned(),
            opens: opens.to_vec(),
        }
    }

    /// A program that opens addresses only is not offered paths (Settings ▸ Apps ▸ Files lists the
    /// ones that are), and one that opens paths is not an address's.
    #[test]
    fn a_program_that_opens_addresses_only_is_not_one_a_path_is_offered_to() {
        let browser = opener(&[Opens::Url]);
        assert!(!browser.opens_paths() && browser.opens_addresses());
        let editor = opener(&[Opens::File, Opens::Folder]);
        assert!(editor.opens_paths() && !editor.opens_addresses());
        let both = opener(&[Opens::File, Opens::Url]);
        assert!(both.opens_paths() && both.opens_addresses());
    }
}
