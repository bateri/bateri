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
use crate::sfnt::{self, Style};
use crate::system::{Backend, Font, FontSystem};

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
    /// The faces of the files that **were** registered (the symbol font's excepted: no page is
    /// set in it), for the web page of a program's view:
    /// its web engine runs in another process, which never sees what this one registered, so
    /// it is given these files and told which face each is. A family the machine already had
    /// is not here (its files were never registered), so the page keeps the user's copy too.
    pub faces: Vec<AppFace>,
}

/// One registered font file, as a web page declares it (`@font-face`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppFace {
    /// The family the file declares about itself.
    pub family: String,
    /// The file.
    pub file: PathBuf,
    /// Italic or oblique.
    pub italic: bool,
    /// The weights it covers on the CSS scale, lowest and highest: the same value twice for a
    /// static face, the span of the `wght` axis for a variable one.
    pub weight: (u16, u16),
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
        let outcome = carry_out(
            plan(groups, family_resolves),
            Backend::register_file,
            sfnt::style_of,
        );
        // Not "the pass registered the default": the user's own copy of it
        // counts, the pass left it alone on purpose.
        if family_resolves(DEFAULT_FAMILY) {
            DEFAULT_AVAILABLE.store(true, Ordering::Release);
        }
        outcome
    })
}

/// What [`register_app_fonts`] did, if it ran: the same answer it gave. `None` in a
/// process that never ran it.
pub fn app_fonts() -> Option<&'static AppFonts> {
    OUTCOME.get()
}

/// Registers the files of `plan` with `register` and says what happened: a family whose
/// files all registered is `registered`, a file that did not is `refused`, and every
/// registered file whose style `style_of` can read is a face, except the symbol font's. The
/// families the plan left alone have no faces by construction.
pub(crate) fn carry_out(
    plan: Plan,
    register: impl Fn(&Path) -> bool,
    style_of: impl Fn(&Path) -> Option<Style>,
) -> AppFonts {
    let mut outcome = AppFonts {
        kept: plan.kept,
        ..AppFonts::default()
    };
    for (family, files) in plan.register {
        let mut all = true;
        for file in files {
            if !register(&file) {
                all = false;
                outcome.refused.push(file);
                continue;
            }
            // The symbol font is no family to set a page in, and the Font list leaves it out too.
            if family == SYMBOL_FAMILY {
                continue;
            }
            if let Some(Style { italic, weight }) = style_of(&file) {
                outcome.faces.push(AppFace {
                    family: family.clone(),
                    file,
                    italic,
                    weight,
                });
            }
        }
        if all {
            outcome.registered.push(family);
        }
    }
    outcome
}

/// Did [`register_app_fonts`] run in this process, and does the app's default
/// family open by its name (its own file or the user's copy)?
pub fn app_fonts_registered() -> bool {
    DEFAULT_AVAILABLE.load(Ordering::Acquire)
}

/// The family the app starts with when no `family` is written, if the app's
/// fonts are there ([`app_fonts_registered`]): what the Font list calls the
/// default. `None`: the system's own chain (SF Mono, else Menlo) is.
pub fn app_default_family() -> Option<&'static str> {
    app_fonts_registered().then_some(DEFAULT_FAMILY)
}

