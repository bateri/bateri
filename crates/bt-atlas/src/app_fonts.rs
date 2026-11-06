//! The fonts that ship with the app, and how they reach the Font list.
//!
//! The app's `fonts` directory holds one directory per family. The fonts are
//! registered for **this process only** (the machine's font list, and with it
//! every other app's, is not touched), once at launch, before the first pane
//! opens its atlas and before the Font list is taken: a family registered
//! later would not show until the next launch.
//!
//! **A copy the user installed comes first.** A family that already opens by
//! its name is left alone: its files are not registered, so the user's own
//! version, hinting and rules are what they see. The question is asked per
//! family and before any of its files is registered, with the family name the
//! files declare about themselves (the directory name is not trusted).
//!
//! The decision is platformless and takes its two questions as arguments
//! ([`plan`]), so it is tested without a font system; only the registration
//! itself reaches [`Backend`].

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::rules::same_family;
use crate::system::{Backend, FontSystem};

/// The family the app opens when no `family` is written.
pub(crate) const DEFAULT_FAMILY: &str = "JetBrains Mono";

/// The symbol font: monospaced, but nothing to type in. It is left out of the
/// Font list by this name, and it is where private-use-area symbols come from
/// when the selected font has none.
pub(crate) const SYMBOL_FAMILY: &str = "Symbols Nerd Font Mono";

/// The point size a family is probed at: the answer does not depend on it.
const PROBE_SIZE: f64 = 12.0;

/// Set once the pass has run and [`DEFAULT_FAMILY`] opens by its name: the
/// only thing that tells the rest of the crate the app's fonts are there. A
/// process that never ran the pass (a test binary, a build without the
/// directory) never has it, whatever the machine has installed.
static DEFAULT_AVAILABLE: AtomicBool = AtomicBool::new(false);

static OUTCOME: OnceLock<AppFonts> = OnceLock::new();

/// What [`register_app_fonts`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AppFonts {
    /// The families whose files were registered.
    pub registered: Vec<String>,
    /// The families the machine already had under that name: the app's copy is
    /// left out.
    pub kept: Vec<String>,
    /// The files the font system refused.
    pub refused: Vec<PathBuf>,
}

/// What the pass will do with the families it found.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Plan {
    /// Family and files, for the families to register.
    pub register: Vec<(String, Vec<PathBuf>)>,
    /// The families to leave alone: the user has them.
    pub kept: Vec<String>,
}

/// Registers the fonts under `dir` (`dir/{Family}/*.ttf`, `.otf`) for this
/// process, once: a second call answers what the first did, whatever `dir`.
///
/// On a backend that cannot register a file (Linux) nothing is registered.
pub fn register_app_fonts(dir: &Path) -> &'static AppFonts {
    OUTCOME.get_or_init(|| {
        let groups = group(&font_files(dir), Backend::file_family);
        let Plan { register, kept } = plan(groups, family_resolves);
        let mut outcome = AppFonts {
            kept,
            ..AppFonts::default()
        };
        for (family, files) in register {
            let mut all = true;
            for file in files {
                if !Backend::register_file(&file) {
                    all = false;
                    outcome.refused.push(file);
                }
            }
            if all {
                outcome.registered.push(family);
            }
        }
        // Not "the pass registered the default": the user's own copy of it
        // counts, the pass left it alone on purpose.
        if family_resolves(DEFAULT_FAMILY) {
            DEFAULT_AVAILABLE.store(true, Ordering::Release);
        }
        outcome
    })
}

/// Did [`register_app_fonts`] run in this process, and does the app's default
/// family open by its name (its own file or the user's copy)?
pub fn app_fonts_registered() -> bool {
    DEFAULT_AVAILABLE.load(Ordering::Acquire)
}

/// Does `name` open as itself? `Backend::open` never fails; the family the
/// opened font reports about itself is what answers.
fn family_resolves(name: &str) -> bool {
    let (_, returned) = Backend::open(name, PROBE_SIZE);
    same_family(&returned, name)
}

