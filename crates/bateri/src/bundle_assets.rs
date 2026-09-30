//! Content check of the bundle inputs (006 phase-4).
//!
//! Not a separate `tests/` test but part of the bin's unit-test bundle: that
//! bundle is already wired into `make check`. Had it lived under `tests/`,
//! cargo would link the application binary separately on every run
//! (`CARGO_BIN_EXE_*`).
//!
//! Why it exists: `alacritty_terminal` is Apache-2.0 and the license text must
//! ship with the `.app`. If the text or the attribution is deleted no build,
//! clippy or smoke run turns red — the violation is **silent**. This test runs
//! in `make check` and checks the inputs (`assets/bundle/`, `assets/shell/`).
//!
//! What it does not cover: whether the inputs are **copied into the package**.
//! The product is born only in `make bundle` and `bundle`'s own check sees it;
//! it is not repeated here, because for the test to build the product it would
//! need a release build.
//!
//! `plutil` is a macOS tool; this crate only compiles on macOS anyway
//! (`bt-shell-macos` → AppKit).

use std::path::{Path, PathBuf};
use std::process::Command;

fn asset(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/bundle")
        .join(name)
}

/// The input directory of the zsh wrapper (`assets/shell/zsh`).
fn shell_asset_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/shell/zsh")
}

fn read_asset(name: &str) -> String {
    let path = asset(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} could not be read: {e}", path.display()))
}