/// Opens the font no `family` asks for: the app's default family where the
/// app's fonts are registered, the system's chain everywhere else (a test
/// binary, a build without the directory). Every place that opens "the
/// default" comes through here, so the grid, the font options and the
/// proportional text agree on which font that is.
pub(crate) fn open_default(size: f64) -> (Font, String) {
    if app_fonts_registered() {
        let (font, returned) = Backend::open(DEFAULT_FAMILY, size);
        if same_family(&returned, DEFAULT_FAMILY) {
            return (font, returned);
        }
    }
    Backend::open_default(size)
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

    /// Every made-up file is regular, except the ones named `Italic`; a name with `Var` covers
    /// 100 to 800.
    fn styled(path: &Path) -> Option<Style> {
        let name = path.file_stem()?.to_str()?;
        Some(Style {
            italic: name.contains("Italic"),
            weight: if name.contains("Var") {
                (100, 800)
            } else {
                (400, 400)
            },
        })
    }

    #[test]
    fn only_the_files_that_were_registered_become_faces_of_the_page() {
        let files = paths(&[
            "a/Mine-Regular.ttf",
            "a/Mine-Italic.ttf",
            "b/Ours-Var.ttf",
            "b/Ours-VarItalic.ttf",
            "c/Torn-Regular.ttf",
            "c/Torn-Bold.ttf",
        ]);
        // The user has Mine; the system refuses one file of Torn.
        let plan = plan(group(&files, declared), |name| name == "Mine");
        let outcome = carry_out(plan, |file| !file.ends_with("Torn-Bold.ttf"), styled);
        assert_eq!(outcome.kept, ["Mine"]);
        assert_eq!(outcome.registered, ["Ours"]);
        assert_eq!(outcome.refused, paths(&["c/Torn-Bold.ttf"]));
        let faces: Vec<(&str, &str, bool, (u16, u16))> = outcome
            .faces
            .iter()
            .map(|face| {
                let name = face.file.to_str().unwrap();
                (face.family.as_str(), name, face.italic, face.weight)
            })
            .collect();
        assert_eq!(
            faces,
            [
                ("Ours", "b/Ours-Var.ttf", false, (100, 800)),
                ("Ours", "b/Ours-VarItalic.ttf", true, (100, 800)),
                // The family that lost a file keeps the face of the one that registered: it is
                // what the grid has of it.
                ("Torn", "c/Torn-Regular.ttf", false, (400, 400)),
            ],
            "nothing of the user's family, nothing of the refused file"
        );
    }

    #[test]
    fn the_symbol_font_registers_and_is_no_face_of_a_page() {
        let files = paths(&["a/Symbols Nerd Font Mono.ttf", "a/Ours.ttf"]);
        let outcome = carry_out(plan(group(&files, declared), |_| false), |_| true, styled);
        assert!(outcome.registered.contains(&SYMBOL_FAMILY.to_owned()));
        let families: Vec<&str> = outcome.faces.iter().map(|f| f.family.as_str()).collect();
        assert_eq!(families, ["Ours"]);
    }

    #[test]
    fn a_file_whose_style_cannot_be_read_registers_and_has_no_face() {
        let plan = plan(group(&paths(&["a/Ours.ttf"]), declared), |_| false);
        let outcome = carry_out(plan, |_| true, |_| None);
        assert_eq!(outcome.registered, ["Ours"]);
        assert!(outcome.faces.is_empty());
    }

    /// A process that never ran the launch's pass keeps the system's chain for
    /// the default font, whatever else it registered: the lib tests and
    /// `raster_digest` rest on the chain staying SF Mono / Menlo. The test
    /// fonts are registered here on purpose — JetBrains Mono is among them and
    /// would be the default if registering any font were enough.
    #[cfg(target_os = "macos")]
    #[test]
    fn registering_the_test_fonts_does_not_change_the_default_font() {
        crate::system::fixture::register_test_fonts();
        assert!(!app_fonts_registered());
        assert_eq!(app_default_family(), None);
        let (_, name) = open_default(12.0);
        let (_, chain) = Backend::open_default(12.0);
        assert_eq!(name, chain);
        assert!(!same_family(&name, DEFAULT_FAMILY));
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

    /// A program's page asks for a font by its file's name, so two shipped files of one name
    /// would be one font to it.
    #[test]
    fn the_shipped_files_have_names_of_their_own() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts");
        let names: Vec<String> = font_files(&dir)
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(names.len() >= 20, "{names:?}");
        let mut sorted = names.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len(), "{names:?}");
    }

    #[test]
    fn a_missing_directory_has_no_fonts() {
        assert!(font_files(Path::new("/nonexistent/bt-app-fonts")).is_empty());
    }
}