/// The font files one level below `dir`, in a fixed order. A file starting
/// with a dot (`.DS_Store`, an AppleDouble `._` sibling) is not one.
pub(crate) fn font_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(families) = std::fs::read_dir(dir) else {
        return files;
    };
    for family in families.flatten() {
        let Ok(entries) = std::fs::read_dir(family.path()) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let hidden = entry.file_name().to_string_lossy().starts_with('.');
            let font = path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    ["ttf", "otf", "ttc", "otc"]
                        .iter()
                        .any(|known| extension.eq_ignore_ascii_case(known))
                });
            if font && !hidden {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

/// `files` grouped by the family each declares. A file whose family cannot be
/// read is not a font the pass can use and is left out.
pub(crate) fn group(
    files: &[PathBuf],
    family_of: impl Fn(&Path) -> Option<String>,
) -> BTreeMap<String, Vec<PathBuf>> {
    let mut groups: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for file in files {
        if let Some(family) = family_of(file) {
            groups.entry(family).or_default().push(file.clone());
        }
    }
    groups
}

/// Which families to register: those that do not already open by their name.
/// `resolves` is asked once per family, before any of its files is registered.
pub(crate) fn plan(
    groups: BTreeMap<String, Vec<PathBuf>>,
    resolves: impl Fn(&str) -> bool,
) -> Plan {
    let mut plan = Plan::default();
    for (family, files) in groups {
        if resolves(&family) {
            plan.kept.push(family);
        } else {
            plan.register.push((family, files));
        }
    }
    plan
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(PathBuf::from).collect()
    }

    /// The family a made-up file name declares: the part before the dash.
    fn declared(path: &Path) -> Option<String> {
        let stem = path.file_stem()?.to_str()?;
        (stem != "broken").then(|| stem.split('-').next().unwrap_or(stem).to_owned())
    }

    #[test]
    fn files_are_grouped_by_the_family_they_declare_not_by_where_they_lie() {
        let files = paths(&["a/Mono-Regular.ttf", "b/Mono-Italic.ttf", "c/Other.otf"]);
        let groups = group(&files, declared);
        assert_eq!(
            groups["Mono"],
            paths(&["a/Mono-Regular.ttf", "b/Mono-Italic.ttf"])
        );
        assert_eq!(groups["Other"], paths(&["c/Other.otf"]));
    }

    #[test]
    fn a_file_whose_family_cannot_be_read_is_left_out() {
        let groups = group(&paths(&["a/broken.ttf", "a/Mono.ttf"]), declared);
        assert_eq!(groups.len(), 1);
        assert!(groups.contains_key("Mono"));
    }

    #[test]
    fn a_family_the_user_already_has_is_not_registered() {
        let files = paths(&["a/Mine-Regular.ttf", "a/Mine-Bold.ttf", "b/Ours.ttf"]);
        let plan = plan(group(&files, declared), |name| name == "Mine");
        assert_eq!(plan.kept, ["Mine"]);
        assert_eq!(plan.register, [("Ours".to_owned(), paths(&["b/Ours.ttf"]))]);
    }

    #[test]
    fn every_family_is_asked_once_before_anything_is_registered() {
        let asked = RefCell::new(Vec::new());
        let files = paths(&["a/One.ttf", "a/One-Bold.ttf", "b/Two.ttf"]);
        let _ = plan(group(&files, declared), |name| {
            asked.borrow_mut().push(name.to_owned());
            false
        });
        assert_eq!(*asked.borrow(), ["One", "Two"]);
    }

    #[test]
    fn nothing_registers_when_the_user_has_every_family() {
        let files = paths(&["a/One.ttf", "b/Two.ttf"]);
        let plan = plan(group(&files, declared), |_| true);
        assert!(plan.register.is_empty());
        assert_eq!(plan.kept, ["One", "Two"]);
    }

    #[test]
    fn hidden_and_foreign_files_are_not_fonts() {
        let root = std::env::temp_dir().join(format!("bt-app-fonts-{}", std::process::id()));
        let family = root.join("Mono");
        std::fs::create_dir_all(&family).unwrap();
        for name in [
            "Mono.ttf",
            "Mono.OTF",
            "._Mono.ttf",
            ".DS_Store",
            "OFL.txt",
            "README",
        ] {
            std::fs::write(family.join(name), b"x").unwrap();
        }
        let found: Vec<String> = font_files(&root)
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(found, ["Mono.OTF", "Mono.ttf"]);
    }

    #[test]
    fn a_missing_directory_has_no_fonts() {
        assert!(font_files(Path::new("/nonexistent/bt-app-fonts")).is_empty());
    }
}