/// The raw value of a single key from the template; `None` if the key is missing.
fn plist_value(key: &str) -> Option<String> {
    let out = Command::new("plutil")
        .args(["-extract", key, "raw", "-o", "-"])
        .arg(asset("Info.plist.in"))
        .output()
        .expect("plutil could not be run");
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

#[test]
fn info_plist_template_launches_the_binary() {
    let lint = Command::new("plutil")
        .arg("-lint")
        .arg(asset("Info.plist.in"))
        .output()
        .expect("plutil could not be run");
    assert!(
        lint.status.success(),
        "Info.plist.in is not a valid plist: {}{}",
        String::from_utf8_lossy(&lint.stdout),
        String::from_utf8_lossy(&lint.stderr)
    );
    // The executable name is read from the bin target, not written by hand: if
    // the two diverge LaunchServices cannot open the package and the symptom is
    // Finder's "the application cannot be opened" message, not a build failure.
    assert_eq!(
        plist_value("CFBundleExecutable").as_deref(),
        Some(env!("CARGO_BIN_NAME"))
    );
    assert_eq!(plist_value("CFBundlePackageType").as_deref(), Some("APPL"));
    assert!(
        plist_value("CFBundleIdentifier").is_some_and(|id| !id.is_empty()),
        "CFBundleIdentifier is missing"
    );
    // A GPU-drawn terminal must not open blurry on Retina.
    assert_eq!(
        plist_value("NSHighResolutionCapable").as_deref(),
        Some("true")
    );
    let icon = plist_value("CFBundleIconFile").expect("CFBundleIconFile is missing");
    assert!(
        asset(&format!("{icon}.png")).is_file(),
        "icon source is missing: assets/bundle/{icon}.png"
    );
}

/// The version and the macOS floor are **not written** into the template:
/// `make bundle` fills the version from `Cargo.toml` and the floor from the
/// binary's `minos` (which itself comes from `.cargo/config.toml`). Seeing a
/// plain `14.0` in the template would mean the package keeps the old number
/// when the floor is raised. The placeholders have no explanation inside the
/// template, because `sed` would fill the explanation too and leak it into the
/// product; the explanation is in the `Makefile`'s `bundle` comment.
#[test]
fn info_plist_template_derives_version_and_minimum_os() {
    assert_eq!(
        plist_value("LSMinimumSystemVersion").as_deref(),
        Some("@MACOS_MIN@")
    );
    assert_eq!(
        plist_value("CFBundleShortVersionString").as_deref(),
        Some("@VERSION@")
    );
    assert_eq!(plist_value("CFBundleVersion").as_deref(), Some("@VERSION@"));
}

/// Sparkle's three keys. The feed is a placeholder, because `make bundle`
/// fills it from `FEED_URL` (so it can be overridden to another address in a
/// trial); the public key is fixed — if it changes, installed copies reject the
/// signature of new versions, so a diff that changes it must be forced to
/// change this test too. The bundle identifier is also Sparkle's yardstick: an
/// update installs only onto the same `CFBundleIdentifier`.
#[test]
fn info_plist_template_carries_the_updater_keys() {
    assert_eq!(plist_value("SUFeedURL").as_deref(), Some("@FEED_URL@"));
    assert_eq!(
        plist_value("SUPublicEDKey").as_deref(),
        Some("WC9PPr7SL5v2LvmQShrOYVEawDoB5wWngrinpVZE6Tw=")
    );
    assert_eq!(
        plist_value("SUEnableAutomaticChecks").as_deref(),
        Some("true")
    );
    assert_eq!(
        plist_value("CFBundleIdentifier").as_deref(),
        Some("dev.bateri.bateri")
    );
}

/// The zsh wrapper's inventory is **exactly** these five files.
///
/// The "nothing missing" half is also asked by `bt-shell-common` (the test of
/// `child::zsh_wrapper_dir`); the half this one alone sees is **the excess**.
/// It comes in two kinds and both are silent:
///
/// - `make bundle` runs through two hand-written lists (copy and `cmp`). A new
///   entry that did not make it into the lists never enters the package, and the
///   product check does not look for it either — the gate stays green, the
///   wrapper is installed incomplete.
/// - An arm that **writes** into the directory: ZDOTDIR points here for a while
///   during the session and in 009 phase-3 `/etc/zshrc` really did spawn a
///   `.zsh_history` once. It was a path by which user data would enter the
///   product.
///
/// `.DS_Store` is not counted: Finder produces it, it does not enter the repo
/// (`.gitignore`) and since the copy lines count the names one by one it cannot
/// leak into the package. A gate that fails while the code is right would cost
/// more than the defect it sees.
#[test]
fn zsh_wrapper_inventory_is_exactly_what_the_bundle_copies() {
    let dir = shell_asset_dir();
    let mut found: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{} could not be read: {e}", dir.display()))
        .map(|entry| {
            entry
                .expect("directory entry could not be read")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name != ".DS_Store")
        .collect();
    found.sort();
    assert_eq!(
        found,
        [".zlogin", ".zprofile", ".zshenv", ".zshrc", "bateri.zsh"],
        "the assets/shell/zsh inventory changed; `make bundle`'s copy and cmp \
         lists must be updated too"
    );
}

/// GPL-3.0 §4: everyone who receives the binary is given a copy of the
/// license. The text is `LICENSE` at the repo root (gnu.org's text, `make
/// bundle` copies it into the package and `cmp`s it), the About panel
/// (`Credits.html`) states the license and where the source is, and the
/// manifest's SPDX is the same license — if any of the three diverges no build
/// turns red, the violation is silent.
#[test]
fn own_license_ships_with_notice() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let license = std::fs::read_to_string(root.join("LICENSE")).expect("LICENSE could not be read");
    assert!(
        license.starts_with("                    GNU GENERAL PUBLIC LICENSE\n                       Version 3, 29 June 2007"),
        "LICENSE is not the gnu.org text of GPL-3.0"
    );
    let credits = read_asset("Credits.html");
    for needle in [
        "GNU General Public License, version 3",
        "or any later version",
        "github.com/bateri/bateri",
        "Contents/Resources/LICENSE",
    ] {
        assert!(
            credits.contains(needle),
            "{needle:?} is not in Credits.html"
        );
    }
    assert_eq!(env!("CARGO_PKG_LICENSE"), "GPL-3.0-or-later");
}

/// Apache-2.0 §4(a) and MIT: the recipient is given a copy of the license (the
/// file is the output of `tools/third_party_notices.py`). The attribution text
/// (`Credits.html`) is the file AppKit's standard About panel reads.
#[test]
fn third_party_license_ships_with_attribution() {
    let licenses = read_asset("THIRD-PARTY-LICENSES.txt");
    for needle in [
        "alacritty_terminal",
        "Apache License",
        "Version 2.0",
        "MIT License",
        "Sparkle",
    ] {
        assert!(
            licenses.contains(needle),
            "{needle:?} is not in THIRD-PARTY-LICENSES.txt"
        );
    }
    let credits = read_asset("Credits.html");
    for needle in [
        "alacritty_terminal",
        "Apache License",
        "Sparkle",
        "MIT License",
        "THIRD-PARTY-LICENSES.txt",
    ] {
        assert!(
            credits.contains(needle),
            "{needle:?} is not in Credits.html"
        );
    }
}

/// AppKit's About panel imports `Credits.html` without a declared charset
/// and decodes it as Latin-1, so any raw non-ASCII byte in the rendered
/// markup shows up as mojibake (`©` → `Â©`, `Ö` → `Ã–`; seen by the user
/// 2026-09-30). Non-ASCII text must be written as HTML entities; only the
/// leading comment, which is never rendered, may carry raw UTF-8.
#[test]
fn credits_markup_is_ascii() {
    let credits = read_asset("Credits.html");
    let body = credits
        .split_once("-->")
        .map_or(credits.as_str(), |(_, rest)| rest);
    for (i, line) in body.lines().enumerate() {
        assert!(
            line.is_ascii(),
            "Credits.html markup line {} has raw non-ASCII; use an HTML entity: {line:?}",
            i + 1
        );
    }
}
