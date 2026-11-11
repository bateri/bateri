//! What a ⌘-clicked link **is** on disk and what opening it does.
//!
//! `bt-core` finds the link and gives its raw target ([`bt_core::LinkHit`]): a URL, a path
//! **candidate** with its `:line:col` suffix already split off, or an OSC 8 URI. Whether the
//! path exists and what the click does are decided here, in pure functions:
//!
//! - [`local_path`] — the file-system text a link names: the path candidate itself, or the
//!   percent-decoded path of a `file://` URL (`bt-core` already dropped foreign authorities and
//!   every `file://` of a remote session).
//! - [`resolve`] — that text → an existing [`Resolved`] (`~` to the home directory, a relative
//!   path to the pane's OSC 7 directory); the `stat` is **injected**, so the tests use a fake
//!   and the shell passes [`stat`]. **A path that does not exist is not a link.** The `stat`
//!   also follows the path's symlinks to the object they name ([`Resolved::target`]): a link's
//!   own name says nothing about what opening it opens.
//! - [`resolve_first`] — a path query's candidates (`My Drive`'s `Drive`, …, `My Drive`) →
//!   the first that exists, iTerm2's semantic history.
//! - [`action`] — the policy for a link that is not a path: `http`, `https`, `ftp` and `mailto`
//!   open, an OSC 8 scheme off the common list asks first, `bateri://` is swallowed on every
//!   path. **A path has no action here**: a click on it opens in the program the person chose
//!   (`open_with`, decided in [`crate::open_with`]), on the computer the path is on, and
//!   nothing of it is handed to the system to open.
//!
//! This module has no queue, no `dispatch2` and no AppKit (`make audit`). The background queue
//! and the return to the main queue are the platform shell's, the `pane::RemoteProbe`
//! precedent.
//! [`hostname`] reads the machine's name for `SessionOptions::hostname` — `child`'s precedent
//! of a thin system read next to the policy.

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use bt_core::LinkKind;

/// What a file-system object is — the answer of the injected `stat`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    /// A directory (a package such as `.app` too).
    Dir,
    /// Anything else that exists; `executable` is any of the three `x` bits.
    File { executable: bool },
}

/// A path that exists, with what it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    /// The path as the link names it — a symlink stays the symlink: what is revealed in
    /// Finder, copied and handed to the pane's owner.
    pub path: PathBuf,
    /// What [`Resolved::target`] is.
    pub entry: Entry,
    /// The object `path` names once every symlink (and `..`) on it is followed — `realpath`,
    /// read by the same background `stat` as `entry`. **What a click opens is judged here and
    /// opened from here**, never from `path`: the program is handed the object the path names,
    /// and whether it is a file or a folder is that object's. The macOS shell's `stat`
    /// resolves a **Finder alias** before this one runs — an alias is a regular file
    /// `canonicalize` does not follow. `path` itself when nothing on it is a link, an alias
    /// or a `..`.
    pub target: PathBuf,
}

/// What a ⌘-click does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkAction {
    /// Hand the URL to its default application (`http`, `https`, `ftp`, `mailto`).
    OpenUrl(String),
    /// An OSC 8 URI with an uncommon scheme: ask with the whole target before opening.
    Confirm(String),
    /// `bateri://`: our own scheme is never opened from a click (sending it to ourselves
    /// means nothing).
    Swallow,
}

/// The URL schemes that open without asking — the plain-text scanner's list minus `file`,
/// which is resolved as a path.
const OPEN_SCHEMES: [&str; 4] = ["http", "https", "ftp", "mailto"];

