//! Content check of the bundle inputs.
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

/// The input directory of the fish wrapper (`assets/shell/fish`).
fn fish_asset_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/shell/fish")
}

/// The input directory of the bash wrapper (`assets/shell/bash`).
fn bash_asset_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/shell/bash")
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

/// The entitlements `make bundle` signs the application with are the two the page sandbox's
/// runtime needs under the hardened runtime and nothing more: an entitlement is a permission the
/// package holds for as long as it is signed with it, so the set is not widened by editing the
/// file in passing (and not narrowed either: without the second the runtime is killed the moment
/// it makes the code it compiled executable).
#[test]
fn entitlements_are_the_two_the_page_sandbox_needs() {
    let path = asset("bateri.entitlements");
    let lint = Command::new("plutil")
        .arg("-lint")
        .arg(&path)
        .output()
        .expect("plutil could not be run");
    assert!(
        lint.status.success(),
        "bateri.entitlements is not a valid plist: {}{}",
        String::from_utf8_lossy(&lint.stdout),
        String::from_utf8_lossy(&lint.stderr)
    );
    let json = Command::new("plutil")
        .args(["-convert", "json", "-o", "-"])
        .arg(&path)
        .output()
        .expect("plutil could not be run");
    let json = String::from_utf8_lossy(&json.stdout);
    for key in ["allow-jit", "allow-unsigned-executable-memory"] {
        assert!(
            json.contains(&format!(r#""com.apple.security.cs.{key}":true"#)),
            "the entitlement {key} is missing: {json}"
        );
    }
    assert_eq!(
        json.matches(r#"":"#).count(),
        2,
        "an entitlement beyond the two: {json}"
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

/// The zsh wrapper's inventory is **exactly** these six files (the ZDOTDIR
/// swap, `zdotdir.zsh`, joined with remote shell integration).
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
///   during the session and during development `/etc/zshrc` really did spawn a
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
        [
            ".zlogin",
            ".zprofile",
            ".zshenv",
            ".zshrc",
            "bateri.zsh",
            "zdotdir.zsh"
        ],
        "the assets/shell/zsh inventory changed; `make bundle`'s copy and cmp \
         lists must be updated too"
    );
}

/// The fish wrapper's inventory is **exactly** one file, `vendor_conf.d/bateri.fish`.
///
/// For the reasons of the zsh test above: `make bundle` copies the file by name
/// and `cmp`s it, so a second file added to the directory never enters the
/// package while everything stays green. fish makes it matter more than zsh
/// does — the package's `shell/` directory is an `XDG_DATA_DIRS` entry, and fish
/// reads a `functions/` or `completions/` directory in it just as it reads
/// `vendor_conf.d`: whatever lands in this tree runs in the user's shell.
#[test]
fn fish_wrapper_inventory_is_exactly_what_the_bundle_copies() {
    fn walk(dir: &Path, prefix: &str, found: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("{} could not be read: {e}", dir.display()))
        {
            let entry = entry.expect("directory entry could not be read");
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == ".DS_Store" {
                continue;
            }
            let path = format!("{prefix}{name}");
            if entry.path().is_dir() {
                walk(&entry.path(), &format!("{path}/"), found);
            } else {
                found.push(path);
            }
        }
    }
    let mut found = Vec::new();
    walk(&fish_asset_dir(), "", &mut found);
    found.sort();
    assert_eq!(
        found,
        ["vendor_conf.d/bateri.fish"],
        "the assets/shell/fish inventory changed; `make bundle`'s copy and cmp \
         lists must be updated too"
    );
}

/// The bash wrapper's inventory is **exactly** one file, `bateri.bash`.
///
/// For the reasons of the zsh test above: `make bundle` copies the file by name and `cmp`s it,
/// so a second file added to the directory never enters the package while everything stays
/// green. `ENV` names the one file; nothing else in the directory is ever read.
#[test]
fn bash_wrapper_inventory_is_exactly_what_the_bundle_copies() {
    let mut found: Vec<String> = std::fs::read_dir(bash_asset_dir())
        .expect("assets/shell/bash could not be read")
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
        ["bateri.bash"],
        "the assets/shell/bash inventory changed; `make bundle`'s copy and cmp \
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
        "iTerm2-Color-Schemes",
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
        "iTerm2-Color-Schemes",
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

/// The package carries kablo's files where the application looks for them, for
/// all four targets, and takes them from a release directory it checks.
///
/// The files themselves are not in this repository (a kablo release is the
/// sibling repository's `dist`), so this reads the recipe and the code that opens
/// the package: if one moves the directory, or drops a target, the other is wrong
/// and no run notices — a server would get no kablo, and every `ssh` would be the
/// plain one without a word.
#[test]
fn the_package_carries_kablo_where_the_application_looks_for_it() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let makefile = std::fs::read_to_string(root.join("Makefile")).expect("Makefile");
    assert!(
        makefile.contains("KABLO_DIST ?= ../kablo/dist"),
        "the development source of kablo's release moved"
    );
    assert!(
        makefile.contains("$(STAGE)/Contents/Resources/kablo"),
        "the package no longer puts kablo's files in Resources/kablo"
    );
    let service = std::fs::read_to_string(root.join("crates/bt-shell-macos/src/hold_service.rs"))
        .expect("hold_service.rs");
    assert!(
        service.contains("Resources/kablo"),
        "the application no longer opens kablo's files in Resources/kablo"
    );
    let targets = makefile
        .lines()
        .find_map(|line| line.strip_prefix("KABLO_TARGETS = "))
        .expect("the targets the recipe requires");
    let mut found: Vec<&str> = targets.split_whitespace().collect();
    found.sort_unstable();
    assert_eq!(
        found,
        [
            "aarch64-apple-darwin",
            "aarch64-unknown-linux-musl",
            "x86_64-apple-darwin",
            "x86_64-unknown-linux-musl"
        ],
        "kablo's targets are the four `kablo_proto::install::Target::ALL` names"
    );
    // The recipe never names a kablo version: the release's names say it.
    assert!(
        !makefile
            .lines()
            .any(|line| line.contains("kablo-0.") || line.contains("kablo_version")),
        "a kablo version is written in the Makefile"
    );
}