/// The URL's scheme (the text before the first `:`), if it has a valid one.
fn scheme(url: &str) -> Option<&str> {
    let (scheme, _) = url.split_once(':')?;
    let mut chars = scheme.chars();
    let first = chars.next()?;
    (first.is_ascii_alphabetic()
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')))
    .then_some(scheme)
}

fn has_scheme(url: &str, name: &str) -> bool {
    scheme(url).is_some_and(|scheme| scheme.eq_ignore_ascii_case(name))
}

/// The file-system text a link names, if it names one: the path candidate as is, or the
/// decoded path of a `file://` URL (plain text or OSC 8). `None` for every other URL, for a
/// `file://` URL without a path and for a malformed percent escape.
///
/// The authority is **not** checked here: `bt-core`'s hit test already turned every
/// non-local `file://` (and every `file://` of a remote session) into "not a link"
/// (`shell::is_local_authority`, the single function OSC 7 shares).
pub fn local_path(target: &str, kind: &LinkKind) -> Option<PathBuf> {
    match kind {
        LinkKind::Path { .. } => Some(PathBuf::from(target)),
        LinkKind::Url | LinkKind::Osc8 => file_url_path(target),
    }
}

/// `file://AUTH/a%20b` → `/a b`. The query and the fragment are not part of the path; a
/// literal `?` or `#` in a file name arrives percent-encoded. The decoded bytes need not be
/// UTF-8 (a Unix path is bytes).
fn file_url_path(url: &str) -> Option<PathBuf> {
    if !has_scheme(url, "file") {
        return None;
    }
    let rest = url.get("file:".len()..)?.strip_prefix("//")?;
    let path = &rest[rest.find('/')?..];
    let path = path.split(['?', '#']).next().unwrap_or(path);
    let mut out = Vec::with_capacity(path.len());
    let mut bytes = path.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let high = (bytes.next()? as char).to_digit(16)?;
            let low = (bytes.next()? as char).to_digit(16)?;
            out.push((high * 16 + low) as u8);
        } else {
            out.push(byte);
        }
    }
    Some(PathBuf::from(OsString::from_vec(out)))
}

/// The candidate → the object on disk, or `None`: **a path that does not exist is not a
/// link** (iTerm2's semantic history).
///
/// - `~` and `~/…` go to `home`; `~user` is not expanded (not a link — it would need a passwd
///   lookup per hover and nothing in the scanner's output asks for it).
/// - An absolute path is taken as is.
/// - A relative path is joined to `cwd`, the pane's **current** OSC 7 directory — not the
///   directory the line was printed in (a named known limit).
///
/// The `:line:col` suffix is already off the candidate (`bt-core` splits it into
/// [`LinkKind::Path`]); it is **not** stripped again here, so a file whose name really ends
/// in `:12` still resolves. `stat` follows symlinks and answers with what it found there
/// ([`stat`]): the object and its path, [`Resolved::target`].
pub fn resolve(
    candidate: &Path,
    cwd: Option<&Path>,
    home: Option<&Path>,
    stat: impl Fn(&Path) -> Option<(Entry, PathBuf)>,
) -> Option<Resolved> {
    let text = candidate.as_os_str();
    if text.is_empty() {
        return None;
    }
    let path = if let Ok(rest) = candidate.strip_prefix("~") {
        let home = home?;
        if rest.as_os_str().is_empty() {
            home.to_path_buf()
        } else {
            home.join(rest)
        }
    } else if candidate.is_absolute() {
        candidate.to_path_buf()
    } else if candidate.to_str().is_some_and(|text| text.starts_with('~')) {
        // `~user/…`: see the doc.
        return None;
    } else {
        // `components` drops the `.` of `./src` (a cleaner path for Finder); `..` stays, a
        // lexical `..` would be wrong across a symlink.
        cwd.filter(|cwd| cwd.is_absolute())?
            .join(candidate)
            .components()
            .collect()
    };
    let (entry, target) = stat(&path)?;
    Some(Resolved {
        path,
        entry,
        target,
    })
}

/// A path query's answer (iTerm2's semantic history): the
/// **first** candidate that [`resolve`]s, with its index — the candidates come
/// from `bt-core` in the order they are to be tried
/// ([`bt_core::LinkHit::candidates`], shortest first), so `My Drive` wins only
/// when neither `Drive` nor `Drive ` exists. `None` if none does: no link.
///
/// Every candidate is one `stat` (at most 100 per query, `bt-core`'s bound); the
/// platform shell runs this on its background queue.
pub fn resolve_first<'a>(
    candidates: impl IntoIterator<Item = &'a Path>,
    cwd: Option<&Path>,
    home: Option<&Path>,
    stat: impl Fn(&Path) -> Option<(Entry, PathBuf)>,
) -> Option<(usize, Resolved)> {
    candidates
        .into_iter()
        .enumerate()
        .find_map(|(index, candidate)| {
            resolve(candidate, cwd, home, &stat).map(|resolved| (index, resolved))
        })
}

/// The production `stat`: `std::fs::canonicalize`, then `std::fs::metadata` of what it
/// found (symlinks followed — **a link to a script is a script, a link to an `.app` is an
/// `.app`**): what the object is and its real path ([`Resolved::target`]), both from one
/// follow. A Finder alias is not a symlink: the macOS shell's `stat` resolves it first and
/// then calls this one. Blocking: on a network disk it can hang, which is why the platform
/// shell calls it on a background queue, never on the main thread or in the frame path.
pub fn stat(path: &Path) -> Option<(Entry, PathBuf)> {
    let target = std::fs::canonicalize(path).ok()?;
    let meta = std::fs::metadata(&target).ok()?;
    let entry = if meta.is_dir() {
        Entry::Dir
    } else {
        Entry::File {
            executable: meta.permissions().mode() & 0o111 != 0,
        }
    };
    Some((entry, target))
}

/// The policy for a link that is not a path.
///
/// | link | action |
/// |---|---|
/// | `bateri:` anything | [`LinkAction::Swallow`] |
/// | local path / `file://` | `None` — a path has no action here: it opens in the program the person chose |
/// | `file:` without a `//` authority | [`LinkAction::Swallow`] |
/// | `http`/`https`/`ftp`/`mailto` | [`LinkAction::OpenUrl`] |
/// | OSC 8, any other scheme (or none) | [`LinkAction::Confirm`] |
///
/// The swallow row comes **first** and reads the target whatever the kind: the hit test does
/// not hand `bateri://` out today, the row is a one-line defence in depth.
///
/// A `file:` URL [`local_path`] cannot read (`file:/x`, no authority) is swallowed too:
/// handed to `NSWorkspace` as a URL it would **run** a `.command` or launch an `.app`, and the
/// hit test's remote/authority gate does not see it.
pub fn action(target: &str, kind: &LinkKind) -> Option<LinkAction> {
    if has_scheme(target, "bateri") {
        return Some(LinkAction::Swallow);
    }
    if local_path(target, kind).is_some() {
        return None;
    }
    if has_scheme(target, "file") {
        return Some(LinkAction::Swallow);
    }
    let common = OPEN_SCHEMES.iter().any(|name| has_scheme(target, name));
    Some(match kind {
        _ if common => LinkAction::OpenUrl(target.to_owned()),
        LinkKind::Osc8 => LinkAction::Confirm(target.to_owned()),
        // A plain-text URL outside the list cannot come from the scanner; if it ever does,
        // asking is the safe side.
        LinkKind::Url | LinkKind::Path { .. } => LinkAction::Confirm(target.to_owned()),
    })
}

/// This machine's name (`gethostname`) for `SessionOptions::hostname`: GNU `ls --hyperlink`
/// prints `file://$HOSTNAME/…` and OSC 7 may carry the same name, so the "is this authority
/// local" question needs it. `None` if the call fails or the name
/// is empty or not UTF-8 — the authority then falls back to the empty/`localhost` rule.
pub fn hostname() -> Option<String> {
    let mut buf = [0u8; 256];
    // SAFETY: the buffer is valid for `len` bytes; `gethostname` writes at most that many.
    let rc = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
    if rc != 0 {
        return None;
    }
    // POSIX does not promise a terminator on truncation: the name ends at the first NUL or at
    // the buffer's end.
    let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8(buf[..len].to_vec())
        .ok()
        .filter(|name| !name.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "/Users/me";
    const CWD: &str = "/work/proj";

    /// The fake disk: a script, a document, a directory and a package — and symlinks whose
    /// names lie about what they point to, followed as the production [`stat`] does.
    fn fake(path: &Path) -> Option<(Entry, PathBuf)> {
        let target = match path.to_str()? {
            "/work/proj/docs" => "/work/proj/.x/Evil.app",
            "/work/proj/README.md" => "/work/proj/payload.terminal",
            "/work/proj/latest.md" => "/work/proj/notes/2024.md",
            "/work/proj/current" => "/work/proj/src",
            other => other,
        };
        let entry = match target {
            "/work/proj/src/main.rs"
            | "/Users/me/notes.md"
            | "/work/proj/a:12"
            | "/work/proj/payload.terminal"
            | "/work/proj/notes/2024.md" => Entry::File { executable: false },
            "/work/proj/run.sh" => Entry::File { executable: true },
            "/Users/me" | "/work/proj/src" | "/Applications/Foo.app" | "/work/proj/.x/Evil.app" => {
                Entry::Dir
            }
            "/tmp/a b" => Entry::File { executable: false },
            _ => return None,
        };
        Some((entry, target.into()))
    }

    fn res(candidate: &str) -> Option<Resolved> {
        resolve(
            Path::new(candidate),
            Some(Path::new(CWD)),
            Some(Path::new(HOME)),
            fake,
        )
    }

    fn path_kind() -> LinkKind {
        LinkKind::Path {
            line: None,
            col: None,
        }
    }

    fn act(target: &str, kind: LinkKind) -> Option<LinkAction> {
        action(target, &kind)
    }

    #[test]
    fn relative_tilde_and_absolute_paths_resolve() {
        assert_eq!(
            res("src/main.rs"),
            Some(Resolved {
                path: "/work/proj/src/main.rs".into(),
                entry: Entry::File { executable: false },
                target: "/work/proj/src/main.rs".into(),
            })
        );
        assert_eq!(res("./src").map(|r| r.entry), Some(Entry::Dir));
        assert_eq!(
            res("~/notes.md").map(|r| r.path),
            Some("/Users/me/notes.md".into())
        );
        assert_eq!(res("~").map(|r| r.path), Some(HOME.into()));
        assert_eq!(
            res("/Applications/Foo.app").map(|r| r.entry),
            Some(Entry::Dir)
        );
    }

    #[test]
    fn the_first_existing_candidate_wins() {
        let disk = |path: &Path| {
            let entry = match path.to_str()? {
                "/work/proj/My Drive" | "/work/proj/Screen Studio Projects" => Entry::Dir,
                "/work/proj/Drive" | "/work/proj/foo.txt" => Entry::File { executable: false },
                _ => return None,
            };
            Some((entry, path.to_path_buf()))
        };
        let first = |candidates: &[&str]| {
            resolve_first(
                candidates.iter().map(Path::new),
                Some(Path::new(CWD)),
                Some(Path::new(HOME)),
                disk,
            )
            .map(|(index, resolved)| (index, resolved.path))
        };
        // `bt-core`'s order for `Screen Studio Projects` hovered on `Studio`.
        assert_eq!(
            first(&[
                "Studio",
                "Studio ",
                "Studio Projects",
                " Studio",
                "Screen Studio",
                "Screen Studio Projects",
                "My Drive    Screen Studio Projects",
            ]),
            Some((5, "/work/proj/Screen Studio Projects".into()))
        );
        // The shortest one that exists wins, even if a longer one exists too.
        assert_eq!(
            first(&["Drive", "My Drive"]),
            Some((0, "/work/proj/Drive".into()))
        );
        assert_eq!(
            first(&["My", "My ", "My Drive"]),
            Some((2, "/work/proj/My Drive".into()))
        );
        assert_eq!(
            first(&["foo.txt.", "foo.txt"]),
            Some((1, "/work/proj/foo.txt".into()))
        );
        assert_eq!(first(&["nope", "also nope"]), None);
        assert_eq!(first(&[]), None);
        // Each candidate is asked once, in order, and asking stops at the winner.
        let asked = std::cell::RefCell::new(Vec::new());
        let counting = |path: &Path| {
            asked.borrow_mut().push(path.to_path_buf());
            disk(path)
        };
        let found = resolve_first(
            ["a", "Drive", "b"].iter().map(Path::new),
            Some(Path::new(CWD)),
            None,
            counting,
        );
        assert_eq!(found.map(|(index, _)| index), Some(1));
        assert_eq!(asked.borrow().len(), 2);
    }

    #[test]
    fn a_missing_path_is_not_a_link() {
        assert_eq!(res("src/nope.rs"), None);
        assert_eq!(res(""), None);
        // `~user` is not expanded.
        assert_eq!(res("~root/notes.md"), None);
        assert_eq!(act("src/nope.rs", path_kind()), None);
        assert_eq!(act("file:///nope", LinkKind::Osc8), None);
    }

    #[test]
    fn without_a_directory_or_home_only_absolute_paths_resolve() {
        let bare = |candidate: &str| resolve(Path::new(candidate), None, None, fake);
        assert_eq!(bare("src/main.rs"), None);
        assert_eq!(bare("~/notes.md"), None);
        assert!(bare("/work/proj/run.sh").is_some());
        // A relative OSC 7 directory is no anchor.
        assert_eq!(
            resolve(
                Path::new("src/main.rs"),
                Some(Path::new("proj")),
                None,
                fake
            ),
            None
        );
    }

    #[test]
    fn the_suffix_is_bt_cores_and_is_not_stripped_again() {
        // `bt-core` hands `src/main.rs` with `line: 12, col: 3`: the path resolves without the
        // suffix, which is not stripped again here.
        assert_eq!(
            res("src/main.rs").map(|r| r.path),
            Some("/work/proj/src/main.rs".into())
        );
        // A name that really ends in `:12` still resolves as written.
        assert_eq!(res("a:12").map(|r| r.path), Some("/work/proj/a:12".into()));
    }

    #[test]
    fn file_urls_resolve_as_paths() {
        assert_eq!(
            local_path("file:///tmp/a%20b", &LinkKind::Url),
            Some("/tmp/a b".into())
        );
        assert_eq!(
            local_path("FILE://myhost/tmp/x?q#frag", &LinkKind::Osc8),
            Some("/tmp/x".into())
        );
        assert_eq!(local_path("file://host", &LinkKind::Osc8), None);
        assert_eq!(local_path("file:///bad%zz", &LinkKind::Osc8), None);
        assert_eq!(local_path("file:///trunc%2", &LinkKind::Osc8), None);
        assert_eq!(local_path("https://x.dev", &LinkKind::Url), None);
        // Non-UTF-8 bytes stay bytes.
        assert_eq!(
            local_path("file:///%ff", &LinkKind::Osc8),
            Some(PathBuf::from(OsString::from_vec(vec![b'/', 0xff])))
        );
        // A `file://` URL that names a path is a path: it has no action here.
        assert_eq!(act("file:///tmp/a%20b", LinkKind::Osc8), None);
        assert_eq!(act("file://localhost/work/proj/src", LinkKind::Url), None);
    }

    #[test]
    fn the_policy_table() {
        let open_url = |url: &str| Some(LinkAction::OpenUrl(url.to_owned()));
        // URLs on the common list open without asking, plain text or OSC 8.
        for url in [
            "https://example.com",
            "HTTP://x.dev",
            "ftp://ftp.x",
            "mailto:a@b.c",
        ] {
            assert_eq!(act(url, LinkKind::Url), open_url(url), "{url}");
            assert_eq!(act(url, LinkKind::Osc8), open_url(url), "{url}");
        }
        // A path has no action: it opens in the program the person chose, or it is no link.
        for path in [
            "src/main.rs",
            "run.sh",
            "/tmp/a b",
            "src",
            "/Applications/Foo.app",
        ] {
            assert_eq!(act(path, path_kind()), None, "{path}");
        }
        // An uncommon OSC 8 scheme asks with the whole target.
        for uri in [
            "vscode://file/x",
            "x-man-page://ls",
            "ssh://host",
            "no-scheme",
        ] {
            assert_eq!(
                act(uri, LinkKind::Osc8),
                Some(LinkAction::Confirm(uri.to_owned())),
                "{uri}"
            );
        }
        // Our own scheme is swallowed on every path, before anything else.
        for kind in [LinkKind::Osc8, LinkKind::Url, path_kind()] {
            assert_eq!(
                act("bateri://tab/x", kind.clone()),
                Some(LinkAction::Swallow)
            );
            assert_eq!(act("BATERI://block/3", kind), Some(LinkAction::Swallow));
        }
        // A `file:` URL without an authority names no path we read: never a URL to open.
        for kind in [LinkKind::Osc8, LinkKind::Url] {
            for uri in [
                "file:/tmp/x.command",
                "FILE:/Applications/Foo.app",
                "file:x",
            ] {
                assert_eq!(act(uri, kind.clone()), Some(LinkAction::Swallow), "{uri}");
            }
        }
    }

    /// `ls` in a cloned repo lists `docs -> .x/Evil.app` and `README.md -> payload.terminal`:
    /// a link resolves to the object it points to, and stays itself as the path.
    #[test]
    fn a_symlink_resolves_to_its_target_and_stays_itself_as_the_path() {
        let link = |name: &str| res(name).map(|r| (r.path, r.entry, r.target));
        assert_eq!(
            link("docs"),
            Some((
                "/work/proj/docs".into(),
                Entry::Dir,
                "/work/proj/.x/Evil.app".into()
            ))
        );
        assert_eq!(
            link("README.md"),
            Some((
                "/work/proj/README.md".into(),
                Entry::File { executable: false },
                "/work/proj/payload.terminal".into()
            ))
        );
        assert_eq!(
            link("latest.md"),
            Some((
                "/work/proj/latest.md".into(),
                Entry::File { executable: false },
                "/work/proj/notes/2024.md".into()
            ))
        );
        assert_eq!(
            link("current"),
            Some((
                "/work/proj/current".into(),
                Entry::Dir,
                "/work/proj/src".into()
            ))
        );
    }

    #[test]
    fn only_a_url_has_an_action() {
        assert_eq!(
            action("https://x.dev", &LinkKind::Url),
            Some(LinkAction::OpenUrl("https://x.dev".into()))
        );
        assert_eq!(action("missing", &path_kind()), None);
    }

    #[test]
    fn the_production_stat_sees_directories_and_the_x_bit() {
        let dir = std::env::temp_dir().join(format!("bt-links-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let script = dir.join("run.sh");
        let doc = dir.join("doc.txt");
        std::fs::write(&script, "#!/bin/sh\n").expect("write script");
        std::fs::write(&doc, "x").expect("write doc");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        std::fs::set_permissions(&doc, std::fs::Permissions::from_mode(0o644)).expect("chmod");
        let entry = |path: &Path| stat(path).map(|(entry, _)| entry);
        assert_eq!(entry(&dir), Some(Entry::Dir));
        assert_eq!(entry(&script), Some(Entry::File { executable: true }));
        assert_eq!(entry(&doc), Some(Entry::File { executable: false }));
        assert_eq!(stat(&dir.join("nope")), None);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn the_production_stat_follows_a_symlink_to_its_real_path() {
        let dir = std::env::temp_dir().join(format!("bt-links-follow-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).expect("temp dir");
        let script = dir.join("run.sh");
        std::fs::write(&script, "#!/bin/sh\n").expect("write script");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        std::os::unix::fs::symlink("run.sh", dir.join("notes.txt")).expect("link");
        std::os::unix::fs::symlink("../sub", dir.join("sub/up")).expect("link");
        std::os::unix::fs::symlink("gone", dir.join("dangling")).expect("link");
        // `temp_dir` is itself under a link on macOS (`/var` → `/private/var`).
        let real = std::fs::canonicalize(&dir).expect("canonical");
        assert_eq!(
            stat(&dir.join("notes.txt")),
            Some((Entry::File { executable: true }, real.join("run.sh")))
        );
        assert_eq!(
            stat(&dir.join("sub/up/..")),
            Some((Entry::Dir, real.clone()))
        );
        assert_eq!(stat(&dir), Some((Entry::Dir, real)));
        assert_eq!(stat(&dir.join("dangling")), None);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn this_machine_has_a_name() {
        let name = hostname().expect("gethostname");
        assert!(!name.contains('\0'));
    }
}
